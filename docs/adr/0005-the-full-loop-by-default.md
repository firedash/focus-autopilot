# ADR 0005: The full loop by default

Date: 2026-11-17. Status: accepted, not yet run on a Mac. Where this record and
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

3. **Not from another home window.** If the user is in a window they prompt Claude from
   (another terminal, an editor) when a different session finishes, they get the prompt, not
   a countdown. Someone working with one Claude session is asked, not moved. This kept an
   existing rule: sessions waiting at different windows are returned to one at a time, and
   the second by prompt.

4. **The next away switch goes to where the user left off.** When the user arrives at a home
   window from another app while Claude is working or waiting for them, whether focusd
   brought them or they came themselves, the core remembers that app. The next away switch
   goes there first, if it is still running and is not a home app; otherwise to the first
   running app in `away.apps`; otherwise nowhere. `away.resume_last_app` (default on) turns
   this off, and then only `away.apps` is used. Arriving at a home window with nothing going
   on is not coming back to Claude and changes nothing, so the first prompt of a sitting goes
   to the configured away app, as the owner asked.

5. **So with no settings at all, the app now switches the user away.** Not on the first
   prompt, when there is nowhere to go, but from the second on. This reverses the manual-mode
   default on purpose. The away guards are unchanged: the run must have gone on for
   `away_after_seconds` (30), the user must not have touched the keyboard or mouse for
   `away_requires_idle_seconds` (8), the terminal must still be in front, and a countdown
   with "Not now" comes first. To get manual mode back, turn off both settings.

6. **The remembered app is kept in memory only.** It is an app id and name, the same thing
   the core already holds for the front app. It is not written to disk and is gone when
   `focusd` stops.

7. **It is the app, not the window.** macOS activation without the Accessibility permission
   brings an app forward, not a chosen window; the app's own most recent window comes up.

8. **Not verified on a Mac.** The return countdown and the switch back were seen working on
   a Mac from a configured away app (ADR 0003). The same code now runs from any app, and the
   switch to a remembered app uses the same activation call, but neither has been run there.
   `focus-core/tests/full_loop.rs` and `focus-runner/tests/shell_loop.rs` pin the behaviour
   against the fake platform.
