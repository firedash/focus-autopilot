// SPDX-License-Identifier: Apache-2.0
//! Execute the real command bridge against the real loopback receiver.
#![allow(clippy::unwrap_used)]

use std::io::Write as _;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use focus_core::EventKind;
use serde_json::{Value, json};

fn hook(event: &str, input: &[u8], config: &std::path::Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_focusctl"))
        .args(["antigravity-hook", "--event", event, "--config"])
        .arg(config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn native_payload_reaches_receiver_with_existing_redaction() {
    let (tx, rx) = mpsc::channel();
    let token = "a".repeat(64);
    let listener = focus_receiver::spawn(
        focus_receiver::Settings {
            port: 0,
            token: token.clone(),
            salt: "fixture".into(),
        },
        tx,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[receiver]\nport = {}\ntoken = '{token}'\n",
            listener.addr.port()
        ),
    )
    .unwrap();
    let raw = json!({"conversationId":"fixture", "workspacePaths":["/work/api"], "lastUserInput":"deploy API_TOKEN=hunter2", "transcriptPath":"/not/read"});
    let out = hook("SessionStart", &serde_json::to_vec(&raw).unwrap(), &config);
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        json!({})
    );
    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(received.event.session_id, "antigravity:fixture");
    assert_eq!(received.event.kind, EventKind::AgentActivity);
    let tool = json!({"conversationId":"fixture", "workspacePaths":["/work/api"],
        "toolCall":{"name":"run_command", "args":{"CommandLine":"deploy API_TOKEN=hunter2"}}});
    hook("PreToolUse", &serde_json::to_vec(&tool).unwrap(), &config);
    match rx.recv_timeout(Duration::from_secs(2)).unwrap().event.kind {
        EventKind::PreToolUse {
            command: Some(command),
            ..
        } => {
            assert!(!command.contains("hunter2"));
            assert!(command.contains("[redacted]"));
        }
        other => assert!(matches!(other, EventKind::PreToolUse { .. })),
    }
    let mut stop = raw;
    stop["fullyIdle"] = json!(false);
    stop["terminationReason"] = json!("model_stop");
    let out = hook("Stop", &serde_json::to_vec(&stop).unwrap(), &config);
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        json!({"decision":"stop"})
    );
    let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        first.event.kind,
        EventKind::AppFinished { failed: false, .. }
    ));
    stop["fullyIdle"] = json!(true);
    hook("Stop", &serde_json::to_vec(&stop).unwrap(), &config);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap().event.kind,
        EventKind::AppFinished { failed: false, .. }
    ));
}

#[test]
fn malformed_oversized_or_unconfigured_hooks_are_passive_and_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("absent.toml");
    for event in ["Stop", "PreToolUse", "PreInvocation", "SessionStart"] {
        for input in [
            b"not-json-private-sentinel".to_vec(),
            vec![b'x'; 65_537],
            b"{}".to_vec(),
            br#"{"conversationId":"s"}"#.to_vec(),
        ] {
            let out = hook(event, &input, &config);
            assert!(out.status.success(), "{event}");
            let expected = match event {
                "Stop" => json!({"decision":"stop"}),
                "PreToolUse" => json!({"decision":"allow"}),
                _ => json!({}),
            };
            assert_eq!(
                serde_json::from_slice::<Value>(&out.stdout).unwrap(),
                expected
            );
            assert!(!String::from_utf8_lossy(&out.stderr).contains("private-sentinel"));
        }
    }
    assert!(!config.exists());
}

#[test]
fn install_dry_run_and_uninstall_need_no_claude_changes() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("hooks.json");
    let config = dir.path().join("config.toml");
    let original = "{\n  \"other\": {\"Stop\": [{\"command\": \"keep-me\"}]}\n}\n";
    std::fs::write(&settings, original).unwrap();
    let run = |command: &str, dry: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_focusctl"));
        cmd.args([command, "--settings"])
            .arg(&settings)
            .arg("--config")
            .arg(&config);
        if dry {
            cmd.arg("--dry-run");
        }
        cmd.output().unwrap()
    };
    assert!(run("install-antigravity-hooks", true).status.success());
    assert!(!config.exists());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    assert!(run("install-antigravity-hooks", false).status.success());
    let installed = std::fs::read_to_string(&settings).unwrap();
    assert!(!installed.contains("X-Focus-Token"));
    let token_text = std::fs::read_to_string(&config).unwrap();
    assert!(!installed.contains(&token_text));
    assert!(run("install-antigravity-hooks", false).status.success());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), installed);
    assert!(run("uninstall-antigravity-hooks", false).status.success());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
}
