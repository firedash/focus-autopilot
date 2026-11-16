// SPDX-License-Identifier: Apache-2.0
//! A scriptable platform for tests and the headless runner.

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use focus_core::{AppRef, Sound};

use crate::hotkey::{self, HotkeyAction, Hotkeys};
use crate::{Overlay, Platform, PlatformError, Result};

#[derive(Debug, Default)]
struct State {
    front: Option<AppRef>,
    running: Vec<AppRef>,
    idle_seconds: f64,
    refuse_activation: bool,
    sounds: usize,
    toast: Option<String>,
    pressed: Vec<HotkeyAction>,
    hotkeys_on: bool,
    recap: Option<Vec<String>>,
    watchers: Vec<Sender<Option<AppRef>>>,
}

#[derive(Debug, Default)]
pub struct FakePlatform {
    state: Mutex<State>,
}

impl FakePlatform {
    pub fn new(running: Vec<AppRef>, front: Option<AppRef>) -> Self {
        Self {
            state: Mutex::new(State {
                front,
                running,
                ..State::default()
            }),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        // A poisoned lock only means another test thread panicked; the state is still usable.
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut state)
    }

    /// The user (or the OS) brings an app to the front.
    pub fn set_front(&self, app: Option<AppRef>) {
        self.with(|s| {
            s.watchers.retain(|tx| tx.send(app.clone()).is_ok());
            s.front = app;
        });
    }

    pub fn set_idle_seconds(&self, seconds: f64) {
        self.with(|s| s.idle_seconds = seconds);
    }

    /// Makes the OS ignore activation requests, as macOS and Windows sometimes do.
    pub fn refuse_activation(&self, refuse: bool) {
        self.with(|s| s.refuse_activation = refuse);
    }

    /// The user presses a hotkey.
    pub fn press(&self, action: HotkeyAction) {
        self.with(|s| s.pressed.push(action));
    }

    /// The toast text now on screen, if any.
    pub fn toast(&self) -> Option<String> {
        self.with(|s| s.toast.clone())
    }

    /// The last recap card shown.
    pub fn recap(&self) -> Option<Vec<String>> {
        self.with(|s| s.recap.clone())
    }

    pub fn sounds_played(&self) -> usize {
        self.with(|s| s.sounds)
    }
}

impl Platform for FakePlatform {
    fn frontmost(&self) -> Result<Option<AppRef>> {
        Ok(self.with(|s| s.front.clone()))
    }

    fn watch_frontmost(&self, tx: Sender<Option<AppRef>>) {
        self.with(|s| s.watchers.push(tx));
    }

    fn running_apps(&self) -> Result<Vec<AppRef>> {
        Ok(self.with(|s| s.running.clone()))
    }

    fn seconds_since_input(&self) -> Result<f64> {
        Ok(self.with(|s| s.idle_seconds))
    }

    fn activate(&self, target: &AppRef) -> Result<()> {
        let (running, refused) = self.with(|s| {
            (
                s.running.iter().any(|app| app.matches(target)),
                s.refuse_activation,
            )
        });
        if !running {
            return Err(PlatformError::NotRunning);
        }
        if !refused {
            self.set_front(Some(target.clone()));
        }
        Ok(())
    }

    fn play_sound(&self, _sound: Sound) {
        self.with(|s| s.sounds += 1);
    }
}

impl Hotkeys for FakePlatform {
    fn register(&self, keys: &hotkey::Keys<'_>) -> Vec<String> {
        let mut problems = Vec::new();
        let mut any = false;
        for (key, _, name) in keys.each() {
            match hotkey::parse(key) {
                Ok(Some(_)) => any = true,
                Ok(None) => {}
                Err(e) => problems.push(format!("{name} hotkey: {e}")),
            }
        }
        self.with(|s| s.hotkeys_on = any);
        problems
    }

    fn take_pressed(&self) -> Vec<HotkeyAction> {
        self.with(|s| std::mem::take(&mut s.pressed))
    }

    fn active(&self) -> bool {
        self.with(|s| s.hotkeys_on)
    }
}

impl Overlay for FakePlatform {
    fn show_toast(&self, text: &str) {
        self.with(|s| s.toast = Some(text.to_owned()));
    }

    fn hide_toast(&self) {
        self.with(|s| s.toast = None);
    }

    fn show_recap(&self, lines: &[String], _for_how_long: std::time::Duration) {
        self.with(|s| s.recap = Some(lines.to_vec()));
    }

    fn tick(&self, _now: std::time::Instant) {}

    fn next_deadline(&self) -> Option<std::time::Instant> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use super::*;

    fn app(id: &str) -> AppRef {
        AppRef {
            id: id.to_owned(),
            name: id.to_owned(),
            pid: 1,
            window: None,
        }
    }

    #[test]
    fn activation_moves_focus_unless_the_os_refuses() {
        let platform = FakePlatform::new(vec![app("term"), app("game")], Some(app("term")));
        let (tx, rx) = channel();
        platform.watch_frontmost(tx);

        platform.activate(&app("game")).unwrap();
        assert_eq!(platform.frontmost().unwrap(), Some(app("game")));
        assert_eq!(rx.try_recv().unwrap(), Some(app("game")));

        platform.refuse_activation(true);
        platform.activate(&app("term")).unwrap();
        assert_eq!(platform.frontmost().unwrap(), Some(app("game")));
        assert!(rx.try_recv().is_err());

        assert_eq!(
            platform.activate(&app("gone")),
            Err(PlatformError::NotRunning)
        );
    }

    #[test]
    fn readings_and_sounds_are_scriptable() {
        let platform = FakePlatform::default();
        platform.set_idle_seconds(8.5);
        assert!((platform.seconds_since_input().unwrap() - 8.5).abs() < f64::EPSILON);
        assert!(platform.running_apps().unwrap().is_empty());
        platform.play_sound(Sound::Return);
        assert_eq!(platform.sounds_played(), 1);
    }
}
