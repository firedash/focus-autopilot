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
