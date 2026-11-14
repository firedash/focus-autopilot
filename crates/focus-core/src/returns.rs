// SPDX-License-Identifier: Apache-2.0
//! The return half of the switch controller, plus the inputs that act on whichever toast is
//! showing: idle readings during the input-quiet wait, activation results, and toast actions.

use std::time::{Duration, Instant};

use crate::engine::{Core, Pending, log};
use crate::events::AppRef;
use crate::io::{Effect, Sound};
use crate::mode::{Mode, Phase, QUIET_SLACK, ReturnStep};
use crate::recap;
use crate::session::WaitKind;
use crate::toast;

/// Coming back on your own shows the recap only after at least this long away.
const MIN_AWAY_FOR_RECAP: Duration = Duration::from_secs(20);

impl Core {
    pub(crate) fn advance_idle(&mut self, now: Instant, fx: &mut Vec<Effect>) -> Mode {
        if !self.automation_on() || self.came_home(now, fx) {
            return Mode::Idle;
        }
        let Some(next) = self.pending(now).into_iter().next() else {
            return Mode::Idle;
        };
        if self.front_is(&next.home) {
            // The user is already looking at it: nothing to do, now or later.
            self.mark_handled(&next.session);
            return Mode::Idle;
        }
        self.start_return(next, now, fx)
    }

    /// The user left the away app for a home app on their own.
    fn came_home(&mut self, now: Instant, fx: &mut Vec<Effect>) -> bool {
        let at_home = self
            .frontmost
            .as_ref()
            .is_some_and(|front| self.is_home(front));
        if !at_home {
            return false;
        }
        let Some(away) = self.away.take() else {
            return false;
        };
        self.touched = true;
        fx.push(log("came_back", &away.session));
        let waiting_here: Vec<String> = self
            .pending(now)
            .into_iter()
            .filter(|p| self.front_is(&p.home))
            .map(|p| p.session)
            .collect();
        if now.saturating_duration_since(away.since) >= MIN_AWAY_FOR_RECAP {
            let session = waiting_here.first().unwrap_or(&away.session);
            self.show_recap(session, now, fx);
        }
        for session in &waiting_here {
            self.mark_handled(session);
        }
        true
    }

    fn start_return(&mut self, next: Pending, now: Instant, fx: &mut Vec<Effect>) -> Mode {
        if self.cfg.ui.sound_on_return {
            fx.push(Effect::PlaySound(Sound::Return));
        }
        // A Stop right after an away switch gets the prompt, so the user isn't flipped back.
        let debounce = self.cfg.switching.debounce;
        let just_switched_away = next.kind == WaitKind::Done
            && self
                .last_away_switch
                .is_some_and(|at| now.saturating_duration_since(at) < debounce);
        if self.on_away_app() && !just_switched_away {
            self.show_return_countdown(next.session, next.home, 0, now, fx)
        } else {
            self.show_return_prompt(next.session, next.home, 0, fx)
        }
    }

    fn show_return_countdown(
        &self,
        session: String,
        home: AppRef,
        snoozes: u32,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        let Some(waiting) = self.sessions.get(&session) else {
            return Mode::Idle;
        };
        let length = self.cfg.countdown.length;
        fx.push(Effect::ShowToast(toast::return_countdown(
            waiting, &home, length,
        )));
        fx.push(log("return_countdown", &session));
        let step = ReturnStep::Countdown(Phase::Counting { ends: now + length });
        Mode::Return {
            session,
            home,
            snoozes,
            step,
        }
    }

    fn show_return_prompt(
        &self,
        session: String,
        home: AppRef,
        snoozes: u32,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        let Some(waiting) = self.sessions.get(&session) else {
            return Mode::Idle;
        };
        fx.push(Effect::ShowToast(toast::return_prompt(waiting)));
        fx.push(log("return_prompt", &session));
        Mode::Return {
            session,
            home,
            snoozes,
            step: ReturnStep::Prompt { verifying: None },
        }
    }

    pub(crate) fn advance_return(
        &mut self,
        session: String,
        home: AppRef,
        snoozes: u32,
        step: ReturnStep,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        let mode = Mode::Return {
            session,
            home,
            snoozes,
            step,
        };
        let Mode::Return {
            session,
            home,
            snoozes,
            step,
        } = mode.clone()
        else {
            return mode;
        };
        let still_waiting = self.automation_on()
            && self
                .sessions
                .get(&session)
                .is_some_and(|s| s.waiting(now, &self.cfg.switching).is_some());
        if !still_waiting || self.front_is(&home) {
            if mode.toast_visible() {
                fx.push(Effect::HideToast);
            }
            if still_waiting {
                self.complete_return(&session, now, fx);
            }
            return Mode::Idle;
        }
        let quiet_max = self.cfg.switching.input_quiet_max;
        match step {
            ReturnStep::Countdown(Phase::Counting { ends }) if now >= ends => {
                if self.on_away_app() {
                    let step = ReturnStep::Countdown(Phase::Quiet { since: now });
                    Mode::Return {
                        session,
                        home,
                        snoozes,
                        step,
                    }
                } else {
                    self.show_return_prompt(session, home, snoozes, fx)
                }
            }
            ReturnStep::Countdown(Phase::Quiet { since })
                if now >= since + quiet_max + QUIET_SLACK =>
            {
                self.show_return_prompt(session, home, snoozes, fx)
            }
            ReturnStep::Countdown(Phase::Verifying { until })
            | ReturnStep::Prompt {
                verifying: Some(until),
            } if now >= until => self.show_return_prompt(session, home, snoozes, fx),
            ReturnStep::Snoozed { until } if now >= until => {
                if snoozes < self.cfg.countdown.max_snoozes && self.on_away_app() {
                    self.show_return_countdown(session, home, snoozes, now, fx)
                } else {
                    self.show_return_prompt(session, home, snoozes, fx)
                }
            }
            _ => mode,
        }
    }

    /// The user is back at the home window: show the recap and settle every waiting session.
    fn complete_return(&mut self, session: &str, now: Instant, fx: &mut Vec<Effect>) {
        fx.push(log("returned", session));
        self.show_recap(session, now, fx);
        for pending in self.pending(now) {
            self.mark_handled(&pending.session);
        }
        self.away = None;
    }

    fn show_recap(&self, session: &str, now: Instant, fx: &mut Vec<Effect>) {
        let Some(shown) = self.sessions.get(session) else {
            return;
        };
        let others = self
            .pending(now)
            .iter()
            .filter(|p| p.session != session)
            .count();
        fx.push(Effect::ShowRecap(recap::build(shown, others)));
    }

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
                    self.away_verified(session, now, fx);
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
                    self.away_verified(session, now, fx);
                    Mode::Idle
                } else {
                    fx.push(Effect::ShowToast(toast::away_prompt(&target)));
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
                    ReturnStep::Countdown(Phase::Verifying { .. })
                    | ReturnStep::Prompt { verifying: Some(_) },
            } => {
                if verified {
                    self.complete_return(&session, now, fx);
                    Mode::Idle
                } else {
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
