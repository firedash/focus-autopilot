// SPDX-License-Identifier: Apache-2.0
//! Every config key from design section 11, with its default. Parsing config.toml is the
//! shell's job; the core only holds and checks the values.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub receiver: Receiver,
    pub home: Home,
    pub away: Away,
    pub switching: Switching,
    pub countdown: Countdown,
    pub hotkeys: Hotkeys,
    pub ui: Ui,
    pub recap: Recap,
    pub history: History,
    pub ignore: Ignore,
    pub stats: Stats,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            receiver: Receiver::default(),
            home: Home::default(),
            away: Away::default(),
            switching: Switching::default(),
            countdown: Countdown::default(),
            hotkeys: Hotkeys::default(),
            ui: Ui::default(),
            recap: Recap::default(),
            history: History::default(),
            ignore: Ignore::default(),
            stats: Stats::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receiver {
    /// Must match the installed hooks.
    pub port: u16,
    /// Written by `focusctl install-hooks`. Never logged.
    pub token: String,
}

impl Default for Receiver {
    fn default() -> Self {
        Self {
            port: 47821,
            token: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Home {
    /// Bundle ids (macOS) or executable names (Windows) treated as home apps, on top of the
    /// built-in list.
    pub extra_apps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Away {
    /// Ordered; the first running app wins.
    pub apps: Vec<String>,
    /// Subset of `apps` that get a longer countdown with a warning (V1).
    pub immersive: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switching {
    /// `away_after_seconds`
    pub away_after: Duration,
    /// `away_after_seconds_when_long`
    pub away_after_when_long: Duration,
    /// `short_grace_seconds`
    pub short_grace: Duration,
    /// `away_requires_idle_seconds`
    pub away_requires_idle: Duration,
    /// `input_quiet_ms`
    pub input_quiet: Duration,
    /// `input_quiet_max_seconds`
    pub input_quiet_max: Duration,
    /// `debounce_seconds`
    pub debounce: Duration,
    /// `needs_input_debounce_seconds`
    pub needs_input_debounce: Duration,
    /// `resume_idle_seconds`
    pub resume_idle: Duration,
    /// `stale_after_seconds`
    pub stale_after: Duration,
    pub return_on_needs_input: bool,
    /// `verify_after_ms`
    pub verify_after: Duration,
}

impl Default for Switching {
    fn default() -> Self {
        Self {
            away_after: Duration::from_secs(30),
            away_after_when_long: Duration::from_secs(10),
            short_grace: Duration::from_secs(45),
            away_requires_idle: Duration::from_secs(8),
            input_quiet: Duration::from_millis(600),
            input_quiet_max: Duration::from_secs(5),
            debounce: Duration::from_secs(5),
            needs_input_debounce: Duration::from_secs(2),
            resume_idle: Duration::from_secs(8),
            stale_after: Duration::from_secs(600),
            return_on_needs_input: true,
            verify_after: Duration::from_millis(400),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Countdown {
    /// `seconds`
    pub length: Duration,
    /// `snooze_seconds`
    pub snooze: Duration,
    pub max_snoozes: u32,
    /// `away_prompt_seconds`
    pub away_prompt: Duration,
}

impl Default for Countdown {
    fn default() -> Self {
        Self {
            length: Duration::from_secs(3),
            snooze: Duration::from_secs(60),
            max_snoozes: 3,
            away_prompt: Duration::from_secs(30),
        }
    }
}

/// An empty string turns a hotkey off. Defaults differ by OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkeys {
    pub focus_key: String,
    pub pause: String,
    /// V1.
    pub declare_long: String,
}

impl Hotkeys {
    pub fn macos() -> Self {
        Self {
            focus_key: "Control+Option+Command+Space".into(),
            pause: "Control+Option+Command+P".into(),
            declare_long: String::new(),
        }
    }

    pub fn windows() -> Self {
        Self {
            focus_key: "Ctrl+Alt+Space".into(),
            pause: "Ctrl+Alt+Shift+P".into(),
            declare_long: String::new(),
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        if cfg!(windows) {
            Self::windows()
        } else {
            Self::macos()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    pub sound_on_return: bool,
    /// `recap_seconds`
    pub recap_shown_for: Duration,
    pub launch_at_login: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            sound_on_return: true,
            recap_shown_for: Duration::from_secs(10),
            launch_at_login: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecapMode {
    #[default]
    Basic,
    /// V1, opt-in.
    Llm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recap {
    pub mode: RecapMode,
    /// `llm_timeout_seconds`
    pub llm_timeout: Duration,
}

impl Default for Recap {
    fn default() -> Self {
        Self {
            mode: RecapMode::Basic,
            llm_timeout: Duration::from_secs(20),
        }
    }
}

/// V1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub retention_days: u32,
    /// Random, written on first run.
    pub salt: String,
}

impl Default for History {
    fn default() -> Self {
        Self {
            retention_days: 90,
            salt: String::new(),
        }
    }
}

/// V1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ignore {
    pub cwd_globs: Vec<String>,
}

/// `[stats]`: what the shell records for the statistics page. Everything stays on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// `track_app_time`: record how long each app is in front (app ids and names only, never
    /// window titles or content), so the statistics page can show where waiting time goes.
    pub track_app_time: bool,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            track_app_time: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `receiver.port` is 0.
    PortZero,
    /// `countdown.seconds` is 0; every automatic switch must be announced.
    NoCountdown,
    /// An app is both an away app and a home app.
    AwayAppIsHomeApp(String),
    /// `away.immersive` names an app that isn't in `away.apps`.
    ImmersiveNotAwayApp(String),
}

impl Config {
    /// Checks the rules the design states for config values. `is_home` says whether an app id
    /// is a home app, built-in or extra.
    ///
    /// # Errors
    /// Returns every rule the config breaks.
    pub fn validate(&self, is_home: impl Fn(&str) -> bool) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();
        if self.receiver.port == 0 {
            errors.push(ConfigError::PortZero);
        }
        if self.countdown.length.is_zero() {
            errors.push(ConfigError::NoCountdown);
        }
        for app in &self.away.apps {
            if is_home(app) {
                errors.push(ConfigError::AwayAppIsHomeApp(app.clone()));
            }
        }
        for app in &self.away.immersive {
            if !self.away.apps.iter().any(|a| a.eq_ignore_ascii_case(app)) {
                errors.push(ConfigError::ImmersiveNotAwayApp(app.clone()));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design() {
        let c = Config::default();
        assert!(c.enabled);
        assert_eq!(c.receiver.port, 47821);
        assert_eq!(c.switching.away_after, Duration::from_secs(30));
        assert_eq!(c.switching.stale_after, Duration::from_secs(600));
        assert_eq!(c.switching.verify_after, Duration::from_millis(400));
        assert_eq!(c.countdown.length, Duration::from_secs(3));
        assert_eq!(c.countdown.max_snoozes, 3);
        assert!(c.ui.sound_on_return);
        assert_eq!(c.recap.mode, RecapMode::Basic);
        assert_eq!(c.history.retention_days, 90);
        assert_eq!(Hotkeys::windows().focus_key, "Ctrl+Alt+Space");
        assert_eq!(Hotkeys::macos().pause, "Control+Option+Command+P");
        assert_eq!(c.validate(|_| false), Ok(()));
    }

    #[test]
    fn validate_reports_every_broken_rule() {
        let mut c = Config::default();
        c.receiver.port = 0;
        c.countdown.length = Duration::ZERO;
        c.away.apps = vec!["Code.exe".into()];
        c.away.immersive = vec!["Doom.exe".into()];
        let errors = c.validate(|id| id == "Code.exe").unwrap_err();
        assert_eq!(
            errors,
            vec![
                ConfigError::PortZero,
                ConfigError::NoCountdown,
                ConfigError::AwayAppIsHomeApp("Code.exe".into()),
                ConfigError::ImmersiveNotAwayApp("Doom.exe".into()),
            ]
        );
    }
}
