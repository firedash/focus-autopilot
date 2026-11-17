# Connecting Claude Code to Focus Autopilot

Focus Autopilot learns what Claude is doing from Claude Code's hooks: small entries in Claude
Code's settings that tell it to send a message to `focusd` when a prompt starts, a tool is
used, an approval is needed or a run ends. This page says how they get there, what exactly
is written, how to do it by hand, and which places it covers.

## The usual way: one command

```bash
./target/release/focusctl install-hooks --dry-run   # shows the change, writes nothing
./target/release/focusctl install-hooks             # backs the file up, adds the hooks
```

On Windows the program is `.\target\release\focusctl.exe`. The command:

- creates Focus Autopilot's `config.toml` with a random 64-character token, if there is none;
- adds eleven hook entries to Claude Code's user settings, beside whatever is already there;
- keeps a copy of the file as it was, next to it, ending in `.focus-autopilot.bak`.

Then restart every running Claude Code session, and fully quit and reopen the Claude desktop
app if you use its Code tab: hooks are read at startup. `focusctl doctor` confirms that the
hooks are in place and carry the current token, and `/hooks` inside Claude Code lists them.

`focusctl uninstall-hooks` removes exactly these entries and nothing else.

This has been run by a person on a Mac and on Windows.

## Where the settings file is

| System | File |
| --- | --- |
| macOS, Linux | `~/.claude/settings.json` |
| Windows | `%USERPROFILE%\.claude\settings.json`, normally `C:\Users\<you>\.claude\settings.json` |
| Claude Code inside WSL | `~/.claude/settings.json` inside WSL, a different file from the Windows one |

`--scope project` and `--scope local` write to `.claude/settings.json` and
`.claude/settings.local.json` in the current folder instead, so the hooks apply to one
project. Prefer `local` for that: the entries carry the token, and a project's
`settings.json` is normally committed and shared.

## What is written

Eleven entries, one per event, all the same apart from the event name. Each tells Claude
Code to post the event to `focusd` on this machine, with the token, and to give up after two
seconds:

```json
{
  "hooks": {
    "UserPromptSubmit":  [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "PreToolUse":        [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "PermissionRequest": [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "PermissionDenied":  [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "Notification":      [{ "matcher": "permission_prompt|elicitation_dialog|elicitation_url_dialog",
                            "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "Elicitation":       [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "SubagentStart":     [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "SubagentStop":      [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "Stop":              [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "StopFailure":       [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }],
    "SessionEnd":        [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47821/hook", "timeout": 2, "headers": { "X-Focus-Token": "YOUR_TOKEN" } }] }]
  }
}
```

`47821` is the default port (`[receiver] port` in `config.toml`), and the token is
`[receiver] token` there. The list of events lives in `crates/focusctl/src/hooks.rs`.
`focusd` answers every hook at once with an empty reply, so it never blocks or changes what
Claude does. If `focusd` is not running, a hook gets no answer; by Claude Code's
documentation that is an error it notes and carries on from.

## Doing it by hand

There is no need to, but you may prefer to read every line before it goes into another
tool's settings. The safe way is to have `focusctl` write to a scratch file and copy from it:

1. Write the entries to a scratch file. This creates `config.toml` and its token as usual,
   and does not touch Claude Code's settings.

   ```bash
   ./target/release/focusctl install-hooks --settings scratch.json
   ```

2. Back up the real file. On macOS:

   ```bash
   cp ~/.claude/settings.json ~/.claude/settings.json.bak
   ```

   On Windows, in PowerShell:

   ```powershell
   Copy-Item "$env:USERPROFILE\.claude\settings.json" "$env:USERPROFILE\.claude\settings.json.bak"
   ```

   If there is no such file yet, there is nothing to back up; you will create it in step 3.

3. Open both files and copy across. If the real file is empty or missing, its contents
   become the contents of `scratch.json`. If it has other settings, add the `"hooks"` key
   beside them. If it already has a `"hooks"` section, add each event's entry inside that
   section, next to any hooks of your own for the same event, instead of replacing it.

4. Check the result is strict JSON: no trailing commas and no comments. Then restart Claude
   Code and run `focusctl doctor`.

To undo it by hand, delete the entries whose `url` is `http://127.0.0.1:47821/hook`, or put
the backup back.

If the token ever changes (`focusctl rotate-token`), the entries must change with it. The
command does that for the file it manages; hand-made copies in other files have to be edited.

## Which places this covers

The hooks belong to Claude Code, not to one window, and every place that runs Claude Code on
your machine reads the same settings file. So one install covers:

| Where Claude Code runs | Status |
| --- | --- |
| A terminal | Seen working on a Mac |
| The Claude desktop app's Code tab, with a local folder | Seen working on Windows |
| The Claude Code extension in VS Code or in Cursor | Should work, by Anthropic's documentation; not tried here |
| JetBrains IDEs | Unknown |

Two conditions apply to all of them. Each needs its window recognised as somewhere you
prompt Claude from; if it is not, `focusd` prints the line to add to `config.toml`. And the
session has to run on the same machine as `focusd`.

It does not cover:

- **Cloud sessions**, in the desktop app or on the web. They run elsewhere, so nothing on
  your machine is told.
- **Cowork** in the Claude desktop app. Tried on Windows on 2026-11-17 with `focusd` running:
  no hook arrived. Cowork does its work in a sandbox of its own, and other users have
  reported that it does not run these hooks at all.
- **Claude Code inside WSL** with `focusd` on Windows. WSL has its own settings file, and its
  `127.0.0.1` is not Windows's by default.
- **Other agents**: Cursor's own agent, GitHub Copilot, Codex, Gemini CLI. Each has its own
  way of reporting what it is doing, or none. Supporting one means writing a translator from
  its events to the ones Focus Autopilot understands; the rest of the app would not change.
  None is written.
