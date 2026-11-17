# Running Focus Autopilot on Windows (developer build)

The Windows version is new. It is compiled, linked and tested on a Windows machine in CI, but
**no person has used it on a Windows desktop yet**. Expect to find problems; the `focusd`
window prints what it sees and why it holds back, which is what to send when something is off.

It needs Claude Code running on Windows itself (the native `claude` command, or the Code tab
of the Claude desktop app). Claude Code inside WSL cannot reach it by default: WSL's
`127.0.0.1` is not Windows's.

## 1. Build

Install Rust from [rustup.rs](https://rustup.rs); it asks for the Microsoft C++ build tools.
Then, in PowerShell:

```powershell
git clone https://github.com/firedash/focus-autopilot
cd focus-autopilot
cargo build --release -p focusd -p focusctl
```

## 2. Install the hooks

```powershell
.\target\release\focusctl.exe install-hooks --dry-run   # shows the change to settings.json
.\target\release\focusctl.exe install-hooks             # backs it up, merges, writes the token
```

This edits `%USERPROFILE%\.claude\settings.json` and creates
`%LOCALAPPDATA%\io.github.firedash.focusautopilot\config.toml`. Restart any running Claude
Code session, and fully quit and reopen the Claude desktop app if you use its Code tab.
Undo with `.\target\release\focusctl.exe uninstall-hooks`.

## 3. Check the hotkeys

```powershell
.\target\release\focusd.exe --test-hotkeys
```

Press `Ctrl+Alt+Shift+Space` and `Ctrl+Alt+Shift+P` from any window. Each press should print a
line. A `problem:` line means another program owns that combination: pick another under
`[hotkeys]` in `config.toml`.

## 4. Check the box

```powershell
.\target\release\focusd.exe --test-box
```

A dark box with an orange edge should appear at the top centre of the screen for 10 seconds,
then the recap card for 6. If you see neither, say so: nothing else needs to be running for
this test.

## 5. Run

```powershell
.\target\release\focusd.exe
```

Leave it running. Every time you change window it prints `front: <program>.exe`, which is
how you can see it recognises your windows.

1. Send Claude a prompt. `focusd` prints `hook: UserPromptSubmit`. If it also prints
   `note: this prompt came from ... which is not a home app`, copy the line it suggests into
   `config.toml`: the built-in list of terminals and editors was written from memory.
2. Switch to another window. When Claude finishes, a box appears at the top of the screen
   with a sound, counts down 5 seconds, and brings the window you prompted from forward,
   with a recap for a few seconds. `Ctrl+Alt+Shift+Space` during the countdown snoozes it.
3. Send another prompt and wait. After about 30 seconds a box says "Claude will take a
   while" and you are moved back to the window you were in.

`.\target\release\focusctl.exe ui` opens the Statistics and Configuration page, and
`.\target\release\focusctl.exe doctor` checks the setup.

## What is different from the Mac

- **Exact windows.** A return goes to the very window you prompted from, and the move back
  goes to the very window you left, not just the app.
- **One empty mouse event.** Windows refuses to let a background program bring a window
  forward. When that happens `focusd` sends one mouse event that moves nothing and clicks
  nothing, which Windows counts as input to `focusd`, and asks again. To turn it off:

  ```toml
  [switching]
  force_foreground = false
  ```

  Then a refused switch shows "Go back?" and `Ctrl+Alt+Shift+Space` does it.
- **Not there yet:** start at login, and the LLM recap is untried.

## If something is off

Run `focusd` like this and send the last lines of the log:

```powershell
.\target\release\focusd.exe 2>&1 | Tee-Object focusd.log
```

- **No `front:` lines as you change windows:** the adapter is not seeing the foreground
  window at all.
- **`front: unknown.exe`:** that window belongs to a program Windows will not name to an
  ordinary process, such as one run as administrator.
- **A box says "Go back?" when you expected a countdown:** the `log: asked_because_...` line
  before it says why. `the_switch_was_refused` means Windows did not bring the window
  forward even after the empty mouse event.
- **No box at all, but `TOAST` lines in the log:** the window was not drawn; say which
  Windows version and whether you use more than one monitor or display scaling.
