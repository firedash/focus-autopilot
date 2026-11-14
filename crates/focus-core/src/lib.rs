// SPDX-License-Identifier: Apache-2.0
//! Every decision Focus Autopilot makes lives in this crate.
//!
//! [`Core`] is a deterministic state machine: inputs in, effects out. It does no I/O, calls no
//! OS API and reads no clock; the shell passes the time in and executes the effects. That keeps
//! each decision testable with recorded traces and a fake clock, and keeps the code an auditor
//! has to trust in one place.
#![forbid(unsafe_code)]

mod away;
mod config;
mod engine;
mod estimator;
mod events;
mod guards;
mod io;
mod mode;
mod recap;
mod returns;
mod session;
mod toast;

pub use config::{
    Away, Config, ConfigError, Countdown, History, Home, Hotkeys, Ignore, Recap, RecapMode,
    Receiver, Switching, Ui,
};
pub use engine::Core;
pub use estimator::{Tier, away_threshold};
pub use events::{
    AgentEvent, AppRef, COMMAND_MAX_CHARS, EventKind, MESSAGE_MAX_CHARS, Observation,
    PATH_MAX_CHARS, PROMPT_MAX_CHARS, UserAction, clean_text,
};
pub use guards::GuardFail;
pub use io::{Effect, Input, LogEvent, Note, PauseFor, ProbeKind, Sound, Status};
pub use recap::RecapCard;
pub use session::SessionState;
pub use toast::{Toast, ToastKind};
