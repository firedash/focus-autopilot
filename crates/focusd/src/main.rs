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
        let is_home = |id: &str| {
            apps.iter().any(|a| a.eq_ignore_ascii_case(id))
                || config
                    .home
                    .extra_apps
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(id))
        };
        config
            .validate(is_home)
            .map_err(|e| format!("invalid config: {e:?}"))?;
        Ok((config, apps))
    }

    pub fn run() -> Result<(), String> {
        let mut config_path = paths::config_path().ok_or("no HOME")?;
        let mut port = None;
        let mut list_apps = false;
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
                "--help" | "-h" => {
                    println!("usage: focusd [--config FILE] [--port N] [--list-apps]");
                    return Ok(());
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        let platform = MacPlatform::new();
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
        if config.away.apps.is_empty() {
            println!("manual mode: no away app, so nothing switches you away on its own.");
        } else {
            println!("away apps: {}.", config.away.apps.join(", "));
        }
        let problems = platform.register(&config.hotkeys.focus_key, &config.hotkeys.pause);
        for problem in &problems {
            println!("hotkey problem: {problem}");
        }
        let focus_hint = match hotkey::parse(&config.hotkeys.focus_key) {
            Ok(Some(chord)) if !problems.iter().any(|p| p.starts_with("focus")) => {
                Some(hotkey::pretty(&chord))
            }
            _ => None,
        };
        match &focus_hint {
            Some(key) => println!("press {key} to act on a toast."),
            None => println!("no focus hotkey: type `click` here to act on a toast."),
        }
        println!("Commands: click, pause, resume, quit.");

        let rx = focus_runner::inputs(hook_rx);
        let options = focus_runner::Options {
            recap_for: config.ui.recap_shown_for,
            focus_hint,
        };
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
