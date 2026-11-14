# Status

Last updated 2026-11-14.

## Built and tested

`focus-core`: the code every other part will call. It covers milestone M1 and the V1 backlog
items that are pure decision logic.

| Design item | Where | State |
| --- | --- | --- |
| Core contract: `Input`, `Effect`, `Core::handle/tick/next_deadline` (section 5) | `io.rs`, `engine.rs` | done |
| Session transitions, all 12 rows (section 7; backlog B1) | `session.rs`, `run.rs` | done |
| Resume heuristic after a needs-input pause (B2) | `session.rs`, `engine.rs` | done |
| Away guards (D1) | `guards.rs` | done |
| Switch controller: countdowns, input-quiet guard, verification, fallback prompts, snooze, debounce, pause (D2 to D7) | `away.rs`, `returns.rs`, `actions.rs`, `mode.rs` | done |
| Sound on return (D8) | `returns.rs` | effect emitted; playing it is the shell's job |
| Basic recap card (F1) and run statistics on it (F3) | `recap.rs` | text built; the window is the shell's job |
| Toast text (section 8) | `toast.rs` | done |
| Every config key with its default (section 11; G1) | `config.rs` | values and validation; TOML parsing not written |
| Text cleaning: ANSI and control characters stripped, lengths capped (section 6) | `events.rs` | done |
| Immersive flag: longer countdown with a warning (C4) | `away.rs`, `toast.rs` | done |
| Run records for `runs.jsonl` (E1) | `run.rs`, `hooks.rs` | record emitted as an effect; writing the file is the shell's job |
| History-based estimate (E2) | `estimator.rs` | done; the shell loads past runs and passes them in |
| Live signals raise the estimate (E3) | `estimator.rs`, `run.rs` | done |
| Declare-long hotkey (E4) | `hooks.rs`, `guards.rs` | decision done; registering the hotkey is the shell's job |
| Return queue across windows (H1) | `returns.rs` | done |
| Ignore projects by folder pattern (G5) | `glob.rs`, `hooks.rs` | done |
| LLM recap (F2) | `recap_llm.rs`, `redact.rs`, `hooks.rs` | prompt text, secret redaction, reply handling and timeout done; running the `claude` CLI is the shell's job |
| `Platform` trait, fake platform, built-in home apps (section 8) | `focus-platform` | trait and fake done; app ids unverified |

Checks run on Linux with Rust 1.97.0: `cargo fmt --check`, `cargo clippy --all-targets` with
the workspace lints from section 13 (warnings denied, pedantic, no `unwrap`, `expect`, `panic`
or `todo` outside tests), and `cargo test`: 84 tests in the first two crates, all passing; the new crates add 20 more (`cargo test --workspace`). The integration tests
replay the design's first fixture and 59 scripted scenarios through a fake clock and a fake
shell.

The V1 items were built before the MVP has run on a real machine, which is out of the
design's order. If the M2 platform spike changes the switching model, they may need rework.

## Runnable now (headless)

`focus-receiver` (M0, partly) and `focus-headless` exist. `cargo run -p focus-headless` listens
on 127.0.0.1, takes real hook POSTs, runs them through `focus-core` with a real clock and a fake
platform, and prints every effect (toasts, activation, recap, sound). Type `front <id>`, `idle
<s>`, `click`, `pause`, `resume` or `quit` to play the user and the OS. There is still no real
window switching: that needs the M2 adapters.

| Item | Where | State |
| --- | --- | --- |
| Receiver checks: path, method, Host, Origin, constant-time token, content type, 8 MB limit, rate limit | `focus-receiver/src/check.rs`, `server.rs` | done, negative tests over a real socket |
| Payload parsing, truncation, ANSI stripping, project hash | `payload.rs` | done; unknown field types read as absent |
| `config.toml` loading, every key; unknown keys are errors | `config_file.rs` | done |
| Fuzz targets, strace test, `focusctl record`, CI, repository files (rest of M0) | | not built |



- Line coverage on `focus-core`. The target is 90%; `cargo-llvm-cov` wasn't available where
  this was built, so the number is unknown.
- Performance budgets.

## Not built

Nothing below exists yet, so there is no app to run.

- **M0 (rest)** `focusctl record`, CI, and the repository files from section 13
  (SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, CODEOWNERS, THREAT_MODEL.md,
  PERMISSIONS.md, AUDIT.md, REPRODUCIBLE.md, deny.toml, workflows).
- **M2** macOS and Windows adapters and the platform spike. Needs real machines.
- **M3** the Tauri shell that wires receiver, core and platform together.
- **M4** `focusctl install-hooks`, `uninstall-hooks`, `rotate-token`.
- **M5** overlay windows, tray, wizard, launch at login, `focusctl doctor`.
- **M6** release engineering.
- Fuzz targets and the strace integration test for `focus-headless`.

Why the platform work stops here: nothing could be run on macOS or Windows. The receiver and
headless runner were added later, once crates.io was reachable.

## Before continuing

1. `rust-toolchain.toml` pins 1.97.0, the version this was checked with.
2. `Cargo.lock` is included and lists only the two workspace crates.
3. Verify the app ids in `crates/focus-platform/src/known_apps.rs` on real installs. They
   were written from memory.
4. Read the records in `docs/adr/`: they list where the design left a choice open and what
   was chosen.
5. The core's trace tests still read fixtures with a test-only JSON reader
   (`crates/focus-core/tests/support/json.rs`). `focus-receiver/tests/http.rs` parses the same
   fixture with the real parser; moving the core tests onto it would make `focus-core` depend
   on the receiver in dev, which was left undone.
