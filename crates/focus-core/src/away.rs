// SPDX-License-Identifier: Apache-2.0
//! The away half of the switch controller: countdown, input-quiet wait, activation,
//! verification, and the fallback prompt when the OS refuses.

use std::time::{Duration, Instant};

use crate::engine::{Core, log};
use crate::events::AppRef;
use crate::guards::GuardFail;
use crate::io::{Effect, Note};
use crate::mode::{AwayInfo, Mode, Phase, QUIET_SLACK, VERIFY_GRACE};
use crate::run::AwayHow;
use crate::session::SessionState;
use crate::toast;

/// Apps flagged immersive get at least this long a countdown (backlog C4).
const IMMERSIVE_COUNTDOWN: Duration = Duration::from_secs(5);

impl Core {
    pub(crate) fn agent_name(&self, session: &str) -> &'static str {
        self.sessions.get(session).map_or("Claude", |s| s.run.agent)
    }

    /// Starts the away countdown if every guard holds. Runs on each fresh idle reading.
    pub(crate) fn try_arm_away(&mut self, idle: Duration, now: Instant, fx: &mut Vec<Effect>) {
        if self.mode != Mode::Idle {
            return;
        }
        let checked = self.check_away(idle, now);
        self.away_held_by = checked.as_ref().err().copied();
        match checked {
            Ok((session, target)) => {
                let flagged = &self.cfg.away.immersive;
                let immersive = flagged.iter().any(|id| id.eq_ignore_ascii_case(&target.id));
                let length = if immersive {
                    self.cfg.countdown.length.max(IMMERSIVE_COUNTDOWN)
                } else {
                    self.cfg.countdown.length
                };
                fx.push(Effect::ShowToast(toast::away_countdown(
                    &target,
                    length,
                    immersive,
                    self.agent_name(&session),
                )));
                fx.push(log("away_countdown", &session));
                self.note = None;
                let phase = Phase::Counting { ends: now + length };
                self.mode = Mode::AwayCountdown {
                    session,
                    target,
                    phase,
                };
            }
            Err(GuardFail::NoAwayApp) => self.note = Some(Note::NoAwayAppRunning),
            Err(_) => {}
        }
    }

    pub(crate) fn advance_away_countdown(
        &mut self,
        session: String,
        target: AppRef,
        phase: Phase,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        if let Phase::Verifying { until } = phase {
            // No answer from the shell: treat the switch as refused.
            return if now >= until {
                self.show_away_prompt(session, target, now, fx)
            } else {
                Mode::AwayCountdown {
                    session,
                    target,
                    phase,
                }
            };
        }
        if !self.away_still_wanted(&session) {
            fx.push(Effect::HideToast);
            return Mode::Idle;
        }
        let quiet_max = self.cfg.switching.input_quiet_max;
        match phase {
            Phase::Counting { ends } if now >= ends => Mode::AwayCountdown {
                session,
                target,
                phase: Phase::Quiet { since: now },
            },
            Phase::Quiet { since } if now >= since + quiet_max + QUIET_SLACK => {
                self.show_away_prompt(session, target, now, fx)
            }
            phase => Mode::AwayCountdown {
                session,
                target,
                phase,
            },
        }
    }

    pub(crate) fn advance_away_prompt(
        &mut self,
        session: String,
        target: AppRef,
        until: Instant,
        verifying: Option<Instant>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        match verifying {
            Some(deadline) if now >= deadline => {
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
            None if now >= until || !self.away_still_wanted(&session) => {
                fx.push(Effect::HideToast);
                Mode::Idle
            }
            _ => Mode::AwayPrompt {
                session,
                target,
                until,
                verifying,
            },
        }
    }

    /// The conditions an away countdown or prompt rechecks on every step: the run is still
    /// going, its home window is still in front, and nobody needs the user.
    fn away_still_wanted(&self, session: &str) -> bool {
        let run_going = self.sessions.get(session).is_some_and(|s| {
            s.state == SessionState::Running
                && s.run.home.as_ref().is_some_and(|home| self.front_is(home))
        });
        self.automation_on() && !self.someone_waiting() && run_going
    }

    /// The fallback prompt. Showing it uses up the run's away switch, so it appears once.
    pub(crate) fn show_away_prompt(
        &mut self,
        session: String,
        target: AppRef,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        self.spend_away(&session);
        fx.push(Effect::ShowToast(toast::away_prompt(
            &target,
            self.agent_name(&session),
        )));
        fx.push(log("away_prompt", &session));
        let until = now + self.cfg.countdown.away_prompt;
        Mode::AwayPrompt {
            session,
            target,
            until,
            verifying: None,
        }
    }

    pub(crate) fn request_activation(
        &self,
        target: &AppRef,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) -> Instant {
        let verify_after = self.cfg.switching.verify_after;
        fx.push(Effect::HideToast);
        fx.push(Effect::Activate {
            target: target.clone(),
            verify_after,
        });
        now + verify_after + VERIFY_GRACE
    }

    pub(crate) fn away_verified(
        &mut self,
        session: String,
        how: AwayHow,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        self.spend_away(&session);
        self.set_away_how(&session, how);
        fx.push(log("away_switched", &session));
        self.last_away_switch = Some(now);
        self.away = Some(AwayInfo {
            since: now,
            session,
        });
    }

    pub(crate) fn set_away_how(&mut self, session: &str, how: AwayHow) {
        if let Some(s) = self.sessions.get_mut(session) {
            s.run.away_how = how;
        }
    }
}
