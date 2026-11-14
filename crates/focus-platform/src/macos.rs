// SPDX-License-Identifier: Apache-2.0
//! The macOS adapter (design section 8). Uses `AppKit` and `CoreGraphics` through typed bindings.
//! No call here triggers an OS permission prompt: `NSWorkspace` and `NSRunningApplication` need
//! none, and the idle-time read asks the event source for a timestamp, never for input.
//!
//! Status: type-checked from Linux only; it has not run on a Mac. See `docs/adr` once the M2
//! spike records what the real OS does.

use std::sync::mpsc::Sender;

use focus_core::{AppRef, Sound};
use objc2_app_kit::{
    NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication, NSSound,
    NSWorkspace,
};
use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop, NSString, NSURL, ns_string};

use std::cell::Cell;

use crate::hotkey::{self, HotkeyAction, Hotkeys};
use crate::macos_hotkeys;
use crate::macos_overlay::MacOverlay;
use crate::{Overlay, Platform, PlatformError, Result};

/// Create it on the main thread: `AppKit` windows need it.
pub struct MacPlatform {
    overlay: MacOverlay,
    /// Hotkey ids: 1 is the focus key, 2 is pause. Set once registration succeeds.
    hotkeys_on: Cell<bool>,
}

impl MacPlatform {
    /// Deliberately no `Default`: building one has side effects and a thread requirement.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            overlay: MacOverlay::new(),
            hotkeys_on: Cell::new(false),
        }
    }

    /// `NSWorkspace` only refreshes `frontmostApplication` while the main run loop turns, and a
    /// command-line daemon doesn't turn it on its own. A short pump before each read fixes that.
    fn pump() {
        let run_loop = NSRunLoop::currentRunLoop();
        let until = NSDate::dateWithTimeIntervalSinceNow(0.01);
        // SAFETY: `NSDefaultRunLoopMode` is a valid mode constant and `until` is a live date.
        unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &until) };
    }
}

fn to_ref(app: &NSRunningApplication) -> Option<AppRef> {
    let id = app.bundleIdentifier()?.to_string();
    let name = app
        .localizedName()
        .map_or_else(|| id.clone(), |n| n.to_string());
    Some(AppRef {
        id,
        name,
        pid: u32::try_from(app.processIdentifier()).ok()?,
        window: None,
    })
}

impl Platform for MacPlatform {
    fn frontmost(&self) -> Result<Option<AppRef>> {
        Self::pump();
        let workspace = NSWorkspace::sharedWorkspace();
        Ok(workspace
            .frontmostApplication()
            .and_then(|app| to_ref(&app)))
    }

    fn watch_frontmost(&self, _tx: Sender<Option<AppRef>>) {
        // Not implemented: the shell polls `frontmost` instead, until the notification
        // observer lands with the M2 spike.
    }

    fn running_apps(&self) -> Result<Vec<AppRef>> {
        let workspace = NSWorkspace::sharedWorkspace();
        Ok(workspace
            .runningApplications()
            .iter()
            .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .filter_map(|app| to_ref(&app))
            .collect())
    }

    fn seconds_since_input(&self) -> Result<f64> {
        // `kCGAnyInputEventType` is `~0`; the call returns a time, not input content.
        let any_input = CGEventType(u32::MAX);
        Ok(CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            any_input,
        ))
    }

    fn activate(&self, target: &AppRef) -> Result<()> {
        let pid = i32::try_from(target.pid).map_err(|_| PlatformError::NotRunning)?;
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .ok_or(PlatformError::NotRunning)?;
        // A request only: macOS 14 may refuse a background process; the core verifies.
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        Ok(())
    }

    fn open_project(&self, target: &AppRef, folder: &str) -> Result<bool> {
        let Some(url) = crate::editor_url::editor_url(&target.id, folder) else {
            return Ok(false);
        };
        let Some(url) = NSURL::URLWithString(&NSString::from_str(&url)) else {
            return Ok(false);
        };
        // The OS routes the URL to the editor registered for its scheme, which brings it
        // forward with the window that has the folder open. No process is started here.
        let opened = NSWorkspace::sharedWorkspace().openURL(&url);
        if opened {
            Ok(true)
        } else {
            Err(PlatformError::Os(
                "the editor could not open the project URL".into(),
            ))
        }
    }

    fn play_sound(&self, _sound: Sound) {
        if let Some(sound) = NSSound::soundNamed(ns_string!("Glass")) {
            sound.play();
        }
    }
}

impl Overlay for MacPlatform {
    fn show_toast(&self, text: &str) {
        self.overlay.show_toast(text);
    }

    fn hide_toast(&self) {
        self.overlay.hide_toast();
    }

    fn show_recap(&self, lines: &[String], for_how_long: std::time::Duration) {
        self.overlay.show_recap(lines, for_how_long);
    }

    fn tick(&self, now: std::time::Instant) {
        Self::pump();
        self.overlay.dispatch_events();
        if self.hotkeys_on.get() {
            macos_hotkeys::pump();
        }
        self.overlay.tick(now);
    }

    fn next_deadline(&self) -> Option<std::time::Instant> {
        self.overlay.next_deadline()
    }
}

const FOCUS_ID: u32 = 1;
const PAUSE_ID: u32 = 2;

impl Hotkeys for MacPlatform {
    fn register(&self, focus_key: &str, pause_key: &str) -> Vec<String> {
        let mut problems = Vec::new();
        for (text, id, name) in [
            (focus_key, FOCUS_ID, "focus"),
            (pause_key, PAUSE_ID, "pause"),
        ] {
            match hotkey::parse(text) {
                Ok(None) => {}
                Ok(Some(chord)) => match macos_hotkeys::register(&chord, id) {
                    Ok(()) => self.hotkeys_on.set(true),
                    Err(e) => problems.push(format!("{name} hotkey `{text}`: {e}")),
                },
                Err(e) => problems.push(format!("{name} hotkey: {e}")),
            }
        }
        problems
    }

    fn take_pressed(&self) -> Vec<HotkeyAction> {
        macos_hotkeys::take()
            .into_iter()
            .filter_map(|id| match id {
                FOCUS_ID => Some(HotkeyAction::Focus),
                PAUSE_ID => Some(HotkeyAction::TogglePause),
                _ => None,
            })
            .collect()
    }

    fn active(&self) -> bool {
        self.hotkeys_on.get()
    }
}
