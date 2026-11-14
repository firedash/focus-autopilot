# Focus Autopilot

A local background app that moves a developer between Claude Code and an away app (a game,
say), driven by the agent's state: away when a prompt runs long, back when Claude finishes or
needs permission, with a countdown before every switch and a short recap on return.

Design: "Focus Autopilot — Feature Backlog & Technical Design (v1: Claude Code on macOS and
Windows)". Section numbers in code comments refer to that document.

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
