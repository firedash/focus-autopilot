// SPDX-License-Identifier: Apache-2.0
//! `focusctl`: install and remove focusd's Claude Code hooks, rotate the token, open the
//! statistics and configuration page, set up launch at login, and diagnose a setup.
#![forbid(unsafe_code)]

mod agent;
mod config;
mod doctor;
mod files;
mod hooks;
mod ui;

use std::path::{Path, PathBuf};
use std::time::Duration;

use files::Op;

const USAGE: &str = "usage: focusctl <command> [options]

commands:
  install-hooks     add Focus Autopilot's hooks to Claude Code's settings (and create the token)
  uninstall-hooks   remove them again, leaving everything else as it was
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
  --no-open         ui: write the page's launcher file and print where it is, open nothing";

fn main() {
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
    dry_run: bool,
    no_open: bool,
    scope: Option<String>,
    settings: Option<PathBuf>,
    config: Option<PathBuf>,
    agents_dir: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        command: args.next().ok_or(USAGE)?,
        dry_run: false,
        no_open: false,
        scope: None,
        settings: None,
        config: None,
        agents_dir: None,
    };
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))
        };
        match arg.as_str() {
            "--dry-run" => parsed.dry_run = true,
            "--no-open" => parsed.no_open = true,
            "--scope" => parsed.scope = Some(value()?),
            "--settings" => parsed.settings = Some(PathBuf::from(value()?)),
            "--config" => parsed.config = Some(PathBuf::from(value()?)),
            "--agents-dir" => parsed.agents_dir = Some(PathBuf::from(value()?)),
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
    let settings = match &args.settings {
        Some(path) => path.clone(),
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
        "install-hooks" | "rotate-token" => {
            install_hooks(&args, &settings, &config_path)?;
        }
        "uninstall-hooks" => {
            let port = config::ensure_port(&config_path);
            show(
                &files::apply(&settings, &Op::Uninstall { port }, args.dry_run)?,
                args.dry_run,
            );
        }
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
