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
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop, ns_string};

use crate::{Platform, PlatformError, Result};

#[derive(Debug, Default)]
pub struct MacPlatform;

impl MacPlatform {
    pub fn new() -> Self {
        Self
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

    fn play_sound(&self, _sound: Sound) {
        if let Some(sound) = NSSound::soundNamed(ns_string!("Glass")) {
            sound.play();
        }
    }
}
