# Running Focus Autopilot on your Mac (developer build)

This is a developer build. The switching, hooks and on-screen boxes have run on a Mac; the
global hotkey has not yet. Expect to find and fix problems; the notes at the
end say where to look first.

## 1. Build

```bash
git clone https://github.com/firedash/focus-autopilot && cd focus-autopilot
git checkout claude/wonderful-faraday-gxop6h
cargo build --release -p focusd -p focusctl
```

Needs Xcode command line tools (`xcode-select --install`). Rust 1.97.0 is picked up from
`rust-toolchain.toml`.

## 2. Install the hooks

```bash
./target/release/focusctl install-hooks --dry-run   # shows the diff to ~/.claude/settings.json
./target/release/focusctl install-hooks             # backs it up, merges, writes the token
```

This also creates `~/Library/Application Support/io.github.firedash.focusautopilot/config.toml`
with a random token. Restart any running Claude Code sessions. Undo with
`./target/release/focusctl uninstall-hooks`.

## 3. Run

```bash
./target/release/focusd
```

Leave it running in a terminal. It prints the hotkey to press (by default `⌃⌥⌘Space`, that is
Control+Option+Command+Space) and says "manual mode": **nothing switches you away on its own.**
Go to any other window while Claude works.

In another terminal window start `claude` and give it a task. Switch to any other app. When
Claude finishes or needs your approval, a box appears at the top of the screen, a sound plays,
and the box says which key takes you back. Press it and you land in the window you prompted
from, with a recap card for a few seconds. If you go back on your own, the box goes away.

Commands typed in the `focusd` window still work: `click` (same as the hotkey), `pause`,
`resume`, `quit`. The window you prompted from must be a known home app (Terminal, iTerm2,
VS Code, JetBrains, Cursor and so on; list in `crates/focus-platform/src/known_apps.rs`). Add
others under `[home] extra_apps` in `config.toml`.

Change the hotkeys in `config.toml`; an empty string turns one off. A chord needs at least one
of Control, Option, Shift, Command:

```toml
[hotkeys]
focus_key = "Control+Option+Space"
pause = "Control+Option+P"
```

## 4. Optional: switch away automatically

If you also want focusd to move you to a chosen app when a run gets long, name it. This is
the old behaviour, with a countdown and an automatic return:

```bash
./target/release/focusd --list-apps      # bundle id <tab> name of every running app
```

```toml
[away]
apps = ["com.apple.Notes"]

[switching]
short_grace_seconds = 5
away_after_seconds = 5
away_requires_idle_seconds = 3
```

## What to check first if it misbehaves

- **No box appears when Claude finishes.** Check that `focusd` printed `front: <id>` for your
  terminal and that id is a home app. If you were still in that window when Claude finished,
  nothing is shown, by design.
- **The hotkey does nothing.** `focusd` prints `hotkey problem: ...` at startup if the OS refused
  the chord (another app may own it); pick another in `[hotkeys]`, or type `click` in the
  `focusd` window.
- **Away app doesn't come forward.** macOS 14+ may refuse a background process activating
  another app. The core notices (it verifies after `verify_after_ms`) and falls back to a
  prompt; if so, that is the M2 finding to record.
- **Front-app changes arrive late.** `NSWorkspace` is read with a short run-loop pump and
  polled every 500 ms; a notification observer is not written.
- **Idle time looks wrong.** It comes from `CGEventSourceSecondsSinceLastEventType`.
- Everything the adapter calls needs no permission prompt. If macOS shows one, that is a bug
  against the design: report it.
