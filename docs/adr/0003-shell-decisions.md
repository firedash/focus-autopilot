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

11. **Secrets are redacted at the receiver and on the card.** The first version redacted only
    the text sent for the LLM recap, so the on-screen card showed a key typed into a prompt in
    full: a problem on a screen share. The receiver now redacts prompts, commands, messages
    and errors before they are stored, reading 128 characters past the truncation point so a
    key that straddles the cut is seen whole; the core redacts the card again as defence in
    depth. Redaction is a best-effort pattern filter (known key prefixes, `name=value` pairs
    for secret-like names, bearer tokens, PEM blocks), not a guarantee.

12. **Toasts and the recap name the project.** The last folder of the session's `cwd` appears
    on every toast ("Claude is done in api. Go back?"), on the recap card as `Project:`, and
    in the `Also:` lines for other waiting sessions (item 17). With several sessions in one
    app and no exact-window return, the key still lands in the app, not the session; the name
    at least says which one is waiting.

13. **Run history is written and read.** `runs.jsonl` gets one metadata line per finished
    run, in the design's format and with mode 0600; it is loaded at startup into the
    estimator, and lines past `history.retention_days`, and unreadable ones, are dropped.
    It holds no prompt text, command or path; the project is a salted hash.

14. **Logs hold names, not text.** When `focusd`'s output is a terminal it prints prompts,
    toasts and recaps for debugging. When it is a file, as under a launch agent, it prints
    event names and outcomes only. `--verbose` forces the full output.

15. **Launch at login is a plist `focusctl` writes.** `install-agent` writes
    `~/Library/LaunchAgents/io.github.firedash.focusautopilot.plist` and prints the
    `launchctl` command that loads it; it does not load it itself. The `PATH` at install time
    is saved in the plist, because a launch agent has none of the shell's, and `claude` for
    LLM recaps must stay findable. There is still no tray icon or installer.

16. **Exact-window return stays unbuilt, and is not a two-way choice.** An earlier version of
    this record said macOS offers only the Accessibility permission or launching the editor's
    command line. That framing was too narrow, and it missed the real blocker: the hook says
    which *project* a session is in, not which *window*, so any route needs a way to tie a
    session to a window. The routes, none of them built:
    - **Editors:** open `vscode://file/<project folder>` (and `cursor://...`) through the
      normal macOS open-URL call. No permission and no spawned process. Checked by hand on a
      Mac with one folder open in VS Code: the URL brought that window forward. Built (item
      21), opt-in.
    - **Terminals:** carry the terminal's own session id in the hook. Anthropic's hooks page
      documents `allowedEnvVars` for HTTP hooks, which lets a header take a value from an
      environment variable, so a header such as `X-Focus-Term: $ITERM_SESSION_ID` is plausible
      (also `TERM_SESSION_ID`, `TMUX_PANE`, `KITTY_WINDOW_ID`). Unverified in practice. With
      the id, iTerm2 and Terminal.app can be told to select that session through Apple
      Events, which costs a one-time per-app Automation prompt, milder than Accessibility.
    - **Accessibility:** a later opt-in catch-all (design item C5), or never; on its own it
      does not say which window belongs to a session.
    Spawning a process is not a principled objection (the LLM recap already runs `claude`);
    the cost of the terminal route is per-app code and a soft permission prompt, against the
    "asks for no OS permission" pitch. Order if built: editor URL route first, terminals as an
    opt-in second.

17. **A finished session's result is never reduced to a name.** Returning to an app settles
    every session waiting in that app, because with no window identity the core cannot tell
    which window the user is looking at. An earlier build then showed the others only as
    `Also waiting: web`, so a session that finished while the user was away could vanish with
    its result unseen. The card now has one line per other waiting session with its outcome
    (`Also:    web - Header fixed.`, or what it needs), two lines at most and then a count, and
    the lines are kept under an LLM summary too. Stepping through one card per session was
    the alternative; the lines were chosen because they lose nothing and need no new
    interaction.

18. **Same folder name, different projects.** When two live sessions are in different
    folders with the same last name, both are labelled `parent/name` (`team-a/api`,
    `team-b/api`); otherwise the short name is used.

19. **Redaction keeps the quotes.** Redacting a value now leaves the quotes and punctuation
    around it, so `'Authorization: Bearer x'` becomes `'Authorization: Bearer [redacted]'`
    instead of swallowing the closing quote. Known gaps, accepted as best-effort: a secret in
    plain prose (`my password is hunter2xyz`) and attached short flags (`mysql -pSecret`), the
    latter because `-p` is a common harmless flag.

20. **The hotkey has a second delivery path and a self-test.** Carbon hotkey presses are
    delivered only when something services the event queue. Besides `AppKit`'s event
    dispatch, the shell now also pulls pending Carbon events and sends them to the dispatcher.
    `focusd --test-hotkeys` registers the configured chords and prints each press for 60
    seconds, so "the key does nothing" can be separated from the rest of the app. Neither has
    been run on a Mac.

21. **Editor return by project URL, opt-in.** With `[return] open_project_in_editor = true`,
    a return to VS Code, VS Code Insiders or Cursor opens `<scheme>://file/<percent-encoded
    project folder>` through `NSWorkspace.openURL` instead of only activating the app. The
    core attaches the session's folder to the activation for returns only (never for an away
    switch), and the shell falls back to ordinary activation if the option is off, the app is
    not such an editor, or the request fails. The activation is still verified afterwards.
    Off by default because the URL cannot tell the editor to focus a window rather than open a
    folder: when Claude ran in a *subfolder* of an open workspace, the editor may open a new
    window for it. Only editors whose behaviour has been checked are in the scheme table
    (`editor_url.rs`). Terminals still return to the app, not the window (item 16).

22. **Config differences from the design.** The design says an unknown key in `config.toml`
    produces a warning and is ignored, and that the file is re-read when it changes. As built,
    an unknown key or a wrongly typed value is an error that stops `focusd` from starting, so a
    typo cannot leave a setting silently at its default, and the file is read once at startup:
    changes need a restart. The design's behaviour may return with the tray, which has
    somewhere to show a warning.
