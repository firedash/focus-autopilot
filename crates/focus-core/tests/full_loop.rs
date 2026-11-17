// SPDX-License-Identifier: Apache-2.0
//! The loop with the default settings: go anywhere while Claude works, get called back after
//! a countdown, and after the next prompt get sent back to where you left off.

mod support;

use focus_core::{Config, ToastKind};
use support::{BROWSER, Sim, TERMINAL, TERRARIA, app, config, prompt, stop};

const MAIL: &str = "com.example.Mail";

/// One prompt from Terminal with the default settings, and the browser running too.
fn fresh(config: Config) -> Sim {
    let mut sim = Sim::new(config);
    sim.running.push(app(BROWSER));
    sim.running.push(app(MAIL));
    sim.hook(prompt("Refactor the parser"));
    sim
}

/// The user goes to the browser, Claude finishes at 60 s, and focusd brings them back.
fn called_back_from_the_browser(config: Config) -> Sim {
    let mut sim = fresh(config);
    sim.at(5_000);
    sim.focus(BROWSER);
    sim.at(60_000);
    sim.hook(stop("Parser refactored."));
    sim.at(70_000);
    sim
}

fn away_switches(sim: &Sim) -> Vec<&str> {
    sim.activations()
        .into_iter()
        .map(|(_, id)| id)
        .filter(|id| *id != TERMINAL)
        .collect()
}

#[test]
fn by_default_a_finished_run_counts_down_and_brings_the_user_back() {
    let sim = called_back_from_the_browser(Config::default());
    assert_eq!(sim.toasts(), vec![(60_000, ToastKind::ReturnCountdown)]);
    assert_eq!(
        sim.toast_texts(),
        vec!["Claude is done in app. Back to Terminal in 5 s."]
    );
    assert_eq!(sim.sounds(), 1);
    assert_eq!(sim.activations(), vec![(65_000, TERMINAL)]);
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn the_key_snoozes_a_return_from_any_app() {
    let mut sim = fresh(Config::default());
    sim.at(5_000);
    sim.focus(BROWSER);
    sim.at(60_000);
    sim.hook(stop("Done."));
    sim.at(62_000);
    sim.toast_action();
    sim.at(100_000);
    assert!(sim.activations().is_empty(), "snoozed for a minute");
    sim.at(130_000);
    assert_eq!(
        sim.toasts(),
        vec![
            (60_000, ToastKind::ReturnCountdown),
            (122_000, ToastKind::ReturnCountdown)
        ]
    );
    assert_eq!(sim.activations(), vec![(127_000, TERMINAL)]);
}

#[test]
fn the_first_prompt_switches_nowhere_without_an_away_app() {
    let mut sim = fresh(Config::default());
    sim.at(300_000);
    assert!(sim.toasts().is_empty());
    assert!(sim.activations().is_empty());
}

#[test]
fn the_next_prompt_sends_the_user_back_to_where_they_left_off() {
    let mut sim = called_back_from_the_browser(Config::default());
    sim.at(80_000);
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(200_000);
    let (at, kind) = sim.toasts()[1];
    assert_eq!(kind, ToastKind::AwayCountdown);
    assert_eq!(
        sim.toast_texts()[1],
        format!("Claude will take a while. Switching to {BROWSER} in 3 s.")
    );
    assert_eq!(sim.activations()[1], (at + 3_000, BROWSER));
    // And the loop closes: called back from there again when Claude is done.
    sim.hook_for("s1", "p2", stop("Tests added."));
    sim.at(210_000);
    assert_eq!(sim.activations()[2], (205_000, TERMINAL));
}

#[test]
fn where_the_user_left_off_comes_before_the_away_app() {
    // The first turn goes to the away app; the user moves on to the browser from there.
    let mut sim = Sim::new(config());
    sim.running.push(app(BROWSER));
    sim.hook(prompt("Run the tests and fix failures"));
    sim.at(40_000);
    assert_eq!(away_switches(&sim), vec![TERRARIA]);
    sim.focus(BROWSER);
    sim.at(100_000);
    sim.hook(stop("All green."));
    sim.at(110_000);
    sim.hook_for("s1", "p2", prompt("Now fix the lints"));
    sim.at(300_000);
    assert_eq!(away_switches(&sim), vec![TERRARIA, BROWSER]);
}

#[test]
fn resume_can_be_turned_off_and_then_the_away_app_is_used_every_time() {
    let mut config = config();
    config.away.resume_last_app = false;
    let mut sim = Sim::new(config);
    sim.running.push(app(BROWSER));
    sim.hook(prompt("Run the tests and fix failures"));
    sim.at(40_000);
    sim.focus(BROWSER);
    sim.at(100_000);
    sim.hook(stop("All green."));
    sim.at(110_000);
    sim.hook_for("s1", "p2", prompt("Now fix the lints"));
    sim.at(300_000);
    assert_eq!(away_switches(&sim), vec![TERRARIA, TERRARIA]);

    // With no away app either, nothing switches the user away.
    let mut config = Config::default();
    config.away.resume_last_app = false;
    let mut sim = called_back_from_the_browser(config);
    sim.at(80_000);
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(300_000);
    assert!(away_switches(&sim).is_empty());
}

#[test]
fn an_app_that_has_quit_is_not_switched_to() {
    let mut sim = called_back_from_the_browser(config());
    sim.running.retain(|running| running.id != BROWSER);
    sim.at(80_000);
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(300_000);
    // The browser being gone, the configured away app is used.
    assert_eq!(away_switches(&sim), vec![TERRARIA]);
}

#[test]
fn arriving_at_the_terminal_with_nothing_going_on_changes_nothing() {
    let mut sim = called_back_from_the_browser(Config::default());
    // Claude is done and seen. The user reads mail, comes back, and only then prompts.
    sim.at(80_000);
    sim.focus(MAIL);
    sim.at(90_000);
    sim.focus(TERMINAL);
    sim.at(95_000);
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(300_000);
    assert_eq!(away_switches(&sim), vec![BROWSER]);
}

#[test]
fn coming_back_on_your_own_while_claude_works_is_remembered_too() {
    let mut sim = fresh(Config::default());
    sim.at(5_000);
    sim.focus(MAIL);
    sim.at(50_000);
    sim.focus(TERMINAL);
    sim.at(60_000);
    sim.hook(stop("Done."));
    sim.at(70_000);
    sim.hook_for("s1", "p2", prompt("Next"));
    sim.at(300_000);
    assert_eq!(away_switches(&sim), vec![MAIL]);
}
