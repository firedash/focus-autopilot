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

---

# Part 4: recheck of F4 to F6 (feat/focusctl-setup 0833111) and the release archive (feat/release-binaries 73afbdd)

Run 2026-11-19. Built from detached worktrees of the two commits (F4 0aaf44d, F5 4aee042, F6 0833111). Same sandbox method and boundaries as before; the owner's files and `focusd` (pid 30604) were untouched and still running at the end; every sandbox `focusd` I started was stopped. Each pipe test ran under a 25 s guard so a hang would be reported, not waited on.

| Finding | Result | What I ran and saw |
| --- | --- | --- |
| **F4** `setup` hangs when output is piped | **PASS** | `focusd` not running, hooks absent, each run returned in about 3 s, printed `focusd: started` and `Ready`, and left `focusd` listening (`doctor`: `focusd is running ... accepts the token`). Readers tried: `\| cat` (Git Bash); `2>&1 \| Tee-Object file` and `\| Select-String Ready` (Windows PowerShell 5.1); `2>&1 \| findstr Ready` and `\| more` (cmd, from a `.cmd` file); `$(focusctl setup)` captured by a script. The same steps hung twice on 071a6f9 |
| **F5** wrong wording when another program holds the port | **PASS** | A plain TCP listener on 47998. Step 2 now reads `focusd: not started, because something else is already using port 47998 (details below)`; no `focusd` started; step 3 `FAIL something other than focusd is answering on port 47998`; exit 1. The fix hint now reads `set a different port under [receiver] in config.toml, then run focusctl setup, which writes the hooks for the new port and starts focusd`. It no longer sends the user to `install-hooks`; with the F1 fix that sequence is safe either way. I did not run the "change the port, run setup again" step this time |
| **F6** `Ready` while no hotkey registered | **PASS** | Default keys, held by the owner's `focusd`. Exit 0, a `note` under the checks naming both keys and the reason, the advice "stop focusd, pick other keys under [hotkeys] (the quickstart lists the valid ones), and run setup again. Until then the box cannot be acted on from the keyboard", and the last line `Ready, with a warning: focusd could not register its hotkeys ...`. Control with free keys: no note, plain `Ready` |

## Release archive (73afbdd)

| Check | Result |
| --- | --- |
| `cargo build --release --locked --target x86_64-pc-windows-msvc` | PASS, 28 s |
| Windows packaging steps from `release.yml` (minus `gh upload`), Windows PowerShell 5.1 | PASS. The zip holds exactly `focusctl.exe`, `focusd.exe`, `LICENSE`, `READ-ME-FIRST.txt`; `sha256sum -c` OK; `READ-ME-FIRST.txt` is byte-identical to `docs/release-readme.txt` apart from line endings |
| Following the text with this branch's own `focusctl.exe` | **FAIL as expected**: step 2 `focusctl.exe setup` gives `focusctl: unknown command setup`, exit 1. The commit message says `setup` is on the other branch; this proves a release cut from this branch alone would ship a README that does not work |
| Following the text with the `setup` branch's executables (a simulated merge), in a folder whose name has a space | PASS: `setup` found the `focusd` beside it, `Ready`, `doctor` all ok, `uninstall-hooks` left `{"theme":"dark"}` |

### Findings

**R3. A stray file was committed: `t-pack.zip` at the repository root in 73afbdd (confirmed, minor).**
`git ls-files` on that commit lists `t-pack.zip` (5 KB, holding `t-pack\LICENSE` and `t-pack\READ-ME-FIRST.txt`). It looks like a leftover from trying the packaging commands, and the commit message does not mention it. It should come out before merge.

**R4. The archive text is right for a new user, with four gaps (suggestions).**
Read as someone who has only the zip: the steps are in the right order, say what `setup` does, and `Windows protected your PC` is covered. Gaps:
1. It says "Restart any Claude Code session"; `windows-quickstart.md` also says to fully quit and reopen the Claude desktop app if the Code tab is used, because hooks are read at startup. A desktop-app user following only the zip would see nothing happen.
2. It does not name the focus key (`Ctrl+Alt+Shift+Space`) that snoozes a countdown, so a new user does not learn how to stop a switch.
3. The SmartScreen advice covers running `focusctl.exe`. `setup` then starts `focusd.exe` in the background, which may raise its own warning that the user cannot see. I could not test this: nothing here carries the web "mark of the web" flag that a downloaded zip would.
4. It depends on the `setup` command (see the FAIL above), so the order of merging matters.

## Not verified (part 4)

- The download case: `READ-ME-FIRST.txt` and the executables with the "mark of the web" and a real SmartScreen prompt; the draft release job; `pwsh` 7; macOS and its Gatekeeper `xattr` line.
- `setup` on macOS, and with a pipe on macOS.
- Real Claude sessions and hooks.

---

# Part 5: the real v0.1.0 Windows download (draft release, tag at f5cb063)

Run 2026-11-19 on the same Windows 11 machine. Assets were fetched with the signed-in GitHub tool, read-only (`gh release view` and `gh release download`); the draft was not edited, published or deleted. The sandbox is the usual one (redirected `HOME`/`USERPROFILE` and `FOCUS_AUTOPILOT_HOME`, port 47999, hotkeys `Ctrl+Alt+Shift+7`/`+8`, pre-written `config.toml`). The owner's `settings.json`, `config.toml` and `focusd` (pid 30604) were not touched and the owner's `focusd` was still running at the end; every `focusd` I started was stopped.

The release: `v0.1.0`, draft, not a pre-release, six assets (the three archives and their `.sha256` files). The tag commit f5cb063 contains the F4 fix (0aaf44d).

| # | Step | Result |
| --- | --- | --- |
| 1 | `gh release download ... --pattern "*windows*"` into an empty folder | PASS: zip (1,422,003 bytes) and `.sha256` |
| 2 | SHA-256 | PASS: `sha256sum -c` says OK (`3557b6b5...a72c3`) |
| 3 | Unzip; the four files | PASS: `focusctl.exe` (922,112), `focusd.exe` (2,341,888), `LICENSE`, `READ-ME-FIRST.txt` (2,454), all inside one folder. Zip entries use `/`, so the backslash worry from R1 (made with Windows PowerShell 5.1 here) does not apply to the real archive |
| 4 | Read `READ-ME-FIRST.txt` | The text has the changes asked for after part 4: step 3 says to quit and reopen the desktop app, it names the focus key, and it warns about a second SmartScreen prompt, marked untested |
| 5 | Follow it: `.\focusctl.exe setup` in PowerShell from the unzipped folder | PASS: `hooks: added`, `focusd: started`, four `ok` checks, `Ready`, exit 0 |
| 6 | `focusctl doctor` | PASS: "Everything checked is in order" |
| 7 | A synthetic hook sent to the sandbox `focusd` (nothing sent to Claude) | PASS: wrong token 401, right token 200, log shows `hook: UserPromptSubmit` and the "not a home app" note |
| 8 | `focusctl uninstall-hooks` | PASS: settings back to `{"theme":"dark"}` |
| 9 | Warnings | None appeared. See "SmartScreen and the mark of the web" |
| 10 | Version of the two programs | See F8 |
| 11 | macOS archives: sha256 and file list only, not run | PASS: both `sha256sum -c` OK; each holds a folder with `LICENSE`, `READ-ME-FIRST.txt`, `focusctl`, `focusd` (`tar -tzf`) |

## SmartScreen and the mark of the web

- The file from `gh release download` has **no** `Zone.Identifier` stream (`Get-Item -Stream *` lists only `:$DATA`; neither do the extracted files). The download tool does not add one, so this is not what a person downloading in a browser would have.
- **Simulation, not the real thing:** I copied the unzipped folder and added `Zone.Identifier` (`ZoneId=3`, `HostUrl=https://github.com/`) to all four files, as a browser would. `focusctl.exe setup` from a PowerShell console then ran the same way: `Ready`, exit 0, no window, no prompt. A program started from a console by `CreateProcess` is not normally checked by SmartScreen; the check happens when a file is opened from Explorer or `ShellExecute`.
- So: no warning in either run. What I could **not** test is the first double-click or "Run" of a browser-downloaded file, which is what the text warns about. I did not launch anything through `ShellExecute`, to avoid putting a prompt on the owner's screen. A screenshot was taken after the simulated run and checked for a SmartScreen window by title (none); it was not kept, because it shows the owner's desktop.

## Findings

**F7. `setup` still hangs when run through an extra shell layer with output captured (confirmed twice, scope limited).**
Steps: from Git Bash, `powershell -NoProfile -c "Set-Location <folder>; .\focusctl.exe setup" 2>&1 | cat > out.txt`, `focusd` not running.
Expected: returns once `Ready` is printed.
Happened: the output (with `Ready`) is complete and `focusd` is up, but the pipeline never ends (stopped after 20 s; and the same chain with `| cut` ran past 120 s). The direct form, `focusctl.exe setup 2>&1 | cat` from Git Bash, returns in 3 s on this release, so the F4 fix works for that case.
Likely cause (not proven): the fix clears the inherit flag on `focusctl`'s own stdin, stdout and stderr, but the outer pipe is a handle `focusctl` inherited from `powershell.exe` as an ordinary handle, and `focusd` inherits it from `focusctl`.
Who is hit: someone whose tool starts PowerShell and reads its output until the end, such as a script runner or an editor task that wraps `setup`. A person typing in a console or piping inside one PowerShell session is not. Medium to low.

**F8. Neither program can say what version it is (confirmed, minor).**
Steps: `focusctl.exe --version`, `focusd.exe --version`.
Expected: a version, since the file is `v0.1.0`.
Happened: `focusctl: unknown command --version` and `focusd: unknown argument --version`. The Windows file properties (`FileVersion`, `ProductVersion`) are empty too. A user who reports a bug cannot say which build they run, other than by file hash.

## Not verified (part 5)

- The first run of a browser-downloaded file under a real SmartScreen check (see above), and `setup` started by double-clicking.
- Real Claude sessions and hooks; the countdown and switches on live windows.
- The macOS programs (not run, as instructed) and the macOS `xattr` step.
- Whether the "second SmartScreen prompt" on `focusd.exe` exists.
