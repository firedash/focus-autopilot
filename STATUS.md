# Status

Last updated 2026-11-14.

**Where it stands:** a developer build runs on a Mac. `focusd` takes real Claude Code hooks,
decides with `focus-core`, shows on-screen boxes, plays a sound and brings you back with one
key. The switching, boxes and recap have been run on a Mac with real Claude Code in a
terminal. Everything marked "unrun" below compiles and passes clippy for macOS but has not
been seen working. See [docs/mac-quickstart.md](docs/mac-quickstart.md) to run it.

**Unverified on a Mac:** the global hotkey, and every surface except the terminal.

Where the build departs from `docs/design.md`, [ADR 0003](docs/adr/0003-shell-decisions.md)
wins. The biggest departure: **manual mode is the default**. Nothing switches you away on its
own; you go where you like and one key brings you back. Automatic switching to an away app is
optional.

## What runs

| Part | Crate | State |
| --- | --- | --- |
| Every decision: sessions, guards, switch controller, estimator, recap text, redaction | `focus-core` | done; 84 original tests plus manual-mode and recap-fallback tests |
| Hook receiver: path, method, Host, Origin, constant-time token, content type, 8 MB limit, rate limit | `focus-receiver` | done; negative tests over a real socket |
| Payload parsing, truncation, ANSI stripping, project hash; `config.toml` loading (unknown keys are errors) | `focus-receiver` | done |
| `focusctl install-hooks`, `uninstall-hooks`, `rotate-token` (dry run, backup, atomic write, symlinks, strict JSON, idempotent; uninstall restores the original bytes) | `focusctl` | done and tested |
| The shell loop shared by both runners; front-app polling every 500 ms; activation verified against the real front app | `focus-runner` | done |
| LLM recap: runs the user's `claude` CLI (no tools, no hooks), three lines of at most 120 characters, basic card as fallback | `focus-runner`, `focus-core` | tested with fake CLIs and end to end with a fake `claude`; not yet against every real CLI version |
| macOS adapter: front app, running apps, activation, idle time, sound | `focus-platform/src/macos.rs` | run on a Mac |
| On-screen toast and recap card (non-activating panels) | `focus-platform/src/macos_overlay.rs` | run on a Mac |
| Global hotkeys (focus key acts on the toast, pause key); chord parser | `hotkey.rs`, `macos_hotkeys.rs` | parser tested; registration unrun |
| Warning when a prompt comes from an app that is not a home app, with the config line to add | `focus-runner` | done |
| Secrets redacted at the receiver (before storing, with a window past the cut) and again on the recap card | `focus-receiver`, `focus-core` | done; tested including a key straddling the truncation point |
| Project name on every toast and recap, other waiting projects named | `focus-core` | done and tested; the key still returns to the app, not the session |
| Run history: `runs.jsonl` written (mode 0600, no text) and loaded into the estimator, with retention | `focus-runner` | done; tested, and across two launches of the headless runner |
| Logs hold event names only when output is a file; full text only to a terminal | `focus-runner`, `focusd` | done |
| Launch at login: `focusctl install-agent` / `uninstall-agent` (writes the plist, prints the `launchctl` command) | `focusctl` | the plist is tested; never loaded by `launchd` here |
| Fake platform, overlay and hotkeys; headless runner | `focus-platform`, `focus-headless` | done; runs on Linux |

Checks run on Linux with Rust 1.97.0: `cargo fmt --check`, `cargo clippy --workspace
--all-targets` with the workspace lints from section 13 of the design (warnings denied,
pedantic, no `unwrap`, `expect`, `panic` or `todo` outside tests), and `cargo test
--workspace`: 152 tests, all passing. macOS code is checked with `cargo clippy -p focusd -p
focus-platform -p focus-runner --target aarch64-apple-darwin`.

## Which Claude Code surfaces

Only the terminal has been tested. Anthropic's docs say Claude Code fires the same hooks
wherever it runs, so one install *should* cover other surfaces, but that is unverified here.
Each also needs its app in the home-app list (`focusd` prints the line to add).

| Surface | Tested | What is known |
| --- | --- | --- |
| Terminal | yes | works |
| VS Code, Claude Code extension | no | docs: shares `~/.claude/settings.json` with the CLI |
| Cursor, same extension or `claude` in its terminal | no | docs offer the extension for Cursor; Cursor's own agent is not supported |
| Claude Desktop, Code tab | no | docs say hooks fire; users report some events not firing, so it may not work fully |
| JetBrains | no | not mentioned in the hooks docs; unknown |
| Cloud sessions | n/a | not supported: no local hooks |

Returns go to the app, not the window, so with several sessions in one app the key lands in
the app; the project name on the toast says which session is waiting. Exact-window return
(backlog C5) is not built.

## Not built

- **M0 (rest):** `focusctl record`, CI, fuzz targets, the strace integration test, and the
  repository files from section 13 (SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md,
  CODEOWNERS, THREAT_MODEL.md, PERMISSIONS.md, AUDIT.md, REPRODUCIBLE.md, deny.toml,
  workflows).
- **M2:** the platform spike recorded in an ADR, and the whole Windows adapter.
- **M3/M5:** the Tauri shell, menu-bar icon, click on a toast, first-run wizard, an installer.
  `focusd` runs in a terminal or, with `focusctl install-agent`, from a launch agent.
- **M4 (rest):** `focusctl doctor`, `--scope project` and `--scope local`, the failed-hook
  visibility check.
- **M6:** release engineering.
- Front-app change notifications (polled instead), exact-window return (needs the opt-in
  Accessibility permission or spawning the editor's command line; see ADR 0003 item 16), the
  declare-long hotkey.
- Line coverage on `focus-core` is unmeasured; the target is 90%. No performance budgets
  measured.

## Before continuing

1. `rust-toolchain.toml` pins 1.97.0, the version this was checked with.
2. Verify the app ids in `crates/focus-platform/src/known_apps.rs` against real installs of
   VS Code, Cursor and the Claude Desktop app. They were written from memory. `focusd` prints
   `front: <id>` whenever the front app changes, which gives the real id.
3. Read the records in `docs/adr/`: they list where the design left a choice open and what
   was chosen.
4. The core's trace tests still read fixtures with a test-only JSON reader
   (`crates/focus-core/tests/support/json.rs`). `focus-receiver/tests/http.rs` parses the same
   fixture with the real parser; moving the core tests onto it would make `focus-core` depend
   on the receiver in dev, which was left undone.
