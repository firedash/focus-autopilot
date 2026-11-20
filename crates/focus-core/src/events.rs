// SPDX-License-Identifier: Apache-2.0
//! What the core is told: normalized hook events, platform observations and user actions.

use std::time::Duration;

/// Kept-string limits from the design (section 6). The receiver applies them when it parses a
/// payload; the core applies them again when it stores a value, so nothing longer is ever held.
pub const PROMPT_MAX_CHARS: usize = 500;
pub const MESSAGE_MAX_CHARS: usize = 1_500;
pub const COMMAND_MAX_CHARS: usize = 200;
pub const PATH_MAX_CHARS: usize = 300;

/// An app, and on Windows one of its windows, that can be in front or be activated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRef {
    /// Bundle id on macOS, executable name on Windows.
    pub id: String,
    /// Display name used in toast text.
    pub name: String,
    pub pid: u32,
    /// Window handle on Windows; `None` on macOS, where activation is per app.
    pub window: Option<u64>,
}

impl AppRef {
    /// True when `other` is this target: the same app and, where both carry a window handle,
    /// the same window.
    pub fn matches(&self, other: &AppRef) -> bool {
        self.id.eq_ignore_ascii_case(&other.id)
            && match (self.window, other.window) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

/// One Claude Code hook event, reduced to the fields the design lists in section 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub session_id: String,
    /// Identifies the run. Missing on older Claude Code; the event then belongs to the
    /// session's current run.
    pub prompt_id: Option<String>,
    pub cwd: String,
    /// Salted hash of `cwd`, computed by the shell; groups runs by project in history.
    pub project: String,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// Provider liveness, not evidence of a new user submission or a tool call.
    AgentActivity,
    /// App-level attention, independent of prompt/home-window capture. The app is not named
    /// here: the core takes it from `antigravity.app_name` in its own configuration.
    AppFinished {
        message: String,
        failed: bool,
    },
    AppNeedsInput,
    UserPromptSubmit {
        prompt: String,
        /// Length of the full prompt, before the receiver truncated it.
        prompt_chars: usize,
        permission_mode: Option<String>,
    },
    PreToolUse {
        tool_name: String,
        /// `tool_input.command`, for Bash and PowerShell only.
        command: Option<String>,
        /// `tool_input.file_path`, for Edit and Write only.
        file_path: Option<String>,
        /// `effort.level`.
        effort: Option<String>,
    },
    PermissionRequest {
        tool_name: String,
        command: Option<String>,
        file_path: Option<String>,
    },
    PermissionDenied,
    Notification {
        notification_type: String,
    },
    Elicitation {
        mcp_server_name: String,
    },
    SubagentStart,
    SubagentStop,
    Stop {
        stop_hook_active: bool,
        last_assistant_message: String,
        background_tasks: u32,
    },
    StopFailure {
        error: String,
    },
    SessionEnd,
}

/// What the shell observed on the platform.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// The frontmost app changed, or answers a frontmost probe. `None` when it can't be read.
    Frontmost(Option<AppRef>),
    /// Time since the last keyboard or mouse input anywhere. Never the input itself.
    Idle(Duration),
    /// Regular running apps, answering a running-apps probe.
    RunningApps(Vec<AppRef>),
    /// Result of the last `Effect::Activate`, read `verify_after` later.
    Activation { verified: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAction {
    /// A click on the visible toast, or the focus key.
    ToastAction,
    Pause(crate::PauseFor),
    Resume,
    TogglePause,
    /// The declare-long hotkey: this run will take a while, switch me away now.
    DeclareLong,
}

/// Cuts a line to at most `max_chars`, at a word boundary with an ellipsis when there is one
/// to cut at, rather than stopping mid-word.
pub(crate) fn shorten(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        return line.to_owned();
    }
    let room: String = line.chars().take(max_chars.saturating_sub(1)).collect();
    let cut = room
        .rfind(' ')
        .filter(|&at| at > room.len() / 2)
        .unwrap_or(room.len());
    format!("{}…", room[..cut].trim_end())
}

/// The last folder of a project path, for telling sessions apart on a toast.
pub(crate) fn project_name(cwd: &str) -> String {
    let name = cwd
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    shorten(&clean_text(name, 80), 32)
}

/// `parent/name` for a project folder, used when two sessions' folders share a last name.
pub(crate) fn project_name_with_parent(cwd: &str) -> String {
    let parts: Vec<&str> = cwd
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .filter(|p| !p.is_empty())
        .take(2)
        .collect();
    let label = match parts.as_slice() {
        [name, parent] => format!("{parent}/{name}"),
        [name] => (*name).to_owned(),
        _ => String::new(),
    };
    shorten(&clean_text(&label, 120), 40)
}

/// Makes outside text safe to keep and show: drops ANSI escape sequences and control
/// characters, turns line breaks and tabs into spaces, and keeps at most `max_chars`.
pub fn clean_text(raw: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut kept = 0;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if kept == max_chars {
            break;
        }
        match c {
            '\u{1b}' => skip_escape(&mut chars),
            '\n' | '\r' | '\t' => {
                out.push(' ');
                kept += 1;
            }
            c if c.is_control() => {}
            c => {
                out.push(c);
                kept += 1;
            }
        }
    }
    out
}

/// Consumes the rest of an escape sequence whose ESC was just read.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        // CSI: parameters, then one final byte in '@'..='~'.
        Some('[') => {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
        // OSC: runs to BEL or to the string terminator ESC '\'.
        Some(']') => {
            while let Some(c) = chars.next() {
                if c == '\u{7}' {
                    break;
                }
                if c == '\u{1b}' {
                    chars.next();
                    break;
                }
            }
        }
        // Two-character escape, or a lone ESC at the end.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_and_controls() {
        let raw = "\u{1b}[31mred\u{1b}[0m\u{0}\u{7} two\nlines\u{1b}]0;title\u{7}!";
        assert_eq!(clean_text(raw, 100), "red two lines!");
    }

    #[test]
    fn truncates_by_characters_not_bytes() {
        assert_eq!(clean_text("héllo wörld", 5), "héllo");
        assert_eq!(clean_text("abc", 0), "");
    }

    #[test]
    fn window_handles_decide_a_match_only_when_both_known() {
        let app = |window| AppRef {
            id: "Code.exe".into(),
            name: "VS Code".into(),
            pid: 1,
            window,
        };
        assert!(app(Some(1)).matches(&app(Some(1))));
        assert!(!app(Some(1)).matches(&app(Some(2))));
        assert!(app(None).matches(&app(Some(2))));
        let other = AppRef {
            id: "code.EXE".into(),
            ..app(None)
        };
        assert!(app(None).matches(&other));
    }
}
