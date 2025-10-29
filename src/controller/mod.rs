//! Application logic controller module
//!
//! Coordinates between process monitoring, HDR control, and GUI.
//! Manages HDR state with debouncing to prevent rapid toggling.

pub mod app_controller;

pub use app_controller::{AppController, AppState};
