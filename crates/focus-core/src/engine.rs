// SPDX-License-Identifier: Apache-2.0
//! The core itself (design section 5): it takes inputs and the time, and returns effects.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::estimator::History;
use crate::events::{AppRef, Observation, UserAction};
use crate::io::{Effect, Input, LogEvent, Note, PauseFor, ProbeKind, Status};
use crate::mode::{AwayInfo, Mode};
use crate::session::{Change, Session, SessionState, WaitKind};

/// How often idle time is read while an away switch is armed or a session awaits an answer.
const GUARD_PROBE_EVERY: Duration = Duration::from_secs(1);
/// How often idle time is read while waiting for a quiet moment before a switch.
const QUIET_PROBE_EVERY: Duration = Duration::from_millis(100);
/// Sessions tracked at once; events from further new sessions are dropped.
pub(crate) const MAX_SESSIONS: usize = 256;
/// A settle pass normally ends after one or two steps; this bounds it.
const MAX_SETTLE_STEPS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pause {
    Off,
    Until(Instant),
    Indefinite,
}

/// A session waiting for the user to come back to its home window.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub session: String,
    pub home: AppRef,
    pub kind: WaitKind,
    since: Instant,
}

pub struct Core {
    pub(crate) cfg: Config,
    known_home_apps: Vec<String>,
    pub(crate) sessions: BTreeMap<String, Session>,
    pub(crate) mode: Mode,
    pub(crate) away: Option<AwayInfo>,
    pub(crate) frontmost: Option<AppRef>,
    pub(crate) running_apps: Option<(Vec<AppRef>, Instant)>,
    pub(crate) last_away_switch: Option<Instant>,
    pub(crate) note: Option<Note>,
    /// Set when a step changed something other than the mode, so settling continues.
    pub(crate) touched: bool,
    pause: Pause,
    pub(crate) history: History,
    pub(crate) recap_seq: u64,
    /// The latest summary request, with the basic card held back until it is answered.
    pub(crate) last_recap: Option<crate::hooks::PendingRecap>,
    last_probe: Option<Instant>,
    clock: Option<Instant>,
}

impl Core {
    /// `known_home_apps` is the built-in home-app list for this OS; an entry ending in `*`
    /// matches by prefix. `home.extra_apps` in the config adds to it.
    pub fn new(config: Config, known_home_apps: Vec<String>) -> Self {
        Self {
            cfg: config,
            known_home_apps,
            sessions: BTreeMap::new(),
            mode: Mode::Idle,
            away: None,
            frontmost: None,
            running_apps: None,
            last_away_switch: None,
            note: None,
            touched: false,
            pause: Pause::Off,
            history: History::default(),
            recap_seq: 0,
            last_recap: None,
            last_probe: None,
            clock: None,
        }
    }

    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.clock = Some(now);
        match input {
            Input::Hook(ev) => self.on_hook(&ev, now, &mut fx),
            Input::Observed(observation) => self.on_observation(observation, now, &mut fx),
            Input::User(action) => self.on_user(action, now, &mut fx),
            Input::Config(config) => self.cfg = *config,
            Input::History(samples) => self.history.replace(samples),
            Input::Summary { id, text } => self.on_summary(id, &text, now, &mut fx),
            Input::SummaryFailed { id } => self.on_summary_failed(id, &mut fx),
        }
        self.settle(now, &mut fx);
        fx
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.clock = Some(now);
        self.settle(now, &mut fx);
        fx
    }

    /// When `tick` should next be called if no input arrives first.
    pub fn next_deadline(&self) -> Option<Instant> {
        let now = self.clock?;
        let switching = &self.cfg.switching;
        let pause = match self.pause {
            Pause::Until(at) => Some(at),
            _ => None,
        };
        let probe = self
            .probe_plan(now)
            .map(|(every, _)| self.last_probe.map_or(now, |last| last + every));
        let sessions = self
            .sessions
            .values()
            .flat_map(|s| s.deadlines(now, switching));
        let recap = self.recap_deadline();
        sessions
            .chain(pause)
            .chain(recap)
            .chain(probe)
            .chain(self.mode.deadline(switching.input_quiet_max))
            .min()
    }

    pub fn status(&self, now: Instant) -> Status {
        if !self.cfg.enabled {
            return Status::Paused { until: None };
        }
        match self.pause {
            Pause::Until(at) => return Status::Paused { until: Some(at) },
            Pause::Indefinite => return Status::Paused { until: None },
            Pause::Off => {}
        }
        if self.away.is_some() {
            return Status::Away;
        }
        let active =
            |s: &&Session| matches!(s.state, SessionState::Running | SessionState::NeedsInput);
        match self
            .sessions
            .values()
            .filter(active)
            .map(|s| s.run.started)
            .min()
        {
            Some(started) => Status::Running {
                elapsed: now.saturating_duration_since(started),
            },
            None => Status::Idle,
        }
    }

    pub fn note(&self) -> Option<Note> {
        self.note
    }

    /// The state of a session, for tests and diagnostics.
    pub fn session_state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.get(session_id).map(|s| s.state)
    }

    pub(crate) fn automation_on(&self) -> bool {
        self.cfg.enabled && self.pause == Pause::Off
    }

    pub(crate) fn is_home(&self, app: &AppRef) -> bool {
        let known = self.known_home_apps.iter().chain(&self.cfg.home.extra_apps);
        known
            .into_iter()
            .any(|entry| app_id_matches(entry, &app.id))
    }

    pub(crate) fn is_away_app(&self, app: &AppRef) -> bool {
        self.cfg
            .away
            .apps
            .iter()
            .any(|id| id.eq_ignore_ascii_case(&app.id))
    }

    pub(crate) fn front_is(&self, target: &AppRef) -> bool {
        self.frontmost
            .as_ref()
            .is_some_and(|front| target.matches(front))
    }

    pub(crate) fn on_away_app(&self) -> bool {
        self.frontmost
            .as_ref()
            .is_some_and(|front| self.is_away_app(front))
    }

    /// Sessions waiting for the user: needs-input before done, then oldest first.
    pub(crate) fn pending(&self, now: Instant) -> Vec<Pending> {
        let mut pending: Vec<Pending> = self
            .sessions
            .iter()
            .filter_map(|(id, s)| {
                let kind = s.waiting(now, &self.cfg.switching)?;
                let home = s.run.home.clone()?;
                Some(Pending {
                    session: id.clone(),
                    home,
                    kind,
                    since: s.run.waiting_since,
                })
            })
            .collect();
        pending.sort_by_key(|p| (p.kind, p.since));
        pending
    }

    pub(crate) fn mark_handled(&mut self, session: &str) {
        if let Some(s) = self.sessions.get_mut(session) {
            s.run.return_handled = true;
            self.touched = true;
        }
    }

    pub(crate) fn spend_away(&mut self, session: &str) {
        if let Some(s) = self.sessions.get_mut(session) {
            s.run.away_spent = true;
        }
    }

    fn on_observation(&mut self, observation: Observation, now: Instant, fx: &mut Vec<Effect>) {
        match observation {
            Observation::Frontmost(app) => {
                self.frontmost = app;
                self.note_manual_away();
            }
            Observation::RunningApps(apps) => self.running_apps = Some((apps, now)),
            Observation::Idle(idle) => self.on_idle(idle, now, fx),
            Observation::Activation { verified } => self.on_activation(verified, now, fx),
        }
    }

    fn on_idle(&mut self, idle: Duration, now: Instant, fx: &mut Vec<Effect>) {
        self.resume_answered_sessions(idle, now, fx);
        self.quiet_check(idle, now, fx);
        self.try_arm_away(idle, now, fx);
    }

    /// Input can only answer a prompt while its home window is in front, so the resume
    /// heuristic looks at those sessions alone.
    fn resume_answered_sessions(&mut self, idle: Duration, now: Instant, fx: &mut Vec<Effect>) {
        let resume_idle = self.cfg.switching.resume_idle;
        let front = self.frontmost.clone();
        for (id, session) in &mut self.sessions {
            let at_home = match (&session.run.home, &front) {
                (Some(home), Some(front)) => home.matches(front),
                _ => false,
            };
            if at_home && session.observe_idle(idle, now, resume_idle) {
                fx.push(log("resumed_after_input", id));
            }
        }
    }

    fn on_user(&mut self, action: UserAction, now: Instant, fx: &mut Vec<Effect>) {
        match action {
            UserAction::ToastAction => self.on_toast_action(now, fx),
            UserAction::Pause(PauseFor::Minutes(minutes)) => {
                self.pause = Pause::Until(now + Duration::from_secs(u64::from(minutes) * 60));
            }
            UserAction::Pause(PauseFor::UntilResumed) => self.pause = Pause::Indefinite,
            UserAction::Resume => self.pause = Pause::Off,
            UserAction::DeclareLong => self.declare_long(fx),
            UserAction::TogglePause => {
                self.pause = if self.pause == Pause::Off {
                    Pause::Indefinite
                } else {
                    Pause::Off
                };
            }
        }
    }

    fn settle(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        self.recap_timed_out(now, fx);
        if matches!(self.pause, Pause::Until(at) if now >= at) {
            self.pause = Pause::Off;
        }
        let stale_after = self.cfg.switching.stale_after;
        for (id, session) in &mut self.sessions {
            if session.expire_stale(now, stale_after) {
                fx.push(log("run_stale", id));
            }
        }
        for _ in 0..MAX_SETTLE_STEPS {
            self.touched = false;
            let before = std::mem::replace(&mut self.mode, Mode::Idle);
            let after = self.advance(before.clone(), now, fx);
            let changed = after != before;
            self.mode = after;
            if !changed && !self.touched {
                break;
            }
        }
        self.take_records(now, fx);
        self.schedule_probes(now, fx);
    }

    fn advance(&mut self, mode: Mode, now: Instant, fx: &mut Vec<Effect>) -> Mode {
        match mode {
            Mode::Idle => self.advance_idle(now, fx),
            Mode::AwayCountdown {
                session,
                target,
                phase,
            } => self.advance_away_countdown(session, target, phase, now, fx),
            Mode::AwayPrompt {
                session,
                target,
                until,
                verifying,
            } => self.advance_away_prompt(session, target, until, verifying, now, fx),
            Mode::Return {
                session,
                home,
                snoozes,
                step,
            } => self.advance_return(session, home, snoozes, step, now, fx),
        }
    }

    /// Which idle polling is needed now: the interval, and whether running apps are needed too.
    fn probe_plan(&self, now: Instant) -> Option<(Duration, bool)> {
        if self.mode.in_quiet() {
            Some((QUIET_PROBE_EVERY, false))
        } else if self.mode == Mode::Idle && self.ready_run(now).is_ok() {
            Some((GUARD_PROBE_EVERY, true))
        } else if self.automation_on() && self.answer_could_rearm_away() {
            Some((GUARD_PROBE_EVERY, false))
        } else {
            None
        }
    }

    /// True when a session is waiting on the user in the window they are looking at and
    /// could still get an away switch once they answer.
    fn answer_could_rearm_away(&self) -> bool {
        self.sessions.values().any(|s| {
            s.state == SessionState::NeedsInput
                && !s.run.away_spent
                && s.run.home.as_ref().is_some_and(|home| self.front_is(home))
        })
    }

    fn schedule_probes(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        let plan = self.probe_plan(now);
        if !matches!(plan, Some((_, true))) {
            self.note = None;
        }
        let Some((every, with_apps)) = plan else {
            return;
        };
        if self.last_probe.is_some_and(|last| now < last + every) {
            return;
        }
        if with_apps {
            fx.push(Effect::Probe(ProbeKind::RunningApps));
        }
        fx.push(Effect::Probe(ProbeKind::Idle));
        self.last_probe = Some(now);
    }
}

pub(crate) fn log(name: &'static str, session: &str) -> Effect {
    Effect::Log(LogEvent {
        name,
        session: Some(session.to_owned()),
    })
}

pub(crate) fn change_name(change: Change) -> Option<&'static str> {
    match change {
        Change::None => None,
        Change::Ignored => Some("late_event_ignored"),
        Change::RunStarted => Some("run_started"),
        Change::Adopted => Some("run_adopted_without_home"),
        Change::EnteredNeedsInput => Some("needs_input"),
        Change::LeftNeedsInput => Some("needs_input_cleared"),
        Change::Finished => Some("run_finished"),
        Change::Continued => Some("run_continued"),
    }
}

/// Case-insensitive app-id match; an entry ending in `*` matches by prefix.
pub fn app_id_matches(entry: &str, id: &str) -> bool {
    match entry.strip_suffix('*') {
        Some(prefix) => id
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix)),
        None => entry.eq_ignore_ascii_case(id),
    }
}

#[cfg(test)]
mod tests {
    use super::app_id_matches as id_matches;

    #[test]
    fn app_ids_match_exactly_or_by_prefix() {
        assert!(id_matches("com.apple.Terminal", "com.apple.terminal"));
        assert!(!id_matches("com.apple.Terminal", "com.apple.TerminalX"));
        assert!(id_matches("com.jetbrains.*", "com.jetbrains.intellij"));
        assert!(!id_matches("com.jetbrains.*", "com.jet"));
        assert!(!id_matches("com.jetbrains.*", "org.jetbrains.x"));
    }
}
