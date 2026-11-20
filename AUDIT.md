// SPDX-License-Identifier: Apache-2.0
//! Inputs that act on whichever switch is in progress: idle readings during the input-quiet
//! wait, activation results from the shell, and toast actions from the user.

use std::time::{Duration, Instant};

use crate::engine::{Core, log};
use crate::io::Effect;
use crate::mode::{Mode, Phase, ReturnStep};
use crate::run::{AwayHow, ReturnHow};
use crate::toast;

impl Core {
    /// The input-quiet guard: once a countdown has ended, switch only after a moment with no
    /// keyboard or mouse input; if input continues, the prompt replaces the switch.
    pub(crate) fn quiet_check(&mut self, idle: Duration, now: Instant, fx: &mut Vec<Effect>) {
        let quiet = idle >= self.cfg.switching.input_quiet;
        let quiet_max = self.cfg.switching.input_quiet_max;
        let mode = std::mem::replace(&mut self.mode, Mode::Idle);
        self.mode = match mode {
            Mode::AwayCountdown {
                session,
                target,
                phase: Phase::Quiet { since },
            } => {
                if quiet {
                    let until = self.request_activation(&target, now, fx);
                    Mode::AwayCountdown {
                        session,
                        target,
                        phase: Phase::Verifying { until },
                    }
                } else if now >= since + quiet_max {
                    self.show_away_prompt(session, target, now, fx)
                } else {
                    Mode::AwayCountdown {
                        session,
                        target,
                        phase: Phase::Quiet { since },
                    }
                }
            }
            Mode::Return {
                session,
                home,
                snoozes,
                step: ReturnStep::Countdown(Phase::Quiet { since }),
            } => {
                if quiet {
                    let until = self.request_activation(&home, now, fx);
                    let step = ReturnStep::Countdown(Phase::Verifying { until });
                    Mode::Return {
                        session,
                        home,
                        snoozes,
                        step,
                    }
                } else if now >= since + quiet_max {
                    fx.push(log("asked_because_input_never_paused", &session));
                    self.show_return_prompt(session, home, snoozes, fx)
                } else {
                    let step = ReturnStep::Countdown(Phase::Quiet { since });
                    Mode::Return {
                        session,
                        home,
                        snoozes,
                        step,
                    }
                }
            }
            mode => mode,
        };
    }

    /// The shell's report on the last activation request.
    pub(crate) fn on_activation(&mut self, verified: bool, now: Instant, fx: &mut Vec<Effect>) {
        let mode = std::mem::replace(&mut self.mode, Mode::Idle);
        self.mode = match mode {
            Mode::AwayCountdown {
                session,
                target,
                phase: Phase::Verifying { .. },
            } => {
                if verified {
                    self.away_verified(session, AwayHow::Auto, now, fx);
                    Mode::Idle
                } else {
                    self.show_away_prompt(session, target, now, fx)
                }
            }
            Mode::AwayPrompt {
                session,
                target,
                until,
                verifying: Some(_),
            } => {
                if verified {
                    self.away_verified(session, AwayHow::Prompt, now, fx);
                    Mode::Idle
                } else {
                    fx.push(Effect::ShowToast(toast::away_prompt(
                        &target,
                        self.agent_name(&session),
                    )));
                    Mode::AwayPrompt {
                        session,
                        target,
                        until,
                        verifying: None,
                    }
                }
            }
            Mode::Return {
                session,
                home,
                snoozes,
                step:
                    step @ (ReturnStep::Countdown(Phase::Verifying { .. })
                    | ReturnStep::Prompt { verifying: Some(_) }),
            } => {
                if verified {
                    let how = return_how(&step).unwrap_or(ReturnHow::Auto);
                    self.complete_return(&session, how, now, fx);
                    Mode::Idle
                } else {
                    fx.push(log("asked_because_the_switch_was_refused", &session));
                    self.show_return_prompt(session, home, snoozes, fx)
                }
            }
            mode => mode,
        };
    }

    /// A click on the toast or the focus key: the thing that won't happen on its own.
    pub(crate) fn on_toast_action(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        let mode = std::mem::replace(&mut self.mode, Mode::Idle);
        self.mode = match mode {
            // "Not now": no away switch for the rest of this run.
            Mode::AwayCountdown {
                session,
                phase: Phase::Counting { .. } | Phase::Quiet { .. },
                ..
            } => {
                self.spend_away(&session);
                fx.push(Effect::HideToast);
                fx.push(log("away_cancelled", &session));
                self.set_away_how(&session, AwayHow::Cancelled);
                Mode::Idle
            }
            // "Switch now".
            Mode::AwayPrompt {
                session,
                target,
                until,
                verifying: None,
            } => {
                let deadline = self.request_activation(&target, now, fx);
                Mode::AwayPrompt {
                    session,
                    target,
                    until,
                    verifying: Some(deadline),
                }
            }
            // "Snooze".
            Mode::Return {
                session,
                home,
                snoozes,
                step: ReturnStep::Countdown(Phase::Counting { .. } | Phase::Quiet { .. }),
            } => {
                fx.push(Effect::HideToast);
                fx.push(log("return_snoozed", &session));
                if let Some(s) = self.sessions.get_mut(&session) {
                    s.run.snoozes = s.run.snoozes.saturating_add(1);
                }
                let step = ReturnStep::Snoozed {
                    until: now + self.cfg.countdown.snooze,
                };
                Mode::Return {
                    session,
                    home,
                    snoozes: snoozes + 1,
                    step,
                }
            }
            // "Go back now".
            Mode::Return {
                session,
                home,
                snoozes,
                step: ReturnStep::Prompt { verifying: None },
            } => {
                let deadline = self.request_activation(&home, now, fx);
                let step = ReturnStep::Prompt {
                    verifying: Some(deadline),
                };
                Mode::Return {
                    session,
                    home,
                    snoozes,
                    step,
                }
            }
            mode => mode,
        };
    }
}

/// How the return happened, if the step shows focusd had just asked for the switch.
pub(crate) fn return_how(step: &ReturnStep) -> Option<ReturnHow> {
    match step {
        ReturnStep::Countdown(Phase::Verifying { .. }) => Some(ReturnHow::Auto),
        ReturnStep::Prompt { verifying: Some(_) } => Some(ReturnHow::Prompt),
        _ => None,
    }
}
