// SPDX-License-Identifier: Apache-2.0
//! The loopback HTTP server. It binds 127.0.0.1 only, replies `200` with an empty body as soon
//! as a request passes the checks, and queues the parsed event. It never blocks, vetoes or
//! changes anything Claude Code does. Security-relevant: changes need a maintainer review.

use std::convert::Infallible;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener as StdListener};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use focus_core::AgentEvent;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST, ORIGIN};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::sync::oneshot;

use crate::check::{
    Head, MAX_API_BODY, MAX_BODY, RateLimit, Reject, TOKEN_HEADER, check_api, check_head,
    check_page,
};
use crate::payload::{ParseError, parse};
use crate::ui::{self, UiBackend};

#[derive(Debug, Clone)]
pub struct Settings {
    /// `0` picks a free port, for tests.
    pub port: u16,
    pub token: String,
    /// Salt for project hashes.
    pub salt: String,
}

/// A parsed event with the monotonic time it arrived; payloads carry no timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub at: Instant,
    pub event: AgentEvent,
}

/// A running receiver. Dropping it stops the server.
pub struct Running {
    pub addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

struct Shared {
    port: u16,
    token: String,
    salt: String,
    tx: Sender<Received>,
    rate: Mutex<RateLimit>,
    /// The statistics and settings page, when the shell provides one.
    ui: Option<Arc<dyn UiBackend>>,
}

/// Binds 127.0.0.1 and serves on a background thread.
///
/// # Errors
/// When the port can't be bound (for example it is already in use) or the runtime can't start.
pub fn spawn(settings: Settings, tx: Sender<Received>) -> io::Result<Running> {
    spawn_with_ui(settings, tx, None)
}

/// Like [`spawn`], and also serves the statistics and settings page backed by `ui`.
///
/// # Errors
/// When the port can't be bound or the runtime can't start.
pub fn spawn_with_ui(
    settings: Settings,
    tx: Sender<Received>,
    ui: Option<Arc<dyn UiBackend>>,
) -> io::Result<Running> {
    let std_listener = StdListener::bind((Ipv4Addr::LOCALHOST, settings.port))?;
    std_listener.set_nonblocking(true)?;
    let addr = std_listener.local_addr()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let shared = Arc::new(Shared {
        port: addr.port(),
        token: settings.token,
        salt: settings.salt,
        tx,
        rate: Mutex::new(RateLimit::new(Instant::now())),
        ui,
    });
    let (stop, stopped) = oneshot::channel();
    std::thread::Builder::new()
        .name("focus-receiver".into())
        .spawn(move || {
            runtime.block_on(async move {
                if let Ok(listener) = tokio::net::TcpListener::from_std(std_listener) {
                    accept_loop(listener, shared, stopped).await;
                }
            });
        })?;
    Ok(Running {
        addr,
        stop: Some(stop),
    })
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    shared: Arc<Shared>,
    mut stopped: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut stopped => return,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    let service = service_fn(move |req| {
                        let shared = Arc::clone(&shared);
                        async move { Ok::<_, Infallible>(handle(&shared, req).await) }
                    });
                    // A slow or silent client gets cut off; it must never pile up.
                    let conn = hyper::server::conn::http1::Builder::new()
                        .header_read_timeout(std::time::Duration::from_secs(5))
                        .timer(hyper_util::rt::TokioTimer::new())
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(stream), service);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), conn).await;
                });
            }
        }
    }
}

type Reply = Response<Full<Bytes>>;

fn reply(status: u16) -> Reply {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    response
}

/// A reply with a body. Nothing here may be cached, sniffed as another type, or framed.
fn reply_with(status: u16, content_type: &'static str, body: impl Into<Bytes>) -> Reply {
    let mut response = Response::new(Full::new(body.into()));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    let headers = response.headers_mut();
    for (name, value) in [
        ("content-type", content_type),
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", ui::CONTENT_SECURITY_POLICY),
    ] {
        headers.insert(name, hyper::header::HeaderValue::from_static(value));
    }
    response
}

fn json(status: u16, body: String) -> Reply {
    reply_with(status, "application/json; charset=utf-8", body)
}

fn header_str(req: &Request<Incoming>, name: hyper::header::HeaderName) -> Option<&str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

fn rate_ok(shared: &Shared) -> bool {
    shared
        .rate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .allow(Instant::now())
}

async fn handle(shared: &Shared, req: Request<Incoming>) -> Reply {
    let content_length = header_str(&req, CONTENT_LENGTH).and_then(|v| v.parse::<u64>().ok());
    let head = Head {
        method: req.method().as_str(),
        path: req.uri().path(),
        host: header_str(&req, HOST),
        origin_present: req.headers().contains_key(ORIGIN),
        origin: header_str(&req, ORIGIN),
        fetch_site: req
            .headers()
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok()),
        token: req
            .headers()
            .get(TOKEN_HEADER)
            .map(hyper::header::HeaderValue::as_bytes),
        content_type: header_str(&req, CONTENT_TYPE),
        content_length,
    };
    let path = head.path;
    if let Some(backend) = &shared.ui {
        if path == "/ui" || path.starts_with("/ui/") {
            return match (check_page(&head, shared.port), ui::asset(path)) {
                (Err(reject), _) => reply(reject.status()),
                (Ok(()), None) => reply(Reject::NotFound.status()),
                (Ok(()), Some(asset)) => reply_with(200, asset.content_type, asset.body),
            };
        }
        if path.starts_with("/api/") {
            if let Err(reject) = check_api(&head, shared.port, &shared.token) {
                return reply(reject.status());
            }
            if !rate_ok(shared) {
                return reply(Reject::RateLimited.status());
            }
            return api(Arc::clone(backend), req).await;
        }
    }
    if let Err(reject) = check_head(&head, shared.port, &shared.token) {
        return reply(reject.status());
    }
    if !rate_ok(shared) {
        return reply(Reject::RateLimited.status());
    }
    // `Limited` also stops a chunked body that lies about, or omits, its length.
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return reply(Reject::TooLarge.status()),
    };
    let at = Instant::now();
    match parse(&body, &shared.salt) {
        Ok(event) => {
            let _ = shared.tx.send(Received { at, event });
            reply(200)
        }
        // An event we don't register: acknowledge so Claude Code sees success.
        Err(ParseError::UnknownEvent(_)) => reply(200),
        Err(ParseError::Malformed) => reply(400),
    }
}

/// One API call, already checked. The backend reads and writes files, so it runs off the
/// server's thread: a hook arriving meanwhile is still answered at once.
async fn api(backend: Arc<dyn UiBackend>, req: Request<Incoming>) -> Reply {
    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    let tz = ui::tz_minutes(req.uri().query());
    let body = if method == "POST" {
        match Limited::new(req.into_body(), MAX_API_BODY).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return reply(Reject::TooLarge.status()),
        }
    } else {
        Bytes::new()
    };
    let work = tokio::task::spawn_blocking(move || match (method.as_str(), path.as_str()) {
        ("GET", "/api/stats") => Some(Ok(backend.stats(tz))),
        ("GET", "/api/config") => Some(Ok(backend.config())),
        ("POST", "/api/config") => Some(backend.set_config(&body)),
        ("POST", "/api/stats/clear") => Some(Ok(backend.clear_stats(tz))),
        _ => None,
    });
    match work.await {
        Ok(Some(Ok(body))) => json(200, body),
        Ok(Some(Err(message))) => json(400, serde_json::json!({ "error": message }).to_string()),
        Ok(None) => reply(Reject::NotFound.status()),
        Err(_) => reply(500),
    }
}
