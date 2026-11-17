// SPDX-License-Identifier: Apache-2.0
//! The return half of the switch controller: countdown, snooze, fallback prompt, and the
//! recap when the user is back.

use std::time::{Duration, Instant};

use crate::actions::return_how;
use crate::engine::{Core, Pending, log};
use crate::events::AppRef;
use crate::io::{Effect, Sound};
use crate::mode::{Mode, Phase, QUIET_SLACK, ReturnStep};
use crate::run::ReturnHow;
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
            self.set_return_how(session, ReturnHow::Manual);
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
        if self.returns_by_countdown() && !just_switched_away {
            self.show_return_countdown(next.session, next.home, 0, now, fx)
        } else {
            self.show_return_prompt(next.session, next.home, 0, fx)
        }
    }

    /// Whether a return from where the user is now happens by itself after a countdown. Not
    /// from another home window: someone working with one Claude session is asked, not moved,
    /// when another one finishes.
    fn returns_by_countdown(&self) -> bool {
        let in_a_home_window = self
            .frontmost
            .as_ref()
            .is_some_and(|front| self.is_home(front));
        self.cfg.switching.auto_return && !in_a_home_window
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
        let length = self.cfg.countdown.return_length;
        let label = self.project_label(&session);
        fx.push(Effect::ShowToast(toast::return_countdown(
            waiting, &label, &home, length,
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

    pub(crate) fn show_return_prompt(
        &self,
        session: String,
        home: AppRef,
        snoozes: u32,
        fx: &mut Vec<Effect>,
    ) -> Mode {
        let Some(waiting) = self.sessions.get(&session) else {
            return Mode::Idle;
        };
        let label = self.project_label(&session);
        fx.push(Effect::ShowToast(toast::return_prompt(waiting, &label)));
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
                // In front because focusd asked, or because the user came back themselves.
                let how = return_how(&step).unwrap_or(ReturnHow::Manual);
                self.complete_return(&session, how, now, fx);
            }
            return Mode::Idle;
        }
        let quiet_max = self.cfg.switching.input_quiet_max;
        match step {
            ReturnStep::Countdown(Phase::Counting { ends }) if now >= ends => {
                // Still wanted: the setting can change, and the user can move to another
                // home window, while a countdown runs.
                if self.returns_by_countdown() {
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
                if snoozes < self.cfg.countdown.max_snoozes && self.returns_by_countdown() {
                    self.show_return_countdown(session, home, snoozes, now, fx)
                } else {
                    self.show_return_prompt(session, home, snoozes, fx)
                }
            }
            _ => mode,
        }
    }

    /// The user is back at a home window: show the recap and settle the sessions waiting
    /// there. Sessions waiting at another window stay queued and get their own return next
    /// (backlog H1).
    pub(crate) fn complete_return(
        &mut self,
        session: &str,
        how: ReturnHow,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        fx.push(log("returned", session));
        self.show_recap(session, now, fx);
        let here: Vec<String> = self
            .pending(now)
            .into_iter()
            .filter(|p| p.session == session || self.front_is(&p.home))
            .map(|p| p.session)
            .collect();
        for id in &here {
            self.mark_handled(id);
        }
        self.set_return_how(session, how);
        self.away = None;
    }

    fn set_return_how(&mut self, session: &str, how: ReturnHow) {
        if let Some(s) = self.sessions.get_mut(session) {
            s.run.return_how = how;
        }
    }
}
