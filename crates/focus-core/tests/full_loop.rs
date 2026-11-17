// SPDX-License-Identifier: Apache-2.0
//! The loop with the default settings: go anywhere while Claude works, get called back after
//! a countdown, and after the next prompt get sent back to where you left off.

mod support;

use focus_core::{Config, Input, Observation, ToastKind};
use support::{BROWSER, Sim, TERMINAL, TERRARIA, VSCODE, app, config, prompt, stop};

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

/// VS Code is on the home-app list, but with no Claude session in it, it is just the app the
/// user is working in: they are brought back from it, and sent back to it.
#[test]
fn an_editor_with_no_claude_session_in_it_is_like_any_other_app() {
    let mut sim = fresh(Config::default());
    sim.running.push(app(VSCODE));
    sim.at(5_000);
    sim.focus(VSCODE);
    sim.at(60_000);
    sim.hook(stop("Parser refactored."));
    sim.at(70_000);
    assert_eq!(sim.toasts(), vec![(60_000, ToastKind::ReturnCountdown)]);
    assert_eq!(sim.activations(), vec![(65_000, TERMINAL)]);
    sim.at(80_000);
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(300_000);
    assert_eq!(away_switches(&sim), vec![VSCODE]);
}

/// Nothing in front for a moment on the way back, as an OS can report mid-switch.
fn nothing_in_front(sim: &mut Sim) {
    sim.front = None;
    sim.feed(Input::Observed(Observation::Frontmost(None)));
}

#[test]
fn a_moment_with_nothing_in_front_does_not_lose_where_the_user_left_off() {
    let mut sim = fresh(Config::default());
    sim.at(5_000);
    sim.focus(MAIL);
    sim.at(50_000);
    nothing_in_front(&mut sim);
    sim.at(51_000);
    sim.focus(TERMINAL);
    assert_eq!(sim.core.left_off().map(|app| app.id.as_str()), Some(MAIL));

    // A long gap is not "straight from that app".
    let mut sim = fresh(Config::default());
    sim.at(5_000);
    sim.focus(MAIL);
    sim.at(20_000);
    nothing_in_front(&mut sim);
    sim.at(50_000);
    sim.focus(TERMINAL);
    assert_eq!(sim.core.left_off(), None);
}

#[test]
fn the_core_says_what_is_holding_an_away_switch_back() {
    let mut config = Config::default();
    config.switching.away_requires_idle = std::time::Duration::from_secs(8);
    let mut sim = called_back_from_the_browser(config);
    let now = |sim: &Sim| sim.core.away_wait(sim.instant());
    assert_eq!(now(&sim), None, "no run is going");
    sim.at(80_000);
    sim.typing = true;
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(90_000);
    assert_eq!(
        now(&sim).as_deref(),
        Some("the run has to go on for 30 s first")
    );
    sim.at(120_000);
    assert_eq!(
        now(&sim).as_deref(),
        Some("waiting for 8 s without keyboard or mouse input")
    );
    sim.typing = false;
    sim.input();
    sim.at(140_000);
    assert_eq!(now(&sim), None, "switched");
    assert_eq!(away_switches(&sim), vec![BROWSER]);

    // Nowhere to go: no app the user was called back from, and no away app.
    let mut sim = fresh(Config::default());
    sim.at(60_000);
    assert_eq!(
        now(&sim).as_deref(),
        Some("nowhere to go: no app you were called back from, and no away app running")
    );
}

#[test]
fn by_default_a_user_who_is_typing_still_gets_the_away_countdown() {
    // No idle time is required before the countdown. The switch itself still waits for a
    // pause in the typing, so a keystroke does not land in the other app.
    let mut sim = called_back_from_the_browser(Config::default());
    sim.at(80_000);
    sim.typing = true;
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(200_000);
    assert_eq!(sim.toasts()[1].1, ToastKind::AwayCountdown);
    // Five seconds of typing past the countdown and the switch becomes a question.
    assert_eq!(sim.toasts()[2].1, ToastKind::AwayPrompt);
    assert!(away_switches(&sim).is_empty(), "no switch while typing");

    // A pause in the typing just after the countdown lets the switch through.
    let mut sim = called_back_from_the_browser(Config::default());
    sim.at(80_000);
    sim.typing = true;
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(109_000);
    assert_eq!(sim.toasts().len(), 1, "not before the run is old enough");
    sim.at(114_000);
    assert_eq!(sim.toasts()[1], (110_000, ToastKind::AwayCountdown));
    sim.typing = false;
    sim.input();
    sim.at(120_000);
    assert_eq!(sim.activations().last(), Some(&(114_600, BROWSER)));
}

#[test]
fn coming_back_to_look_at_claude_is_not_answered_by_moving_the_user_out_again() {
    let mut sim = called_back_from_the_browser(Config::default());
    sim.at(80_000);
    sim.hook_for("s1", "p2", prompt("Now add tests for it"));
    sim.at(82_000);
    // The user leaves on their own, and a minute into the run comes back to have a look.
    sim.focus(MAIL);
    sim.at(140_000);
    sim.focus(TERMINAL);
    sim.at(149_000);
    assert!(
        away_switches(&sim).is_empty(),
        "nine seconds after coming back, still here"
    );
    // The threshold counts again from when they came back.
    sim.at(200_000);
    assert_eq!(away_switches(&sim), vec![MAIL]);
}
