// SPDX-License-Identifier: Apache-2.0
//! The four toasts (design section 8). Every automatic switch is announced by one. The focus
//! key always does the thing that won't happen on its own.

use std::time::Duration;

use crate::events::AppRef;
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

/// `immersive` adds the warning for apps that are hard to leave (backlog C4).
pub(crate) fn away_countdown(
    target: &AppRef,
    countdown: Duration,
    immersive: bool,
    agent: &str,
) -> Toast {
    let warning = if immersive {
        &format!(" It may be hard to leave when {agent} is done.")
    } else {
        ""
    };
    let (name, secs) = (&target.name, countdown.as_secs());
    Toast {
        kind: ToastKind::AwayCountdown,
        text: format!("{agent} will take a while. Switching to {name} in {secs} s.{warning}"),
        action: "Not now",
        countdown: Some(countdown),
    }
}

pub(crate) fn away_prompt(target: &AppRef, agent: &str) -> Toast {
    Toast {
        kind: ToastKind::AwayPrompt,
        text: format!("{agent} will take a while. Switch to {}?", target.name),
        action: "Switch now",
        countdown: None,
    }
}

pub(crate) fn return_countdown(
    session: &Session,
    project: &str,
    home: &AppRef,
    countdown: Duration,
) -> Toast {
    Toast {
        kind: ToastKind::ReturnCountdown,
        text: format!(
            "{} Back to {} in {} s.",
            headline(session, project),
            home.name,
            countdown.as_secs()
        ),
        action: "Snooze",
        countdown: Some(countdown),
    }
}

pub(crate) fn return_prompt(session: &Session, project: &str) -> Toast {
    Toast {
        kind: ToastKind::ReturnPrompt,
        text: format!("{} Go back?", headline(session, project)),
        action: "Go back now",
        countdown: None,
    }
}

/// "Claude is done in my-app.": the project name tells parallel sessions apart.
fn headline(session: &Session, project: &str) -> String {
    let agent = session.run.agent;
    let base = match (&session.run.need, &session.run.outcome) {
        (Some(Need::Approval(_) | Need::Plan), _) => "needs your approval",
        (Some(Need::Question), _) => "has a question",
        (Some(Need::Mcp(_) | Need::Input), _) => "needs your input",
        (None, Some(Outcome::Error(_))) => "stopped with an error",
        (None, Some(Outcome::Done { .. })) if session.run.attention_app.is_some() => {
            "finished a task"
        }
        (None, _) => "is done",
    };
    if project.is_empty() {
        format!("{agent} {base}.")
    } else {
        format!("{agent} {base} in {project}.")
    }
            }
