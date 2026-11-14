// SPDX-License-Identifier: Apache-2.0
//! The basic recap card (design sections 8 and 10): what was asked, what Claude did, and what
//! needs a decision. Built from hook payload fields only. The shell must show every line as
//! plain text.

use std::time::Duration;

use crate::run::{Need, Outcome, Run};
use crate::session::{Session, SessionState};

/// The card is small; a longer line is cut with an ellipsis.
const LINE_MAX_CHARS: usize = 160;
const FILES_SHOWN: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapCard {
    /// At most six lines of plain text.
    pub lines: Vec<String>,
}

/// `others` is how many other sessions are also waiting for the user.
pub(crate) fn build(session: &Session, others: usize) -> RecapCard {
    let run = &session.run;
    let mut lines = Vec::new();
    if !run.prompt.is_empty() {
        lines.push(format!("Asked:   {}", run.prompt));
    }
    lines.push(status_line(session));
    if let Some(touched) = touched_line(&run.files, &run.cwd) {
        lines.push(touched);
    }
    if let Some(stats) = stats_line(run) {
        lines.push(stats);
    }
    if others > 0 {
        let others = u32::try_from(others).unwrap_or(u32::MAX);
        lines.push(format!("Also waiting: {}", count(others, "other session")));
    }
    RecapCard {
        lines: lines.iter().map(|line| fit(line)).collect(),
    }
}

fn status_line(session: &Session) -> String {
    let run = &session.run;
    match (session.state, &run.need, &run.outcome) {
        (SessionState::NeedsInput, Some(need), _) => format!("Needs:   {}", need_text(need)),
        (_, _, Some(Outcome::Done { message, .. })) if !message.is_empty() => {
            format!("Claude:  {message}")
        }
        (_, _, Some(Outcome::Done { .. })) => "Claude:  finished".to_owned(),
        (_, _, Some(Outcome::Error(error))) => {
            format!("Claude:  stopped with an error ({error})")
        }
        _ => "Claude:  still working".to_owned(),
    }
}

pub(crate) fn need_text(need: &Need) -> String {
    match need {
        Need::Approval(what) => format!("approval to {what}"),
        Need::Plan => "approval of Claude's plan".to_owned(),
        Need::Question => "an answer to Claude's question".to_owned(),
        Need::Mcp(server) => format!("your input for {server}"),
        Need::Input => "your input".to_owned(),
    }
}

/// Run statistics for a finished run (backlog F3): duration, tool calls, sub-agents, and
/// background tasks still running.
fn stats_line(run: &Run) -> Option<String> {
    let outcome = run.outcome.as_ref()?;
    let duration = run.stopped_at?.saturating_duration_since(run.started);
    let mut parts = vec![duration_text(duration)];
    if run.tools > 0 {
        parts.push(count(run.tools, "tool call"));
    }
    if run.subagents > 0 {
        parts.push(count(run.subagents, "sub-agent"));
    }
    if let Outcome::Done {
        background_tasks, ..
    } = outcome
        && *background_tasks > 0
    {
        parts.push(format!(
            "{} still running",
            count(*background_tasks, "background task")
        ));
    }
    Some(parts.join(" · "))
}

fn duration_text(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min", (secs + 30) / 60)
    }
}

fn touched_line(files: &[String], cwd: &str) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let shown: Vec<&str> = files
        .iter()
        .take(FILES_SHOWN)
        .map(|f| relative(f, cwd))
        .collect();
    let line = format!("Touched: {}", shown.join(", "));
    if files.len() > FILES_SHOWN {
        return Some(format!("{line} (+{} more)", files.len() - FILES_SHOWN));
    }
    Some(line)
}

/// Shows a path relative to the project folder when it lies inside it.
fn relative<'a>(path: &'a str, cwd: &str) -> &'a str {
    if cwd.is_empty() {
        return path;
    }
    path.strip_prefix(cwd)
        .and_then(|rest| rest.strip_prefix(['/', '\\']))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(path)
}

fn count(n: u32, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn fit(line: &str) -> String {
    if line.chars().count() <= LINE_MAX_CHARS {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(LINE_MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_shown_relative_to_the_project() {
        assert_eq!(relative("/work/app/src/sum.ts", "/work/app"), "src/sum.ts");
        assert_eq!(
            relative(r"C:\work\app\src\sum.ts", r"C:\work\app"),
            r"src\sum.ts"
        );
        assert_eq!(
            relative("/work/application/x.ts", "/work/app"),
            "/work/application/x.ts"
        );
        assert_eq!(relative("/elsewhere/x.ts", "/work/app"), "/elsewhere/x.ts");
        assert_eq!(relative("/work/app", "/work/app"), "/work/app");
    }

    #[test]
    fn long_lines_are_cut_with_an_ellipsis() {
        let line = "x".repeat(LINE_MAX_CHARS + 40);
        let cut = fit(&line);
        assert_eq!(cut.chars().count(), LINE_MAX_CHARS);
        assert!(cut.ends_with('…'));
        assert_eq!(fit("short"), "short");
    }

    #[test]
    fn extra_files_are_counted() {
        let files: Vec<String> = (0..7).map(|i| format!("/p/f{i}.rs")).collect();
        assert_eq!(
            touched_line(&files, "/p").unwrap(),
            "Touched: f0.rs, f1.rs, f2.rs, f3.rs, f4.rs (+2 more)"
        );
        assert_eq!(touched_line(&[], "/p"), None);
        assert_eq!(duration_text(Duration::from_secs(59)), "59 s");
        assert_eq!(duration_text(Duration::from_secs(89)), "1 min");
        assert_eq!(duration_text(Duration::from_secs(240)), "4 min");
    }
}
