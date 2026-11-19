// SPDX-License-Identifier: Apache-2.0
//! The platform layer (design section 8): everything that reads from or acts on the OS goes
//! through [`Platform`]. This is the only crate that may use `unsafe`, each block with a written
//! justification. There is an adapter for macOS and one for Windows, and a fake for tests.
//!
//! No call here may trigger an OS permission prompt, read input content or read window
//! titles. Nothing synthesizes a keystroke or a click; the Windows adapter can send one empty
//! mouse event to be allowed a switch (see `windows.rs` and ADR 0006).

pub mod fake;
pub mod hotkey;
pub mod inherit;
pub mod known_apps;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
mod macos_hotkeys;
#[cfg(target_os = "macos")]
mod macos_overlay;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
mod windows_overlay;

pub use hotkey::{HotkeyAction, Hotkeys};

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use focus_core::{AppRef, Sound};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    /// The OS call failed; the text is for the log and holds no user content.
    Os(String),
    /// The app to activate is no longer running.
    NotRunning,
}

pub type Result<T> = std::result::Result<T, PlatformError>;

pub trait Platform {
    /// The frontmost app: id, name, pid, and on Windows the window handle.
    ///
    /// # Errors
    /// When the OS call fails.
    fn frontmost(&self) -> Result<Option<AppRef>>;

    /// Sends the frontmost app whenever it changes, from OS notifications, without polling.
    fn watch_frontmost(&self, tx: Sender<Option<AppRef>>);

    /// Regular running apps, for the wizard and the away list.
    ///
    /// # Errors
    /// When the OS call fails.
    fn running_apps(&self) -> Result<Vec<AppRef>>;

    /// Global idle time. Never input content.
    ///
    /// # Errors
    /// When the OS call fails.
    fn seconds_since_input(&self) -> Result<f64>;

    /// Asks the OS to bring `target` to the front. This is a request the OS may refuse
    /// silently; the core verifies every switch.
    ///
    /// # Errors
    /// When the target is gone or the OS call fails.
    fn activate(&self, target: &AppRef) -> Result<()>;

    fn play_sound(&self, sound: Sound);
}

/// The on-screen toast and recap card (design section 8). Nothing here may take keyboard focus:
/// a toast that stole a keystroke would defeat its own purpose. Called from the shell thread,
/// which on macOS is the main thread.
pub trait Overlay {
    /// Shows this text as the toast, replacing any other.
    fn show_toast(&self, text: &str);
    fn hide_toast(&self);
    /// Shows the recap card for `for_how_long`, then hides it on a later `tick`.
    fn show_recap(&self, lines: &[String], for_how_long: Duration);
    /// Lets the overlay run its timers and draw. Call it every loop iteration.
    fn tick(&self, now: Instant);
    /// When `tick` must next be called for a timer to fire.
    fn next_deadline(&self) -> Option<Instant>;
}
