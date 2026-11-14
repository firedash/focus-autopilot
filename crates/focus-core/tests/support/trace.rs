// SPDX-License-Identifier: Apache-2.0
//! Loads a recorded trace: one hook payload per line, in the format `focusctl record` writes.
//! `_t_ms` is the receive time in milliseconds since the trace started.

use std::path::Path;

use focus_core::{AgentEvent, EventKind};

use super::json::{Json, parse};

pub fn load(name: &str) -> Vec<(u64, AgentEvent)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path).expect("fixture should be readable");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| event(&parse(line)))
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn event(payload: &Json) -> (u64, AgentEvent) {
    let at = payload.number("_t_ms").expect("_t_ms") as u64;
    let input = payload.get("tool_input").cloned().unwrap_or(Json::Null);
    let tool_name = payload.text("tool_name").unwrap_or_default();
    let name = payload.text("hook_event_name").expect("hook_event_name");
    let kind = match name.as_str() {
        "UserPromptSubmit" => {
            let prompt = payload.text("prompt").unwrap_or_default();
            EventKind::UserPromptSubmit {
                prompt_chars: prompt.chars().count(),
                prompt,
                permission_mode: payload.text("permission_mode"),
            }
        }
        "PreToolUse" => EventKind::PreToolUse {
            tool_name,
            command: input.text("command"),
            file_path: input.text("file_path"),
            effort: payload
                .get("effort")
                .and_then(|effort| effort.text("level")),
        },
        "PermissionRequest" => EventKind::PermissionRequest {
            tool_name,
            command: input.text("command"),
            file_path: input.text("file_path"),
        },
        "PermissionDenied" => EventKind::PermissionDenied,
        "Notification" => EventKind::Notification {
            notification_type: payload.text("notification_type").unwrap_or_default(),
        },
        "Elicitation" => EventKind::Elicitation {
            mcp_server_name: payload.text("mcp_server_name").unwrap_or_default(),
        },
        "SubagentStart" => EventKind::SubagentStart,
        "SubagentStop" => EventKind::SubagentStop,
        "Stop" => EventKind::Stop {
            stop_hook_active: payload.flag("stop_hook_active"),
            last_assistant_message: payload.text("last_assistant_message").unwrap_or_default(),
            background_tasks: payload.len("background_tasks") as u32,
        },
        "StopFailure" => EventKind::StopFailure {
            error: payload.text("error").unwrap_or_default(),
        },
        "SessionEnd" => EventKind::SessionEnd,
        other => panic!("unknown hook event {other}"),
    };
    let event = AgentEvent {
        session_id: payload.text("session_id").expect("session_id"),
        prompt_id: payload.text("prompt_id"),
        cwd: payload.text("cwd").unwrap_or_default(),
        project: "proj-a".to_owned(),
        kind,
    };
    (at, event)
}
