// SPDX-License-Identifier: Apache-2.0
//! Test harness: a fake clock and a fake platform that executes the core's effects the way
//! the real shell will, so traces can be replayed end to end.
#![allow(dead_code, clippy::expect_used, clippy::panic, clippy::unwrap_used)]

pub mod json;
pub mod trace;

use std::time::{Duration, Instant};

use focus_core::{
    AgentEvent, AppRef, Config, Core, Effect, EventKind, Input, Observation, ToastKind, UserAction,
};

pub const TERMINAL: &str = "com.apple.Terminal";
pub const VSCODE: &str = "com.microsoft.VSCode";
pub const TERRARIA: &str = "com.re-logic.terraria";
pub const BROWSER: &str = "com.apple.Safari";

pub fn app(id: &str) -> AppRef {
    let name = match id {
        TERMINAL => "Terminal",
        VSCODE => "VS Code",
        TERRARIA => "Terraria",
        other => other,
    };
    AppRef {
        id: id.to_owned(),
        name: name.to_owned(),
        pid: 1,
        window: None,
    }
}

pub fn config() -> Config {
    let mut config = Config::default();
    config.away.apps = vec![TERRARIA.to_owned()];
    config
}

pub struct Sim {
    pub core: Core,
    pub front: Option<AppRef>,
    pub running: Vec<AppRef>,
    /// Whether the OS honours activation requests.
    pub os_allows_switch: bool,
    /// Whether the fake shell reports activation results at all.
    pub shell_verifies: bool,
    /// Continuous keyboard or mouse input: idle time is always zero.
    pub typing: bool,
    /// Every effect, with the time in milliseconds since the start.
    pub log: Vec<(u64, Effect)>,
    start: Instant,
    now: Instant,
    last_input: Instant,
    pending_verify: Option<(Instant, bool)>,
}

impl Sim {
    /// The user is in Terminal, with Terraria running in the background.
    pub fn new(config: Config) -> Self {
        let start = Instant::now();
        let mut sim = Self {
            core: Core::new(config, vec![TERMINAL.to_owned(), VSCODE.to_owned()]),
            front: Some(app(TERMINAL)),
            running: vec![app(TERMINAL), app(TERRARIA)],
            os_allows_switch: true,
            shell_verifies: true,
            typing: false,
            log: Vec::new(),
            start,
            now: start,
            last_input: start,
            pending_verify: None,
        };
        sim.feed(Input::Observed(Observation::Frontmost(sim.front.clone())));
        sim
    }

    /// A sim with one run started from Terminal at time zero.
    pub fn started() -> Self {
        let mut sim = Self::new(config());
        sim.hook(prompt("Run the tests and fix failures"));
        sim
    }

    pub fn status(&self) -> focus_core::Status {
        self.core.status(self.now)
    }

    pub fn ms(&self) -> u64 {
        u64::try_from(self.now.duration_since(self.start).as_millis()).unwrap()
    }

    /// Moves the clock to `ms`, ticking the core at each of its deadlines on the way.
    pub fn at(&mut self, ms: u64) {
        let target = self.start + Duration::from_millis(ms);
        assert!(target >= self.now, "time only moves forward");
        for _ in 0..1_000_000 {
            let verify = self.pending_verify.map(|(at, _)| at);
            let next = [self.core.next_deadline(), verify]
                .into_iter()
                .flatten()
                .min();
            let Some(next) = next.filter(|at| *at <= target) else {
                self.now = target;
                return;
            };
            self.now = self.now.max(next);
            match self.pending_verify {
                Some((at, verified)) if at <= self.now => {
                    self.pending_verify = None;
                    self.feed(Input::Observed(Observation::Activation { verified }));
                }
                _ => {
                    let fx = self.core.tick(self.now);
                    self.run(fx);
                }
            }
        }
        panic!(
            "the core keeps asking to be ticked at {} ms without making progress",
            self.ms()
        );
    }

    pub fn feed(&mut self, input: Input) {
        let fx = self.core.handle(input, self.now);
        self.run(fx);
    }

    fn run(&mut self, fx: Vec<Effect>) {
        for effect in fx {
            self.log.push((self.ms(), effect.clone()));
            match effect {
                Effect::Probe(focus_core::ProbeKind::Idle) => {
                    let idle = self.idle();
                    self.feed(Input::Observed(Observation::Idle(idle)));
                }
                Effect::Probe(focus_core::ProbeKind::RunningApps) => {
                    let apps = self.running.clone();
                    self.feed(Input::Observed(Observation::RunningApps(apps)));
                }
                Effect::Probe(focus_core::ProbeKind::Frontmost) => {
                    let front = self.front.clone();
                    self.feed(Input::Observed(Observation::Frontmost(front)));
                }
                Effect::Activate {
                    target,
                    verify_after,
                } => {
                    if self.shell_verifies {
                        self.pending_verify =
                            Some((self.now + verify_after, self.os_allows_switch));
                    }
                    if self.os_allows_switch {
                        self.front = Some(target);
                        self.feed(Input::Observed(Observation::Frontmost(self.front.clone())));
                    }
                }
                _ => {}
            }
        }
    }

    fn idle(&self) -> Duration {
        if self.typing {
            Duration::ZERO
        } else {
            self.now.duration_since(self.last_input)
        }
    }

    /// A hook event for session `s1`, run `p1`.
    pub fn hook(&mut self, kind: EventKind) {
        self.hook_for("s1", "p1", kind);
    }

    pub fn hook_for(&mut self, session: &str, run: &str, kind: EventKind) {
        self.hook_in(session, run, "/work/app", kind);
    }

    /// Like `hook_for`, from a given project folder.
    pub fn hook_in(&mut self, session: &str, run: &str, cwd: &str, kind: EventKind) {
        if matches!(kind, EventKind::UserPromptSubmit { .. }) {
            // The user has just pressed Enter.
            self.last_input = self.now;
        }
        self.feed(Input::Hook(AgentEvent {
            session_id: session.to_owned(),
            prompt_id: Some(run.to_owned()),
            cwd: cwd.to_owned(),
            project: "proj-a".to_owned(),
            kind,
        }));
    }

    /// The user presses a key or moves the mouse.
    pub fn input(&mut self) {
        self.last_input = self.now;
    }

    /// The user switches to another app themselves.
    pub fn focus(&mut self, id: &str) {
        self.last_input = self.now;
        self.front = Some(app(id));
        self.feed(Input::Observed(Observation::Frontmost(self.front.clone())));
    }

    /// A click on the toast, or the focus key.
    pub fn toast_action(&mut self) {
        self.last_input = self.now;
        self.feed(Input::User(UserAction::ToastAction));
    }

    pub fn user(&mut self, action: UserAction) {
        self.feed(Input::User(action));
    }

    pub fn toasts(&self) -> Vec<(u64, ToastKind)> {
        self.log
            .iter()
            .filter_map(|(ms, fx)| match fx {
                Effect::ShowToast(toast) => Some((*ms, toast.kind)),
                _ => None,
            })
            .collect()
    }

    pub fn toast_texts(&self) -> Vec<&str> {
        self.log
            .iter()
            .filter_map(|(_, fx)| match fx {
                Effect::ShowToast(toast) => Some(toast.text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn activations(&self) -> Vec<(u64, &str)> {
        self.log
            .iter()
            .filter_map(|(ms, fx)| match fx {
                Effect::Activate { target, .. } => Some((*ms, target.id.as_str())),
                _ => None,
            })
            .collect()
    }

    pub fn recaps(&self) -> Vec<Vec<&str>> {
        self.log
            .iter()
            .filter_map(|(_, fx)| match fx {
                Effect::ShowRecap(card) => Some(card.lines.iter().map(String::as_str).collect()),
                _ => None,
            })
            .collect()
    }

    pub fn records(&self) -> Vec<&focus_core::RunRecord> {
        self.log
            .iter()
            .filter_map(|(_, fx)| match fx {
                Effect::AppendRun(record) => Some(record),
                _ => None,
            })
            .collect()
    }

    pub fn summary_requests(&self) -> Vec<(u64, &str)> {
        self.log
            .iter()
            .filter_map(|(_, fx)| match fx {
                Effect::SummarizeRecap { id, prompt, .. } => Some((*id, prompt.as_str())),
                _ => None,
            })
            .collect()
    }

    pub fn sounds(&self) -> usize {
        self.count(|fx| matches!(fx, Effect::PlaySound(_)))
    }

    pub fn hides(&self) -> usize {
        self.count(|fx| matches!(fx, Effect::HideToast))
    }

    pub fn logged(&self, name: &str) -> usize {
        self.count(|fx| matches!(fx, Effect::Log(event) if event.name == name))
    }

    fn count(&self, wanted: impl Fn(&Effect) -> bool) -> usize {
        self.log.iter().filter(|(_, fx)| wanted(fx)).count()
    }

    /// Runs the standard opening: prompt at 0, away countdown at 30 s, in Terraria by 33.4 s.
    pub fn away() -> Self {
        let mut sim = Self::started();
        sim.at(34_000);
        assert_eq!(sim.activations(), vec![(33_000, TERRARIA)]);
        assert_eq!(sim.front, Some(app(TERRARIA)));
        sim
    }
}

pub fn prompt(text: &str) -> EventKind {
    EventKind::UserPromptSubmit {
        prompt: text.to_owned(),
        prompt_chars: text.chars().count(),
        permission_mode: Some("default".to_owned()),
    }
}

pub fn tool(name: &str) -> EventKind {
    EventKind::PreToolUse {
        tool_name: name.to_owned(),
        command: None,
        file_path: None,
        effort: None,
    }
}

pub fn bash(command: &str) -> EventKind {
    EventKind::PreToolUse {
        tool_name: "Bash".to_owned(),
        command: Some(command.to_owned()),
        file_path: None,
        effort: Some("high".to_owned()),
    }
}

pub fn edit(path: &str) -> EventKind {
    EventKind::PreToolUse {
        tool_name: "Edit".to_owned(),
        command: None,
        file_path: Some(path.to_owned()),
        effort: None,
    }
}

pub fn permission(command: &str) -> EventKind {
    EventKind::PermissionRequest {
        tool_name: "Bash".to_owned(),
        command: Some(command.to_owned()),
        file_path: None,
    }
}

pub fn stop(message: &str) -> EventKind {
    EventKind::Stop {
        stop_hook_active: false,
        last_assistant_message: message.to_owned(),
        background_tasks: 0,
    }
}
