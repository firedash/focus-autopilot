# Permissions

**Focus Autopilot asks for no OS permission.** If macOS ever shows a permission prompt for
`focusd` or `focusctl`, that is a bug: please report it.

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

## What it never does

- Read keystrokes, mouse positions, the screen, window titles or the clipboard.
- Send keystrokes or clicks, or script other apps (no Apple Events, no Automation).
- Read Claude Code's transcript files.
- Change system settings or work around the OS's focus rules.

## Things that are not OS permissions but are worth knowing

- **Claude Code's settings.** `focusctl install-hooks` adds hook entries to
  `~/.claude/settings.json`. It shows the change first with `--dry-run`, backs the file up,
  and `uninstall-hooks` removes exactly those entries.
- **Time per app.** With `stats.track_app_time = true` (the default), `focusd` records how
  long each app is in front, as app names and durations in `usage.json` in its data folder.
  It can be turned off on the Configuration tab and erased on the Statistics tab.
- **The LLM recap** (off by default) runs your own `claude` command, which talks to the
  provider your Claude Code already uses.
- **Start at login** is only set up if you run `focusctl install-agent`.

## Optional permissions

None today. Returning to the exact window of an app, rather than to the app, would need
Accessibility or per-app Automation (see ADR 0003, item 16); it is not built, and if it ever
is it will be opt-in and listed here.
