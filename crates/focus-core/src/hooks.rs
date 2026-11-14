// SPDX-License-Identifier: Apache-2.0
//! Hook events entering the core, and what the V1 backlog hangs off them: ignored projects
//! (G5), history-based estimates (E2), run records (E1), the declare-long hotkey (E4) and
//! the opt-in LLM summary (F2).

use std::time::Instant;

use crate::config::RecapMode;
use crate::engine::{Core, MAX_SESSIONS, change_name, log};
use crate::estimator::PromptLen;
use crate::events::{AgentEvent, EventKind};
use crate::glob;
use crate::io::Effect;
use crate::recap_llm;
use crate::run::{AwayHow, RunOutcome, RunRecord, Start};
use crate::session::{Session, SessionState};

impl Core {
    pub(crate) fn on_hook(&mut self, ev: &AgentEvent, now: Instant, fx: &mut Vec<Effect>) {
        if glob::matches_any(&self.cfg.ignore.cwd_globs, &ev.cwd) {
            return;
        }
        if ev.kind == EventKind::SessionEnd {
            if let Some(mut session) = self.sessions.remove(&ev.session_id) {
                fx.push(log("session_ended", &ev.session_id));
                if let Some(record) = session.run.record() {
                    self.emit_record(record, fx);
                }
            }
            return;
        }
        let start = self.start_for(ev);
        let (change, replaced) = if let Some(session) = self.sessions.get_mut(&ev.session_id) {
            let change = session.apply(ev, &start, now);
            (change, session.outbox.take())
        } else if self.sessions.len() < MAX_SESSIONS {
            let (session, change) = Session::first(ev, &start, now);
            self.sessions.insert(ev.session_id.clone(), session);
            (change, None)
        } else {
            fx.push(log("session_limit_reached", &ev.session_id));
            return;
        };
        if let Some(record) = replaced {
            self.emit_record(record, fx);
        }
        if let Some(name) = change_name(change) {
            fx.push(log(name, &ev.session_id));
        }
    }

    /// What is known at prompt submit: the home window and how long similar runs took.
    fn start_for(&self, ev: &AgentEvent) -> Start {
        let EventKind::UserPromptSubmit {
            prompt_chars,
            permission_mode,
            ..
        } = &ev.kind
        else {
            return Start::default();
        };
        let prompt_len = PromptLen::of_chars(*prompt_chars);
        Start {
            home: self.frontmost.clone().filter(|front| self.is_home(front)),
            estimate: self
                .history
                .estimate(&ev.project, permission_mode.as_deref(), prompt_len),
        }
    }

    /// Writes history lines for runs that have closed for good.
    pub(crate) fn take_records(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        let records: Vec<RunRecord> = self
            .sessions
            .values_mut()
            .filter_map(|s| s.take_record(now))
            .collect();
        for record in records {
            self.emit_record(record, fx);
        }
    }

    fn emit_record(&mut self, record: RunRecord, fx: &mut Vec<Effect>) {
        // Only completed runs say how long such a run takes.
        if record.outcome == RunOutcome::Done {
            self.history.push(record.sample());
        }
        fx.push(Effect::AppendRun(record));
    }

    /// The declare-long hotkey: applies to the run whose home window is in front.
    pub(crate) fn declare_long(&mut self, fx: &mut Vec<Effect>) {
        let front = self.frontmost.clone();
        let target = self.sessions.iter_mut().find(|(_, s)| {
            s.state == SessionState::Running
                && matches!((&s.run.home, &front), (Some(home), Some(front)) if home.matches(front))
        });
        if let Some((id, session)) = target {
            session.run.declare_long();
            fx.push(log("declared_long", id));
        }
    }

    /// The user went to an away app themselves while a run was going.
    pub(crate) fn note_manual_away(&mut self) {
        if !self.on_away_app() {
            return;
        }
        for session in self.sessions.values_mut() {
            let going = matches!(
                session.state,
                SessionState::Running | SessionState::NeedsInput
            );
            if going && session.run.home.is_some() && session.run.away_how == AwayHow::None {
                session.run.away_how = AwayHow::Manual;
            }
        }
    }

    /// In LLM mode, asks the shell for a summary to replace the basic card just shown.
    pub(crate) fn request_summary(&mut self, session: &str, now: Instant, fx: &mut Vec<Effect>) {
        if self.cfg.recap.mode != RecapMode::Llm {
            return;
        }
        let Some(shown) = self.sessions.get(session) else {
            return;
        };
        let prompt = recap_llm::prompt(shown);
        self.recap_seq += 1;
        self.last_recap = Some((self.recap_seq, now));
        let timeout = self.cfg.recap.llm_timeout;
        fx.push(Effect::SummarizeRecap {
            id: self.recap_seq,
            prompt,
            timeout,
        });
    }

    /// The summary replaces the basic card only if it is for the latest card and in time.
    pub(crate) fn on_summary(&mut self, id: u64, text: &str, now: Instant, fx: &mut Vec<Effect>) {
        let Some((latest, issued)) = self.last_recap else {
            return;
        };
        if id != latest {
            return;
        }
        self.last_recap = None;
        let in_time = now.saturating_duration_since(issued) <= self.cfg.recap.llm_timeout;
        if let Some(card) = recap_llm::parse(text).filter(|_| in_time) {
            fx.push(Effect::ShowRecap(card));
        }
    }
}
