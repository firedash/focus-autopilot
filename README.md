# Focus Autopilot

[![ci](https://github.com/firedash/focus-autopilot/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/firedash/focus-autopilot/actions/workflows/ci.yml)

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
- A page with two tabs, opened with `focusctl ui`: **Statistics** shows where your time went
  while Claude worked, and **Configuration** changes every setting without editing a file.
- Optionally, name an away app and it also switches you there when a prompt runs long, with a
  countdown that one key cancels or snoozes, so keystrokes never land in the wrong window.

v1 supports Claude Code on macOS and Windows. Nothing leaves your machine by default, and the
app asks for no OS permission.

Design: "Focus Autopilot — Feature Backlog & Technical Design (v1: Claude Code on macOS and
Windows)". Section numbers in code comments refer to that document. The design and the code
call the optional app it switches you to the "away app". Where the build departs from the
design, [docs/adr/0003-shell-decisions.md](docs/adr/0003-shell-decisions.md) records why and wins.

**This is a developer build. Read [STATUS.md](STATUS.md) before anything else:** it says what
has been seen working on a real Mac, what has only been verified on Linux, and what compiles for
macOS but has never been run. The global hotkeys are in that last group.

## What it looks like

The page opened by `focusctl ui`. These pictures use made-up data (`scripts/ui-check/seed.py`),
not anyone's real usage; `scripts/ui-check/screenshots.sh` regenerates them.

**Statistics**: what stands out, how long Claude worked each day, and where your time went
while it did.

<img src="docs/images/statistics.png" alt="The Statistics tab: a list of insights, a column chart of Claude's working time over seven days, five totals for today, and a list of apps with bars split into time while Claude worked and the rest of the time" width="760">

**Configuration**: every setting, changed in place and applied at once. A change that would
break a rule is refused with the reason, and your `config.toml` and its comments are left alone.

<img src="docs/images/configuration.png" alt="The Configuration tab: switches for the app, sound, recording time per app and the LLM recap, then the away app settings with number fields" width="620">

Dark mode and a phone-width layout, both automatic:

<p>
<img src="docs/images/statistics-dark.png" alt="The Statistics tab in dark mode" width="460">
&nbsp;
<img src="docs/images/statistics-phone.png" alt="The Statistics tab at phone width" width="230">
</p>

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
  focus-receiver/  loopback hook receiver, payload parsing, config loading, and the page
    assets/ui/     the Statistics and Configuration page: plain HTML, CSS and JavaScript
  focus-runner/    the shell loop, time per app, statistics, settings, run history
  focus-headless/  dev runner: receiver + core + fake platform, effects printed
  focusd/          the daemon (macOS): receiver + core + real platform adapter
  focusctl/        hooks, token, the page, doctor, launch at login
scripts/
  check-repo.py    repository rules: required files, SPDX headers, dependency entries
  ui-check/        drives the page in a real browser
docs/adr/          architecture decision records
```

Trust documents: [SECURITY.md](SECURITY.md), [THREAT_MODEL.md](THREAT_MODEL.md),
[PERMISSIONS.md](PERMISSIONS.md) (it asks for none), [AUDIT.md](AUDIT.md) (where each thing it
does to your machine lives in the code), [DEPENDENCIES.md](DEPENDENCIES.md).

## Try it on a Mac

See [docs/mac-quickstart.md](docs/mac-quickstart.md). In short:

```bash
cargo build --release -p focusd -p focusctl
./target/release/focusctl install-hooks     # adds the hooks, creates the token
./target/release/focusd                     # leave it running
./target/release/focusctl ui                # statistics and configuration, in your browser
./target/release/focusctl doctor            # if something isn't working
```

## Try it anywhere (fake platform)

```bash
cargo run -p focus-headless -- --port 47821
```

It prints a token and the address of the page (`ui: http://127.0.0.1:47821/ui/#t=...`). Send
hooks with `curl -H "X-Focus-Token: <token>" -H "Content-Type: application/json" -d
'{"hook_event_name":"UserPromptSubmit","session_id":"s1","cwd":"/w","prompt":"hi"}'
http://127.0.0.1:47821/hook`, then a `Stop` event, and watch the toasts and recap. Type
`front <app id>` to play the user switching apps. To see the page with data in it:

```bash
scripts/ui-check/run.sh        # seeds made-up data and checks the page in Chromium
```

## Check it

```
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
python3 scripts/check-repo.py
```

CI runs these on Linux, builds and tests on macOS, and runs `cargo deny`. The macOS code can
also be type-checked from Linux: `rustup target add aarch64-apple-darwin`, then
`cargo clippy --workspace --all-targets --target aarch64-apple-darwin`.

License: Apache-2.0, see [LICENSE](LICENSE).
