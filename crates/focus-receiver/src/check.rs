// SPDX-License-Identifier: Apache-2.0
//! The request checks from the design's threat model, as pure functions so each one has a
//! negative test. Security-relevant: changes need a maintainer review.

use std::time::Instant;

use subtle::ConstantTimeEq;

pub const TOKEN_HEADER: &str = "x-focus-token";
pub const HOOK_PATH: &str = "/hook";
pub const MAX_BODY: usize = 8 * 1024 * 1024;
/// Token bucket: sustained requests per second and burst size.
const RATE: f64 = 200.0;
const BURST: f64 = 1_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    NotFound,
    MethodNotAllowed,
    /// Host header isn't `127.0.0.1:<port>` or `localhost:<port>`: blocks DNS rebinding.
    ForeignHost,
    /// Any Origin header: browsers send one, Claude Code doesn't.
    HasOrigin,
    BadToken,
    NotJson,
    TooLarge,
    RateLimited,
}

impl Reject {
    pub fn status(self) -> u16 {
        match self {
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::ForeignHost | Self::HasOrigin => 403,
            Self::BadToken => 401,
            Self::NotJson => 415,
            Self::TooLarge => 413,
            Self::RateLimited => 429,
        }
    }
}

/// The parts of a request the checks read.
pub struct Head<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub host: Option<&'a str>,
    pub origin_present: bool,
    pub token: Option<&'a [u8]>,
    pub content_type: Option<&'a str>,
    pub content_length: Option<u64>,
}

/// Runs every header-level check, cheapest and most telling first.
///
/// # Errors
/// The first check the request fails.
pub fn check_head(head: &Head<'_>, port: u16, expected_token: &str) -> Result<(), Reject> {
    if head.path != HOOK_PATH {
        return Err(Reject::NotFound);
    }
    if head.method != "POST" {
        return Err(Reject::MethodNotAllowed);
    }
    if !host_ok(head.host, port) {
        return Err(Reject::ForeignHost);
    }
    if head.origin_present {
        return Err(Reject::HasOrigin);
    }
    if !token_ok(head.token, expected_token) {
        return Err(Reject::BadToken);
    }
    if !is_json(head.content_type) {
        return Err(Reject::NotJson);
    }
    if head.content_length.is_some_and(|n| n > MAX_BODY as u64) {
        return Err(Reject::TooLarge);
    }
    Ok(())
}

fn host_ok(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else { return false };
    let host = host.to_ascii_lowercase();
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// Constant time in the token's content. An empty expected token never matches, so a config
/// without a token accepts nothing.
fn token_ok(given: Option<&[u8]>, expected: &str) -> bool {
    match given {
        Some(given) if !expected.is_empty() => given.ct_eq(expected.as_bytes()).into(),
        _ => false,
    }
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|v| v.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

/// A token bucket with the design's limits.
#[derive(Debug)]
pub struct RateLimit {
    tokens: f64,
    last: Instant,
}

impl RateLimit {
    pub fn new(now: Instant) -> Self {
        Self {
            tokens: BURST,
            last: now,
        }
    }

    pub fn allow(&mut self, now: Instant) -> bool {
        let gained = now.saturating_duration_since(self.last).as_secs_f64() * RATE;
        self.tokens = (self.tokens + gained).min(BURST);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn good() -> Head<'static> {
        Head {
            method: "POST",
            path: "/hook",
            host: Some("127.0.0.1:47821"),
            origin_present: false,
            token: Some(b"secret"),
            content_type: Some("application/json"),
            content_length: Some(10),
        }
    }

    fn run(head: &Head<'_>) -> Result<(), Reject> {
        check_head(head, 47821, "secret")
    }

    #[test]
    fn a_proper_request_passes() {
        assert_eq!(run(&good()), Ok(()));
        let local = Head {
            host: Some("LocalHost:47821"),
            content_type: Some("application/json; charset=utf-8"),
            ..good()
        };
        assert_eq!(run(&local), Ok(()));
    }

    #[test]
    fn each_forgery_is_rejected() {
        let cases = [
            (
                Head {
                    path: "/other",
                    ..good()
                },
                Reject::NotFound,
            ),
            (
                Head {
                    method: "GET",
                    ..good()
                },
                Reject::MethodNotAllowed,
            ),
            (
                Head {
                    host: Some("evil.test:47821"),
                    ..good()
                },
                Reject::ForeignHost,
            ),
            (
                Head {
                    host: Some("127.0.0.1:1"),
                    ..good()
                },
                Reject::ForeignHost,
            ),
            (
                Head {
                    host: None,
                    ..good()
                },
                Reject::ForeignHost,
            ),
            (
                Head {
                    origin_present: true,
                    ..good()
                },
                Reject::HasOrigin,
            ),
            (
                Head {
                    token: Some(b"wrong"),
                    ..good()
                },
                Reject::BadToken,
            ),
            (
                Head {
                    token: Some(b"secre"),
                    ..good()
                },
                Reject::BadToken,
            ),
            (
                Head {
                    token: None,
                    ..good()
                },
                Reject::BadToken,
            ),
            (
                Head {
                    content_type: Some("text/plain"),
                    ..good()
                },
                Reject::NotJson,
            ),
            (
                Head {
                    content_type: None,
                    ..good()
                },
                Reject::NotJson,
            ),
            (
                Head {
                    content_length: Some(MAX_BODY as u64 + 1),
                    ..good()
                },
                Reject::TooLarge,
            ),
        ];
        for (head, expected) in cases {
            assert_eq!(run(&head), Err(expected));
        }
    }

    #[test]
    fn an_empty_configured_token_accepts_nothing() {
        let head = Head {
            token: Some(b""),
            ..good()
        };
        assert_eq!(check_head(&head, 47821, ""), Err(Reject::BadToken));
    }

    #[test]
    fn the_rate_limit_allows_a_burst_then_refills() {
        let start = Instant::now();
        let mut limit = RateLimit::new(start);
        let allowed = (0..1_100).filter(|_| limit.allow(start)).count();
        assert_eq!(allowed, 1_000);
        assert!(!limit.allow(start));
        assert!(limit.allow(start + Duration::from_millis(100)));
    }
}
