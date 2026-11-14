# ADR 0001: Choices made in focus-core where the design left room

Date: 2026-11-14. Status: accepted, open to revision with real traces.

The design's sections 5 and 7 fix the contract and the rules. These are the points it didn't
settle, and what the core does.

1. **`Core::new` takes the built-in home-app list.** The design's signature is
   `Core::new(config)`. The built-in list lives in `focus-platform`, which depends on
   `focus-core`, so the shell passes it in: `Core::new(config, known_home_apps)`.

2. **Who verifies a switch.** `Effect::Activate` carries `verify_after`. The shell reads the
   frontmost app that much later and reports `Observation::Activation { verified }`. If no
   report arrives within `verify_after` plus 2 s, the core treats the switch as refused.

3. **Idle time is polled only when a decision needs it.** The core emits `Probe(Idle)` once a
   second while an away switch is armed or a session at the home window awaits an answer, and
   every 100 ms during the input-quiet wait. Otherwise it asks for nothing.

4. **Home and Away are one controller mode.** The design's Home and Away states behave the
   same for return triggers, so the controller has one idle mode plus a record of whether
   focusd put the user in an away app. That record drives the "came back on your own" recap
   (after 20 s or more away) and the 5 s debounce.

5. **The away fallback prompt uses up the run's away switch.** Otherwise a prompt that times
   out after 30 s would be followed by another countdown a second later.

6. **A second Stop for a finished run.** A Stop with `stop_hook_active` true arriving in DONE
   is treated as "Claude continued, then stopped again": the run ends in DONE with the new
   message, and a fresh return trigger fires if the user is away.

7. **Events for a closed run after 60 s, or after the stale timeout,** start a new run with
   no home window. The session is tracked but never switches, as the design requires for
   wake-ups.

8. **The resume heuristic counts input only while the home window is in front.** Input in a
   game can't answer a permission prompt, so it must not clear NEEDS_INPUT and withdraw the
   return prompt.

9. **Returning settles every waiting session.** The MVP returns for the first waiting session
   (needs-input before done, then oldest) and lists the others on the recap card. After that
   return the others are considered handled; there is no second toast. V1's queue (H1)
   replaces this.

10. **The 5 s debounce applies to Stop only,** as backlog item D5 says. A permission prompt
    right after an away switch still gets its 2 s debounce and a countdown.

11. **Paused means no toasts at all.** Sessions keep updating. A session that finished during
    the pause triggers its return when automation resumes, if the user is still away.

12. **Normalized events carry only what the MVP decides on.** `permission_mode`, `effort` and
    `agent_id` are in the payload table for V1 history and estimation; `AgentEvent` doesn't
    carry them yet.

## Known risk to check with real traces

A `PreToolUse` moves a session from NEEDS_INPUT to RUNNING (section 7). If Claude Code fires
`PreToolUse` for a second, parallel tool call after the first call's `PermissionRequest`, the
session would look like it's running while a prompt is still open. The backup `Notification`
hook should put it back, but confirm the event order with `focusctl record` before M3.
