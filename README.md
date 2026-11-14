# Focus Autopilot

Working with a coding agent splits your day into two kinds of work. **Prompting work** is
intermittent: you write a prompt, wait anywhere from seconds to twenty minutes, review the
result, and prompt again. **Steady work** is whatever you would rather be doing during those
waits, and it is yours to define: writing, reading, design, email, another project, a break.

Focus Autopilot is a local background app that moves you between the two, driven by the
agent's state:

- You go wherever you like while a prompt runs: another window, another app, a break.
- When the agent finishes or needs your approval and you are not in the window you prompted
  from, you get a sound and a small box on screen. One key takes you back to that window and
  shows a short recap: what you asked, what the agent did, what it needs from you.
- Every box names the project ("Claude is done in api"), so parallel sessions can be told
  apart, and secrets typed into a prompt are redacted before they are stored or shown.
- Optionally, name an away app and it also switches you there when a prompt runs long, with a
  countdown that one key cancels or snoozes, so keystrokes never land in the wrong window.

v1 supports Claude Code on macOS and Windows. Nothing leaves your machine by default, and the
app asks for no OS permission.

Design: "Focus Autopilot — Feature Backlog & Technical Design (v1: Claude Code on macOS and
Windows)". Section numbers in code comments refer to that document. The design and the code
call the optional app it switches you to the "away app". Where the build departs from the
design, [docs/adr/0003-shell-decisions.md](docs/adr/0003-shell-decisions.md) records why and wins.

**This repository is at an early stage. Read [STATUS.md](STATUS.md) before anything else:**
a developer build of the macOS daemon runs: the switching, the on-screen boxes and the recap have
been run on a Mac with Claude Code in a terminal. The global hotkey and every other Claude Code
surface are untested, and there is no menu-bar icon or installer yet.

## Where it runs

Only **Claude Code in a terminal** has been tested. Focus Autopilot listens to Claude Code's
hooks, and Anthropic's documentation says Claude Code fires the same hooks wherever it runs,
so other places *should* work with the same install. That is a reading of the docs, not
something verified here. Each one also needs its app in the home-app list; if it is missing,
`focusd` says so and prints the line to add.

| Where you run Claude Code | Tested | What is known |
| --- | --- | --- |
| Terminal (Terminal, iTerm2, Ghostty, Warp, ...) | yes, on a Mac | works |
| VS Code, Claude Code extension | no | Anthropic's docs say the extension shares `~/.claude/settings.json` with the CLI, hooks included |
| Cursor, same extension or `claude` in its terminal | no | the docs offer the extension for Cursor; `claude` in a terminal inside it is the terminal case. Cursor's own built-in agent is a different product with its own hooks and is not supported |
| Claude Desktop app, Code tab | no, and uncertain | the docs say hooks fire there, but users have reported some events not firing in Desktop, so it may not work fully. The chat tab is not Claude Code and sends no hooks |
| JetBrains IDEs | no | Anthropic's hooks page does not mention JetBrains, so nothing is known |
| Claude Code on the web or other cloud sessions | not supported | they fire no local hooks |

Returns go to the app, not a specific window: with several windows of one editor you land in
whichever one the OS raises.

## Layout

```
crates/
  focus-core/      every decision; no I/O, no unsafe, no dependencies
  focus-platform/  the Platform, Overlay and Hotkeys traits, home apps, macOS adapter, a fake
  focus-receiver/  hardened loopback hook receiver, payload parsing, config.toml loading
  focus-runner/    the shell loop shared by the two runners
  focus-headless/  dev runner: receiver + core + fake platform, effects printed
  focusd/          the daemon (macOS): receiver + core + real platform adapter
  focusctl/        install and remove the Claude Code hooks, rotate the token, launch at login
docs/adr/          architecture decision records
```

## Try it on a Mac

See [docs/mac-quickstart.md](docs/mac-quickstart.md): build, install the hooks, run `focusd`.
The switching, the on-screen boxes and the recap have run on a Mac; the global hotkey and the
VS Code, Cursor and Desktop paths have not. Optional extras: start at login
(`focusctl install-agent`), three-line LLM recaps, and an away app.

## Try it anywhere (fake platform)

```
cargo run -p focus-headless -- --port 47821
```

It prints a token. Send hooks with `curl -H "X-Focus-Token: <token>" -H "Content-Type:
application/json" -d '{"hook_event_name":"UserPromptSubmit","session_id":"s1","cwd":"/w","prompt":"hi"}'
http://127.0.0.1:47821/hook`, then a `Stop` event, and watch the toasts and recap. A config
with an away app and short thresholds (`switching.away_after_seconds`, `short_grace_seconds`, `countdown.seconds`)
makes the away switch happen in seconds. `[recap] mode = "llm"` turns on three-line summaries
written by your own `claude` CLI (each line cut at 120 characters).

## Check it

```
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

The macOS code only builds on a Mac, but can be type-checked from Linux:
`rustup target add aarch64-apple-darwin`, then
`cargo clippy -p focusd -p focus-platform -p focus-runner --target aarch64-apple-darwin`.

License: Apache-2.0, see [LICENSE](LICENSE).
