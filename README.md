# Focus Autopilot

Working with a coding agent splits your day into two kinds of work. **Prompting work** is
intermittent: you write a prompt, wait anywhere from seconds to twenty minutes, review the
result, and prompt again. **Steady work** is whatever you would rather be doing during those
waits, and it is yours to define: writing, reading, design, email, another project, a break.

Focus Autopilot is a local background app that moves you between the two, driven by the
agent's state:

- When a prompt runs long enough, it switches you to the app you chose for steady work.
- When the agent finishes or needs your approval, it brings you back to the window you
  prompted from and shows a short recap: what you asked, what the agent did, what it needs.
- Every switch is announced by a short countdown that one click or one key cancels or
  snoozes, so keystrokes never land in the wrong window.

v1 supports Claude Code on macOS and Windows. Nothing leaves your machine by default, and the
app asks for no OS permission.

Design: "Focus Autopilot — Feature Backlog & Technical Design (v1: Claude Code on macOS and
Windows)". Section numbers in code comments refer to that document. The design and the code
call the app you switch to the "away app".

**This repository is at an early stage. Read [STATUS.md](STATUS.md) before anything else:**
the decision core, a headless runner and a first macOS daemon are built; the daemon has not run on a Mac yet.

## Layout

```
crates/
  focus-core/      every decision; no I/O, no unsafe, no dependencies
  focus-platform/  the Platform trait, built-in home apps, and a fake for tests
  focus-receiver/  hardened loopback hook receiver, payload parsing, config.toml loading
  focus-runner/    the shell loop shared by the two runners
  focus-headless/  dev runner: receiver + core + fake platform, effects printed
  focusd/          the daemon (macOS): receiver + core + real platform adapter
  focusctl/        install and remove the Claude Code hooks, rotate the token
docs/adr/          architecture decision records
```

## Try it on a Mac

See [docs/mac-quickstart.md](docs/mac-quickstart.md). It has not run on a Mac yet.

## Try it anywhere (fake platform)

```
cargo run -p focus-headless -- --port 47821
```

It prints a token. Send hooks with `curl -H "X-Focus-Token: <token>" -H "Content-Type:
application/json" -d '{"hook_event_name":"UserPromptSubmit","session_id":"s1","cwd":"/w","prompt":"hi"}'
http://127.0.0.1:47821/hook`, then a `Stop` event, and watch the toasts and recap. A config
with short thresholds (`switching.away_after_seconds`, `short_grace_seconds`, `countdown.seconds`)
makes the away switch happen in seconds.

## Check it

```
cargo fmt --check
cargo clippy --all-targets
cargo test
```

License: Apache-2.0, see [LICENSE](LICENSE).
