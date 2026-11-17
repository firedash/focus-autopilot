// SPDX-License-Identifier: Apache-2.0
//! The away guards (design section 7). All must hold before an away countdown starts.

use std::time::{Duration, Instant};

use crate::engine::Core;
use crate::events::AppRef;
use crate::session::SessionState;

/// A running-apps answer older than this is not trusted for choosing the away app.
const RUNNING_APPS_MAX_AGE: Duration = Duration::from_secs(3);

/// The first guard that failed, in the order they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuardFail {
    /// Automation is paused or disabled.
    Paused,
    /// Some session is waiting on the user.
    SomeoneWaiting,
    /// No running run has a home window and an unused away switch.
    NoCandidate,
    /// The tier threshold hasn't been reached.
    NotElapsed,
    /// The home window isn't frontmost.
    HomeNotInFront,
    /// The user has been active too recently.
    NotIdle,
    /// The running apps haven't been reported yet.
    AppsUnknown,
    /// None of the configured away apps is running.
    NoAwayApp,
}

impl Core {
    /// True while any session is waiting for an answer, debounced or not.
    pub(crate) fn someone_waiting(&self) -> bool {
        self.sessions
            .values()
            .any(|s| s.state == SessionState::NeedsInput)
    }

    /// The guards that need no fresh reading: a running run with a home window and an unused
    /// away switch, past its threshold, with that home window in front.
    pub(crate) fn ready_run(&self, now: Instant) -> Result<&str, GuardFail> {
        if !self.automation_on() {
            return Err(GuardFail::Paused);
        }
        if self.someone_waiting() {
            return Err(GuardFail::SomeoneWaiting);
        }
        let mut closest = GuardFail::NoCandidate;
        for (id, session) in &self.sessions {
            let Some(threshold_at) = session.away_threshold_at(&self.cfg.switching) else {
                continue;
            };
            let home_in_front = session
                .run
                .home
                .as_ref()
                .is_some_and(|home| self.front_is(home));
            let fail = if now < threshold_at {
                GuardFail::NotElapsed
            } else if !home_in_front {
                GuardFail::HomeNotInFront
            } else {
                return Ok(id);
            };
            closest = closest.max(fail);
        }
        Err(closest)
    }

    /// Every away guard. On success, the session to switch for and the away app to switch to.
    pub(crate) fn check_away(
        &self,
        idle: Duration,
        now: Instant,
    ) -> Result<(String, AppRef), GuardFail> {
        let session = self.ready_run(now)?.to_owned();
        // Pressing the declare-long hotkey is input, and it is the user asking to be switched.
        let asked = self
            .sessions
            .get(&session)
            .is_some_and(|s| s.run.declared_long);
        if !asked && idle < self.cfg.switching.away_requires_idle {
            return Err(GuardFail::NotIdle);
        }
        let target = self.away_target(now)?;
        Ok((session, target))
    }

    /// Where an away switch goes: the app the user left off in when they last came back to
    /// Claude, if it is still running, and otherwise the first configured away app that is.
    fn away_target(&self, now: Instant) -> Result<AppRef, GuardFail> {
        let Some((apps, at)) = &self.running_apps else {
            return Err(GuardFail::AppsUnknown);
        };
        if now.saturating_duration_since(*at) > RUNNING_APPS_MAX_AGE {
            return Err(GuardFail::AppsUnknown);
        }
        let running = |id: &str| apps.iter().find(|app| app.id.eq_ignore_ascii_case(id));
        let left_off = self
            .left_off
            .as_ref()
            .filter(|_| self.cfg.away.resume_last_app)
            .and_then(|app| running(&app.id))
            // It may have been made a home app since.
            .filter(|app| !self.is_home(app));
        left_off
            .or_else(|| self.cfg.away.apps.iter().find_map(|id| running(id)))
            .cloned()
            .ok_or(GuardFail::NoAwayApp)
    }
}
