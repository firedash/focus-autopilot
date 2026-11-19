# Status

Last updated 2026-11-17.

**Where it stands:** a developer build that a person has run on a Mac and on Windows.
`focusd` takes real Claude Code hooks, decides with `focus-core`, plays a sound, shows a box,
and after a countdown brings you back to the window you prompted from, with a recap. After
your next prompt it moves you back to the app you left. It also serves a page with two tabs,
**Statistics** and **Configuration**, opened with `focusctl ui`. To run it, see
[docs/mac-quickstart.md](docs/mac-quickstart.md) or
[docs/windows-quickstart.md](docs/windows-quickstart.md).

## What has been seen working, and by whom

There are three kinds of evidence here, strongest first: a person saw it on a desktop; a CI
machine ran it on that operating system; tests passed against a fake platform on Linux. The
table is the first kind only. "Not tried" means nobody has looked, not that it fails.

| | On a Mac | On Windows |
| --- | --- | --- |
| Hooks reach `focusd` | From Claude Code in a terminal | From a local session in the Claude desktop app's Code tab: the prompt, tool uses, the finish and a late sub-agent report all arrived (2026-11-17) |
| The app in front is recognised | Yes | Yes, by executable name, as the user moved between windows |
| The toast and the recap card can be seen | Yes, in real runs | Yes, in `focusd --test-box` and in a recorded real run (`docs/images/demo.gif`, 2026-11-18) |
| The automatic return: a countdown, the switch back, the recap | Yes, from an app that is not an away app (2026-11-17) | Yes: `ACTIVATE claude.exe`, then `front: claude.exe` (2026-11-17). Whether the empty mouse event was needed is not known |
| The hotkeys in the self-test (`focusd --test-hotkeys`) | Focus and pause (2026-11-16) | Focus and pause. The first default focus key, `Ctrl+Alt+Space`, belonged to the Claude desktop app |
| The focus key during a real run | Not seen | It snoozed a return countdown |
| The move away to a configured away app, and back | Yes (2026-11-14) | Not tried |
| The move back to where you left off | Seen at least once on 2026-11-17, under a 3 second idle requirement that has since been removed. Not re-run after the two fixes that followed | Yes: the recording shows "Switching to brave in 3 s", the browser coming forward, and then the return (2026-11-18) |
| The "Go back?" box (`auto_return` off, or a refused switch) | The box was seen; the key on it was not confirmed | Not tried |
| The three-line LLM recap | Yes | Not tried |
| `focusctl install-hooks` | Yes | Yes, and `focusctl doctor` found the config, the hooks and the running `focusd` |
| The Statistics and Configuration page against a real `focusd` | Not tried | Not tried |
| Time per app as read from the real OS | Not checked | Not checked |
| Start at login | Not tried | Not built |
| The declare-long hotkey (off by default) | Not tried | Not tried |
| Claude Code in VS Code, Cursor or JetBrains | Not tried | Not tried |

**By a machine only.** On every push CI builds, lints and tests on Linux, on a macOS runner
and on a Windows runner. The Windows job also installs the hooks, starts the real
`focusd.exe`, sends it one hook and checks that it reported the front window and received
the hook. The page is checked in Chromium on Linux by `scripts/ui-check`, by hand.

**What real use found that the tests had not.** Each of these was a bug or a wrong guess
that only showed when a person ran the app: a leftover setting stopped `focusd` starting; a
rule meant to protect someone working with another Claude session also caught anyone working
in an editor or the Claude app; a sub-agent's late "finished" report cancelled the return;
`focusctl doctor` misread a closed port on Windows; and the first Windows focus key was
already taken by the Claude desktop app. All are fixed. Expect more of the same in whatever
the table says is not tried.

`docs/design.md` describes the app as built. Where the build departed from the design as
first written (`docs/design-original.md`), the decision records in `docs/adr/` say why:
[0003](docs/adr/0003-shell-decisions.md) for the shell,
[0004](docs/adr/0004-statistics-and-configuration-page.md) for the page,
[0005](docs/adr/0005-the-full-loop-by-default.md) for the loop and
[0006](docs/adr/0006-the-windows-adapter.md) for Windows. Two settings
(`switching.auto_return`, `away.resume_last_app`) turn the automatic switches off, which
gives the manual mode that was the default until 2026-11-17.

## What exists

| Part | Crate | State |
| --- | --- | --- |
| Every decision: sessions, guards, switch controller, estimator, recap text, redaction | `focus-core` | done; 97% line coverage |
| **The full loop by default**: automatic return after a countdown from any app; the away switch goes to where you left off, then to `away.apps` | `focus-core` (`returns.rs`, `guards.rs`, `engine.rs`) | done and tested against the fake platform; the return seen on a Mac and on Windows, the move back seen once on a Mac (table above) |
| Hook receiver: path, method, Host, Origin, constant-time token, content type, 8 MB limit, rate limit | `focus-receiver` | done; negative tests over a real socket |
| Payload parsing, truncation, redaction, project hash; `config.toml` loading (an unknown key is an error; a setting an older build had is skipped with a note) | `focus-receiver` | done |
| **Statistics and Configuration page**: three static files and a JSON API on the same loopback port, with Host, Origin, `Sec-Fetch-Site` and token checks and a strict Content-Security-Policy | `focus-receiver` (`ui.rs`, `assets/ui/`) | done; 47 browser checks in `scripts/ui-check` |
| **Time per app**: sampled twice a second, split by whether Claude was working, with switches and time-to-return; saved in half-hour slots | `focus-runner` (`usage.rs`) | done and tested |
| **Statistics**: totals for today and seven days, per-day and per-app breakdowns in the viewer's time zone, up to six insights | `focus-runner` (`stats.rs`) | done and tested |
| **Settings**: an explicit list of changeable settings; edits keep the file's comments; validated before writing; applied live; a hand edit of `config.toml` is picked up within two seconds | `focus-runner` (`settings.rs`, `ui.rs`) | done and tested |
| The shell loop shared by both runners | `focus-runner` | done; integration-tested against the fake platform |
| LLM recap through the user's `claude` command | `focus-runner`, `focus-core` | run on a Mac |
| Run history in `runs.jsonl`, loaded into the estimator | `focus-runner` | done |
| `focusctl`: `install-hooks`, `uninstall-hooks`, `rotate-token`, `--scope user/project/local`, `ui`, `doctor`, `install-agent`, `uninstall-agent` (macOS only) | `focusctl` | done and tested on Linux; `install-hooks` used on a Mac and on Windows, `doctor` on Windows |
| macOS adapter: front app, running apps, activation, idle time, sound, toast and recap panels | `focus-platform` | run on a Mac |
| Global hotkeys: focus, pause, declare-long; `focusd --test-hotkeys`, `--test-box` | `focus-platform` | focus and pause registered and delivered in the self-test on a Mac and on Windows; the focus key snoozed a countdown in a real run on Windows; declare-long untried |
| **Windows adapter**: front window and its executable, app windows, idle time, switching to an exact window, hotkeys, the two boxes, a sound; `focusd` on Windows | `focus-platform` (`windows.rs`, `windows_overlay.rs`), `focusd` | run on Windows by the owner (table above), and on a Windows CI machine on every push |
| Fake platform, overlay and hotkeys; headless runner | `focus-platform`, `focus-headless` | done; runs on Linux |
| CI: format, lint, test and repository rules on Linux; build, lint and test on macOS and on Windows; `cargo deny` | `.github/workflows/ci.yml` | all four jobs passing on `main` |
| SECURITY, CONTRIBUTING, CODE_OF_CONDUCT, THREAT_MODEL, PERMISSIONS, AUDIT, CODEOWNERS, pull request template, dependabot, `deny.toml` | repository root, `.github/` | written |

## Checks

Run on Linux with Rust 1.97.0:

- `cargo fmt --check`, `cargo clippy --workspace --all-targets` with the workspace lints
  (warnings denied, pedantic, no `unwrap`, `expect`, `panic` or `todo` outside tests).
- `cargo test --workspace`: 243 tests, all passing, before `focusctl setup` (7 tests added).
- `focusctl setup`: its tests use temporary files and a stand-in for focusd, never the real
  `~/.claude/settings.json`. Run end to end once on Windows (hooks written, focusd started in the
  background, second run changed nothing). Not run on macOS: the macOS start path (own process
  group, no terminal) is type-checked only, and a closed terminal's hang-up signal is untested.
- `cargo clippy --workspace --all-targets` again with `--target aarch64-apple-darwin` and
  with `--target x86_64-pc-windows-msvc`: the macOS and Windows code type-checks and is
  lint-clean.
- `cargo deny check`: advisories, bans, licenses and sources pass.
- `python3 scripts/check-repo.py`: required files, SPDX headers, a DEPENDENCIES.md entry for
  every dependency, `unsafe` confined to `focus-platform`.
- `scripts/ui-check/run.sh`: 47 checks of the page in Chromium, all passing.

On GitHub Actions (`.github/workflows/ci.yml`), on every push: the Linux checks above, a build,
lint and test run on a macOS runner (the first place the macOS adapter is actually linked),
the same on a Windows runner (where the Windows adapter's calls are made against a real Windows),
and `cargo deny`. All four jobs pass on `main`. The browser checks are not in CI: it has no
browser set up.

Measured with `cargo llvm-cov` on Linux on 2026-11-17: **97.3% line coverage on
`focus-core`** (the design's target is 90%), 89.9% across the workspace. The lowest file in
`focus-core` is `recap_llm.rs` at 81%. The macOS and Windows adapters are not compiled on
Linux, so they are not in these numbers.

## Measured against the design's own targets

- **Size.** The design aimed for under 6,000 lines of first-party code for the MVP, excluding
  tests. The code is now about 11,000 lines of Rust and 1,750 of HTML, CSS and JavaScript.
  It is well over, because V1 items (estimator, LLM recap, history), the page, the automatic
  loop and a second platform adapter were built.
  The split by crate still lets an auditor read the security-relevant parts first.
- **Performance budgets.** Not measured.
- **Fuzzing, the `strace` test, signed releases, `focusctl audit`.** Not built.

## Which Claude Code surfaces

Two have been run: Claude Code in a terminal on a Mac, and a local session in the Claude
desktop app's Code tab on Windows. Anthropic's docs say Claude Code fires the same hooks
wherever it runs, so one install should cover the others, but those are unverified here. Each
also needs its app in the home-app list; `focusd` prints the line to add, and the
Configuration tab has a picker for it.

| Surface | Tested | What is known |
| --- | --- | --- |
| Terminal | yes, on a Mac | works. Not tried on Windows |
| VS Code, Claude Code extension | no | docs: shares `~/.claude/settings.json` with the CLI |
| Cursor, same extension or `claude` in its terminal | no | docs offer the extension for Cursor; Cursor's own agent is not supported |
| Claude Desktop, Code tab, local session | yes, on Windows | the prompt, tool uses, the finish and a sub-agent's end all arrived, the window was recognised as `claude.exe`, and the return brought it forward (2026-11-17). Approval events and the Mac app are untried |
| Munder Difflin, a third-party app that runs Claude Code | yes, on Windows | hooks arrive and the whole loop works (2026-11-18). That run had `Munder Difflin.exe` added under `[home] extra_apps`; it is on the built-in list since, which has not been re-run. The project on the box is the folder that app runs Claude in, not the repo |
| Claude Desktop, Cowork | yes, on Windows: no hooks | nothing reached `focusd` (2026-11-17); not supported |
| JetBrains | no | not mentioned in the hooks docs; unknown |
| Cloud sessions | n/a | not supported: no local hooks |

On a Mac a return goes to the app, not the window: with several sessions in one app you land
in whichever window macOS raises, and the project name on the box says which session is
waiting. On Windows a return goes to the exact window.

The Claude desktop app has a notification of its own when a session finishes. On Windows it
showed the session's name and a one-line summary, and a click on it opened the app
(2026-11-17). For someone using the desktop app, that already does much of what this app's
return does; what this app adds is the return with no click, the move back to where you
left off, and the statistics.

## Not built

- **Windows beyond the adapter**: start at login is not built, and the LLM recap is untried
  there.
- **Menu-bar icon, click on a toast, first-run wizard, installer.** `focusd` runs in a
  terminal or from a launch agent, and the page is opened with `focusctl ui`.
- **Exact-window return on macOS.** The editor-URL route was tried and withdrawn; see ADR 0003 items
  16 and 21 for what would be needed. Decided on 2026-11-19 to leave it: one window per app
  is fine for now. The Accessibility route, what it would take and why it was postponed, is
  in ADR 0006, "Left for later".
- **Changing hotkeys without a restart.**
- **Statistics per project.** History keeps only a salted hash of the folder.
- **`focusctl record`, `focusctl audit`, fuzz targets, the `strace` test, REPRODUCIBLE.md,
  signing, an SBOM and attestations.**
- **Prebuilt downloads: written, never run.** `.github/workflows/release.yml` builds unsigned
  archives with SHA-256 files for macOS arm64, macOS x64 and Windows x64 on a version tag. It has
  not run on GitHub; only its packaging commands and version check were tried locally.
  See [docs/releasing.md](docs/releasing.md).
- **Front-app change notifications.** The front app is polled twice a second.
- Known redaction gaps: a secret in plain prose, and attached short flags (`mysql -pSecret`).

## Next steps, in order

1. Use it for a week of ordinary work, on either machine, next to Claude's own
   notifications, and see whether the automatic switching is missed when it is off. That
   answers whether the rest of this list is worth doing.
2. Re-run the move back to where you left off on a Mac: send a second prompt, stay in the
   Claude window, and check that you are moved to the app you were in. On Windows it has
   been recorded working.
3. On a Mac, press the focus key during a return countdown, and on the "Go back?" box with
   `switching.auto_return = false`.
4. Run `focusctl ui` against a real `focusd` and check the numbers against a day of use.
5. Test VS Code and Cursor, and correct the app ids in
   `crates/focus-platform/src/known_apps.rs`, which were written from memory. Real runs
   have confirmed four: `com.apple.Terminal`, `com.microsoft.VSCode`, `claude.exe` and
   `WindowsTerminal.exe`.
6. An install that needs no Rust toolchain. The recorded demo is done.

## Before continuing

1. `rust-toolchain.toml` pins 1.97.0, the version this was checked with.
2. Read the records in `docs/adr/`: they list where the design left a choice open and what
   was chosen, including two approaches that were tried and withdrawn.
3. The core's trace tests still read fixtures with a test-only JSON reader
   (`crates/focus-core/tests/support/json.rs`). `focus-receiver/tests/http.rs` parses the same
   fixture with the real parser.
