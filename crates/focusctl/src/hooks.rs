// SPDX-License-Identifier: Apache-2.0
//! Merging focusd's hook entries into Claude Code's `settings.json` and removing them again
//! (design section 6, "Installer rules"). Security-relevant: this edits another tool's
//! settings. focusd's entries are the ones whose URL starts with `http://127.0.0.1:<port>/hook`.

use serde_json::{Map, Value, json};

/// Events focusd registers, with the matcher the design gives each (`None` matches all).
const EVENTS: &[(&str, Option<&str>)] = &[
    ("UserPromptSubmit", None),
    ("PreToolUse", None),
    ("PermissionRequest", None),
    ("PermissionDenied", None),
    (
        "Notification",
        Some("permission_prompt|elicitation_dialog|elicitation_url_dialog"),
    ),
    ("Elicitation", None),
    ("SubagentStart", None),
    ("SubagentStop", None),
    ("Stop", None),
    ("StopFailure", None),
    ("SessionEnd", None),
];

pub fn hook_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/hook")
}

#[derive(Debug, PartialEq, Eq)]
pub enum MergeError {
    /// `settings.json` is valid JSON but not an object, or `hooks` isn't one.
    Shape(&'static str),
}

fn is_ours(hook: &Value, url: &str) -> bool {
    hook.get("url")
        .and_then(Value::as_str)
        .is_some_and(|u| u.starts_with(url))
}

fn entry(matcher: Option<&str>, url: &str, token: &str) -> Value {
    let hook = json!({
        "type": "http",
        "url": url,
        "timeout": 2,
        "headers": { "X-Focus-Token": token },
    });
    let mut entry = Map::new();
    if let Some(m) = matcher {
        entry.insert("matcher".into(), json!(m));
    }
    entry.insert("hooks".into(), json!([hook]));
    Value::Object(entry)
}

/// True when this matcher group holds one of our hooks.
fn group_is_ours(group: &Value, url: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(|h| is_ours(h, url)))
}

/// Adds or refreshes our entry for every event, beside whatever is already there. Running it
/// again with the same token changes nothing.
///
/// # Errors
/// When `settings` or its `hooks` value has an unexpected shape.
pub fn install(settings: &mut Value, port: u16, token: &str) -> Result<(), MergeError> {
    let url = hook_url(port);
    let root = settings
        .as_object_mut()
        .ok_or(MergeError::Shape("settings.json is not a JSON object"))?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(MergeError::Shape("`hooks` is not an object"))?;
    for (event, matcher) in EVENTS {
        let groups = hooks
            .entry((*event).to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or(MergeError::Shape("a hook event is not an array"))?;
        let new = entry(*matcher, &url, token);
        match groups.iter().position(|g| group_is_ours(g, &url)) {
            Some(first) => {
                groups[first] = new;
                let mut i = groups.len();
                while i > first + 1 {
                    i -= 1;
                    if group_is_ours(&groups[i], &url) {
                        groups.remove(i);
                    }
                }
            }
            None => groups.push(new),
        }
    }
    Ok(())
}

/// Removes only our hooks. A matcher group left empty goes too, then an event left empty, then
/// `hooks` itself if nothing else is in it.
///
/// # Errors
/// When `settings` or its `hooks` value has an unexpected shape.
pub fn uninstall(settings: &mut Value, port: u16) -> Result<bool, MergeError> {
    let url = hook_url(port);
    let root = settings
        .as_object_mut()
        .ok_or(MergeError::Shape("settings.json is not a JSON object"))?;
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(false);
    };
    let hooks = hooks
        .as_object_mut()
        .ok_or(MergeError::Shape("`hooks` is not an object"))?;
    let mut removed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = list.len();
                list.retain(|h| !is_ours(h, &url));
                removed |= list.len() != before;
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|l| !l.is_empty())
        });
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        root.remove("hooks");
    }
    Ok(removed)
}

/// The indentation unit of a JSON text: the whitespace before its first indented line.
pub fn indent_of(text: &str) -> String {
    text.lines()
        .find_map(|l| {
            let trimmed = l.trim_start();
            let lead = &l[..l.len() - trimmed.len()];
            (!lead.is_empty() && !trimmed.is_empty()).then(|| lead.to_owned())
        })
        .unwrap_or_else(|| "  ".to_owned())
}

/// Serializes with the given indent and keeps a final newline if the original had one.
pub fn render(value: &Value, indent: &str, trailing_newline: bool) -> String {
    use serde::Serialize as _;
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
    // Serializing a `Value` to memory cannot fail.
    let _ = value.serialize(&mut ser);
    let mut text = String::from_utf8_lossy(&out).into_owned();
    if trailing_newline {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEIRS: &str = r#"{
  "model": "x",
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "say done"
          }
        ]
      }
    ]
  }
}
"#;

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn install_adds_beside_existing_hooks_and_is_idempotent() {
        let mut v = parse(THEIRS);
        install(&mut v, 47821, "tok").unwrap();
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert_eq!(stop[1]["hooks"][0]["url"], "http://127.0.0.1:47821/hook");
        assert_eq!(v["hooks"].as_object().unwrap().len(), 11);
        let once = v.clone();
        install(&mut v, 47821, "tok").unwrap();
        assert_eq!(v, once);
    }

    #[test]
    fn install_with_a_new_token_replaces_in_place() {
        let mut v = parse(THEIRS);
        install(&mut v, 47821, "old").unwrap();
        install(&mut v, 47821, "new").unwrap();
        let text = v.to_string();
        assert!(!text.contains("old"));
        assert_eq!(text.matches("X-Focus-Token").count(), 11);
    }

    #[test]
    fn uninstall_restores_the_original_bytes() {
        let mut v = parse(THEIRS);
        install(&mut v, 47821, "tok").unwrap();
        assert!(uninstall(&mut v, 47821).unwrap());
        assert_eq!(render(&v, &indent_of(THEIRS), true), THEIRS);
    }

    #[test]
    fn uninstall_from_an_empty_settings_leaves_an_empty_object() {
        let mut v = parse("{}");
        install(&mut v, 47821, "tok").unwrap();
        uninstall(&mut v, 47821).unwrap();
        assert_eq!(v, parse("{}"));
        assert!(!uninstall(&mut v, 47821).unwrap());
    }

    #[test]
    fn a_hook_on_another_port_is_not_ours() {
        let mut v = parse("{}");
        install(&mut v, 9999, "tok").unwrap();
        assert!(!uninstall(&mut v, 47821).unwrap());
        assert!(v["hooks"]["Stop"].as_array().is_some());
    }

    #[test]
    fn wrong_shapes_are_errors_not_panics() {
        assert!(install(&mut parse("[]"), 1, "t").is_err());
        assert!(install(&mut parse(r#"{"hooks": 5}"#), 1, "t").is_err());
        assert!(install(&mut parse(r#"{"hooks": {"Stop": {}}}"#), 1, "t").is_err());
    }

    #[test]
    fn notification_gets_its_matcher() {
        let mut v = parse("{}");
        install(&mut v, 47821, "t").unwrap();
        assert_eq!(
            v["hooks"]["Notification"][0]["matcher"],
            "permission_prompt|elicitation_dialog|elicitation_url_dialog"
        );
        assert!(v["hooks"]["Stop"][0].get("matcher").is_none());
    }

    #[test]
    fn indent_is_detected() {
        assert_eq!(indent_of("{\n    \"a\": 1\n}"), "    ");
        assert_eq!(indent_of("{\n\t\"a\": 1\n}"), "\t");
        assert_eq!(indent_of("{}"), "  ");
    }
}
