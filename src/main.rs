//! `EasyHDR` - Automatic HDR management for Windows
//!
//! This application automatically enables and disables HDR on Windows displays
//! based on configured applications. Requires Windows 10 21H2+ (build 19044+).

// Set Windows subsystem to hide console window
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![expect(
    missing_docs,
    reason = "Slint-generated code from include_modules! lacks doc comments"
)]
#![allow(clippy::unwrap_used)] // Slint-generated code from include_modules! uses .unwrap() extensively

// GUI module is only in the binary, not the library
mod gui;

use anyhow::{Context, Result};
use easyhdr::{
    config::ConfigManager,
    controller::{AppController, AppState},
    error::EasyHdrError,
    hdr::HdrController,
    monitor::{HdrStateEvent, HdrStateMonitor, ProcessEvent, ProcessMonitor},
    utils,
};
use gui::GuiController;
use parking_lot::Mutex;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tracing::{error, info, warn};

// Include Slint-generated code
slint::include_modules!();

/// Minimum supported Windows build number (Windows 10 21H2)
const MIN_WINDOWS_BUILD: u32 = 19044;

/// Main entry point for the application
///
/// Performs initialization including logging, version detection, single-instance
/// enforcement, HDR capability detection, and multi-threaded component startup.
#[expect(
    clippy::too_many_lines,
    reason = "Main function coordinates multi-phase application startup"
)]
fn main() -> Result<()> {
    use easyhdr::utils::startup_profiler::{self, StartupPhase};
    let profiler = startup_profiler::get_profiler();
    profiler.record_phase(StartupPhase::AppStart);

    utils::init_logging().context("Failed to initialize logging system")?;
    profiler.record_phase(StartupPhase::LoggingInit);

    info!("EasyHDR v{} starting...", env!("CARGO_PKG_VERSION"));

    // Enforce single instance - only one instance of EasyHDR should run at a time
    // This must be done early, before any other initialization
    let _single_instance_guard = match utils::SingleInstanceGuard::new() {
        Ok(guard) => guard,
        Err(e) => {
            error!("Single instance check failed: {}", e);

            #[cfg(windows)]
            {
                show_error_and_exit(
                    "Another instance of EasyHDR is already running.\n\n\
                     Please close the existing instance before starting a new one.\n\n\
                     Check the system tray for the EasyHDR icon.",
                );
            }

            return Err(e.into());
        }
    };

    info!("Single instance check passed");

    if let Err(e) =
        verify_windows_version().context("Failed to verify Windows version compatibility")
    {
        error!("Windows version check failed: {}", e);
        show_error_and_exit(&format!(
            "EasyHDR requires Windows 10 21H2 (build {MIN_WINDOWS_BUILD}) or later.\n\n\
             Your Windows version is not supported.\n\n\
             Please update Windows to continue."
        ));
        return Err(e);
    }
    profiler.record_phase(StartupPhase::VersionDetection);

    info!("Windows version check passed");

    let config = ConfigManager::load().context("Failed to load application configuration")?;
    profiler.record_phase(StartupPhase::ConfigLoad);
    info!(
        "Configuration loaded with {} monitored apps",
        config.monitored_apps.len()
    );

    #[cfg_attr(
        not(windows),
        expect(
            unused_variables,
            reason = "Components only used on Windows for HDR control and GUI"
        )
    )]
    let (process_monitor, gui_controller, should_show_hdr_warning) =
        match initialize_components(&config).context("Failed to initialize core components") {
            Ok(components) => components,
            Err(e) => {
                error!("Failed to initialize components: {:#}", e);

                // On macOS, show a friendly message
                #[cfg(not(windows))]
                {
                    eprintln!("EasyHDR is a Windows-only application.");
                    eprintln!(
                        "This application cannot run on macOS or other non-Windows platforms."
                    );
                    return Err(e);
                }

                #[cfg(windows)]
                {
                    use easyhdr::error::get_user_friendly_error;

                    let error_message =
                        if let Some(easy_hdr_error) = e.downcast_ref::<EasyHdrError>() {
                            get_user_friendly_error(easy_hdr_error)
                        } else {
                            format!("{e:#}")
                        };

                    show_error_and_exit(&format!(
                        "Failed to initialize EasyHDR:\n\n{error_message}\n\n\
                         Please ensure your display drivers are up to date."
                    ));
                    return Err(e);
                }
            }
        };

    info!("Core components initialized successfully");

    profiler.record_phase(StartupPhase::GuiDisplay);

    #[cfg(windows)]
    {
        use easyhdr::utils::memory_profiler;
        info!("Logging initial memory usage");
        memory_profiler::get_profiler().log_stats();
    }

    info!("Starting process monitor thread");
    let _monitor_handle = process_monitor.start();

    profiler.record_phase(StartupPhase::AppReady);
    profiler.log_summary();

    // Note: HDR warning notification is now handled via AppState.show_no_hdr_warning
    // and shown as a non-blocking tray notification by the GUI controller
    #[cfg(windows)]
    if should_show_hdr_warning {
        info!("No HDR-capable displays detected at startup - notification will be shown via tray");
    }

    info!("Starting GUI event loop");
    gui_controller
        .run()
        .context("GUI event loop terminated with error")?;

    #[cfg(windows)]
    {
        use easyhdr::utils::memory_profiler;
        info!("Logging final memory usage before shutdown");
        memory_profiler::get_profiler().log_stats();
    }

    info!("EasyHDR shutting down");

    Ok(())
}

/// Verifies that the Windows version is compatible (Windows 10 21H2+ / build 19044+).
fn verify_windows_version() -> Result<()> {
    #[cfg(windows)]
    {
        use easyhdr::hdr::WindowsVersion;

        let version = WindowsVersion::detect().context("Failed to detect Windows version")?;

        let build_number =
            get_windows_build_number().context("Failed to retrieve Windows build number")?;

        info!(
            "Detected Windows version: {:?}, build: {}",
            version, build_number
        );

        if build_number < MIN_WINDOWS_BUILD {
            return Err(
                EasyHdrError::ConfigError(easyhdr::error::StringError::new(format!(
                    "Windows build {build_number} is too old. Minimum required: {MIN_WINDOWS_BUILD}"
                )))
                .into(),
            );
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        Err(EasyHdrError::ConfigError(easyhdr::error::StringError::new(
            "EasyHDR is a Windows-only application",
        ))
        .into())
    }
}

/// Gets the Windows build number using `RtlGetVersion`.
///
/// # Safety
///
/// Sound FFI call: ntdll.dll and `RtlGetVersion` guaranteed present on Windows;
/// function pointer validated before transmute; structure correctly sized with
/// `dwOSVersionInfoSize`; return status checked before data access.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Required for Windows FFI to call RtlGetVersion from ntdll.dll"
)]
fn get_windows_build_number() -> Result<u32> {
    use std::mem::{size_of, transmute};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    use windows::core::HSTRING;

    // Define the function signature for RtlGetVersion
    type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32;

    unsafe {
        let ntdll_name = HSTRING::from("ntdll.dll");
        let ntdll =
            LoadLibraryW(&ntdll_name).map_err(|e| EasyHdrError::HdrControlFailed(Box::new(e)))?;

        let proc_name = windows::core::s!("RtlGetVersion");
        let rtl_get_version_ptr = GetProcAddress(ntdll, proc_name);

        if rtl_get_version_ptr.is_none() {
            return Err(
                EasyHdrError::HdrControlFailed(easyhdr::error::StringError::new(
                    "RtlGetVersion not found in ntdll.dll",
                ))
                .into(),
            );
        }

        let rtl_get_version: RtlGetVersionFn = transmute(rtl_get_version_ptr);

        #[expect(
            clippy::cast_possible_truncation,
            reason = "size_of::<OSVERSIONINFOEXW>() is a compile-time constant that fits in u32"
        )]
        let mut version_info = OSVERSIONINFOEXW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
            ..Default::default()
        };

        let status = rtl_get_version(&raw mut version_info);

        if status != 0 {
            return Err(
                EasyHdrError::HdrControlFailed(easyhdr::error::StringError::new(format!(
                    "RtlGetVersion failed with status: {status}"
                )))
                .into(),
            );
        }

        Ok(version_info.dwBuildNumber)
    }
}

/// Logs comprehensive HDR startup summary for diagnostics, including Windows version,
/// detected displays, HDR capabilities, and current HDR state.
#[cfg(windows)]
fn log_hdr_startup_summary(hdr_controller: &HdrController) {
    use easyhdr::hdr::version::WindowsVersion;

    info!("=== HDR Startup Summary ===");

    let windows_version = hdr_controller.get_windows_version();
    let build_number = WindowsVersion::get_build_number().unwrap_or(0);

    info!("Windows Version: {:?}", windows_version);
    info!("Windows Build Number: {}", build_number);

    let displays = hdr_controller.get_display_cache();
    info!("Total Displays Detected: {}", displays.len());

    if displays.is_empty() {
        warn!("No displays were detected by the system!");
        info!("=== End HDR Startup Summary ===");
        return;
    }

    for (index, disp) in displays.iter().enumerate() {
        info!("--- Display {} ---", index);
        info!(
            "  Adapter ID: LowPart={:#010x}, HighPart={:#010x}",
            disp.adapter_id.LowPart, disp.adapter_id.HighPart
        );
        info!("  Target ID: {}", disp.target_id);
        info!("  HDR Supported: {}", disp.supports_hdr);

        if disp.supports_hdr {
            match hdr_controller.is_hdr_enabled(disp) {
                Ok(enabled) => {
                    info!("  HDR Currently Enabled: {}", enabled);
                }
                Err(e) => {
                    warn!("  Failed to check if HDR is enabled: {}", e);
                }
            }
        } else {
            info!("  HDR Currently Enabled: N/A (not supported)");
        }
    }

    let hdr_capable_count = displays.iter().filter(|d| d.supports_hdr).count();
    info!(
        "HDR-Capable Displays: {} of {}",
        hdr_capable_count,
        displays.len()
    );

    info!("=== End HDR Startup Summary ===");
}

#[cfg(not(windows))]
fn log_hdr_startup_summary(_hdr_controller: &HdrController) {}

/// Initializes all core components including HDR controller, process monitor,
/// HDR state monitor, app controller, and GUI. Returns a tuple of
/// (`ProcessMonitor`, `GuiController`, `should_show_hdr_warning`).
fn initialize_components(
    config: &easyhdr::config::AppConfig,
) -> Result<(ProcessMonitor, GuiController, bool)> {
    use easyhdr::utils::startup_profiler::{self, StartupPhase};
    let profiler = startup_profiler::get_profiler();

    info!("Checking for HDR-capable displays");
    let temp_hdr_controller = HdrController::new().context("Failed to create HDR controller")?;
    profiler.record_phase(StartupPhase::HdrControllerInit);

    log_hdr_startup_summary(&temp_hdr_controller);

    let hdr_capable_count = temp_hdr_controller
        .get_display_cache()
        .iter()
        .filter(|d| d.supports_hdr)
        .count();

    let should_show_hdr_warning = if hdr_capable_count == 0 {
        warn!("No HDR-capable displays detected");
        warn!("The application will run but HDR toggling will not work");
        true
    } else {
        info!("Found {} HDR-capable display(s)", hdr_capable_count);
        false
    };

    let channel_capacity = 32;
    let (process_event_tx, process_event_rx) = mpsc::sync_channel::<ProcessEvent>(channel_capacity);
    let (hdr_state_tx, hdr_state_rx) = mpsc::sync_channel::<HdrStateEvent>(channel_capacity);
    let (app_state_tx, app_state_rx) = mpsc::sync_channel::<AppState>(channel_capacity);

    let monitoring_interval = Duration::from_millis(config.preferences.monitoring_interval_ms);
    info!(
        "Creating process monitor with interval: {:?}",
        monitoring_interval
    );
    let process_monitor = ProcessMonitor::new(monitoring_interval, process_event_tx);
    let watch_state_ref = process_monitor.get_watch_state_ref();
    profiler.record_phase(StartupPhase::ProcessMonitorInit);

    info!("Creating HDR state monitor");
    let hdr_state_monitor = HdrStateMonitor::new(
        HdrController::new().context("Failed to create HDR controller for state monitoring")?,
        hdr_state_tx,
    )
    .context("Failed to create HDR state monitor")?;
    profiler.record_phase(StartupPhase::HdrMonitorInit);

    info!("Creating application controller");
    let app_controller = AppController::new(
        config.clone(),
        process_event_rx,
        hdr_state_rx,
        app_state_tx,
        watch_state_ref,
    )
    .context("Failed to create application controller")?;
    profiler.record_phase(StartupPhase::AppControllerInit);

    let app_controller_handle = Arc::new(Mutex::new(app_controller));

    info!("Creating GUI controller");
    let gui_controller = GuiController::new(Arc::clone(&app_controller_handle), app_state_rx)
        .context("Failed to create GUI controller")?;
    profiler.record_phase(StartupPhase::GuiControllerInit);

    info!("Starting application controller thread");
    let _controller_handle = AppController::spawn_event_loop(Arc::clone(&app_controller_handle));

    info!("Sending initial state to populate GUI");
    {
        let controller_guard = app_controller_handle.lock();
        controller_guard.send_initial_state();
    }

    info!("Starting HDR state monitor thread");
    let _hdr_monitor_handle = hdr_state_monitor.start();

    Ok((process_monitor, gui_controller, should_show_hdr_warning))
}

/// Shows an error dialog and exits the application.
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

/// Shows an error dialog and exits the application (non-Windows fallback).
#[cfg(not(windows))]
fn show_error_and_exit(message: &str) {
    eprintln!("ERROR: {message}");
    std::process::exit(1);
}
