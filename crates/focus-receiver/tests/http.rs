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
use std::sync::{Arc, Mutex};

use focus_receiver::{Received, Settings, UiBackend, spawn, spawn_with_ui};

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
    // The server refuses on the declared Content-Length, before it reads any body.
    // Send only the head: pushing 8 MB at a server that has already answered and
    // closed makes Windows reset the connection and drop the 413 before the client
    // reads it, which is a property of the transport, not of the limit.
    let mut s = TcpStream::connect(running.addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let head = format!(
        "POST /hook HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Focus-Token: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        running.addr.port(),
        8 * 1024 * 1024 + 1
    );
    s.write_all(head.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let status: u16 = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    assert_eq!(status, 413);
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

/// A backend that records what it was asked and answers with fixed JSON.
#[derive(Default)]
struct FakeUi {
    calls: Mutex<Vec<String>>,
}

impl FakeUi {
    fn note(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl UiBackend for FakeUi {
    fn stats(&self, tz_minutes: i32) -> String {
        self.note(format!("stats tz={tz_minutes}"));
        r#"{"stats":true}"#.to_owned()
    }

    fn config(&self) -> String {
        self.note("config".to_owned());
        r#"{"config":true}"#.to_owned()
    }

    fn set_config(&self, body: &[u8]) -> Result<String, String> {
        let text = String::from_utf8_lossy(body).into_owned();
        self.note(format!("set {text}"));
        if text.contains("bad") {
            Err("that value is not allowed".to_owned())
        } else {
            Ok(r#"{"saved":true}"#.to_owned())
        }
    }

    fn clear_stats(&self, _tz_minutes: i32) -> String {
        self.note("clear".to_owned());
        r#"{"cleared":true}"#.to_owned()
    }
}

fn start_ui() -> (focus_receiver::Running, Receiver<Received>, Arc<FakeUi>) {
    let (tx, rx) = channel();
    let ui = Arc::new(FakeUi::default());
    let running = spawn_with_ui(
        Settings {
            port: 0,
            token: TOKEN.into(),
            salt: "s".into(),
        },
        tx,
        Some(ui.clone()),
    )
    .unwrap();
    (running, rx, ui)
}

/// Status, lower-cased headers and body of the reply to a raw request.
fn fetch(addr: SocketAddr, request: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let _ = s.write_all(request.as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let status = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    (status, head.to_lowercase(), body.to_owned())
}

fn get(addr: SocketAddr, path: &str, extra: &str) -> (u16, String, String) {
    fetch(
        addr,
        &format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{extra}Connection: close\r\n\r\n",
            addr.port()
        ),
    )
}

fn post(addr: SocketAddr, path: &str, extra: &str, body: &str) -> (u16, String, String) {
    fetch(
        addr,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            addr.port(),
            body.len()
        ),
    )
}

const AUTH: &str = "X-Focus-Token: test-token\r\n";
const JSON: &str = "Content-Type: application/json\r\n";

#[test]
fn the_page_is_served_with_a_strict_policy_and_no_token() {
    let (running, _rx, _ui) = start_ui();
    for path in ["/ui", "/ui/", "/ui/app.js", "/ui/style.css"] {
        let (status, head, _) = get(running.addr, path, "");
        assert_eq!(status, 200, "{path}");
        assert!(
            head.contains("content-security-policy: default-src 'none'"),
            "{path}: {head}"
        );
        assert!(head.contains("x-content-type-options: nosniff"));
        assert!(head.contains("x-frame-options: deny"));
        assert!(head.contains("cache-control: no-store"));
    }
    let (_, head, body) = get(running.addr, "/ui/", "");
    assert!(head.contains("content-type: text/html"));
    assert!(body.contains("<!doctype html>"));
    assert_eq!(get(running.addr, "/ui/nope.js", "").0, 404);
    assert_eq!(get(running.addr, "/ui/../Cargo.toml", "").0, 404);
}

#[test]
fn the_page_is_not_served_to_other_hosts_or_sites() {
    let (running, _rx, _ui) = start_ui();
    let a = running.addr;
    let foreign_host = fetch(
        a,
        "GET /ui/ HTTP/1.1\r\nHost: evil.test\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(foreign_host.0, 403);
    assert_eq!(get(a, "/ui/", "Origin: http://evil.test\r\n").0, 403);
    assert_eq!(post(a, "/ui/", "", "").0, 405);
}

#[test]
fn the_api_answers_its_own_page_with_the_token() {
    let (running, _rx, ui) = start_ui();
    let a = running.addr;
    let own = format!(
        "Origin: http://127.0.0.1:{}\r\nSec-Fetch-Site: same-origin\r\n",
        a.port()
    );
    let (status, head, body) = get(a, "/api/stats?tz=330", &format!("{AUTH}{own}"));
    assert_eq!((status, body.as_str()), (200, r#"{"stats":true}"#));
    assert!(head.contains("content-type: application/json"));
    assert!(head.contains("cache-control: no-store"));
    assert_eq!(get(a, "/api/config", AUTH).2, r#"{"config":true}"#);
    let saved = post(
        a,
        "/api/config",
        &format!("{AUTH}{JSON}{own}"),
        r#"{"values":{"enabled":false}}"#,
    );
    assert_eq!((saved.0, saved.2.as_str()), (200, r#"{"saved":true}"#));
    assert_eq!(
        post(a, "/api/stats/clear", &format!("{AUTH}{JSON}"), "{}").2,
        r#"{"cleared":true}"#
    );
    assert_eq!(
        *ui.calls.lock().unwrap(),
        [
            "stats tz=330",
            "config",
            r#"set {"values":{"enabled":false}}"#,
            "clear"
        ]
    );
}

#[test]
fn a_refused_setting_comes_back_as_a_readable_error() {
    let (running, _rx, _ui) = start_ui();
    let (status, _, body) = post(
        running.addr,
        "/api/config",
        &format!("{AUTH}{JSON}"),
        r#"{"values":"bad"}"#,
    );
    assert_eq!(status, 400);
    assert_eq!(body, r#"{"error":"that value is not allowed"}"#);
}

#[test]
fn the_api_refuses_strangers_and_never_reaches_the_backend() {
    let (running, _rx, ui) = start_ui();
    let a = running.addr;
    let evil = "Origin: http://evil.test\r\n";
    let cases = [
        (get(a, "/api/stats", "").0, 401, "no token"),
        (get(a, "/api/stats", "X-Focus-Token: wrong\r\n").0, 401, "wrong token"),
        (get(a, "/api/stats", &format!("{AUTH}{evil}")).0, 403, "another site, even with the token"),
        (get(a, "/api/config", &format!("{AUTH}Sec-Fetch-Site: cross-site\r\n")).0, 403, "cross-site fetch"),
        (post(a, "/api/config", &format!("{AUTH}{JSON}{evil}"), "{}").0, 403, "cross-site write"),
        (post(a, "/api/config", &format!("{AUTH}Content-Type: text/plain\r\n"), "{}").0, 415, "a form-style write"),
        (post(a, "/api/config", AUTH, "{}").0, 415, "a write with no content type"),
        (
            fetch(a, &format!("GET /api/stats HTTP/1.1\r\nHost: evil.test:{}\r\n{AUTH}Connection: close\r\n\r\n", a.port())).0,
            403,
            "a rebound host name",
        ),
        (post(a, "/api/config", &format!("{AUTH}{JSON}"), &"x".repeat(70_000)).0, 413, "an oversized write"),
    ];
    for (status, expected, what) in cases {
        assert_eq!(status, expected, "{what}");
    }
    assert!(
        ui.calls.lock().unwrap().is_empty(),
        "the backend was reached"
    );
    // Known caller, unknown endpoint or method for it.
    assert_eq!(get(a, "/api/nope", AUTH).0, 404);
    assert_eq!(get(a, "/api/stats/clear", AUTH).0, 404);
    assert_eq!(post(a, "/api/stats", &format!("{AUTH}{JSON}"), "{}").0, 404);
}

#[test]
fn hooks_work_as_before_with_the_page_enabled() {
    let (running, rx, _ui) = start_ui();
    assert_eq!(send(running.addr, &good(running.addr)), 200);
    assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok());
    // A hook with an Origin is still refused, even this server's own.
    let own = format!("http://127.0.0.1:{}", running.addr.port());
    let from_page = Req {
        extra: vec![("Origin", own.as_str())],
        ..good(running.addr)
    };
    assert_eq!(send(running.addr, &from_page), 403);
}

#[test]
fn without_a_backend_there_is_no_page_and_no_api() {
    let (running, _rx) = start();
    assert_eq!(get(running.addr, "/ui/", "").0, 404);
    assert_eq!(get(running.addr, "/api/stats", AUTH).0, 404);
}
