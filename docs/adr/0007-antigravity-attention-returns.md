<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR 0007: Antigravity attention returns (experimental)

Date: 2026-11-20. Status: experimental; built and tested against a fake platform on Linux, and
its bridge process tests run on Linux, macOS and Windows CI. No person has run it with a real
Antigravity. Where this record and `docs/design.md` disagree, this record wins.

Antigravity, Google's coding-agent IDE, has command hooks. This is the first provider other than
Claude Code. The goal is deliberately small: when Antigravity finishes a task or explicitly asks
for input, bring its app forward. It does not rebuild Claude's prompt, away and return loop.
Details and the install steps are in [docs/antigravity.md](../antigravity.md).

1. **Command hooks, through `focusctl antigravity-hook`.** Antigravity runs a shell command
   per event and supports no HTTP handler, so each hook starts `focusctl`, which turns the
   documented fields into the event shape `focusd` already accepts and posts it to the same
   authenticated loopback receiver. The token is read from `config.toml` on each call and is
   never written into Antigravity's hook file or a command line.

2. **App-level, not window-level.** There is no user-prompt event, so no home window is
   captured. The app is found at finish time from a fresh running-apps snapshot; several
   windows of one app are treated as one, and two apps that share a display name but not an
   identity are refused. This promises the app, not the conversation or the window.

3. **`focusd` names the app, the hook does not.** The display name comes from
   `[antigravity] app_name` in `focusd`'s own config (default `Antigravity`). An earlier draft
   let the hook payload carry the name; that let anything holding the token pick a window and
   made the documented override a config key `focusd` rejected at startup. It is now a known
   setting, validated, and a payload field that names an app is ignored.

4. **New event kinds, not borrowed Claude ones.** `AgentActivity` is liveness (session start,
   model call), never a user submission. `AppFinished` and `AppNeedsInput` are the two
   requests for attention. The agent is told apart by a `session_id` that starts with
   `antigravity:`; the wording on boxes and recaps uses the agent's name. An explicit agent
   field on the event would be sturdier and is left for when there is a third provider.

5. **Stops.** `model_stop` and `NO_TOOL_CALL` are normal finishes; an error, a non-empty
   `error` field or `max_steps_exceeded` is shown as a failure. Recognised cancellation
   reasons, and unknown reasons with no error, are quiet and clear the session. `fullyIdle` is
   not a gate: other work may still be running, and the recap says so. Helpers continuing, and
   a repeated Stop, do not withdraw or repeat a return that has not been handled.

6. **Input requests are only the explicit tools.** `ask_question` and `ask_permission` seen
   before a tool runs. There is no general permission or notification event, so an approval
   dialog raised by an ordinary tool is not covered, and a call to an ordinary tool is not
   treated as a request.

7. **Passive replies.** The command always exits 0 and prints a valid reply: `allow` for
   tool calls (delegate to the next hook or the default policy, not `auto_approve`), `stop`
   for Stop, an empty object otherwise. It never edits arguments, injects steps, continues the
   agent or reads a transcript. A failure prints one generic line to stderr.

8. **Installation is merge-only.** `install-antigravity-hooks` adds one named bundle,
   `focus-autopilot`, to `~/.gemini/config/hooks.json` (or `.agents/hooks.json`), with dry run
   and a backup. It refuses a bundle of that name it did not write, and uninstall removes only
   the bundle it recognises by shape.

9. **Consequences seen in the code.** An Antigravity run is recorded in `runs.jsonl` and
   counts as "Claude working" time in the statistics. Every tool call and model call starts a
   `focusctl` process, and with `focusd` stopped each waits up to 400 ms to connect; that cost
   is expected and has not been measured. When an Antigravity finish and a Claude finish are
   both waiting, the user is taken to Antigravity first and Claude's return then asks
   ("Go back?"), because the window the user is in hosts a session.

**Not established, and to check first in a real session:** that installing the hook leaves
Antigravity's approval prompts as they were; what Antigravity does when the hook command is
missing or fails; which shell runs the command on Windows (the path is quoted for `cmd`);
whether the hooks fire at all on each edition (a forum report says `Stop` did not on a Windows
IDE build); the real termination reasons; and the OS display name of the app.
