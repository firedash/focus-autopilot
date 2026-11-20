// SPDX-License-Identifier: Apache-2.0
//! Antigravity attention hooks with passive replies. No transcript reads or agent control.
//! Conversation and model-call starts are activity, not verified user submissions.
//! No home window is captured. See docs/antigravity.md for the remaining work.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

const OWNER: &str = "focus-autopilot";
const EVENTS: &[&str] = &["SessionStart", "PreInvocation", "PreToolUse", "Stop"];
const MAX_INPUT: u64 = 65_536;
const TIMEOUT: Duration = Duration::from_millis(400);

pub fn passive_reply(event: &str) -> Value {
    match event {
        "Stop" => json!({"decision": "stop"}),
        // allow delegates to the remaining hooks/default policy, unlike auto_approve.
        "PreToolUse" => json!({"decision": "allow"}),
        _ => json!({}),
    }
}

/// Never embed the token in another program's settings or a process argument.
pub fn command(exe: &Path, config: &Path, event: &str) -> Result<String, String> {
    Ok(format!(
        "{} antigravity-hook --event {event} --config {}",
        shell_path(exe)?,
        shell_path(config)?
    ))
}

fn shell_path(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or("hook paths must be UTF-8")?;
    if text.chars().any(char::is_control) {
        return Err("hook paths must not contain control characters".into());
    }
    if cfg!(windows) {
        // cmd expands these even inside quotes. Refuse instead of making a command
        // that works for ordinary paths but means something else for unusual ones.
        if text.contains(['"', '%', '!']) {
            return Err("Windows hook paths must not contain quotes, % or !".into());
        }
        Ok(format!("\"{text}\""))
    } else {
        Ok(format!("'{}'", text.replace('\'', "'\"'\"'")))
    }
}

pub fn install(value: &mut Value, exe: &Path, config: &Path) -> Result<(), String> {
    let root = value
        .as_object_mut()
        .ok_or("hooks.json must be an object")?;
    if let Some(existing) = root.get(OWNER) {
        // Do not silently replace someone else's bundle sharing our friendly name.
        if !is_ours(existing) {
            return Err("a non-managed focus-autopilot hook bundle already exists".into());
        }
    }
    let enabled = root.get(OWNER).and_then(|v| v.get("enabled")).cloned();
    let mut bundle = serde_json::Map::new();
    if let Some(enabled) = enabled {
        bundle.insert("enabled".into(), enabled);
    }
    for event in EVENTS {
        let handler =
            json!({"type":"command", "command":command(exe, config, event)?, "timeout":2});
        let handlers = if *event == "PreToolUse" {
            json!([{"matcher":".*", "hooks":[handler]}])
        } else {
            json!([handler])
        };
        bundle.insert((*event).into(), handlers);
    }
    root.insert(OWNER.into(), Value::Object(bundle));
    Ok(())
}

fn is_ours(bundle: &Value) -> bool {
    let Some(map) = bundle.as_object() else {
        return false;
    };
    // Ownership comes from our exact command-hook shape, not undocumented fields.
    map.keys()
        .all(|key| EVENTS.contains(&key.as_str()) || key == "enabled")
        && EVENTS.iter().all(|event| {
            let Some(entries) = bundle.get(*event).and_then(Value::as_array) else {
                return false;
            };
            if entries.len() != 1 {
                return false;
            }
            let handler = if *event == "PreToolUse" {
                if entries[0].as_object().is_none_or(|g| g.len() != 2) {
                    return false;
                }
                if entries[0].get("matcher").and_then(Value::as_str) != Some(".*") {
                    return false;
                }
                let Some(hooks) = entries[0].get("hooks").and_then(Value::as_array) else {
                    return false;
                };
                if hooks.len() != 1 {
                    return false;
                }
                &hooks[0]
            } else {
                &entries[0]
            };
            handler.as_object().is_some_and(|h| {
                h.len() == 3
                    && h.get("type").and_then(Value::as_str) == Some("command")
                    && h.get("timeout").and_then(Value::as_u64) == Some(2)
                    && h.get("command").and_then(Value::as_str).is_some_and(|c| {
                        c.contains(&format!(" antigravity-hook --event {event} --config "))
                    })
            })
        })
}

pub fn uninstall(value: &mut Value) -> Result<(), String> {
    let root = value
        .as_object_mut()
        .ok_or("hooks.json must be an object")?;
    if root.get(OWNER).is_some_and(is_ours) {
        root.remove(OWNER);
    }
    Ok(())
}

/// Translate only documented fields. The receiver applies its usual redaction,
/// string limits and project hashing after this conversion.
pub fn normalize(event: &str, raw: &Value) -> Result<Value, String> {
    if !EVENTS.contains(&event) || !raw.is_object() {
        return Err("unsupported Antigravity event or non-object payload".into());
    }
    let id = raw
        .get("conversationId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .ok_or("missing or overlong conversationId")?;
    let paths = raw.get("workspacePaths").and_then(Value::as_array);
    // No arbitrary first-folder choice when several workspaces are mounted.
    let cwd = paths
        .filter(|p| p.len() == 1)
        .and_then(|p| p[0].as_str())
        .unwrap_or("");
    let mut body = json!({"session_id":format!("antigravity:{id}"), "cwd":cwd});
    match event {
        "SessionStart" | "PreInvocation" => body["hook_event_name"] = json!("AgentActivity"),
        "PreToolUse" => {
            let tool = raw
                .pointer("/toolCall/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            if matches!(tool, "ask_question" | "ask_permission") {
                body["hook_event_name"] = json!("AppNeedsInput");
                body["target_app"] = json!("Antigravity");
            } else {
                body["hook_event_name"] = json!("PreToolUse");
                body["tool_name"] = json!(tool);
                let args = raw.pointer("/toolCall/args");
                body["tool_input"] = json!({
                    "command":args.and_then(|a| a.get("CommandLine")).and_then(Value::as_str),
                    "file_path":args.and_then(|a| a.get("TargetFile").or_else(|| a.get("AbsolutePath"))).and_then(Value::as_str)
                });
            }
        }
        "Stop" => {
            let reason = raw
                .get("terminationReason")
                .and_then(Value::as_str)
                .unwrap_or("");
            // Cancellation is a choice to stop, not a request for attention. The
            // official schema is open-ended, so unknown reasons without an error
            // are quiet too. SessionEnd withdraws any pending return/input state;
            // the next event can adopt this conversation again without a home.
            let cancelled = matches!(
                reason.to_ascii_lowercase().as_str(),
                "cancelled"
                    | "canceled"
                    | "user_cancelled"
                    | "user_canceled"
                    | "user_cancel"
                    | "user_stop"
                    | "user_stopped"
                    | "interrupted"
                    | "abort"
                    | "aborted"
            );
            let has_error = !raw
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty();
            let known = matches!(
                reason,
                "model_stop" | "NO_TOOL_CALL" | "error" | "max_steps_exceeded"
            );
            if cancelled || (!known && !has_error) {
                body["hook_event_name"] = json!("SessionEnd");
                return Ok(body);
            }
            let normal = matches!(reason, "model_stop" | "NO_TOOL_CALL")
                && raw
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty();
            body["hook_event_name"] = json!("AppFinished");
            body["target_app"] = json!("Antigravity");
            body["failed"] = json!(!normal);
            body["last_assistant_message"] = json!(if !normal {
                "Agent stopped without normal completion"
            } else if raw.get("fullyIdle").and_then(Value::as_bool) == Some(true) {
                "Task finished"
            } else {
                "Task finished; other work may still be running"
            });
        }
        _ => return Err("unsupported Antigravity event".into()),
    }
    Ok(body)
}

/// Bounded loopback delivery. Do not use URLs or hosts from incoming hook data.
fn deliver(port: u16, token: &str, body: &Value) -> Result<(), String> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("receiver token is missing or is not hexadecimal".into());
    }
    let payload = serde_json::to_vec(body).map_err(|_| "cannot encode hook")?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream =
        TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|_| "focusd is unavailable")?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| "cannot set read timeout")?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|_| "cannot set write timeout")?;
    let head = format!(
        "POST /hook HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nX-Focus-Token: {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&payload))
        .map_err(|_| "cannot send hook to focusd")?;
    let mut response = [0_u8; 13];
    stream
        .read_exact(&mut response)
        .map_err(|_| "focusd did not answer")?;
    if response != *b"HTTP/1.1 200 " {
        return Err("focusd did not accept the hook".into());
    }
    Ok(())
}

/// Hook failures never change Antigravity's control flow. Only generic diagnostic
/// text goes to stderr; stdout always contains the protocol reply, never logs.
pub fn run(event: &str, config: &Path) {
    let attempt = || -> Result<(), String> {
        let mut input = Vec::new();
        std::io::stdin()
            .take(MAX_INPUT + 1)
            .read_to_end(&mut input)
            .map_err(|_| "cannot read hook input")?;
        if input.len() as u64 > MAX_INPUT {
            return Err("hook input exceeded 64 KiB".into());
        }
        let raw: Value = serde_json::from_slice(&input).map_err(|_| "hook input is not JSON")?;
        let mut body = normalize(event, &raw)?;
        if body.get("target_app").is_some() {
            let text = std::fs::read_to_string(config).map_err(|_| "cannot read config")?;
            let doc = text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|_| "invalid config")?;
            if let Some(name) = doc.get("antigravity").and_then(|v| v.get("app_name")) {
                let name = name
                    .as_str()
                    .filter(|n| !n.is_empty() && n.len() <= 80 && !n.chars().any(char::is_control))
                    .ok_or("invalid Antigravity app_name")?;
                body["target_app"] = json!(name);
            }
        }
        let token = crate::config::token_of(config).ok_or("receiver token is missing")?;
        deliver(crate::config::ensure_port(config), &token, &body)
    };
    if let Err(problem) = attempt() {
        eprintln!("focusctl Antigravity: {problem}");
    }
    println!("{}", passive_reply(event));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> Value {
        json!({"conversationId":"abc", "workspacePaths":["/work/api"], "lastUserInput":"fix tests"})
    }

    #[test]
    fn neither_session_nor_model_start_invents_a_user_submission() {
        let start = normalize("SessionStart", &raw()).unwrap();
        assert_eq!(start["session_id"], "antigravity:abc");
        assert!(start.get("prompt").is_none());
        assert_eq!(start["hook_event_name"], "AgentActivity");
        for _ in 0..3 {
            assert_eq!(
                normalize("PreInvocation", &raw()).unwrap()["hook_event_name"],
                "AgentActivity"
            );
        }
    }

    #[test]
    fn normal_stops_request_app_return_without_waiting_for_helpers() {
        for reason in ["model_stop", "NO_TOOL_CALL"] {
            for idle in [json!(true), json!(false), Value::Null, json!("true")] {
                let mut r = raw();
                r["terminationReason"] = json!(reason);
                r["fullyIdle"] = idle;
                let out = normalize("Stop", &r).unwrap();
                assert_eq!(out["hook_event_name"], "AppFinished");
                assert_eq!(out["target_app"], "Antigravity");
                assert_eq!(out["failed"], false);
                assert!(
                    !out["last_assistant_message"]
                        .as_str()
                        .unwrap()
                        .contains("fix tests")
                );
            }
        }
        for reason in ["error", "max_steps_exceeded"] {
            let mut r = raw();
            r["terminationReason"] = json!(reason);
            assert_eq!(normalize("Stop", &r).unwrap()["failed"], true);
        }
        let mut r = raw();
        r["terminationReason"] = json!("model_stop");
        r["error"] = json!("failure");
        assert_eq!(normalize("Stop", &r).unwrap()["failed"], true);
    }

    #[test]
    fn cancellation_and_unclassified_stops_are_quiet_not_completion() {
        for reason in [
            "cancelled",
            "canceled",
            "user_cancelled",
            "USER_CANCELED",
            "user_stop",
            "interrupted",
            "aborted",
            "future-value",
            "",
        ] {
            let mut r = raw();
            r["terminationReason"] = json!(reason);
            assert_eq!(
                normalize("Stop", &r).unwrap()["hook_event_name"],
                "SessionEnd"
            );
        }
        let mut r = raw();
        r["terminationReason"] = json!("future-value");
        r["error"] = json!("runtime failure");
        assert_eq!(normalize("Stop", &r).unwrap()["failed"], true);
        r["terminationReason"] = json!("cancelled");
        assert_eq!(
            normalize("Stop", &r).unwrap()["hook_event_name"],
            "SessionEnd"
        );
    }

    #[test]
    fn no_transcript_reads_or_invented_answer_recaps() {
        let mut r = raw();
        r["transcriptPath"] = json!("/private/file");
        r["fullyIdle"] = json!(true);
        r["terminationReason"] = json!("model_stop");
        let body = normalize("Stop", &r).unwrap();
        assert_eq!(body["last_assistant_message"], "Task finished");
        assert!(body.get("transcriptPath").is_none());
        r["workspacePaths"] = json!(["/one", "/two"]);
        assert_eq!(normalize("Stop", &r).unwrap()["cwd"], "");
    }

    #[test]
    fn explicit_questions_and_permission_tools_request_input_return() {
        for tool in ["ask_question", "ask_permission"] {
            let mut r = raw();
            r["toolCall"] = json!({"name":tool, "args":{}});
            let out = normalize("PreToolUse", &r).unwrap();
            assert_eq!(out["hook_event_name"], "AppNeedsInput");
            assert_eq!(out["target_app"], "Antigravity");
        }
        let mut r = raw();
        r["toolCall"] = json!({"name":"run_command", "args":{"CommandLine":"echo hi", "TargetFile":"/a.rs", "other":"private"}});
        let body = normalize("PreToolUse", &r).unwrap();
        assert_eq!(body["hook_event_name"], "PreToolUse");
        assert_eq!(body["tool_input"]["command"], "echo hi");
        assert!(body["tool_input"].get("other").is_none());
    }

    #[test]
    fn invalid_identity_and_events_are_refused() {
        assert!(normalize("Stop", &json!({})).is_err());
        assert!(normalize("Stop", &json!({"conversationId":""})).is_err());
        assert!(normalize("Stop", &json!({"conversationId":"x".repeat(81)})).is_err());
        assert!(normalize("Stop", &json!([])).is_err());
        assert!(normalize("PermissionRequest", &raw()).is_err());
    }

    #[test]
    fn installation_is_idempotent_owned_and_token_free() {
        let mut value = json!({"other":{"Stop":[{"command":"mine"}]}});
        let original = value.clone();
        let exe = Path::new("/path with spaces/focusctl");
        let config = Path::new("/private/config.toml");
        install(&mut value, exe, config).unwrap();
        let once = value.clone();
        install(&mut value, exe, config).unwrap();
        assert_eq!(value, once);
        assert_eq!(value[OWNER]["Stop"][0]["timeout"], 2);
        assert!(value[OWNER].get("hooks").is_none());
        assert!(!value.to_string().contains("X-Focus-Token"));
        uninstall(&mut value).unwrap();
        assert_eq!(value, original);
        let mut conflict = json!({"focus-autopilot":{"Stop":[]}});
        assert!(install(&mut conflict, exe, config).is_err());
        let before = conflict.clone();
        uninstall(&mut conflict).unwrap();
        assert_eq!(conflict, before);
    }

    #[test]
    fn observer_replies_never_force_execution_or_approve_tools() {
        assert_eq!(passive_reply("Stop"), json!({"decision":"stop"}));
        assert_eq!(passive_reply("PreToolUse"), json!({"decision":"allow"}));
        assert_eq!(passive_reply("SessionStart"), json!({}));
        assert!(command(Path::new("bad\npath"), Path::new("config"), "Stop").is_err());
    }
}
