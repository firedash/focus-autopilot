// SPDX-License-Identifier: Apache-2.0
//! Two things a user sees on screen: which project a toast is about, and that a secret typed
//! into a prompt never appears on the recap card.

mod support;

use focus_core::{Config, PauseFor, UserAction};
use support::{Sim, VSCODE, permission, prompt, stop};

#[test]
fn the_toast_and_card_name_the_project_and_the_other_waiting_ones() {
    let mut sim = Sim::new(Config::default());
    sim.hook_in("s1", "p1", "/work/api", prompt("Add pagination"));
    sim.at(2_000);
    sim.focus(VSCODE);
    sim.hook_in("s2", "q1", "/work/web", prompt("Fix the header"));
    sim.at(5_000);
    sim.focus("com.example.Elsewhere");
    sim.hook_in("s2", "q1", "/work/web", stop("Header fixed."));
    sim.at(20_000);
    sim.hook_in("s1", "p1", "/work/api", permission("cargo test"));
    sim.at(30_000);
    // `web` finished first, so its toast came first, and it names the project.
    assert_eq!(sim.toast_texts()[0], "Claude is done in web. Go back?");
    sim.toast_action();
    sim.at(31_000);
    // The key takes the user to the finished project's window.
    assert_eq!(sim.activations().last().map(|a| a.1), Some(VSCODE));
    let card = &sim.recaps()[0];
    assert_eq!(card[0], "Project: web");
    assert_eq!(*card.last().unwrap(), "Also waiting: api");
    // Then the other session, named on its own toast.
    sim.at(40_000);
    assert!(
        sim.toast_texts()
            .contains(&"Claude needs your approval in api. Go back?")
    );
}

#[test]
fn a_session_with_no_folder_is_not_given_an_empty_name() {
    let mut sim = Sim::new(Config::default());
    sim.hook_in("s1", "p1", "", prompt("Hello"));
    sim.at(2_000);
    sim.focus("com.example.Elsewhere");
    sim.hook_in("s1", "p1", "", stop("Hi."));
    sim.at(10_000);
    assert_eq!(sim.toast_texts()[0], "Claude is done. Go back?");
    assert!(
        !sim.recaps()
            .iter()
            .flatten()
            .any(|l| l.starts_with("Project:"))
    );
}

#[test]
fn secrets_never_reach_the_recap_card() {
    let mut sim = Sim::new(Config::default());
    sim.hook(prompt(
        "Deploy with API_TOKEN=hunter2 and key sk-abcdefghijklmnopqrstuvwxyz0123 then ghp_abcdefghijklmnopqrstu",
    ));
    sim.at(2_000);
    sim.focus("com.example.Elsewhere");
    sim.hook(permission(
        "curl -H 'Authorization: Bearer s3cr3tvalue' https://x.test",
    ));
    sim.at(10_000);
    sim.user(UserAction::Pause(PauseFor::Minutes(1)));
    sim.user(UserAction::Resume);
    sim.toast_action();
    sim.at(11_000);
    let card = sim.recaps().concat().join("\n");
    for secret in [
        "hunter2",
        "sk-abcdefghijklmnopqrstuvwxyz0123",
        "ghp_abcdefghijklmnopqrstu",
        "s3cr3tvalue",
    ] {
        assert!(
            !card.contains(secret),
            "{secret} leaked onto the card: {card}"
        );
    }
    assert!(card.contains("[redacted]"));
}
