//! System tray integration
//!
//! This module implements system tray icon and menu functionality.
//!
//! # Requirements
//!
//! - Requirement 5.10: Display an icon showing HDR state in the system tray
//! - Requirement 5.11: Show context menu with "Open", "Current HDR State: ON/OFF", and "Exit"
//! - Requirement 5.12: Left-click to restore main window
//!
//! # Implementation Notes
//!
//! This module uses the `tray-icon` crate to create a system tray icon with a context menu.
//! The tray icon displays the current HDR state and provides quick access to the main window.
//!
//! ## Task 11.1: Create TrayIcon struct
//!
//! The TrayIcon struct contains:
//! - `tray`: The actual tray icon from the tray-icon crate
//! - `menu`: The context menu for the tray icon
//! - `window_handle`: A weak reference to the MainWindow for showing/hiding

#[cfg(windows)]
use easyhdr::error::{EasyHdrError, Result};
#[cfg(windows)]
use slint::{ComponentHandle, Weak};

#[cfg(windows)]
use tray_icon::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};

/// System tray icon with context menu
///
/// This struct manages the system tray icon and its associated context menu.
/// It provides methods to create the tray icon, update its state, and handle
/// menu events.
///
/// # Fields
///
/// - `tray`: The tray icon instance from the tray-icon crate
/// - `menu`: The context menu attached to the tray icon
/// - `window_handle`: Weak reference to the MainWindow for restoration
///
/// # Requirements
///
/// - Requirement 5.10: Display tray icon showing HDR state
/// - Requirement 5.11: Context menu with Open, Status, and Exit items
#[cfg(windows)]
pub struct TrayIcon {
    /// The actual tray icon
    tray: tray_icon::TrayIcon,
    /// The context menu for the tray icon
    menu: Menu,
    /// Weak reference to the main window
    window_handle: Weak<crate::MainWindow>,
}

/// Placeholder for non-Windows platforms
#[cfg(not(windows))]
pub struct TrayIcon;

#[cfg(windows)]
impl TrayIcon {
    /// Create a new tray icon with context menu
    ///
    /// This constructor creates a system tray icon with a context menu containing:
    /// - "Open" - Restores the main window
    /// - "Current HDR State: OFF" - Displays current HDR status (disabled/info only)
    /// - Separator
    /// - "Exit" - Exits the application
    ///
    /// # Arguments
    ///
    /// * `window` - Reference to the MainWindow for restoration
    ///
    /// # Returns
    ///
    /// Returns a Result containing the TrayIcon or an error if creation fails.
    ///
    /// # Requirements
    ///
    /// - Requirement 5.10: Create tray icon with HDR state indicator
    /// - Requirement 5.11: Create context menu with required items
    ///
    /// # Example
    ///
    /// ```no_run
    /// use easyhdr::gui::TrayIcon;
    /// # use slint::include_modules;
    /// # slint::include_modules!();
    ///
    /// let main_window = MainWindow::new().unwrap();
    /// let tray_icon = TrayIcon::new(&main_window)?;
    /// # Ok::<(), easyhdr::error::EasyHdrError>(())
    /// ```
    pub fn new(window: &crate::MainWindow) -> Result<Self> {
        use tracing::{debug, info};

        info!("Creating system tray icon");

        // Create the context menu
        let tray_menu = Menu::new();

        // Create menu items
        // "Open" - Restores the main window
        let open_item = MenuItem::new("Open", true, None);

        // "Current HDR State: OFF" - Info item showing HDR status (disabled)
        let status_item = MenuItem::new("Current HDR State: OFF", false, None);

        // Separator
        let separator = PredefinedMenuItem::separator();

        // "Exit" - Exits the application
        let exit_item = MenuItem::new("Exit", true, None);

        // Append items to menu
        tray_menu
            .append(&open_item)
            .map_err(|e| EasyHdrError::ConfigError(format!("Failed to add Open menu item: {}", e)))?;

        tray_menu
            .append(&status_item)
            .map_err(|e| EasyHdrError::ConfigError(format!("Failed to add Status menu item: {}", e)))?;

        tray_menu
            .append(&separator)
            .map_err(|e| EasyHdrError::ConfigError(format!("Failed to add separator: {}", e)))?;

        tray_menu
            .append(&exit_item)
            .map_err(|e| EasyHdrError::ConfigError(format!("Failed to add Exit menu item: {}", e)))?;

        debug!("Tray menu created with 4 items");

        // Create a default icon (simple colored square)
        // TODO: Replace with actual icon assets when available (Task 15.1)
        let icon = Self::create_default_icon(false)?;

        // Build the tray icon
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu.clone()))
            .with_icon(icon)
            .with_tooltip("EasyHDR")
            .build()
            .map_err(|e| EasyHdrError::ConfigError(format!("Failed to build tray icon: {}", e)))?;

        info!("System tray icon created successfully");

        Ok(Self {
            tray,
            menu: tray_menu,
            window_handle: window.as_weak(),
        })
    }

    /// Create a default tray icon
    ///
    /// This creates a simple 32x32 RGBA icon as a placeholder until actual icon assets
    /// are available. The icon is a colored square:
    /// - Red when HDR is disabled
    /// - Green when HDR is enabled
    ///
    /// # Arguments
    ///
    /// * `hdr_enabled` - Whether HDR is currently enabled
    ///
    /// # Returns
    ///
    /// Returns a Result containing the Icon or an error if creation fails.
    ///
    /// # Implementation Notes
    ///
    /// This is a temporary implementation. Task 15.1 will replace this with
    /// actual icon assets (icon_hdr_on.ico and icon_hdr_off.ico).
    fn create_default_icon(hdr_enabled: bool) -> Result<Icon> {
        use tracing::debug;

        const ICON_SIZE: usize = 32;
        let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];

        // Choose color based on HDR state
        let (r, g, b) = if hdr_enabled {
            (0, 204, 0) // Green for HDR ON
        } else {
            (204, 0, 0) // Red for HDR OFF
        };

        // Fill the icon with the chosen color
        for y in 0..ICON_SIZE {
            for x in 0..ICON_SIZE {
                let idx = (y * ICON_SIZE + x) * 4;

                // Create a border for better visibility
                if x == 0 || x == ICON_SIZE - 1 || y == 0 || y == ICON_SIZE - 1 {
                    // Border: darker version of the color
                    rgba[idx] = (r / 2) as u8;
                    rgba[idx + 1] = (g / 2) as u8;
                    rgba[idx + 2] = (b / 2) as u8;
                    rgba[idx + 3] = 255;
                } else {
                    // Interior: full color
                    rgba[idx] = r as u8;
                    rgba[idx + 1] = g as u8;
                    rgba[idx + 2] = b as u8;
                    rgba[idx + 3] = 255;
                }
            }
        }

        debug!("Created default tray icon (HDR: {})", if hdr_enabled { "ON" } else { "OFF" });

        Icon::from_rgba(rgba, ICON_SIZE as u32, ICON_SIZE as u32)
            .map_err(|e| EasyHdrError::ConfigError(format!("Failed to create icon from RGBA: {}", e)))
    }
}

/// Stub implementation for non-Windows platforms
#[cfg(not(windows))]
impl TrayIcon {
    /// Create a new tray icon (stub for non-Windows)
    ///
    /// This is a stub implementation for non-Windows platforms.
    /// The actual tray icon functionality is only available on Windows.
    #[allow(dead_code)]
    pub fn new(_window: &crate::MainWindow) -> easyhdr::error::Result<Self> {
        Ok(Self)
    }
}

