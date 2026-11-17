// SPDX-License-Identifier: Apache-2.0
//! The configuration page's view of `config.toml`: every setting a user may change, as flat
//! `section.key` names with JSON values, and the edit that writes a change back. The file is
//! edited in place, so the user's comments and layout survive, and nothing is written unless
//! the whole result is a valid config.
//!
//! The receiver's token and port and the history salt are deliberately not here: the token and
//! salt are secrets, and the port must match the installed hooks (`focusctl install-hooks`).

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use focus_core::{Config, ConfigError, RecapMode};
use focus_platform::hotkey;
use focus_receiver::config_file;
use serde_json::{Map, Value, json};
use toml_edit::{Array, DocumentMut, Item, value};

const LIST_MAX_ITEMS: usize = 50;
const TEXT_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    /// Whole number within these bounds, inclusive.
    Int(i64, i64),
    Text,
    List,
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct Key {
    pub name: &'static str,
    pub kind: Kind,
    /// Takes effect only when `focusd` is restarted.
    pub restart: bool,
}

const fn key(name: &'static str, kind: Kind) -> Key {
    Key {
        name,
        kind,
        restart: false,
    }
}

const fn restart(name: &'static str, kind: Kind) -> Key {
    Key {
        name,
        kind,
        restart: true,
    }
}

/// Every setting the page can change.
pub const KEYS: &[Key] = &[
    key("enabled", Kind::Bool),
    key("recap.mode", Kind::Choice(&["basic", "llm"])),
    key("recap.llm_timeout_seconds", Kind::Int(1, 120)),
    key("ui.sound_on_return", Kind::Bool),
    key("ui.recap_seconds", Kind::Int(1, 120)),
    key("away.apps", Kind::List),
    key("away.immersive", Kind::List),
    key("away.resume_last_app", Kind::Bool),
    key("home.extra_apps", Kind::List),
    key("switching.return_on_needs_input", Kind::Bool),
    key("switching.auto_return", Kind::Bool),
    restart("switching.force_foreground", Kind::Bool),
    key("switching.away_after_seconds", Kind::Int(0, 3_600)),
    key(
        "switching.away_after_seconds_when_long",
        Kind::Int(0, 3_600),
    ),
    key("switching.short_grace_seconds", Kind::Int(0, 3_600)),
    key("switching.away_requires_idle_seconds", Kind::Int(0, 600)),
    key("switching.input_quiet_ms", Kind::Int(0, 10_000)),
    key("switching.input_quiet_max_seconds", Kind::Int(0, 60)),
    key("switching.debounce_seconds", Kind::Int(0, 600)),
    key("switching.needs_input_debounce_seconds", Kind::Int(0, 60)),
    key("switching.resume_idle_seconds", Kind::Int(0, 600)),
    key("switching.stale_after_seconds", Kind::Int(60, 86_400)),
    key("switching.verify_after_ms", Kind::Int(50, 5_000)),
    key("countdown.seconds", Kind::Int(1, 60)),
    key("countdown.return_seconds", Kind::Int(1, 60)),
    key("countdown.snooze_seconds", Kind::Int(5, 3_600)),
    key("countdown.max_snoozes", Kind::Int(0, 20)),
    key("countdown.away_prompt_seconds", Kind::Int(5, 600)),
    restart("hotkeys.focus_key", Kind::Text),
    restart("hotkeys.pause", Kind::Text),
    restart("hotkeys.declare_long", Kind::Text),
    key("history.retention_days", Kind::Int(1, 3_650)),
    key("ignore.cwd_globs", Kind::List),
    key("stats.track_app_time", Kind::Bool),
];

fn secs(d: Duration) -> Value {
    json!(d.as_secs())
}

fn millis(d: Duration) -> Value {
    json!(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The current value of one setting, or `Null` for a name that isn't one.
fn value_of(c: &Config, name: &str) -> Value {
    let w = &c.switching;
    match name {
        "enabled" => json!(c.enabled),
        "recap.mode" => json!(match c.recap.mode {
            RecapMode::Basic => "basic",
            RecapMode::Llm => "llm",
        }),
        "recap.llm_timeout_seconds" => secs(c.recap.llm_timeout),
        "ui.sound_on_return" => json!(c.ui.sound_on_return),
        "ui.recap_seconds" => secs(c.ui.recap_shown_for),
        "away.apps" => json!(c.away.apps),
        "away.immersive" => json!(c.away.immersive),
        "away.resume_last_app" => json!(c.away.resume_last_app),
        "home.extra_apps" => json!(c.home.extra_apps),
        "switching.return_on_needs_input" => json!(w.return_on_needs_input),
        "switching.auto_return" => json!(w.auto_return),
        "switching.force_foreground" => json!(w.force_foreground),
        "switching.away_after_seconds" => secs(w.away_after),
        "switching.away_after_seconds_when_long" => secs(w.away_after_when_long),
        "switching.short_grace_seconds" => secs(w.short_grace),
        "switching.away_requires_idle_seconds" => secs(w.away_requires_idle),
        "switching.input_quiet_ms" => millis(w.input_quiet),
        "switching.input_quiet_max_seconds" => secs(w.input_quiet_max),
        "switching.debounce_seconds" => secs(w.debounce),
        "switching.needs_input_debounce_seconds" => secs(w.needs_input_debounce),
        "switching.resume_idle_seconds" => secs(w.resume_idle),
        "switching.stale_after_seconds" => secs(w.stale_after),
        "switching.verify_after_ms" => millis(w.verify_after),
        "countdown.seconds" => secs(c.countdown.length),
        "countdown.return_seconds" => secs(c.countdown.return_length),
        "countdown.snooze_seconds" => secs(c.countdown.snooze),
        "countdown.max_snoozes" => json!(c.countdown.max_snoozes),
        "countdown.away_prompt_seconds" => secs(c.countdown.away_prompt),
        "hotkeys.focus_key" => json!(c.hotkeys.focus_key),
        "hotkeys.pause" => json!(c.hotkeys.pause),
        "hotkeys.declare_long" => json!(c.hotkeys.declare_long),
        "history.retention_days" => json!(c.history.retention_days),
        "ignore.cwd_globs" => json!(c.ignore.cwd_globs),
        "stats.track_app_time" => json!(c.stats.track_app_time),
        _ => Value::Null,
    }
}

/// Every setting with its current value.
pub fn values(config: &Config) -> Map<String, Value> {
    KEYS.iter()
        .map(|k| (k.name.to_owned(), value_of(config, k.name)))
        .collect()
}

/// The settings among `changed` that need a restart to take effect and differ between the two
/// configs.
pub fn restart_needed(before: &Config, after: &Config) -> Vec<&'static str> {
    KEYS.iter()
        .filter(|k| k.restart && value_of(before, k.name) != value_of(after, k.name))
        .map(|k| k.name)
        .collect()
}

fn text_ok(text: &str) -> bool {
    text.chars().count() <= TEXT_MAX_CHARS && !text.chars().any(char::is_control)
}

/// Checks one value against its setting's type and turns it into a TOML value.
fn to_toml(k: &Key, v: &Value) -> Result<Item, String> {
    let wrong = |expected: &str| format!("`{}` must be {expected}", k.name);
    match k.kind {
        Kind::Bool => v.as_bool().map(value).ok_or_else(|| wrong("true or false")),
        Kind::Int(min, max) => {
            let n = v
                .as_i64()
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| wrong(&format!("a whole number from {min} to {max}")))?;
            Ok(value(n))
        }
        Kind::Text => {
            let text = v
                .as_str()
                .filter(|t| text_ok(t))
                .ok_or_else(|| wrong("text"))?;
            Ok(value(text.trim()))
        }
        Kind::Choice(options) => {
            let choice = v
                .as_str()
                .filter(|c| options.contains(c))
                .ok_or_else(|| wrong(&format!("one of: {}", options.join(", "))))?;
            Ok(value(choice))
        }
        Kind::List => {
            let items = v.as_array().ok_or_else(|| wrong("a list"))?;
            if items.len() > LIST_MAX_ITEMS {
                return Err(wrong(&format!(
                    "a list of at most {LIST_MAX_ITEMS} entries"
                )));
            }
            let mut array = Array::new();
            for item in items {
                let text = item
                    .as_str()
                    .map(str::trim)
                    .filter(|t| !t.is_empty() && text_ok(t))
                    .ok_or_else(|| wrong("a list of short, non-empty text entries"))?;
                array.push(text);
            }
            Ok(value(array))
        }
    }
}

/// Applies `changes` (setting name to new value) to the text of a `config.toml` and returns
/// the new text. Comments, order and settings not named are left as they were.
///
/// # Errors
/// An unknown setting, a value of the wrong type or out of range, or a file that isn't TOML.
pub fn edit(text: &str, changes: &Map<String, Value>) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("config.toml is not valid TOML: {e}"))?;
    for (name, new) in changes {
        let k = KEYS
            .iter()
            .find(|k| k.name == name)
            .ok_or_else(|| format!("`{name}` is not a setting that can be changed here"))?;
        let item = to_toml(k, new)?;
        match name.split_once('.') {
            None => doc[name.as_str()] = item,
            Some((section, field)) => {
                let table = doc
                    .entry(section)
                    .or_insert_with(toml_edit::table)
                    .as_table_mut()
                    .ok_or_else(|| format!("`{section}` in config.toml is not a section"))?;
                table[field] = item;
            }
        }
    }
    Ok(doc.to_string())
}

fn explain(error: &ConfigError) -> String {
    match error {
        ConfigError::PortZero => "the receiver port can't be 0".to_owned(),
        ConfigError::NoCountdown => {
            "the countdown can't be 0 seconds: every automatic switch must be announced".to_owned()
        }
        ConfigError::NoReturnCountdown => {
            "the countdown before a return can't be 0 seconds: every automatic switch must be \
             announced"
                .to_owned()
        }
        ConfigError::AwayAppIsHomeApp(app) => format!(
            "{app} is a home app (somewhere you prompt Claude from), so it can't also be an \
             away app"
        ),
        ConfigError::ImmersiveNotAwayApp(app) => {
            format!("{app} is marked immersive but is not in the away apps")
        }
    }
}

/// A sentence about settings in the text that an older build had and this one ignores, or
/// `None` when there are none.
#[must_use]
pub fn retired_note(text: &str) -> Option<String> {
    let keys = config_file::retired(text);
    if keys.is_empty() {
        return None;
    }
    let names: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
    let (verb, it) = if keys.len() == 1 {
        ("is no longer a setting and is", "it")
    } else {
        ("are no longer settings and are", "them")
    };
    Some(format!(
        "config.toml: {} {verb} ignored; you can delete {it}",
        names.join(", ")
    ))
}

/// Parses and validates config text the way `focusd` does at startup. `known_home` is the
/// built-in home-app list.
///
/// # Errors
/// A readable sentence saying what is wrong.
pub fn check(text: &str, known_home: &[String]) -> Result<Config, String> {
    let config = config_file::parse(text).map_err(|e| e.to_string())?;
    config
        .validate(|id| {
            known_home
                .iter()
                .chain(&config.home.extra_apps)
                .any(|entry| focus_core::app_id_matches(entry, id))
        })
        .map_err(|errors| errors.iter().map(explain).collect::<Vec<_>>().join("; "))?;
    for (name, chord) in [
        ("focus key", &config.hotkeys.focus_key),
        ("pause key", &config.hotkeys.pause),
        ("declare-long key", &config.hotkeys.declare_long),
    ] {
        hotkey::parse(chord).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(config)
}

/// Replaces the config file through a temporary file, readable by the owner only.
///
/// # Errors
/// When the folder or file can't be written.
pub fn write(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("config path has no folder")?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".config.{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let written = options
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&tmp, path));
    written.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn a_retired_setting_gets_a_sentence_and_does_not_fail_the_check() {
        let text = "[return]\nopen_project_in_editor = true\n";
        assert!(check(text, &home()).is_ok());
        assert_eq!(
            retired_note(text).as_deref(),
            Some("config.toml: `return` is no longer a setting and is ignored; you can delete it")
        );
        assert_eq!(retired_note("enabled = true"), None);
    }

    fn home() -> Vec<String> {
        vec![
            "com.apple.Terminal".to_owned(),
            "com.jetbrains.*".to_owned(),
        ]
    }

    #[test]
    fn every_setting_has_a_value_and_the_defaults_round_trip() {
        let defaults = Config::default();
        let all = values(&defaults);
        assert_eq!(all.len(), KEYS.len());
        for (name, v) in &all {
            assert!(!v.is_null(), "{name} has no value");
        }
        // Writing every default into an empty file gives back the default config.
        let text = edit("", &all).unwrap();
        assert_eq!(check(&text, &home()).unwrap(), defaults);
    }

    #[test]
    fn a_change_is_written_and_everything_else_is_left_alone() {
        let before = "# my notes\nenabled = true\n\n[receiver]\ntoken = \"secret\" # keep\n\n[recap]\nmode = \"basic\"\n";
        let after = edit(
            before,
            &changes(
                json!({"recap.mode": "llm", "away.apps": ["com.apple.Notes"], "enabled": false}),
            ),
        )
        .unwrap();
        assert!(after.starts_with("# my notes\nenabled = false\n"));
        assert!(after.contains("token = \"secret\" # keep"));
        assert!(after.contains("mode = \"llm\""));
        let config = check(&after, &home()).unwrap();
        assert_eq!(config.recap.mode, RecapMode::Llm);
        assert_eq!(config.away.apps, ["com.apple.Notes"]);
        assert!(!config.enabled);
        assert_eq!(config.receiver.token, "secret");
    }

    #[test]
    fn secrets_and_unknown_names_cannot_be_set() {
        for name in [
            "receiver.token",
            "receiver.port",
            "history.salt",
            "nope",
            "recap.bogus",
        ] {
            let err = edit("", &changes(json!({ name: "x" }))).unwrap_err();
            assert!(err.contains("not a setting"), "{name}: {err}");
        }
    }

    #[test]
    fn wrong_types_and_out_of_range_values_are_refused() {
        let bad = [
            json!({"enabled": "yes"}),
            json!({"recap.mode": "fancy"}),
            json!({"countdown.seconds": 0}),
            json!({"countdown.seconds": 61}),
            json!({"countdown.seconds": 2.5}),
            json!({"countdown.seconds": "3"}),
            json!({"away.apps": "com.apple.Notes"}),
            json!({"away.apps": [1, 2]}),
            json!({"away.apps": [""]}),
            json!({"away.apps": ["a\nb"]}),
            json!({"hotkeys.pause": 5}),
            json!({"hotkeys.pause": "x".repeat(300)}),
        ];
        for change in bad {
            assert!(
                edit("", &changes(change.clone())).is_err(),
                "{change} was accepted"
            );
        }
        let too_many: Vec<String> = (0..60).map(|i| format!("app{i}")).collect();
        assert!(edit("", &changes(json!({ "away.apps": too_many }))).is_err());
    }

    #[test]
    fn a_config_that_breaks_a_rule_is_explained_in_words() {
        let text = edit("", &changes(json!({"away.apps": ["com.apple.Terminal"]}))).unwrap();
        let err = check(&text, &home()).unwrap_err();
        assert!(err.contains("com.apple.Terminal is a home app"), "{err}");
        let text = edit(
            "",
            &changes(json!({"away.apps": ["com.jetbrains.intellij"]})),
        )
        .unwrap();
        assert!(
            check(&text, &home()).is_err(),
            "a prefix home entry must match too"
        );
        let text = edit("", &changes(json!({"away.immersive": ["Doom"]}))).unwrap();
        assert!(
            check(&text, &home())
                .unwrap_err()
                .contains("not in the away apps")
        );
        let text = edit("", &changes(json!({"hotkeys.pause": "Space"}))).unwrap();
        assert!(check(&text, &home()).unwrap_err().contains("pause key"));
        let text = edit("", &changes(json!({"hotkeys.pause": ""}))).unwrap();
        assert!(
            check(&text, &home()).is_ok(),
            "an empty hotkey turns it off"
        );
    }

    #[test]
    fn only_changed_restart_settings_are_reported() {
        let before = Config::default();
        let mut after = before.clone();
        after.recap.mode = RecapMode::Llm;
        assert!(restart_needed(&before, &after).is_empty());
        after.hotkeys.pause = "Control+Option+P".into();
        assert_eq!(restart_needed(&before, &after), ["hotkeys.pause"]);
    }

    #[test]
    fn a_file_that_is_not_toml_is_refused_untouched() {
        assert!(edit("= broken", &changes(json!({"enabled": false}))).is_err());
        assert!(edit("receiver = 5", &changes(json!({"recap.mode": "llm"}))).is_ok());
        assert!(edit("recap = 5", &changes(json!({"recap.mode": "llm"}))).is_err());
    }

    #[test]
    fn the_file_is_written_privately() {
        let dir = std::env::temp_dir().join(format!("focus-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        write(&path, "enabled = true\n").unwrap();
        write(&path, "enabled = false\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "enabled = false\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(dir);
    }
}
