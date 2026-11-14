// SPDX-License-Identifier: Apache-2.0
//! The shell loop (design section 5): one thread sleeps until the next input or
//! `Core::next_deadline`, feeds the core, and carries out its effects against a [`Platform`].
//! Toasts and recaps are printed to the terminal until the overlay UI (M5) exists.
#![forbid(unsafe_code)]

pub mod history;
pub mod paths;
pub mod recap_cli;

use std::io::BufRead as _;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use focus_core::{AppRef, Core, Effect, Input, Observation, ProbeKind, UserAction};
use focus_platform::{HotkeyAction, Hotkeys, Overlay, Platform};
use focus_receiver::Received;

pub enum Msg {
    Hook(Received),
    Line(String),
    /// A finished LLM recap, from the thread that ran the CLI.
    Summary {
        id: u64,
        text: String,
    },
    /// The CLI couldn't produce a summary.
    SummaryFailed {
        id: u64,
    },
}

/// What a terminal command asks of the loop.
pub enum Control {
    Quit,
    Continue(Vec<Effect>),
}

/// How the shell is set up, beyond the core's own config.
pub struct Options {
    /// How long the recap card stays on screen.
    pub recap_for: Duration,
    /// The focus hotkey as it reads on screen (`⌃⌥⌘Space`), or `None` if it is off or failed
    /// to register. Shown under each toast so the user knows what to press.
    pub focus_hint: Option<String>,
    /// Where to run the Claude CLI for LLM recaps, and the channel its replies come back on.
    pub recap: Option<RecapRunner>,
    /// Says whether an app id is a home app, to explain a prompt that was ignored.
    pub is_home: Box<dyn Fn(&str) -> bool>,
    /// Print prompts, commands, recap text and toast text. Off when output goes to a file, so
    /// a log never holds what was typed; then only event names and outcomes are printed.
    pub verbose: bool,
    /// On a return to an editor that supports it, open the session's folder in it (which
    /// focuses the window that has it) instead of only activating the app.
    pub open_project_in_editor: bool,
    /// Where run history lives and how long to keep it, or `None` to keep none.
    pub history: Option<HistoryFile>,
}

/// `runs.jsonl` and its retention.
pub struct HistoryFile {
    pub path: std::path::PathBuf,
    pub retention_days: u32,
}

/// Everything needed to run LLM recaps off the shell thread.
pub struct RecapRunner {
    pub dir: std::path::PathBuf,
    pub replies: Sender<Msg>,
}

/// How often hotkey presses are collected while any hotkey is registered.
const POLL_HOTKEYS: Duration = Duration::from_millis(100);

/// How often the front app is read. Interim: the macOS adapter has no change notification yet.
const POLL_FRONT: Duration = Duration::from_millis(500);

/// Forwards hooks and stdin lines into one queue. Stdin closing is not an error: a daemon may
/// run without a terminal.
pub fn inputs(hooks: Receiver<Received>) -> (Receiver<Msg>, Sender<Msg>) {
    let (tx, rx) = mpsc::channel();
    let hook_tx: Sender<Msg> = tx.clone();
    let handle = tx.clone();
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
    (rx, handle)
}

struct PendingVerify {
    at: Instant,
    target: AppRef,
}

/// Runs until `commands` returns [`Control::Quit`] or the input queue closes.
pub fn run<P: Platform + Overlay + Hotkeys>(
    core: &mut Core,
    platform: &P,
    rx: &Receiver<Msg>,
    options: &Options,
    mut commands: impl FnMut(&str, &mut Core, Instant) -> Control,
) {
    // Like a real shell: say what is in front before any hook arrives, so the next prompt
    // captures the right home window.
    let mut front = platform.frontmost().ok().flatten();
    let mut startup = Vec::new();
    if let Some(file) = &options.history {
        let samples = history::load(&file.path, file.retention_days);
        println!("history: {} past runs loaded", samples.len());
        startup.extend(core.handle(Input::History(samples), Instant::now()));
    }
    startup.extend(core.handle(
        Input::Observed(Observation::Frontmost(front.clone())),
        Instant::now(),
    ));
    announce_front(front.as_ref());
    let mut effects = startup;
    let mut pending: Option<PendingVerify> = None;
    let mut next_poll = Instant::now() + POLL_FRONT;
    loop {
        drain(core, platform, options, &mut effects, &mut pending);
        platform.tick(Instant::now());
        for pressed in platform.take_pressed() {
            let action = match pressed {
                HotkeyAction::Focus => UserAction::ToastAction,
                HotkeyAction::TogglePause => UserAction::TogglePause,
            };
            effects.extend(core.handle(Input::User(action), Instant::now()));
        }
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
        if platform.active() {
            wake = wake.min(now + POLL_HOTKEYS);
        }
        let msg = rx.recv_timeout(wake.saturating_duration_since(now));
        let now = Instant::now();
        match msg {
            Ok(Msg::Hook(hook)) => {
                if options.verbose {
                    println!("hook: {:?}", hook.event.kind);
                } else {
                    println!("hook: {}", kind_name(&hook.event.kind));
                }
                if matches!(
                    hook.event.kind,
                    focus_core::EventKind::UserPromptSubmit { .. }
                ) {
                    warn_if_not_home(platform, options);
                }
                effects.extend(core.handle(Input::Hook(hook.event), hook.at));
            }
            Ok(Msg::Line(line)) => match commands(&line, core, now) {
                Control::Quit => return,
                Control::Continue(more) => effects.extend(more),
            },
            Ok(Msg::Summary { id, text }) => {
                effects.extend(core.handle(Input::Summary { id, text }, now));
            }
            Ok(Msg::SummaryFailed { id }) => {
                effects.extend(core.handle(Input::SummaryFailed { id }, now));
            }
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

/// Runs the CLI on its own thread so the shell loop never waits for it; the reply comes back
/// as a message.
fn summarize(options: &Options, id: u64, prompt: String, timeout: Duration) {
    let Some(runner) = &options.recap else {
        println!("(LLM recap requested, but no working folder is set)");
        return;
    };
    let dir = runner.dir.clone();
    let replies = runner.replies.clone();
    std::thread::spawn(move || {
        match recap_cli::run_cli("claude", recap_cli::CLAUDE_ARGS, &dir, &prompt, timeout) {
            Ok(text) => {
                let _ = replies.send(Msg::Summary { id, text });
            }
            Err(e) => {
                println!("LLM recap failed: {e}");
                let _ = replies.send(Msg::SummaryFailed { id });
            }
        }
    });
}

/// A prompt typed in an app that isn't a home app is ignored, because there is no window to
/// return to. Say so, and say how to fix it.
fn warn_if_not_home<P: Platform>(platform: &P, options: &Options) {
    if let Ok(Some(front)) = platform.frontmost()
        && !(options.is_home)(&front.id)
    {
        println!(
            "note: this prompt came from {} ({}), which is not a home app, so no return is \
             set up for it. To use it, add \"{}\" under [home] extra_apps in config.toml.",
            front.name, front.id, front.id
        );
    }
}

/// The toast text, with a second line saying which key does what.
fn toast_text(text: &str, action: &str, options: &Options) -> String {
    match (&options.focus_hint, action.is_empty()) {
        (Some(key), false) => format!("{text}\n{key}: {action}"),
        _ => text.to_owned(),
    }
}

/// Tries the editor route for a return: true if the editor was asked for the window that has
/// the project open, false to activate the app the ordinary way (the option is off, there is
/// no folder, the app is not such an editor, or the request failed).
fn open_in_editor<P: Platform>(
    platform: &P,
    options: &Options,
    target: &AppRef,
    folder: Option<&str>,
) -> bool {
    let (true, Some(folder)) = (options.open_project_in_editor, folder) else {
        return false;
    };
    match platform.open_project(target, folder) {
        Ok(true) => {
            if options.verbose
                && let Some(url) = focus_platform::editor_url::editor_url(&target.id, folder)
            {
                // The exact URL, so a wrong window can be compared against the folder each
                // window really has open. Only in a terminal: the path is the user's.
                println!("OPEN project in {}: {url}", target.id);
            } else {
                println!("OPEN project in {}", target.id);
            }
            true
        }
        Ok(false) => false,
        Err(e) => {
            println!("could not open the project in {}: {e:?}", target.id);
            false
        }
    }
}

/// The event's name without its fields, which can hold prompts and commands.
fn kind_name(kind: &focus_core::EventKind) -> String {
    format!("{kind:?}")
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("")
        .to_owned()
}

/// Printed so a user can see which app id the shell reads, and compare it with the home-app
/// list when a switch doesn't happen.
fn announce_front(app: Option<&AppRef>) {
    match app {
        Some(app) => println!("front: {} ({})", app.id, app.name),
        None => println!("front: unknown"),
    }
}

fn drain<P: Platform + Overlay + Hotkeys>(
    core: &mut Core,
    platform: &P,
    options: &Options,
    effects: &mut Vec<Effect>,
    pending: &mut Option<PendingVerify>,
) {
    while !effects.is_empty() {
        let batch = std::mem::take(effects);
        let now = Instant::now();
        for effect in batch {
            carry_out(effect, platform, options, core, effects, pending, now);
        }
    }
}

fn carry_out<P: Platform + Overlay + Hotkeys>(
    effect: Effect,
    platform: &P,
    options: &Options,
    core: &mut Core,
    out: &mut Vec<Effect>,
    pending: &mut Option<PendingVerify>,
    now: Instant,
) {
    match effect {
        Effect::ShowToast(toast) => {
            if options.verbose {
                println!("TOAST [{:?}] {}", toast.kind, toast.text);
            } else {
                println!("TOAST [{:?}]", toast.kind);
            }
            platform.show_toast(&toast_text(&toast.text, toast.action, options));
        }
        Effect::HideToast => {
            println!("toast hidden");
            platform.hide_toast();
        }
        Effect::Activate {
            target,
            verify_after,
            folder,
        } => {
            if !open_in_editor(platform, options, &target, folder.as_deref()) {
                match platform.activate(&target) {
                    Ok(()) => println!("ACTIVATE {}", target.id),
                    Err(e) => println!("ACTIVATE {} failed: {e:?}", target.id),
                }
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
            if options.verbose {
                for line in &card.lines {
                    println!("  {line}");
                }
            }
            platform.show_recap(&card.lines, options.recap_for);
        }
        Effect::PlaySound(sound) => {
            platform.play_sound(sound);
            println!("sound: {sound:?}");
        }
        Effect::AppendRun(record) => {
            println!(
                "run recorded ({:?}, {} s)",
                record.outcome,
                record.duration.as_secs()
            );
            if let Some(file) = &options.history
                && let Err(e) = history::append(&file.path, &record, record.started)
            {
                println!("could not save run history: {e}");
            }
        }
        Effect::SummarizeRecap {
            id,
            prompt,
            timeout,
        } => summarize(options, id, prompt, timeout),
        Effect::Log(log) => println!("log: {}", log.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(hint: Option<&str>) -> Options {
        Options {
            recap_for: Duration::from_secs(10),
            focus_hint: hint.map(str::to_owned),
            recap: None,
            is_home: Box::new(|_| true),
            verbose: true,
            open_project_in_editor: false,
            history: None,
        }
    }

    #[test]
    fn a_toast_names_the_key_that_acts_on_it() {
        let with_key = options(Some("⌃⌥⌘Space"));
        assert_eq!(
            toast_text("Claude is done. Go back?", "Go back now", &with_key),
            "Claude is done. Go back?\n⌃⌥⌘Space: Go back now"
        );
    }

    fn app(id: &str) -> AppRef {
        AppRef {
            id: id.to_owned(),
            name: id.to_owned(),
            pid: 1,
            window: None,
        }
    }

    fn fake() -> focus_platform::fake::FakePlatform {
        focus_platform::fake::FakePlatform::new(
            vec![app("com.microsoft.VSCode"), app("com.apple.Terminal")],
            Some(app("com.apple.Terminal")),
        )
    }

    #[test]
    fn an_editor_return_opens_the_project_when_the_option_is_on() {
        let platform = fake();
        let mut on = options(None);
        on.open_project_in_editor = true;
        let vscode = app("com.microsoft.VSCode");
        assert!(open_in_editor(&platform, &on, &vscode, Some("/work/api")));
        assert_eq!(
            platform.projects_opened(),
            vec![("com.microsoft.VSCode".to_owned(), "/work/api".to_owned())]
        );
    }

    #[test]
    fn otherwise_the_app_is_activated_the_ordinary_way() {
        let platform = fake();
        let mut on = options(None);
        on.open_project_in_editor = true;
        let vscode = app("com.microsoft.VSCode");
        // Option off, no folder, or not an editor: the editor route is not taken.
        assert!(!open_in_editor(
            &platform,
            &options(None),
            &vscode,
            Some("/work/api")
        ));
        assert!(!open_in_editor(&platform, &on, &vscode, None));
        assert!(!open_in_editor(
            &platform,
            &on,
            &app("com.apple.Terminal"),
            Some("/work/api")
        ));
        assert!(platform.projects_opened().is_empty());
    }

    #[test]
    fn an_event_name_never_carries_its_fields() {
        let kind = focus_core::EventKind::UserPromptSubmit {
            prompt: "deploy with the secret".into(),
            prompt_chars: 22,
            permission_mode: None,
        };
        assert_eq!(kind_name(&kind), "UserPromptSubmit");
        assert_eq!(kind_name(&focus_core::EventKind::SessionEnd), "SessionEnd");
    }

    #[test]
    fn without_a_hotkey_the_text_is_unchanged() {
        assert_eq!(
            toast_text("Claude is done.", "Go back now", &options(None)),
            "Claude is done."
        );
        assert_eq!(toast_text("Hi", "", &options(Some("K"))), "Hi");
    }
}
