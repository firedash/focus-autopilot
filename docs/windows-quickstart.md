# Running Focus Autopilot on Windows (developer build)

The Windows version is new. One person has run it on one Windows desktop, with a local
session in the Claude desktop app's Code tab: the hooks arrived, the windows were
recognised, both hotkeys worked, the box and the recap card appeared, and the countdown
brought the Claude window back. Being moved back to where you left off works too, and is in
the recording at the top of the README. The LLM recap and the Statistics and Configuration
page have not been tried there. It is also compiled, linked
and tested on a Windows machine in CI on every push. Expect to find problems; the `focusd`
window prints what it sees and why it holds back, which is what to send when something is off.

It needs Claude Code running on Windows itself (the native `claude` command, or the Code tab
of the Claude desktop app). Claude Code inside WSL cannot reach it by default: WSL's
`127.0.0.1` is not Windows's.

## 0. Or use the download

The [releases page](https://github.com/firedash/focus-autopilot/releases) has a Windows zip (unsigned). Unzip it, open PowerShell in the
folder and run `.\focusctl.exe setup`; then skip to step 3. Windows may say "Windows protected
your PC": choose More info, then Run anyway. `READ-ME-FIRST.txt` in the zip has the same
steps. The zip has been run through `setup` in a test, but not yet with a real Claude prompt.

## 1. Build (only if you are not using the download)

Install Rust from [rustup.rs](https://rustup.rs); it asks for the Microsoft C++ build tools.
Then, in PowerShell:

```powershell
git clone https://github.com/firedash/focus-autopilot
cd focus-autopilot
cargo build --release -p focusd -p focusctl
```

## 2. Install the hooks and start focusd

One command does this step and checks the result; it is safe to run again:

```powershell
.\target\release\focusctl.exe setup
```

It installs the hooks (as below), starts `focusd` in the background with its log in
`%LOCALAPPDATA%\io.github.firedash.focusautopilot\focusd.log`, runs the checks and ends with one
line saying `Ready` or what to fix. To see the change before it is made, or to do each part
yourself, use the commands below instead.

```powershell
.\target\release\focusctl.exe install-hooks --dry-run   # shows the change to settings.json
.\target\release\focusctl.exe install-hooks             # backs it up, merges, writes the token
```

This edits `%USERPROFILE%\.claude\settings.json` and creates
`%LOCALAPPDATA%\io.github.firedash.focusautopilot\config.toml`. Restart any running Claude
Code session, and fully quit and reopen the Claude desktop app if you use its Code tab.
Undo with `.\target\release\focusctl.exe uninstall-hooks`. [hooks.md](hooks.md) shows exactly
what is written and how to do it by hand.

## 3. Check the hotkeys

```powershell
.\target\release\focusd.exe --test-hotkeys
```

Press `Ctrl+Alt+Shift+Space` and `Ctrl+Alt+Shift+P` from any window. Each press should print a
line. A `problem:` line means another program owns that combination: pick another under
`[hotkeys]` in `config.toml`.

**Which keys.** A chord is modifiers plus one key, joined with `+`, for example
`Ctrl+Alt+Shift+B`. Modifiers: `Control` (or `Ctrl`), `Option` (or `Alt`), `Shift`, and `Command`
(or `Cmd`, `Super`, `Meta`; the Windows key on Windows). At least one modifier is required. The
key is one of `A` to `Z`, `0` to `9`, `Space`, `Return` (or `Enter`), `Escape` (or `Esc`) or
`Tab`. Function keys such as `F9`, arrows and punctuation are not accepted yet; `focusd`
says `is not a key I know`. The settings are `focus_key`, `pause` and `declare_long`; an empty
string turns one off.

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
   `note: this prompt came from ... which is not a home app`, see the next section before
   going on: nothing will bring you back until that is fixed.
2. Switch to another window. When Claude finishes, a box appears at the top of the screen
   with a sound, counts down 5 seconds, and brings the window you prompted from forward,
   with a recap for a few seconds. `Ctrl+Alt+Shift+Space` during the countdown snoozes it.
3. Send another prompt and wait. After about 30 seconds a box says "Claude will take a
   while" and you are moved back to the window you were in.

`.\target\release\focusctl.exe ui` opens the Statistics and Configuration page, and
`.\target\release\focusctl.exe doctor` checks the setup.

## If you prompt Claude from an app that is not on the list

Focus Autopilot only brings you back to windows it knows you prompt Claude from. It has a
built-in list of terminals, editors and the Claude app
(`crates/focus-platform/src/known_apps.rs`). Any other program that runs Claude Code has to
be added, once. Until it is, the hooks still arrive, but when Claude finishes there is no
box and no switch, which looks exactly like the app not working.

`focusd` tells you when this happens, on the line after the prompt:

```text
note: this prompt came from Some Editor (SomeEditor.exe), which is not a home app, so no return is set up for it. To use it, add "SomeEditor.exe" under [home] extra_apps in config.toml.
```

Do what it says. Open the config:

```powershell
notepad "$env:LOCALAPPDATA\io.github.firedash.focusautopilot\config.toml"
```

and add the name it printed, exactly, with the `.exe`:

```toml
[home]
extra_apps = ["SomeEditor.exe"]
```

Save. `focusd` prints `config.toml changed: applied` within a couple of seconds; there is no
need to restart it. The Configuration tab (`focusctl ui`) has the same setting under "Home
apps", with a picker of running programs. The change applies from your next prompt: a run
that had already started has no window to return to.

This has been done for real. Munder Difflin, a third-party app that runs Claude Code, was
added this way on Windows, and the recording at the top of the README was made with it. It
is on the built-in list now, so it needs no setting.

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

## Stopping and upgrading focusd

`setup` starts `focusd` with no window, so there is nothing to close. To stop it, run
`taskkill /F /IM focusd.exe` in PowerShell, or end `focusd.exe` in Task Manager. To upgrade,
stop it, unzip or build the new version, and run `focusctl setup` again: `setup` leaves a
`focusd` that is already running alone, even an older one. Its log is
`%LOCALAPPDATA%\io.github.firedash.focusautopilot\focusd.log`, and `setup` moves it to
`focusd.log.old` once it passes 1 MB.

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
