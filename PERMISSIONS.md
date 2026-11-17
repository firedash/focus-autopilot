# Permissions

**Focus Autopilot asks for no OS permission.** If macOS or Windows ever shows a permission
prompt for `focusd` or `focusctl`, that is a bug: please report it.

This page lists what the app does that an OS could gate, and why none of it needs a grant.
Changes to this page need a maintainer's review.

## What it does without a permission

| It does this | With this OS facility | Why no permission is involved |
| --- | --- | --- |
| Reads which app is in front, and which apps are running | `NSWorkspace` | App identity is public to every process. Window titles and contents are never read |
| Reads how long since the last keyboard or mouse input | `CGEventSourceSecondsSinceLastEventType` | It returns a duration only. No key, click or position is ever seen |
| Brings an app to the front | `NSRunningApplication.activate` | It is a request the OS may refuse; the app then shows a prompt instead |
| Reacts to its own hotkeys | Carbon `RegisterEventHotKey` | The OS reports only that a registered chord was pressed. Other keystrokes are never seen, unlike key-event monitoring, which would need Accessibility |
| Shows a small box on screen | A non-activating panel that ignores the mouse | It cannot take keyboard focus |
| Plays a sound | `NSSound` | |
| Listens on `127.0.0.1` | A loopback socket | Loopback only; the macOS firewall prompt is for non-loopback listeners |

On Windows the same things are done with these, none of which needs a grant or elevation:

| It does this | With this OS facility | Why no permission is involved |
| --- | --- | --- |
| Reads which window is in front, which program owns it, and which app windows exist | `GetForegroundWindow`, `EnumWindows`, `QueryFullProcessImageNameW` | Only the executable's file name is kept. A window's title is never read, only its length, to tell a real window from an invisible helper |
| Reads how long since the last keyboard or mouse input | `GetLastInputInfo` | It returns a time only |
| Brings a window to the front | `SetForegroundWindow` | A request Windows may refuse |
| Is allowed that request | `SendInput` with one empty mouse event | See below |
| Reacts to its own hotkeys | `RegisterHotKey` | Windows reports only that a registered combination was pressed. There is no keyboard hook |
| Shows a small box on screen | A topmost popup window that is never activated and passes every click through | It cannot take keyboard focus |
| Plays a sound | `MessageBeep` | |

**The one piece of input the app ever sends.** Windows lets a program bring a window forward
only if it received the last input, so a switch made on a timer is normally refused. When
that happens, the Windows build sends one mouse event that moves the pointer nowhere and
presses no button, which makes it the last program to receive input, and asks again. It never
sends a keystroke or a click. `switching.force_foreground = false` turns this off; a refused
switch then becomes a question, and the focus key answers it. The macOS build sends nothing.

## What it never does

- Read keystrokes, mouse positions, the screen, window titles or the clipboard.
- Send keystrokes or clicks, or script other apps (no Apple Events, no Automation). The only
  input it ever sends is the empty mouse event on Windows described above.
- Read Claude Code's transcript files.
- Change system settings.

## Things that are not OS permissions but are worth knowing

- **Claude Code's settings.** `focusctl install-hooks` adds hook entries to
  `~/.claude/settings.json`. It shows the change first with `--dry-run`, backs the file up,
  and `uninstall-hooks` removes exactly those entries.
- **Time per app.** With `stats.track_app_time = true` (the default), `focusd` records how
  long each app is in front, as app names and durations in `usage.json` in its data folder.
  It can be turned off on the Configuration tab and erased on the Statistics tab.
- **The LLM recap** (off by default) runs your own `claude` command, which talks to the
  provider your Claude Code already uses.
- **Start at login** is only set up if you run `focusctl install-agent`, which exists for
  macOS only.
- **It moves you between windows by itself.** By default a finished run brings the window
  you prompted from forward after a countdown, and a long next prompt moves you back to the
  app you left. Both are announced first and can be turned off
  (`switching.auto_return`, `away.resume_last_app`).

## Optional permissions

None today. On macOS, returning to the exact window of an app, rather than to the app, would
need Accessibility or per-app Automation (see ADR 0003, item 16); it is not built, and if it ever
is it will be opt-in and listed here.
