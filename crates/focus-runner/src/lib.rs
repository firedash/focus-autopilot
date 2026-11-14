// SPDX-License-Identifier: Apache-2.0
//! The shell loop (design section 5): one thread sleeps until the next input or
//! `Core::next_deadline`, feeds the core, and carries out its effects against a [`Platform`].
//! Toasts and recaps are printed to the terminal until the overlay UI (M5) exists.
#![forbid(unsafe_code)]

pub mod paths;

use std::io::BufRead as _;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use focus_core::{AppRef, Core, Effect, Input, Observation, ProbeKind};
use focus_platform::{Overlay, Platform};
use focus_receiver::Received;

pub enum Msg {
    Hook(Received),
    Line(String),
}

/// What a terminal command asks of the loop.
pub enum Control {
    Quit,
    Continue(Vec<Effect>),
}

/// How often the front app is read. Interim: the macOS adapter has no change notification yet.
const POLL_FRONT: Duration = Duration::from_millis(500);

/// Forwards hooks and stdin lines into one queue. Stdin closing is not an error: a daemon may
/// run without a terminal.
pub fn inputs(hooks: Receiver<Received>) -> Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    let hook_tx: Sender<Msg> = tx.clone();
    std::thread::spawn(move || {
        while let Ok(received) = hooks.recv() {
            if hook_tx.send(Msg::Hook(received)).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(Msg::Line(line)).is_err() {
                return;
            }
        }
    });
    rx
}

struct PendingVerify {
    at: Instant,
    target: AppRef,
}

/// Runs until `commands` returns [`Control::Quit`] or the input queue closes.
pub fn run<P: Platform + Overlay>(
    core: &mut Core,
    platform: &P,
    rx: &Receiver<Msg>,
    recap_for: Duration,
    mut commands: impl FnMut(&str, &mut Core, Instant) -> Control,
) {
    // Like a real shell: say what is in front before any hook arrives, so the next prompt
    // captures the right home window.
    let mut front = platform.frontmost().ok().flatten();
    let startup = core.handle(
        Input::Observed(Observation::Frontmost(front.clone())),
        Instant::now(),
    );
    announce_front(front.as_ref());
    let mut effects = startup;
    let mut pending: Option<PendingVerify> = None;
    let mut next_poll = Instant::now() + POLL_FRONT;
    loop {
        drain(core, platform, recap_for, &mut effects, &mut pending);
        platform.tick(Instant::now());
        let now = Instant::now();
        let mut wake = next_poll;
        if let Some(at) = core.next_deadline() {
            wake = wake.min(at);
        }
        if let Some(p) = &pending {
            wake = wake.min(p.at);
        }
        if let Some(at) = platform.next_deadline() {
            wake = wake.min(at);
        }
        let msg = rx.recv_timeout(wake.saturating_duration_since(now));
        let now = Instant::now();
        match msg {
            Ok(Msg::Hook(hook)) => {
                println!("hook: {:?}", hook.event.kind);
                effects.extend(core.handle(Input::Hook(hook.event), hook.at));
            }
            Ok(Msg::Line(line)) => match commands(&line, core, now) {
                Control::Quit => return,
                Control::Continue(more) => effects.extend(more),
            },
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => effects.extend(core.tick(now)),
        }
        if now >= next_poll {
            next_poll = now + POLL_FRONT;
            let now_front = platform.frontmost().ok().flatten();
            if now_front != front {
                front.clone_from(&now_front);
                announce_front(now_front.as_ref());
                effects
                    .extend(core.handle(Input::Observed(Observation::Frontmost(now_front)), now));
            }
        }
        if let Some(p) = pending.take_if(|p| now >= p.at) {
            {
                let actual = platform.frontmost().ok().flatten();
                let verified = actual.as_ref().is_some_and(|a| p.target.matches(a));
                effects.extend(core.handle(Input::Observed(Observation::Frontmost(actual)), now));
                effects.extend(
                    core.handle(Input::Observed(Observation::Activation { verified }), now),
                );
            }
        }
    }
}

/// Printed so a user can see which app id the shell reads, and compare it with the home-app
/// list when a switch doesn't happen.
fn announce_front(app: Option<&AppRef>) {
    match app {
        Some(app) => println!("front: {} ({})", app.id, app.name),
        None => println!("front: unknown"),
    }
}

fn drain<P: Platform + Overlay>(
    core: &mut Core,
    platform: &P,
    recap_for: Duration,
    effects: &mut Vec<Effect>,
    pending: &mut Option<PendingVerify>,
) {
    while !effects.is_empty() {
        let batch = std::mem::take(effects);
        let now = Instant::now();
        for effect in batch {
            carry_out(effect, platform, recap_for, core, effects, pending, now);
        }
    }
}

fn carry_out<P: Platform + Overlay>(
    effect: Effect,
    platform: &P,
    recap_for: Duration,
    core: &mut Core,
    out: &mut Vec<Effect>,
    pending: &mut Option<PendingVerify>,
    now: Instant,
) {
    match effect {
        Effect::ShowToast(toast) => {
            println!("TOAST [{:?}] {}", toast.kind, toast.text);
            platform.show_toast(&toast.text);
        }
        Effect::HideToast => {
            println!("toast hidden");
            platform.hide_toast();
        }
        Effect::Activate {
            target,
            verify_after,
        } => {
            match platform.activate(&target) {
                Ok(()) => println!("ACTIVATE {}", target.id),
                Err(e) => println!("ACTIVATE {} failed: {e:?}", target.id),
            }
            *pending = Some(PendingVerify {
                at: now + verify_after,
                target,
            });
        }
        Effect::Probe(kind) => {
            let observation = match kind {
                ProbeKind::Idle => platform
                    .seconds_since_input()
                    .ok()
                    .map(|s| Observation::Idle(Duration::from_secs_f64(s.max(0.0)))),
                ProbeKind::Frontmost => platform.frontmost().ok().map(Observation::Frontmost),
                ProbeKind::RunningApps => {
                    platform.running_apps().ok().map(Observation::RunningApps)
                }
            };
            if let Some(o) = observation {
                out.extend(core.handle(Input::Observed(o), now));
            }
        }
        Effect::ShowRecap(card) => {
            println!("RECAP");
            for line in &card.lines {
                println!("  {line}");
            }
            platform.show_recap(&card.lines, recap_for);
        }
        Effect::PlaySound(sound) => {
            platform.play_sound(sound);
            println!("sound: {sound:?}");
        }
        Effect::AppendRun(record) => println!("run recorded: {record:?}"),
        Effect::SummarizeRecap { .. } => println!("(LLM recap requested; not supported yet)"),
        Effect::Log(log) => println!("log: {}", log.name),
    }
}
