// SPDX-License-Identifier: Apache-2.0
//! `focusctl doctor` (design backlog G4): checks the pieces that have to line up for Focus
//! Autopilot to work, and says how to fix each one that doesn't. It only reads; the one thing
//! it runs is `claude --version`.
//!
//! Not checked here, because only the running app on the real OS can: whether the hotkeys
//! register (`focusd --test-hotkeys`) and whether idle time can be read.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use focus_core::{Config, RecapMode};
use serde_json::Value;

use crate::hooks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Works, but the user may want to know.
    Note,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub level: Level,
    pub what: String,
    pub fix: Option<String>,
}

fn ok(what: impl Into<String>) -> Check {
    Check {
        level: Level::Ok,
        what: what.into(),
        fix: None,
    }
}

fn note(what: impl Into<String>, fix: &str) -> Check {
    Check {
        level: Level::Note,
        what: what.into(),
        fix: Some(fix.to_owned()),
    }
}

fn fail(what: impl Into<String>, fix: &str) -> Check {
    Check {
        level: Level::Fail,
        what: what.into(),
        fix: Some(fix.to_owned()),
    }
}

/// Is there a valid `config.toml` with a token?
pub fn config(path: &Path, known_home: &[String]) -> (Check, Option<Config>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (
            fail(
                format!("no config file at {}", path.display()),
                "run `focusctl install-hooks`, which creates it",
            ),
            None,
        );
    };
    match focus_runner::settings::check(&text, known_home) {
        Err(problem) => (
            fail(
                format!("config.toml is not valid: {problem}"),
                "correct that line in config.toml, or change the setting with `focusctl ui`",
            ),
            None,
        ),
        Ok(config) if config.receiver.token.is_empty() => (
            fail(
                "config.toml has no receiver token",
                "run `focusctl install-hooks`, which writes one",
            ),
            None,
        ),
        Ok(config) => {
            let note = focus_runner::settings::retired_note(&text)
                .map(|note| format!("; {note}"))
                .unwrap_or_default();
            (
                ok(format!("config.toml is valid ({}){note}", path.display())),
                Some(config),
            )
        }
    }
}

/// Are our hooks in Claude Code's settings, with the current token?
pub fn hooks(settings: &Path, port: u16, token: &str) -> Check {
    let fix = "run `focusctl install-hooks`, then restart running Claude Code sessions";
    let Ok(text) = std::fs::read_to_string(settings) else {
        return fail(
            format!("no Claude Code settings at {}", settings.display()),
            fix,
        );
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return fail(
            format!("{} is not strict JSON", settings.display()),
            "remove comments or trailing commas from it, then run `focusctl install-hooks`",
        );
    };
    let found = hooks::installed(&value, port, token);
    if found.complete() {
        ok(format!("hooks are installed in {}", settings.display()))
    } else if !found.missing.is_empty() {
        fail(
            format!(
                "{} of 11 hook events are missing from {} ({})",
                found.missing.len(),
                settings.display(),
                found.missing.join(", ")
            ),
            fix,
        )
    } else {
        fail(
            "the installed hooks carry an old token, so focusd will refuse them",
            fix,
        )
    }
}

/// One HTTP GET to the loopback port. `Ok(None)` when nothing is listening.
fn get(port: u16, path: &str, token: &str) -> std::io::Result<Option<(u16, String)>> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
        Ok(stream) => stream,
        // Windows retries a refused connection for longer than the timeout, so on a loopback
        // port a timeout means the same as a refusal: nothing is listening.
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Focus-Token: {token}\r\nConnection: close\r\n\r\n"
    )?;
    let mut reply = String::new();
    // A reply cut short by a timeout is still worth reading the status from.
    let _ = stream.read_to_string(&mut reply);
    let status = reply
        .strip_prefix("HTTP/1.")
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let body = reply.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    Ok(Some((status, body.to_owned())))
}

/// Is focusd running on the port and does it accept the token? On success also returns the
/// statistics it reported, for [`events`].
pub fn daemon(port: u16, token: &str) -> (Check, Option<Value>) {
    let other_program = fail(
        format!("something other than focusd is answering on port {port}"),
        "set a different `port` under [receiver] in config.toml, run `focusctl install-hooks`, \
         and restart focusd",
    );
    match get(port, "/api/stats", token) {
        Ok(None) => (
            fail(
                format!("focusd is not running (nothing is listening on port {port})"),
                "start `focusd`, or run `focusctl install-agent` so it starts at login",
            ),
            None,
        ),
        Ok(Some((200, body))) => match serde_json::from_str::<Value>(&body) {
            Ok(stats) if stats.get("week").is_some() => (
                ok(format!(
                    "focusd is running on port {port} and accepts the token"
                )),
                Some(stats),
            ),
            _ => (other_program, None),
        },
        Ok(Some((401, _))) => (
            fail(
                format!("focusd on port {port} refuses the token in config.toml"),
                "restart focusd: it was started before the token last changed",
            ),
            None,
        ),
        Ok(Some((404, _))) => (
            fail(
                format!("port {port} answers, but not as this version of focusd"),
                "restart focusd from the current build; if it still fails, another program has \
                 the port",
            ),
            None,
        ),
        Ok(Some(_)) | Err(_) => (other_program, None),
    }
}

/// Has focusd seen any Claude Code prompt lately?
pub fn events(stats: &Value) -> Check {
    match stats["week"]["prompts"].as_u64() {
        Some(n) if n > 0 => ok(format!("{n} Claude Code prompts seen in the last 7 days")),
        _ => note(
            "focusd has not seen a Claude Code prompt in the last 7 days",
            "send a prompt. If it still shows none: restart Claude Code sessions that were open \
             before the hooks were installed, accept the workspace trust dialog in that folder, \
             and check that your organisation does not restrict HTTP hooks (allowedHttpHookUrls)",
        ),
    }
}

/// The first `claude` on `path_env`, as the shell would find it. On Windows the command is
/// `claude.exe` (the native installer) or `claude.cmd` (npm).
pub fn find_claude(path_env: &str) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude"]
    } else {
        &["claude"]
    };
    std::env::split_paths(path_env)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Is the `claude` command there? It is required only for LLM recaps.
pub fn claude_cli(found: Option<&Path>, version: Option<&str>, config: Option<&Config>) -> Check {
    let wanted = config.is_some_and(|c| c.recap.mode == RecapMode::Llm);
    match (found, wanted) {
        (Some(path), _) => ok(match version {
            Some(version) => format!("claude is at {} ({version})", path.display()),
            None => format!("claude is at {}", path.display()),
        }),
        (None, true) => fail(
            "LLM recaps are on, but the `claude` command is not on the PATH",
            "install Claude Code or fix the PATH; under a launch agent, rerun \
             `focusctl install-agent` from a shell where `claude` works",
        ),
        (None, false) => note(
            "the `claude` command is not on the PATH",
            "nothing to do unless you turn on LLM recaps, which run it",
        ),
    }
}

/// Is the launch agent in place? Optional, so its absence is only a note.
pub fn agent(agents_dir: &Path) -> Check {
    let plist = crate::agent::plist_path(agents_dir);
    if plist.is_file() {
        ok("focusd is set to start at login")
    } else {
        note(
            "focusd is not set to start at login",
            "run `focusctl install-agent` if you want that",
        )
    }
}

/// Prints the checks and returns true when none failed.
pub fn report(checks: &[Check]) -> bool {
    for check in checks {
        let tag = match check.level {
            Level::Ok => "ok  ",
            Level::Note => "note",
            Level::Fail => "FAIL",
        };
        println!("{tag}  {}", check.what);
        if let Some(fix) = &check.fix {
            println!("      {fix}");
        }
    }
    let failed = checks.iter().filter(|c| c.level == Level::Fail).count();
    println!();
    if failed == 0 {
        println!("Everything checked is in order.");
    } else {
        println!("{failed} problem(s) found. Fix the first one and run `focusctl doctor` again.");
    }
    println!(
        "Not checked here: whether the hotkeys register. Run `focusd --test-hotkeys` for that."
    );
    failed == 0
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::mpsc::channel;

    use super::*;

    fn home() -> Vec<String> {
        vec!["com.apple.Terminal".to_owned()]
    }

    #[test]
    fn config_problems_are_named_with_their_fix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let (missing, _) = config(&path, &home());
        assert_eq!(missing.level, Level::Fail);
        assert!(missing.fix.unwrap().contains("install-hooks"));
        std::fs::write(&path, "[receiver]\ntokn = \"x\"\n").unwrap();
        let (typo, parsed) = config(&path, &home());
        assert!(
            typo.what.contains("receiver.tokn") && parsed.is_none(),
            "{}",
            typo.what
        );
        std::fs::write(&path, "enabled = true\n").unwrap();
        assert!(config(&path, &home()).0.what.contains("no receiver token"));
        std::fs::write(
            &path,
            "[receiver]\ntoken = \"t\"\n[away]\napps = [\"com.apple.Terminal\"]\n",
        )
        .unwrap();
        assert!(config(&path, &home()).0.what.contains("is a home app"));
        std::fs::write(&path, "[receiver]\ntoken = \"t\"\n").unwrap();
        let (good, parsed) = config(&path, &home());
        assert_eq!(good.level, Level::Ok);
        assert_eq!(parsed.unwrap().receiver.token, "t");
    }

    #[test]
    fn hook_problems_are_named_with_their_fix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(
            hooks(&path, 47821, "t")
                .what
                .starts_with("no Claude Code settings")
        );
        std::fs::write(&path, "{ // comment\n}").unwrap();
        assert!(hooks(&path, 47821, "t").what.contains("not strict JSON"));
        std::fs::write(&path, "{}").unwrap();
        assert!(
            hooks(&path, 47821, "t")
                .what
                .starts_with("11 of 11 hook events are missing")
        );
        let mut value: Value = serde_json::from_str("{}").unwrap();
        crate::hooks::install(&mut value, 47821, "t").unwrap();
        std::fs::write(&path, value.to_string()).unwrap();
        assert_eq!(hooks(&path, 47821, "t").level, Level::Ok);
        assert!(hooks(&path, 47821, "rotated").what.contains("old token"));
    }

    struct Backend;

    impl focus_receiver::UiBackend for Backend {
        fn stats(&self, _tz: i32) -> String {
            r#"{"week":{"prompts":4}}"#.to_owned()
        }

        fn config(&self) -> String {
            "{}".to_owned()
        }

        fn set_config(&self, _body: &[u8]) -> Result<String, String> {
            Err("no".to_owned())
        }

        fn clear_stats(&self, _tz: i32) -> String {
            "{}".to_owned()
        }
    }

    #[test]
    fn a_running_focusd_is_recognised_and_a_wrong_token_explained() {
        let (tx, _rx) = channel();
        let settings = focus_receiver::Settings {
            port: 0,
            token: "tok".into(),
            salt: "s".into(),
        };
        let running = focus_receiver::spawn_with_ui(settings, tx, Some(Arc::new(Backend))).unwrap();
        let port = running.addr.port();
        let (check, stats) = daemon(port, "tok");
        assert_eq!(check.level, Level::Ok, "{}", check.what);
        assert_eq!(
            events(&stats.unwrap()).what,
            "4 Claude Code prompts seen in the last 7 days"
        );
        let (refused, none) = daemon(port, "old");
        assert!(refused.what.contains("refuses the token") && none.is_none());
        assert!(refused.fix.unwrap().contains("restart focusd"));
    }

    #[test]
    fn nothing_listening_and_a_stranger_on_the_port_are_told_apart() {
        // A port that was free a moment ago.
        let free = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(daemon(free, "t").0.what.contains("not running"));
        // A program that accepts and answers with something that isn't focusd.
        let stranger = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = stranger.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = stranger.accept() {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
            }
        });
        let (check, _) = daemon(port, "t");
        assert!(
            check.what.contains("something other than focusd"),
            "{}",
            check.what
        );
    }

    #[test]
    fn no_events_yet_is_a_note_with_the_usual_causes() {
        let quiet = events(&serde_json::json!({"week": {"prompts": 0}}));
        assert_eq!(quiet.level, Level::Note);
        assert!(quiet.fix.unwrap().contains("workspace trust"));
    }

    #[test]
    fn claude_is_required_only_for_llm_recaps() {
        let mut llm = Config::default();
        llm.recap.mode = RecapMode::Llm;
        let basic = Config::default();
        assert_eq!(claude_cli(None, None, Some(&llm)).level, Level::Fail);
        assert_eq!(claude_cli(None, None, Some(&basic)).level, Level::Note);
        assert_eq!(claude_cli(None, None, None).level, Level::Note);
        let found = claude_cli(
            Some(Path::new("/usr/local/bin/claude")),
            Some("2.1.325"),
            Some(&llm),
        );
        assert_eq!(found.what, "claude is at /usr/local/bin/claude (2.1.325)");
    }

    #[test]
    fn claude_is_found_on_the_path_like_a_shell_would() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("claude"), "").unwrap();
        let path_env = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(
            find_claude(&path_env.to_string_lossy()),
            Some(b.join("claude"))
        );
        assert_eq!(find_claude(&a.to_string_lossy()), None);
    }

    #[test]
    fn the_launch_agent_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(agent(dir.path()).level, Level::Note);
        std::fs::write(crate::agent::plist_path(dir.path()), "").unwrap();
        assert_eq!(agent(dir.path()).level, Level::Ok);
    }
}
