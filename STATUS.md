# Status

Last updated 2026-11-14.

## Built and tested

`focus-core`, the M1 milestone: the code every other part will call.

| Design item | Where | State |
| --- | --- | --- |
| Core contract: `Input`, `Effect`, `Core::handle/tick/next_deadline` (section 5) | `io.rs`, `engine.rs` | done |
| Session transitions, all 12 rows (section 7; backlog B1) | `session.rs` | done |
| Resume heuristic after a needs-input pause (B2) | `session.rs`, `engine.rs` | done |
| Away guards (D1) | `guards.rs` | done |
| Switch controller: countdowns, input-quiet guard, verification, fallback prompts, snooze, debounce, pause (D2 to D7) | `away.rs`, `returns.rs`, `mode.rs` | done |
| Sound on return (D8) | `returns.rs` | effect emitted; playing it is the shell's job |
| Basic recap card (F1) | `recap.rs` | text built; the window is the shell's job |
| Toast text (section 8) | `toast.rs` | done |
| Every config key with its default (section 11; G1) | `config.rs` | values and validation; TOML parsing not written |
| Elapsed-time away threshold, tier table (section 9, layer 0) | `estimator.rs` | done; layers 1 to 3 are V1 |
| Text cleaning: ANSI and control characters stripped, lengths capped (section 6) | `events.rs` | done |
| `Platform` trait, fake platform, built-in home apps (section 8) | `focus-platform` | trait and fake done; app ids unverified |

Checks run on Linux with Rust 1.97.0: `cargo fmt --check`, `cargo clippy --all-targets` with
the workspace lints from section 13 (warnings denied, pedantic, no `unwrap`, `expect`, `panic`
or `todo` outside tests), and `cargo test`: 55 tests, all passing. The integration tests
replay the design's first fixture and 42 scripted scenarios through a fake clock and a fake
shell.

## Not measured

- Line coverage on `focus-core`. The target is 90%; `cargo-llvm-cov` wasn't available where
  this was built, so the number is unknown.
- Performance budgets.

## Not built

Nothing below exists yet, so there is no app to run.

- **M0** `focus-receiver` (HTTP receiver, payload parsing, threat-model checks), config
  loading from TOML, `focusctl record`, CI, and the repository files from section 13
  (SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, CODEOWNERS, THREAT_MODEL.md,
  PERMISSIONS.md, AUDIT.md, REPRODUCIBLE.md, deny.toml, workflows).
- **M2** macOS and Windows adapters and the platform spike. Needs real machines.
- **M3** the Tauri shell that wires receiver, core and platform together.
- **M4** `focusctl install-hooks`, `uninstall-hooks`, `rotate-token`.
- **M5** overlay windows, tray, wizard, launch at login, `focusctl doctor`.
- **M6** release engineering.
- `focus-headless`, fuzz targets, the strace integration test.

Why it stops here: the first part was built in a Linux sandbox with no access to crates.io,
so nothing that needs a dependency (hyper, serde, toml, tauri, objc2, windows) could be
compiled, and nothing could be run on macOS or Windows. `focus-core` has no dependencies by
design, which is why it could be finished and tested.

## Before continuing

1. `rust-toolchain.toml` pins 1.97.0, the version this was checked with.
2. `Cargo.lock` is included and lists only the two workspace crates.
3. Verify the app ids in `crates/focus-platform/src/known_apps.rs` on real installs. They
   were written from memory.
4. Read `docs/adr/0001-core-decisions.md`: it lists where the design left a choice open and
   what was chosen.
5. The trace tests read fixtures with a small test-only JSON reader
   (`crates/focus-core/tests/support/json.rs`). Once `focus-receiver` exists, load fixtures
   through its parser instead so the tests exercise the real one.
