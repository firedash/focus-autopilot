// SPDX-License-Identifier: Apache-2.0
//! Two things a user sees on screen: which project a toast is about, and that a secret typed
//! into a prompt never appears on the recap card.

mod support;

use focus_core::{Config, Input, PauseFor, RecapMode, UserAction};
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
    // The other session's outcome is on the card, not just its name: it is settled by this
    // return, so this is the only place its result is ever shown.
    assert_eq!(
        *card.last().unwrap(),
        "Also:    api - needs approval to run cargo test"
    );
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

#[test]
fn two_projects_with_the_same_folder_name_are_told_apart_by_their_parent() {
    let mut sim = Sim::new(Config::default());
    sim.hook_in("s1", "p1", "/work/team-a/api", prompt("Add pagination"));
    sim.at(2_000);
    sim.focus(VSCODE);
    sim.hook_in("s2", "q1", "/work/team-b/api", prompt("Fix the header"));
    sim.at(5_000);
    sim.focus("com.example.Elsewhere");
    sim.hook_in("s2", "q1", "/work/team-b/api", stop("Header fixed."));
    sim.at(20_000);
    assert_eq!(
        sim.toast_texts()[0],
        "Claude is done in team-b/api. Go back?"
    );
}

#[test]
fn a_lone_project_keeps_its_short_name() {
    let mut sim = Sim::new(Config::default());
    sim.hook_in("s1", "p1", "/work/team-a/api", prompt("Add pagination"));
    sim.at(2_000);
    sim.focus("com.example.Elsewhere");
    sim.hook_in("s1", "p1", "/work/team-a/api", stop("Done."));
    sim.at(10_000);
    assert_eq!(sim.toast_texts()[0], "Claude is done in api. Go back?");
}

#[test]
fn more_than_two_other_waiting_sessions_are_counted_not_listed() {
    let mut sim = Sim::new(Config::default());
    sim.hook_in("s0", "p0", "/work/main", prompt("Main task"));
    sim.at(1_000);
    for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
        let session = format!("s{}", i + 1);
        sim.hook_in(&session, "p", &format!("/work/{name}"), prompt("Task"));
    }
    sim.at(2_000);
    sim.focus("com.example.Elsewhere");
    for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
        let session = format!("s{}", i + 1);
        sim.hook_in(&session, "p", &format!("/work/{name}"), stop("Done."));
    }
    sim.hook_in("s0", "p0", "/work/main", permission("ls"));
    sim.at(20_000);
    sim.toast_action();
    sim.at(21_000);
    let card = &sim.recaps()[0];
    let also: Vec<&&str> = card.iter().filter(|l| l.starts_with("Also:")).collect();
    assert_eq!(also.len(), 3, "{card:?}");
    assert_eq!(*also[2], "Also:    2 more sessions");
}

#[test]
fn a_summary_keeps_the_other_sessions_results_beneath_it() {
    let mut config = Config::default();
    config.recap.mode = RecapMode::Llm;
    let mut sim = Sim::new(config);
    sim.hook_in("s1", "p1", "/work/api", prompt("Add pagination"));
    sim.at(2_000);
    sim.focus(VSCODE);
    sim.hook_in("s2", "q1", "/work/web", prompt("Fix the header"));
    sim.at(5_000);
    sim.focus("com.example.Elsewhere");
    sim.hook_in("s2", "q1", "/work/web", stop("Header fixed."));
    sim.hook_in("s1", "p1", "/work/api", stop("Pagination added."));
    sim.at(20_000);
    sim.toast_action();
    sim.at(21_000);
    let (id, _) = sim.summary_requests()[0];
    sim.feed(Input::Summary {
        id,
        text: "Asked: header\nDid: fixed it\nNeeds: nothing\n".into(),
    });
    let card = sim.recaps().concat();
    assert_eq!(
        card[..3],
        ["Asked: header", "Did: fixed it", "Needs: nothing"]
    );
    assert!(
        card[3].starts_with("Also:    "),
        "the other session's result must follow the summary: {card:?}"
    );
}
