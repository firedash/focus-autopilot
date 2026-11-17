// SPDX-License-Identifier: Apache-2.0
//! Dev-only runner (design section 5). Receives real hook POSTs on 127.0.0.1, feeds the core
//! with a real clock, and carries effects out against a fake platform, printing each one.
//!
//! Type commands on stdin to play the user and the OS:
//!   front <id>   bring an app to the front        idle <seconds>   set idle time
//!   click        click the toast / focus key      pause | resume   toggle pause
//!   quit
//!
//! It keeps its files in `FOCUS_AUTOPILOT_HOME`, or in a `focus-headless` folder in the temp
//! directory, and prints the address of the statistics and configuration page.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use focus_core::{AppRef, Config, Core, Input, Observation, UserAction};
use focus_platform::fake::FakePlatform;
use focus_platform::known_apps;
use focus_receiver::{Received, Settings, random_hex};
use focus_runner::{Control, settings, ui};

fn main() {
    if let Err(message) = run() {
        eprintln!("focus-headless: {message}");
        std::process::exit(1);
    }
}

fn app(id: &str) -> AppRef {
    AppRef {
        id: id.to_owned(),
        // `com.apple.Notes` reads as `Notes`, as a real platform would name it.
        name: id.rsplit('.').next().unwrap_or(id).to_owned(),
        pid: 100,
        window: None,
    }
}

struct Args {
    config_path: Option<PathBuf>,
    port: Option<u16>,
    home: String,
    away: String,
}

/// `Ok(None)` after printing help.
fn parse_args() -> Result<Option<Args>, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        config_path: None,
        port: None,
        home: "home-terminal".to_owned(),
        away: "away-notes".to_owned(),
    };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--config" => parsed.config_path = Some(PathBuf::from(value("--config")?)),
            "--port" => {
                parsed.port = Some(value("--port")?.parse::<u16>().map_err(|e| e.to_string())?);
            }
            "--home-app" => parsed.home = value("--home-app")?,
            "--away-app" => parsed.away = value("--away-app")?,
            "--help" | "-h" => {
                println!(
                    "usage: focus-headless [--config FILE] [--port N] [--home-app ID] [--away-app ID]"
                );
                return Ok(None);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Some(parsed))
}

/// Where this runner keeps its files: `FOCUS_AUTOPILOT_HOME`, or a folder in the temp dir so
/// it never touches a real install's data.
fn data_dir() -> PathBuf {
    std::env::var_os("FOCUS_AUTOPILOT_HOME").map_or_else(
        || std::env::temp_dir().join("focus-headless"),
        PathBuf::from,
    )
}

/// Reads the config, creating a minimal one with a fresh token if there is none.
fn load_config(path: &Path, known_home: &[String]) -> Result<Config, String> {
    if !path.exists() {
        let token = random_hex().map_err(|e| e.to_string())?;
        settings::write(path, &format!("[receiver]\ntoken = \"{token}\"\n"))?;
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut config = settings::check(&text, known_home)?;
    if let Some(note) = settings::retired_note(&text) {
        println!("note: {note}");
    }
    if config.receiver.token.is_empty() {
        config.receiver.token = random_hex().map_err(|e| e.to_string())?;
    }
    Ok(config)
}

fn run() -> Result<(), String> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };
    let dir = data_dir();
    let config_path = args
        .config_path
        .clone()
        .unwrap_or_else(|| dir.join("config.toml"));
    let mut known_home = known_apps::for_this_os();
    known_home.push(args.home.clone());
    let config = load_config(&config_path, &known_home)?;
    let token = config.receiver.token.clone();
    let port = args.port.unwrap_or(config.receiver.port);

    let (hook_tx, hook_rx) = mpsc::channel::<Received>();
    let (rx, replies) = focus_runner::inputs(hook_rx);
    let shared = Arc::new(ui::Shared::new(
        config.clone(),
        known_home.clone(),
        ui::Files {
            config: config_path,
            runs: Some(dir.join("runs.jsonl")),
            usage: Some(dir.join("usage.json")),
        },
        replies.clone(),
    ));
    let salt = random_hex().map_err(|e| e.to_string())?;
    let listener = focus_receiver::spawn_with_ui(
        Settings {
            port,
            token: token.clone(),
            salt,
        },
        hook_tx,
        Some(shared.clone()),
    )
    .map_err(|e| format!("can't listen on 127.0.0.1:{port}: {e}"))?;

    let platform = FakePlatform::new(
        vec![app(&args.home), app(&args.away)],
        Some(app(&args.home)),
    );
    platform.set_idle_seconds(60.0);

    println!("listening on http://{}/hook", listener.addr);
    println!("token (header X-Focus-Token): {token}");
    println!("ui: http://{}/ui/#t={token}", listener.addr);
    println!(
        "home app: {}, other app: {}; you are in {}. Type `quit` to stop.",
        args.home, args.away, args.home
    );

    let mut options = focus_runner::Options::new(&config, known_home.clone());
    options.recap = Some(focus_runner::RecapRunner {
        dir: dir.join("recap-workdir"),
        replies,
    });
    options.history = Some(focus_runner::HistoryFile {
        path: dir.join("runs.jsonl"),
        retention_days: config.history.retention_days,
    });
    options.ui = Some(shared);
    let mut core = Core::new(config, known_home);
    focus_runner::run(
        &mut core,
        &platform,
        &rx,
        &mut options,
        |line, core, now| command(line, &platform, core, now),
    );
    drop(listener);
    Ok(())
}

/// The user and OS commands this runner adds on top of the shared loop.
fn command(line: &str, platform: &FakePlatform, core: &mut Core, now: Instant) -> Control {
    let mut words = line.split_whitespace();
    let mut effects = Vec::new();
    match (words.next(), words.next()) {
        (Some("quit" | "exit"), _) => return Control::Quit,
        (Some("front"), Some(id)) => {
            platform.set_front(Some(app(id)));
            effects
                .extend(core.handle(Input::Observed(Observation::Frontmost(Some(app(id)))), now));
        }
        (Some("idle"), Some(secs)) => match secs.parse::<f64>() {
            Ok(s) => platform.set_idle_seconds(s),
            Err(_) => println!("idle needs a number"),
        },
        (Some(word @ ("click" | "pause" | "resume")), _) => {
            let action = match word {
                "click" => UserAction::ToastAction,
                "pause" => UserAction::TogglePause,
                _ => UserAction::Resume,
            };
            effects.extend(core.handle(Input::User(action), now));
        }
        (Some(other), _) => println!("unknown command {other}"),
        (None, _) => {}
    }
    Control::Continue(effects)
}
