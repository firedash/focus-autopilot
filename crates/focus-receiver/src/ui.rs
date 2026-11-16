// SPDX-License-Identifier: Apache-2.0
//! The statistics and configuration page: three static files under `/ui/` and a small JSON
//! API under `/api/`. The receiver only routes and checks requests; what the numbers are and
//! how a setting is saved is the backend's business, supplied by the shell.
//!
//! The page has no inline script or style and loads nothing from anywhere else, which the
//! Content-Security-Policy below enforces, so text from prompts or app names can never run.
//! Security-relevant: changes need a maintainer review.

/// What the page can ask for. Implemented by the shell.
pub trait UiBackend: Send + Sync + 'static {
    /// The statistics as JSON. `tz_minutes` is the viewer's offset east of UTC.
    fn stats(&self, tz_minutes: i32) -> String;
    /// The settings and their current values as JSON.
    fn config(&self) -> String;
    /// Applies a settings change. `Ok` is the new settings as JSON; `Err` is a sentence for the
    /// user saying why nothing was changed.
    ///
    /// # Errors
    /// When the change is invalid or can't be saved.
    fn set_config(&self, body: &[u8]) -> Result<String, String>;
    /// Erases the recorded statistics and returns the (now empty) statistics.
    fn clear_stats(&self, tz_minutes: i32) -> String;
}

pub struct Asset {
    pub content_type: &'static str,
    pub body: &'static str,
}

/// Everything is same-origin and nothing is inline: no script, style, frame or request can
/// come from or go to anywhere but this server.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
     style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; \
     form-action 'none'; frame-ancestors 'none'";

/// The page's own files, by request path.
pub fn asset(path: &str) -> Option<Asset> {
    let (content_type, body) = match path {
        "/ui" | "/ui/" | "/ui/index.html" => (
            "text/html; charset=utf-8",
            include_str!("../assets/ui/index.html"),
        ),
        "/ui/app.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../assets/ui/app.js"),
        ),
        "/ui/style.css" => (
            "text/css; charset=utf-8",
            include_str!("../assets/ui/style.css"),
        ),
        _ => return None,
    };
    Some(Asset { content_type, body })
}

/// The `tz` query parameter: minutes east of UTC, limited to offsets that exist.
pub fn tz_minutes(query: Option<&str>) -> i32 {
    query
        .into_iter()
        .flat_map(|q| q.split('&'))
        .find_map(|pair| pair.strip_prefix("tz="))
        .and_then(|v| v.parse::<i32>().ok())
        .map_or(0, |minutes| minutes.clamp(-12 * 60, 14 * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_files_are_served_and_nothing_else() {
        for path in ["/ui", "/ui/", "/ui/index.html"] {
            assert!(asset(path).unwrap().body.contains("<!doctype html>"));
        }
        assert!(
            asset("/ui/app.js")
                .unwrap()
                .content_type
                .starts_with("text/javascript")
        );
        assert!(
            asset("/ui/style.css")
                .unwrap()
                .content_type
                .starts_with("text/css")
        );
        for path in ["/ui/../Cargo.toml", "/ui/secret", "/ui//", "/uix", "/"] {
            assert!(asset(path).is_none(), "{path}");
        }
    }

    #[test]
    fn the_page_has_nothing_inline_for_the_policy_to_block() {
        let html = asset("/ui/").unwrap().body;
        assert!(!html.contains("<style"), "styles belong in style.css");
        assert!(!html.contains(" style="), "no inline style attributes");
        assert!(!html.contains(" onclick=") && !html.contains(" onload="));
        // Every script tag loads a file; none carries code.
        for tag in html.split("<script").skip(1) {
            let open = tag.split('>').next().unwrap_or("");
            assert!(open.contains("src="), "inline script: {open}");
        }
        assert!(
            !html.contains("http://") && !html.contains("https://"),
            "nothing external"
        );
        let js = asset("/ui/app.js").unwrap().body;
        assert!(
            !js.contains("innerHTML") && !js.contains("eval("),
            "text is set as text"
        );
    }

    #[test]
    fn the_time_zone_is_read_and_bounded() {
        assert_eq!(tz_minutes(Some("tz=330")), 330);
        assert_eq!(tz_minutes(Some("a=1&tz=-480")), -480);
        assert_eq!(tz_minutes(Some("tz=99999")), 840);
        assert_eq!(tz_minutes(Some("tz=-99999")), -720);
        assert_eq!(tz_minutes(Some("tz=abc")), 0);
        assert_eq!(tz_minutes(None), 0);
    }
}
