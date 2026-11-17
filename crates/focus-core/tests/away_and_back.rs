// SPDX-License-Identifier: Apache-2.0
//! The full loop and the away half of the switch controller (backlog B2, C3, D1, D3, D4, D6, D7).

mod support;

use focus_core::{
    Effect, Input, Note, Observation, PauseFor, SessionState, Status, ToastKind, UserAction,
};
use support::{BROWSER, Sim, TERMINAL, TERRARIA, app, config, permission, stop, trace};

/// The design's first fixture: approve a test run, play while it runs, come back to a recap.
/// `npm test` is a slow command, so the run counts as long and the away switch comes early.
#[test]
fn replayed_trace_switches_away_and_back_with_a_recap() {
    let mut sim = Sim::new(config());
    for (at, event) in trace::load("approve_then_tests.jsonl") {
        match at {
            95_000 => {
                // Seven seconds in, the user approves the permission prompt and waits.
                sim.at(7_000);
                sim.input();
                sim.at(94_000);
                assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
            }
            131_000 => {
                // Still playing a second before Claude stops.
                sim.at(130_000);
                sim.input();
            }
            _ => {}
        }
        sim.at(at);
        sim.feed(Input::Hook(event));
    }
    sim.at(140_000);

    assert_eq!(
        sim.toasts(),
        vec![
            (16_200, ToastKind::AwayCountdown),
            (131_000, ToastKind::ReturnCountdown)
        ]
    );
    assert_eq!(
        sim.toast_texts(),
        vec![
            "Claude will take a while. Switching to Terraria in 3 s.",
            "Claude is done in app. Back to Terminal in 5 s.",
        ]
    );
    assert_eq!(
        sim.activations(),
        vec![(19_200, TERRARIA), (136_000, TERMINAL)]
    );
    assert_eq!(sim.sounds(), 1);
    assert_eq!(
        sim.recaps(),
        vec![vec![
            "Project: app",
            "Asked:   Run the tests and fix failures",
            "Claude:  Fixed the failing sum test; all 42 tests pass.",
            "Touched: src/sum.ts",
            "2 min · 2 tool calls",
        ]]
    );
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Done));
    assert_eq!(sim.status(), Status::Idle);
}

#[test]
fn an_unanswered_prompt_never_triggers_an_away_switch() {
    let mut sim = Sim::started();
    sim.at(5_000);
    sim.hook(permission("npm test"));
    sim.at(300_000);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::NeedsInput));
    assert!(sim.toasts().is_empty());
    assert!(sim.activations().is_empty());
}

#[test]
fn approving_then_going_idle_rearms_the_away_switch() {
    let mut sim = Sim::started();
    sim.at(5_000);
    sim.hook(permission("npm test"));
    sim.at(7_000);
    sim.input();
    sim.at(14_900);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::NeedsInput));
    sim.at(15_100);
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    sim.at(40_000);
    assert_eq!(sim.toasts(), vec![(30_000, ToastKind::AwayCountdown)]);
    assert_eq!(sim.activations(), vec![(33_000, TERRARIA)]);
}

#[test]
fn not_now_cancels_away_switches_for_the_rest_of_the_run() {
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.toast_action();
    sim.at(500_000);
    assert_eq!(sim.toasts(), vec![(30_000, ToastKind::AwayCountdown)]);
    assert_eq!(sim.hides(), 1);
    assert_eq!(sim.logged("away_cancelled"), 1);
    assert!(sim.activations().is_empty());
    // The user never left, so the end of the run needs no toast, sound or recap.
    sim.hook(stop("Done."));
    sim.at(510_000);
    assert_eq!(sim.toasts().len(), 1);
    assert_eq!(sim.sounds(), 0);
    assert!(sim.recaps().is_empty());
}

#[test]
fn a_refused_switch_falls_back_to_a_one_key_prompt() {
    let mut sim = Sim::started();
    sim.os_allows_switch = false;
    sim.at(34_000);
    assert_eq!(
        sim.toasts(),
        vec![
            (30_000, ToastKind::AwayCountdown),
            (33_400, ToastKind::AwayPrompt)
        ]
    );
    assert_eq!(
        sim.toast_texts()[1],
        "Claude will take a while. Switch to Terraria?"
    );
    // A click gives focusd the standing to switch.
    sim.at(40_000);
    sim.os_allows_switch = true;
    sim.toast_action();
    sim.at(41_000);
    assert_eq!(
        sim.activations(),
        vec![(33_000, TERRARIA), (40_000, TERRARIA)]
    );
    assert_eq!(sim.logged("away_switched"), 1);
    assert_eq!(sim.status(), Status::Away);
}

#[test]
fn an_ignored_away_prompt_goes_away_and_is_not_repeated() {
    let mut sim = Sim::started();
    sim.os_allows_switch = false;
    sim.at(500_000);
    assert_eq!(sim.toasts().len(), 2);
    assert_eq!(sim.activations().len(), 1);
    let last_hide = sim
        .log
        .iter()
        .rev()
        .find(|(_, fx)| *fx == Effect::HideToast)
        .unwrap();
    assert_eq!(last_hide.0, 63_400);
}

#[test]
fn a_refused_click_shows_the_prompt_again() {
    let mut sim = Sim::started();
    sim.os_allows_switch = false;
    sim.at(40_000);
    sim.toast_action();
    sim.at(41_000);
    let kinds: Vec<ToastKind> = sim.toasts().into_iter().map(|(_, kind)| kind).collect();
    assert_eq!(
        kinds,
        vec![
            ToastKind::AwayCountdown,
            ToastKind::AwayPrompt,
            ToastKind::AwayPrompt
        ]
    );
}

#[test]
fn a_shell_that_never_reports_back_counts_as_a_refusal() {
    let mut sim = Sim::started();
    sim.os_allows_switch = false;
    sim.shell_verifies = false;
    sim.at(36_000);
    assert_eq!(
        sim.toasts(),
        vec![
            (30_000, ToastKind::AwayCountdown),
            (35_400, ToastKind::AwayPrompt)
        ]
    );
}

#[test]
fn an_active_user_is_never_switched_away() {
    let mut sim = Sim::started();
    sim.typing = true;
    sim.at(120_000);
    assert!(sim.toasts().is_empty());
    sim.typing = false;
    sim.input();
    sim.at(130_000);
    assert_eq!(sim.toasts(), vec![(128_000, ToastKind::AwayCountdown)]);
}

#[test]
fn input_that_continues_after_the_countdown_gets_the_prompt_instead() {
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.typing = true;
    sim.at(40_000);
    assert_eq!(
        sim.toasts(),
        vec![
            (30_000, ToastKind::AwayCountdown),
            (38_000, ToastKind::AwayPrompt)
        ]
    );
    assert!(sim.activations().is_empty());
}

#[test]
fn leaving_the_home_window_withdraws_the_countdown() {
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.focus(BROWSER);
    sim.at(200_000);
    assert_eq!(sim.toasts().len(), 1);
    assert_eq!(sim.hides(), 1);
    assert!(sim.activations().is_empty());
}

#[test]
fn a_run_ending_during_the_countdown_withdraws_it() {
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.hook(stop("Done."));
    sim.at(60_000);
    assert_eq!(sim.toasts().len(), 1);
    assert_eq!(sim.hides(), 1);
    assert!(sim.activations().is_empty());
    assert!(sim.recaps().is_empty());
}

#[test]
fn no_running_away_app_means_no_switch_and_a_tray_note() {
    let mut sim = Sim::started();
    sim.running.retain(|a| a.id != TERRARIA);
    sim.at(35_000);
    assert!(sim.toasts().is_empty());
    assert_eq!(sim.core.note(), Some(Note::NoAwayAppRunning));
    sim.running.push(app(TERRARIA));
    sim.at(40_000);
    assert_eq!(sim.toasts(), vec![(36_000, ToastKind::AwayCountdown)]);
    assert_eq!(sim.core.note(), None);
}

#[test]
fn the_first_running_away_app_in_config_order_wins() {
    let mut config = config();
    config.away.apps = vec!["not.running".into(), "com.second".into(), TERRARIA.into()];
    let mut sim = Sim::new(config);
    sim.running.push(app("COM.SECOND"));
    sim.hook(support::prompt("go"));
    sim.at(34_000);
    assert_eq!(sim.activations(), vec![(33_000, "COM.SECOND")]);
}

#[test]
fn on_windows_only_the_exact_home_window_counts() {
    let mut sim = Sim::new(config());
    let window = |handle| focus_core::AppRef {
        window: Some(handle),
        ..app(TERMINAL)
    };
    sim.front = Some(window(1));
    sim.feed(Input::Observed(Observation::Frontmost(sim.front.clone())));
    sim.hook(support::prompt("go"));
    sim.at(5_000);
    sim.front = Some(window(2));
    sim.feed(Input::Observed(Observation::Frontmost(sim.front.clone())));
    sim.at(60_000);
    assert!(sim.toasts().is_empty());
    sim.front = Some(window(1));
    sim.feed(Input::Observed(Observation::Frontmost(sim.front.clone())));
    // Back at the home window: the threshold counts again from here.
    sim.at(70_000);
    assert!(sim.toasts().is_empty());
    sim.at(100_000);
    assert_eq!(sim.toasts().len(), 1);
}

#[test]
fn a_prompt_sent_from_an_unknown_app_never_switches() {
    let mut sim = Sim::new(config());
    sim.focus(BROWSER);
    sim.hook(support::prompt("go"));
    sim.focus(TERMINAL);
    sim.at(120_000);
    assert!(sim.toasts().is_empty());
}

#[test]
fn an_extra_home_app_from_config_counts_as_home() {
    let mut config = config();
    config.home.extra_apps = vec![BROWSER.into()];
    let mut sim = Sim::new(config);
    sim.focus(BROWSER);
    sim.hook(support::prompt("go"));
    sim.at(34_000);
    assert_eq!(sim.activations(), vec![(33_000, TERRARIA)]);
}

#[test]
fn while_paused_state_updates_but_nothing_switches() {
    let mut sim = Sim::started();
    sim.at(10_000);
    sim.user(UserAction::Pause(PauseFor::UntilResumed));
    sim.at(100_000);
    assert!(sim.toasts().is_empty());
    assert_eq!(sim.core.session_state("s1"), Some(SessionState::Running));
    assert_eq!(sim.status(), Status::Paused { until: None });
    sim.user(UserAction::Resume);
    sim.at(105_000);
    assert_eq!(sim.toasts(), vec![(100_000, ToastKind::AwayCountdown)]);
}

#[test]
fn a_timed_pause_ends_on_its_own_and_the_hotkey_toggles() {
    let mut sim = Sim::started();
    sim.user(UserAction::Pause(PauseFor::Minutes(1)));
    sim.at(59_000);
    assert!(sim.toasts().is_empty());
    sim.at(64_000);
    assert_eq!(sim.toasts(), vec![(60_000, ToastKind::AwayCountdown)]);
    // Pausing while a toast shows withdraws it.
    let mut sim = Sim::started();
    sim.at(31_000);
    sim.user(UserAction::TogglePause);
    assert_eq!(sim.hides(), 1);
    sim.at(100_000);
    assert!(sim.activations().is_empty());
    sim.user(UserAction::TogglePause);
    sim.at(104_000);
    assert_eq!(sim.activations(), vec![(103_000, TERRARIA)]);
}

#[test]
fn disabling_in_config_stops_switching() {
    let mut sim = Sim::started();
    let mut off = config();
    off.enabled = false;
    sim.feed(Input::Config(Box::new(off)));
    sim.at(120_000);
    assert!(sim.toasts().is_empty());
    assert_eq!(sim.status(), Status::Paused { until: None });
}

#[test]
fn nothing_is_scheduled_before_the_first_input() {
    let core = focus_core::Core::new(config(), Vec::new());
    assert_eq!(core.next_deadline(), None);
    assert_eq!(core.status(std::time::Instant::now()), Status::Idle);
}
