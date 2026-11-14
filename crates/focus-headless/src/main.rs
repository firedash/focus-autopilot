// SPDX-License-Identifier: Apache-2.0
//! Dev-only runner (design section 5). Receives real hook POSTs on 127.0.0.1, feeds the core
//! with a real clock, and carries effects out against a fake platform, printing each one.
//!
//! Type commands on stdin to play the user and the OS:
//!   front <id>   bring an app to the front        idle <seconds>   set idle time
//!   click        click the toast / focus key      pause | resume   toggle pause
//!   quit
#![forbid(unsafe_code)]

use std::io::BufRead as _;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use focus_core::{AppRef, Config, Core, Effect, Input, Observation, ProbeKind, UserAction};
use focus_platform::Platform as _;
use focus_platform::fake::FakePlatform;
use focus_platform::known_apps;
use focus_receiver::{Received, Settings, config_file, random_hex};

enum Msg {
    Hook(Received),
    Line(String),
    StdinClosed,
}

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

    let (tx, rx) = mpsc::channel::<Msg>();
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
    let forward = tx.clone();
    std::thread::spawn(move || {
        while let Ok(received) = hook_rx.recv() {
            if forward.send(Msg::Hook(received)).is_err() {
                break;
            }
        }
    });
    let stdin_tx = tx;
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if stdin_tx.send(Msg::Line(line)).is_err() {
                return;
            }
        }
        let _ = stdin_tx.send(Msg::StdinClosed);
    });

    let platform = FakePlatform::new(vec![app(&home), app(&away)], Some(app(&home)));
    platform.set_idle_seconds(60.0);
    let mut apps = known_apps::for_this_os();
    apps.push(home.clone());
    config
        .validate(|id| apps.iter().any(|a| a.eq_ignore_ascii_case(id)))
        .map_err(|e| format!("invalid config: {e:?}"))?;
    let mut core = Core::new(config, apps);

    println!("listening on http://{}/hook", listener.addr);
    println!("token (header X-Focus-Token): {token}");
    println!("home app: {home}, away app: {away}; you are in {home}. Type `quit` to stop.");

    // Like a real shell: tell the core what is in front before any hook arrives, so the next
    // prompt captures the right home window.
    let front = platform.frontmost().ok().flatten();
    let startup = core.handle(
        Input::Observed(Observation::Frontmost(front)),
        Instant::now(),
    );
    drop(startup);
    event_loop(&rx, &platform, &mut core);
    drop(listener);
    Ok(())
}

fn event_loop(rx: &mpsc::Receiver<Msg>, platform: &FakePlatform, core: &mut Core) {
    let mut pending_verify: Option<Instant> = None;
    loop {
        let now = Instant::now();
        let mut wake = core.next_deadline();
        if let Some(at) = pending_verify {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        let msg = match wake {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(now)),
            None => rx.recv_timeout(Duration::from_secs(3600)),
        };
        let now = Instant::now();
        let mut effects = Vec::new();
        match msg {
            Ok(Msg::Hook(hook)) => {
                println!("hook: {:?}", hook.event.kind);
                effects.extend(core.handle(Input::Hook(hook.event), hook.at));
            }
            Ok(Msg::Line(line)) => {
                if !command(&line, platform, core, &mut effects, now) {
                    return;
                }
            }
            Ok(Msg::StdinClosed) | Err(RecvTimeoutError::Disconnected) => {}
            Err(RecvTimeoutError::Timeout) => effects.extend(core.tick(now)),
        }
        if pending_verify.is_some_and(|at| now >= at) {
            pending_verify = None;
            let verified = platform.frontmost().ok().flatten();
            effects.extend(core.handle(Input::Observed(Observation::Frontmost(verified)), now));
            effects.extend(core.handle(
                Input::Observed(Observation::Activation { verified: true }),
                now,
            ));
        }
        while !effects.is_empty() {
            let batch = std::mem::take(&mut effects);
            for effect in batch {
                carry_out(
                    effect,
                    platform,
                    core,
                    &mut effects,
                    &mut pending_verify,
                    now,
                );
            }
        }
    }
}

/// Returns false to quit.
fn command(
    line: &str,
    platform: &FakePlatform,
    core: &mut Core,
    effects: &mut Vec<Effect>,
    now: Instant,
) -> bool {
    let mut words = line.split_whitespace();
    match (words.next(), words.next()) {
        (Some("quit" | "exit"), _) => return false,
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
    true
}

fn carry_out(
    effect: Effect,
    platform: &FakePlatform,
    core: &mut Core,
    out: &mut Vec<Effect>,
    pending_verify: &mut Option<Instant>,
    now: Instant,
) {
    match effect {
        Effect::ShowToast(toast) => println!("TOAST [{:?}] {}", toast.kind, toast.text),
        Effect::HideToast => println!("toast hidden"),
        Effect::Activate {
            target,
            verify_after,
        } => {
            match platform.activate(&target) {
                Ok(()) => println!("ACTIVATE {}", target.id),
                Err(e) => println!("ACTIVATE {} failed: {e:?}", target.id),
            }
            *pending_verify = Some(now + verify_after);
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
            for line in card.lines {
                println!("  {line}");
            }
        }
        Effect::PlaySound(sound) => {
            platform.play_sound(sound);
            println!("sound: {sound:?}");
        }
        Effect::AppendRun(record) => println!("run recorded: {record:?}"),
        Effect::SummarizeRecap { .. } => println!("(LLM recap requested; not supported headless)"),
        Effect::Log(log) => println!("log: {}", log.name),
    }
}
