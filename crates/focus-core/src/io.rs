// SPDX-License-Identifier: Apache-2.0
//! The core contract (design section 5): the inputs the shell passes in and the effects it
//! must carry out. The shell owns every side effect.

use std::time::{Duration, Instant};

use crate::config::Config;
use crate::estimator::RunSample;
use crate::events::{AgentEvent, AppRef, Observation, UserAction};
use crate::recap::RecapCard;
use crate::run::RunRecord;
use crate::toast::Toast;

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A normalized Claude Code hook payload.
    Hook(AgentEvent),
    /// Something the shell read from the platform.
    Observed(Observation),
    User(UserAction),
    Config(Box<Config>),
    /// Past runs read from `runs.jsonl` at startup, oldest first. Replaces what the core holds.
    History(Vec<RunSample>),
    /// The reply to `Effect::SummarizeRecap`, if it arrived in time.
    Summary {
        id: u64,
        text: String,
    },
    /// The CLI couldn't be run or failed; show the basic card now rather than at the timeout.
    SummaryFailed {
        id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Show this toast, replacing any other. It must not take keyboard focus.
    ShowToast(Toast),
    HideToast,
    /// Ask the OS to bring `target` to the front, read the frontmost app `verify_after`
    /// later, and report `Observation::Activation`.
    Activate {
        target: AppRef,
        verify_after: Duration,
        /// For a return, the project folder of the session being returned to, so an editor can
        /// be asked for the window that has it open. `None` for an away switch or no folder.
        folder: Option<String>,
    },
    /// Read a value now and report it as an `Observation`.
    Probe(ProbeKind),
    ShowRecap(RecapCard),
    PlaySound(Sound),
    /// Append this run to the history file.
    AppendRun(RunRecord),
    /// Opt-in: run the local Claude Code CLI with `prompt` on stdin and report its reply as
    /// `Input::Summary` with the same id, unless it takes longer than `timeout`.
    SummarizeRecap {
        id: u64,
        prompt: String,
        timeout: Duration,
    },
    Log(LogEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Idle,
    Frontmost,
    RunningApps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Return,
}

/// An event name and a session id. Never prompt text, messages, commands or paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEvent {
    pub name: &'static str,
    pub session: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseFor {
    Minutes(u32),
    UntilResumed,
}

/// The tray's status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running {
        elapsed: Duration,
    },
    Away,
    /// `until` is `None` when paused until resumed, or when `enabled` is false.
    Paused {
        until: Option<Instant>,
    },
}

/// Why an expected switch isn't happening, for the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    NoAwayAppRunning,
}
