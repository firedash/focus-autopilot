// SPDX-License-Identifier: Apache-2.0
//! The request checks from the design's threat model, as pure functions so each one has a
//! negative test. Security-relevant: changes need a maintainer review.

use std::time::Instant;

use subtle::ConstantTimeEq;

pub const TOKEN_HEADER: &str = "x-focus-token";
pub const HOOK_PATH: &str = "/hook";
pub const MAX_BODY: usize = 8 * 1024 * 1024;
/// The settings page sends small JSON; nothing legitimate is larger.
pub const MAX_API_BODY: usize = 64 * 1024;
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
    /// A page or API request from some other site.
    ForeignOrigin,
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
            Self::ForeignHost | Self::HasOrigin | Self::ForeignOrigin => 403,
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
    /// The Origin header's value, when it is readable text.
    pub origin: Option<&'a str>,
    /// `Sec-Fetch-Site`, which browsers set and pages can't forge.
    pub fetch_site: Option<&'a str>,
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

/// True when the request names no origin, or names this server. A present but unreadable
/// Origin header is foreign.
fn origin_ok(head: &Head<'_>, port: u16) -> bool {
    if !head.origin_present {
        return true;
    }
    head.origin.is_some_and(|origin| {
        let origin = origin.to_ascii_lowercase();
        origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
    })
}

/// Checks a request for one of the page's own files. They hold no data, so no token is
/// needed, but they are only served to this machine, by name, and never into another site.
///
/// # Errors
/// The first check the request fails.
pub fn check_page(head: &Head<'_>, port: u16) -> Result<(), Reject> {
    if head.method != "GET" {
        return Err(Reject::MethodNotAllowed);
    }
    if !host_ok(head.host, port) {
        return Err(Reject::ForeignHost);
    }
    if !origin_ok(head, port) {
        return Err(Reject::ForeignOrigin);
    }
    Ok(())
}

/// Checks a request to the page's API, which reads statistics and changes settings. On top of
/// the Host check it needs the token, must come from this server's own page (a browser on
/// another site is refused by Origin and by `Sec-Fetch-Site`), and a write must be JSON, which
/// a cross-site form cannot send.
///
/// # Errors
/// The first check the request fails.
pub fn check_api(head: &Head<'_>, port: u16, expected_token: &str) -> Result<(), Reject> {
    if head.method != "GET" && head.method != "POST" {
        return Err(Reject::MethodNotAllowed);
    }
    if !host_ok(head.host, port) {
        return Err(Reject::ForeignHost);
    }
    if !origin_ok(head, port)
        || head
            .fetch_site
            .is_some_and(|site| !site.eq_ignore_ascii_case("same-origin"))
    {
        return Err(Reject::ForeignOrigin);
    }
    if !token_ok(head.token, expected_token) {
        return Err(Reject::BadToken);
    }
    if head.method == "POST" {
        if !is_json(head.content_type) {
            return Err(Reject::NotJson);
        }
        if head.content_length.is_some_and(|n| n > MAX_API_BODY as u64) {
            return Err(Reject::TooLarge);
        }
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
            origin: None,
            fetch_site: None,
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

    fn api() -> Head<'static> {
        Head {
            method: "GET",
            path: "/api/stats",
            ..good()
        }
    }

    #[test]
    fn the_page_is_served_to_this_machine_only() {
        let page = Head {
            method: "GET",
            path: "/ui/",
            token: None,
            content_type: None,
            ..good()
        };
        assert_eq!(check_page(&page, 47821), Ok(()));
        let own = Head {
            origin_present: true,
            origin: Some("http://127.0.0.1:47821"),
            ..good()
        };
        assert_eq!(
            check_page(
                &Head {
                    method: "GET",
                    ..own
                },
                47821
            ),
            Ok(())
        );
        let cases = [
            (
                Head {
                    method: "POST",
                    path: "/ui/",
                    ..good()
                },
                Reject::MethodNotAllowed,
            ),
            (
                Head {
                    method: "GET",
                    host: Some("evil.test:47821"),
                    ..good()
                },
                Reject::ForeignHost,
            ),
            (
                Head {
                    method: "GET",
                    host: None,
                    ..good()
                },
                Reject::ForeignHost,
            ),
            (
                Head {
                    method: "GET",
                    origin_present: true,
                    origin: Some("http://evil.test"),
                    ..good()
                },
                Reject::ForeignOrigin,
            ),
        ];
        for (head, expected) in cases {
            assert_eq!(check_page(&head, 47821), Err(expected));
        }
    }

    #[test]
    fn the_api_accepts_its_own_page_with_the_token() {
        assert_eq!(check_api(&api(), 47821, "secret"), Ok(()));
        let from_page = Head {
            origin_present: true,
            origin: Some("http://LOCALHOST:47821"),
            fetch_site: Some("same-origin"),
            method: "POST",
            ..api()
        };
        assert_eq!(check_api(&from_page, 47821, "secret"), Ok(()));
    }

    #[test]
    fn the_api_refuses_everything_else() {
        let foreign = |origin: Option<&'static str>| Head {
            origin_present: true,
            origin,
            ..api()
        };
        let cases = [
            (
                Head {
                    token: None,
                    ..api()
                },
                Reject::BadToken,
            ),
            (
                Head {
                    token: Some(b"wrong"),
                    ..api()
                },
                Reject::BadToken,
            ),
            (
                Head {
                    host: Some("evil.test:47821"),
                    ..api()
                },
                Reject::ForeignHost,
            ),
            (foreign(Some("http://evil.test")), Reject::ForeignOrigin),
            (
                foreign(Some("https://127.0.0.1:47821")),
                Reject::ForeignOrigin,
            ),
            (foreign(Some("http://127.0.0.1:1")), Reject::ForeignOrigin),
            (foreign(Some("null")), Reject::ForeignOrigin),
            // An Origin header that isn't readable text is still an origin, and not ours.
            (foreign(None), Reject::ForeignOrigin),
            (
                Head {
                    fetch_site: Some("cross-site"),
                    ..api()
                },
                Reject::ForeignOrigin,
            ),
            (
                Head {
                    fetch_site: Some("same-site"),
                    ..api()
                },
                Reject::ForeignOrigin,
            ),
            (
                Head {
                    fetch_site: Some("none"),
                    ..api()
                },
                Reject::ForeignOrigin,
            ),
            (
                Head {
                    method: "PUT",
                    ..api()
                },
                Reject::MethodNotAllowed,
            ),
            (
                Head {
                    method: "DELETE",
                    ..api()
                },
                Reject::MethodNotAllowed,
            ),
            (
                Head {
                    method: "POST",
                    content_type: Some("text/plain"),
                    ..api()
                },
                Reject::NotJson,
            ),
            (
                Head {
                    method: "POST",
                    content_type: Some("application/x-www-form-urlencoded"),
                    ..api()
                },
                Reject::NotJson,
            ),
            (
                Head {
                    method: "POST",
                    content_length: Some(MAX_API_BODY as u64 + 1),
                    ..api()
                },
                Reject::TooLarge,
            ),
        ];
        for (head, expected) in cases {
            assert_eq!(check_api(&head, 47821, "secret"), Err(expected));
        }
        assert_eq!(check_api(&api(), 47821, ""), Err(Reject::BadToken));
    }

    #[test]
    fn a_hook_with_any_origin_is_still_refused_even_our_own() {
        let own = Head {
            origin_present: true,
            origin: Some("http://127.0.0.1:47821"),
            ..good()
        };
        assert_eq!(run(&own), Err(Reject::HasOrigin));
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
