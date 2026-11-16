// SPDX-License-Identifier: Apache-2.0
//! What stands behind the statistics and configuration page: state shared between the shell
//! loop, which records and applies, and the receiver's thread, which answers the page.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard, PoisonError};

use focus_core::{AppRef, Config};
use focus_receiver::UiBackend;
use serde_json::{Map, Value, json};

use crate::Msg;
use crate::history::unix_now;
use crate::settings;
use crate::stats::{self, Query};
use crate::usage::Usage;

/// The files the page reads and writes.
#[derive(Debug, Clone)]
pub struct Files {
    pub config: PathBuf,
    /// `runs.jsonl`, if history is kept.
    pub runs: Option<PathBuf>,
    /// `usage.json`, if app time is kept across launches.
    pub usage: Option<PathBuf>,
}

pub struct Shared {
    usage: Mutex<Usage>,
    status: Mutex<&'static str>,
    running_apps: Mutex<Vec<AppRef>>,
    config: Mutex<Config>,
    /// One settings change at a time: read, edit, check, write.
    saving: Mutex<()>,
    notify: Mutex<Sender<Msg>>,
    files: Files,
    /// The built-in home apps; the config adds to them.
    known_home: Vec<String>,
}

/// A poisoned lock only means a thread panicked while holding it; the data is still the best
/// there is, and the page must keep working.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// Loads the saved usage, dropping what is older than the config's retention.
    pub fn new(config: Config, known_home: Vec<String>, files: Files, notify: Sender<Msg>) -> Self {
        let mut usage = files.usage.as_deref().map(Usage::load).unwrap_or_default();
        usage.prune(unix_now(), config.history.retention_days);
        Self {
            usage: Mutex::new(usage),
            status: Mutex::new("idle"),
            running_apps: Mutex::new(Vec::new()),
            config: Mutex::new(config),
            saving: Mutex::new(()),
            notify: Mutex::new(notify),
            files,
            known_home,
        }
    }

    pub fn usage(&self) -> MutexGuard<'_, Usage> {
        lock(&self.usage)
    }

    pub fn config_now(&self) -> Config {
        lock(&self.config).clone()
    }

    pub fn set_config_now(&self, config: Config) {
        *lock(&self.config) = config;
    }

    pub fn set_status(&self, status: &'static str) {
        *lock(&self.status) = status;
    }

    pub fn set_running_apps(&self, apps: Vec<AppRef>) {
        *lock(&self.running_apps) = apps;
    }

    pub fn files(&self) -> &Files {
        &self.files
    }

    pub fn known_home(&self) -> &[String] {
        &self.known_home
    }

    /// Writes the usage file, if there is one.
    ///
    /// # Errors
    /// When the file can't be written.
    pub fn save_usage(&self) -> Result<(), String> {
        let Some(path) = &self.files.usage else {
            return Ok(());
        };
        // Clone first so the lock isn't held while writing.
        let snapshot = self.usage().clone();
        snapshot.save(path)
    }

    fn is_home(&self, config: &Config, id: &str) -> bool {
        self.known_home
            .iter()
            .chain(&config.home.extra_apps)
            .any(|entry| focus_core::app_id_matches(entry, id))
    }

    fn stats_json(&self, tz_minutes: i32) -> String {
        let config = self.config_now();
        let runs = self
            .files
            .runs
            .as_deref()
            .map(stats::read_runs)
            .unwrap_or_default();
        let usage = self.usage().clone();
        let status = *lock(&self.status);
        let is_home = |id: &str| self.is_home(&config, id);
        let summary = stats::summarize(
            &usage,
            &runs,
            &Query {
                now_unix: unix_now(),
                tz_minutes,
                is_home: &is_home,
                status,
                tracking: config.stats.track_app_time,
            },
        );
        serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_owned())
    }

    fn config_json(&self, config: &Config, restart_required: &[&str]) -> String {
        let apps: Vec<Value> = lock(&self.running_apps)
            .iter()
            .map(|app| json!({ "id": app.id, "name": app.name, "home": self.is_home(config, &app.id) }))
            .collect();
        let restart_keys: Vec<&str> = settings::KEYS
            .iter()
            .filter(|k| k.restart)
            .map(|k| k.name)
            .collect();
        json!({
            "values": settings::values(config),
            "running_apps": apps,
            "restart_keys": restart_keys,
            "restart_required": restart_required,
            "config_path": self.files.config.to_string_lossy(),
            "port": config.receiver.port,
        })
        .to_string()
    }

    fn apply(&self, body: &[u8]) -> Result<String, String> {
        let request: Value = serde_json::from_slice(body)
            .map_err(|_| "the request was not valid JSON".to_owned())?;
        let changes: &Map<String, Value> = request
            .get("values")
            .and_then(Value::as_object)
            .ok_or("the request had no `values` to change")?;
        let _one_at_a_time = lock(&self.saving);
        let path = &self.files.config;
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let edited = settings::edit(&text, changes)?;
        let config = settings::check(&edited, &self.known_home)?;
        settings::write(path, &edited)?;
        let before = self.config_now();
        let restart = settings::restart_needed(&before, &config);
        self.set_config_now(config.clone());
        let reply = self.config_json(&config, &restart);
        // The loop applies it to the core; if the loop is gone the file is still saved.
        let _ = lock(&self.notify).send(Msg::Config(Box::new(config)));
        Ok(reply)
    }
}

impl UiBackend for Shared {
    fn stats(&self, tz_minutes: i32) -> String {
        self.stats_json(tz_minutes)
    }

    fn config(&self) -> String {
        self.config_json(&self.config_now(), &[])
    }

    fn set_config(&self, body: &[u8]) -> Result<String, String> {
        self.apply(body)
    }

    fn clear_stats(&self, tz_minutes: i32) -> String {
        *self.usage() = Usage::default();
        if let Some(path) = &self.files.usage {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = &self.files.runs {
            let _ = std::fs::remove_file(path);
        }
        // The estimator's memory of past runs goes too.
        let _ = lock(&self.notify).send(Msg::HistoryCleared);
        self.stats_json(tz_minutes)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, channel};

    use super::*;
    use crate::usage::Moment;

    fn setup(name: &str) -> (Shared, Receiver<Msg>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("focus-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = channel();
        let files = Files {
            config: dir.join("config.toml"),
            runs: Some(dir.join("runs.jsonl")),
            usage: Some(dir.join("usage.json")),
        };
        let shared = Shared::new(
            Config::default(),
            vec!["com.apple.Terminal".into()],
            files,
            tx,
        );
        (shared, rx, dir)
    }

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_settings_change_is_saved_applied_and_reported() {
        let (shared, rx, dir) = setup("save");
        std::fs::write(
            dir.join("config.toml"),
            "# mine\n[receiver]\ntoken = \"t\"\n",
        )
        .unwrap();
        let reply = shared
            .set_config(br#"{"values":{"recap.mode":"llm","hotkeys.pause":"Control+Option+P"}}"#)
            .unwrap();
        let reply = parse(&reply);
        assert_eq!(reply["values"]["recap.mode"], "llm");
        assert_eq!(reply["restart_required"], json!(["hotkeys.pause"]));
        let saved = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(saved.starts_with("# mine\n") && saved.contains("token = \"t\""));
        assert!(saved.contains("mode = \"llm\""));
        match rx.try_recv() {
            Ok(Msg::Config(config)) => assert_eq!(config.recap.mode, focus_core::RecapMode::Llm),
            _ => panic!("the loop was not told"),
        }
        assert_eq!(parse(&shared.config())["values"]["recap.mode"], "llm");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_invalid_change_saves_nothing_and_tells_nobody() {
        let (shared, rx, dir) = setup("invalid");
        std::fs::write(dir.join("config.toml"), "enabled = true\n").unwrap();
        for body in [
            &br#"{"values":{"away.apps":["com.apple.Terminal"]}}"#[..],
            br#"{"values":{"receiver.token":"stolen"}}"#,
            br#"{"values":{"countdown.seconds":0}}"#,
            br#"{"nothing":1}"#,
            b"not json",
        ] {
            assert!(shared.set_config(body).is_err());
        }
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            "enabled = true\n"
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(shared.config_now(), Config::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_config_reply_never_carries_the_token() {
        let (shared, _rx, dir) = setup("token");
        let mut config = Config::default();
        config.receiver.token = "super-secret-token".into();
        config.history.salt = "super-secret-salt".into();
        shared.set_config_now(config);
        let text = shared.config();
        assert!(!text.contains("super-secret"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn running_apps_are_listed_with_whether_they_are_home_apps() {
        let (shared, _rx, dir) = setup("apps");
        let app = |id: &str, name: &str| AppRef {
            id: id.into(),
            name: name.into(),
            pid: 1,
            window: None,
        };
        shared.set_running_apps(vec![
            app("com.apple.Terminal", "Terminal"),
            app("com.apple.Notes", "Notes"),
        ]);
        let apps = parse(&shared.config())["running_apps"].clone();
        assert_eq!(apps[0]["home"], true);
        assert_eq!(
            (apps[1]["name"].as_str(), apps[1]["home"].as_bool()),
            (Some("Notes"), Some(false))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clearing_statistics_erases_usage_and_history_and_tells_the_loop() {
        let (shared, rx, dir) = setup("clear");
        std::fs::write(
            dir.join("runs.jsonl"),
            "{\"start\":\"2026-11-14T12:00:00Z\",\"duration_s\":60}\n",
        )
        .unwrap();
        shared.usage().add_time(
            unix_now(),
            Moment {
                app_id: "a",
                app_name: "A",
                is_home: false,
                claude_working: true,
            },
            90_000,
        );
        shared.save_usage().unwrap();
        assert_eq!(parse(&shared.stats(0))["today"]["tracked_s"], 90);
        let after = parse(&shared.clear_stats(0));
        assert_eq!(after["today"]["tracked_s"], 0);
        assert_eq!(after["week"]["prompts"], 0);
        assert!(!dir.join("runs.jsonl").exists() && !dir.join("usage.json").exists());
        assert!(matches!(rx.try_recv(), Ok(Msg::HistoryCleared)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saved_usage_is_there_at_the_next_launch() {
        let (shared, _rx, dir) = setup("persist");
        shared.usage().add_time(
            unix_now(),
            Moment {
                app_id: "a",
                app_name: "A",
                is_home: false,
                claude_working: false,
            },
            5_000,
        );
        shared.save_usage().unwrap();
        let (tx, _rx2) = channel();
        let again = Shared::new(Config::default(), Vec::new(), shared.files().clone(), tx);
        assert_eq!(parse(&again.stats(0))["today"]["tracked_s"], 5);
        let _ = std::fs::remove_dir_all(dir);
    }
}
