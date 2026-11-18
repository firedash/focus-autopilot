# ADR 0005: The full loop by default

Date: 2026-11-17. Status: accepted; the return has run on a Mac and on Windows, the move
back once on a Mac. Where this record and
`docs/design.md` or ADR 0003 item 1 disagree, this record wins.

ADR 0003 item 1 made manual mode the default: nothing moved the user, and a prompt waited for
the focus key. After using that, the owner described the loop they want instead:

1. Prompt Claude from a terminal.
2. After a while, be moved to another app.
3. Work there.
4. When Claude is done or needs input, a sound and a box.
5. A 5 second countdown, then the terminal comes forward.
6. After the next prompt, be moved back to wherever the steady work was left off, whichever
   app that is, not necessarily a configured away app.

These are the choices that implement it.

1. **Returns happen by themselves, after a countdown, from any app.** `switching.auto_return`
   (default on) makes every return a countdown followed by the switch, wherever the user is.
   Before, a countdown was used only from a configured away app, and everywhere else a prompt
   waited. Off, every return is the prompt and the focus key. The countdown is
   `countdown.return_seconds`, 5 by default, separate from the away countdown
   (`countdown.seconds`, 3). A length of 0 is refused: every automatic switch is announced.

2. **The safeguards of the away-app return apply unchanged.** The focus key snoozes a
   countdown (`countdown.snooze_seconds`, `countdown.max_snoozes`). When the countdown ends
   the switch waits for a moment without keyboard or mouse input, and if input continues for
   `switching.input_quiet_max_seconds` a prompt replaces the switch, so a keystroke does not
   land in the terminal. A switch the OS refuses falls back to the prompt. A run that ends
   within `switching.debounce_seconds` of an away switch gets the prompt, so the user is not
   flipped straight back.

3. **Not from a window with another Claude session in it.** If the user is in an app that
   some other session was prompted from when this one finishes, they get the prompt, not a
   countdown. Someone working with one Claude session is asked, not moved. This kept an
   existing rule: sessions waiting at different windows are returned to one at a time, and
   the second by prompt. The first version of this rule asked in any app on the home-app
   list, and on its first run on a Mac the owner, working in such an app with no session in
   it, got "Go back?" where a countdown was expected. An editor or the Claude app with no
   Claude Code session in it is now treated like any other app, for the return and as a place
   to be sent back to. When the user is asked and not moved, the log says why
   (`asked_because_...`).

4. **The next away switch goes to where the user left off.** When the user arrives at a home
   window from another app while Claude is working or waiting for them, whether focusd
   brought them or they came themselves, the core remembers that app. The next away switch
   goes there first, if it is still running and has no Claude session in it; otherwise to the first
   running app in `away.apps`; otherwise nowhere. `away.resume_last_app` (default on) turns
   this off, and then only `away.apps` is used. Arriving at a home window with nothing going
   on is not coming back to Claude and changes nothing, so the first prompt of a sitting goes
   to the configured away app, as the owner asked.

5. **So with no settings at all, the app now switches the user away.** Not on the first
   prompt, when there is nowhere to go, but from the second on. This reverses the manual-mode
   default on purpose. The away guards: the run must have gone on for `away_after_seconds`
   (30), the terminal must still be in front, and a countdown with "Not now" comes first. To
   get manual mode back, turn off both settings.

   **No idle time is required before the away countdown** (`away_requires_idle_seconds` is
   now 0 by default; it was 8). On a Mac the owner found that even 3 seconds without input
   was often not met, so the switch did not come. The safeguard that matters is kept: after
   the countdown the switch still waits for a pause in the typing
   (`switching.input_quiet_ms`), and after 5 seconds of continuous input it becomes a
   question. One thing the idle requirement did by accident is now done on purpose: when the
   user comes back to the home window from another app while the run is going, no away
   switch starts for 8 seconds, so coming back to look at Claude is not answered by being
   moved out again three seconds later. The first version restarted the whole away threshold
   instead, and the owner then saw no away switch at all on runs that took a while: any
   detour through another app, a screenshot tool included, cost up to 45 seconds again.

   A moment with nothing in front on the way back, up to 5 seconds, does not lose the app.
   The shell prints where the user left off and, while an away switch is held back, which
   guard is holding it (`not switching away: ...`), because "why didn't it switch" was the
   first question on a Mac and the guards are otherwise silent.

6. **The remembered app is kept in memory only.** It is an app id and name, the same thing
   the core already holds for the front app. It is not written to disk and is gone when
   `focusd` stops.

7. **It is the app, not the window.** macOS activation without the Accessibility permission
   brings an app forward, not a chosen window; the app's own most recent window comes up.

8. **Partly verified on a desktop.** The return countdown and the switch back were seen
   working on a Mac from a configured away app (ADR 0003), and on 2026-11-17, after the fix
   in item 3, the owner saw the automatic return work from another app, first on a Mac and
   later that day on Windows. The move to a remembered app was seen at least once on the
   Mac, while a 3 second idle requirement was still set; it has not been re-run since the
   idle requirement was dropped and the hold-off after coming back was fixed. On Windows it
   was recorded working on 2026-11-18 (`docs/images/demo.gif`): the away countdown, the
   switch to the browser the user had been in, and the return. The focus key snoozed a countdown in a real run on Windows.
   One more rule came out of that run: a sub-agent's end reported just after Claude's own
   Stop cancelled the return, because any event within a minute of a Stop reopened the run.
   Only an event that shows Claude working or asking again reopens it now.
   `focus-core/tests/full_loop.rs` and `focus-runner/tests/shell_loop.rs` pin the behaviour
   against the fake platform.
