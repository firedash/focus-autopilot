// SPDX-License-Identifier: Apache-2.0
//! The per-session state machine (design section 7, "Session transitions").

use std::time::{Duration, Instant};

use crate::config::Switching;
use crate::estimator::{Tier, away_threshold};
use crate::events::{
    AgentEvent, AppRef, COMMAND_MAX_CHARS, EventKind, MESSAGE_MAX_CHARS, PATH_MAX_CHARS,
    PROMPT_MAX_CHARS, clean_text,
};

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
/// Distinct touched files and commands remembered per run; the recap shows fewer.
const MAX_FILES_KEPT: usize = 64;
const MAX_COMMANDS_KEPT: usize = 3;
const ERROR_MAX_CHARS: usize = 80;
const NAME_MAX_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Running,
    NeedsInput,
    Done,
}

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

/// One prompt and everything Claude does until it stops. Held in memory only.
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub id: Option<String>,
    pub started: Instant,
    pub prompt: String,
    pub cwd: String,
    /// The window the prompt was sent from. `None` means focusd never switches for this run.
    pub home: Option<AppRef>,
    pub tier: Tier,
    pub tools: u32,
    pub subagents: u32,
    pub files: Vec<String>,
    pub commands: Vec<String>,
    /// An away switch happened or was cancelled; there won't be another in this run.
    pub away_spent: bool,
    pub need: Option<Need>,
    pub outcome: Option<Outcome>,
    /// When the session last entered `NeedsInput` or `Done`.
    pub waiting_since: Instant,
    /// The user is back, or was never away, for the current wait.
    pub return_handled: bool,
    input_after_needs: bool,
    last_event: Instant,
    stopped_at: Option<Instant>,
}

impl Run {
    fn new(ev: &AgentEvent, prompt: &str, home: Option<AppRef>, now: Instant) -> Self {
        Self {
            id: ev.prompt_id.clone(),
            started: now,
            prompt: clean_text(prompt, PROMPT_MAX_CHARS),
            cwd: clean_text(&ev.cwd, PATH_MAX_CHARS),
            home,
            tier: Tier::default(),
            tools: 0,
            subagents: 0,
            files: Vec::new(),
            commands: Vec::new(),
            away_spent: false,
            need: None,
            outcome: None,
            waiting_since: now,
            return_handled: false,
            input_after_needs: false,
            last_event: now,
            stopped_at: None,
        }
    }

    fn note_tool(&mut self, command: Option<&String>, file_path: Option<&String>) {
        self.tools = self.tools.saturating_add(1);
        if let Some(path) = file_path {
            let path = clean_text(path, PATH_MAX_CHARS);
            if !self.files.contains(&path) && self.files.len() < MAX_FILES_KEPT {
                self.files.push(path);
            }
        }
        if let Some(command) = command
            && self.commands.len() < MAX_COMMANDS_KEPT
        {
            self.commands.push(clean_text(command, COMMAND_MAX_CHARS));
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub state: SessionState,
    pub run: Run,
}

impl Session {
    /// Creates the session from its first event. `home` is the frontmost window if it's a home
    /// app; it is used only when the event is a prompt.
    pub fn first(ev: &AgentEvent, home: Option<AppRef>, now: Instant) -> (Self, Change) {
        let mut session = Self {
            state: SessionState::Running,
            run: Run::new(ev, "", None, now),
        };
        let change = match session.apply(ev, home, now) {
            // focusd started mid-run: there is no home window, so it never switches.
            Change::None => Change::Adopted,
            change => change,
        };
        (session, change)
    }

    pub fn apply(&mut self, ev: &AgentEvent, home: Option<AppRef>, now: Instant) -> Change {
        if let EventKind::UserPromptSubmit { prompt } = &ev.kind {
            self.run = Run::new(ev, prompt, home, now);
            self.state = SessionState::Running;
            return Change::RunStarted;
        }
        if self.is_from_another_run(ev) {
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
                self.run = Run::new(ev, "", None, now);
                self.state = SessionState::Running;
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
        stop_hook_active || recent
    }

    fn on_running(&mut self, ev: &AgentEvent, now: Instant) -> Change {
        match &ev.kind {
            EventKind::PreToolUse {
                tool_name,
                command,
                file_path,
            } => {
                self.run.note_tool(command.as_ref(), file_path.as_ref());
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
                self.run.subagents = self.run.subagents.saturating_add(1);
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
        eligible.then(|| self.run.started + away_threshold(self.run.tier, switching))
    }

    /// Times at which `tick` has something to do for this session.
    pub fn deadlines(&self, now: Instant, switching: &Switching) -> impl Iterator<Item = Instant> {
        let stale = (self.state == SessionState::Running)
            .then(|| self.run.last_event + switching.stale_after);
        let debounce = (self.state == SessionState::NeedsInput && self.run.home.is_some())
            .then(|| self.run.waiting_since + switching.needs_input_debounce)
            .filter(|at| *at > now);
        let threshold = self.away_threshold_at(switching).filter(|at| *at > now);
        [stale, debounce, threshold].into_iter().flatten()
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
