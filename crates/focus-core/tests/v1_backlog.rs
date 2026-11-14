// SPDX-License-Identifier: Apache-2.0
//! V1 backlog items that live in the core: run records (E1), history and live-signal
//! estimates (E2, E3), the declare-long hotkey (E4), immersive apps (C4), the return queue
//! (H1), ignored projects (G5) and the LLM recap wiring (F2).

mod support;

use std::time::Duration;

use focus_core::{
    AwayHow, EventKind, Input, PauseFor, PromptLen, RecapMode, ReturnHow, RunOutcome, RunSample,
    SessionState, Tier, ToastKind, UserAction,
};
use support::{
    BROWSER, Sim, TERMINAL, TERRARIA, VSCODE, bash, config, permission, prompt, stop, tool,
};

fn past_runs(count: usize, secs: u64) -> Input {
    let sample = RunSample {
        project: "proj-a".into(),
        permission_mode: Some("default".into()),
        prompt_len: PromptLen::Short,
        duration: Duration::from_secs(secs),
    };
    Input::History(vec![sample; count])
}

#[test]
fn a_finished_run_is_recorded_once_with_counts_and_no_text() {
    let mut sim = Sim::away();
    sim.at(40_000);
    sim.hook(bash("npm test -- --token=hunter2"));
    sim.at(50_000);
    sim.hook(EventKind::SubagentStart);
    sim.at(100_000);
    sim.hook(stop("All 42 tests pass."));
    sim.at(159_000);
    assert!(sim.records().is_empty());
    sim.at(1_000_000);
    let records = sim.records();
    assert_eq!(records.len(), 1);
    let record = records[0];
    assert_eq!(record.run_id.as_deref(), Some("p1"));
    assert_eq!(record.project, "proj-a");
    assert_eq!(record.duration, Duration::from_secs(100));
    assert_eq!(record.outcome, RunOutcome::Done);
    assert_eq!(record.permission_mode.as_deref(), Some("default"));
    assert_eq!(record.effort.as_deref(), Some("high"));
    assert_eq!(record.prompt_len, PromptLen::Short);
    assert_eq!(
        (record.tools, record.subagents, record.slow_commands),
        (1, 1, 1)
    );
    assert!(!record.needed_input);
    assert_eq!(record.predicted_tier, Tier::Long);
    assert_eq!(
        (record.away, record.returned, record.snoozes),
        (AwayHow::Auto, ReturnHow::Auto, 0)
    );
    // The record type has no field that could hold the prompt, the command or a path.
    let dump = format!("{record:?}");
    assert!(!dump.contains("hunter2") && !dump.contains("npm") && !dump.contains("/work"));
}

#[test]
fn stale_replaced_and_ended_runs_are_recorded_as_such() {
    let mut sim = Sim::away();
    sim.at(700_000);
    let record = sim.records()[0];
    assert_eq!(
        (record.outcome, record.away, record.returned),
        (RunOutcome::Stale, AwayHow::Auto, ReturnHow::None)
    );
    assert_eq!(record.duration, Duration::ZERO);

    let mut sim = Sim::started();
    sim.at(5_000);
    sim.hook(tool("Read"));
    sim.at(10_000);
    sim.hook_for("s1", "p2", prompt("Second prompt"));
    assert_eq!(sim.records().len(), 1);
    assert_eq!(sim.records()[0].outcome, RunOutcome::Stale);
    assert_eq!(sim.records()[0].duration, Duration::from_secs(5));
    sim.at(20_000);
    sim.hook_for(
        "s1",
        "p2",
        EventKind::StopFailure {
            error: "rate_limit".into(),
        },
    );
    sim.hook_for("s1", "p2", EventKind::SessionEnd);
    assert_eq!(sim.records().len(), 2);
    assert_eq!(sim.records()[1].outcome, RunOutcome::Error);
    assert_eq!(sim.records()[1].run_id.as_deref(), Some("p2"));
}

#[test]
fn records_say_how_the_user_left_and_came_back() {
    // "Not now", then the run ends while the user is still home.
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.toast_action();
    sim.hook(permission("npm test"));
    sim.at(40_000);
    sim.hook(stop("Done."));
    sim.at(200_000);
    let record = sim.records()[0];
    assert_eq!(
        (record.away, record.returned),
        (AwayHow::Cancelled, ReturnHow::None)
    );
    assert!(record.needed_input);

    // Went to the game themselves, snoozed once, then came back themselves.
    let mut sim = Sim::started();
    sim.at(5_000);
    sim.focus(TERRARIA);
    sim.at(50_000);
    sim.hook(stop("Done."));
    sim.at(51_000);
    sim.toast_action();
    sim.at(70_000);
    sim.focus(TERMINAL);
    sim.at(200_000);
    let record = sim.records()[0];
    assert_eq!(
        (record.away, record.returned, record.snoozes),
        (AwayHow::Manual, ReturnHow::Manual, 1)
    );

    // Both switches refused by the OS, both completed from the prompt.
    let mut sim = Sim::started();
    sim.os_allows_switch = false;
    sim.at(40_000);
    sim.os_allows_switch = true;
    sim.toast_action();
    sim.at(100_000);
    sim.os_allows_switch = false;
    sim.hook(stop("Done."));
    sim.at(110_000);
    sim.os_allows_switch = true;
    sim.toast_action();
    sim.at(300_000);
    let record = sim.records()[0];
    assert_eq!(
        (record.away, record.returned),
        (AwayHow::Prompt, ReturnHow::Prompt)
    );
}

#[test]
fn a_run_first_seen_mid_way_is_not_recorded() {
    let mut sim = Sim::new(config());
    sim.hook_for("s9", "p9", tool("Read"));
    sim.hook_for("s9", "p9", stop("Done."));
    sim.at(500_000);
    sim.hook_for("s9", "p9", EventKind::SessionEnd);
    assert!(sim.records().is_empty());
}

#[test]
fn history_of_long_runs_brings_the_away_switch_forward() {
    let mut sim = Sim::new(config());
    sim.feed(past_runs(5, 400));
    sim.hook(prompt("Refactor the parser"));
    sim.at(20_000);
    assert_eq!(sim.toasts(), vec![(10_000, ToastKind::AwayCountdown)]);
}

#[test]
fn history_of_short_runs_holds_the_away_switch_back() {
    let mut sim = Sim::new(config());
    sim.feed(past_runs(5, 12));
    sim.hook(prompt("Rename a variable"));
    sim.at(44_000);
    assert!(sim.toasts().is_empty());
    sim.at(50_000);
    assert_eq!(sim.toasts(), vec![(45_000, ToastKind::AwayCountdown)]);
}

#[test]
fn too_little_history_leaves_elapsed_time_in_charge() {
    let mut sim = Sim::new(config());
    sim.feed(past_runs(4, 400));
    sim.hook(prompt("Refactor the parser"));
    sim.at(40_000);
    assert_eq!(sim.toasts(), vec![(30_000, ToastKind::AwayCountdown)]);
}

#[test]
fn finished_runs_feed_the_next_estimate() {
    let mut sim = Sim::new(config());
    sim.feed(past_runs(4, 400));
    sim.typing = true;
    sim.hook(prompt("First"));
    sim.at(400_000);
    sim.hook(stop("Done."));
    // Sixty seconds later the run is in history, making five similar runs.
    sim.at(470_000);
    assert_eq!(sim.records().len(), 1);
    sim.typing = false;
    sim.hook_for("s1", "p2", prompt("Second"));
    sim.at(490_000);
    assert_eq!(sim.toasts(), vec![(480_000, ToastKind::AwayCountdown)]);
}

#[test]
fn live_signals_raise_the_estimate() {
    // A sub-agent start.
    let mut sim = Sim::started();
    sim.at(2_000);
    sim.hook(EventKind::SubagentStart);
    sim.at(20_000);
    assert_eq!(sim.toasts(), vec![(10_000, ToastKind::AwayCountdown)]);
    // A test command.
    let mut sim = Sim::started();
    sim.at(2_000);
    sim.hook(bash("cargo test --workspace"));
    sim.at(20_000);
    assert_eq!(sim.toasts(), vec![(10_000, ToastKind::AwayCountdown)]);
    // More than fifteen tool calls.
    let mut sim = Sim::started();
    for _ in 0..15 {
        sim.hook(tool("Read"));
    }
    sim.at(20_000);
    assert!(sim.toasts().is_empty());
    sim.hook(tool("Read"));
    sim.at(25_000);
    assert_eq!(sim.toasts(), vec![(20_000, ToastKind::AwayCountdown)]);
}

#[test]
fn the_declare_long_hotkey_starts_the_away_switch_now() {
    let mut sim = Sim::started();
    sim.at(3_000);
    sim.input();
    sim.user(UserAction::DeclareLong);
    assert_eq!(sim.toasts(), vec![(3_000, ToastKind::AwayCountdown)]);
    sim.at(10_000);
    assert_eq!(sim.activations(), vec![(6_000, TERRARIA)]);
    assert_eq!(sim.logged("declared_long"), 1);
}

#[test]
fn declare_long_overrides_an_earlier_not_now_but_not_the_other_guards() {
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.toast_action();
    sim.at(60_000);
    sim.input();
    sim.user(UserAction::DeclareLong);
    sim.at(70_000);
    assert_eq!(sim.activations(), vec![(63_000, TERRARIA)]);

    // Pressed in another app, it does nothing.
    let mut sim = Sim::started();
    sim.focus(BROWSER);
    sim.user(UserAction::DeclareLong);
    sim.at(120_000);
    assert!(sim.toasts().is_empty());
    assert_eq!(sim.logged("declared_long"), 0);
}

#[test]
fn an_immersive_away_app_gets_a_longer_countdown_and_a_warning() {
    let mut config = config();
    config.away.immersive = vec![TERRARIA.into()];
    let mut sim = Sim::new(config);
    sim.hook(prompt("go"));
    sim.at(40_000);
    assert_eq!(
        sim.toast_texts(),
        vec![
            "Claude will take a while. Switching to Terraria in 5 s. It may be hard to leave when Claude is done."
        ]
    );
    assert_eq!(sim.activations(), vec![(35_000, TERRARIA)]);
}

#[test]
fn sessions_waiting_at_different_windows_are_returned_to_one_at_a_time() {
    let mut sim = Sim::started();
    sim.at(2_000);
    sim.focus(VSCODE);
    sim.hook_for("s2", "q1", prompt("Refactor the parser"));
    sim.at(40_000);
    assert_eq!(sim.activations(), vec![(35_000, TERRARIA)]);
    sim.user(UserAction::Pause(PauseFor::UntilResumed));
    sim.hook_for("s2", "q1", stop("Refactored."));
    sim.at(50_000);
    sim.hook(permission("cargo test"));
    sim.at(60_000);
    sim.user(UserAction::Resume);
    sim.at(70_000);
    // Needs-input first: back to Terminal, and the card names what else is waiting.
    assert_eq!(sim.activations()[1], (63_000, TERMINAL));
    assert_eq!(sim.recaps()[0][2], "Needs:   approval to run cargo test");
    assert_eq!(sim.recaps()[0][3], "Also waiting: app");
    // Then the finished session at the other window, by prompt because the user is working.
    let kinds: Vec<ToastKind> = sim.toasts().into_iter().map(|(_, kind)| kind).collect();
    assert_eq!(
        kinds,
        vec![
            ToastKind::AwayCountdown,
            ToastKind::ReturnCountdown,
            ToastKind::ReturnPrompt
        ]
    );
    assert_eq!(sim.sounds(), 2);
    sim.toast_action();
    sim.at(71_000);
    assert_eq!(sim.activations()[2], (70_000, VSCODE));
    assert_eq!(
        sim.recaps()[1][..3],
        [
            "Project: app",
            "Asked:   Refactor the parser",
            "Claude:  Refactored."
        ]
    );
    sim.at(500_000);
    assert_eq!(sim.toasts().len(), 3);
}

#[test]
fn events_from_ignored_folders_are_dropped_entirely() {
    let mut config = config();
    config.ignore.cwd_globs = vec!["/work/*".into()];
    let mut sim = Sim::new(config);
    sim.hook(prompt("go"));
    assert_eq!(sim.core.session_state("s1"), None);
    sim.at(120_000);
    sim.hook(stop("Done."));
    sim.at(300_000);
    assert!(sim.log.is_empty());

    let mut other = support::config();
    other.ignore.cwd_globs = vec!["/elsewhere/**".into()];
    let mut sim = Sim::new(other);
    sim.hook(prompt("go"));
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
}

fn llm_sim() -> Sim {
    let mut config = config();
    config.recap.mode = RecapMode::Llm;
    let mut sim = Sim::new(config);
    sim.hook(prompt("Deploy with API_TOKEN=hunter2 and fix the tests"));
    sim.at(34_000);
    sim.hook(bash("npm test"));
    sim.at(100_000);
    sim.hook(stop("Deployed; all tests pass."));
    sim.at(104_000);
    sim
}

#[test]
fn llm_mode_holds_back_the_basic_card_and_shows_the_summary_if_in_time() {
    let mut sim = llm_sim();
    // The long basic card is not shown first: only the request goes out.
    assert!(sim.recaps().is_empty());
    let (id, text) = sim.summary_requests()[0];
    assert!(text.starts_with("Summarize this coding-agent run"));
    assert!(text.contains("Prompt: Deploy with API_TOKEN=[redacted] and fix the tests"));
    assert!(text.contains("Commands run: npm test"));
    assert!(text.contains("Last message: Deployed; all tests pass."));
    assert!(!text.contains("hunter2"));
    sim.at(110_000);
    let reply = "Asked: deploy and fix tests\nDid: deployed, tests pass\nNeeds: nothing\n";
    sim.feed(Input::Summary {
        id,
        text: reply.into(),
    });
    assert_eq!(
        sim.recaps(),
        vec![vec![
            "Asked: deploy and fix tests",
            "Did: deployed, tests pass",
            "Needs: nothing"
        ]]
    );
    // A second reply for the same card is ignored, and the basic card never appears.
    sim.feed(Input::Summary {
        id,
        text: reply.into(),
    });
    sim.at(200_000);
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn an_unusable_or_mismatched_summary_falls_back_to_the_basic_card() {
    let mut sim = llm_sim();
    let (id, _) = sim.summary_requests()[0];
    // A reply for some other request changes nothing.
    sim.feed(Input::Summary {
        id: id + 1,
        text: "Asked: x".into(),
    });
    assert!(sim.recaps().is_empty());
    // An empty reply for this one shows the basic card at once.
    sim.feed(Input::Summary {
        id,
        text: "  \n".into(),
    });
    assert_eq!(sim.recaps().len(), 1);
    // The card is shown on screen, so the secret typed into the prompt is redacted there too.
    assert_eq!(
        sim.recaps()[0][1],
        "Asked:   Deploy with API_TOKEN=[redacted] and fix the tests"
    );
    assert!(!sim.recaps()[0].join("\n").contains("hunter2"));
    // And a later real reply for it is ignored.
    sim.feed(Input::Summary {
        id,
        text: "Asked: x".into(),
    });
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn a_failed_summary_shows_the_basic_card_at_once() {
    let mut sim = llm_sim();
    let (id, _) = sim.summary_requests()[0];
    assert!(sim.recaps().is_empty());
    sim.feed(Input::SummaryFailed { id });
    assert_eq!(sim.recaps().len(), 1);
    sim.feed(Input::SummaryFailed { id });
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn no_answer_by_the_timeout_shows_the_basic_card_and_a_late_reply_is_ignored() {
    let mut sim = llm_sim();
    let (id, _) = sim.summary_requests()[0];
    sim.at(120_000);
    assert!(sim.recaps().is_empty());
    sim.at(130_000);
    assert_eq!(sim.recaps().len(), 1);
    sim.feed(Input::Summary {
        id,
        text: "Asked: x".into(),
    });
    assert_eq!(sim.recaps().len(), 1);
}

#[test]
fn basic_mode_never_asks_for_a_summary() {
    let mut sim = Sim::away();
    sim.at(100_000);
    sim.hook(stop("Done."));
    sim.at(110_000);
    assert_eq!(sim.recaps().len(), 1);
    assert!(sim.summary_requests().is_empty());
}
