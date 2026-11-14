// SPDX-License-Identifier: Apache-2.0
//! With no away app configured the user goes wherever they like. focusd never switches them
//! away; when Claude is done or needs them, they get a prompt and one key takes them back.

mod support;

use focus_core::{Config, ToastKind};
use support::{BROWSER, Sim, TERMINAL, permission, prompt, stop};

fn manual() -> Sim {
    let mut sim = Sim::new(Config::default());
    sim.hook(prompt("Refactor the parser"));
    sim
}

#[test]
fn nothing_switches_the_user_away_and_done_gives_a_prompt() {
    let mut sim = manual();
    sim.at(5_000);
    sim.focus(BROWSER);
    sim.at(120_000);
    // Two minutes in, still no automatic switch and no toast of any kind.
    assert!(sim.toasts().is_empty());
    assert!(sim.activations().is_empty());
    sim.hook(stop("Parser refactored."));
    sim.at(121_000);
    assert_eq!(sim.toasts(), vec![(120_000, ToastKind::ReturnPrompt)]);
    assert_eq!(sim.sounds(), 1);
    // The prompt stays until acted on: no countdown, no automatic switch back.
    sim.at(300_000);
    assert!(sim.activations().is_empty());
    assert_eq!(sim.toasts().len(), 1);
}

#[test]
fn the_key_takes_the_user_back_and_shows_the_recap() {
    let mut sim = manual();
    sim.at(5_000);
    sim.focus(BROWSER);
    sim.at(60_000);
    sim.hook(stop("Done."));
    sim.at(61_000);
    sim.toast_action();
    sim.at(62_000);
    assert_eq!(sim.activations(), vec![(61_000, TERMINAL)]);
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn a_permission_prompt_also_waits_for_the_key() {
    let mut sim = manual();
    sim.at(5_000);
    sim.focus(BROWSER);
    sim.at(30_000);
    sim.hook(permission("rm -rf build"));
    sim.at(40_000);
    assert_eq!(sim.toasts(), vec![(32_000, ToastKind::ReturnPrompt)]);
    assert!(sim.activations().is_empty());
    sim.toast_action();
    sim.at(41_000);
    assert_eq!(sim.activations(), vec![(40_000, TERMINAL)]);
}

#[test]
fn staying_in_the_home_window_shows_nothing() {
    let mut sim = manual();
    sim.at(90_000);
    sim.hook(stop("Done."));
    sim.at(100_000);
    assert!(sim.toasts().is_empty());
    assert!(sim.activations().is_empty());
}

#[test]
fn going_back_on_your_own_dismisses_the_prompt() {
    let mut sim = manual();
    sim.at(5_000);
    sim.focus(BROWSER);
    sim.at(60_000);
    sim.hook(stop("Done."));
    sim.at(62_000);
    sim.focus(TERMINAL);
    assert_eq!(sim.hides(), 1);
    assert!(sim.activations().is_empty());
}
