//! EasyHDR - Automatic HDR management for Windows
//!
//! This application automatically enables and disables HDR on Windows displays
//! based on configured applications.
//!
//! # Requirements
//!
//! - Requirement 10.1: Compile as Windows GUI subsystem application (no console window)
//! - Requirement 10.6: Function correctly on Windows 10 21H2+ (build 19044+)

// Set Windows subsystem to hide console window
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// GUI module is only in the binary, not the library
mod gui;

use easyhdr::{
    config::ConfigManager,
    controller::{AppController, AppState},
    error::{EasyHdrError, Result},
    hdr::HdrController,
    monitor::{ProcessEvent, ProcessMonitor},
    utils,
};
use gui::GuiController;
use parking_lot::Mutex;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tracing::{error, info, warn};

// Include Slint-generated code
slint::include_modules!();

/// Minimum supported Windows build number (Windows 10 21H2)
const MIN_WINDOWS_BUILD: u32 = 19044;

fn main() -> Result<()> {
    // Initialize logging first so we can log errors
    utils::init_logging()?;

    info!("EasyHDR v{} starting...", env!("CARGO_PKG_VERSION"));

    // Detect Windows version and verify compatibility
    // Requirement 10.6: Function correctly on Windows 10 21H2+
    if let Err(e) = verify_windows_version() {
        error!("Windows version check failed: {}", e);
        show_error_and_exit(&format!(
            "EasyHDR requires Windows 10 21H2 (build {}) or later.\n\n\
             Your Windows version is not supported.\n\n\
             Please update Windows to continue.",
            MIN_WINDOWS_BUILD
        ));
        return Err(e);
    }

    info!("Windows version check passed");

    // Load configuration
    let config = ConfigManager::load()?;
    info!("Configuration loaded with {} monitored apps", config.monitored_apps.len());

    // Initialize core components
    // This may fail if run on macOS (development environment)
    let (process_monitor, gui_controller) =
        match initialize_components(config) {
            Ok(components) => components,
            Err(e) => {
                error!("Failed to initialize components: {}", e);

                // On macOS, show a friendly message
                #[cfg(not(windows))]
                {
                    eprintln!("EasyHDR is a Windows-only application.");
                    eprintln!("This application cannot run on macOS or other non-Windows platforms.");
                    return Err(e);
                }

                // On Windows, show error dialog
                #[cfg(windows)]
                {
                    show_error_and_exit(&format!(
                        "Failed to initialize EasyHDR:\n\n{}\n\n\
                         Please ensure your display drivers are up to date.",
                        get_user_friendly_error(&e)
                    ));
                    return Err(e);
                }
            }
        };

    info!("Core components initialized successfully");

    // Task 16.1: Log initial memory usage
    #[cfg(windows)]
    {
        use easyhdr::utils::memory_profiler;
        info!("Logging initial memory usage");
        memory_profiler::get_profiler().log_stats();
    }

    // Start background threads
    info!("Starting process monitor thread");
    let _monitor_handle = process_monitor.start();

    // Note: AppController thread is started inside initialize_components
    // and is managed by GuiController

    // Run GUI event loop (blocks until application exits)
    info!("Starting GUI event loop");
    gui_controller.run()?;

    // Task 16.1: Log final memory usage before shutdown
    #[cfg(windows)]
    {
        use easyhdr::utils::memory_profiler;
        info!("Logging final memory usage before shutdown");
        memory_profiler::get_profiler().log_stats();
    }

    info!("EasyHDR shutting down");

    Ok(())
}

/// Verify that the Windows version is compatible
///
/// # Requirements
///
/// - Requirement 10.6: Function correctly on Windows 10 21H2+ (build 19044+)
fn verify_windows_version() -> Result<()> {
    #[cfg(windows)]
    {
        use easyhdr::hdr::WindowsVersion;

        let version = WindowsVersion::detect()?;

        // Get the actual build number for detailed checking
        // We need to check if it's at least Windows 10 21H2 (build 19044)
        let build_number = get_windows_build_number()?;

        info!("Detected Windows version: {:?}, build: {}", version, build_number);

        if build_number < MIN_WINDOWS_BUILD {
            return Err(EasyHdrError::ConfigError(format!(
                "Windows build {} is too old. Minimum required: {}",
                build_number, MIN_WINDOWS_BUILD
            )));
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        // On non-Windows platforms, return an error
        Err(EasyHdrError::ConfigError(
            "EasyHDR is a Windows-only application".to_string()
        ))
    }
}

/// Get the Windows build number
///
/// Uses the same method as WindowsVersion::detect() but returns the raw build number
#[cfg(windows)]
fn get_windows_build_number() -> Result<u32> {
    use windows::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::HSTRING;
    use std::mem::{size_of, transmute};

    unsafe {
        // Load ntdll.dll
        let ntdll_name = HSTRING::from("ntdll.dll");
        let ntdll = LoadLibraryW(&ntdll_name)?;

        // Get RtlGetVersion function pointer
        let proc_name = windows::core::s!("RtlGetVersion");
        let rtl_get_version_ptr = GetProcAddress(ntdll, proc_name);

        if rtl_get_version_ptr.is_none() {
            return Err(EasyHdrError::HdrControlFailed(
                "RtlGetVersion not found in ntdll.dll".to_string()
            ));
        }

        // Define the function signature for RtlGetVersion
        type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32;
        let rtl_get_version: RtlGetVersionFn = transmute(rtl_get_version_ptr);

        // Prepare version info structure
        let mut version_info = OSVERSIONINFOEXW::default();
        version_info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOEXW>() as u32;

        // Call RtlGetVersion
        let status = rtl_get_version(&mut version_info);

        if status != 0 {
            return Err(EasyHdrError::HdrControlFailed(
                format!("RtlGetVersion failed with status: {}", status)
            ));
        }

        Ok(version_info.dwBuildNumber)
    }
}

/// Initialize all core components
///
/// # Requirements
///
/// - Requirement 3.1: Detect Windows version
/// - Requirement 3.2: Enumerate displays
/// - Requirement 2.1: Create process monitor with configured interval
/// - Requirement 4.7: Apply startup delay if configured
fn initialize_components(
    config: easyhdr::config::AppConfig,
) -> Result<(ProcessMonitor, GuiController)> {
    // First, create a temporary HdrController to check for HDR-capable displays
    // This is just for the warning message - AppController will create its own
    info!("Checking for HDR-capable displays");
    let temp_hdr_controller = HdrController::new()?;

    // Check if any displays support HDR and warn if none found
    // Requirement 10.9: Show clear messaging about hardware compatibility
    let hdr_capable_count = temp_hdr_controller.get_display_cache()
        .iter()
        .filter(|d| d.supports_hdr)
        .count();

    if hdr_capable_count == 0 {
        warn!("No HDR-capable displays detected");
        warn!("The application will run but HDR toggling will not work");

        #[cfg(windows)]
        {
            // Show a warning dialog but don't exit
            show_warning_dialog(
                "No HDR-capable displays were detected.\n\n\
                 The application will run, but HDR toggling will not work until \
                 an HDR-capable display is connected.\n\n\
                 Please ensure:\n\
                 - Your display supports HDR\n\
                 - Display drivers are up to date\n\
                 - HDR is enabled in Windows display settings"
            );
        }
    } else {
        info!("Found {} HDR-capable display(s)", hdr_capable_count);
    }

    // Create mpsc channels for communication
    let (process_event_tx, process_event_rx) = mpsc::channel::<ProcessEvent>();
    let (app_state_tx, app_state_rx) = mpsc::channel::<AppState>();

    // Create ProcessMonitor with configured interval
    let monitoring_interval = Duration::from_millis(config.preferences.monitoring_interval_ms);
    info!("Creating process monitor with interval: {:?}", monitoring_interval);
    let process_monitor = ProcessMonitor::new(monitoring_interval, process_event_tx);
    let watch_list_ref = process_monitor.get_watch_list_ref();

    // Apply startup delay if configured
    // Requirement 4.7: Implement optional startup delay
    let startup_delay = config.preferences.startup_delay_ms;
    if startup_delay > 0 {
        info!("Applying startup delay: {}ms", startup_delay);
        std::thread::sleep(Duration::from_millis(startup_delay));
    }

    // Create AppController (it will create its own HdrController)
    info!("Creating application controller");
    let app_controller = AppController::new(
        config.clone(),
        process_event_rx,
        app_state_tx,
        watch_list_ref,
    )?;

    // Wrap AppController in Arc<Mutex<>> for sharing between GUI and background thread
    let app_controller_handle = Arc::new(Mutex::new(app_controller));

    // Start AppController event loop in background thread
    info!("Starting application controller thread");
    let controller_for_thread = Arc::clone(&app_controller_handle);
    let _controller_handle = std::thread::spawn(move || {
        let mut controller = controller_for_thread.lock();
        controller.run();
    });

    // Create GuiController
    info!("Creating GUI controller");
    let gui_controller = GuiController::new(app_controller_handle, app_state_rx)?;

    Ok((process_monitor, gui_controller))
}

/// Show an error dialog and exit the application
///
/// # Requirements
///
/// - Requirement 7.1: Show modal dialog with user-friendly error message
/// - Requirement 7.6: Include OK button to dismiss
#[cfg(windows)]
fn show_error_and_exit(message: &str) {
    use rfd::MessageDialog;

    MessageDialog::new()
        .set_title("EasyHDR - Error")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .set_level(rfd::MessageLevel::Error)
        .show();

    std::process::exit(1);
}

/// Show an error dialog and exit the application (non-Windows fallback)
#[cfg(not(windows))]
fn show_error_and_exit(message: &str) {
    eprintln!("ERROR: {}", message);
    std::process::exit(1);
}

/// Show a warning dialog (non-blocking)
///
/// # Requirements
///
/// - Requirement 10.9: Show clear messaging about hardware compatibility
#[cfg(windows)]
fn show_warning_dialog(message: &str) {
    use rfd::MessageDialog;

    MessageDialog::new()
        .set_title("EasyHDR - Warning")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .set_level(rfd::MessageLevel::Warning)
        .show();
}

/// Get a user-friendly error message from an EasyHdrError
///
/// # Requirements
///
/// - Requirement 7.2: Show "Your display doesn't support HDR" for HdrNotSupported
/// - Requirement 7.3: Show "Unable to control HDR - check display drivers" for driver issues
/// - Requirement 7.5: Provide troubleshooting hints
#[cfg(windows)]
fn get_user_friendly_error(error: &EasyHdrError) -> String {
    match error {
        EasyHdrError::HdrNotSupported => {
            "Your display doesn't support HDR.\n\n\
             Please check your hardware specifications and ensure:\n\
             - Your display supports HDR10 or higher\n\
             - Your GPU supports HDR output\n\
             - You're using a compatible connection (HDMI 2.0+ or DisplayPort 1.4+)"
                .to_string()
        }
        EasyHdrError::HdrControlFailed(_) | EasyHdrError::DriverError(_) => {
            "Unable to control HDR.\n\n\
             Please ensure:\n\
             - Your display drivers are up to date\n\
             - HDR is enabled in Windows display settings\n\
             - Your display is properly connected"
                .to_string()
        }
        EasyHdrError::ProcessMonitorError(_) => {
            "Failed to monitor processes.\n\n\
             The application may not function correctly.\n\
             Try restarting the application."
                .to_string()
        }
        EasyHdrError::ConfigError(_) => {
            "Failed to load or save configuration.\n\n\
             Your settings may not persist.\n\
             Check that you have write permissions to:\n\
             %APPDATA%\\EasyHDR"
                .to_string()
        }
        #[cfg(windows)]
        EasyHdrError::WindowsApiError(e) => {
            format!(
                "A Windows API error occurred:\n\n{}\n\n\
                 Please ensure your Windows installation is up to date.",
                e
            )
        }
        EasyHdrError::IoError(e) => {
            format!(
                "A file system error occurred:\n\n{}\n\n\
                 Please check file permissions and disk space.",
                e
            )
        }
        EasyHdrError::JsonError(e) => {
            format!(
                "Configuration file is corrupted:\n\n{}\n\n\
                 The application will use default settings.",
                e
            )
        }
    }
}
