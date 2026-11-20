// SPDX-License-Identifier: Apache-2.0
//! `focusctl`: install and remove focusd's Claude Code hooks, rotate the token, open the
//! statistics and configuration page, set up launch at login, and diagnose a setup.
#![forbid(unsafe_code)]

mod agent;
mod antigravity;
mod config;
mod doctor;
mod files;
mod hooks;
mod setup;
mod ui;

use std::path::{Path, PathBuf};
use std::time::Duration;

use files::Op;

const USAGE: &str = "usage: focusctl <command> [options]

commands:
  setup             first run in one step: install the hooks, start focusd in the background,
                    check everything, and say what to do next (safe to run again)
  install-hooks     add Focus Autopilot's hooks to Claude Code's settings (and create the token)
  uninstall-hooks   remove them again, leaving everything else as it was
  install-antigravity-hooks   add experimental Antigravity command hooks
  uninstall-antigravity-hooks remove only those hooks
  antigravity-hook   internal passive hook bridge, --event EVENT
  rotate-token      replace the token in config.toml and in the hooks
  ui                open the statistics and configuration page in your browser
  doctor            check the setup and say how to fix what is wrong
  install-agent     start focusd at login
  uninstall-agent   stop starting focusd at login

options:
  --dry-run         install-hooks, uninstall-hooks: show the change, write nothing
  --scope SCOPE     which Claude Code settings file: user (default, ~/.claude/settings.json),
                    project (./.claude/settings.json) or local (./.claude/settings.local.json)
  --settings FILE   a specific Claude Code settings file, instead of --scope
  --config FILE     Focus Autopilot's config.toml, instead of the one in its data folder
  --agents-dir DIR  where launch agents live, instead of ~/Library/LaunchAgents
  --focusd FILE     setup: the focusd to start, instead of the one beside focusctl or on the PATH
  --version         print the version and exit
  --no-open         ui: write the page's launcher file and print where it is, open nothing";

/// `focusctl --version`: the program and its Cargo package version, the same one the release
/// archives are named for.
fn version_line() -> String {
    format!("focusctl {}", env!("CARGO_PKG_VERSION"))
}

fn main() {
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V")) {
        println!("{}", version_line());
        return;
    }
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(message) => {
            eprintln!("focusctl: {message}");
            std::process::exit(1);
        }
    }
}

struct Args {
    command: String,
    event: Option<String>,
    dry_run: bool,
    no_open: bool,
    scope: Option<String>,
    settings: Option<PathBuf>,
    config: Option<PathBuf>,
    agents_dir: Option<PathBuf>,
    focusd: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        command: args.next().ok_or(USAGE)?,
        event: None,
        dry_run: false,
        no_open: false,
        scope: None,
        settings: None,
        config: None,
        agents_dir: None,
        focusd: None,
    };
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))
        };
        match arg.as_str() {
            "--dry-run" => parsed.dry_run = true,
            "--no-open" => parsed.no_open = true,
            "--event" => parsed.event = Some(value()?),
            "--scope" => parsed.scope = Some(value()?),
            "--settings" => parsed.settings = Some(PathBuf::from(value()?)),
            "--config" => parsed.config = Some(PathBuf::from(value()?)),
            "--agents-dir" => parsed.agents_dir = Some(PathBuf::from(value()?)),
            "--focusd" => parsed.focusd = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    Ok(parsed)
}

fn home() -> Result<PathBuf, String> {
    focus_runner::paths::home_dir()
        .ok_or_else(|| "the HOME environment variable is not set".to_owned())
}

/// The Claude Code settings file for a scope. `project` and `local` are relative to the
/// folder `focusctl` is run in, which is how Claude Code finds them too.
fn settings_for(scope: Option<&str>, home: &Path) -> Result<PathBuf, String> {
    match scope.unwrap_or("user") {
        "user" => Ok(home.join(".claude/settings.json")),
        "project" => Ok(PathBuf::from(".claude/settings.json")),
        "local" => Ok(PathBuf::from(".claude/settings.local.json")),
        other => Err(format!(
            "--scope must be user, project or local, not `{other}`"
        )),
    }
}

/// `Ok(false)` means the command ran and found problems (`doctor`).
fn run() -> Result<bool, String> {
    let args = parse_args()?;
    if matches!(args.command.as_str(), "--help" | "-h" | "help") {
        println!("{USAGE}");
        return Ok(true);
    }
    if args.command == "antigravity-hook" {
        // Always answer passively, even if arguments/config/input are unusable.
        let path = args
            .config
            .clone()
            .or_else(focus_runner::paths::config_path)
            .unwrap_or_default();
        antigravity::run(args.event.as_deref().unwrap_or(""), &path);
        return Ok(true);
    }
    let is_antigravity = args.command.ends_with("antigravity-hooks");
    let settings = match &args.settings {
        Some(path) => path.clone(),
        None if is_antigravity => match args.scope.as_deref().unwrap_or("user") {
            "user" => home()?.join(".gemini/config/hooks.json"),
            "project" | "local" => PathBuf::from(".agents/hooks.json"),
            _ => return Err("Antigravity scope must be user, project or local".into()),
        },
        None => settings_for(args.scope.as_deref(), &home()?)?,
    };
    let config_path = match &args.config {
        Some(path) => path.clone(),
        None => {
            focus_runner::paths::config_path().ok_or("the HOME environment variable is not set")?
        }
    };
    let agents_dir = || match &args.agents_dir {
        Some(dir) => Ok(dir.clone()),
        None => home().map(|home| home.join("Library/LaunchAgents")),
    };
    match args.command.as_str() {
        "install-antigravity-hooks" => {
            if !args.dry_run {
                config::ensure_token(&config_path, false)?;
            }
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let absolute_config = if config_path.is_absolute() {
                config_path.clone()
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(&config_path)
            };
            show(
                &files::apply(
                    &settings,
                    &Op::InstallAntigravity {
                        exe,
                        config: absolute_config,
                    },
                    args.dry_run,
                )?,
                args.dry_run,
            );
            println!(
                "Experimental Antigravity completion/input return. See docs/antigravity.md for limits."
            );
            println!("Restart Antigravity and inspect its Hooks panel. See docs/antigravity.md.");
        }
        "uninstall-antigravity-hooks" => {
            show(
                &files::apply(&settings, &Op::UninstallAntigravity, args.dry_run)?,
                args.dry_run,
            );
        }
        "install-hooks" | "rotate-token" => {
            install_hooks(&args, &settings, &config_path)?;
        }
        "uninstall-hooks" => {
            show(
                &files::apply(&settings, &Op::Uninstall, args.dry_run)?,
                args.dry_run,
            );
        }
        "setup" => return run_setup(&args, &settings, &config_path),
        "ui" => open_ui(&config_path, args.no_open)?,
        "doctor" => return Ok(run_doctor(&settings, &config_path, &agents_dir()?)),
        "install-agent" | "uninstall-agent" if !cfg!(target_os = "macos") => {
            return Err(
                "starting at login is only built for macOS so far; on this system start \
                 `focusd` yourself"
                    .to_owned(),
            );
        }
        "install-agent" => install_agent(&agents_dir()?)?,
        "uninstall-agent" => uninstall_agent(&agents_dir()?)?,
        other => return Err(format!("unknown command {other}\n\n{USAGE}")),
    }
    Ok(true)
}

fn install_hooks(args: &Args, settings: &Path, config_path: &Path) -> Result<(), String> {
    if args.dry_run {
        // A dry run must not touch config.toml either: read what is there, or show a
        // placeholder if no token exists yet.
        let token = config::token_of(config_path)
            .unwrap_or_else(|| "<token written on install>".to_owned());
        let port = config::ensure_port(config_path);
        show(
            &files::apply(settings, &Op::Install { port, token }, true)?,
            true,
        );
        return Ok(());
    }
    let receiver = config::ensure_token(config_path, args.command == "rotate-token")?;
    let outcome = files::apply(
        settings,
        &Op::Install {
            port: receiver.port,
            token: receiver.token,
        },
        false,
    )?;
    show(&outcome, false);
    println!("config: {}", config_path.display());
    println!("Restart running Claude Code sessions so they pick up the hooks.");
    if args.command == "rotate-token" {
        println!("Restart focusd too: it keeps the token it started with.");
    }
    Ok(())
}

/// `focusctl setup`: hooks, focusd, checks, one result. `Ok(false)` when it is not ready.
fn run_setup(args: &Args, settings: &Path, config_path: &Path) -> Result<bool, String> {
    if args.dry_run {
        return Err(
            "setup has no --dry-run; `focusctl install-hooks --dry-run` previews the hooks change"
                .to_owned(),
        );
    }
    // focusd must read the same config.toml as this command, and its log goes beside it.
    let log = config_path.with_file_name("focusd.log");
    let config_arg = config_path.to_string_lossy().into_owned();
    let mut start = || {
        let focusd = if let Some(path) = &args.focusd {
            path.clone()
        } else {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf));
            let path_env = std::env::var("PATH").unwrap_or_default();
            setup::find_focusd(exe_dir.as_deref(), &path_env).ok_or(
                "focusd was not found next to focusctl or on the PATH. From a download, keep \
                 focusd in the same folder as focusctl; from source, build it with `cargo \
                 build --release -p focusd`",
            )?
        };
        setup::spawn_detached(&focusd, &["--config", &config_arg], &log)?;
        Ok(format!("{}, log {}", focusd.display(), log.display()))
    };
    println!("Focus Autopilot setup");
    let result = setup::run(
        settings,
        config_path,
        &mut start,
        Some(&log),
        setup::START_WAIT,
        &mut |line| println!("{line}"),
    )?;
    println!("\n{}", result.summary);
    Ok(result.ready)
}

fn open_ui(config_path: &Path, no_open: bool) -> Result<(), String> {
    let token = config::token_of(config_path).unwrap_or_default();
    let address = ui::address(config::ensure_port(config_path), &token)?;
    let data_dir =
        focus_runner::paths::data_dir().ok_or("the HOME environment variable is not set")?;
    let page = ui::write_page(&data_dir, &ui::redirect_page(&address))?;
    if no_open {
        println!("Open this file in your browser: {}", page.display());
        return Ok(());
    }
    match ui::open_in_browser(&page) {
        Ok(()) => println!("Opened the Focus Autopilot page in your browser."),
        Err(problem) => println!(
            "{problem}. Open this file in your browser instead: {}",
            page.display()
        ),
    }
    println!("If the page says it can't reach Focus Autopilot, start `focusd` first.");
    Ok(())
}

/// Runs every check that can be run from here. Returns true when nothing failed.
fn run_doctor(settings: &Path, config_path: &Path, agents_dir: &Path) -> bool {
    let known_home = focus_platform::known_apps::for_this_os();
    let mut checks = Vec::new();
    let (config_check, parsed) = doctor::config(config_path, &known_home);
    checks.push(config_check);
    // Without a config there is still a port to probe and hooks to look for.
    let port = parsed
        .as_ref()
        .map_or_else(|| config::ensure_port(config_path), |c| c.receiver.port);
    let token = parsed
        .as_ref()
        .map(|c| c.receiver.token.clone())
        .unwrap_or_default();
    checks.push(doctor::hooks(settings, port, &token));
    let (daemon_check, stats) = doctor::daemon(port, &token);
    checks.push(daemon_check);
    if let Some(stats) = &stats {
        checks.push(doctor::events(stats));
    }
    let claude = doctor::find_claude(&std::env::var("PATH").unwrap_or_default());
    let version = claude.as_ref().and_then(|_| claude_version());
    checks.push(doctor::claude_cli(
        claude.as_deref(),
        version.as_deref(),
        parsed.as_ref(),
    ));
    if cfg!(target_os = "macos") {
        checks.push(doctor::agent(agents_dir));
    }
    doctor::report(&checks)
}

/// What `claude --version` prints, if it answers within a few seconds.
fn claude_version() -> Option<String> {
    let dir = std::env::temp_dir();
    let out = focus_runner::recap_cli::run_cli(
        "claude",
        &["--version"],
        &dir,
        "",
        Duration::from_secs(5),
    )
    .ok()?;
    let line = out.lines().next()?.trim();
    (!line.is_empty() && line.len() <= 80).then(|| line.to_owned())
}

fn install_agent(agents_dir: &Path) -> Result<(), String> {
    let log = focus_runner::paths::data_dir()
        .ok_or("the HOME environment variable is not set")?
        .join("focusd.log");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path_env = std::env::var("PATH").unwrap_or_default();
    let plist = agent::install(agents_dir, &exe, &path_env, &log)?;
    println!("wrote {}", plist.display());
    println!("focusd will start at your next login. To start it now:");
    println!("  launchctl bootstrap gui/$(id -u) {}", plist.display());
    println!(
        "Its log, with event names only and no prompt text, is {}",
        log.display()
    );
    println!("`focusd` keeps the PATH you have now, so it can find `claude` for LLM recaps.");
    Ok(())
}

fn uninstall_agent(agents_dir: &Path) -> Result<(), String> {
    let path = agent::plist_path(agents_dir);
    if agent::uninstall(agents_dir)? {
        println!("removed {}", path.display());
        println!("If it is running, stop it with:");
        println!("  launchctl bootout gui/$(id -u)/{}", agent::LABEL);
    } else {
        println!("{} was not there; nothing to do", path.display());
    }
    Ok(())
}

fn show(outcome: &files::Outcome, dry_run: bool) {
    if let Some(link) = &outcome.via_symlink {
        println!(
            "{} is a symlink; editing its target {}",
            link.display(),
            outcome.target.display()
        );
    }
    if !outcome.changed() {
        println!("{}: no changes needed", outcome.target.display());
    } else if dry_run {
        println!(
            "--- {} (dry run, nothing written)\n{}",
            outcome.target.display(),
            files::diff(&outcome.old, &outcome.new)
        );
    } else {
        println!("updated {}", outcome.target.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_names_the_program_and_the_package_version() {
        assert_eq!(
            version_line(),
            concat!("focusctl ", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn each_scope_names_the_file_claude_code_reads() {
        let home = Path::new("/Users/me");
        assert_eq!(
            settings_for(None, home).unwrap(),
            Path::new("/Users/me/.claude/settings.json")
        );
        assert_eq!(
            settings_for(Some("user"), home).unwrap(),
            Path::new("/Users/me/.claude/settings.json")
        );
        assert_eq!(
            settings_for(Some("project"), home).unwrap(),
            Path::new(".claude/settings.json")
        );
        assert_eq!(
            settings_for(Some("local"), home).unwrap(),
            Path::new(".claude/settings.local.json")
        );
        assert!(
            settings_for(Some("global"), home)
                .unwrap_err()
                .contains("user, project or local")
        );
    }
      }
