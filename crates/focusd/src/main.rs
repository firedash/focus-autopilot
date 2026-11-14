// SPDX-License-Identifier: Apache-2.0
//! `focusd`: the daemon. Runs in a terminal for now; toasts and recaps print there (the overlay
//! UI is milestone M5). Reads `config.toml` from the data folder, which `focusctl
//! install-hooks` creates along with the token.
//!
//! Commands on stdin: `click` (act on the toast), `pause`, `resume`, `quit`.
#![forbid(unsafe_code)]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("focusd needs macOS. On other systems use focus-headless; see STATUS.md.");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    if let Err(message) = mac::run() {
        eprintln!("focusd: {message}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::IsTerminal as _;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Instant;

    use focus_core::{Config, Core, Input, UserAction};
    use focus_platform::hotkey;
    use focus_platform::known_apps;
    use focus_platform::macos::MacPlatform;
    use focus_platform::{Hotkeys as _, Platform as _};
    use focus_receiver::{Settings, config_file, random_hex};
    use focus_runner::{Control, paths};

    fn load(path: &PathBuf, port: Option<u16>) -> Result<(Config, Vec<String>), String> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "{}: {e}. Run `focusctl install-hooks` first.",
                path.display()
            )
        })?;
        let mut config = config_file::parse(&text).map_err(|e| e.to_string())?;
        if let Some(port) = port {
            config.receiver.port = port;
        }
        if config.receiver.token.is_empty() {
            return Err("config.toml has no receiver.token. Run `focusctl install-hooks`.".into());
        }
        let apps = known_apps::for_this_os();
        config
            .validate(|id| home_match(&apps, &config, id))
            .map_err(|e| format!("invalid config: {e:?}"))?;
        Ok((config, apps))
    }

    /// Prints how this run is set up, registers the hotkeys, and returns the focus key as it
    /// reads on screen.
    fn announce(platform: &MacPlatform, config: &Config) -> Option<String> {
        if config.away.apps.is_empty() {
            println!("manual mode: no away app, so nothing switches you away on its own.");
        } else {
            println!("away apps: {}.", config.away.apps.join(", "));
        }
        let focus_hint = setup_hotkeys(platform, config);
        match &focus_hint {
            Some(key) => println!("press {key} to act on a toast."),
            None => println!("no focus hotkey: type `click` here to act on a toast."),
        }
        if config.returns.open_project_in_editor {
            println!(
                "WARNING: open_project_in_editor is experimental. In one test it brought the right \
                 VS Code window forward but left its terminal without the running Claude session. \
                 Do not use it with work you care about."
            );
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
    ) -> focus_runner::Options {
        let home_apps: Vec<String> = apps
            .iter()
            .chain(&config.home.extra_apps)
            .cloned()
            .collect();
        let retention_days = config.history.retention_days;
        focus_runner::Options {
            recap_for: config.ui.recap_shown_for,
            focus_hint,
            recap: paths::data_dir().map(|dir| focus_runner::RecapRunner {
                dir: dir.join("recap-workdir"),
                replies,
            }),
            verbose,
            open_project_in_editor: config.returns.open_project_in_editor,
            history: paths::data_dir().map(|dir| focus_runner::HistoryFile {
                path: dir.join("runs.jsonl"),
                retention_days,
            }),
            is_home: Box::new(move |id| {
                home_apps
                    .iter()
                    .any(|entry| focus_core::app_id_matches(entry, id))
            }),
        }
    }

    /// `focusd --test-hotkeys`: registers the configured hotkeys and prints every press for 60
    /// seconds, and nothing else, so a hotkey that does nothing can be told apart from the rest
    /// of the app. Uses the config's chords if there is a config, else the defaults.
    fn test_hotkeys(platform: &MacPlatform, config_path: &PathBuf) -> Result<(), String> {
        use focus_platform::{HotkeyAction, Overlay as _};
        let config = std::fs::read_to_string(config_path)
            .ok()
            .and_then(|text| config_file::parse(&text).ok())
            .unwrap_or_default();
        let problems = platform.register(&config.hotkeys.focus_key, &config.hotkeys.pause);
        for problem in &problems {
            println!("problem: {problem}");
        }
        if !platform.active() {
            return Err("no hotkey could be registered, so there is nothing to test".into());
        }
        println!(
            "Registered focus = `{}`, pause = `{}`. Press them now, from any app. Stops in 60 s.",
            config.hotkeys.focus_key, config.hotkeys.pause
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
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if presses == 0 {
            println!(
                "No presses seen. Registration reported no error, so macOS accepted the chord"
            );
            println!("but nothing was delivered: the event loop is the likely cause.");
        } else {
            println!("{presses} press(es) seen: the hotkeys work.");
        }
        Ok(())
    }

    /// Built-in and configured home apps, with the same matching the core uses.
    fn home_match(apps: &[String], config: &Config, id: &str) -> bool {
        apps.iter()
            .chain(&config.home.extra_apps)
            .any(|entry| focus_core::app_id_matches(entry, id))
    }

    /// Registers the hotkeys, reports problems, and returns the focus key as it reads on screen.
    fn setup_hotkeys(platform: &MacPlatform, config: &Config) -> Option<String> {
        let problems = platform.register(&config.hotkeys.focus_key, &config.hotkeys.pause);
        for problem in &problems {
            println!("hotkey problem: {problem}");
        }
        match hotkey::parse(&config.hotkeys.focus_key) {
            Ok(Some(chord)) if !problems.iter().any(|p| p.starts_with("focus")) => {
                Some(hotkey::pretty(&chord))
            }
            _ => None,
        }
    }

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
        let platform = MacPlatform::new();
        if test_hotkeys_only {
            return test_hotkeys(&platform, &config_path);
        }
        if list_apps {
            for app in platform.running_apps().map_err(|e| format!("{e:?}"))? {
                println!("{}\t{}", app.id, app.name);
            }
            return Ok(());
        }

        let (config, apps) = load(&config_path, port)?;

        let (hook_tx, hook_rx) = mpsc::channel();
        let salt = if config.history.salt.is_empty() {
            random_hex().map_err(|e| e.to_string())?
        } else {
            config.history.salt.clone()
        };
        let listener = focus_receiver::spawn(
            Settings {
                port: config.receiver.port,
                token: config.receiver.token.clone(),
                salt,
            },
            hook_tx,
        )
        .map_err(|e| format!("can't listen on 127.0.0.1:{}: {e}", config.receiver.port))?;
        println!("focusd listening on http://{}/hook", listener.addr);
        let focus_hint = announce(&platform, &config);
        println!("Commands: click, pause, resume, quit.");

        let (rx, replies) = focus_runner::inputs(hook_rx);
        let verbose = force_verbose || std::io::stdout().is_terminal();
        let options = build_options(&config, &apps, focus_hint, replies, verbose);
        let mut core = Core::new(config, apps);
        focus_runner::run(
            &mut core,
            &platform,
            &rx,
            &options,
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
