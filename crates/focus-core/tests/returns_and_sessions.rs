// SPDX-License-Identifier: Apache-2.0
//! The return half of the switch controller and the session transitions
//! (backlog B1, D2, D3, D5, D6, D8, F1 and the multi-session rule).

mod support;

use focus_core::{EventKind, PauseFor, SessionState, Status, ToastKind, UserAction};
use support::{BROWSER, Sim, TERMINAL, TERRARIA, bash, config, permission, prompt, stop, tool};

fn kinds(sim: &Sim) -> Vec<ToastKind> {
    sim.toasts().into_iter().map(|(_, kind)| kind).collect()
}

#[test]
fn a_stop_right_after_an_away_switch_gets_the_prompt_not_a_flip_back() {
    let mut sim = Sim::away();
    sim.at(36_000);
    sim.hook(stop("Done."));
    sim.at(60_000);
    assert_eq!(sim.toasts()[1], (36_000, ToastKind::ReturnPrompt));
    assert_eq!(sim.toast_texts()[1], "Claude is done in app. Go back?");
    assert_eq!(sim.sounds(), 1);
    assert_eq!(sim.activations().len(), 1);
    // One click or the focus key completes the return.
    sim.toast_action();
    sim.at(61_000);
    assert_eq!(sim.activations()[1], (60_000, TERMINAL));
    assert_eq!(
        sim.recaps(),
        vec![vec![
            "Project: app",
            "Asked:   Run the tests and fix failures",
            "Claude:  Done.",
            "36 s",
        ]]
    );
}

#[test]
fn a_needs_input_that_clears_within_two_seconds_does_nothing() {
    let mut sim = Sim::away();
    sim.at(60_000);
    sim.hook(permission("npm test"));
    sim.at(61_000);
    sim.hook(bash("npm test"));
    sim.at(70_000);
    assert_eq!(sim.toasts().len(), 1);
    assert_eq!(sim.sounds(), 0);
}

#[test]
fn a_permission_prompt_brings_the_user_back_with_what_is_needed() {
    let mut sim = Sim::away();
    sim.at(70_000);
    sim.hook(permission("npm test"));
    sim.at(80_000);
    assert_eq!(sim.toasts()[1], (72_000, ToastKind::ReturnCountdown));
    assert_eq!(
        sim.toast_texts()[1],
        "Claude needs your approval in app. Back to Terminal in 3 s."
    );
    assert_eq!(sim.activations()[1], (75_000, TERMINAL));
    assert_eq!(
        sim.recaps(),
        vec![vec![
            "Project: app",
            "Asked:   Run the tests and fix failures",
            "Needs:   approval to run npm test"
        ]]
    );
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::NeedsInput));
}

#[test]
fn return_on_needs_input_can_be_turned_off() {
    let mut config = config();
    config.switching.return_on_needs_input = false;
    let mut sim = Sim::new(config);
    sim.hook(prompt("go"));
    sim.at(34_000);
    sim.hook(permission("npm test"));
    sim.at(100_000);
    assert_eq!(kinds(&sim), vec![ToastKind::AwayCountdown]);
}

#[test]
fn from_another_app_the_user_gets_a_prompt_and_coming_back_shows_the_recap() {
    let mut sim = Sim::away();
    sim.at(50_000);
    sim.focus(BROWSER);
    sim.at(100_000);
    sim.hook(stop("All green."));
    sim.at(120_000);
    assert_eq!(sim.toasts()[1], (100_000, ToastKind::ReturnPrompt));
    assert_eq!(sim.activations().len(), 1);
    sim.focus(TERMINAL);
    assert_eq!(sim.hides(), 2);
    assert_eq!(sim.recaps().len(), 1);
    assert_eq!(sim.status(), Status::Idle);
    // Settled: nothing more happens for this run.
    sim.at(400_000);
    assert_eq!(sim.toasts().len(), 2);
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn coming_back_on_your_own_shows_the_recap_only_after_twenty_seconds_away() {
    let mut sim = Sim::away();
    sim.at(45_000);
    sim.focus(TERMINAL);
    assert!(sim.recaps().is_empty());
    assert_eq!(
        sim.status(),
        Status::Running {
            elapsed: std::time::Duration::from_secs(45)
        }
    );

    let mut sim = Sim::away();
    sim.at(40_000);
    sim.hook(support::edit("/work/app/src/a.rs"));
    sim.at(60_000);
    sim.focus(TERMINAL);
    assert_eq!(
        sim.recaps(),
        vec![vec![
            "Project: app",
            "Asked:   Run the tests and fix failures",
            "Claude:  still working",
            "Touched: src/a.rs"
        ]]
    );
    // The away switch for this run is used up.
    sim.at(300_000);
    assert_eq!(sim.toasts().len(), 1);
}

#[test]
fn snooze_works_three_times_and_then_the_prompt_stays() {
    let mut sim = Sim::away();
    sim.at(100_000);
    sim.hook(stop("Done."));
    for click_at in [101_000, 162_000, 223_000] {
        sim.at(click_at);
        sim.toast_action();
    }
    sim.at(1_000_000);
    assert_eq!(
        sim.toasts(),
        vec![
            (30_000, ToastKind::AwayCountdown),
            (100_000, ToastKind::ReturnCountdown),
            (161_000, ToastKind::ReturnCountdown),
            (222_000, ToastKind::ReturnCountdown),
            (283_000, ToastKind::ReturnPrompt),
        ]
    );
    assert_eq!(sim.logged("return_snoozed"), 3);
    assert_eq!(sim.sounds(), 1);
    assert_eq!(sim.activations().len(), 1);
}

#[test]
fn coming_back_during_a_snooze_shows_the_recap() {
    let mut sim = Sim::away();
    sim.at(100_000);
    sim.hook(stop("Done."));
    sim.at(101_000);
    sim.toast_action();
    sim.at(120_000);
    sim.focus(TERMINAL);
    sim.at(400_000);
    assert_eq!(sim.recaps().len(), 1);
    assert_eq!(sim.toasts().len(), 2);
}

#[test]
fn a_return_waits_for_quiet_input_and_falls_back_to_the_prompt() {
    let mut sim = Sim::away();
    sim.typing = true;
    sim.at(100_000);
    sim.hook(stop("Done."));
    sim.at(120_000);
    assert_eq!(
        sim.toasts()[1..],
        [
            (100_000, ToastKind::ReturnCountdown),
            (108_000, ToastKind::ReturnPrompt)
        ]
    );
    assert_eq!(sim.activations().len(), 1);

    // A short pause in the game is enough.
    let mut sim = Sim::away();
    sim.typing = true;
    sim.at(100_000);
    sim.hook(stop("Done."));
    sim.at(104_000);
    sim.typing = false;
    sim.input();
    sim.at(110_000);
    assert_eq!(sim.activations()[1], (104_600, TERMINAL));
}

#[test]
fn a_refused_return_falls_back_to_the_prompt() {
    let mut sim = Sim::away();
    sim.at(100_000);
    sim.os_allows_switch = false;
    sim.hook(stop("Done."));
    sim.at(110_000);
    assert_eq!(
        sim.toasts()[1..],
        [
            (100_000, ToastKind::ReturnCountdown),
            (103_400, ToastKind::ReturnPrompt)
        ]
    );
    assert!(sim.recaps().is_empty());
    sim.os_allows_switch = true;
    sim.toast_action();
    sim.at(111_000);
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn a_new_prompt_from_elsewhere_withdraws_the_return_prompt() {
    let mut sim = Sim::away();
    sim.focus(BROWSER);
    sim.at(100_000);
    sim.hook(stop("Done."));
    assert_eq!(sim.toasts().len(), 2);
    sim.at(110_000);
    sim.hook_for("s1", "p2", prompt("next"));
    assert_eq!(sim.hides(), 2);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
}

#[test]
fn an_error_stop_returns_and_names_the_error() {
    let mut sim = Sim::away();
    sim.at(100_000);
    sim.hook(EventKind::StopFailure {
        error: "rate_limit".into(),
    });
    sim.at(110_000);
    assert_eq!(
        sim.toast_texts()[1],
        "Claude stopped with an error in app. Back to Terminal in 3 s."
    );
    assert_eq!(
        sim.recaps()[0][2],
        "Claude:  stopped with an error (rate_limit)"
    );
}

#[test]
fn questions_plans_elicitations_and_notifications_all_need_input() {
    let cases: [(EventKind, &str, &str); 4] = [
        (
            tool("AskUserQuestion"),
            "Claude has a question in app.",
            "an answer to Claude's question",
        ),
        (
            tool("ExitPlanMode"),
            "Claude needs your approval in app.",
            "approval of Claude's plan",
        ),
        (
            EventKind::Elicitation {
                mcp_server_name: "github".into(),
            },
            "Claude needs your input in app.",
            "your input for github",
        ),
        (
            EventKind::Notification {
                notification_type: "permission_prompt".into(),
            },
            "Claude needs your input in app.",
            "your input",
        ),
    ];
    for (event, headline, need) in cases {
        let mut sim = Sim::away();
        sim.at(100_000);
        sim.hook(event);
        sim.at(110_000);
        assert_eq!(
            sim.toast_texts()[1],
            format!("{headline} Back to Terminal in 3 s.")
        );
        assert_eq!(sim.recaps()[0][2], format!("Needs:   {need}"));
    }
    // Other notifications are not a reason to come back.
    let mut sim = Sim::away();
    sim.hook(EventKind::Notification {
        notification_type: "idle_prompt".into(),
    });
    sim.at(100_000);
    assert_eq!(sim.toasts().len(), 1);
}

#[test]
fn with_two_sessions_waiting_needs_input_goes_first_and_the_card_lists_the_rest() {
    let mut sim = Sim::started();
    sim.at(1_000);
    sim.hook_for("s2", "q1", prompt("Refactor the parser"));
    sim.at(35_000);
    sim.user(UserAction::Pause(PauseFor::UntilResumed));
    sim.hook_for("s2", "q1", stop("Refactored."));
    sim.at(40_000);
    sim.hook(permission("cargo test"));
    sim.at(50_000);
    assert_eq!(sim.toasts().len(), 1);
    sim.user(UserAction::Resume);
    sim.at(60_000);
    assert_eq!(
        sim.toast_texts()[1],
        "Claude needs your approval in app. Back to Terminal in 3 s."
    );
    assert_eq!(
        sim.recaps(),
        vec![vec![
            "Project: app",
            "Asked:   Run the tests and fix failures",
            "Needs:   approval to run cargo test",
            "Also waiting: app",
        ]]
    );
    // One switch settles both; no second toast for the finished session.
    sim.at(200_000);
    assert_eq!(sim.toasts().len(), 2);
}

#[test]
fn a_waiting_session_blocks_away_switches_for_every_session() {
    let mut sim = Sim::started();
    sim.hook_for("s2", "q1", prompt("other work"));
    sim.at(5_000);
    sim.hook_for("s2", "q1", permission("rm -rf build"));
    sim.at(200_000);
    assert!(sim.toasts().is_empty());
}

#[test]
fn a_quiet_run_goes_stale_without_a_return_or_recap() {
    let mut sim = Sim::away();
    sim.at(599_000);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    sim.at(601_000);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Idle));
    assert_eq!(sim.logged("run_stale"), 1);
    sim.at(2_000_000);
    assert_eq!(sim.toasts().len(), 1);
    assert!(sim.recaps().is_empty());
    // A late event wakes the session, but with no home window it never switches.
    sim.hook(tool("Read"));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    sim.hook(stop("Done."));
    sim.at(2_100_000);
    assert_eq!(sim.toasts().len(), 1);
}

#[test]
fn a_stop_hook_continuation_reopens_the_run_without_a_second_away_switch() {
    let mut sim = Sim::started();
    sim.at(20_000);
    sim.hook(stop("First pass done."));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Done));
    sim.at(25_000);
    sim.hook(tool("Read"));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    assert_eq!(sim.logged("run_continued"), 1);
    sim.at(300_000);
    assert!(sim.toasts().is_empty());
    // A second Stop that reports an active Stop hook closes the run again.
    sim.hook(EventKind::Stop {
        stop_hook_active: true,
        last_assistant_message: "Second pass done.".into(),
        background_tasks: 2,
    });
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Done));
    sim.at(400_000);
    sim.hook(EventKind::Stop {
        stop_hook_active: true,
        last_assistant_message: "Third pass done.".into(),
        background_tasks: 0,
    });
    assert_eq!(sim.logged("run_finished"), 3);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Done));
}

#[test]
fn a_session_first_seen_mid_run_is_tracked_but_never_switches() {
    let mut sim = Sim::new(config());
    sim.hook_for("s9", "p9", tool("Read"));
    assert_eq!(sim.core.session_state("s9"), Some(SessionState::Running));
    assert_eq!(sim.logged("run_adopted_without_home"), 1);
    sim.at(120_000);
    sim.focus(TERRARIA);
    sim.hook_for("s9", "p9", stop("Done."));
    sim.at(200_000);
    assert!(sim.toasts().is_empty());
    assert_eq!(sim.core.session_state("s9"), Some(SessionState::Done));
}

#[test]
fn late_events_from_an_earlier_run_are_ignored() {
    let mut sim = Sim::started();
    sim.at(10_000);
    sim.hook_for("s1", "p2", prompt("Second prompt"));
    sim.hook_for("s1", "p1", stop("Late."));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    assert_eq!(sim.logged("late_event_ignored"), 1);
    // Without a prompt id (older Claude Code) the event belongs to the current run.
    sim.feed(focus_core::Input::Hook(focus_core::AgentEvent {
        session_id: "s1".into(),
        prompt_id: None,
        cwd: "/work/app".into(),
        project: "proj-a".into(),
        kind: stop("Done."),
    }));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Done));
}

#[test]
fn session_end_forgets_the_session() {
    let mut sim = Sim::started();
    sim.hook(EventKind::SessionEnd);
    assert_eq!(sim.core.session_state("s1"), None);
    assert_eq!(sim.status(), Status::Idle);
    sim.hook_for("never-seen", "p", EventKind::SessionEnd);
    assert_eq!(sim.logged("session_ended"), 1);
    sim.at(120_000);
    assert!(sim.toasts().is_empty());
}

#[test]
fn denial_subagents_and_repeat_requests_keep_the_state_right() {
    let mut sim = Sim::started();
    sim.hook(EventKind::SubagentStart);
    sim.hook(EventKind::SubagentStop);
    sim.hook(permission("npm test"));
    sim.hook(permission("npm run lint"));
    sim.hook(EventKind::SubagentStop);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::NeedsInput));
    sim.hook(EventKind::PermissionDenied);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    sim.hook(permission("npm test"));
    sim.hook(EventKind::SubagentStart);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    sim.hook(permission("npm test"));
    sim.hook(tool("AskUserQuestion"));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::NeedsInput));
    sim.hook(stop("Done."));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Done));
}

#[test]
fn recap_text_is_cleaned_and_bounded() {
    let mut sim = Sim::new(config());
    sim.hook(prompt(&format!(
        "\u{1b}[1mFix\u{1b}[0m the\nbug {}",
        "x".repeat(900)
    )));
    sim.at(34_000);
    for i in 0..8 {
        sim.hook(support::edit(&format!("/work/app/f{i}.rs")));
    }
    sim.at(100_000);
    sim.hook(EventKind::Stop {
        stop_hook_active: false,
        last_assistant_message: "<b>done</b>\u{7}".into(),
        background_tasks: 1,
    });
    sim.at(110_000);
    let recap = &sim.recaps()[0];
    assert!(recap[1].starts_with("Asked:   Fix the bug xxx"));
    assert_eq!(recap[1].chars().count(), 120);
    assert_eq!(recap[2], "Claude:  <b>done</b>");
    assert_eq!(
        recap[3],
        "Touched: f0.rs, f1.rs, f2.rs, f3.rs, f4.rs (+3 more)"
    );
    assert_eq!(
        recap[4],
        "2 min · 8 tool calls · 1 background task still running"
    );
    assert!(recap.len() <= 6);
}
