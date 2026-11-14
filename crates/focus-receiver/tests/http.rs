// SPDX-License-Identifier: Apache-2.0
//! Negative and positive tests against the real server over a real socket.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::format_push_string
)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use focus_core::EventKind;
use focus_receiver::{Received, Settings, spawn};

const TOKEN: &str = "test-token";
const BODY: &str = r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"p1","cwd":"/w","prompt":"hi"}"#;

struct Req<'a> {
    method: &'a str,
    path: &'a str,
    host: Option<String>,
    extra: Vec<(&'a str, &'a str)>,
    content_type: Option<&'a str>,
    token: Option<&'a str>,
    body: String,
}

fn start() -> (focus_receiver::Running, Receiver<Received>) {
    let (tx, rx) = channel();
    let running = spawn(
        Settings {
            port: 0,
            token: TOKEN.into(),
            salt: "s".into(),
        },
        tx,
    )
    .unwrap();
    (running, rx)
}

fn good(addr: SocketAddr) -> Req<'static> {
    Req {
        method: "POST",
        path: "/hook",
        host: Some(format!("127.0.0.1:{}", addr.port())),
        extra: vec![],
        content_type: Some("application/json"),
        token: Some(TOKEN),
        body: BODY.into(),
    }
}

fn send(addr: SocketAddr, req: &Req<'_>) -> u16 {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut text = format!("{} {} HTTP/1.1\r\n", req.method, req.path);
    if let Some(h) = &req.host {
        text += &format!("Host: {h}\r\n");
    }
    if let Some(t) = req.token {
        text += &format!("X-Focus-Token: {t}\r\n");
    }
    if let Some(c) = req.content_type {
        text += &format!("Content-Type: {c}\r\n");
    }
    for (k, v) in &req.extra {
        text += &format!("{k}: {v}\r\n");
    }
    text += &format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        req.body.len(),
        req.body
    );
    // The server may answer and close before a huge body is fully sent.
    let _ = s.write_all(text.as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

#[test]
fn a_good_hook_is_acknowledged_and_queued() {
    let (running, rx) = start();
    assert_eq!(send(running.addr, &good(running.addr)), 200);
    let got = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(got.event.session_id, "s1");
    assert!(matches!(got.event.kind, EventKind::UserPromptSubmit { .. }));
}

#[test]
fn forged_and_malformed_requests_are_rejected_and_nothing_is_queued() {
    let (running, rx) = start();
    let a = running.addr;
    let mut cases: Vec<(Req<'_>, u16)> = Vec::new();
    cases.push((
        Req {
            token: None,
            ..good(a)
        },
        401,
    ));
    cases.push((
        Req {
            token: Some("wrong"),
            ..good(a)
        },
        401,
    ));
    cases.push((
        Req {
            host: Some("evil.test".into()),
            ..good(a)
        },
        403,
    ));
    cases.push((
        Req {
            host: Some(format!("evil.test:{}", a.port())),
            ..good(a)
        },
        403,
    ));
    cases.push((
        Req {
            extra: vec![("Origin", "http://evil.test")],
            ..good(a)
        },
        403,
    ));
    cases.push((
        Req {
            content_type: Some("text/plain"),
            ..good(a)
        },
        415,
    ));
    cases.push((
        Req {
            path: "/nope",
            ..good(a)
        },
        404,
    ));
    cases.push((
        Req {
            method: "GET",
            ..good(a)
        },
        405,
    ));
    cases.push((
        Req {
            body: "not json".into(),
            ..good(a)
        },
        400,
    ));
    for (req, expected) in &cases {
        assert_eq!(send(a, req), *expected, "{} {}", req.method, req.path);
    }
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_body_over_8_mb_is_refused() {
    let (running, rx) = start();
    let big = Req {
        body: "x".repeat(8 * 1024 * 1024 + 1),
        ..good(running.addr)
    };
    assert_eq!(send(running.addr, &big), 413);
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn an_event_focusd_does_not_register_is_acknowledged_and_dropped() {
    let (running, rx) = start();
    let req = Req {
        body: r#"{"hook_event_name":"PostToolUse","session_id":"s"}"#.into(),
        ..good(running.addr)
    };
    assert_eq!(send(running.addr, &req), 200);
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn the_recorded_fixture_parses_event_by_event() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../focus-core/tests/fixtures/approve_then_tests.jsonl"
    );
    let text = std::fs::read_to_string(path).unwrap();
    let kinds: Vec<_> = text
        .lines()
        .map(|l| focus_receiver::parse(l.as_bytes(), "s").unwrap().kind)
        .collect();
    assert!(matches!(kinds[0], EventKind::UserPromptSubmit { .. }));
    assert!(matches!(kinds[2], EventKind::PermissionRequest { .. }));
    assert!(matches!(kinds[4], EventKind::Stop { .. }));
    assert_eq!(kinds.len(), 5);
}

#[test]
fn dropping_the_handle_stops_the_server() {
    let (running, _rx) = start();
    let addr = running.addr;
    drop(running);
    std::thread::sleep(Duration::from_millis(300));
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
}
