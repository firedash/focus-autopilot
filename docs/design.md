# Focus Autopilot — Feature Backlog & Technical Design (v1: Claude Code on macOS and Windows)

Nov 14, 2026 · firedash

## 1. Overview

Focus Autopilot (working name) is a local background app that moves a developer between intermittent prompting work with a coding agent and steady work in an app they choose, driven by the agent's state. v1 supports Claude Code only, on macOS and Windows: when a prompt runs long enough, it switches you to your chosen away app; when Claude finishes or needs permission, it switches you back and shows a short recap of where you left off.

**Problem.** Prompt-and-wait coding leaves idle gaps that vary from seconds to 20 minutes. Pomodoro timers don't fit that rhythm, and moving to other work means either checking back constantly or forgetting to return. Existing tools cover only half the loop: they bring you back, or they entertain you while you wait, but none does both with real apps and a recap (see section 3).

**Target user.** A developer running Claude Code who wants to use wait time for steady work of their own choosing, such as writing, reading, another project or a break, without forgetting to return.

**v1 goals**

- Detect when a Claude Code turn starts, needs input, and ends, using Claude Code hooks.
- Switch focus to a configured away app on long runs, and back to the coding window on completion or permission prompts.
- Warn before every automatic switch with a short countdown and an easy snooze, so keystrokes never land in the wrong window.
- Show a recap card on return: what you asked, what Claude did, and what needs your decision.
- Estimate wait length from elapsed time and local history, and use it to decide whether to switch at all.
- Run on macOS and Windows from one codebase: a shared core with thin per-OS adapters.
- Earn trust as an open-source project: small, readable, few dependencies, and nothing leaves the machine by default (section 13).

**Non-goals for v1**

- Support for agents other than Claude Code (Codex, Cursor, Antigravity are later).
- Linux support (planned for V2).
- Pausing or controlling the away app, or any app-specific integration.
- Auto-approving permission prompts or answering Claude's questions.
- Reading keystrokes, screen contents or Claude Code transcripts.
- Claude Code sessions running in the cloud, which fire no local hooks.
- Cloud sync, accounts, analytics or multi-machine use.
- Mobile apps.

**Supported platforms.** macOS 14 (Sonoma) or later, as one universal build for Apple silicon and Intel; Windows 10 22H2 and Windows 11 on x64. Claude Code: any version whose hooks reference documents HTTP hooks and `prompt_id`; the README records the tested version and `focusctl doctor` checks it. English only in v1.

**Terms**

| Term | Meaning |
| --- | --- |
| Session | One Claude Code conversation, identified by `session_id` |
| Run | One prompt and everything Claude does until it stops, identified by `prompt_id` |
| Home window | The terminal, IDE or Claude app window the prompt was sent from |
| Away app | An app the user chose for steady work during waits; what counts as steady work is up to them |
| Switch | An automatic focus change, always announced by a countdown toast |
| Fallback prompt | The one-click or one-key toast shown when the OS refuses a switch or an automatic one would be unsafe |
| Focus key | The hotkey that acts on whichever toast is showing |
| focusd | The always-running app (product name Focus Autopilot) |
| focusctl | The command-line tool for hooks, diagnostics, recording and audits |

## 2. Working model and product principles

The unit of work is the dispatch, wait, review loop, not a clock block. Focused attention belongs at the two ends: writing a sharp prompt or spec, and reviewing the result. The wait in the middle should be spent according to how long it is.

**Wait-time tiers (defaults, user-configurable)**

| Tier | Expected wait | Behavior |
| --- | --- | --- |
| Short | under 30 s | Stay put. Leaving costs more than it saves. |
| Medium | 30 s to about 5 min | Switch to the away app, or stay for same-context micro-tasks (skim last diff, draft next prompt). |
| Long | over about 5 min | Real break: switch to the away app and suggest a break. |

**Principles**

1. **Never surprise the user.** Every automatic switch is preceded by a visible countdown (default 3 s) that a click or the focus key cancels or snoozes. This is the biggest risk, because people already complain about apps stealing focus.
2. **React to elapsed time first, predict second.** Agents give no ETA, so v1 decides from elapsed time and refines with history and live tool events (section 9).
3. **Steady work must be easy to leave.** Work that can be put down at any moment fits best (reading, writing, review, planning). Anything that punishes interruption, such as a live call or a real-time game, does not. The app warns before switching into an app marked as immersive.
4. **Return with context.** Coming back should take seconds: a recap card names the last prompt, what Claude did, and any decision needed.
5. **Stay out of the agent's way.** Hooks must be fast and fail-safe: if the app is down, Claude Code works exactly as before.
6. **The user is always in control.** The tray and one global hotkey pause all automation.
7. **Only manage a context the user is in.** Switch away only from the home window while the user is idle; switch back automatically only from an away app. Anywhere else, show the fallback prompt and let the user decide.
8. **Observe as little as possible.** focusd reads the frontmost app, global idle time and hook events. It never reads keystrokes, screen contents or transcripts.

## 3. Market snapshot

No tool found runs the full loop: switch away on prompt, switch back on completion, with a recap, for real apps. Existing tools cover pieces of it. Findings come from web search results as of 2026-11-13 and were not exhaustively verified.

| Tool | Category | What it does | Gap vs Focus Autopilot |
| --- | --- | --- | --- |
| [Claude Focus](https://marketplace.visualstudio.com/items?itemName=evileye.claude-focus) | Focus back | VS Code extension that brings the window to the front when Claude Code needs attention or finishes | VS Code only; no away-switch, no recap |
| [Hook recipe, issue #53126](https://github.com/anthropics/claude-code/issues/53126) | Focus back | Windows hooks on Stop and PermissionRequest that beep and focus VS Code | Manual script; one direction |
| [code-notify-hook](https://www.pkgstats.com/pkg:@poli.digital/code-notify-hook) | Focus back | Notifications with click-to-focus on the exact IDE or terminal window | Needs a click; no away-switch |
| [claude-notifications-go](https://newreleases.io/project/github/777genius/claude-notifications-go/release/v1.40.0) | Focus back | Windows notifications can bring the originating terminal into focus | One direction; no recap |
| [agent-arcade](https://github.com/Kuldeep-045/agent-arcade) | Waiting games | Terminal games that pause and beep when the agent finishes | Games live in the terminal, not your real apps |
| [Agent Arcade overlay](https://danwahlin.github.io/agent-arcade/) | Waiting games | Transparent desktop overlay platformer | Built-in games only |
| [Backgrind](https://backgrind.com/play/minecraft/) | Overlay | Always-on-top agent window over borderless-fullscreen games, chimes on decision or finish | Puts the agent over the game; does not switch or recap |
| [Claude Code Agent View](https://pasqualepillitteri.it/en/news/2384/claude-code-agent-view-cli-dashboard-sessions-2026) | Session dashboard | Lists all sessions by state | No focus automation |
| [AgentManager](https://chatgate.ai/post/agentmanager) | Session dashboard | Floating macOS panel for Claude Code sessions | Monitoring only |

**Implications.** Claude Code's terminal already shows its own recap when you leave a session and return ([ClaudeDevs](https://x.com/ClaudeDevs/status/2046613932518023259)), so the recap should be cross-app and agent-agnostic. Users also file issues asking to stop focus stealing ([#94460](https://github.com/anthropics/claude-code/issues/94460)), so the countdown and snooze are product requirements, not polish.

## 4. Feature backlog

The MVP closes the loop on macOS and Windows: hooks in, state machines with guards, switch away and back with a countdown and fallback, and a basic recap. V1 adds estimation, the LLM recap, multi-session queueing and exact-window return on macOS. V2 and later widen agents and platforms.

| ID | Epic | Feature | Priority | Acceptance criteria |
| --- | --- | --- | --- | --- |
| A1 | Hook ingestion | `focusctl install-hooks` and `uninstall-hooks` manage the hooks listed in section 6 in the user's settings.json | MVP | `--dry-run` prints the diff; backs up, merges and writes atomically; writes through symlinks; refuses files that aren't strict JSON; a repeated install changes nothing; uninstall leaves everything else byte for byte |
| A2 | Hook ingestion | Hardened local event receiver on 127.0.0.1 | MVP | Replies 200 within 10 ms; rejects a wrong token, a foreign Host header, any Origin header, non-JSON bodies and bodies over 8 MB; a stopped app never blocks Claude Code |
| A3 | Hook ingestion | `focusctl record` writes scrubbed traces for test fixtures | MVP | String values become same-length placeholders; field names, structure and timing are kept; output is safe to commit |
| A4 | Hook ingestion | `focusctl rotate-token` | MVP | Config and hooks get the new token in one step; the old token is rejected at once |
| B1 | Session state | Per-session state machine and the global switch controller (section 7) | MVP | Replayed traces end in the expected states, including interrupts, permission prompts, questions, Stop-hook continuations and two overlapping sessions |
| B2 | Session state | Resume heuristic after a needs-input pause (section 7) | MVP | An approve-then-long-command trace returns to RUNNING after input and 8 s idle; an unanswered prompt never triggers an away switch |
| C1 | Platform | macOS and Windows adapters for frontmost app, running apps, idle time, activation, hotkeys, overlay and sound | MVP | Every call works on both OSes without an OS permission prompt; activation results are verified correctly in 9 of 10 manual attempts |
| C2 | Platform | Home window captured at prompt submit (section 8) | MVP | Prompts from each built-in home app record that app, and on Windows the exact window; an unknown app is ignored until added as a home app |
| C3 | Platform | Ordered away-app list; the first running app is used | MVP | With none running, no away switch happens and the tray says why; the away and home lists can't overlap |
| C4 | Platform | Immersive flag for away apps | V1 | Flagged apps get a 5 s countdown with a warning |
| C5 | Platform | Exact-window return on macOS with the optional Accessibility permission | V1 | With permission, returns to the window the prompt came from; without it, app-level return works and no permission prompt appears unless the user enables the option |
| C6 | Platform | Opt-in forceful activation on Windows | Later | Built only if the M2 spike shows most switches are refused; off by default and documented in PERMISSIONS.md |
| D1 | Switch policy | Away switch after the threshold, with the guards in section 7 | MVP | After 30 s with no Stop, the user in the home window and idle 8 s: countdown, then the away app, or the fallback prompt if the OS refuses |
| D2 | Switch policy | Return on Stop, a permission prompt or a question | MVP | From an away app: countdown, then the home window and recap; from any other app: the fallback prompt; nothing if the user is already home |
| D3 | Switch policy | Countdown toast with cancel and snooze | MVP | A click or the focus key cancels an away switch for the rest of the run, or snoozes a return for 60 s, at most 3 times; the toast never takes keyboard focus |
| D4 | Switch policy | Pause | MVP | The tray offers 30 min, 1 h and until resumed, and a hotkey toggles it; while paused, state still updates but nothing switches |
| D5 | Switch policy | Debounce rules (section 7) | MVP | A Stop within 5 s of an away switch shows the fallback prompt instead of switching; a needs-input that clears within 2 s does nothing |
| D6 | Switch policy | Input-quiet guard | MVP | A switch waits for 600 ms without keyboard or mouse input, up to 5 s; if input continues, the fallback prompt appears instead |
| D7 | Switch policy | Fallback prompt when the OS refuses a switch | MVP | Refusal is detected within 400 ms; one click or the focus key completes the switch on both OSes |
| D8 | Switch policy | Sound on return | MVP | A system sound, on by default and configurable, plays even when the toast can't be seen |
| D9 | Switch policy | Suggest an away activity by tier | V2 | The suggestion appears in the countdown |
| D10 | Switch policy | Optional pause integration for away apps | Later | Per-app opt-in; never sends keystrokes by default |
| F1 | Recap | Basic recap card: prompt, start of the last message, files touched, what needs a decision | MVP | Appears within 1 s of returning; never takes focus; renders all text as plain text; closes after 10 s or soon after the user starts typing |
| F2 | Recap | LLM recap through the local Claude Code CLI (section 10) | V1 | Off by default; replaces the basic text within 20 s or not at all; runs with no tools and no hooks; never reads the transcript |
| F3 | Recap | Run statistics on the card: duration, tool calls, sub-agents, background tasks still running | V1 | Counts match the replayed trace |
| E1 | Estimation | Append each run's metadata to a local JSON Lines file | V1 | One line per run with the fields in section 11; no prompt text, paths or commands |
| E2 | Estimation | History-based estimate chooses the tier | V1 | With 5 or more similar runs, the predicted tier matches the actual one for most replayed runs; otherwise elapsed time decides |
| E3 | Estimation | Live signals raise the estimate | V1 | A replayed trace with a sub-agent start or a test command moves the run to the Long tier |
| E4 | Estimation | Declare-long hotkey | V1 | Pressing it during a run sets the Long tier and starts the away switch |
| E5 | Estimation | Learned model beyond the median | Later | Beats the median on held-out runs |
| H1 | Multi-session | Queue returns from several sessions | V1 | Two sessions finishing close together return one at a time, needs-input first; the card lists the rest |
| G1 | Settings | TOML config with documented defaults and live reload | MVP | Starts with no config file; an invalid edit keeps the last good config and shows the error in the tray |
| G2 | Settings | First-run wizard (section 8) | MVP | The user finishes with hooks installed, an away app chosen and one test switch seen |
| G3 | Settings | Launch at login | MVP | Off until enabled in the wizard or tray; uses the OS login-item mechanism |
| G4 | Settings | `focusctl doctor` | MVP | Checks hooks, receiver, port, hotkeys, idle-time access, Claude Code version and events seen, with a fix for each failure |
| G5 | Settings | Ignore projects by folder pattern | V1 | Events from matching folders are ignored entirely |
| G6 | Settings | Away-app profiles by project or time of day | V2 | The profile is chosen from the project or schedule |
| I1 | Insights | Local dashboard: time waiting, time away, time to return | V2 | Reads only the local runs file; no network calls |
| O1 | Open source | Repository files from the first public commit: LICENSE, SECURITY.md, CONTRIBUTING.md, CODE\_OF\_CONDUCT.md, CODEOWNERS, THREAT\_MODEL.md, PERMISSIONS.md, DEPENDENCIES.md, AUDIT.md | MVP | All files exist and are filled in; AUDIT.md names every listener, file written, process spawned and OS API that observes the user |
| O2 | Open source | CI on every pull request (section 13) | MVP | Main is branch-protected; a pull request can't merge unless every check passes |
| O3 | Security | Negative tests and fuzz targets for the receiver and the settings.json merge | MVP | Forged and malformed requests are rejected in tests; fuzz targets run in CI without crashes |
| O4 | Security | No telemetry and no outbound network except the opt-in LLM recap | MVP | The Linux integration test fails on any non-loopback connection or any write outside the allowed paths |
| O5 | Open source | Signed releases with checksums, SBOM and build provenance | V1 | macOS opens the app without Gatekeeper overrides; Windows binaries are signed once SignPath approval arrives; unsigned binaries rebuild from the tag with matching checksums |
| J1 | Other agents | Codex adapter (turn-complete notify) | V2 | The same state machine runs on a different event source |
| J2 | Other agents | Adapters for Cursor, Antigravity, Gemini CLI | V2 | One adapter per agent behind the common event schema |
| J3 | Platforms | Linux (X11 first, Wayland best effort) | V2 | Same acceptance as C1 on Linux |

## 5. Architecture

One local desktop process ("focusd") receives Claude Code hook events over HTTP, tracks each session's state, decides when to switch, moves focus through OS adapters, and shows a countdown and a recap. Nothing leaves the machine except the optional recap call to the Anthropic API.

&#91;embedded content: architecture · 8 components in one local process\]

Hook events enter on the left. The policy engine is the only part that decides, and the window controller is the only part that touches other apps.

**Proposed v1 stack (confirm in section 15):** Rust, with a Tauri 2 shell for the tray, global hotkeys and the small UI. One codebase covers macOS and Windows, Rust rules out whole classes of memory bugs in code that parses outside input, and Cargo's tooling makes the dependency tree checkable. The workspace is split so an auditor can read the important parts first:

- `focus-core`: every decision (state machines, guards, estimator, recap builder). No I/O, no OS calls, no `unsafe`.
- `focus-receiver`: the HTTP receiver and payload parsing into normalized events. No UI and no OS calls, so it runs headless in tests.
- `focus-platform`: the per-OS adapters plus a fake used in tests. The only crate allowed `unsafe`, each block with a written safety justification.
- `focus-app` and `focusctl`: the thin Tauri shell (wiring, overlay, tray, wizard, store) and the command-line tool (hooks, token, recording, doctor, audit, test switch).
- `focus-headless`: a dev-only runner that wires the receiver and core to the fake platform for the Linux integration test.

Electron, the earlier proposal, was dropped because its Chromium and npm dependency tree is too large to audit. A native app per OS would behave best but doubles the UI work. Tauri 2.8 and later can create non-focusable windows ([Tauri 2.8 release notes](https://v2.tauri.app/release/tauri-runtime/v2.8.0/)), but floating one over a full-screen game without taking focus must be proven on both OSes in milestone M2. If it falls short, the overlay moves into `focus-platform` as a small native window (a non-activating panel on macOS, a no-activate topmost window on Windows).

**Components**

| Component | Responsibility |
| --- | --- |
| Hook client | Claude Code's own HTTP hooks POST each event's JSON to focusd; no script or shim |
| Event receiver | HTTP server on 127.0.0.1 that checks each request, replies 200 at once, parses only known fields and queues a normalized event |
| Session manager | One state machine per `session_id` (section 7) |
| Policy engine | The global switch controller: guards, countdowns, fallback prompts, debounce and pause (section 7) |
| Estimator | Wait tier and range from history and live signals (section 9) |
| Window controller | The platform layer: frontmost app, running apps, idle time, activation, hotkeys, overlay windows and sound, one adapter per OS (section 8) |
| Overlay UI | Non-focusable toast and recap card, tray menu, first-run wizard |
| Recap service | Builds the recap from the run's events, with an optional LLM summary (section 10) |
| Store | TOML config, JSON Lines run history (V1) and rotating logs; no database (section 11) |

**Data flow.** Claude Code fires a hook, the event receiver acknowledges it, the session manager updates state, the policy engine asks the estimator and then the window controller to act, and the overlay UI shows the countdown and recap.

**Core contract.** `focus-core` is a deterministic state machine: inputs in, effects out, no I/O. The shell owns every side effect, so each decision is testable with recorded traces and a fake clock, and the code an auditor must trust sits in one place.

```rust
pub enum Input {
    Hook(AgentEvent),       // normalized Claude Code payload (section 6)
    Observed(Observation),  // frontmost app changed, idle seconds, switch verified or refused
    User(UserAction),       // toast click, focus key, pause, declare-long, wizard test
    Config(Config),
}

pub enum Effect {
    ShowToast(Toast),       // countdown or fallback prompt (section 8)
    HideToast,
    Activate(AppRef),       // the shell verifies after verify_after_ms and reports back
    Probe(ProbeKind),       // ask the shell for idle time or the frontmost app now
    ShowRecap(RecapCard),
    PlaySound(Sound),
    AppendRun(RunRecord),   // V1
    Log(LogEvent),          // event names, ids and outcomes only
}

impl Core {
    pub fn new(config: Config) -> Self;
    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect>;
    pub fn tick(&mut self, now: Instant) -> Vec<Effect>;
    pub fn next_deadline(&self) -> Option<Instant>;
}
```

The shell runs the core on one thread, sleeps until the next input or `next_deadline()`, and executes effects in order. Calls that touch windows (activation, overlay, hotkeys) run on the main thread, as AppKit requires. The platform layer is one trait with a macOS implementation, a Windows implementation and a fake for tests:

```rust
pub trait Platform {
    fn frontmost(&self) -> Result<AppRef>;              // app id, pid, window handle on Windows
    fn watch_frontmost(&self, tx: Sender<AppRef>);      // OS change notifications, no polling
    fn running_apps(&self) -> Result<Vec<AppInfo>>;
    fn seconds_since_input(&self) -> Result<f64>;       // global idle time, never input content
    fn activate(&self, target: &AppRef) -> Result<()>;  // a request; the core verifies it
    fn play_sound(&self, sound: Sound);
}
```

**Starter dependencies.** Each still needs a DEPENDENCIES.md entry; anything else needs a written reason.

| Crate | Used for | Note |
| --- | --- | --- |
| tauri 2.8 or later, tauri-plugin-global-shortcut | Shell, tray, webview UI, hotkeys | 2.8 added non-focusable windows; no shell, fs, http or updater plugins |
| hyper 1.x with hyper-util and http-body-util | Local receiver | Runs on the async runtime Tauri already ships; no web framework |
| serde, serde\_json with preserve\_order, toml | Payloads, settings.json merge, config |  |
| subtle, getrandom | Constant-time token check, token generation |  |
| sha2 | Project keys in run history (V1) |  |
| tracing, tracing-subscriber, tracing-appender | Rotating logs |  |
| objc2 and its AppKit, Foundation, CoreGraphics and ServiceManagement crates | macOS APIs | Typed bindings, only in focus-platform |
| windows | Win32 APIs | Microsoft's official bindings, only in focus-platform |

Not used: an HTTP client, telemetry or crash reporting, an auto-updater, a database, Node.js or npm packages.

## 6. Claude Code integration

Claude Code gives hooks no ETA, progress, duration or token counts, so focusd works from event timing and a few payload fields. Every event below supports the `http` hook type; `SessionStart` doesn't, which is why focusd doesn't use it ([hooks reference](https://code.claude.com/docs/en/hooks), read 2026-11-14).

**Hooks to register**

| Event | Matcher | focusd uses it to | Fields read |
| --- | --- | --- | --- |
| UserPromptSubmit | none | Start a run and capture the home window | `permission_mode`, `prompt` |
| PreToolUse | all tools | Count tools, spot slow commands, questions and touched files, resume after a pause | `tool_name`, `agent_id`, `effort.level`; from `tool_input` only `command` (Bash, PowerShell) and `file_path` (Edit, Write) |
| PermissionRequest | all tools | Enter needs-input and return | `tool_name`, a one-line summary of `tool_input` |
| PermissionDenied | all tools | Leave needs-input | `tool_name` |
| Notification | permission\_prompt, elicitation\_dialog, elicitation\_url\_dialog | Backup needs-input signal | `notification_type` |
| Elicitation | all MCP servers | Enter needs-input: an MCP server is asking the user | `mcp_server_name` |
| SubagentStart, SubagentStop | all agent types | Count sub-agents and raise the estimate | `agent_id`, `agent_type` |
| Stop | none | End the run; return and recap | `stop_hook_active`, `last_assistant_message`, the number of `background_tasks` |
| StopFailure | all errors | End the run with an error | `error` |
| SessionEnd | all reasons | Forget the session | `reason` |

Every event also carries `session_id`, `prompt_id`, `cwd` and `hook_event_name`. If `prompt_id` is missing, as on an older Claude Code, the event belongs to the session's current run.

**Questions count as needs-input.** A PreToolUse for `AskUserQuestion` (Claude asks a question) or `ExitPlanMode` (Claude wants a plan approved) means Claude is waiting for the user, so it's handled like a permission prompt.

**Deliberately not registered.** PostToolUse and PostToolUseFailure carry full tool output, such as file contents and test logs, that focusd doesn't need, and they fire after a tool finishes, which is too late to help. Stop doesn't fire when the user interrupts with Esc, so section 7 handles interrupts with idle and staleness rules.

**Payload handling.** The receiver deserializes only the fields above into typed structs and skips the rest unparsed. It accepts bodies up to 8 MB, because a Write's `tool_input` and a long `last_assistant_message` can be large; bigger bodies get 413 and are logged as dropped. Kept strings are truncated (prompt 500 characters, message 1,500, command 200, path 300) and stripped of control characters and ANSI escapes. Payloads carry no timestamp, so focusd stamps each event with its own monotonic receive time.

**Hook entries.** Every event uses the same receiver URL; the event name comes from `hook_event_name` in the body. A minimal pair:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [ { "type": "http", "url": "http://127.0.0.1:47821/hook",
                     "timeout": 2, "headers": { "X-Focus-Token": "<token>" } } ] }
    ],
    "Notification": [
      { "matcher": "permission_prompt|elicitation_dialog|elicitation_url_dialog",
        "hooks": [ { "type": "http", "url": "http://127.0.0.1:47821/hook",
                     "timeout": 2, "headers": { "X-Focus-Token": "<token>" } } ] }
    ]
  }
}
```

HTTP hooks always run synchronously (only command hooks can be async), so the receiver answers immediately with `200` and an empty body, which Claude Code treats as success with no decision. `timeout: 2` caps the delay if focusd ever hangs.

**Token.** The installer writes a random 256-bit token into each entry, so nothing depends on environment variables. It stops web pages and other users' processes from posting events. It doesn't stop software already running as the user, which can read both files (section 13). Developers often publish their dotfiles, so `focusctl rotate-token` replaces the token in config and hooks in one step.

**Installer rules (focusctl)**

- Target the user-level `~/.claude/settings.json` (on Windows `%USERPROFILE%\.claude\settings.json`) so every project is covered. `--scope project` or `--scope local` (V1) writes the project files instead.
- `--dry-run` prints the exact diff. A real run backs up to `settings.json.focus-autopilot.bak`, merges, writes a temporary file in the same folder, then renames it over the original.
- If the file is a symlink, as dotfile managers create, write to its target and say so; never replace the link with a regular file.
- If the file isn't strict JSON (for example it has comments), stop with an error and change nothing.
- Keep key order and the file's indentation; add focusd's entries beside existing hooks, never replace them. focusd's entries are the ones whose URL starts with `http://127.0.0.1:<port>/hook`, so uninstall removes only those.
- If the file changes between read and rename, start over, at most 3 times.
- The wizard runs `focusctl install-hooks`, showing the dry-run diff first, so the always-running app never edits another tool's settings itself.

**Things that stop hooks firing.** In interactive sessions, Claude Code holds back all hooks, including user-level ones, until the folder's workspace trust dialog is accepted. Sessions started before install need a restart. Organizations can restrict HTTP hook URLs with `allowedHttpHookUrls`. `focusctl doctor` explains each case when no events arrive.

**When focusd isn't running**, Claude Code treats each failed hook as a non-blocking error. Milestone M4 checks how visible that is; if it's noisy, Quit offers "Quit and remove hooks" and the wizard re-adds them.

**WSL.** If Claude Code runs inside WSL2 on Windows, 127.0.0.1 may not reach the Windows host unless WSL's mirrored networking mode is on. `focusctl` detects WSL and prints the setup steps.

**First fixture.** A synthetic trace in the recorded format. `_t_ms` is the receive time in milliseconds since the trace started; it isn't part of Claude's payload.

```json
{"_t_ms":0,"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"p1","cwd":"/work/app","permission_mode":"default","prompt":"Run the tests and fix failures"}
{"_t_ms":4100,"hook_event_name":"PreToolUse","session_id":"s1","prompt_id":"p1","cwd":"/work/app","tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"npm test"},"effort":{"level":"high"}}
{"_t_ms":4200,"hook_event_name":"PermissionRequest","session_id":"s1","prompt_id":"p1","cwd":"/work/app","tool_name":"Bash","tool_input":{"command":"npm test"}}
{"_t_ms":95000,"hook_event_name":"PreToolUse","session_id":"s1","prompt_id":"p1","cwd":"/work/app","tool_name":"Edit","tool_use_id":"t2","tool_input":{"file_path":"/work/app/src/sum.ts"}}
{"_t_ms":131000,"hook_event_name":"Stop","session_id":"s1","prompt_id":"p1","cwd":"/work/app","stop_hook_active":false,"last_assistant_message":"Fixed the failing sum test; all 42 tests pass.","background_tasks":[]}
```

## 7. Session state machine and policy rules

Each Claude Code session has four states: IDLE, RUNNING, NEEDS\_INPUT and DONE. Events move a session between them; the policy engine reacts to those moves.

&#91;embedded content: state machine · 4 states, 6 transitions\]

A new prompt always starts a run. A permission prompt or a Stop moves the session into a state that triggers the return to the coding window.

**Session transitions**

| Event | From | To | Side effects |
| --- | --- | --- | --- |
| UserPromptSubmit | any | RUNNING | New run: `prompt_id`, start time, prompt (memory only); home = the frontmost window if it's a home app; counters and flags reset |
| PreToolUse, SubagentStart, SubagentStop | RUNNING | RUNNING | Update counters, touched files and the estimate |
| PermissionRequest, Elicitation, a needs-input Notification, or PreToolUse for AskUserQuestion or ExitPlanMode | RUNNING | NEEDS\_INPUT | Note what's needed (memory only); return trigger |
| The next PreToolUse, PermissionDenied or SubagentStart for the same run | NEEDS\_INPUT | RUNNING | Clear the note |
| Resume heuristic: user input after entering NEEDS\_INPUT, then 8 s idle, and still no agent event | NEEDS\_INPUT | RUNNING | Most likely a long tool was approved; away guards apply again |
| Stop | RUNNING or NEEDS\_INPUT | DONE | Close the run; return trigger; recap |
| StopFailure | RUNNING or NEEDS\_INPUT | DONE (error) | Return trigger; the recap shows the error type |
| Any event for the same run within 60 s of Stop, or Stop with `stop_hook_active` true | DONE | RUNNING | Another Stop hook made Claude continue; no second away switch in this run |
| No agent event for 600 s (`stale_after_seconds`) | RUNNING | IDLE | Close the run quietly, with no return and no recap; covers interrupts and crashes |
| SessionEnd | any | removed | Drop the session |
| First event from an unknown session (focusd started mid-run) | none | RUNNING | No home window, so focusd never switches for this run |
| An event whose `prompt_id` isn't the current run's | any | unchanged | Ignore a late event from an earlier run |

**Switch controller.** Focus is global, so one controller owns every switch, however many sessions run. It shows at most one toast at a time.

| State | Toast | Leaves when |
| --- | --- | --- |
| Home | none | All away guards pass → Away countdown. A return trigger → handled as in Away |
| Away countdown | Switching to the away app in 3 s | Countdown ends and input is quiet → activate, then Away if verified or Away prompt if refused. Not now → Home, with no more away switches this run. The run ends or the user leaves the home window → Home |
| Away prompt | Claude will take a while. Switch? | Click or focus key → activate → Away. The run ends, the user leaves the home window, or 30 s pass → Home |
| Away | none | Return trigger with an away app in front → Return countdown; with another app in front → Return prompt. The user comes back on their own → Home, with the recap if away 20 s or more |
| Return countdown | Back to Claude in 3 s | Countdown ends and input is quiet → activate, then Home with the recap if verified, or Return prompt if refused. Snooze → Snoozed |
| Snoozed | none | 60 s pass → Return countdown, at most 3 times, then Return prompt. The user comes back → Home with the recap |
| Return prompt | Claude is done. Go back? (stays until acted on) | Click or focus key → activate → Home with the recap. The user comes back → Home with the recap |

**Away guards** (all must hold):

1. A run is RUNNING with a home window, and no away switch has happened or been cancelled in it.
2. Elapsed time has reached the tier threshold (section 9).
3. The home window is frontmost (on Windows, that exact window).
4. The user has been idle for 8 s (`away_requires_idle_seconds`).
5. An away app is running, automation isn't paused, and no session is waiting on the user.

**Return trigger.** When a run with a home window enters NEEDS\_INPUT or DONE: with an away app in front, whoever opened it, start the return countdown; with the home window in front, do nothing; with any other app in front, show the return prompt. Every return plays the sound.

**Debounce and safety**

- A Stop within 5 s of an away switch shows the return prompt instead of a countdown, so the user isn't flipped straight back.
- A NEEDS\_INPUT that clears within 2 s is ignored, because a hook or auto mode approved it.
- Every countdown rechecks its conditions when it ends. The input-quiet guard then waits for 600 ms without keyboard or mouse input, up to 5 s, before any activation; if input continues, the prompt replaces the switch.

**Several sessions.** Away guard 5 covers every session. Return triggers queue: needs-input before done, then oldest first. The MVP returns for the first and lists the others on the recap card; V1 adds a proper queue (H1).

**Known limits.** An interrupted run looks like one that's still thinking until the user sends a new prompt or 600 s pass. The idle guard and a cancelled countdown keep that from causing a wrong switch. Background tasks reported at Stop may wake the session later; a wake-up with no home window never switches.

## 8. Window switching, overlay and UI

Every platform call goes through one interface (section 5) with a macOS and a Windows adapter; Linux follows in V2. None of the calls below triggers an OS permission prompt, and PERMISSIONS.md states that as a promise. Both OSes treat a background app's request to change focus as something they may refuse, so the core verifies every switch.

**Platform calls**

| Need | macOS 14 and later | Windows 10 and 11 |
| --- | --- | --- |
| Frontmost app and window | `NSWorkspace.frontmostApplication`; changes from `NSWorkspaceDidActivateApplicationNotification` | `GetForegroundWindow`; changes from `SetWinEventHook` with `EVENT_SYSTEM_FOREGROUND` |
| Running apps, for the wizard and away list | `NSWorkspace.runningApplications`, regular apps only | Visible top-level windows from `EnumWindows` and their executables |
| Global idle time | `CGEventSourceSecondsSinceLastEventType` on the combined session state, the call Chromium uses for idle detection | `GetLastInputInfo` |
| Activate an app | `NSRunningApplication.activate`; when focusd is the active app (after a toast click), first `NSApp.yieldActivation(to:)`, then `activate(from:)` | `ShowWindow(SW_RESTORE)` if minimized, then `SetForegroundWindow` on the saved window |
| Hotkeys | Carbon `RegisterEventHotKey` | `RegisterHotKey` |
| Overlay | Non-activating `NSPanel` above normal windows, joining all Spaces and full-screen apps | Topmost tool window with `WS_EX_NOACTIVATE`, shown without activating |
| Sound | `NSSound` system sound | `PlaySound` with a system sound alias |
| Launch at login | `SMAppService.mainApp`; macOS shows its login-item notice | A value under the user's `CurrentVersion\Run` registry key |

**Why switches get refused.** Since macOS 14, activation is a request the active app may decline, and a background app's request can fail silently ([Apple: cooperative activation](https://developer.apple.com/documentation/appkit/passing-control-from-one-app-to-another-with-cooperative-activation)). Windows lets a process take the foreground only under conditions such as being the foreground process or having received the last input event, and may refuse even then ([Microsoft: SetForegroundWindow](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setforegroundwindow)). A click on focusd's toast gives it that standing on both OSes. Whether the focus key does too is tested in M2.

**Verify every switch.** 400 ms after an activation request (`verify_after_ms`), the shell reads the frontmost app and reports the result. If the target isn't in front, the controller shows the fallback prompt (section 7). On some setups most switches will take one keypress; that is a normal flow, not an error.

**Never used:** Apple Events (an Automation prompt per target app), event taps or low-level keyboard hooks (Input Monitoring), window titles or screen capture (Screen Recording), Accessibility (except the opt-in feature C5), thread-input attachment, synthesized keystrokes, and changes to the foreground-lock timeout. If Windows refuses most switches in testing, backlog item C6 may add an opt-in forceful mode, off by default.

**Capturing the home window.** At UserPromptSubmit the user has just pressed Enter, so focusd reads the frontmost app. If it's a home app, it becomes the run's home. On Windows the exact window handle is saved. On macOS, activating the app brings back its most recent window, normally the one the prompt came from; exact-window return needs Accessibility and is the opt-in V1 feature C5. If the frontmost app isn't a home app (a script, a phone, the browser), the run has no home and focusd never switches for it.

Built-in home apps live in `focus-platform/src/known_apps.rs` as bundle ids and executable names, extended by `home.extra_apps`:

- macOS: Terminal, iTerm2, Ghostty, WezTerm, kitty, Alacritty, Warp, VS Code, VS Code Insiders, Cursor, Windsurf, Zed, JetBrains IDEs, Claude.
- Windows: Windows Terminal, Console Host, VS Code, Cursor, Windsurf, JetBrains IDEs, Claude.

**Toasts.** One small overlay near the top-right corner of the display that holds the frontmost window, or else the display with the pointer. It never takes keyboard focus and renders text as plain text. The focus key always does the thing that won't happen on its own.

| Toast | Example text | Click or focus key |
| --- | --- | --- |
| Away countdown | Claude will take a while. Switching to Obsidian in 3 s. | Not now: no away switch this run |
| Away prompt | Claude will take a while. Switch to Obsidian? | Switch now |
| Return countdown | Claude is done. Back to your terminal in 3 s. | Snooze 60 s |
| Return prompt | Claude needs your approval. Go back? | Go back now |

Default focus key: Control+Option+Command+Space on macOS, because Control+Option+Space already switches input sources there, and Ctrl+Alt+Space on Windows. It's registered only while a toast is visible. Default pause hotkey: Control+Option+Command+P on macOS and Ctrl+Alt+Shift+P on Windows. Both are configurable and can be turned off; if one can't be registered, the tray and doctor say so.

**Recap card.** Shown after a return, or when the user comes back after 20 s or more away. Same corner, never focusable, plain text, at most 6 lines:

```text
Asked:   Run the tests and fix failures
Claude:  Fixed the failing sum test; all 42 tests pass.
Touched: src/sum.ts
4 min · 23 tool calls · 1 sub-agent        (V1)
```

For a needs-input return, the second line reads "Needs: approval to run npm test" or "Needs: an answer to Claude's question". The card closes after 10 s, on a click, or 3 s after the user starts typing (idle time resets).

**Tray menu.** A status line (Idle, Claude running 0:42, Away, Paused until 15:30); Pause for 30 min, 1 h or until resumed; Away apps (choose from running apps, reorder); Open config; Reload config; Run doctor; Open logs folder; Clear history (V1); Remove hooks; Quit, which offers "Quit and remove hooks".

**First-run wizard.** The only focusable window:

1. What focusd does and never does: no keystrokes, no screen reading, no network.
2. Install hooks: show the `focusctl install-hooks --dry-run` diff, apply on confirm.
3. Choose away apps from the running apps, with advice to run any full-screen app, such as a game, borderless windowed.
4. Test: a real away switch and return with countdowns; if the OS refuses, the wizard shows the one-key prompt working.
5. Launch at login (off by default) and sound on return (on by default).
6. Done: restart open Claude Code sessions, then send any prompt.

**Display limits.** Overlays and clean switching need borderless-windowed mode. Exclusive full-screen games may flash, minimize or ignore the switch, and may hide the toast; the sound still plays.

## 9. Wait-time estimation

Claude Code gives no ETA, so v1 starts with elapsed time only and adds estimates in layers. Each layer is optional: if it has no data, the one before it decides. The output is a tier (Short, Medium, Long) and a range, never a single confident number.

**Layer 0: elapsed time (MVP).** No estimate. Switch away when elapsed time reaches `awayAfterSeconds` (default 30 s).

**Layer 1: history (V1).** Log every run (section 11). Group runs by project key (a salted hash of the folder), `permission_mode` and prompt length (under 200 characters, 200 to 1,000, over 1,000). The estimate is the median duration of the last 20 runs in the group, with the 25th to 75th percentile as the range. Use it only with 5 or more samples; otherwise use the median of all runs with the same prompt length, then Layer 0.

**Layer 2: live signals (V1).** While the run is going, raise a floor under the estimate when these events appear. The values are starting guesses to tune from real logs.

| Signal | Estimate floor |
| --- | --- |
| SubagentStart | 300 s |
| PreToolUse of Bash or PowerShell whose command starts with a test, build or install tool (for example pytest, npm, pnpm, yarn, cargo, go test, gradle, mvn, dotnet, xcodebuild, docker build, make) | 120 s |
| More than 15 tool calls so far | 180 s |

**Layer 3: user override (V1).** A hotkey during RUNNING marks the run as Long.

**How the tier changes behavior**

| Estimated tier | Away switch happens |
| --- | --- |
| Short (under 30 s) | Not before 45 s elapsed (grace), in case the estimate is wrong |
| Unknown | At 30 s elapsed |
| Medium or Long | At 10 s elapsed |

All away guards in section 7 still apply, so the idle guard usually adds a few seconds. Log the predicted tier and the actual duration for every run so later layers can be compared against the baseline. Don't ask the model how long it will take: models estimate their own runtime poorly.

## 10. Recap service

The recap card answers three questions in a few lines: what did I ask, what did Claude do, and what does it need from me. It's built from hook payloads only; focusd never reads the transcript.

**Inputs** (memory only, discarded when the next run starts or the card closes):

- The prompt, first 500 characters.
- Touched files from Edit and Write calls (up to 5 shown) and commands run (first 200 characters, up to 3).
- For needs-input: the tool name and a one-line summary of what awaits approval, or that Claude asked a question.
- From Stop: `last_assistant_message` (first 1,500 characters) and the number of background tasks still running. From StopFailure: the error type.

**Basic mode (MVP, no network).** Layout in section 8. Text is set with `textContent`, never parsed as HTML or Markdown, because prompts and agent output can contain markup.

**LLM mode (V1, opt-in).** focusd runs the user's own Claude Code CLI, so there's no API key to store and the text only goes to the provider the user already uses ([CLI reference](https://code.claude.com/docs/en/cli-reference)):

```text
claude -p --model haiku --tools "" --strict-mcp-config
          --settings '{"disableAllHooks": true}' --no-session-persistence
          --max-turns 1 --output-format text
```

- Spawned directly with an argument array, never through a shell, with an empty folder in focusd's data directory as the working directory and the input on stdin.
- `--tools ""` removes all built-in tools; `--strict-mcp-config` without `--mcp-config` loads no MCP servers; `disableAllHooks` stops the recap session from firing focusd's own hooks.
- 20 s timeout. The basic card shows first and is replaced only if the summary arrives in time.
- `focusctl doctor` runs `claude auth status` before the option can be turned on.

Recap prompt, sent on stdin:

```text
Summarize this coding-agent run for a developer returning from a break.
Reply with at most three short lines of plain text, no markdown:
Asked: <what the developer asked>
Did: <what the agent did>
Needs: <the decision the developer must make, or "nothing">
Use only the facts below. Do not guess.
---
<prompt, tool summary, last message>
```

**Privacy rules**

- Send only the truncated fields above, never file contents or the transcript.
- Before sending, redact likely secrets: common API-key formats, `password=` and `token=` pairs, and PEM blocks.
- Off by default. The wizard explains that summaries count toward the user's Claude usage.

## 11. Config and data model

Everything is local: one TOML config, one JSON Lines history file (V1) and rotating logs. Every option has a default, so the app runs with no config file. No prompt text, messages, commands, paths or tokens are ever written to history or logs.

**Files**

| File | macOS | Windows | Contents |
| --- | --- | --- | --- |
| config.toml | `~/Library/Application Support/<bundle id>` | `%APPDATA%\<bundle id>` | The settings below, including the hook token |
| runs.jsonl (V1) | same folder | same folder | One line per run, metadata only; trimmed to `history.retention_days` once a day |
| logs | `~/Library/Logs/<bundle id>` | `%LOCALAPPDATA%\<bundle id>\logs` | Daily files, 7 kept; event names, ids and outcomes only |
| recap-workdir (V1) | data folder | data folder | Empty working directory for LLM recaps |

The bundle id is `io.github.<owner>.focusautopilot`. On macOS the data folder is created with mode 0700 and its files 0600; `%APPDATA%` is already private to the user on Windows. `FOCUS_AUTOPILOT_HOME` overrides the data folder for tests.

**config.toml with every default**

```toml
# Focus Autopilot. Delete a line to return to its default.
enabled = true

[receiver]
port = 47821                  # must match the installed hooks; rerun focusctl install-hooks after changing
token = "<written by focusctl install-hooks>"

[home]
extra_apps = []               # bundle ids (macOS) or executable names (Windows) treated as home apps

[away]
apps = []                     # ordered; the first running app wins, e.g. ["Obsidian.exe"]
immersive = []                # subset of apps that get a longer countdown with a warning (V1)

[switching]
away_after_seconds = 30
away_after_seconds_when_long = 10
short_grace_seconds = 45
away_requires_idle_seconds = 8
input_quiet_ms = 600
input_quiet_max_seconds = 5
debounce_seconds = 5
needs_input_debounce_seconds = 2
resume_idle_seconds = 8
stale_after_seconds = 600
return_on_needs_input = true
verify_after_ms = 400

[countdown]
seconds = 3
snooze_seconds = 60
max_snoozes = 3
away_prompt_seconds = 30

[hotkeys]                     # "" turns a hotkey off; defaults differ by OS
focus_key = "Control+Option+Command+Space"   # Windows: "Ctrl+Alt+Space"
pause = "Control+Option+Command+P"           # Windows: "Ctrl+Alt+Shift+P"
declare_long = ""                            # V1

[ui]
sound_on_return = true
recap_seconds = 10
launch_at_login = false

[recap]
mode = "basic"                # "basic" or "llm" (V1)
llm_timeout_seconds = 20

[history]                     # V1
retention_days = 90
salt = "<random, written on first run>"

[ignore]                      # V1
cwd_globs = []
```

Unknown keys produce a warning in the tray and log and are ignored. An invalid value on reload keeps the last good config and shows the error; on first start the default is used for that key. focusd checks the file's modification time every 2 s, so no file-watching dependency is needed.

**runs.jsonl line (V1)**

```json
{"v":1,"run_id":"<prompt_id>","project":"<first 16 hex of SHA-256(salt + cwd)>","start":"2026-11-14T08:31:02Z","duration_s":184,"outcome":"done","permission_mode":"default","effort":"high","prompt_len":"medium","tools":23,"subagents":1,"slow_commands":2,"needed_input":true,"predicted_tier":"long","away":"auto","return":"prompt","snoozes":0}
```

`outcome` is done, error or stale; `away` is none, auto, prompt, cancelled or manual; `return` is none, auto, prompt or manual. The per-install salt means project keys can't be matched across machines.

## 12. Risks, edge cases and mitigations

The highest risk is that automatic switching annoys the user or endangers their work. The countdown, the input-quiet guard, the frontmost-app rules and pause are the main defenses, and all are in the MVP.

| Risk or edge case | Mitigation |
| --- | --- |
| Focus changes while the user is typing, so keystrokes land in the wrong window (users already report this with Claude's own app: [#94460](https://github.com/anthropics/claude-code/issues/94460), [#102066](https://github.com/anthropics/claude-code/issues/102066)) | Countdown before every switch; input-quiet guard; non-focusable toast; cancel, snooze and pause |
| A permission prompt gets an accidental answer after a focus jump | Countdown and input-quiet guard; focusd never sends keys |
| The user interrupts with Esc and Claude Code sends no Stop | Away switch needs 8 s idle in the home window; a cancelled countdown ends away switches for the run; the stale timeout closes the run |
| A long command runs after a permission approval with no events | The resume heuristic (input, then 8 s idle) re-arms the away switch |
| The OS refuses automatic switches (macOS cooperative activation, Windows foreground rules, elevated target apps on Windows) | Verify after 400 ms; one-click or one-key fallback prompt; never bypass OS protections by default |
| Hooks don't fire: sessions started before install, an untrusted workspace, managed policy | The wizard and doctor explain each case; the tray shows "no events seen yet" |
| A hook slows or blocks Claude Code | The receiver answers in milliseconds; `timeout: 2`; no vetoing |
| Failed hooks are noisy while focusd isn't running | Check visibility in M4; Quit offers "Quit and remove hooks" |
| The recap run triggers focusd's own hooks | `disableAllHooks` through `--settings`, plus a dedicated working directory |
| Untrusted text (prompts, agent output, commands) rendered in the UI | Plain text only, strict CSP, no HTML or Markdown rendering |
| Secrets in prompts or output reach the recap model | Opt-in, truncation, redaction, no file contents |
| settings.json is a symlink or has comments | Write through the link; refuse files that aren't strict JSON |
| The token is published with someone's dotfiles | Low impact (loopback events only); `focusctl rotate-token` |
| Global hotkeys clash with the OS, IDEs or games | Three-modifier defaults; the focus key is registered only while a toast shows; configurable; doctor reports failures |
| The macOS idle-time call needs a permission on some version | Checked in the M2 spike; if so, idle guards fall back to time-only rules there |
| Exclusive full-screen games ignore the switch or hide the toast | Recommend borderless windowed; sound on return; warn for immersive apps |
| The Tauri overlay takes focus or can't float over games | M2 spike; native overlay fallback in focus-platform |
| Several monitors or Spaces | Activate apps rather than move windows; show the toast on the frontmost window's display |
| Two sessions finish together | One switch at a time; queue; the card lists the rest |
| Short runs cause flip-flopping | Short-tier grace period; a Stop within 5 s of an away switch gets the prompt, not an automatic return |
| Claude Code inside WSL2 can't reach focusd | focusctl detects WSL and prints setup steps, such as turning on mirrored networking |
| Port 47821 is already in use | Startup error in the tray; change `receiver.port` and rerun `focusctl install-hooks` |
| Hook payloads change between Claude Code versions | Typed parsing that ignores unknown fields; a missing `prompt_id` falls back to the current run; tested version range in the README |
| A public repo attracts malicious pull requests or dependency attacks | CODEOWNERS, required reviews, dependency policy and pinned CI actions (section 13) |
| Anthropic adds native focus or recap features | Stay agent-agnostic and cross-app; the away-and-back loop is the differentiator |

## 13. Open source, security and code standards

Users will let this app watch their coding agent and move their focus, so trust is the product. These rules keep the code small enough for a stranger to audit in an afternoon, and they apply from the first commit. Numbers are targets to hold, not measurements.

**Trust principles**

1. **Small.** MVP target: under 6,000 lines of first-party Rust and UI code, excluding tests. Security-relevant code (receiver, installer, platform adapters) lives in clearly named files.
2. **Few dependencies, each justified.** DEPENDENCIES.md lists every direct dependency with its purpose, license and why the standard library is not enough. CI fails when a dependency is added without an entry.
3. **Local by default.** No telemetry, no crash upload, no self-updater. The only network traffic is loopback and the opt-in recap call.
4. **Least privilege, stated openly.** The default install asks for no OS permission; PERMISSIONS.md lists the optional ones, why each is needed and what breaks without it.
5. **Never act behind the user's back.** No synthesized keystrokes or clicks, no system-setting changes, and no bypassing OS focus protections by default.
6. **Reversible.** Uninstall removes the hooks and the app's data directory and leaves the user's other settings as they were.

**Threat model**

| Threat | Example | Mitigation |
| --- | --- | --- |
| Forged events sent to the receiver | A web page or another user's process posts to 127.0.0.1 to trigger switches | Bind to 127.0.0.1 only; random 256-bit token compared in constant time; require `Content-Type: application/json`; require a Host header of `127.0.0.1:<port>` or `localhost:<port>`, which blocks DNS rebinding; reject any request with an `Origin` header; token-bucket rate limit of 200 requests per second, burst 1,000 |
| Malformed or hostile payloads | Huge, deeply nested or invalid JSON in a prompt or tool input | 8 MB body limit; typed parsing of known fields only; nesting-depth limit; fuzz tests |
| Untrusted text in focusd's UI | A prompt or agent message containing HTML or script | Plain text through `textContent` only; strict Content-Security-Policy; no Markdown rendering; each window can call only the commands it needs |
| Injection into OS commands | A folder or app name passed into a shell, PowerShell or AppleScript | Call OS APIs directly; the only spawned processes are `focusctl` and the opt-in `claude` recap, each started with an argument array, never a shell |
| Damage to the user's Claude Code settings | The installer corrupts settings.json, drops other hooks or breaks a dotfiles symlink | Dry-run diff, backup, atomic write through symlinks, merge of marked entries only, refusal of non-JSON files, round-trip and fuzz tests |
| Leaking secrets | Prompts, agent output or commands written to logs or sent away | Content stays in memory; logs and history hold only names, ids, counts and hashes; the opt-in recap sends truncated, redacted text through the user's own Claude Code |
| Supply-chain compromise | A malicious dependency, CI action or release binary | The supply chain rules below |
| Permission creep | A feature quietly needing Accessibility, Input Monitoring or Screen Recording | No permission in the default install; Accessibility only for the opt-in feature C5; changes to PERMISSIONS.md need CODEOWNERS review |

Out of scope: malware already running as the user, which can read the token and config anyway.

**Code standards**

- **Structure.** `focus-core` holds every decision, has no I/O and sets `#![forbid(unsafe_code)]`, as do all crates except `focus-platform`. There, every `unsafe` block carries a `// SAFETY:` comment, enforced by `clippy::undocumented_unsafe_blocks`. The shell crates only wire things together.
- **Lints.** Set once in `[workspace.lints]`: rustc warnings denied; clippy `pedantic` at warn; `unwrap_used`, `expect_used`, `panic` and `todo` denied outside tests. `cargo fmt --check` runs in CI.
- **UI.** Plain HTML, CSS and JavaScript files, with no Node.js, npm or bundler. CSP `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:` plus Tauri's IPC origin. Tauri capabilities give each window only the commands it calls.
- **Concurrency.** The core runs on one thread and receives everything through a channel; window, overlay and hotkey calls run on the main thread. No shared mutable state between threads.
- **Errors and panics.** Typed errors and no silent failures: a failed OS call is logged and, when the user is affected, shown in the tray or toast. A panic is a bug: the panic hook logs its location, automation switches to paused and the tray shows a warning.
- **Readability.** Functions under about 50 lines and files under about 400; thresholds as named constants that match the config keys; comments explain why, not what.
- **Tests.** At least 90% line coverage on `focus-core`, driven by recorded traces and an injectable clock; negative tests for the receiver; fuzz targets for payload parsing and the settings.json merge; a manual checklist per OS for the adapters.
- **Logging.** Structured, levelled and rotated, and never containing prompt text, messages, commands, paths or tokens.
- **Performance budgets.** Hook reply under 10 ms at the 99th percentile; idle CPU under 0.5%; memory under 80 MB with the webview loaded; a switch decision within 5 ms of an event.
- **Decisions.** Each significant choice gets a short architecture decision record in `docs/adr/`.

**Supply chain and releases**

- Commit `Cargo.lock` and pin the toolchain in `rust-toolchain.toml`. Gate dependencies with `cargo-deny` (advisories, licenses, banned crates, allowed sources) and `cargo-audit`, and record crate reviews with `cargo-vet`.
- Dependabot for Cargo and GitHub Actions, weekly and grouped; every update still passes full CI and needs a review.
- Pin GitHub Actions to full commit SHAs. Workflows get read-only tokens by default, and only the release job, in a protected `release` environment, can read signing secrets.
- Build releases only in CI from signed tags, with `cargo auditable` so each binary carries its dependency list. Publish SHA-256 checksums, a CycloneDX SBOM and build-provenance attestations.
- Reproducibility: unsigned binaries must rebuild bit for bit from the tag with the pinned toolchain; REPRODUCIBLE.md explains how to compare a signed release after stripping its signature.
- macOS: Developer ID signature, hardened runtime and notarization. Windows: free signing through the [SignPath Foundation](https://signpath.org/) program, whose certificate names SignPath Foundation as the publisher; until approval, Windows builds ship unsigned with checksums.
- No auto-update in v1: "Check for updates" opens the releases page.

**Repository and governance**

- License: Apache-2.0, with SPDX headers on every file.
- Branch protection on `main`: required reviews and status checks, no force pushes, signed commits.
- CODEOWNERS requires a maintainer review for the receiver, installer, platform adapters, CI workflows, dependency files and PERMISSIONS.md.
- SECURITY.md: private reporting through GitHub's private vulnerability reporting, a 7-day first-response target and coordinated disclosure.
- A pull request template with a security checklist: does this add a permission, dependency, network call, file write or spawned process? Each needs a stated reason.
- Conventional Commits, SemVer starting at 0.1.0, and a CHANGELOG generated from commits.
- Contributions signed off under the Developer Certificate of Origin.

**Audit aids**

- AUDIT.md, one page, says where in the code each of these lives: the network listener, every file read or written, every process spawned, every OS API that observes the user, every optional permission and every `unsafe` block.
- `focusctl audit` prints the same facts from the built app: the dependency tree (from `cargo auditable` data), `unsafe` locations, registered hotkeys, the listener address and macOS entitlements.
- A Linux integration test runs the receiver and core headless with the fake platform, under `strace` in a temporary home, and replays the fixtures. It fails on any `connect()` to a non-loopback address and on any write outside the data folder and the marked settings.json entries.

**CI checks on every pull request**

| Check | Example tools | Runs on |
| --- | --- | --- |
| Format and lint | cargo fmt, cargo clippy | Linux, macOS, Windows |
| Core and receiver tests with coverage | cargo test, cargo-llvm-cov (90% on focus-core) | Linux |
| Platform and app build and tests | cargo test, cargo build | macOS, Windows |
| Fuzz smoke run | cargo-fuzz, 60 s per target | Linux |
| Network and file-write integration test | focus-headless under strace | Linux |
| Dependencies and licenses | cargo-deny, cargo-audit, GitHub dependency review | Linux |
| Secret scan | gitleaks | Linux |
| Required files, SPDX headers, DEPENDENCIES.md entries | small repo script | Linux |
| Release build, signing, notarization, SBOM, attestation (tags only) | release workflow | macOS, Windows |

## 14. Build plan for an LLM

Build the MVP in seven milestones, starting from an empty repository. Each ends in something that runs and is tested before the next starts.

**Prerequisites.** Rust stable pinned in `rust-toolchain.toml` (edition 2024); `cargo install tauri-cli --locked`; Xcode Command Line Tools on macOS; Visual Studio Build Tools with the C++ workload on Windows, where WebView2 ships with Windows 11 and current Windows 10. No Node.js.

1. **M0, repository, CI and receiver.** Cargo workspace with the crates from section 5; the repository files from section 13; CI from section 13; config loading with defaults; `focus-receiver` with every check in the threat model; `focusctl record`. Done when CI is green on all three OSes, a sample payload is accepted, and negative tests reject forged requests.
2. **M1, core state machines.** Session transitions and the switch controller from section 7 in `focus-core`, driven by fixture traces, a fake clock and the fake platform. Done when every fixture ends in the expected states and effects and core coverage is at least 90%.
3. **M2, platform spike on both OSes.** Every call in section 8's table, plus `focusctl test-switch <app>`. Record the answers in an ADR: does background activation work on each OS; does the focus key grant switch rights like a click; does the toast float over a borderless game without taking focus; does the idle-time call need a permission. Done when the manual checklist passes 9 of 10 attempts per OS, counting the fallback prompt. Stop and report if a finding contradicts the design.
4. **M3, policy end to end.** Away guards, countdowns, the input-quiet guard, verification and fallback prompts, debounce, pause and sound in the real app on the real platform. Done when replayed traces drive the expected switches on both OSes and a cancelled countdown produces none.
5. **M4, hook installer.** `focusctl install-hooks`, `uninstall-hooks` and `rotate-token` with every rule in section 6, on both OSes. Done when install, repeated install and uninstall round-trip the file exactly, including a symlinked file, and the fuzz target passes. Also record how visible failed hooks are while focusd is stopped.
6. **M5, recap, tray, wizard and doctor.** Basic recap card, tray menu, first-run wizard, launch at login and `focusctl doctor`. Done when a new user on each OS goes from install to a full away-and-back cycle with a real Claude Code prompt without reading the docs.
7. **M6, release engineering.** A release workflow producing a universal macOS .dmg and a per-user Windows NSIS installer, with checksums, SBOM and provenance; signing and notarization when the secrets exist, unsigned artifacts otherwise. Done when a tag produces every artifact and the unsigned binaries rebuild from the tag with matching checksums.

V1 then follows the backlog: run history and estimation (E1 to E4), the LLM recap (F2), run statistics (F3), the immersive flag and exact-window return (C4, C5), the multi-session queue (H1), ignore patterns (G5) and signed releases (O5).

**Suggested repository layout**

```text
focus-autopilot/
  Cargo.toml  Cargo.lock  rust-toolchain.toml  deny.toml  rustfmt.toml  clippy.toml
  README.md  LICENSE  SECURITY.md  CONTRIBUTING.md  CODE_OF_CONDUCT.md  CHANGELOG.md
  ARCHITECTURE.md  THREAT_MODEL.md  PERMISSIONS.md  DEPENDENCIES.md  AUDIT.md  REPRODUCIBLE.md
  docs/adr/                       architecture decision records
  docs/manual-test-checklist.md   per-OS adapter checks
  .github/
    workflows/ci.yml  workflows/release.yml  dependabot.yml
    CODEOWNERS  PULL_REQUEST_TEMPLATE.md
  crates/
    focus-core/                   every decision; no I/O, no unsafe
      src/  events.rs  session.rs  controller.rs  guards.rs  estimator.rs  recap.rs  config.rs
      tests/fixtures/             synthetic and scrubbed traces (.jsonl)
    focus-receiver/               HTTP receiver and parsing into AgentEvent
    focus-platform/               OS adapters; the only crate with unsafe
      src/  lib.rs  macos.rs  windows.rs  fake.rs  known_apps.rs
    focus-app/                    Tauri shell: wiring, overlay, tray, wizard, store
      ui/   toast.html  recap.html  wizard.html  app.css  app.js
    focusctl/                     install-hooks, uninstall-hooks, rotate-token, record, doctor, audit, test-switch
    focus-headless/               dev-only runner for the Linux integration test
  fuzz/                           cargo-fuzz targets
```

**Test plan.** Unit-test `focus-core` with fixture traces and an injectable clock. Record real hook payloads on both OSes with `focusctl record` and add them as scrubbed fixtures. Run the receiver's negative tests, the fuzz targets and the strace integration test in CI. Adapters follow `docs/manual-test-checklist.md` on each OS: switch to and from a terminal, an IDE and a borderless game; repeat on a second monitor and across Spaces; repeat with an elevated target app on Windows; confirm the fallback prompt appears whenever a switch is refused.

**Prompt to paste into the coding LLM**

```text
You are building the MVP of Focus Autopilot, an open-source desktop app for macOS and Windows that switches a developer between prompting work in Claude Code and steady work in an away app they choose, with countdowns, a one-key fallback and a recap.

Read the attached design document fully before writing code. Section 15 lists decisions that are final; don't reopen them. Start from an empty repository.

Work through milestones M0 to M6 in order. After each milestone, run its checks, commit, and summarize what passed. Continue to the next milestone unless a check fails or the M2 spike contradicts the design; in those cases stop and report.

Constraints:
- Rust workspace with a Tauri 2.8+ shell, laid out as in section 14. Every decision lives in focus-core (no I/O, no unsafe). unsafe only in focus-platform, each block with a SAFETY comment.
- Follow sections 6 and 7 exactly for hooks, payload fields, session transitions and the switch controller. If a payload field differs from https://code.claude.com/docs/en/hooks, follow the docs and record it in an ADR.
- Treat section 13 as hard requirements: threat-model mitigations, code standards, CI checks and repository files.
- The receiver replies 200 with an empty body immediately and never blocks, vetoes or changes anything Claude Code does.
- Every automatic switch goes through the switch controller: guards, countdown, input-quiet guard, verification, fallback prompt. Never synthesize input, change system settings, use focus workarounds, or request an OS permission in the default flow.
- Spawn processes only with argument arrays, never through a shell. Never build script text from runtime values.
- Add no dependency outside the starter list in section 5 without a DEPENDENCIES.md entry giving purpose, license and why the standard library isn't enough.
- Never write prompt text, agent output, commands, paths or tokens to logs or history.
- Implement every config key in section 11 with its default.
- When something is unspecified, choose what best fits sections 2 and 13, record it in docs/adr, and continue.

Out of scope for the MVP: agents other than Claude Code, Linux, estimation beyond elapsed time, the LLM recap, the multi-session queue UI.
```

## 15. Decisions and assumptions

Every earlier open question is decided here so the build can start without blockers. Claude Code should treat these as final; the last column says what evidence would justify revisiting one.

| Decision | Choice | Why | Revisit if |
| --- | --- | --- | --- |
| Platforms | macOS 14 or later as a universal build; Windows 10 22H2 and 11 on x64; Linux in V2 | Where most developers run Claude Code locally; macOS 14 has the cooperative-activation APIs | Demand for Linux or Windows on Arm |
| Stack | Rust workspace with a Tauri 2.8+ shell and a plain HTML, CSS and JavaScript UI; no Node.js | Memory-safe parsing, one codebase, a small auditable dependency tree; 2.8 adds non-focusable windows | The M2 spike shows Tauri can't host the overlay, which then moves to native code |
| License | Apache-2.0 | Explicit patent grant and OSI approval; one license keeps the SignPath application simple | Never, once published |
| Switching model | Try automatic, verify, fall back to one click or key; no OS workarounds | Trust, and honesty about OS limits | The spike shows Windows refuses nearly every switch; then consider C6 |
| Home window | The frontmost known home app at prompt submit | Zero configuration and no process inspection | Many real runs end up with no home |
| Away apps | A user-chosen ordered list; focusd never launches them | Launching an app is slow and surprising | Users ask for launching |
| Away guards | Home window in front, 8 s idle, tier threshold reached | Never yank an active user | Reports of missed or unwanted switches |
| Permission prompts | Return right away, after debounce and countdown | Claude is blocked until the user answers | Users find it too eager |
| Claude Code surfaces | Any local surface that loads user settings: CLI, IDE extensions, desktop app | The same hooks fire everywhere | A surface fires no hooks; doctor reports it |
| Recap | Basic in the MVP; opt-in LLM summary through the local `claude` CLI in V1; no API keys | Zero setup and no secrets to store | Summaries regularly take over 20 s |
| Storage | TOML config and JSON Lines history; no database | Human-readable, auditable, no C dependency | History outgrows a simple file scan |
| Hotkeys | Focus key registered only while a toast shows; three-modifier defaults | Avoids clashes, such as macOS input-source switching | Conflict reports |
| Multi-session | MVP: global rules and one switch at a time; V1: queue UI | Safe now, richer later | Heavy parallel-session users |
| Distribution | GitHub Releases with a notarized .dmg and a per-user NSIS installer that needs no admin rights; Homebrew and winget in V1 | Verifiable artifacts, no elevation |  |
| Updates | No auto-update; the tray links to releases | No network traffic by default |  |
| Name | Working name Focus Autopilot; names live in one branding module and `tauri.conf.json` | Easy to rename before 1.0 | The name check finds a conflict; similar names such as FocusPilot exist |
| Bundle id | `io.github.<owner>.focusautopilot` | Needs no domain | The project gets a domain |

**Maintainer actions.** None of these block milestones M0 to M5.

- [ ] Create the public GitHub repository; turn on private vulnerability reporting and branch protection.
- [ ] Join the Apple Developer Program and create a Developer ID certificate for notarized macOS releases (M6).
- [ ] Apply to the SignPath Foundation program for free Windows signing. It requires an OSI license, multi-factor authentication for the whole team and a credit line in the README, and the certificate names SignPath Foundation as publisher. Until approval, Windows builds ship unsigned with checksums.
- [ ] Check the name on GitHub, crates.io, app stores and trademark registers before the first public release.
- [ ] Record the first real hook traces on both OSes with `focusctl record` and commit them as fixtures.
