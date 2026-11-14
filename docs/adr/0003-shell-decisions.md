# ADR 0003: Choices made while building the shell (receiver, focusd, focusctl, overlay)

Date: 2026-11-14. Status: accepted, open to revision after real use. Where this record and
`docs/design.md` disagree, this record wins; `design.md` carries a pointer to it.

1. **Manual mode is the default; automatic switching away is optional.** The design's loop
   was: switch to a configured away app after a long run, switch back on completion. In use
   it was simpler and less surprising to let the user go to any window themselves. With
   `away.apps` empty (the default) nothing switches the user away. When Claude finishes or
   needs approval while the user is not in the window they prompted from, they get a sound,
   a toast and the one-key prompt "Go back?"; the focus key takes them back and shows the
   recap. If `away.apps` is set, the old behaviour (countdown away, countdown back) still
   runs. No core change was needed: the core already prompted instead of counting down
   whenever the user was not on an away app. `focus-core/tests/manual_mode.rs` pins this.
   Not switching the user away also removes the main safety risk the countdowns existed for,
   a keystroke landing in the wrong window, for the away direction.

2. **The overlay is two non-activating floating panels.** A borderless `NSPanel` that
   ignores the mouse and is shown with `orderFrontRegardless` cannot take keyboard focus.
   One panel shows the toast, one the recap card. The Tauri shell in the design is not used
   yet; `focusd` runs in a terminal and also prints every toast and recap there.

3. **Global hotkeys use Carbon `RegisterEventHotKey`.** It needs no permission and never
   sees other keystrokes, unlike key-event monitoring, which needs Accessibility. The
   binding is hand-written FFI in `focus-platform/src/macos_hotkeys.rs`, the only place in
   the project that declares foreign functions by hand, because the `objc2` crates do not
   cover it. Each `unsafe` block carries a written justification.

4. **The LLM recap holds back the basic card.** The design showed the basic card first and
   replaced it with the summary. In use, a long card followed by a short one was noisy. In
   LLM mode the basic card is now the fallback: shown at once if the CLI fails or returns
   nothing usable, or after `recap.llm_timeout_seconds` if there is no answer. Summary
   lines are cut at a word boundary with an ellipsis at 120 characters, and the prompt asks
   for lines under 100. `Input::SummaryFailed` tells the core a run failed so the fallback
   does not wait for the timeout. LLM mode is still off by default.

5. **Which Claude Code surfaces work.** Hooks are Claude Code's, not the terminal's. The
   hooks reference says Claude Code fires the same events in terminals, IDE extensions and
   the Desktop app; the VS Code page says the extension shares `~/.claude/settings.json`
   with the CLI, and it installs in Cursor and other VS Code forks. So the receiver should need no
   change per surface. Only the terminal has been tested; the Desktop app in particular has
   user reports of events not firing, and the hooks page does not mention JetBrains. What each surface needs is its app in the home-app list. `focusd`
   now prints a note, with the exact config line, when a prompt arrives from an app that
   is not a home app. Cursor's own agent, Copilot and other agents have different hook
   systems and remain out of scope. Cloud sessions fire no local hooks.

6. **Returns go to the app, not the window.** On macOS, activation is per app, so with
   several windows of one editor the user lands in whichever window the OS raises. Exact
   window return (backlog C5) is not built.

7. **Front-app changes are polled every 500 ms.** The macOS adapter has no change
   notification yet. The core also probes on its own schedule, so this only affects how
   quickly a manual app switch is noticed.

8. **Activation is verified, never assumed.** The shell reads the front app after
   `verify_after` and reports whether it is the target. An early version always reported
   success, which would have hidden a refusal by the OS.

9. **A shared shell loop.** `focus-runner` holds the loop that feeds the core and carries
   out effects, so `focusd` (real macOS adapter) and `focus-headless` (fake platform) run the
   same code. Dependencies of the new crates are listed in `DEPENDENCIES.md`.

10. **`focusctl` uninstall restores the original bytes.** Re-serialising `settings.json`
    can change its formatting. `uninstall-hooks` therefore restores the first backup's exact
    bytes when its content matches what the uninstall produced, and otherwise writes the
    re-serialised file. The first backup is kept, never overwritten.
