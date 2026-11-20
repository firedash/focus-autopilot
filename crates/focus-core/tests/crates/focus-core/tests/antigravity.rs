// SPDX-License-Identifier: Apache-2.0
//! App-level completion/input returns do not require a fabricated user prompt.
mod support;

use focus_core::{AppRef, Config, Effect, EventKind, RecapMode, SessionState};
use support::{BROWSER, Sim, app};

const AG: &str = "observed.example.antigravity";
fn antigravity() -> AppRef {
    AppRef {
        name: "Antigravity".into(),
        ..app(AG)
    }
}
fn sim() -> Sim {
    let mut cfg = Config::default();
    cfg.recap.mode = RecapMode::Llm;
    let mut sim = Sim::new(cfg);
    sim.running.push(antigravity());
    sim.focus(BROWSER);
    sim
}
fn done() -> EventKind {
    EventKind::AppFinished {
        app_name: "Antigravity".into(),
        message: "Task finished; other work may still be running".into(),
        failed: false,
    }
}
fn needs_input() -> EventKind {
    EventKind::AppNeedsInput {
        app_name: "Antigravity".into(),
    }
}

#[test]
fn completion_brings_app_forward_without_any_prompt_or_home_capture() {
    let mut sim = sim();
    sim.at(60_000);
    sim.hook_for("antigravity:a", "", done());
    sim.at(70_000);
    assert_eq!(sim.activations(), vec![(65_000, AG)]);
    assert!(sim.toast_texts()[0].starts_with("Antigravity finished a task"));
    assert!(
        sim.recaps()[0]
            .iter()
            .any(|s| s.contains("other work may still be running"))
    );
    assert!(
        !sim.log
            .iter()
            .any(|(_, fx)| matches!(fx, Effect::SummarizeRecap { .. }))
    );
}

#[test]
fn helpers_continuing_do_not_withdraw_the_completion_return() {
    let mut sim = sim();
    sim.at(60_000);
    sim.hook_for("antigravity:a", "", done());
    sim.at(61_000);
    sim.hook_for("antigravity:a", "", EventKind::AgentActivity);
    sim.hook_for(
        "antigravity:a",
        "",
        EventKind::PreToolUse {
            tool_name: "run_command".into(),
            command: None,
            file_path: None,
            effort: None,
        },
    );
    sim.at(70_000);
    assert_eq!(sim.activations(), vec![(65_000, AG)]);
}

#[test]
fn explicit_input_request_brings_app_forward_after_debounce() {
    let mut sim = sim();
    sim.at(60_000);
    sim.hook_for("antigravity:a", "", needs_input());
    sim.at(70_000);
    assert_eq!(sim.activations(), vec![(67_000, AG)]);
    assert!(sim.toast_texts()[0].starts_with("Antigravity needs your input"));
    assert_eq!(
        sim.core.session_state("antigravity:a"),
        Some(SessionState::NeedsInput)
    );
    sim.hook_for("antigravity:a", "", EventKind::AgentActivity);
    sim.at(300_000);
    // Resolving input must not arm the old automatic-away loop.
    assert_eq!(sim.activations().len(), 1);
}

#[test]
fn input_resolved_immediately_causes_no_interruption() {
    let mut sim = sim();
    sim.at(60_000);
    sim.hook_for("antigravity:a", "", needs_input());
    sim.at(61_000);
    sim.hook_for("antigravity:a", "", EventKind::AgentActivity);
    sim.at(90_000);
    assert!(sim.activations().is_empty());
    assert!(sim.toasts().is_empty());
}

#[test]
fn unknown_or_ambiguous_app_never_falls_back_to_the_foreground_window() {
    for ambiguous in [false, true] {
        let mut sim = sim();
        if ambiguous {
            sim.running.push(AppRef {
                id: "another.application".into(),
                ..antigravity()
            });
        } else {
            sim.running.retain(|a| a.id != AG);
        }
        sim.at(60_000);
        sim.hook_for("antigravity:a", "", done());
        sim.at(90_000);
        assert!(sim.activations().is_empty());
        assert!(sim.toasts().is_empty());
    }
}

#[test]
fn duplicate_stop_is_silent_but_a_new_execution_can_return_again() {
    let mut sim = sim();
    sim.at(60_000);
    sim.hook_for("antigravity:a", "", done());
    sim.at(70_000);
    sim.focus(BROWSER);
    sim.hook_for("antigravity:a", "", done());
    sim.at(80_000);
    assert_eq!(sim.activations().len(), 1);
    sim.hook_for("antigravity:a", "", EventKind::AgentActivity);
    sim.at(120_000);
    sim.hook_for("antigravity:a", "", done());
    sim.at(130_000);
    assert_eq!(sim.activations().len(), 2);
}

#[test]
fn missing_or_busy_session_start_never_moves_the_user() {
    let mut sim = sim();
    sim.hook_for("antigravity:a", "", EventKind::AgentActivity);
    sim.at(120_000);
    assert!(sim.activations().is_empty());
    assert!(sim.toasts().is_empty());
}

#[test]
fn already_in_antigravity_is_quiet_and_snooze_still_works() {
    let mut sim = sim();
    sim.front = Some(antigravity());
    sim.feed(focus_core::Input::Observed(
        focus_core::Observation::Frontmost(sim.front.clone()),
    ));
    sim.hook_for("antigravity:a", "", done());
    sim.at(10_000);
    assert!(sim.toasts().is_empty());
    assert!(sim.activations().is_empty());
    sim.focus(BROWSER);
    sim.hook_for("antigravity:b", "", done());
    sim.at(11_000);
    sim.toast_action();
    sim.at(30_000);
    assert!(sim.activations().is_empty());
}

#[test]
fn cancellation_withdraws_pending_completion_or_input_without_activation() {
    for kind in [done(), needs_input()] {
        let mut sim = sim();
        sim.at(60_000);
        sim.hook_for("antigravity:a", "", kind);
        sim.at(61_000);
        sim.hook_for("antigravity:a", "", EventKind::SessionEnd);
        sim.at(90_000);
        assert!(sim.activations().is_empty());
        // The same conversation may start work again after a manual cancellation.
        sim.hook_for("antigravity:a", "", EventKind::AgentActivity);
        sim.at(120_000);
        sim.hook_for("antigravity:a", "", done());
        sim.at(130_000);
        assert_eq!(sim.activations().len(), 1);
    }
}
