// SPDX-License-Identifier: Apache-2.0
//! The whole shell loop against the fake platform, with a real clock: a prompt, the user
//! leaving, Claude finishing, the toast, the hotkey, the return and the recap; and beside
//! that, time per app being recorded, a settings change applied live, a hand-edited config
//! picked up, and the statistics saved on the way out.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use focus_core::{AgentEvent, AppRef, Config, Core, EventKind, Input, UserAction};
use focus_platform::fake::FakePlatform;
use focus_platform::hotkey::Keys;
use focus_platform::{HotkeyAction, Hotkeys as _, Platform as _};
use focus_receiver::{Received, UiBackend as _};
use focus_runner::usage::SLOT_SECS;
use focus_runner::{Control, HistoryFile, Msg, Options, ui};

const TERMINAL: &str = "com.apple.Terminal";
const BROWSER: &str = "com.example.Browser";

fn app(id: &str, name: &str) -> AppRef {
    AppRef {
        id: id.to_owned(),
        name: name.to_owned(),
        pid: 1,
        window: None,
    }
}

fn hook(kind: EventKind) -> Msg {
    Msg::Hook(Received {
        at: Instant::now(),
        event: AgentEvent {
            session_id: "s1".to_owned(),
            prompt_id: Some("p1".to_owned()),
            cwd: "/work/api".to_owned(),
            project: "abc".to_owned(),
            kind,
        },
    })
}

/// Waits until `ready` holds, failing the test with `what` if it never does.
fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct Setup {
    dir: PathBuf,
    shared: Arc<ui::Shared>,
    platform: FakePlatform,
    tx: Sender<Msg>,
    /// Taken by the loop's thread: a receiver can't be shared, only moved.
    rx: std::sync::Mutex<Option<std::sync::mpsc::Receiver<Msg>>>,
    known_home: Vec<String>,
}

fn setup(name: &str) -> Setup {
    let dir = std::env::temp_dir().join(format!("focus-loop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), "[receiver]\ntoken = \"t\"\n").unwrap();
    let (tx, rx) = channel();
    let known_home = vec![TERMINAL.to_owned()];
    let shared = Arc::new(ui::Shared::new(
        Config::default(),
        known_home.clone(),
        ui::Files {
            config: dir.join("config.toml"),
            runs: Some(dir.join("runs.jsonl")),
            usage: Some(dir.join("usage.json")),
        },
        tx.clone(),
    ));
    let terminal = app(TERMINAL, "Terminal");
    let platform = FakePlatform::new(
        vec![terminal.clone(), app(BROWSER, "Browser")],
        Some(terminal),
    );
    // Someone is at the keyboard, so time in front counts.
    platform.set_idle_seconds(1.0);
    Setup {
        dir,
        shared,
        platform,
        tx,
        rx: std::sync::Mutex::new(Some(rx)),
        known_home,
    }
}

/// Runs the loop on its own thread while `script` plays the user, Claude and the page.
fn with_loop(s: &Setup, script: impl FnOnce()) {
    let config = s.shared.config_now();
    let mut options = Options::new(&config, s.known_home.clone());
    options.verbose = false;
    options.ui = Some(s.shared.clone());
    options.history = Some(HistoryFile {
        path: s.dir.join("runs.jsonl"),
        retention_days: 90,
    });
    let mut core = Core::new(config, s.known_home.clone());
    let rx =
        s.rx.lock()
            .unwrap()
            .take()
            .expect("the loop runs once per setup");
    let (core, options, platform) = (&mut core, &mut options, &s.platform);
    std::thread::scope(|scope| {
        let running = scope.spawn(move || {
            focus_runner::run(core, platform, &rx, options, |line, core, now| match line {
                "quit" => Control::Quit,
                "pause" => {
                    Control::Continue(core.handle(Input::User(UserAction::TogglePause), now))
                }
                _ => Control::Continue(Vec::new()),
            });
        });
        script();
        s.tx.send(Msg::Line("quit".to_owned())).unwrap();
        running.join().unwrap();
    });
}

#[test]
fn a_run_ends_with_a_toast_and_the_hotkey_brings_the_user_back() {
    let s = setup("return");
    let problems = s.platform.register(&Keys {
        focus: "Control+Option+Space",
        pause: "",
        declare_long: "",
    });
    assert!(problems.is_empty(), "{problems:?}");
    with_loop(&s, || {
        s.tx.send(hook(EventKind::UserPromptSubmit {
            prompt: "Add pagination".to_owned(),
            prompt_chars: 14,
            permission_mode: None,
        }))
        .unwrap();
        // The user wanders off; the loop notices on its next look at the front app.
        std::thread::sleep(Duration::from_millis(700));
        s.platform.set_front(Some(app(BROWSER, "Browser")));
        // Long enough for at least two looks at the front app: time is credited per look.
        std::thread::sleep(Duration::from_millis(1_600));
        assert_eq!(
            s.platform.toast(),
            None,
            "nothing is shown while Claude works"
        );
        s.tx.send(hook(EventKind::Stop {
            stop_hook_active: false,
            last_assistant_message: "Pagination added.".to_owned(),
            background_tasks: 0,
        }))
        .unwrap();
        wait_for("the return prompt", || s.platform.toast().is_some());
        assert_eq!(
            s.platform.toast().unwrap(),
            "Claude is done in api. Go back?",
            "no hotkey hint is configured on these options, so the text is the core's"
        );
        assert_eq!(s.platform.sounds_played(), 1);
        // One key press: back in the terminal, toast gone, recap up.
        s.platform.press(HotkeyAction::Focus);
        wait_for("the return to the terminal", || {
            s.platform
                .frontmost()
                .unwrap()
                .is_some_and(|front| front.id == TERMINAL)
        });
        wait_for("the recap card", || s.platform.recap().is_some());
        assert_eq!(s.platform.toast(), None);
        // Stay a moment, so the loop's next look at the front app sees the switch back.
        std::thread::sleep(Duration::from_millis(700));
        let recap = s.platform.recap().unwrap();
        assert_eq!(recap[0], "Project: api");
        assert!(
            recap
                .iter()
                .any(|line| line == "Claude:  Pagination added."),
            "{recap:?}"
        );
    });

    // Time in front was recorded for both apps, split by whether Claude was working.
    let usage = s.shared.usage().clone();
    let slot = usage.slots.values().next().expect("some time was recorded");
    assert!(slot.apps[TERMINAL].ms >= 500, "{slot:?}");
    assert!(slot.apps[BROWSER].ms >= 500, "{slot:?}");
    assert!(
        slot.apps[BROWSER].claude_ms >= 400,
        "the browser was in front while Claude worked"
    );
    assert!(
        slot.claude_away_ms >= 400 && slot.claude_home_ms >= 400,
        "{slot:?}"
    );
    assert!(slot.switches >= 2, "terminal to browser and back: {slot:?}");
    assert_eq!(slot.reactions, 1, "one call, one return");
    assert_eq!(usage.names[BROWSER], "Browser");
    assert!(
        usage
            .slots
            .keys()
            .all(|slot_no| slot_no * SLOT_SECS > 1_700_000_000)
    );
    // And saved on the way out, so the next launch has it.
    assert!(
        s.dir.join("usage.json").is_file(),
        "statistics are saved when the loop ends"
    );
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn settings_changes_reach_the_running_core_from_the_page_and_from_the_file() {
    let s = setup("settings");
    with_loop(&s, || {
        // From the page: saved to the file and applied at once.
        let reply = s
            .shared
            .set_config(br#"{"values":{"stats.track_app_time":false,"ui.recap_seconds":25}}"#)
            .unwrap();
        assert!(reply.contains("\"ui.recap_seconds\":25"));
        std::thread::sleep(Duration::from_millis(300));
        let on_disk = std::fs::read_to_string(s.dir.join("config.toml")).unwrap();
        assert!(on_disk.contains("recap_seconds = 25") && on_disk.contains("token = \"t\""));

        // With recording off, time in front stops accumulating.
        let before: u64 = s
            .shared
            .usage()
            .slots
            .values()
            .flat_map(|slot| slot.apps.values())
            .map(|a| a.ms)
            .sum();
        std::thread::sleep(Duration::from_millis(1_200));
        let after: u64 = s
            .shared
            .usage()
            .slots
            .values()
            .flat_map(|slot| slot.apps.values())
            .map(|a| a.ms)
            .sum();
        assert_eq!(before, after, "recording was turned off");

        // By hand: the loop notices the file changed and applies it.
        let edited = on_disk.replace("recap_seconds = 25", "recap_seconds = 40");
        std::fs::write(s.dir.join("config.toml"), edited).unwrap();
        wait_for("the hand edit to be applied", || {
            s.shared.config_now().ui.recap_shown_for == Duration::from_secs(40)
        });

        // A broken hand edit is not applied; the last good config stays.
        std::fs::write(s.dir.join("config.toml"), "[ui]\nrecap_secs = 1\n").unwrap();
        std::thread::sleep(Duration::from_millis(2_600));
        assert_eq!(
            s.shared.config_now().ui.recap_shown_for,
            Duration::from_secs(40)
        );
    });
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn clearing_statistics_from_the_page_reaches_the_loop() {
    let s = setup("clear");
    with_loop(&s, || {
        std::thread::sleep(Duration::from_millis(1_200));
        assert!(
            !s.shared.usage().slots.is_empty(),
            "some time was recorded first"
        );
        let stats = s.shared.clear_stats(0);
        assert!(stats.contains("\"tracked_s\":0"), "{stats}");
        // The loop keeps running and keeps recording afterwards.
        std::thread::sleep(Duration::from_millis(1_200));
        assert!(
            !s.shared.usage().slots.is_empty(),
            "recording continues after a clear"
        );
    });
    let _ = std::fs::remove_dir_all(&s.dir);
}
