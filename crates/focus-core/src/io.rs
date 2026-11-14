// SPDX-License-Identifier: Apache-2.0
//! The core contract (design section 5): the inputs the shell passes in and the effects it
//! must carry out. The shell owns every side effect.

use std::time::{Duration, Instant};

use crate::config::Config;
use crate::events::{AgentEvent, AppRef, Observation, UserAction};
use crate::recap::RecapCard;
use crate::toast::Toast;

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A normalized Claude Code hook payload.
    Hook(AgentEvent),
    /// Something the shell read from the platform.
    Observed(Observation),
    User(UserAction),
    Config(Box<Config>),
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
    },
    /// Read a value now and report it as an `Observation`.
    Probe(ProbeKind),
    ShowRecap(RecapCard),
    PlaySound(Sound),
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
