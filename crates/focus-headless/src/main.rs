// SPDX-License-Identifier: Apache-2.0
//! Dev-only runner (design section 5). Receives real hook POSTs on 127.0.0.1, feeds the core
//! with a real clock, and carries effects out against a fake platform, printing each one.
//!
//! Type commands on stdin to play the user and the OS:
//!   front <id>   bring an app to the front        idle <seconds>   set idle time
//!   click        click the toast / focus key      pause | resume   toggle pause
//!   quit
#![forbid(unsafe_code)]

use std::sync::mpsc;
use std::time::Instant;

use focus_core::{AppRef, Config, Core, Input, Observation, UserAction};
use focus_platform::fake::FakePlatform;
use focus_platform::known_apps;
use focus_receiver::{Received, Settings, config_file, random_hex};
use focus_runner::Control;

fn main() {
    if let Err(message) = run() {
        eprintln!("focus-headless: {message}");
        std::process::exit(1);
    }
}

fn app(id: &str) -> AppRef {
    AppRef {
        id: id.to_owned(),
        name: id.to_owned(),
        pid: 100,
        window: None,
    }
}

struct Args {
    config: Config,
    port: Option<u16>,
    home: Option<String>,
    away: Option<String>,
}

/// `Ok(None)` after printing help.
fn parse_args() -> Result<Option<Args>, String> {
    let mut args = std::env::args().skip(1);
    let mut config = Config::default();
    let mut port = None;
    let mut home = None;
    let mut away = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--config" => {
                let path = value("--config")?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                config = config_file::parse(&text).map_err(|e| e.to_string())?;
            }
            "--port" => port = Some(value("--port")?.parse::<u16>().map_err(|e| e.to_string())?),
            "--home-app" => home = Some(value("--home-app")?),
            "--away-app" => away = Some(value("--away-app")?),
            "--help" | "-h" => {
                println!(
                    "usage: focus-headless [--config FILE] [--port N] [--home-app ID] [--away-app ID]"
                );
                return Ok(None);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Some(Args {
        config,
        port,
        home,
        away,
    }))
}

fn run() -> Result<(), String> {
    let Some(Args {
        mut config,
        port,
        home,
        away,
    }) = parse_args()?
    else {
        return Ok(());
    };
    let home = home.unwrap_or_else(|| "home-terminal".to_owned());
    let away = away.unwrap_or_else(|| "away-notes".to_owned());
    if config.away.apps.is_empty() {
        config.away.apps = vec![away.clone()];
    }
    config.home.extra_apps.push(home.clone());
    let token = if config.receiver.token.is_empty() {
        random_hex().map_err(|e| e.to_string())?
    } else {
        config.receiver.token.clone()
    };
    config.receiver.token.clone_from(&token);
    let port = port.unwrap_or(config.receiver.port);

    let (hook_tx, hook_rx) = mpsc::channel::<Received>();
    let salt = random_hex().map_err(|e| e.to_string())?;
    let listener = focus_receiver::spawn(
        Settings {
            port,
            token: token.clone(),
            salt,
        },
        hook_tx,
    )
    .map_err(|e| format!("can't listen on 127.0.0.1:{port}: {e}"))?;
    let (rx, replies) = focus_runner::inputs(hook_rx);

    let platform = FakePlatform::new(vec![app(&home), app(&away)], Some(app(&home)));
    platform.set_idle_seconds(60.0);
    let mut apps = known_apps::for_this_os();
    apps.push(home.clone());
    config
        .validate(|id| apps.iter().any(|a| a.eq_ignore_ascii_case(id)))
        .map_err(|e| format!("invalid config: {e:?}"))?;
    let is_home = {
        let apps = apps.clone();
        move |id: &str| apps.iter().any(|a| focus_core::app_id_matches(a, id))
    };
    let mut core = Core::new(config, apps);

    println!("listening on http://{}/hook", listener.addr);
    println!("token (header X-Focus-Token): {token}");
    println!("home app: {home}, away app: {away}; you are in {home}. Type `quit` to stop.");

    focus_runner::run(
        &mut core,
        &platform,
        &rx,
        &focus_runner::Options {
            recap_for: std::time::Duration::from_secs(10),
            focus_hint: None,
            recap: Some(focus_runner::RecapRunner {
                dir: std::env::temp_dir().join("focus-headless-recap"),
                replies,
            }),
            is_home: Box::new(is_home),
        },
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
