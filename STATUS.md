# Status

Last updated 2026-11-17.

**Windows:** there is a Windows build as of today. It has passed its tests on a Windows CI
machine. Seen on a person's Windows desktop so far: `focusd` starts and `focusctl doctor`
finds it, the config and the hooks; it names the window in front as the user moves between
windows; a prompt sent from a local session in the Claude desktop app's Code tab arrives, as
does the hook when Claude finishes; after a countdown Windows let `focusd` bring the Claude
window forward (`ACTIVATE claude.exe`, then `front: claude.exe`), and the focus key snoozed a
countdown in a real run; in the hotkey self-test the pause key and the focus key
(`Ctrl+Alt+Shift+Space`; the first default, `Ctrl+Alt+Space`, belongs to the Claude desktop
app) register and their presses are delivered; and in `focusd --test-box` both the toast and
the recap card are visible at the top of the screen. The owner then ran the loop with
nothing else going on and reported that it works as expected. Whether that run included the
move away after the next prompt was not said.
See [docs/windows-quickstart.md](docs/windows-quickstart.md).

**Where it stands:** a developer build runs on a Mac. `focusd` takes real Claude Code hooks,
decides with `focus-core`, shows on-screen boxes, plays a sound and brings you back after a
countdown. After your next prompt it moves you back to the app you left off in. It also
serves a page with two tabs, **Statistics** and **Configuration**, opened
with `focusctl ui`. See [docs/mac-quickstart.md](docs/mac-quickstart.md) to run it.

**What has and has not been seen working.** The distinction matters here, because several
macOS parts were written from Linux:

| Seen working on a real Mac | Verified on Linux only | Compiles for macOS, never run |
| --- | --- | --- |
| Hooks from Claude Code in a terminal | Everything in `focus-core`, the receiver, `focusctl` | The focus key acting on a real toast during a run (the key itself is delivered; see left) |
| Switching away and back | The Statistics and Configuration page, in Chromium | `focusctl ui` opening the browser with `open` |
| The toast and recap boxes | The whole shell loop, against the fake platform | The launch agent being loaded by `launchd` |
| The three-line LLM recap | `focusctl doctor`, `ui --no-open`, `--scope` | Time per app as read from real macOS |
| Global hotkeys: `focusd --test-hotkeys` registered the focus and pause keys and printed each press (2026-11-16) | The away switch to where you left off (ADR 0005) | The declare-long hotkey (off by default) |
| The automatic return: a countdown, then the switch back to the terminal, from an app that is not an away app (2026-11-17) | Snoozing a return countdown with the focus key | |
| | | Claude Code in VS Code, Cursor, the Desktop app |

Where the build departs from `docs/design.md`, the ADRs win:
[0003](docs/adr/0003-shell-decisions.md) for the shell and
[0004](docs/adr/0004-statistics-and-configuration-page.md) for the page and
[0005](docs/adr/0005-the-full-loop-by-default.md) for the loop. The loop as of 0005: a
finished run **brings you back by itself after a 5 second countdown**, from any app, and the
next long prompt **moves you back to the app you left off in**. Two settings
(`switching.auto_return`, `away.resume_last_app`) turn these off, which gives the manual mode
that was the default until 2026-11-17. "Switching away and back" in the table above was seen
on a Mac with a configured away app. The automatic return from another app has now been seen
there too; the away switch to where you left off has not.

## What exists

| Part | Crate | State |
| --- | --- | --- |
| Every decision: sessions, guards, switch controller, estimator, recap text, redaction | `focus-core` | done; 97% line coverage |
| **The full loop by default**: automatic return after a countdown from any app; the away switch goes to where you left off, then to `away.apps` | `focus-core` (`returns.rs`, `guards.rs`, `engine.rs`) | done and tested against the fake platform; not run on a Mac |
| Hook receiver: path, method, Host, Origin, constant-time token, content type, 8 MB limit, rate limit | `focus-receiver` | done; negative tests over a real socket |
| Payload parsing, truncation, redaction, project hash; `config.toml` loading (an unknown key is an error; a setting an older build had is skipped with a note) | `focus-receiver` | done |
| **Statistics and Configuration page**: three static files and a JSON API on the same loopback port, with Host, Origin, `Sec-Fetch-Site` and token checks and a strict Content-Security-Policy | `focus-receiver` (`ui.rs`, `assets/ui/`) | done; 47 browser checks in `scripts/ui-check` |
| **Time per app**: sampled twice a second, split by whether Claude was working, with switches and time-to-return; saved in half-hour slots | `focus-runner` (`usage.rs`) | done and tested |
| **Statistics**: totals for today and seven days, per-day and per-app breakdowns in the viewer's time zone, up to six insights | `focus-runner` (`stats.rs`) | done and tested |
| **Settings**: an explicit list of changeable settings; edits keep the file's comments; validated before writing; applied live; a hand edit of `config.toml` is picked up within two seconds | `focus-runner` (`settings.rs`, `ui.rs`) | done and tested |
| The shell loop shared by both runners | `focus-runner` | done; integration-tested against the fake platform |
| LLM recap through the user's `claude` command | `focus-runner`, `focus-core` | run on a Mac |
| Run history in `runs.jsonl`, loaded into the estimator | `focus-runner` | done |
| `focusctl`: `install-hooks`, `uninstall-hooks`, `rotate-token`, `--scope user/project/local`, `ui`, `doctor`, `install-agent`, `uninstall-agent` | `focusctl` | done and tested on Linux |
| macOS adapter: front app, running apps, activation, idle time, sound, toast and recap panels | `focus-platform` | run on a Mac |
| Global hotkeys: focus, pause, declare-long; `focusd --test-hotkeys` | `focus-platform` | focus and pause registered and delivered on a Mac in the self-test; not yet seen acting on a toast in a real run; declare-long untried |
| **Windows adapter**: front window and its executable, app windows, idle time, switching to an exact window, hotkeys, the two boxes, a sound; `focusd` on Windows | `focus-platform` (`windows.rs`, `windows_overlay.rs`), `focusd` | built; on a Windows CI machine it reads the front window, lists windows, registers a hotkey and shows and hides the boxes without taking focus. Never used by a person |
| Fake platform, overlay and hotkeys; headless runner | `focus-platform`, `focus-headless` | done; runs on Linux |
| CI: format, lint, test and repository rules on Linux; build, lint and test on macOS and on Windows; `cargo deny` | `.github/workflows/ci.yml` | passing on `main`, including a real macOS build, link and test run |
| SECURITY, CONTRIBUTING, CODE_OF_CONDUCT, THREAT_MODEL, PERMISSIONS, AUDIT, CODEOWNERS, pull request template, dependabot, `deny.toml` | repository root, `.github/` | written |

## Checks

Run on Linux with Rust 1.97.0:

- `cargo fmt --check`, `cargo clippy --workspace --all-targets` with the workspace lints
  (warnings denied, pedantic, no `unwrap`, `expect`, `panic` or `todo` outside tests).
- `cargo test --workspace`: 243 tests, all passing.
- `cargo clippy --workspace --all-targets --target aarch64-apple-darwin`: the macOS code
  type-checks and is lint-clean.
- `cargo deny check`: advisories, bans, licenses and sources pass.
- `python3 scripts/check-repo.py`: required files, SPDX headers, a DEPENDENCIES.md entry for
  every dependency, `unsafe` confined to `focus-platform`.
- `scripts/ui-check/run.sh`: 47 checks of the page in Chromium, all passing.

On GitHub Actions (`.github/workflows/ci.yml`), on every push: the Linux checks above, a build,
lint and test run on a macOS runner (the first place the macOS adapter is actually linked),
the same on a Windows runner (where the Windows adapter's calls are made against a real Windows),
and `cargo deny`. All four jobs pass on `main`. The browser checks are not in CI: it has no
browser set up.

Measured with `cargo llvm-cov`: **97.0% line coverage on `focus-core`** (the design's target
is 90%), 89.8% across the workspace. The lowest file in `focus-core` is `recap_llm.rs` at 81%.

## Measured against the design's own targets

- **Size.** The design aimed for under 6,000 lines of first-party code for the MVP, excluding
  tests. The code is now about 9,600 lines of Rust and 1,700 of HTML, CSS and JavaScript.
  It is over, mostly because V1 items (estimator, LLM recap, history) and the page were built.
  The split by crate still lets an auditor read the security-relevant parts first.
- **Performance budgets.** Not measured.
- **Fuzzing, the `strace` test, signed releases, `focusctl audit`.** Not built.

## Which Claude Code surfaces

Only the terminal has been tested. Anthropic's docs say Claude Code fires the same hooks
wherever it runs, so one install should cover other surfaces, but that is unverified here.
Each also needs its app in the home-app list; `focusd` prints the line to add, and the
Configuration tab has a picker for it.

| Surface | Tested | What is known |
| --- | --- | --- |
| Terminal | yes | works |
| VS Code, Claude Code extension | no | docs: shares `~/.claude/settings.json` with the CLI |
| Cursor, same extension or `claude` in its terminal | no | docs offer the extension for Cursor; Cursor's own agent is not supported |
| Claude Desktop, Code tab, local session | partly, on Windows | the prompt and the finish hooks arrived and the window was recognised as `claude.exe` (2026-11-17); approval events and the Mac app untried |
| JetBrains | no | not mentioned in the hooks docs; unknown |
| Cloud sessions | n/a | not supported: no local hooks |

Returns go to the app, not the window. With several sessions in one app the key lands in the
app; the project name on the toast says which session is waiting.

## Not built

- **Windows beyond the adapter**: start at login, and the LLM recap is untried. The adapter
  itself is built (ADR 0006) and passes its tests on a Windows CI machine, but no person has
  used it on a Windows desktop.
- **Menu-bar icon, click on a toast, first-run wizard, installer.** `focusd` runs in a
  terminal or from a launch agent, and the page is opened with `focusctl ui`.
- **Exact-window return.** The editor-URL route was tried and withdrawn; see ADR 0003 items
  16 and 21 for what would be needed.
- **Changing hotkeys without a restart.**
- **Statistics per project.** History keeps only a salted hash of the folder.
- **`focusctl record`, `focusctl audit`, fuzz targets, the `strace` test, release
  engineering, REPRODUCIBLE.md.**
- **Front-app change notifications.** The front app is polled twice a second.
- Known redaction gaps: a secret in plain prose, and attached short flags (`mysql -pSecret`).

## Next steps, in order

1. Finish running the loop on a Mac. The countdown bringing you back has been seen. Still to
   see: send a second prompt, leave the keyboard alone, and check that you are moved back to
   the app you were in; press the focus key during a return countdown to snooze it; and with
   `switching.auto_return = false`, press the focus key on the "Go back?" box.
2. Run `focusctl ui` on a Mac and check that the page opens and the numbers look right
   against a day of real use.
3. Test VS Code, Cursor and the Claude Desktop Code tab, and correct the app ids in
   `crates/focus-platform/src/known_apps.rs`, which were written from memory.
4. Decide whether to build a menu-bar icon, now that there is a page for it to open.

## Before continuing

1. `rust-toolchain.toml` pins 1.97.0, the version this was checked with.
2. Read the records in `docs/adr/`: they list where the design left a choice open and what
   was chosen, including two approaches that were tried and withdrawn.
3. The core's trace tests still read fixtures with a test-only JSON reader
   (`crates/focus-core/tests/support/json.rs`). `focus-receiver/tests/http.rs` parses the same
   fixture with the real parser.
