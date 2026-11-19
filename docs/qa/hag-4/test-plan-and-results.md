# HAG-4: first-run test plan and results (Windows, main at 9bb4ab1)

Baseline: `docs/windows-quickstart.md`. Run 2026-11-19 on Windows 11 Home 10.0.22631, 1920x1080, one monitor, Rust 1.100.0 (the task said 1.97.0; `rust-toolchain.toml` did not block the build).

## How it was run (read this first)

The owner's machine already has Focus Autopilot installed and running: 11 hooks in `~/.claude/settings.json`, a `config.toml`, and `focusd.exe` (pid 30604) listening on port 47821. A true fresh install on the real paths would have replaced the owner's token and removed their hooks at the end, so it would not have left the machine as found.

So the plan ran in a **sandbox**: `HOME`/`USERPROFILE` and `FOCUS_AUTOPILOT_HOME` (both read by `crates/focus-runner/src/paths.rs`) pointed at a scratch folder, its own `settings.json` (`{"theme":"dark"}`), port 47999 and hotkeys `Ctrl+Alt+Shift+7` / `+8`. This is a new-user flow in every respect except the paths, the port and the keys.

The real files were still backed up first and compared afterwards. SHA-256 before and after are identical for `~/.claude/settings.json`, `~/.claude/settings.json.focus-autopilot.bak` and the real `config.toml`. The owner's `focusd` (pid 30604) was still running at the end. No real Claude prompt was sent. No switch was made on live windows. No `focusd` command ran with the real config.

## Plan and results

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 1 | `cargo build --release -p focusd -p focusctl` | builds | PASS, 1m05s |
| 2 | `focusctl doctor` before anything | fails with fixes | PASS: three FAILs, each with a fix line (see F2 for one wrong cause) |
| 3 | `focusctl install-hooks --dry-run` | diff shown, nothing written | PASS: diff printed, no config or backup written |
| 4 | `focusctl install-hooks` | merges, backs up, creates config and token | PASS: `settings.json`, `.focus-autopilot.bak`, `config.toml` created; `theme` kept |
| 5 | `focusd --test-hotkeys`, default keys | registers | PASS as an environment effect: "another program already uses this key combination" for both, because the owner's `focusd` holds them. The message is accurate |
| 6 | `focusd --test-hotkeys`, free keys, keys sent with `keybd_event` | a line per press | PASS: `pressed: focus key`, `pressed: pause key` |
| 7 | `focusd --test-box` | box 10 s, recap card 6 s, top centre | PASS: both seen and captured (`test-box-toast.png`, `test-box-recap.png`; cropped to the box so no desktop content is in them) |
| 8 | start `focusd`, then `focusctl doctor` | all ok | PASS: "Everything checked is in order" |
| 9 | POST a synthetic `UserPromptSubmit` (not to Claude) with a wrong and a right token | 401, then 200 | PASS: 401, then 200; log shows `hook: UserPromptSubmit`, `log: run_started`, and the "not a home app" note for explorer.exe |
| 10 | stop sandbox `focusd`, `focusctl uninstall-hooks` | settings back to `{"theme":"dark"}` | **FAIL, F1**, but only after the port was changed between two installs |
| 11 | real files unchanged | identical hashes | PASS |

## Failures and findings

**F1. Hooks left behind after the port changes (confirmed, minor).**
Steps: fresh `settings.json`; `install-hooks` with the default port 47821; set `[receiver] port = 47999` in `config.toml`; `install-hooks` again; `uninstall-hooks`.
Expected: no Focus Autopilot entries left ("removes exactly these entries and nothing else", `docs/hooks.md`), or at least the old ones replaced on the second install.
Happened: the second install added 11 new entries for 47999 beside the 11 for 47821. `uninstall-hooks` removed only the 47999 ones, so 11 entries with the old URL and old token stayed.
Cause (read from `crates/focusctl/src/hooks.rs`): `is_ours` matches by URL prefix, which includes the port, so entries for another port are "not ours" (the unit test `a_hook_on_another_port_is_not_ours` pins this). Effect: dead hooks that each time out after 2 s on every event, and the token stays in `settings.json`. Not seen by a user who never changes the port.

**F2. `doctor` blames the wrong thing when another `focusd` owns the port (minor).**
Steps: with an unrelated `focusd` already on 47821 and a fresh data folder, run `focusctl doctor`.
Expected: say another instance, or a different token, is answering.
Happened: "focusd on port 47821 refuses the token in config.toml. restart focusd: it was started before the token last changed". Restarting is the right action only if it is the same install.

**F3. Function keys are not accepted as hotkeys (observation).**
`focus_key = "Ctrl+Alt+Shift+F9"` gives "`F9` in `Ctrl+Alt+Shift+F9` is not a key I know". Clear message, but `windows-quickstart.md` tells the user to "pick another under `[hotkeys]`" without saying which keys are allowed. The setting is `pause`, not `pause_key` (the error was clear).

## Not verified

- A real Claude Code or Claude desktop session sending real hooks (no real prompt was allowed).
- The countdown, switch-away and return on live windows (not allowed without asking).
- The sound being heard. The log and boxes were checked, not the audio.
- A first install on the real paths, and `uninstall-hooks` against the owner's real `settings.json` (see "How it was run").
- Multiple monitors, display scaling other than 100%, a restart of the Claude desktop app.
- `focusctl ui` and the Statistics tab.
- `--test-box` was run while a full-screen game was in front. The box showed over it.

---

# Part 2: `focusctl setup` (feat/focusctl-setup, 071a6f9) and the release workflow (feat/release-binaries, 99f0e5d)

Run 2026-11-19, same machine and same rules as part 1. Same sandbox method: `HOME`/`USERPROFILE` and `FOCUS_AUTOPILOT_HOME` redirected to scratch folders, ports 47997/47998/47999, hotkeys `Ctrl+Alt+Shift+7`/`+8` except in scenario 8. The owner's `settings.json`, `config.toml` and `focusd` (pid 30604) were not touched; pid 30604 was still running at the end. Built from detached worktrees of the two commits; nothing in them was edited. Every sandbox `focusd` I started was stopped afterwards.

## `focusctl setup`: scenarios

| # | Scenario | Expected | Result |
| --- | --- | --- | --- |
| 1 | First run: fresh settings (`{"theme":"dark"}`), config with only a port and hotkeys | hooks added, `focusd` started in the background, checks run, "Ready" | PASS. `1/3 hooks: added`, `2/3 focusd: started (path, log path)`, four `ok` checks, `Ready: ...`, exit 0. `focusd` kept running after `focusctl` exited. `theme` kept, `.bak` made |
| 2 | Second run, `focusd` already running | no change, no second `focusd` | PASS. `hooks: already in ..., nothing changed`, `focusd: already running on port 47999`, exit 0, one listener on the port |
| 3 | Hooks installed, `focusd` stopped, run again | starts `focusd`, leaves hooks alone | PASS (output to a file). See F4 for the same run with output piped |
| 4 | Port taken by another program (a plain TCP listener on 47998) | not ready, says what to do, starts nothing | PASS on verdict, exit 1, `FAIL something other than focusd is answering on port 47998`. No `focusd` started. Wording problem in step 2, see F5 |
| 5 | `focusd` missing (lone `focusctl.exe` in an empty folder, none on PATH) | not ready, says how to get it | PASS: `not started: focusd was not found next to focusctl or on the PATH; build it with cargo build...`, exit 1. Also PASS with `--focusd C:/nope/focusd.exe`: clear `could not start` error, exit 1 |
| 6 | `setup --dry-run` | refuse or preview | PASS: refuses and points to `install-hooks --dry-run`, exit 1 |
| 7 | `uninstall-hooks` after setup | settings back to `{"theme":"dark"}` | PASS (single port, so F1 does not apply) |
| 8 | Default hotkeys already held by another program | start, but say so | `Ready`, exit 0. See F6 |

### Failures and findings

**F4. `focusctl setup` never returns when its output is piped (confirmed, medium).**
Steps: `focusctl setup | cat` (or `| grep`, or any caller that reads its output through a pipe, such as `Tee-Object`, an editor task runner, or a script capturing output) while `focusd` is not running.
Expected: setup prints its result and exits, as it does when the output goes to a console or a file.
Happened: the command prints `focusd: started`, then never ends. It stayed hung until my tool's 90 s limit, twice. Redirecting to a file (`> out.txt`) returns at once. `focusd` itself is fine and keeps running.
Likely cause (not proven): `spawn_detached` (`crates/focusctl/src/setup.rs:42`) sets the child's stdin, stdout and stderr, but on Windows `CreateProcess` is called with handle inheritance on, so the child also inherits the parent's pipe handle and holds the pipe open, so the reader never sees the end. Check by listing the child's handles, or by running with output to a pipe and the child's handle inheritance turned off.
Who is hit: anyone running `.\target\release\focusctl.exe setup 2>&1 | Tee-Object setup.log`, which is the pattern `windows-quickstart.md` itself uses for `focusd`. Not hit in a plain terminal. I did not test the pipe case on macOS.

**F5. Wrong wording when the port is held by something else (minor).**
Steps: scenario 4. Expected: step 2 does not claim `focusd` is there. Happened: `2/3 focusd: already running on port 47998`, then `3/3` correctly says something else is answering. The final line and the fix are right. The suggested fix, "run `focusctl install-hooks`", is the sequence that triggers F1 (old-port hooks left behind).

**F6. `Ready` is reported when no hotkey could be registered (minor).**
Steps: scenario 8 (default keys already used by another program). Expected: at least a note, because without a focus key the user can only act on a box by typing `click` into the `focusd` window, which a background `focusd` does not have. Happened: `Ready: focusd is running in the background`, exit 0. The problem is only in `focusd.log` (`hotkey problem: ... another program already uses this key combination`, `no focus hotkey: type click here to act on a toast`). `doctor` and `setup` both say hotkeys are not checked.

## Release workflow

Read `.github/workflows/release.yml` and `docs/releasing.md`, then ran the parts that run on Windows:

| Step | Result |
| --- | --- |
| Tag-versus-Cargo.toml check (the `sed` and the `%%-*` comparison) | PASS in Git Bash: Cargo version `0.1.0`; `v0.1.0` and `v0.1.0-rc1` accepted, `v9.9.9` rejected |
| `cargo build --release --locked -p focusd -p focusctl --target x86_64-pc-windows-msvc` | PASS, 46 s |
| Archive step (the workflow's PowerShell, minus the `gh upload` line) | PASS under Windows PowerShell 5.1: zip made (1.37 MB), five files inside: `focusctl.exe`, `focusd.exe`, `LICENSE`, `PERMISSIONS.md`, `README.md` |
| Checksum file and `sha256sum -c` | PASS: `<hash>  <file>`, `OK`. The layout matches `docs/releasing.md` |
| Unzip and run | PASS: `focusctl.exe doctor` and `focusd.exe --help` run from the extracted folder; the extracted `.exe` files have the same SHA-256 as the built ones |

Observations (nothing failed):
- **R1, not verified:** GitHub runs this step with `pwsh` 7; this machine has only Windows PowerShell 5.1 (`pwsh` is not installed). Under 5.1 the zip entry names use backslashes (`folder\focusd.exe`). Windows tools read that; I did not check how `pwsh` 7 names them, nor `unzip` on Linux.
- **R2, doc gap:** the archive has `README.md` only, and its relative links (`docs/windows-quickstart.md`, `STATUS.md`) do not exist in the zip. `docs/releasing.md` explains how to check a download but not what to do with it next. With the setup branch, `focusctl setup` finds a `focusd` beside it, so "unzip, run `focusctl setup`" would fit, but neither page says so. I did not test whether a user in a downloaded folder (no repo) hits other path assumptions beyond the ones in scenario 5.
- **Cannot run here:** the `draft` job, `gh release create/upload`, the macOS builds (Apple silicon and the Intel cross-compile), the tag trigger itself, and the `--prerelease` branch.

## Not verified (part 2)

- Real Claude sessions and hooks, and the countdown or switch on live windows (still not allowed).
- `setup` on macOS, in a pipe on macOS, and with `focusd` started from a path with spaces.
- `setup` against the owner's real `settings.json` and port 47821 (sandbox only).
- Screenshots: nothing here is graphical. The evidence is the command output quoted above; the outputs are in the scratchpad, not committed.

---

# Part 3: recheck of F1 to F3 on fix/first-run-bugs (c22afbe)

Run 2026-11-19. Built from a detached worktree of c22afbe (F1 61a519c, F2 b05d5db, F3 c22afbe); same sandbox method and boundaries as parts 1 and 2. Owner's files and `focusd` (pid 30604) untouched; every sandbox `focusd` stopped. `uninstall-hooks` no longer takes a port, so it was run without one.

| Finding | Result | What I ran and saw |
| --- | --- | --- |
| **F1** hooks left after a port change | **PASS** | Fresh `{"theme":"dark"}`. `install-hooks` (default port): 11 lines for 47821. Set `port = 47999`, `install-hooks` again: 11 lines for 47999, 0 for 47821, one `Stop` entry, so the old entries are replaced, not duplicated. `uninstall-hooks`: settings are exactly `{"theme":"dark"}`. Extra check for over-removal: a settings file with a foreign `Stop` group (`http://127.0.0.1:5000/hook`, `http://127.0.0.1:9000/other`, and a command hook) came back unchanged after `install-hooks` then `uninstall-hooks` |
| **F2** `doctor` blames the wrong cause | **PASS** | Two sandboxes on port 47999: one `focusd` running with token A, a second data folder with token B. Doctor for B: `a focusd on port 47999 refuses the token in config.toml` with the fix "either your focusd, started before the token last changed (restart it), or another Focus Autopilot, from another install or data folder, that owns the port (stop it, or set a different port ..., run `focusctl install-hooks`, and start focusd)". Matching token: `ok ... accepts the token`. Nothing listening: the old "not running" message is unchanged |
| **F3** which hotkeys are valid | **PASS** with a note | Tried the quickstart's list through `focusd --test-hotkeys`. Registered: letters (`B`, `Z`), digits, `Return`, `Enter`, `Escape`, `Esc`, modifiers `Control`, `Option`, `Cmd`, `Super`, `Meta`. `Space` and `Tab` parse; each was reported as already used by another program on this machine (Space is the owner's `focusd`; I did not identify the Tab owner). Not accepted, with the documented message: `F9`, `Up`, `-`; `B` alone gives `needs at least one modifier`; `Win` is not a key name (the doc does not list it). Setting the same chord twice, or `Return` with `Enter`, reports the second as already used, which is correct since they are the same chord |

Note on F3: the doc says `Command` (or `Cmd`, `Super`, `Meta`) is "the Windows key on Windows". I confirmed `Cmd+Ctrl+Alt+Q` and `Super+Ctrl+Alt+W` register. I did not press them, so I did not verify that they are the Windows key.

Not verified: F1 on macOS, and over-removal for hooks whose URL is on `127.0.0.1:47821/hook` but were added by hand by the user (those are indistinguishable from ours by design).
