// SPDX-License-Identifier: Apache-2.0
//! Loads `config.toml` (design section 11) into [`focus_core::Config`]. Every key is
//! optional; a missing key keeps its default. A key the app doesn't know is reported, not
//! silently ignored, so a typo doesn't leave a setting at its default unnoticed.

use std::time::Duration;

use focus_core::Config;
use toml::{Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Syntax(String),
    /// A key with the wrong type, e.g. `enabled = "yes"`.
    Type(String),
    Unknown(Vec<String>),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(e) => write!(f, "config.toml is not valid TOML: {e}"),
            Self::Type(k) => write!(f, "config.toml: `{k}` has the wrong type"),
            Self::Unknown(keys) => write!(f, "config.toml: unknown keys: {}", keys.join(", ")),
        }
    }
}

/// Parses config text over the defaults.
///
/// # Errors
/// Invalid TOML, a wrongly typed value, or unknown keys.
pub fn parse(text: &str) -> Result<Config, LoadError> {
    let mut table: Table = text
        .parse()
        .map_err(|e: toml::de::Error| LoadError::Syntax(e.message().to_owned()))?;
    let mut c = Config::default();
    let mut r = Reader {
        table: &mut table,
        path: String::new(),
        unknown: Vec::new(),
    };
    r.flag("enabled", &mut c.enabled)?;
    r.section("receiver", |s| {
        s.int("port", &mut c.receiver.port)?;
        s.text("token", &mut c.receiver.token)
    })?;
    r.section("home", |s| s.list("extra_apps", &mut c.home.extra_apps))?;
    r.section("away", |s| {
        s.list("apps", &mut c.away.apps)?;
        s.list("immersive", &mut c.away.immersive)
    })?;
    r.section("switching", |s| {
        let w = &mut c.switching;
        s.secs("away_after_seconds", &mut w.away_after)?;
        s.secs("away_after_seconds_when_long", &mut w.away_after_when_long)?;
        s.secs("short_grace_seconds", &mut w.short_grace)?;
        s.secs("away_requires_idle_seconds", &mut w.away_requires_idle)?;
        s.millis("input_quiet_ms", &mut w.input_quiet)?;
        s.secs("input_quiet_max_seconds", &mut w.input_quiet_max)?;
        s.secs("debounce_seconds", &mut w.debounce)?;
        s.secs("needs_input_debounce_seconds", &mut w.needs_input_debounce)?;
        s.secs("resume_idle_seconds", &mut w.resume_idle)?;
        s.secs("stale_after_seconds", &mut w.stale_after)?;
        s.flag("return_on_needs_input", &mut w.return_on_needs_input)?;
        s.millis("verify_after_ms", &mut w.verify_after)
    })?;
    r.section("countdown", |s| {
        s.secs("seconds", &mut c.countdown.length)?;
        s.secs("snooze_seconds", &mut c.countdown.snooze)?;
        s.int("max_snoozes", &mut c.countdown.max_snoozes)?;
        s.secs("away_prompt_seconds", &mut c.countdown.away_prompt)
    })?;
    r.section("hotkeys", |s| {
        s.text("focus_key", &mut c.hotkeys.focus_key)?;
        s.text("pause", &mut c.hotkeys.pause)?;
        s.text("declare_long", &mut c.hotkeys.declare_long)
    })?;
    r.section("ui", |s| {
        s.flag("sound_on_return", &mut c.ui.sound_on_return)?;
        s.secs("recap_seconds", &mut c.ui.recap_shown_for)?;
        s.flag("launch_at_login", &mut c.ui.launch_at_login)
    })?;
    r.section("recap", |s| {
        let mut mode = String::new();
        s.text("mode", &mut mode)?;
        match mode.as_str() {
            "" | "basic" => {}
            "llm" => c.recap.mode = focus_core::RecapMode::Llm,
            _ => return Err(LoadError::Type(s.key("mode"))),
        }
        s.secs("llm_timeout_seconds", &mut c.recap.llm_timeout)
    })?;
    r.section("history", |s| {
        s.int("retention_days", &mut c.history.retention_days)?;
        s.text("salt", &mut c.history.salt)
    })?;
    r.section("ignore", |s| s.list("cwd_globs", &mut c.ignore.cwd_globs))?;
    r.finish()?;
    Ok(c)
}

struct Reader<'a> {
    table: &'a mut Table,
    path: String,
    unknown: Vec<String>,
}

impl Reader<'_> {
    fn key(&self, name: &str) -> String {
        if self.path.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{name}", self.path)
        }
    }

    fn take(&mut self, name: &str) -> Option<Value> {
        self.table.remove(name)
    }

    fn flag(&mut self, name: &str, out: &mut bool) -> Result<(), LoadError> {
        match self.take(name) {
            None => Ok(()),
            Some(Value::Boolean(b)) => {
                *out = b;
                Ok(())
            }
            Some(_) => Err(LoadError::Type(self.key(name))),
        }
    }

    fn text(&mut self, name: &str, out: &mut String) -> Result<(), LoadError> {
        match self.take(name) {
            None => Ok(()),
            Some(Value::String(s)) => {
                *out = s;
                Ok(())
            }
            Some(_) => Err(LoadError::Type(self.key(name))),
        }
    }

    fn int<T: TryFrom<i64>>(&mut self, name: &str, out: &mut T) -> Result<(), LoadError> {
        match self.take(name) {
            None => Ok(()),
            Some(Value::Integer(i)) => {
                *out = T::try_from(i).map_err(|_| LoadError::Type(self.key(name)))?;
                Ok(())
            }
            Some(_) => Err(LoadError::Type(self.key(name))),
        }
    }

    fn secs(&mut self, name: &str, out: &mut Duration) -> Result<(), LoadError> {
        let mut n = out.as_secs();
        self.int(name, &mut n)?;
        *out = Duration::from_secs(n);
        Ok(())
    }

    fn millis(&mut self, name: &str, out: &mut Duration) -> Result<(), LoadError> {
        let mut n = u64::try_from(out.as_millis()).unwrap_or(u64::MAX);
        self.int(name, &mut n)?;
        *out = Duration::from_millis(n);
        Ok(())
    }

    fn list(&mut self, name: &str, out: &mut Vec<String>) -> Result<(), LoadError> {
        match self.take(name) {
            None => Ok(()),
            Some(Value::Array(items)) => {
                *out = items
                    .into_iter()
                    .map(|v| match v {
                        Value::String(s) => Ok(s),
                        _ => Err(LoadError::Type(self.key(name))),
                    })
                    .collect::<Result<_, _>>()?;
                Ok(())
            }
            Some(_) => Err(LoadError::Type(self.key(name))),
        }
    }

    fn section(
        &mut self,
        name: &str,
        read: impl FnOnce(&mut Reader<'_>) -> Result<(), LoadError>,
    ) -> Result<(), LoadError> {
        match self.take(name) {
            None => Ok(()),
            Some(Value::Table(mut inner)) => {
                let mut sub = Reader {
                    table: &mut inner,
                    path: self.key(name),
                    unknown: Vec::new(),
                };
                read(&mut sub)?;
                sub.finish_into(&mut self.unknown);
                Ok(())
            }
            Some(_) => Err(LoadError::Type(self.key(name))),
        }
    }

    fn finish_into(&mut self, unknown: &mut Vec<String>) {
        for key in self.table.keys() {
            unknown.push(self.key(key));
        }
    }

    fn finish(&mut self) -> Result<(), LoadError> {
        let mut unknown = std::mem::take(&mut self.unknown);
        self.finish_into(&mut unknown);
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(LoadError::Unknown(unknown))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_the_defaults() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn keys_override_defaults() {
        let c = parse(
            r#"
            enabled = false
            [receiver]
            port = 5000
            token = "abc"
            [away]
            apps = ["Obsidian.exe"]
            [switching]
            away_after_seconds = 5
            verify_after_ms = 100
            [recap]
            mode = "llm"
            "#,
        )
        .unwrap();
        assert!(!c.enabled);
        assert_eq!(c.receiver.port, 5000);
        assert_eq!(c.receiver.token, "abc");
        assert_eq!(c.away.apps, vec!["Obsidian.exe"]);
        assert_eq!(c.switching.away_after, Duration::from_secs(5));
        assert_eq!(c.switching.verify_after, Duration::from_millis(100));
        assert_eq!(c.switching.stale_after, Duration::from_secs(600));
        assert_eq!(c.recap.mode, focus_core::RecapMode::Llm);
    }

    #[test]
    fn typos_and_wrong_types_are_errors() {
        assert_eq!(
            parse("[switching]\naway_after_secs = 5\nbogus = 1\n[nope]\nx = 1"),
            Err(LoadError::Unknown(vec![
                "switching.away_after_secs".into(),
                "switching.bogus".into(),
                "nope".into()
            ]))
        );
        assert!(matches!(
            parse("enabled = \"yes\""),
            Err(LoadError::Type(_))
        ));
        assert!(matches!(
            parse("[receiver]\nport = 70000"),
            Err(LoadError::Type(_))
        ));
        assert!(matches!(
            parse("[receiver]\nport = -1"),
            Err(LoadError::Type(_))
        ));
        assert!(matches!(parse("= broken"), Err(LoadError::Syntax(_))));
    }
}
