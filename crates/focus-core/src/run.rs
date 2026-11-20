// SPDX-License-Identifier: Apache-2.0
//! One run: a prompt and everything Claude does until it stops. Held in memory only; the
//! record written to history carries counts and categories, never text.

use std::time::{Duration, Instant};

use crate::estimator::{
    Estimate, MANY_TOOLS, MANY_TOOLS_FLOOR, PromptLen, RunSample, SLOW_COMMAND_FLOOR,
    SUBAGENT_FLOOR, Tier, is_slow_command, tier_of,
};
use crate::events::{
    AgentEvent, AppRef, COMMAND_MAX_CHARS, EventKind, PATH_MAX_CHARS, PROMPT_MAX_CHARS, clean_text,
};

/// Distinct touched files and commands remembered per run; the recap shows fewer.
const MAX_FILES_KEPT: usize = 64;
const MAX_COMMANDS_KEPT: usize = 3;
const LABEL_MAX_CHARS: usize = 40;

/// What Claude is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Need {
    /// Approval of a tool call, as a phrase such as "run npm test".
    Approval(String),
    Plan,
    Question,
    Mcp(String),
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done {
        message: String,
        background_tasks: u32,
    },
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Done,
    Error,
    /// Went quiet or was replaced without a Stop: an interrupt or a crash.
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AwayHow {
    #[default]
    None,
    /// Switched after the countdown.
    Auto,
    /// Switched from the fallback prompt.
    Prompt,
    /// The user chose "Not now".
    Cancelled,
    /// The user went to an away app themselves.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReturnHow {
    #[default]
    None,
    Auto,
    Prompt,
    /// The user came back themselves.
    Manual,
}

/// One line of `runs.jsonl` (design section 11), before the shell adds wall-clock time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run_id: Option<String>,
    pub project: String,
    pub started: Instant,
    pub duration: Duration,
    pub outcome: RunOutcome,
    pub permission_mode: Option<String>,
    pub effort: Option<String>,
    pub prompt_len: PromptLen,
    pub tools: u32,
    pub subagents: u32,
    pub slow_commands: u32,
    pub needed_input: bool,
    pub predicted_tier: Tier,
    pub away: AwayHow,
    pub returned: ReturnHow,
    pub snoozes: u32,
}

impl RunRecord {
    pub(crate) fn sample(&self) -> RunSample {
        RunSample {
            project: self.project.clone(),
            permission_mode: self.permission_mode.clone(),
            prompt_len: self.prompt_len,
            duration: self.duration,
        }
    }
}

/// What the core knows when a prompt is submitted.
#[derive(Debug, Clone, Default)]
pub(crate) struct Start {
    /// The frontmost window, if it's a home app.
    pub home: Option<AppRef>,
    /// Layer 1 estimate from similar past runs.
    pub estimate: Option<Estimate>,
}

// The flags are independent facts about a run, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub agent: &'static str,
    /// Set when this run is an app-level attention request (a finished task or an input
    /// request), which is returned to by app name, not by a home window.
    pub attention: bool,
    pub id: Option<String>,
    pub started: Instant,
    pub prompt: String,
    pub cwd: String,
    /// The window the prompt was sent from. `None` means focusd never switches for this run.
    pub home: Option<AppRef>,
    pub tier: Tier,
    pub estimate: Option<Estimate>,
    pub tools: u32,
    pub subagents: u32,
    pub files: Vec<String>,
    pub commands: Vec<String>,
    /// An away switch happened or was cancelled; there won't be another in this run.
    pub away_spent: bool,
    /// The user pressed the declare-long hotkey.
    pub declared_long: bool,
    /// When the user last arrived at this run's home window from another app while it was
    /// running. No away switch for a short grace after it, so coming back to look at Claude
    /// is not answered by being moved straight out again.
    pub back_at_home: Option<Instant>,
    pub need: Option<Need>,
    pub outcome: Option<Outcome>,
    /// When the session last entered `NeedsInput` or `Done`.
    pub waiting_since: Instant,
    /// The user is back, or was never away, for the current wait.
    pub return_handled: bool,
    pub away_how: AwayHow,
    pub return_how: ReturnHow,
    pub snoozes: u32,
    pub needed_input: bool,
    pub input_after_needs: bool,
    pub last_event: Instant,
    pub stopped_at: Option<Instant>,
    /// Already written to history, or never to be (a run adopted mid-way).
    pub recorded: bool,
    project: String,
    permission_mode: Option<String>,
    effort: Option<String>,
    prompt_len: PromptLen,
    slow_commands: u32,
    floor: Option<Duration>,
}

impl Run {
    /// A run started by a prompt, or adopted mid-way when `ev` is any other event.
    pub fn new(ev: &AgentEvent, start: &Start, now: Instant) -> Self {
        let (prompt, permission_mode, prompt_len, started_here) = match &ev.kind {
            EventKind::UserPromptSubmit {
                prompt,
                prompt_chars,
                permission_mode,
            } => (
                clean_text(prompt, PROMPT_MAX_CHARS),
                permission_mode.as_deref().map(label),
                PromptLen::of_chars(*prompt_chars),
                true,
            ),
            _ => (String::new(), None, PromptLen::default(), false),
        };
        let mut run = Self {
            attention: false,
            agent: if ev.session_id.starts_with("antigravity:") {
                "Antigravity"
            } else {
                "Claude"
            },
            id: ev.prompt_id.clone(),
            started: now,
            prompt,
            cwd: clean_text(&ev.cwd, PATH_MAX_CHARS),
            home: start.home.clone().filter(|_| started_here),
            tier: Tier::default(),
            estimate: start.estimate.filter(|_| started_here),
            tools: 0,
            subagents: 0,
            files: Vec::new(),
            commands: Vec::new(),
            away_spent: false,
            declared_long: false,
            back_at_home: None,
            need: None,
            outcome: None,
            waiting_since: now,
            return_handled: false,
            away_how: AwayHow::None,
            return_how: ReturnHow::None,
            snoozes: 0,
            needed_input: false,
            input_after_needs: false,
            last_event: now,
            stopped_at: None,
            recorded: !started_here,
            project: label(&ev.project),
            permission_mode,
            effort: None,
            prompt_len,
            slow_commands: 0,
            floor: None,
        };
        run.retier();
        run
    }

    pub fn note_tool(
        &mut self,
        command: Option<&String>,
        file_path: Option<&String>,
        effort: Option<&String>,
    ) {
        self.tools = self.tools.saturating_add(1);
        if self.tools > MANY_TOOLS {
            self.raise_floor(MANY_TOOLS_FLOOR);
        }
        if let Some(effort) = effort {
            self.effort = Some(label(effort));
        }
        if let Some(path) = file_path {
            let path = clean_text(path, PATH_MAX_CHARS);
            if !self.files.contains(&path) && self.files.len() < MAX_FILES_KEPT {
                self.files.push(path);
            }
        }
        if let Some(command) = command {
            if is_slow_command(command) {
                self.slow_commands = self.slow_commands.saturating_add(1);
                self.raise_floor(SLOW_COMMAND_FLOOR);
            }
            if self.commands.len() < MAX_COMMANDS_KEPT {
                self.commands.push(clean_text(command, COMMAND_MAX_CHARS));
            }
        }
    }

    pub fn note_subagent(&mut self) {
        self.subagents = self.subagents.saturating_add(1);
        self.raise_floor(SUBAGENT_FLOOR);
    }

    pub fn declare_long(&mut self) {
        self.declared_long = true;
        self.away_spent = false;
        self.retier();
    }

    /// Layer 2: a live signal puts a floor under the estimate.
    fn raise_floor(&mut self, floor: Duration) {
        self.floor = Some(self.floor.map_or(floor, |current| current.max(floor)));
        self.retier();
    }

    fn retier(&mut self) {
        let expected = match (self.estimate.map(|e| e.median), self.floor) {
            (Some(median), Some(floor)) => Some(median.max(floor)),
            (median, floor) => median.or(floor),
        };
        self.tier = if self.declared_long {
            Tier::Long
        } else {
            expected.map_or(Tier::Unknown, tier_of)
        };
    }

    /// The history line for this run. Marks it recorded so it is written once.
    pub fn record(&mut self) -> Option<RunRecord> {
        if std::mem::replace(&mut self.recorded, true) {
            return None;
        }
        let (outcome, ended) = match (&self.outcome, self.stopped_at) {
            (Some(Outcome::Done { .. }), Some(at)) => (RunOutcome::Done, at),
            (Some(Outcome::Error(_)), Some(at)) => (RunOutcome::Error, at),
            _ => (RunOutcome::Stale, self.last_event),
        };
        Some(RunRecord {
            run_id: self.id.clone(),
            project: self.project.clone(),
            started: self.started,
            duration: ended.saturating_duration_since(self.started),
            outcome,
            permission_mode: self.permission_mode.clone(),
            effort: self.effort.clone(),
            prompt_len: self.prompt_len,
            tools: self.tools,
            subagents: self.subagents,
            slow_commands: self.slow_commands,
            needed_input: self.needed_input,
            predicted_tier: self.tier,
            away: self.away_how,
            returned: self.return_how,
            snoozes: self.snoozes,
        })
    }
}

/// A short identifier-like value (a mode, an effort level, a project key).
fn label(raw: &str) -> String {
    clean_text(raw, LABEL_MAX_CHARS)
}
