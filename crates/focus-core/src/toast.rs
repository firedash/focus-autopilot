// SPDX-License-Identifier: Apache-2.0
//! The four toasts (design section 8). Every automatic switch is announced by one. The focus
//! key always does the thing that won't happen on its own.

use std::time::Duration;

use crate::events::{AppRef, project_name};
use crate::run::{Need, Outcome};
use crate::session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    AwayCountdown,
    AwayPrompt,
    ReturnCountdown,
    ReturnPrompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    /// Plain text; the shell must not parse it as HTML or Markdown.
    pub text: String,
    /// What a click or the focus key does.
    pub action: &'static str,
    /// Set for countdown toasts; prompts stay until acted on or withdrawn.
    pub countdown: Option<Duration>,
}

const AWAY_HEADLINE: &str = "Claude will take a while.";

/// `immersive` adds the warning for apps that are hard to leave (backlog C4).
pub(crate) fn away_countdown(target: &AppRef, countdown: Duration, immersive: bool) -> Toast {
    let warning = if immersive {
        " It may be hard to leave when Claude is done."
    } else {
        ""
    };
    let (name, secs) = (&target.name, countdown.as_secs());
    Toast {
        kind: ToastKind::AwayCountdown,
        text: format!("{AWAY_HEADLINE} Switching to {name} in {secs} s.{warning}"),
        action: "Not now",
        countdown: Some(countdown),
    }
}

pub(crate) fn away_prompt(target: &AppRef) -> Toast {
    Toast {
        kind: ToastKind::AwayPrompt,
        text: format!("{AWAY_HEADLINE} Switch to {}?", target.name),
        action: "Switch now",
        countdown: None,
    }
}

pub(crate) fn return_countdown(session: &Session, home: &AppRef, countdown: Duration) -> Toast {
    Toast {
        kind: ToastKind::ReturnCountdown,
        text: format!(
            "{} Back to {} in {} s.",
            headline(session),
            home.name,
            countdown.as_secs()
        ),
        action: "Snooze",
        countdown: Some(countdown),
    }
}

pub(crate) fn return_prompt(session: &Session) -> Toast {
    Toast {
        kind: ToastKind::ReturnPrompt,
        text: format!("{} Go back?", headline(session)),
        action: "Go back now",
        countdown: None,
    }
}

/// "Claude is done in my-app.": the project name tells parallel sessions apart.
fn headline(session: &Session) -> String {
    let base = match (&session.run.need, &session.run.outcome) {
        (Some(Need::Approval(_) | Need::Plan), _) => "Claude needs your approval",
        (Some(Need::Question), _) => "Claude has a question",
        (Some(Need::Mcp(_) | Need::Input), _) => "Claude needs your input",
        (None, Some(Outcome::Error(_))) => "Claude stopped with an error",
        (None, _) => "Claude is done",
    };
    let project = project_name(&session.run.cwd);
    if project.is_empty() {
        format!("{base}.")
    } else {
        format!("{base} in {project}.")
    }
}
