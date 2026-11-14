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
only the decision core is built. There is no runnable app yet.

## Layout

```
crates/
  focus-core/      every decision; no I/O, no unsafe, no dependencies
  focus-platform/  the Platform trait, built-in home apps, and a fake for tests
docs/adr/          architecture decision records
```

## Check it

```
cargo fmt --check
cargo clippy --all-targets
cargo test
```

License: Apache-2.0, see [LICENSE](LICENSE).
