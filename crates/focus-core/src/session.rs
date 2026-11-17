// SPDX-License-Identifier: Apache-2.0
//! The per-session state machine (design section 7, "Session transitions").

use std::time::{Duration, Instant};

use crate::config::Switching;
use crate::estimator::away_threshold;
use crate::events::{
    AgentEvent, COMMAND_MAX_CHARS, EventKind, MESSAGE_MAX_CHARS, PATH_MAX_CHARS, clean_text,
};
use crate::run::{Need, Outcome, Run, RunRecord, Start};

/// A user who comes back to a run's home window while it is going is left there at least this
/// long before an away switch: long enough to look at Claude, short enough that a detour
/// through another app does not cost the whole away threshold again.
pub(crate) const BACK_AT_HOME_GRACE: Duration = Duration::from_secs(8);
/// An event for a finished run within this window means another Stop hook made Claude continue.
pub(crate) const CONTINUATION_WINDOW: Duration = Duration::from_secs(60);
/// A `PreToolUse` for one of these means Claude is waiting for the user.
const QUESTION_TOOL: &str = "AskUserQuestion";
const PLAN_TOOL: &str = "ExitPlanMode";
const NEEDS_INPUT_NOTIFICATIONS: [&str; 3] = [
    "permission_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
];
const ERROR_MAX_CHARS: usize = 80;
const NAME_MAX_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Running,
    NeedsInput,
    Done,
}

/// Why a session is waiting for the user to come back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WaitKind {
    NeedsInput,
    Done,
}

/// What an event did to a session; used for logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    None,
    Ignored,
    RunStarted,
    Adopted,
    EnteredNeedsInput,
    LeftNeedsInput,
    Finished,
    Continued,
}

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub state: SessionState,
    pub run: Run,
    /// The history line of a run that was just replaced, for the core to emit.
    pub outbox: Option<RunRecord>,
}

impl Session {
    /// Creates the session from its first event.
    pub fn first(ev: &AgentEvent, start: &Start, now: Instant) -> (Self, Change) {
        let run = Run::new(ev, start, now);
        let mut session = Self {
            state: SessionState::Running,
            run,
            outbox: None,
        };
        if matches!(ev.kind, EventKind::UserPromptSubmit { .. }) {
            return (session, Change::RunStarted);
        }
        // focusd started mid-run: there is no home window, so it never switches.
        let change = match session.on_running(ev, now) {
            Change::None => Change::Adopted,
            change => change,
        };
        (session, change)
    }

    pub fn apply(&mut self, ev: &AgentEvent, start: &Start, now: Instant) -> Change {
        if matches!(ev.kind, EventKind::UserPromptSubmit { .. }) {
            self.replace_run(Run::new(ev, start, now));
            return Change::RunStarted;
        }
        if self.is_from_another_run(ev) {
            return Change::Ignored;
        }
        // A straggler after the run ended, such as the end of a sub-agent reported after
        // Claude's own Stop: it says nothing about Claude working again, so the finished run
        // and the return in progress for it are left alone.
        let ended = matches!(self.state, SessionState::Done | SessionState::Idle);
        if ended && !shows_claude_working(&ev.kind) {
            return Change::Ignored;
        }
        let entry = self.reopen(ev, now);
        self.run.last_event = now;
        let change = match self.state {
            SessionState::NeedsInput => self.on_needs_input(ev, now),
            _ => self.on_running(ev, now),
        };
        if change == Change::None {
            entry
        } else {
            change
        }
    }

    fn replace_run(&mut self, run: Run) {
        self.outbox = self.run.record();
        self.run = run;
        self.state = SessionState::Running;
    }

    fn is_from_another_run(&self, ev: &AgentEvent) -> bool {
        matches!((&ev.prompt_id, &self.run.id), (Some(theirs), Some(ours)) if theirs != ours)
    }

    /// Brings an `Idle` or `Done` session back to `Running` before the event is applied.
    fn reopen(&mut self, ev: &AgentEvent, now: Instant) -> Change {
        match self.state {
            SessionState::Running | SessionState::NeedsInput => Change::None,
            SessionState::Done if self.is_continuation(ev, now) => {
                // Another Stop hook made Claude continue: no second away switch in this run.
                self.state = SessionState::Running;
                self.run.outcome = None;
                self.run.stopped_at = None;
                self.run.away_spent = true;
                Change::Continued
            }
            // A wake-up long after the run closed (a stale run, a background task): track it,
            // but with no home window, so it never switches.
            SessionState::Done | SessionState::Idle => {
                self.replace_run(Run::new(ev, &Start::default(), now));
                Change::Adopted
            }
        }
    }

    fn is_continuation(&self, ev: &AgentEvent, now: Instant) -> bool {
        let stop_hook_active = matches!(
            ev.kind,
            EventKind::Stop {
                stop_hook_active: true,
                ..
            }
        );
        let recent = self
            .run
            .stopped_at
            .is_some_and(|at| now.saturating_duration_since(at) <= CONTINUATION_WINDOW);
        (stop_hook_active || recent) && !self.run.recorded
    }

    fn on_running(&mut self, ev: &AgentEvent, now: Instant) -> Change {
        match &ev.kind {
            EventKind::PreToolUse {
                tool_name,
                command,
                file_path,
                effort,
            } => {
                self.run
                    .note_tool(command.as_ref(), file_path.as_ref(), effort.as_ref());
                match tool_name.as_str() {
                    QUESTION_TOOL => self.enter_needs_input(Need::Question, now),
                    PLAN_TOOL => self.enter_needs_input(Need::Plan, now),
                    _ => Change::None,
                }
            }
            EventKind::PermissionRequest {
                tool_name,
                command,
                file_path,
            } => {
                let need = approval(tool_name, command.as_ref(), file_path.as_ref());
                self.enter_needs_input(need, now)
            }
            EventKind::Elicitation { mcp_server_name } => {
                let server = clean_text(mcp_server_name, NAME_MAX_CHARS);
                self.enter_needs_input(Need::Mcp(server), now)
            }
            EventKind::Notification { notification_type }
                if NEEDS_INPUT_NOTIFICATIONS.contains(&notification_type.as_str()) =>
            {
                self.enter_needs_input(Need::Input, now)
            }
            EventKind::SubagentStart => {
                self.run.note_subagent();
                Change::None
            }
            EventKind::Stop {
                last_assistant_message,
                background_tasks,
                ..
            } => {
                let message = clean_text(last_assistant_message, MESSAGE_MAX_CHARS);
                self.finish(
                    Outcome::Done {
                        message,
                        background_tasks: *background_tasks,
                    },
                    now,
                )
            }
            EventKind::StopFailure { error } => {
                self.finish(Outcome::Error(clean_text(error, ERROR_MAX_CHARS)), now)
            }
            _ => Change::None,
        }
    }

    fn on_needs_input(&mut self, ev: &AgentEvent, now: Instant) -> Change {
        match ev.kind {
            // Claude moved on, so the user answered (or a hook did).
            EventKind::PreToolUse { .. }
            | EventKind::PermissionDenied
            | EventKind::SubagentStart => {
                self.leave_needs_input();
                match self.on_running(ev, now) {
                    Change::None => Change::LeftNeedsInput,
                    change => change,
                }
            }
            EventKind::Stop { .. } | EventKind::StopFailure { .. } => self.on_running(ev, now),
            // A second request, or the backup notification for the first: still waiting.
            _ => Change::None,
        }
    }

    fn enter_needs_input(&mut self, need: Need, now: Instant) -> Change {
        self.state = SessionState::NeedsInput;
        self.run.need = Some(need);
        self.run.needed_input = true;
        self.run.waiting_since = now;
        self.run.return_handled = false;
        self.run.input_after_needs = false;
        Change::EnteredNeedsInput
    }

    fn leave_needs_input(&mut self) {
        self.state = SessionState::Running;
        self.run.need = None;
    }

    fn finish(&mut self, outcome: Outcome, now: Instant) -> Change {
        self.state = SessionState::Done;
        self.run.need = None;
        self.run.outcome = Some(outcome);
        self.run.stopped_at = Some(now);
        self.run.waiting_since = now;
        self.run.return_handled = false;
        Change::Finished
    }

    /// Closes a run that has gone quiet (an interrupt or a crash). Returns true if it did.
    pub fn expire_stale(&mut self, now: Instant, stale_after: Duration) -> bool {
        let stale = self.state == SessionState::Running
            && now.saturating_duration_since(self.run.last_event) >= stale_after;
        if stale {
            self.state = SessionState::Idle;
        }
        stale
    }

    /// The resume heuristic: user input after entering `NeedsInput`, then `resume_idle` of
    /// quiet with no agent event, most likely means a long tool was approved.
    pub fn observe_idle(&mut self, idle: Duration, now: Instant, resume_idle: Duration) -> bool {
        if self.state != SessionState::NeedsInput {
            return false;
        }
        if now
            .checked_sub(idle)
            .is_some_and(|last_input| last_input > self.run.waiting_since)
        {
            self.run.input_after_needs = true;
        }
        let resumed = self.run.input_after_needs && idle >= resume_idle;
        if resumed {
            self.leave_needs_input();
        }
        resumed
    }

    /// Whether this session should bring the user back, and why.
    pub fn waiting(&self, now: Instant, switching: &Switching) -> Option<WaitKind> {
        self.run.home.as_ref()?;
        if self.run.return_handled {
            return None;
        }
        match self.state {
            SessionState::Done => Some(WaitKind::Done),
            // A needs-input that clears within the debounce was approved by a hook or auto mode.
            SessionState::NeedsInput
                if switching.return_on_needs_input
                    && now >= self.run.waiting_since + switching.needs_input_debounce =>
            {
                Some(WaitKind::NeedsInput)
            }
            _ => None,
        }
    }

    /// When the away threshold is reached, for a run that could still get an away switch.
    pub fn away_threshold_at(&self, switching: &Switching) -> Option<Instant> {
        let eligible =
            self.state == SessionState::Running && self.run.home.is_some() && !self.run.away_spent;
        if !eligible {
            return None;
        }
        if self.run.declared_long {
            return Some(self.run.started);
        }
        let old_enough = self.run.started + away_threshold(self.run.tier, switching);
        Some(
            self.back_at_home_until()
                .map_or(old_enough, |at| at.max(old_enough)),
        )
    }

    /// Until when a user who has just come back to this run's home window is left there.
    pub fn back_at_home_until(&self) -> Option<Instant> {
        self.run.back_at_home.map(|at| at + BACK_AT_HOME_GRACE)
    }

    /// When a finished run can no longer be continued by a Stop hook and nobody is waiting
    /// on it, so its history line can be written.
    fn record_due_at(&self) -> Option<Instant> {
        let settled = self.run.home.is_none() || self.run.return_handled;
        match self.state {
            SessionState::Idle => Some(self.run.last_event),
            SessionState::Done if settled => self.run.stopped_at.map(|at| at + CONTINUATION_WINDOW),
            _ => None,
        }
        .filter(|_| !self.run.recorded)
    }

    /// The history line for a run that has closed for good, once.
    pub fn take_record(&mut self, now: Instant) -> Option<RunRecord> {
        if self.record_due_at().is_some_and(|at| now >= at) {
            self.run.record()
        } else {
            None
        }
    }

    /// Times at which `tick` has something to do for this session.
    pub fn deadlines(&self, now: Instant, switching: &Switching) -> impl Iterator<Item = Instant> {
        let stale = (self.state == SessionState::Running)
            .then(|| self.run.last_event + switching.stale_after);
        let debounce = (self.state == SessionState::NeedsInput && self.run.home.is_some())
            .then(|| self.run.waiting_since + switching.needs_input_debounce)
            .filter(|at| *at > now);
        let threshold = self.away_threshold_at(switching).filter(|at| *at > now);
        [stale, debounce, threshold, self.record_due_at()]
            .into_iter()
            .flatten()
    }
}

/// True for an event that can only happen while Claude is working or asking: the ones that
/// may reopen a finished run. The end of a sub-agent, a denial and an ordinary notification
/// can all arrive late, after the Stop of the run they belong to.
fn shows_claude_working(kind: &EventKind) -> bool {
    match kind {
        EventKind::UserPromptSubmit { .. }
        | EventKind::PreToolUse { .. }
        | EventKind::PermissionRequest { .. }
        | EventKind::Elicitation { .. }
        | EventKind::SubagentStart
        | EventKind::Stop { .. }
        | EventKind::StopFailure { .. } => true,
        EventKind::Notification { notification_type } => {
            NEEDS_INPUT_NOTIFICATIONS.contains(&notification_type.as_str())
        }
        EventKind::SubagentStop | EventKind::PermissionDenied | EventKind::SessionEnd => false,
    }
}

fn approval(tool_name: &str, command: Option<&String>, file_path: Option<&String>) -> Need {
    let what = match (command, file_path) {
        (Some(command), _) => format!("run {}", clean_text(command, COMMAND_MAX_CHARS)),
        (None, Some(path)) => format!("edit {}", clean_text(path, PATH_MAX_CHARS)),
        (None, None) => format!("use {}", clean_text(tool_name, NAME_MAX_CHARS)),
    };
    Need::Approval(what)
}
