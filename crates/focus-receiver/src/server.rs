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
use http_body_util::{BodyExt, Empty, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST, ORIGIN};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::sync::oneshot;

use crate::check::{Head, MAX_BODY, RateLimit, Reject, TOKEN_HEADER, check_head};
use crate::payload::{ParseError, parse};

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
}

/// Binds 127.0.0.1 and serves on a background thread.
///
/// # Errors
/// When the port can't be bound (for example it is already in use) or the runtime can't start.
pub fn spawn(settings: Settings, tx: Sender<Received>) -> io::Result<Running> {
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

fn reply(status: u16) -> Response<Empty<Bytes>> {
    let mut response = Response::new(Empty::new());
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    response
}

fn header_str(req: &Request<Incoming>, name: hyper::header::HeaderName) -> Option<&str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

async fn handle(shared: &Shared, req: Request<Incoming>) -> Response<Empty<Bytes>> {
    let content_length = header_str(&req, CONTENT_LENGTH).and_then(|v| v.parse::<u64>().ok());
    let head = Head {
        method: req.method().as_str(),
        path: req.uri().path(),
        host: header_str(&req, HOST),
        origin_present: req.headers().contains_key(ORIGIN),
        token: req
            .headers()
            .get(TOKEN_HEADER)
            .map(hyper::header::HeaderValue::as_bytes),
        content_type: header_str(&req, CONTENT_TYPE),
        content_length,
    };
    if let Err(reject) = check_head(&head, shared.port, &shared.token) {
        return reply(reject.status());
    }
    let allowed = shared
        .rate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .allow(Instant::now());
    if !allowed {
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
