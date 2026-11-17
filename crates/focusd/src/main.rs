// SPDX-License-Identifier: Apache-2.0
//! `focusd`: the daemon. Runs in a terminal for now; toasts and recaps print there (the overlay
//! UI is milestone M5). Reads `config.toml` from the data folder, which `focusctl
//! install-hooks` creates along with the token.
//!
//! Commands on stdin: `click` (act on the toast), `pause`, `resume`, `quit`.
#![forbid(unsafe_code)]

#[cfg(not(any(target_os = "macos", windows)))]
fn main() {
    eprintln!("focusd needs macOS or Windows. On other systems use focus-headless; see STATUS.md.");
    std::process::exit(2);
}

#[cfg(any(target_os = "macos", windows))]
fn main() {
    if let Err(message) = native::run() {
        eprintln!("focusd: {message}");
        std::process::exit(1);
    }
}

/// The daemon on a system that has a platform adapter.
#[cfg(any(target_os = "macos", windows))]
mod native {
    use std::io::IsTerminal as _;
    use std::path::PathBuf;
    use std::sync::{Arc, mpsc};
    use std::time::Instant;

    use focus_core::{Config, Core, Input, UserAction};
    use focus_platform::hotkey;
    use focus_platform::known_apps;
    #[cfg(target_os = "macos")]
    use focus_platform::macos::MacPlatform as Native;
    #[cfg(windows)]
    use focus_platform::windows::WinPlatform as Native;
    use focus_platform::{Hotkeys as _, Platform as _};
    use focus_receiver::{Settings, config_file, random_hex};
    use focus_runner::{Control, paths, ui};

    fn load(path: &PathBuf, port: Option<u16>) -> Result<(Config, Vec<String>), String> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "{}: {e}. Run `focusctl install-hooks` first.",
                path.display()
            )
        })?;
        let apps = known_apps::for_this_os();
        // The same check the settings page and a hand edit go through, with the same wording.
        let mut config = focus_runner::settings::check(&text, &apps)
            .map_err(|problem| format!("{}: {problem}", path.display()))?;
        if let Some(note) = focus_runner::settings::retired_note(&text) {
            println!("note: {note}");
        }
        if let Some(port) = port {
            config.receiver.port = port;
        }
        if config.receiver.token.is_empty() {
            return Err("config.toml has no receiver.token. Run `focusctl install-hooks`.".into());
        }
        Ok((config, apps))
    }

    fn keys(config: &Config) -> hotkey::Keys<'_> {
        hotkey::Keys {
            focus: &config.hotkeys.focus_key,
            pause: &config.hotkeys.pause,
            declare_long: &config.hotkeys.declare_long,
        }
    }

    /// Prints how this run is set up, registers the hotkeys, and returns the focus key as it
    /// reads on screen.
    fn announce(platform: &Native, config: &Config) -> Option<String> {
        if config.switching.auto_return {
            println!(
                "returns: when Claude is done or needs you, a {} s countdown, then you are brought back.",
                config.countdown.return_length.as_secs()
            );
        } else {
            println!("returns: a box asks, and the focus key takes you back.");
        }
        match (config.away.apps.is_empty(), config.away.resume_last_app) {
            (false, true) => println!(
                "switching away: to where you were last called back from, or else {}.",
                config.away.apps.join(", ")
            ),
            (false, false) => println!("switching away: to {}.", config.away.apps.join(", ")),
            (true, true) => println!(
                "switching away: once you have been called back, the next long prompt sends you back \
                 to the app you were in."
            ),
            (true, false) => println!("switching away: off, nothing switches you away."),
        }
        let focus_hint = setup_hotkeys(platform, config);
        match &focus_hint {
            Some(key) => println!("press {key} to act on a toast."),
            None => println!("no focus hotkey: type `click` here to act on a toast."),
        }
        if config.recap.mode == focus_core::RecapMode::Llm {
            println!("recap: summarised by your `claude` CLI (recap.mode = \"llm\").");
        }
        focus_hint
    }

    /// The shell's settings: where to keep history and run the recap, and what to print.
    /// Printing text is for a terminal; a launch agent's log file must not hold what was typed.
    fn build_options(
        config: &Config,
        apps: &[String],
        focus_hint: Option<String>,
        replies: std::sync::mpsc::Sender<focus_runner::Msg>,
        verbose: bool,
        shared: Arc<ui::Shared>,
    ) -> focus_runner::Options {
        let mut options = focus_runner::Options::new(config, apps.to_vec());
        options.focus_hint = focus_hint;
        options.verbose = verbose;
        options.recap = paths::data_dir().map(|dir| focus_runner::RecapRunner {
            dir: dir.join("recap-workdir"),
            replies,
        });
        options.history = paths::data_dir().map(|dir| focus_runner::HistoryFile {
            path: dir.join("runs.jsonl"),
            retention_days: config.history.retention_days,
        });
        options.ui = Some(shared);
        options
    }

    /// `focusd --test-hotkeys`: registers the configured hotkeys and prints every press for 60
    /// seconds, and nothing else, so a hotkey that does nothing can be told apart from the rest
    /// of the app. Uses the config's chords if there is a config, else the defaults.
    fn test_hotkeys(platform: &Native, config_path: &PathBuf) -> Result<(), String> {
        use focus_platform::{HotkeyAction, Overlay as _};
        let config = std::fs::read_to_string(config_path)
            .ok()
            .and_then(|text| config_file::parse(&text).ok())
            .unwrap_or_default();
        let problems = platform.register(&keys(&config));
        for problem in &problems {
            println!("problem: {problem}");
        }
        if !platform.active() {
            return Err("no hotkey could be registered, so there is nothing to test".into());
        }
        println!(
            "Registered focus = `{}`, pause = `{}`, declare-long = `{}` (empty means off). \
             Press them now, from any app. Stops in 60 s.",
            config.hotkeys.focus_key, config.hotkeys.pause, config.hotkeys.declare_long
        );
        let end = Instant::now() + std::time::Duration::from_secs(60);
        let mut presses = 0;
        while Instant::now() < end {
            platform.tick(Instant::now());
            for pressed in platform.take_pressed() {
                presses += 1;
                match pressed {
                    HotkeyAction::Focus => println!("pressed: focus key"),
                    HotkeyAction::TogglePause => println!("pressed: pause key"),
                    HotkeyAction::DeclareLong => println!("pressed: declare-long key"),
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if presses == 0 {
            println!(
                "No presses seen. Registration reported no error, so the OS accepted the chord"
            );
            println!("but nothing was delivered: the event loop is the likely cause.");
        } else {
            println!("{presses} press(es) seen: the hotkeys work.");
        }
        Ok(())
    }

    /// Registers the hotkeys, reports problems, and returns the focus key as it reads on screen.
    fn setup_hotkeys(platform: &Native, config: &Config) -> Option<String> {
        let problems = platform.register(&keys(config));
        for problem in &problems {
            println!("hotkey problem: {problem}");
        }
        match hotkey::parse(&config.hotkeys.focus_key) {
            Ok(Some(chord)) if !problems.iter().any(|p| p.starts_with("focus")) => {
                Some(hotkey::pretty_here(&chord))
            }
            _ => None,
        }
    }

    /// `focusd --list-apps`: the id and name of every running app, one line each. On Windows
    /// the platform lists every window, so repeats are dropped.
    fn print_apps(platform: &Native) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for app in platform.running_apps().map_err(|e| format!("{e:?}"))? {
            if seen.insert(app.id.to_ascii_lowercase()) {
                println!("{}\t{}", app.id, app.name);
            }
        }
        Ok(())
    }

    /// Settings the platform adapter itself reads, once, at startup.
    #[cfg(windows)]
    fn platform_settings(platform: &Native, config_path: &PathBuf) {
        if let Ok(text) = std::fs::read_to_string(config_path)
            && let Ok(config) = config_file::parse(&text)
        {
            platform.set_force_foreground(config.switching.force_foreground);
        }
    }

    #[cfg(not(windows))]
    fn platform_settings(_platform: &Native, _config_path: &PathBuf) {}

    pub fn run() -> Result<(), String> {
        let mut config_path = paths::config_path().ok_or("no HOME")?;
        let mut port = None;
        let mut list_apps = false;
        let mut force_verbose = false;
        let mut test_hotkeys_only = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    config_path = PathBuf::from(args.next().ok_or("--config needs a path")?);
                }
                "--port" => {
                    port = Some(
                        args.next()
                            .ok_or("--port needs a number")?
                            .parse::<u16>()
                            .map_err(|e| e.to_string())?,
                    );
                }
                "--list-apps" => list_apps = true,
                "--verbose" => force_verbose = true,
                "--test-hotkeys" => test_hotkeys_only = true,
                "--help" | "-h" => {
                    println!(
                        "usage: focusd [--config FILE] [--port N] [--list-apps] [--verbose] [--test-hotkeys]"
                    );
                    return Ok(());
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        let platform = Native::new();
        platform_settings(&platform, &config_path);
        if test_hotkeys_only {
            return test_hotkeys(&platform, &config_path);
        }
        if list_apps {
            return print_apps(&platform);
        }

        let (config, apps) = load(&config_path, port)?;

        let (hook_tx, hook_rx) = mpsc::channel();
        let salt = if config.history.salt.is_empty() {
            random_hex().map_err(|e| e.to_string())?
        } else {
            config.history.salt.clone()
        };
        let (rx, replies) = focus_runner::inputs(hook_rx);
        let data = paths::data_dir();
        let shared = Arc::new(ui::Shared::new(
            config.clone(),
            apps.clone(),
            ui::Files {
                config: config_path.clone(),
                runs: data.as_ref().map(|dir| dir.join("runs.jsonl")),
                usage: data.as_ref().map(|dir| dir.join("usage.json")),
            },
            replies.clone(),
        ));
        let listener = focus_receiver::spawn_with_ui(
            Settings {
                port: config.receiver.port,
                token: config.receiver.token.clone(),
                salt,
            },
            hook_tx,
            Some(shared.clone()),
        )
        .map_err(|e| format!("can't listen on 127.0.0.1:{}: {e}", config.receiver.port))?;
        println!("focusd listening on http://{}/hook", listener.addr);
        let focus_hint = announce(&platform, &config);
        println!("statistics and settings: run `focusctl ui`.");
        println!("Commands: click, pause, resume, quit.");

        let verbose = force_verbose || std::io::stdout().is_terminal();
        let mut options = build_options(&config, &apps, focus_hint, replies, verbose, shared);
        let mut core = Core::new(config, apps);
        focus_runner::run(
            &mut core,
            &platform,
            &rx,
            &mut options,
            |line: &str, core: &mut Core, now: Instant| {
                let action = match line.trim() {
                    "quit" | "exit" => return Control::Quit,
                    "click" => UserAction::ToastAction,
                    "pause" => UserAction::TogglePause,
                    "resume" => UserAction::Resume,
                    "" => return Control::Continue(Vec::new()),
                    other => {
                        println!("unknown command {other}");
                        return Control::Continue(Vec::new());
                    }
                };
                Control::Continue(core.handle(Input::User(action), now))
            },
        );
        drop(listener);
        Ok(())
    }
}
