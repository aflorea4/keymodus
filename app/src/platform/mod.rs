//! Everything that differs between operating systems lives behind this module.
//!
//! Each platform file implements the same set of free functions:
//!
//! * input synthesis: [`key_down`], [`key_up`], [`mouse_button`], [`type_text`],
//!   [`media_key`], [`system_action`];
//! * context: [`frontmost_app`] (per-app profiles), [`permissions`],
//!   [`request_permission`];
//! * the "any brand" input hook: [`start_event_tap`];
//! * integration: login item, installed apps, opening files/URLs, revealing
//!   folders, OS settings deep links, Logi Options+ coexistence.
//!
//! Keys are always named by `KeyboardEvent.code` strings (see
//! `logi_core::keys`); each platform translates them to its own codes.

use logi_core::Modifier;
use serde::Serialize;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(windows)]
pub use windows::*;

/// What the generic "any brand" hook reports.
#[derive(Debug, Clone, Copy)]
// Linux has no any-brand hook yet, so nothing constructs these there.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub enum TapEvent {
    /// Buttons 2+ only (0 = left and 1 = right are never touched).
    MouseButton { button: u8, down: bool },
    /// `modified` = a modifier (⌘/Ctrl/Alt/⌥/Shift/Win) was held.
    Key { code: &'static str, down: bool, modified: bool },
}

/// Return `true` to swallow the event.
pub type TapHandler = Box<dyn Fn(TapEvent) -> bool + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct Permissions {
    /// May post synthetic input (macOS Accessibility, Linux /dev/uinput).
    pub accessibility: bool,
    /// May read HID devices (macOS Input Monitoring, Linux hidraw access).
    pub input_monitoring: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptionsPlus {
    pub installed: bool,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub name: String,
    /// Stable id used for per-app profiles: bundle id (macOS), lower-case
    /// executable name (Windows), WM_CLASS / desktop id (Linux).
    pub bundle_id: String,
    pub path: String,
}

/// Feature flags the UI uses to hide what this OS can't do.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub os: &'static str,
    /// Per-app profiles can detect the frontmost app.
    pub per_app: bool,
    /// The "All mice / All keyboards" hook works here.
    pub any_device: bool,
    /// Options+ import / pause.
    pub options_plus: bool,
    /// `SystemAction` names this platform can't perform.
    pub unsupported_system: Vec<&'static str>,
    pub unsupported_media: Vec<&'static str>,
}

pub fn key_tap(code: &str, mods: &[Modifier]) {
    key_down(code, mods);
    key_up(code, mods);
}

/// Fire-and-forget child process, reaped on a background thread.
pub(crate) fn spawn(program: &str, args: &[&str]) {
    use std::process::{Command, Stdio};
    let child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if let Ok(mut c) = child {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}
