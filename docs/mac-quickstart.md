# Running Focus Autopilot on your Mac (developer build)

This is the first build that can run on a Mac. It has **never run on a Mac yet**: the macOS
adapter was type-checked from Linux only. Expect to find and fix problems; the notes at the
end say where to look first. Toasts and recaps print in the terminal (the overlay UI is
milestone M5), and there are no global hotkeys yet.

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

## 3. Choose your away app

```bash
./target/release/focusd --list-apps      # bundle id <tab> name of every running app
```

Add one to `config.toml`. For quick testing, also shorten the waits:

```toml
[away]
apps = ["md.obsidian"]          # your steady-work app; must be running

[switching]
short_grace_seconds = 5
away_after_seconds = 5
away_requires_idle_seconds = 3
```

## 4. Run

```bash
./target/release/focusd
```

Leave it running in a terminal (a Terminal.app or iTerm window works; so does your IDE's
terminal). In another terminal window start `claude`, give it a task that takes a while, and
stop touching the keyboard. You should see a countdown toast in the `focusd` output, then your
away app comes to the front. When Claude stops or asks for approval, `focusd` prints a prompt
and a recap. Type `click` in the `focusd` terminal to act on a toast, `pause` / `resume` to
pause, `quit` to stop.

The window you prompted from must be a known home app (Terminal, iTerm2, VS Code, JetBrains,
Cursor and so on; list in `crates/focus-platform/src/known_apps.rs`). Add others under
`[home] extra_apps`.

## What to check first if it misbehaves

- **No switch happens.** `focusd` prints nothing for guards; check that your terminal's bundle
  id is a home app, the away app is running, and `away_requires_idle_seconds` has passed.
- **Away app doesn't come forward.** macOS 14+ may refuse a background process activating
  another app. The core notices (it verifies after `verify_after_ms`) and falls back to a
  prompt; if so, that is the M2 finding to record.
- **Front-app changes arrive late.** `NSWorkspace` is read with a short run-loop pump and
  polled every 500 ms; a notification observer is not written.
- **Idle time looks wrong.** It comes from `CGEventSourceSecondsSinceLastEventType`.
- Everything the adapter calls needs no permission prompt. If macOS shows one, that is a bug
  against the design: report it.
