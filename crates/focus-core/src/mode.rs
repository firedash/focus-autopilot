// SPDX-License-Identifier: Apache-2.0
//! The switch controller's state (design section 7, "Switch controller"). Focus is global, so
//! there is one controller however many sessions run, and it shows at most one toast.
//!
//! The design's Home and Away states are both `Mode::Idle` here; `Core::away` records whether
//! focusd put the user in an away app.

use std::time::{Duration, Instant};

use crate::events::AppRef;

/// How long past `input_quiet_max` to wait for an idle answer before giving up on the shell.
pub(crate) const QUIET_SLACK: Duration = Duration::from_secs(1);
/// How long past `verify_after` to wait for the shell's verification before treating the
/// switch as refused.
pub(crate) const VERIFY_GRACE: Duration = Duration::from_secs(2);

/// The steps every automatic switch goes through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The countdown toast is showing.
    Counting { ends: Instant },
    /// The countdown ended; waiting for a moment without keyboard or mouse input.
    Quiet { since: Instant },
    /// Activation was requested; waiting to hear whether the OS allowed it.
    Verifying { until: Instant },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReturnStep {
    Countdown(Phase),
    Snoozed {
        until: Instant,
    },
    /// The fallback prompt. `verifying` is set after a click, while the switch is checked.
    Prompt {
        verifying: Option<Instant>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Idle,
    AwayCountdown {
        session: String,
        target: AppRef,
        phase: Phase,
    },
    AwayPrompt {
        session: String,
        target: AppRef,
        until: Instant,
        verifying: Option<Instant>,
    },
    Return {
        session: String,
        home: AppRef,
        snoozes: u32,
        step: ReturnStep,
    },
}

impl Mode {
    pub fn toast_visible(&self) -> bool {
        match self {
            Mode::Idle => false,
            Mode::AwayCountdown { phase, .. } => !matches!(phase, Phase::Verifying { .. }),
            Mode::AwayPrompt { verifying, .. } => verifying.is_none(),
            Mode::Return { step, .. } => match step {
                ReturnStep::Countdown(phase) => !matches!(phase, Phase::Verifying { .. }),
                ReturnStep::Snoozed { .. } => false,
                ReturnStep::Prompt { verifying } => verifying.is_none(),
            },
        }
    }

    pub fn in_quiet(&self) -> bool {
        matches!(
            self,
            Mode::AwayCountdown {
                phase: Phase::Quiet { .. },
                ..
            } | Mode::Return {
                step: ReturnStep::Countdown(Phase::Quiet { .. }),
                ..
            }
        )
    }

    /// When `tick` next has something to do for this mode.
    pub fn deadline(&self, quiet_max: Duration) -> Option<Instant> {
        let phase_deadline = |phase: &Phase| match phase {
            Phase::Counting { ends } => *ends,
            Phase::Quiet { since } => *since + quiet_max + QUIET_SLACK,
            Phase::Verifying { until } => *until,
        };
        match self {
            Mode::Idle => None,
            Mode::AwayCountdown { phase, .. } => Some(phase_deadline(phase)),
            Mode::AwayPrompt {
                until, verifying, ..
            } => Some(verifying.unwrap_or(*until)),
            Mode::Return { step, .. } => match step {
                ReturnStep::Countdown(phase) => Some(phase_deadline(phase)),
                ReturnStep::Snoozed { until } => Some(*until),
                ReturnStep::Prompt { verifying } => *verifying,
            },
        }
    }
}

/// Set while the user is in an away app that focusd switched them to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AwayInfo {
    pub since: Instant,
    pub session: String,
}
