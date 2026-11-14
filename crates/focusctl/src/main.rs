// SPDX-License-Identifier: Apache-2.0
//! `focusctl`: install and remove focusd's Claude Code hooks, and rotate the token.
#![forbid(unsafe_code)]

mod config;
mod files;
mod hooks;

use std::path::PathBuf;

use files::Op;

const USAGE: &str = "usage: focusctl <install-hooks|uninstall-hooks|rotate-token> \
[--dry-run] [--settings FILE] [--config FILE]";

fn main() {
    if let Err(message) = run() {
        eprintln!("focusctl: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or(USAGE)?;
    let mut dry_run = false;
    let mut settings = None;
    let mut config_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--settings" => settings = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--config" => config_path = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            _ => return Err(USAGE.into()),
        }
    }
    let settings = match settings {
        Some(path) => path,
        None => {
            PathBuf::from(std::env::var_os("HOME").ok_or("no HOME")?).join(".claude/settings.json")
        }
    };
    let config_path = config_path
        .or_else(focus_runner::paths::config_path)
        .ok_or("no HOME")?;
    match command.as_str() {
        "install-hooks" | "rotate-token" => {
            let rotate = command == "rotate-token";
            if dry_run {
                // A dry run must not touch config.toml either: read what is there, or show a
                // placeholder if no token exists yet.
                let r = peek(&config_path);
                show(
                    &files::apply(
                        &settings,
                        &Op::Install {
                            port: r.0,
                            token: r.1,
                        },
                        true,
                    )?,
                    true,
                );
            } else {
                let r = config::ensure_token(&config_path, rotate)?;
                let outcome = files::apply(
                    &settings,
                    &Op::Install {
                        port: r.port,
                        token: r.token,
                    },
                    false,
                )?;
                show(&outcome, false);
                println!("config: {}", config_path.display());
                println!("Restart running Claude Code sessions so they pick up the hooks.");
            }
        }
        "uninstall-hooks" => {
            let port = config::ensure_port(&config_path);
            show(
                &files::apply(&settings, &Op::Uninstall { port }, dry_run)?,
                dry_run,
            );
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn peek(config_path: &std::path::Path) -> (u16, String) {
    let token =
        config::token_of(config_path).unwrap_or_else(|| "<token written on install>".to_owned());
    (config::ensure_port(config_path), token)
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
