# Audit map

Where to look to check what this app does to your machine. Each claim names the file that
makes it true, so it can be verified by reading that file. If code and this page disagree,
the page is the bug: please report it.

## Network

| What | Where |
| --- | --- |
| The only listener: `127.0.0.1:<port>` (default 47821), never another interface | `crates/focus-receiver/src/server.rs`, `spawn_with_ui` |
| The checks every request must pass (path, method, Host, Origin, token, content type, size, rate) | `crates/focus-receiver/src/check.rs` |
| Routes: `POST /hook`, `GET /ui/*` (three static files), `GET`/`POST /api/*` | `crates/focus-receiver/src/server.rs`, `handle` and `api`; `src/ui.rs` |
| The page's files, embedded in the binary | `crates/focus-receiver/assets/ui/` |
| `focusctl doctor` connects to that same loopback port to see whether `focusd` answers | `crates/focusctl/src/doctor.rs`, `get` |

The app itself makes **no outbound connection**. The one thing that reaches the network is the
`claude` command, and only if you turn on the LLM recap (see Processes).

## Files

Data folder: `~/Library/Application Support/io.github.firedash.focusautopilot/` on macOS, or
`FOCUS_AUTOPILOT_HOME` (`crates/focus-runner/src/paths.rs`).

| File | Read or written by | Holds |
| --- | --- | --- |
| `config.toml` (data folder) | written by `focusctl/src/config.rs` (token) and `focus-runner/src/settings.rs` (the Configuration tab); parsed by `focus-receiver/src/config_file.rs` | settings and the receiver token; mode 0600 |
| `runs.jsonl` (data folder) | `focus-runner/src/history.rs` | one metadata line per finished run: times, counts, a salted hash of the project folder. No prompt text, commands or paths; mode 0600 |
| `usage.json` (data folder) | `focus-runner/src/usage.rs` | time per app in half-hour slots: app ids, names, milliseconds, counts. No window titles; mode 0600 |
| `open-ui.html` (data folder) | `focusctl/src/ui.rs` | a redirect to the page, with the token after the `#`; mode 0600 |
| `recap-workdir/` (data folder) | created by `focus-runner/src/recap_cli.rs` | an empty folder the `claude` command is run in |
| `focusd.log` (data folder) | the launch agent's stdout, set in `focusctl/src/agent.rs` | event names, app names and outcomes; never prompt text, commands, toast or recap text (`focus-runner/src/lib.rs`, `verbose`) |
| `~/.claude/settings.json`, or the project's with `--scope` | `focusctl/src/files.rs`, `hooks.rs` | only entries whose URL is this app's hook URL are added or removed; a `.focus-autopilot.bak` copy is kept beside it |
| `~/Library/LaunchAgents/io.github.firedash.focusautopilot.plist` | `focusctl/src/agent.rs` | only with `focusctl install-agent` |

`focusd` never reads Claude Code's transcripts or any file in your projects.

## Processes started

| Program | When | Where |
| --- | --- | --- |
| `claude -p ...` with no tools, no MCP servers and hooks disabled | only when `recap.mode = "llm"`, once per recap | `crates/focus-runner/src/recap_cli.rs` |
| `claude --version` | `focusctl doctor` | `crates/focusctl/src/main.rs`, `claude_version` |
| `open <file>` (macOS; `explorer` on Windows; `xdg-open` on Linux) | `focusctl ui` | `crates/focusctl/src/ui.rs`, `open_in_browser` |

Each is started with an argument list, never through a shell.

## OS calls that observe or move the user (macOS)

| Call | Purpose | Where |
| --- | --- | --- |
| `NSWorkspace.frontmostApplication`, `runningApplications` | which app is in front; which are running | `crates/focus-platform/src/macos.rs` |
| `CGEventSourceSecondsSinceLastEventType` | seconds since any input; no input content | `crates/focus-platform/src/macos.rs` |
| `NSRunningApplication.activateWithOptions` | bring an app to the front | `crates/focus-platform/src/macos.rs` |
| `RegisterEventHotKey` (Carbon) | be told when one of up to three chosen chords is pressed | `crates/focus-platform/src/macos_hotkeys.rs` |
| A non-activating `NSPanel` that ignores the mouse | the toast and the recap card | `crates/focus-platform/src/macos_overlay.rs` |
| `NSSound` | the sound on return | `crates/focus-platform/src/macos.rs` |

## OS calls that observe or move the user (Windows)

| Call | Purpose | Where |
| --- | --- | --- |
| `GetForegroundWindow`, `GetWindowThreadProcessId`, `OpenProcess` (query only), `QueryFullProcessImageNameW` | which window is in front and which executable owns it | `crates/focus-platform/src/windows.rs` |
| `EnumWindows`, `IsWindowVisible`, `GetWindow`, `GetWindowLongW`, `GetWindowTextLengthW`, `DwmGetWindowAttribute` | which windows are app windows; the title's length is read, never the title | `crates/focus-platform/src/windows.rs` |
| `GetLastInputInfo`, `GetTickCount` | seconds since any input; no input content | `crates/focus-platform/src/windows.rs` |
| `ShowWindow` (restore), `SetForegroundWindow` | bring a window to the front | `crates/focus-platform/src/windows.rs` |
| `SendInput` with one mouse event that moves nothing and presses nothing | be allowed that request when Windows refused it; off with `switching.force_foreground = false` | `crates/focus-platform/src/windows.rs`, `empty_mouse_event` |
| `RegisterHotKey` | be told when one of up to three chosen combinations is pressed | `crates/focus-platform/src/windows.rs` |
| Two topmost, non-activating, click-through popup windows drawn with GDI | the toast and the recap card | `crates/focus-platform/src/windows_overlay.rs` |
| `MessageBeep` | the sound on return | `crates/focus-platform/src/windows.rs` |

Not used anywhere: Accessibility, Input Monitoring, Screen Recording, Apple Events, event taps,
keyboard or mouse hooks, thread-input attachment, synthesized keystrokes or clicks. The one
empty mouse event above is the only input the app sends. See [PERMISSIONS.md](PERMISSIONS.md).

## `unsafe`

Only `focus-platform` may contain it; every other crate sets `#![forbid(unsafe_code)]`, which
`scripts/check-repo.py` checks. Each block has a `// SAFETY:` comment, enforced by the
`undocumented_unsafe_blocks` lint.

| File | Blocks | What for |
| --- | --- | --- |
| `crates/focus-platform/src/macos_hotkeys.rs` | 6 | hand-declared Carbon functions for hotkeys, which the `objc2` crates do not bind |
| `crates/focus-platform/src/macos_overlay.rs` | 2 | keeping a panel alive after it closes; taking the next AppKit event |
| `crates/focus-platform/src/macos.rs` | 1 | turning the run loop once so the front-app reading is fresh |
| `crates/focus-platform/src/windows.rs` | 20 | Win32 calls through `windows-sys`: every one is `unsafe` by declaration |
| `crates/focus-platform/src/windows_overlay.rs` | 10 | creating, painting, showing and hiding the two overlay windows |

## What the page can do

The statistics and configuration page can read the statistics, read the settings, change the
settings listed in `crates/focus-runner/src/settings.rs` (`KEYS`), and erase the statistics.
It cannot read or change the token, the port or the history salt: they are not in that list,
and a test (`secrets_and_unknown_names_cannot_be_set`) holds that.

## Checking it yourself

- `cargo deny check` for advisories, licenses and crate sources (`deny.toml`).
- `python3 scripts/check-repo.py` for required files, SPDX headers and dependency entries.
- `scripts/ui-check/run.sh` drives the page in a real browser.
- Not built yet: `focusctl audit`, and the test that runs the app under `strace` and fails on
  any non-loopback connection or stray file write.
