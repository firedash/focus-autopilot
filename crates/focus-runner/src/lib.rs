// SPDX-License-Identifier: Apache-2.0
//! The shell loop (design section 5): one thread sleeps until the next input or
//! `Core::next_deadline`, feeds the core, and carries out its effects against a [`Platform`].
//! Toasts and recaps are printed to the terminal until the overlay UI (M5) exists.
#![forbid(unsafe_code)]

pub mod history;
pub mod paths;
pub mod recap_cli;
pub mod settings;
pub mod stats;
pub mod ui;
pub mod usage;

use std::io::BufRead as _;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use focus_core::{AppRef, Config, Core, Effect, Input, Observation, ProbeKind, Status, UserAction};
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
    /// The settings page saved a new, already validated config.
    Config(Box<Config>),
    /// The statistics page erased the history.
    HistoryCleared,
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
    /// The built-in home apps for this OS.
    pub known_home: Vec<String>,
    /// Built-in and configured home apps, to explain a prompt that was ignored and to tell
    /// time spent with Claude from time spent elsewhere.
    pub home_apps: Vec<String>,
    /// Print prompts, commands, recap text and toast text. Off when output goes to a file, so
    /// a log never holds what was typed; then only event names and outcomes are printed.
    pub verbose: bool,
    /// Where run history lives and how long to keep it, or `None` to keep none.
    pub history: Option<HistoryFile>,
    /// State shared with the statistics and configuration page, or `None` for no page.
    pub ui: Option<Arc<ui::Shared>>,
    /// Record how long each app is in front, for the statistics page.
    pub track_app_time: bool,
}

impl Options {
    /// The settings that follow from `config`; everything optional is off.
    pub fn new(config: &Config, known_home: Vec<String>) -> Self {
        let mut options = Self {
            recap_for: config.ui.recap_shown_for,
            focus_hint: None,
            recap: None,
            known_home,
            home_apps: Vec::new(),
            verbose: true,
            history: None,
            ui: None,
            track_app_time: config.stats.track_app_time,
        };
        options.follow(config);
        options
    }

    /// Takes over the parts of a new config that the shell, not the core, acts on.
    fn follow(&mut self, config: &Config) {
        self.recap_for = config.ui.recap_shown_for;
        self.track_app_time = config.stats.track_app_time;
        self.home_apps = self
            .known_home
            .iter()
            .chain(&config.home.extra_apps)
            .cloned()
            .collect();
        if let Some(history) = &mut self.history {
            history.retention_days = config.history.retention_days;
        }
    }

    pub fn is_home(&self, id: &str) -> bool {
        self.home_apps
            .iter()
            .any(|entry| focus_core::app_id_matches(entry, id))
    }
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

/// How often the running apps and the config file are looked at again.
const POLL_SLOW: Duration = Duration::from_secs(2);

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

/// What the loop carries between iterations.
struct Shell {
    pending: Option<PendingVerify>,
    front: Option<AppRef>,
    tracker: usage::Tracker,
    /// The config file's modification time when it was last read.
    config_seen: Option<SystemTime>,
    next_poll: Instant,
    next_slow: Instant,
    /// What was last printed about the away switch, so each change is printed once.
    away_wait: Option<String>,
    left_off: Option<String>,
}

/// Runs until `commands` returns [`Control::Quit`] or the input queue closes.
pub fn run<P: Platform + Overlay + Hotkeys>(
    core: &mut Core,
    platform: &P,
    rx: &Receiver<Msg>,
    options: &mut Options,
    mut commands: impl FnMut(&str, &mut Core, Instant) -> Control,
) {
    let start = Instant::now();
    // Like a real shell: say what is in front before any hook arrives, so the next prompt
    // captures the right home window.
    let mut shell = Shell {
        pending: None,
        front: platform.frontmost().ok().flatten(),
        tracker: usage::Tracker::new(start),
        config_seen: options
            .ui
            .as_ref()
            .and_then(|ui| modified(&ui.files().config)),
        next_poll: start + POLL_FRONT,
        next_slow: start,
        away_wait: None,
        left_off: None,
    };
    let mut effects = Vec::new();
    if let Some(file) = &options.history {
        let samples = history::load(&file.path, file.retention_days);
        println!("history: {} past runs loaded", samples.len());
        effects.extend(core.handle(Input::History(samples), start));
    }
    effects.extend(core.handle(
        Input::Observed(Observation::Frontmost(shell.front.clone())),
        start,
    ));
    announce_front(shell.front.as_ref());
    loop {
        drain(core, platform, options, &mut effects, &mut shell);
        platform.tick(Instant::now());
        for pressed in platform.take_pressed() {
            let action = match pressed {
                HotkeyAction::Focus => UserAction::ToastAction,
                HotkeyAction::TogglePause => UserAction::TogglePause,
                HotkeyAction::DeclareLong => UserAction::DeclareLong,
            };
            effects.extend(core.handle(Input::User(action), Instant::now()));
        }
        let now = Instant::now();
        let mut wake = shell.next_poll;
        if let Some(at) = core.next_deadline() {
            wake = wake.min(at);
        }
        if let Some(p) = &shell.pending {
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
        let Some(more) = on_message(msg, core, platform, options, &mut shell, &mut commands, now)
        else {
            break;
        };
        effects.extend(more);
        if now >= shell.next_poll {
            shell.next_poll = now + POLL_FRONT;
            effects.extend(poll_front(core, platform, options, &mut shell, now));
            explain_away(core, &mut shell, now);
        }
        if now >= shell.next_slow {
            shell.next_slow = now + POLL_SLOW;
            effects.extend(poll_slow(core, platform, options, &mut shell, now));
        }
        if let Some(p) = shell.pending.take_if(|p| now >= p.at) {
            let actual = platform.frontmost().ok().flatten();
            let verified = actual.as_ref().is_some_and(|a| p.target.matches(a));
            effects.extend(core.handle(Input::Observed(Observation::Frontmost(actual)), now));
            effects.extend(core.handle(Input::Observed(Observation::Activation { verified }), now));
        }
    }
    if let Some(ui) = &options.ui
        && shell.tracker.has_unsaved()
        && let Err(e) = ui.save_usage()
    {
        println!("could not save statistics: {e}");
    }
}

/// Handles what the loop woke up for. `None` means stop.
fn on_message<P: Platform>(
    msg: Result<Msg, RecvTimeoutError>,
    core: &mut Core,
    platform: &P,
    options: &mut Options,
    shell: &mut Shell,
    commands: &mut impl FnMut(&str, &mut Core, Instant) -> Control,
    now: Instant,
) -> Option<Vec<Effect>> {
    Some(match msg {
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
            core.handle(Input::Hook(hook.event), hook.at)
        }
        Ok(Msg::Line(line)) => match commands(&line, core, now) {
            Control::Quit => return None,
            Control::Continue(more) => more,
        },
        Ok(Msg::Summary { id, text }) => core.handle(Input::Summary { id, text }, now),
        Ok(Msg::SummaryFailed { id }) => core.handle(Input::SummaryFailed { id }, now),
        Ok(Msg::Config(config)) => {
            println!("settings changed: applied");
            apply_config(core, options, *config, now)
        }
        Ok(Msg::HistoryCleared) => {
            println!("statistics and run history cleared");
            shell.tracker.reset();
            core.handle(Input::History(Vec::new()), now)
        }
        Err(RecvTimeoutError::Disconnected) => return None,
        Err(RecvTimeoutError::Timeout) => core.tick(now),
    })
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Hands a new config to the core and to the parts of the shell that follow it.
fn apply_config(
    core: &mut Core,
    options: &mut Options,
    config: Config,
    now: Instant,
) -> Vec<Effect> {
    options.follow(&config);
    if let Some(ui) = &options.ui {
        ui.set_config_now(config.clone());
    }
    core.handle(Input::Config(Box::new(config)), now)
}

/// Every half second: credit the time since the last look to the app that was in front, then
/// see whether the front app changed.
fn poll_front<P: Platform>(
    core: &mut Core,
    platform: &P,
    options: &Options,
    shell: &mut Shell,
    now: Instant,
) -> Vec<Effect> {
    let now_front = platform.frontmost().ok().flatten();
    let changed = now_front != shell.front;
    if let Some(ui) = options.ui.as_ref().filter(|_| options.track_app_time) {
        let unix = history::unix_now();
        let idle = platform
            .seconds_since_input()
            .ok()
            .map(|s| Duration::from_secs_f64(s.max(0.0)));
        let moment = shell.front.as_ref().map(|app| usage::Moment {
            app_id: &app.id,
            app_name: &app.name,
            is_home: options.is_home(&app.id),
            claude_working: core.claude_working(),
        });
        let mut recorded = ui.usage();
        shell.tracker.sample(&mut recorded, now, unix, moment, idle);
        let other_app = matches!((&shell.front, &now_front), (Some(a), Some(b)) if a.id != b.id);
        if other_app {
            shell.tracker.switched(&mut recorded, unix);
        }
    }
    if !changed {
        return Vec::new();
    }
    shell.front.clone_from(&now_front);
    announce_front(now_front.as_ref());
    core.handle(Input::Observed(Observation::Frontmost(now_front)), now)
}

/// Prints, once per change, where the next away switch would go and what is holding it back:
/// the first thing to look at when a switch that was expected does not come.
fn explain_away(core: &Core, shell: &mut Shell, now: Instant) {
    let left_off = core
        .left_off()
        .map(|app| format!("{} ({})", app.name, app.id));
    if left_off != shell.left_off {
        if let Some(app) = &left_off {
            println!("where you left off: {app}");
        }
        shell.left_off = left_off;
    }
    let wait = core.away_wait(now);
    if wait != shell.away_wait {
        if let Some(why) = &wait {
            println!("not switching away: {why}");
        }
        shell.away_wait = wait;
    }
}

/// Every two seconds: keep the page's view fresh, save statistics when due, and pick up a
/// config file edited by hand.
fn poll_slow<P: Platform>(
    core: &mut Core,
    platform: &P,
    options: &mut Options,
    shell: &mut Shell,
    now: Instant,
) -> Vec<Effect> {
    let Some(ui) = options.ui.clone() else {
        return Vec::new();
    };
    ui.set_status(match core.status(now) {
        Status::Paused { .. } => "paused",
        Status::Away => "away",
        Status::Running { .. } => "running",
        Status::Idle => "idle",
    });
    if let Ok(apps) = platform.running_apps() {
        ui.set_running_apps(apps);
    }
    if shell.tracker.save_due(now) {
        match ui.save_usage() {
            Ok(()) => shell.tracker.saved(now),
            Err(e) => println!("could not save statistics: {e}"),
        }
    }
    let seen = modified(&ui.files().config);
    if seen == shell.config_seen {
        return Vec::new();
    }
    shell.config_seen = seen;
    let Ok(text) = std::fs::read_to_string(&ui.files().config) else {
        return Vec::new();
    };
    match settings::check(&text, ui.known_home()) {
        // The page's own save lands here too, already applied: nothing to do.
        Ok(config) if config == ui.config_now() => Vec::new(),
        Ok(config) => {
            let restart = settings::restart_needed(&ui.config_now(), &config);
            println!("config.toml changed: applied");
            if let Some(note) = settings::retired_note(&text) {
                println!("note: {note}");
            }
            if !restart.is_empty() {
                println!("restart focusd for: {}", restart.join(", "));
            }
            apply_config(core, options, config, now)
        }
        Err(e) => {
            println!("config.toml changed but was not applied, keeping the last good one: {e}");
            Vec::new()
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
        && !options.is_home(&front.id)
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
    shell: &mut Shell,
) {
    while !effects.is_empty() {
        let batch = std::mem::take(effects);
        let now = Instant::now();
        for effect in batch {
            carry_out(effect, platform, options, core, effects, shell, now);
        }
    }
}

fn carry_out<P: Platform + Overlay + Hotkeys>(
    effect: Effect,
    platform: &P,
    options: &Options,
    core: &mut Core,
    out: &mut Vec<Effect>,
    shell: &mut Shell,
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
        } => {
            match platform.activate(&target) {
                Ok(()) => println!("ACTIVATE {}", target.id),
                Err(e) => println!("ACTIVATE {} failed: {e:?}", target.id),
            }
            shell.pending = Some(PendingVerify {
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
        Effect::Log(log) => {
            println!("log: {}", log.name);
            if let Some(ui) = options.ui.as_ref().filter(|_| options.track_app_time) {
                let mut recorded = ui.usage();
                shell
                    .tracker
                    .log(&mut recorded, log.name, now, history::unix_now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(hint: Option<&str>) -> Options {
        let mut options = Options::new(&Config::default(), vec!["com.apple.Terminal".into()]);
        options.focus_hint = hint.map(str::to_owned);
        options
    }

    #[test]
    fn options_follow_a_changed_config() {
        let mut options = options(None);
        assert!(options.is_home("com.apple.Terminal") && !options.is_home("com.example.Editor"));
        assert!(options.track_app_time);
        let mut config = Config::default();
        config.home.extra_apps = vec!["com.example.Editor".into()];
        config.ui.recap_shown_for = Duration::from_secs(25);
        config.stats.track_app_time = false;
        options.follow(&config);
        assert!(options.is_home("com.example.Editor") && options.is_home("com.apple.Terminal"));
        assert_eq!(options.recap_for, Duration::from_secs(25));
        assert!(!options.track_app_time);
    }

    #[test]
    fn a_toast_names_the_key_that_acts_on_it() {
        let with_key = options(Some("⌃⌥⌘Space"));
        assert_eq!(
            toast_text("Claude is done. Go back?", "Go back now", &with_key),
            "Claude is done. Go back?\n⌃⌥⌘Space: Go back now"
        );
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
