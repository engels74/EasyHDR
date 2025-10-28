//! HDR state monitoring module
//!
//! Detects HDR state changes via hidden window receiving `WM_DISPLAYCHANGE`/`WM_SETTINGCHANGE`.
//! Uses periodic rechecks (500ms × 10 = 5s max) to handle race condition where Windows messages
//! arrive before `DisplayConfig` APIs reflect state change. Hidden window (not message-only) required
//! to receive broadcast messages. Performance: <0.1% CPU average.

use crate::error::Result;
use crate::hdr::HdrController;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::mpsc;
use tracing::{debug, info, warn};

#[cfg(windows)]
use crate::error::EasyHdrError;
#[cfg(windows)]
use tracing::error;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MSG,
    PostQuitMessage, RegisterClassW, SetTimer, UnregisterClassW, WINDOW_EX_STYLE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

// Timing constants for HDR state recheck strategy
// These handle the race condition where WM_DISPLAYCHANGE arrives before
// DisplayConfigGetDeviceInfo reflects the actual state change
//
// Based on HDRTray's proven approach: 10 rechecks at 500ms intervals (5 seconds total)
#[cfg(windows)]
const RECHECK_INTERVAL_MS: u32 = 500; // Interval between rechecks
#[cfg(windows)]
const MAX_RECHECK_COUNT: u32 = 10; // Maximum number of rechecks (5 seconds total)

// Timer ID for HDR state rechecks
#[cfg(windows)]
const TIMER_ID_HDR_RECHECK: usize = 1; // Periodic recheck timer

/// HDR state change events
///
/// These events are sent when the HDR state changes externally (e.g., via Windows settings)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdrStateEvent {
    /// HDR was enabled (detected via Windows display change notification)
    Enabled,
    /// HDR was disabled (detected via Windows display change notification)
    Disabled,
}

/// HDR state monitor
///
/// Monitors Windows display configuration changes and detects HDR state transitions.
/// Uses a message-only window to receive `WM_DISPLAYCHANGE` and `WM_SETTINGCHANGE` messages.
pub struct HdrStateMonitor {
    /// Event sender to notify the application controller
    event_sender: mpsc::SyncSender<HdrStateEvent>,
    /// HDR controller for querying HDR state
    hdr_controller: Arc<Mutex<HdrController>>,
    /// Cached HDR state for change detection
    cached_hdr_state: Arc<Mutex<bool>>,
}

impl HdrStateMonitor {
    /// Create a new HDR state monitor
    ///
    /// # Arguments
    ///
    /// * `hdr_controller` - HDR controller for querying HDR state
    /// * `event_sender` - Channel sender for HDR state events
    ///
    /// # Returns
    ///
    /// Returns a new `HdrStateMonitor` instance with initial state detection
    pub fn new(
        hdr_controller: HdrController,
        event_sender: mpsc::SyncSender<HdrStateEvent>,
    ) -> Result<Self> {
        // Detect initial HDR state
        let initial_state = Self::detect_current_hdr_state_internal(&hdr_controller);
        debug!(
            "HdrStateMonitor initialized with HDR state: {}",
            initial_state
        );

        Ok(Self {
            event_sender,
            hdr_controller: Arc::new(Mutex::new(hdr_controller)),
            cached_hdr_state: Arc::new(Mutex::new(initial_state)),
        })
    }

    /// Start monitoring in a background thread
    ///
    /// Creates a message-only window and enters the Windows message loop.
    /// This method spawns a background thread and returns immediately.
    ///
    /// # Returns
    ///
    /// Returns a `JoinHandle` for the background thread
    pub fn start(self) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            #[cfg(windows)]
            {
                info!("Starting HDR state monitor thread");
                if let Err(e) = self.run_message_loop() {
                    error!("HDR state monitor failed: {}", e);
                }
                info!("HDR state monitor thread exited");
            }

            #[cfg(not(windows))]
            {
                info!("HDR state monitor not supported on non-Windows platforms");
            }
        })
    }

    /// Detect current HDR state from the system
    ///
    /// Checks all HDR-capable displays and returns true if any of them have HDR enabled.
    ///
    /// # Arguments
    ///
    /// * `hdr_controller` - Reference to the HDR controller
    ///
    /// # Returns
    ///
    /// Returns true if HDR is enabled on any HDR-capable display, false otherwise.
    fn detect_current_hdr_state_internal(hdr_controller: &HdrController) -> bool {
        // Delegate to the shared implementation in HdrController
        hdr_controller.detect_current_hdr_state()
    }

    /// Check if HDR state has changed and send event if it has
    ///
    /// This method is called when a display configuration change is detected.
    /// It queries the actual HDR state and compares with the cached state.
    #[expect(dead_code)] // Used in Windows-specific window procedure
    fn check_and_notify_hdr_state_change(&self) {
        debug!("Checking for HDR state change");

        // Query current HDR state
        let controller = self.hdr_controller.lock();
        let current_state = Self::detect_current_hdr_state_internal(&controller);
        drop(controller);

        // Compare with cached state
        let mut cached_state = self.cached_hdr_state.lock();
        if current_state == *cached_state {
            debug!("HDR state unchanged: {}", current_state);
        } else {
            info!(
                "HDR state changed: {} -> {}",
                if *cached_state { "ON" } else { "OFF" },
                if current_state { "ON" } else { "OFF" }
            );

            // Update cached state
            *cached_state = current_state;

            // Send event
            let event = if current_state {
                HdrStateEvent::Enabled
            } else {
                HdrStateEvent::Disabled
            };

            if let Err(e) = self.event_sender.send(event) {
                warn!("Failed to send HDR state event: {}", e);
            } else {
                debug!("Sent HDR state event: {:?}", event);
            }
        }
    }

    /// Run the Windows message loop
    ///
    /// Creates a message-only window and processes Windows messages.
    /// This method blocks until the window is destroyed.
    ///
    /// # Safety
    ///
    /// Wide strings null-terminated via `.chain(once(0))`. `WNDCLASSW` initialized with
    /// valid window proc and class name; `RegisterClassW` error checked. `PCWSTR` pointers
    /// valid for unsafe block duration. `CreateWindowExW` null-checked, errors propagated.
    /// `GetMessageW` called with valid stack `MSG` pointer; return values validated
    /// (-1/0/>0). TLS state set before window creation for `window_proc` access.
    /// `DestroyWindow` called on valid handle.
    #[cfg(windows)]
    #[expect(
        unsafe_code,
        reason = "Windows FFI for message-only window creation and message loop via CreateWindowExW and GetMessageW"
    )]
    fn run_message_loop(&self) -> Result<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;

        // Convert strings to wide strings for Windows API
        let class_name_str = "EasyHDR_HdrStateMonitor";
        let window_name_str = "EasyHDR HDR State Monitor";

        let class_name_wide: Vec<u16> = OsStr::new(class_name_str)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let window_name_wide: Vec<u16> = OsStr::new(window_name_str)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Create shared state for window procedure
        let monitor_state = Arc::new(MonitorState {
            hdr_controller: self.hdr_controller.clone(),
            cached_hdr_state: self.cached_hdr_state.clone(),
            event_sender: self.event_sender.clone(),
            recheck_count: Arc::new(Mutex::new(0)),
        });

        unsafe {
            // Register window class
            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                lpszClassName: PCWSTR(class_name_wide.as_ptr()),
                ..Default::default()
            };

            let atom = RegisterClassW(&raw const wnd_class);
            if atom == 0 {
                return Err(crate::error::EasyHdrError::WindowsApiError(
                    windows::core::Error::from_thread(),
                ));
            }

            debug!("Registered window class: {}", class_name_str);

            // Store monitor state in TLS for window procedure access
            MONITOR_STATE_TLS.with(|cell| {
                *cell.borrow_mut() = Some(monitor_state.clone());
            });

            // Create hidden window (not message-only, so it can receive broadcast messages)
            // Position off-screen at (-32000, -32000) to ensure it's never visible
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name_wide.as_ptr()),
                PCWSTR(window_name_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW, // Regular window style (required for broadcast messages)
                -32000,              // Off-screen X position
                -32000,              // Off-screen Y position
                1,                   // Minimal width
                1,                   // Minimal height
                None, // No parent (NOT HWND_MESSAGE - that blocks broadcast messages)
                None,
                None,
                None,
            )
            .map_err(|e| {
                error!("Failed to create hidden window for HDR state monitoring: {e}");
                EasyHdrError::ProcessMonitorError(crate::error::StringError::new(format!(
                    "Failed to create hidden window for HDR state monitoring: {e}"
                )))
            })?;

            if hwnd.0.is_null() {
                let _ = UnregisterClassW(PCWSTR(class_name_wide.as_ptr()), None);
                return Err(crate::error::EasyHdrError::WindowsApiError(
                    windows::core::Error::from_thread(),
                ));
            }

            info!("Created hidden window for HDR state monitoring (positioned off-screen)");

            // Enter message loop
            let mut msg = MSG::default();
            while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&raw const msg);
            }

            // Cleanup
            let _ = UnregisterClassW(PCWSTR(class_name_wide.as_ptr()), None);
            debug!("Unregistered window class and cleaned up");

            Ok(())
        }
    }
}

/// Shared state for the window procedure
#[cfg(windows)]
struct MonitorState {
    hdr_controller: Arc<Mutex<HdrController>>,
    cached_hdr_state: Arc<Mutex<bool>>,
    event_sender: mpsc::SyncSender<HdrStateEvent>,
    recheck_count: Arc<Mutex<u32>>, // Counter for remaining rechecks
}

// Thread-local storage for monitor state
#[cfg(windows)]
thread_local! {
    static MONITOR_STATE_TLS: std::cell::RefCell<Option<Arc<MonitorState>>> = const { std::cell::RefCell::new(None) };
}

/// Window procedure for the hidden window
///
/// Handles `WM_DISPLAYCHANGE`/`WM_SETTINGCHANGE`. Immediate check, then periodic rechecks
/// (500ms × 10 max) if state unchanged. Handles race condition where Windows messages
/// arrive before `DisplayConfig` APIs reflect state change.
///
/// # Safety
///
/// Signature matches `WNDPROC` contract: `unsafe extern "system" fn(HWND, u32, WPARAM,
/// LPARAM) -> LRESULT`. Windows guarantees valid `hwnd` and message parameters. TLS access
/// safe: state initialized in `run_message_loop` before window creation, callback runs on
/// same thread. Nested calls (`PostQuitMessage`, `DefWindowProcW`, `SetTimer`, `KillTimer`)
/// receive valid parameters. State access protected by `Mutex`/`RefCell`, no data races.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows FFI callback for window procedure handling WM_DISPLAYCHANGE and WM_SETTINGCHANGE messages"
)]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DISPLAYCHANGE => {
            debug!("Received WM_DISPLAYCHANGE message");

            // Try immediate check
            if check_hdr_state_change() {
                // State changed immediately - cancel any pending rechecks
                stop_periodic_rechecks(hwnd);
            } else {
                // State didn't change - start periodic rechecks
                debug!(
                    "HDR state unchanged on WM_DISPLAYCHANGE, starting periodic rechecks ({}ms interval, max {} rechecks)",
                    RECHECK_INTERVAL_MS, MAX_RECHECK_COUNT
                );
                start_periodic_rechecks(hwnd);
            }
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            debug!("Received WM_SETTINGCHANGE message");

            // Try immediate check
            if check_hdr_state_change() {
                // State changed immediately - cancel any pending rechecks
                stop_periodic_rechecks(hwnd);
            } else {
                // State didn't change - start periodic rechecks
                debug!(
                    "HDR state unchanged on WM_SETTINGCHANGE, starting periodic rechecks ({}ms interval, max {} rechecks)",
                    RECHECK_INTERVAL_MS, MAX_RECHECK_COUNT
                );
                start_periodic_rechecks(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ID_HDR_RECHECK => {
            // Periodic recheck
            if check_hdr_state_change() {
                // State changed - stop rechecking
                debug!("HDR state change detected during periodic recheck");
                stop_periodic_rechecks(hwnd);
            } else {
                // State still unchanged - check if we should continue rechecking
                MONITOR_STATE_TLS.with(|cell| {
                    if let Some(state) = cell.borrow().as_ref() {
                        let mut count = state.recheck_count.lock();
                        if *count > 0 {
                            *count -= 1;
                            debug!("HDR state still unchanged, {} rechecks remaining", *count);
                        } else {
                            // Max rechecks reached - stop timer
                            warn!(
                                "HDR state not updated after {} rechecks ({}ms total) - possible driver issue or false WM_DISPLAYCHANGE",
                                MAX_RECHECK_COUNT,
                                MAX_RECHECK_COUNT * RECHECK_INTERVAL_MS
                            );
                            stop_periodic_rechecks(hwnd);
                        }
                    }
                });
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            debug!("Received WM_DESTROY message");
            stop_periodic_rechecks(hwnd);
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Start periodic HDR state rechecks
///
/// Initializes the recheck counter and starts a timer for periodic rechecks.
///
/// # Safety
///
/// `hwnd` valid from `window_proc` (Windows-provided). `TIMER_ID_HDR_RECHECK` (1001)
/// uniquely identifies timer. `RECHECK_INTERVAL_MS` (500ms) valid. Null callback posts
/// timer messages to window queue (handled by `window_proc`).
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows FFI for SetTimer to schedule periodic HDR state rechecks"
)]
fn start_periodic_rechecks(hwnd: HWND) {
    MONITOR_STATE_TLS.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            // Reset recheck counter
            *state.recheck_count.lock() = MAX_RECHECK_COUNT;

            // Start timer
            unsafe {
                SetTimer(Some(hwnd), TIMER_ID_HDR_RECHECK, RECHECK_INTERVAL_MS, None);
            }
        }
    });
}

/// Stop periodic HDR state rechecks
///
/// Kills the recheck timer and resets the counter.
///
/// # Safety
///
/// `hwnd` valid from `window_proc` or Windows. `TIMER_ID_HDR_RECHECK` matches ID from
/// `start_periodic_rechecks`. `KillTimer` idempotent (safe if timer nonexistent/killed);
/// result ignored (no recovery needed).
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows FFI for KillTimer to stop periodic HDR state rechecks"
)]
fn stop_periodic_rechecks(hwnd: HWND) {
    MONITOR_STATE_TLS.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            // Reset counter
            *state.recheck_count.lock() = 0;

            // Kill timer
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_ID_HDR_RECHECK);
            }
        }
    });
}

/// Check HDR state and send event if changed
///
/// This function is called from the window procedure when display configuration changes.
///
/// # Returns
///
/// Returns `true` if the HDR state changed and an event was sent, `false` if the state
/// remained unchanged. This allows callers to schedule rechecks if needed.
#[cfg(windows)]
fn check_hdr_state_change() -> bool {
    MONITOR_STATE_TLS.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            // Query current HDR state
            let controller = state.hdr_controller.lock();
            let current_state = HdrStateMonitor::detect_current_hdr_state_internal(&controller);
            drop(controller);

            // Compare with cached state
            let mut cached_state = state.cached_hdr_state.lock();
            if current_state == *cached_state {
                debug!("HDR state unchanged: {}", current_state);
                false // State unchanged
            } else {
                info!(
                    "HDR state changed: {} -> {}",
                    if *cached_state { "ON" } else { "OFF" },
                    if current_state { "ON" } else { "OFF" }
                );

                // Update cached state
                *cached_state = current_state;

                // Send event
                let event = if current_state {
                    HdrStateEvent::Enabled
                } else {
                    HdrStateEvent::Disabled
                };

                if let Err(e) = state.event_sender.send(event) {
                    warn!("Failed to send HDR state event: {e}");
                } else {
                    debug!("Sent HDR state event: {event:?}");
                }

                true // State changed
            }
        } else {
            false // No state available
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hdr_state_monitor_creation() {
        // Create a mock HDR controller
        let hdr_controller = HdrController::new().expect("Failed to create HDR controller");

        // Create event channel
        let (tx, _rx) = mpsc::sync_channel(32);

        // Create HDR state monitor
        let monitor = HdrStateMonitor::new(hdr_controller, tx);
        assert!(monitor.is_ok());
    }

    #[test]
    fn test_hdr_state_event_types() {
        // Test event types
        assert_eq!(HdrStateEvent::Enabled, HdrStateEvent::Enabled);
        assert_eq!(HdrStateEvent::Disabled, HdrStateEvent::Disabled);
        assert_ne!(HdrStateEvent::Enabled, HdrStateEvent::Disabled);
    }

    #[test]
    fn test_detect_current_hdr_state() {
        // Create HDR controller
        let hdr_controller = HdrController::new().expect("Failed to create HDR controller");

        // Detect current state - this should not panic
        let _state = HdrStateMonitor::detect_current_hdr_state_internal(&hdr_controller);
        // The state is either true or false, both are valid
    }

    #[test]
    #[cfg(windows)]
    fn test_monitor_state_structure() {
        // Create HDR controller
        let hdr_controller = HdrController::new().expect("Failed to create HDR controller");

        // Create event channel
        let (tx, _rx) = mpsc::sync_channel(32);

        // Create monitor state
        let state = MonitorState {
            hdr_controller: Arc::new(Mutex::new(hdr_controller)),
            cached_hdr_state: Arc::new(Mutex::new(false)),
            event_sender: tx,
            recheck_count: Arc::new(Mutex::new(0)),
        };

        // Verify state structure
        let cached = *state.cached_hdr_state.lock();
        assert!(!cached);
    }
}
