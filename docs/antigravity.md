# Antigravity: completion and input return

Experimental implementation. The goal is simple: **when Antigravity finishes a task or explicitly asks for input, bring the Antigravity app forward**. Its helpers may continue working. This does not try to reconstruct the full Claude prompt/away/return loop.

## Behavior

- Native `Stop` requests an app-level return. `fullyIdle` is not a gate. If it is not true, the basic recap says other work may still be running.
- `PreToolUse` for `ask_question` or `ask_permission` requests the same return, using the existing input-needed debounce. A request that resolves immediately should not interrupt.
- The existing countdown, typing guard, snooze, pause, manual-return setting and OS activation verification still apply. If Antigravity is already in front, nothing is shown.
- A fresh OS running-apps snapshot resolves the exact display name `Antigravity`. No bundle ID/executable name is guessed, and whichever unrelated app was foreground at task start is irrelevant.
- Multiple windows of the same app identity are app-level: the currently foreground matching window is preferred, otherwise an OS-enumerated matching window is used. This is **not** a promise to open the exact conversation or originating window. Conflicting app identities with the same name fail closed.
- Normal stop reasons are `model_stop` and the observed `NO_TOOL_CALL`. Explicit errors and `max_steps_exceeded` bring attention back with failure wording, never success. Recognized cancellation/interruption reasons are quiet and withdraw pending attention. Unknown reasons without an error are also quiet. The official docs do not enumerate every possible cancellation reason, so real-session verification is required. Internally these quiet stops clear session tracking; the next event adopts the same conversation again.
- Duplicate Stop while still completed is silent. Further model/tool activity does not cancel an unhandled completion return just because helpers continued. Activity after the return is handled permits the next completion.
- SessionStart and PreInvocation are liveness only. They do not invent user prompts or capture home windows. Automatic switching away after the next prompt is not added for Antigravity.
- There is no transcript reader or invented answer summary. Recaps show a plain status; Antigravity does not invoke the optional Claude LLM recap.

## Install from this branch

Released v0.1.1 binaries do not include these commands. Build this branch, then use the built `target/release/focusctl` (or `target\release\focusctl.exe` on Windows):

```text
cargo build --release -p focusctl -p focusd
focusctl install-antigravity-hooks --dry-run
focusctl install-antigravity-hooks
```

Keep the executable in that location: the manifest records its absolute path. Start `focusd` yourself if it is not running; this installer does not start it. Restart Antigravity and inspect **... > Customizations > Hooks** in the IDE agent panel. Test a simple task while another app is foreground.

The default manifest is `~/.gemini/config/hooks.json`. `--scope project` selects `.agents/hooks.json`; for Antigravity, `--scope local` is an alias of project, not a separate local-only file. `--settings FILE` selects a manifest. `--config FILE` selects Focus Autopilot's config and is recorded as an absolute path.

If the OS exposes a different display name, set it in Focus Autopilot's `config.toml`:

```toml
[antigravity]
app_name = "the exact name reported by your OS"
```

`focusd` reads this itself, the way it reads its other settings: a hook cannot name the app, so
nothing a hook sends can choose which window comes forward ([ADR 0007](adr/0007-antigravity-attention-returns.md)).
The default is `Antigravity`. `focusd` applies a hand edit of `config.toml` within a couple of
seconds like its other settings; that has been tested in the core and not on a desktop. Name
lookup is exact apart from letter case. No match logs `attention_app_not_running`; conflicting
identities log `attention_app_ambiguous`. The app is never launched if it is absent, and an
unrelated foreground window is never used as a fallback.

```text
focusctl uninstall-antigravity-hooks --dry-run
focusctl uninstall-antigravity-hooks
```

The generated named `focus-autopilot` bundle is merged, not substituted for the whole manifest. Install is idempotent, preserves unrelated hooks and the bundle's enabled flag, and refuses a conflicting bundle. Uninstall removes only the recognized generated bundle. The first backup is retained beside the manifest with `.focus-autopilot.bak` appended.

## Input-needed coverage

**Covered:** explicit `ask_question` and `ask_permission` tools visible to PreToolUse.

**Not established:** an implicit permission dialog raised by an ordinary tool, or any other UI-only input request. The documented event list has no general PermissionRequest or Notification event. A PreToolUse notification is not proof that an ordinary tool will need approval, so this implementation does not bring the app forward on every tool call.

This limitation must be checked in a real Antigravity session; it is not silently treated as complete approval-prompt coverage.

## Privacy and failure behavior

The bridge uses standard-library TCP to the existing authenticated loopback receiver, with bounded input, short connection/read/write timeouts, and valid passive JSON stdout even on failure. It never changes tool arguments, expands permissions, injects instructions or forces execution to continue. PreToolUse returns `allow`, which delegates to subsequent hooks/default policy, not `auto_approve`.

The token is read from the existing private config on every invocation, never embedded in hooks.json or process arguments. Rotation needs no Antigravity manifest rewrite; restart focusd after rotating its token as usual. Commands and paths pass through the receiver's existing redaction, truncation and text cleaning. Raw prompts, transcripts and artifacts are not read or persisted by this adapter. Multiple workspaces are not silently assigned to an arbitrary first folder.

On Windows, generated hook commands support spaces but reject paths containing quotes, `%` or `!` rather than relying on ambiguous shell expansion. Actual Windows command-hook execution remains a real-machine check.

## Tests and review handoff

The branch includes adapter unit tests, real command-process/loopback receiver integration tests, and simulated core tests for completion, continuing helpers, input-needed debounce, duplicate Stop, unknown/ambiguous app names, provider labels, no Claude LLM recap, already-in-app silence and snooze. Existing Claude regression tests remain in place.

Before treating this as supported on a desktop:

1. Verify hook delivery and command quoting on the actual Antigravity edition/version/OS. Confirm its exact OS display name.
2. Test completion while helpers continue, repeated tasks in one conversation, a new conversation, a cancelled/error task, explicit questions and permission requests, and an ordinary tool's implicit approval dialog.
3. Inspect actual countdown/recap pixels, typing guard, snooze, and OS activation on Mac/Windows. Fixtures and compilation are not real desktop verification.
4. Check the desired app-level behavior with multiple windows. Exact conversation navigation is outside this implementation.
5. Review continuation/dedup behavior. The bridge does not equate model calls with user turns, and it does not wait for every subagent.

## Sources checked 20 November 2026

- Current official hook schema and IDE setup: https://antigravity.google/docs/hooks?tab=ide . The old IDE URL redirects here. The live page includes SessionStart and lastUserInput, which older extracted versions may omit.
- Official hooks and background-task context: https://antigravity.google/blog/google-io-2026-feature-deep-dive/
- Official plugin packaging: https://antigravity.google/docs/plugins/
- Practitioner payloads, including NO_TOOL_CALL: https://atamel.dev/posts/2026/07-16_where_agy_hooks/ . Its old no-UI statement is superseded by the current official docs.
- Community normalization/manifest example: https://github.com/pbakaus/impeccable/pull/1433 . Implementation evidence, not an official API contract.
- Secondary runtime and notification cautions: https://anotifier.io/guides/antigravity-cli-notifications/
