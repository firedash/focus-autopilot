// SPDX-License-Identifier: Apache-2.0
//! Parses a Claude Code hook payload into a [`focus_core::AgentEvent`] (design section 6).
//! Only the listed fields are read; the rest is skipped. A field with an unexpected type is
//! treated as absent, so a Claude Code update that reshapes a field can't break parsing.

use std::fmt::Write as _;

use focus_core::{
    AgentEvent, COMMAND_MAX_CHARS, EventKind, MESSAGE_MAX_CHARS, PATH_MAX_CHARS, PROMPT_MAX_CHARS,
    clean_text, redact,
};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not JSON, not an object, or `session_id` / `hook_event_name` missing.
    Malformed,
    /// A well-formed payload for an event focusd doesn't register.
    UnknownEvent(String),
}

/// Extra characters read past the cut, so a key that straddles it is still seen whole by the
/// redactor before the text is shortened.
const REDACT_WINDOW: usize = 128;

/// Redacts likely secrets, then cleans and shortens. The order matters: truncating first could
/// leave half a key too short to recognise.
fn keep(raw: &str, max_chars: usize) -> String {
    let window: String = raw.chars().take(max_chars + REDACT_WINDOW).collect();
    clean_text(&redact(&window), max_chars)
}

/// `Some` only for a JSON string; any other type reads as absent.
fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        S(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match V::deserialize(d)? {
        V::S(s) => Some(s),
        V::Other(_) => None,
    })
}

fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        B(bool),
        Other(serde::de::IgnoredAny),
    }
    Ok(matches!(V::deserialize(d)?, V::B(true)))
}

/// Number of entries when the value is an array, 0 for anything else.
fn array_len<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        A(Vec<serde::de::IgnoredAny>),
        Other(serde::de::IgnoredAny),
    }
    Ok(match V::deserialize(d)? {
        V::A(a) => a.len(),
        V::Other(_) => 0,
    })
}

#[derive(Deserialize, Default)]
struct ToolInput {
    #[serde(default, deserialize_with = "lenient")]
    command: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    file_path: Option<String>,
}

#[derive(Deserialize, Default)]
struct Effort {
    #[serde(default, deserialize_with = "lenient")]
    level: Option<String>,
}

fn lenient_object<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(
    d: D,
) -> Result<T, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V<T> {
        Object(T),
        Other(serde::de::IgnoredAny),
    }
    Ok(match V::<T>::deserialize(d)? {
        V::Object(t) => t,
        V::Other(_) => T::default(),
    })
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default, deserialize_with = "lenient")]
    hook_event_name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    target_app: Option<String>,
    #[serde(default, deserialize_with = "lenient_bool")]
    failed: bool,
    #[serde(default, deserialize_with = "lenient")]
    session_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    prompt_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    cwd: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    permission_mode: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    prompt: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    tool_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_object")]
    tool_input: ToolInput,
    #[serde(default, deserialize_with = "lenient_object")]
    effort: Effort,
    #[serde(default, deserialize_with = "lenient")]
    notification_type: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    mcp_server_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_bool")]
    stop_hook_active: bool,
    #[serde(default, deserialize_with = "lenient")]
    last_assistant_message: Option<String>,
    #[serde(default, deserialize_with = "array_len")]
    background_tasks: usize,
    #[serde(default, deserialize_with = "lenient")]
    error: Option<String>,
}

/// Parses one payload. `salt` keys the project hash; see [`project_key`].
///
/// # Errors
/// [`ParseError::Malformed`] for bad JSON or missing ids, [`ParseError::UnknownEvent`] for an
/// event focusd doesn't use.
pub fn parse(body: &[u8], salt: &str) -> Result<AgentEvent, ParseError> {
    let raw: Raw = serde_json::from_slice(body).map_err(|_| ParseError::Malformed)?;
    let (Some(name), Some(session_id)) = (raw.hook_event_name, raw.session_id) else {
        return Err(ParseError::Malformed);
    };
    let tool_name = clean_text(&raw.tool_name.unwrap_or_default(), 100);
    let command = raw.tool_input.command.map(|c| keep(&c, COMMAND_MAX_CHARS));
    let file_path = raw
        .tool_input
        .file_path
        .map(|p| clean_text(&p, PATH_MAX_CHARS));
    let kind = match name.as_str() {
        "AgentActivity" => EventKind::AgentActivity,
        "AppFinished" => EventKind::AppFinished {
            app_name: clean_text(&raw.target_app.unwrap_or_default(), 80),
            message: keep(
                &raw.last_assistant_message.unwrap_or_default(),
                MESSAGE_MAX_CHARS,
            ),
            failed: raw.failed,
        },
        "AppNeedsInput" => EventKind::AppNeedsInput {
            app_name: clean_text(&raw.target_app.unwrap_or_default(), 80),
        },
        "UserPromptSubmit" => {
            let full = raw.prompt.unwrap_or_default();
            EventKind::UserPromptSubmit {
                prompt_chars: full.chars().count(),
                prompt: keep(&full, PROMPT_MAX_CHARS),
                permission_mode: raw.permission_mode.map(|m| clean_text(&m, 50)),
            }
        }
        "PreToolUse" => EventKind::PreToolUse {
            tool_name,
            command,
            file_path,
            effort: raw.effort.level.map(|l| clean_text(&l, 50)),
        },
        "PermissionRequest" => EventKind::PermissionRequest {
            tool_name,
            command,
            file_path,
        },
        "PermissionDenied" => EventKind::PermissionDenied,
        "Notification" => EventKind::Notification {
            notification_type: clean_text(&raw.notification_type.unwrap_or_default(), 100),
        },
        "Elicitation" => EventKind::Elicitation {
            mcp_server_name: clean_text(&raw.mcp_server_name.unwrap_or_default(), 100),
        },
        "SubagentStart" => EventKind::SubagentStart,
        "SubagentStop" => EventKind::SubagentStop,
        "Stop" => EventKind::Stop {
            stop_hook_active: raw.stop_hook_active,
            last_assistant_message: keep(
                &raw.last_assistant_message.unwrap_or_default(),
                MESSAGE_MAX_CHARS,
            ),
            background_tasks: u32::try_from(raw.background_tasks).unwrap_or(u32::MAX),
        },
        "StopFailure" => EventKind::StopFailure {
            error: keep(&raw.error.unwrap_or_default(), 200),
        },
        "SessionEnd" => EventKind::SessionEnd,
        _ => return Err(ParseError::UnknownEvent(clean_text(&name, 50))),
    };
    let cwd = clean_text(&raw.cwd.unwrap_or_default(), PATH_MAX_CHARS);
    Ok(AgentEvent {
        session_id: clean_text(&session_id, 100),
        prompt_id: raw.prompt_id.map(|p| clean_text(&p, 100)),
        project: project_key(salt, &cwd),
        cwd,
        kind,
    })
}

/// A salted hash of the folder, so run history can group by project without storing paths.
pub fn project_key(salt: &str, cwd: &str) -> String {
    let digest = Sha256::new()
        .chain_update(salt.as_bytes())
        .chain_update([0])
        .chain_update(cwd.as_bytes())
        .finalize();
    let mut out = String::with_capacity(16);
    for b in &digest[..8] {
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(json: &str) -> Result<AgentEvent, ParseError> {
        parse(json.as_bytes(), "salt")
    }

    #[test]
    fn prompt_is_truncated_but_its_length_is_kept() {
        let long = "x".repeat(900);
        let ev = p(&format!(
            r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s","prompt":"{long}"}}"#
        ))
        .unwrap();
        match ev.kind {
            EventKind::UserPromptSubmit {
                prompt,
                prompt_chars,
                ..
            } => {
                assert_eq!(prompt.chars().count(), PROMPT_MAX_CHARS);
                assert_eq!(prompt_chars, 900);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn secrets_are_redacted_before_anything_is_stored() {
        let ev = p(r#"{"hook_event_name":"UserPromptSubmit","session_id":"s","prompt":"deploy with API_TOKEN=hunter2 and sk-abcdefghijklmnopqrstuvwxyz0123"}"#).unwrap();
        match ev.kind {
            EventKind::UserPromptSubmit { prompt, .. } => {
                assert!(!prompt.contains("hunter2") && !prompt.contains("sk-abcdef"));
                assert!(prompt.contains("[redacted]"));
            }
            other => panic!("{other:?}"),
        }
        let cmd = p(r#"{"hook_event_name":"PermissionRequest","session_id":"s","tool_name":"Bash","tool_input":{"command":"curl -H 'Authorization: Bearer s3cr3t' x"}}"#).unwrap();
        match cmd.kind {
            EventKind::PermissionRequest { command, .. } => {
                assert!(!command.unwrap().contains("s3cr3t"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_key_that_straddles_the_cut_is_still_redacted() {
        let key = "sk-abcdefghijklmnopqrstuvwxyz0123456789";
        let before = PROMPT_MAX_CHARS - 10;
        let prompt = format!("{} {key} tail", "x".repeat(before - 1));
        let body = format!(
            r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s","prompt":"{prompt}"}}"#
        );
        let ev = p(&body).unwrap();
        match ev.kind {
            EventKind::UserPromptSubmit { prompt, .. } => {
                assert!(!prompt.contains("sk-"), "half a key survived: {prompt}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ansi_and_control_characters_are_stripped() {
        let ev = p(r#"{"hook_event_name":"Stop","session_id":"s","last_assistant_message":"\u001b[31mred\u001b[0m\u0007 done\nok","background_tasks":[1,2]}"#).unwrap();
        match ev.kind {
            EventKind::Stop {
                last_assistant_message,
                background_tasks,
                ..
            } => {
                assert_eq!(last_assistant_message, "red done ok");
                assert_eq!(background_tasks, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unexpected_field_types_read_as_absent() {
        let ev = p(r#"{"hook_event_name":"PreToolUse","session_id":"s","tool_name":"Bash","tool_input":"just a string","effort":7,"prompt_id":5}"#).unwrap();
        assert_eq!(ev.prompt_id, None);
        assert_eq!(
            ev.kind,
            EventKind::PreToolUse {
                tool_name: "Bash".into(),
                command: None,
                file_path: None,
                effort: None
            }
        );
    }

    #[test]
    fn only_listed_tool_input_fields_are_read() {
        let ev = p(r#"{"hook_event_name":"PreToolUse","session_id":"s","tool_name":"Write","tool_input":{"file_path":"/a/b.rs","content":"SECRET","command":"x"}}"#).unwrap();
        assert!(
            matches!(ev.kind, EventKind::PreToolUse { file_path: Some(ref f), .. } if f == "/a/b.rs")
        );
    }

    #[test]
    fn bad_input_is_classified() {
        assert_eq!(p("not json"), Err(ParseError::Malformed));
        assert_eq!(p("[]"), Err(ParseError::Malformed));
        assert_eq!(
            p(r#"{"hook_event_name":"Stop"}"#),
            Err(ParseError::Malformed)
        );
        assert_eq!(
            p(r#"{"hook_event_name":"PostToolUse","session_id":"s"}"#),
            Err(ParseError::UnknownEvent("PostToolUse".into()))
        );
    }

    #[test]
    fn deep_nesting_cannot_crash_the_parser() {
        let deep = |field: &str| {
            format!(
                r#"{{"hook_event_name":"Stop","session_id":"s","{field}":{}{}}}"#,
                "[".repeat(100_000),
                "]".repeat(100_000)
            )
        };
        // An unknown field is skipped without recursion, so the event still parses.
        assert!(p(&deep("unknown")).is_ok());
        // A known field is read through serde's recursion limit and refused.
        assert_eq!(
            p(&deep("last_assistant_message")),
            Err(ParseError::Malformed)
        );
    }

    #[test]
    fn project_key_depends_on_salt_and_folder() {
        assert_eq!(project_key("a", "/x"), project_key("a", "/x"));
        assert_ne!(project_key("a", "/x"), project_key("b", "/x"));
        assert_ne!(project_key("a", "/x"), project_key("a", "/y"));
        assert_eq!(project_key("a", "/x").len(), 16);
    }
}
