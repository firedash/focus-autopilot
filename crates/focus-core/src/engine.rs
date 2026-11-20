// SPDX-License-Identifier: Apache-2.0
//! The core itself (design section 5): it takes inputs and the time, and returns effects.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::estimator::{History, away_threshold};
use crate::events::{AppRef, Observation, UserAction};
use crate::guards::GuardFail;
use crate::io::{Effect, Input, LogEvent, Note, PauseFor, ProbeKind, Status};
use crate::mode::{AwayInfo, Mode};
use crate::session::{Change, Session, SessionState, WaitKind};

/// How often idle time is read while an away switch is armed or a session awaits an answer.
const GUARD_PROBE_EVERY: Duration = Duration::from_secs(1);
/// How often idle time is read while waiting for a quiet moment before a switch.
const QUIET_PROBE_EVERY: Duration = Duration::from_millis(100);
/// Sessions tracked at once; events from further new sessions are dropped.
pub(crate) const MAX_SESSIONS: usize = 256;
/// How long nothing, or a passing app, may be in front between the app the user was working
/// in and the home window they arrive at, for that app still to count as where they left off.
const ELSEWHERE_MAX_GAP: Duration = Duration::from_secs(5);
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
    /// The app the user was in when they last came back to a home window while Claude was
    /// working or waiting: where they left off, and where the next away switch goes first.
    pub(crate) left_off: Option<AppRef>,
    /// The last app in front that has no Claude session in it, and when it stopped being in
    /// front. Kept so that a moment with nothing in front between it and a home window does
    /// not lose it.
    elsewhere: Option<(AppRef, Instant)>,
    /// The guard that held back the latest away check that got as far as an idle reading.
    pub(crate) away_held_by: Option<GuardFail>,
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
            left_off: None,
            elsewhere: None,
            away_held_by: None,
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

    /// True while Claude is working in at least one session, paused or not. Waiting for the
    /// user's approval is not working.
    pub fn claude_working(&self) -> bool {
        self.sessions
            .values()
            .any(|s| s.state == SessionState::Running)
    }

    pub fn note(&self) -> Option<Note> {
        self.note
    }

    /// The app the next away switch goes to first: where the user left off. For the shell to
    /// print.
    pub fn left_off(&self) -> Option<&AppRef> {
        self.left_off.as_ref()
    }

    /// Why a run that could still get an away switch is not getting one right now, in words,
    /// for the shell to print. `None` when no run could, or a switch is under way.
    pub fn away_wait(&self, now: Instant) -> Option<String> {
        if self.mode != Mode::Idle {
            return None;
        }
        let switching = &self.cfg.switching;
        let fail = match self.ready_run(now) {
            Ok(_) => self.away_held_by?,
            Err(fail) => fail,
        };
        match fail {
            GuardFail::NoCandidate | GuardFail::AppsUnknown => None,
            GuardFail::Paused => Some("paused".to_owned()),
            GuardFail::SomeoneWaiting => Some("a Claude session is waiting for you".to_owned()),
            GuardFail::NotElapsed => {
                let candidates = || {
                    self.sessions
                        .values()
                        .filter(|s| s.away_threshold_at(switching).is_some())
                };
                // Old enough already, so it is the grace after coming back that holds it.
                let just_back = candidates().any(|s| {
                    now >= s.run.started + away_threshold(s.run.tier, switching)
                        && s.back_at_home_until().is_some_and(|until| now < until)
                });
                if just_back {
                    return Some("you have just come back to this window".to_owned());
                }
                let needs = candidates()
                    .map(|s| away_threshold(s.run.tier, switching))
                    .min()?;
                Some(format!(
                    "the run has to go on for {} s first",
                    needs.as_secs()
                ))
            }
            GuardFail::HomeNotInFront => {
                Some("the window you prompted from is not in front".to_owned())
            }
            GuardFail::NotIdle => Some(format!(
                "waiting for {} s without keyboard or mouse input",
                switching.away_requires_idle.as_secs()
            )),
            GuardFail::NoAwayApp => Some(
                "nowhere to go: no app you were called back from, and no away app running"
                    .to_owned(),
            ),
        }
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

    /// True when some Claude session was prompted from `app`. Narrower than [`Self::is_home`]:
    /// an editor on the home-app list with no session in it is just another app the user
    /// may be doing their steady work in.
    pub(crate) fn hosts_a_session(&self, app: &AppRef) -> bool {
        self.sessions
            .values()
            .any(|s| s.run.home.as_ref().is_some_and(|home| home.matches(app)))
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

    /// The project name for a toast or card. Normally the folder's last name; if another live
    /// session is in a different folder with the same last name, `parent/name` for both, so
    /// two `api` projects can be told apart.
    pub(crate) fn project_label(&self, session: &str) -> String {
        let Some(this) = self.sessions.get(session) else {
            return String::new();
        };
        let name = crate::events::project_name(&this.run.cwd);
        let clash = !name.is_empty()
            && self.sessions.iter().any(|(id, other)| {
                id != session
                    && other.run.cwd != this.run.cwd
                    && crate::events::project_name(&other.run.cwd) == name
            });
        if clash {
            crate::events::project_name_with_parent(&this.run.cwd)
        } else {
            name
        }
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
                let previous = std::mem::replace(&mut self.frontmost, app);
                let previous_front = previous.clone();
                if let Some(previous) = previous.filter(|app| !self.hosts_a_session(app)) {
                    self.elsewhere = Some((previous, now));
                }
                self.note_left_off(now);
                self.note_back_at_home(previous_front.as_ref(), now);
                self.note_manual_away();
            }
            Observation::RunningApps(apps) => {
                // The name comes from our own configuration, never from a hook payload.
                let name = self.cfg.antigravity.app_name.clone();
                for session in self.sessions.values_mut() {
                    let run = &mut session.run;
                    if run.home.is_some()
                        || !matches!(session.state, SessionState::Done | SessionState::NeedsInput)
                    {
                        continue;
                    }
                    if now.saturating_duration_since(run.waiting_since) > Duration::from_secs(3) {
                        continue;
                    }
                    if !run.attention {
                        continue;
                    }
                    let candidates: Vec<_> = apps
                        .iter()
                        .filter(|app| app.name.eq_ignore_ascii_case(&name))
                        .collect();
                    let Some(first) = candidates.first() else {
                        fx.push(log("attention_app_not_running", ""));
                        continue;
                    };
                    // Different app identities with the same display name are not
                    // interchangeable. Multiple windows of one app are app-level.
                    if candidates
                        .iter()
                        .any(|app| !app.id.eq_ignore_ascii_case(&first.id))
                    {
                        fx.push(log("attention_app_ambiguous", ""));
                        continue;
                    }
                    let front = self.frontmost.as_ref();
                    let target = candidates
                        .iter()
                        .find(|app| front.is_some_and(|f| f.matches(app)))
                        .copied()
                        .unwrap_or(first);
                    run.home = Some((*target).clone());
                    run.away_spent = true;
                }
                self.running_apps = Some((apps, now));
            }
            Observation::Idle(idle) => self.on_idle(idle, now, fx),
            Observation::Activation { verified } => self.on_activation(verified, now, fx),
        }
    }

    /// Remembers the app the user came from when they arrive at a window with a Claude
    /// session in it while Claude is working or waiting for them. Arriving with nothing going on is not coming back to
    /// Claude, so it changes nothing.
    fn note_left_off(&mut self, now: Instant) {
        let arrived_home = self
            .frontmost
            .as_ref()
            .is_some_and(|front| self.hosts_a_session(front));
        if !arrived_home {
            return;
        }
        let Some((previous, left_at)) = self.elsewhere.take() else {
            return;
        };
        // Straight from that app, allowing for a moment with nothing in front on the way.
        if now.saturating_duration_since(left_at) > ELSEWHERE_MAX_GAP {
            return;
        }
        let claude_active = self
            .sessions
            .values()
            .any(|s| matches!(s.state, SessionState::Running | SessionState::NeedsInput))
            || !self.pending(now).is_empty();
        if claude_active {
            self.left_off = Some(previous);
        }
    }

    /// Notes, for every running run, that the user has just arrived at its home window from
    /// another app. Nothing in front for a moment is not another app.
    fn note_back_at_home(&mut self, previous: Option<&AppRef>, now: Instant) {
        let Some(front) = self.frontmost.clone() else {
            return;
        };
        for session in self.sessions.values_mut() {
            let Some(home) = &session.run.home else {
                continue;
            };
            let arrived = home.matches(&front) && previous.is_some_and(|p| !home.matches(p));
            if arrived && session.state == SessionState::Running {
                session.run.back_at_home = Some(now);
            }
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
