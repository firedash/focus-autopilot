# Running Focus Autopilot on your Mac (developer build)

This is a developer build. The switching, hooks, on-screen boxes and recap have run on a Mac
with Claude Code in a terminal. The focus and pause hotkeys have been confirmed with the self-test below, though not yet in a
full run. The statistics and configuration page on
a real Mac, and other Claude Code surfaces have not been tried. Expect to find and fix
problems; if something isn't working, start with `focusctl doctor` (below).

## 1. Build

```bash
git clone https://github.com/firedash/focus-autopilot && cd focus-autopilot
cargo build --release -p focusd -p focusctl
```

Needs Xcode command line tools (`xcode-select --install`). Rust 1.97.0 is picked up from
`rust-toolchain.toml`.

## 2. Install the hooks

```bash
./target/release/focusctl install-hooks --dry-run   # shows the diff to ~/.claude/settings.json
./target/release/focusctl install-hooks             # backs it up, merges, writes the token
```

This also creates `~/Library/Application Support/io.github.firedash.focusautopilot/config.toml`
with a random token. Restart any running Claude Code sessions. Undo with
`./target/release/focusctl uninstall-hooks`.

## 3. Run

```bash
./target/release/focusd
```

Leave it running in a terminal. It prints the hotkey (by default `⌃⌥⌘Space`, that is
Control+Option+Command+Space) and how returns and away switches are set up.

In another terminal window start `claude` and give it a task. Switch to any other app. When
Claude finishes or needs your approval, a sound plays and a box appears at the top of the
screen: "Claude is done in api. Back to Terminal in 5 s." Five seconds later you are in the
window you prompted from, with a recap card for a few seconds. The hotkey during the
countdown snoozes it for a minute. If you go back on your own, the box goes away.

Then send another prompt and stay in the terminal without touching anything. After about 30
seconds a box says "Claude will take a while. Switching to <the app you were in> in 3 s.",
and you are moved back to where you left off; the hotkey during that countdown means "not
now". When Claude is done you are called back again. The very first prompt moves you nowhere,
because there is nowhere you left off yet, unless you name an away app (below).

The countdown bringing you back has been seen working on a Mac. Being moved back to where
you left off is new and **has not been run on a Mac**. Two settings turn the loop off, in `config.toml` or on the
Configuration tab:

```toml
[switching]
auto_return = false       # a box asks "Go back?" and waits for the hotkey

[away]
resume_last_app = false   # never move me to the app I was called back from
```

If anything seems off, `focusctl doctor` checks the setup (config, hooks, whether `focusd` is
running and accepts the token, whether prompts have been seen, the `claude` command) and says
how to fix each failure.

Commands typed in the `focusd` window still work: `click` (same as the hotkey), `pause`,
`resume`, `quit`. When `focusd` runs from a terminal it also prints toasts, prompts and recaps
there (`--verbose` forces this when output goes to a file). The window you prompted from must be a known home app (Terminal, iTerm2,
VS Code, JetBrains, Cursor and so on; list in `crates/focus-platform/src/known_apps.rs`). Add
others under `[home] extra_apps` in `config.toml`.

**Check the hotkey first.** It has worked on one Mac; yours may have another app holding the
same keys, and this takes a minute:

```bash
./target/release/focusd --test-hotkeys
```

It registers your hotkeys and prints `pressed: focus key` each time you press one, from any
app, for 60 seconds. If it prints a `problem:` line, macOS refused the chord (another app may
own it). If it registers cleanly but shows no presses, tell us: that means the event loop is
not delivering them, and the typed `click` command is the workaround until it is fixed.

Change the hotkeys in `config.toml` or on the Configuration tab; an empty string turns one
off. A chord needs at least one of Control, Option, Shift, Command. There are three: the focus
key, the pause key, and a "this run will take a while, switch me away now" key (off by
default). A change takes effect the next time `focusd` starts.

```toml
[hotkeys]
focus_key = "Control+Option+Space"
pause = "Control+Option+P"
declare_long = ""
```

## Statistics and configuration

```bash
./target/release/focusctl ui
```

opens a page in your browser with two tabs, served by `focusd` on your own machine. `focusd`
has to be running. The page is plain HTML with no external requests.

**Statistics** shows, for today and the last 7 days: how long Claude worked, how long you
spent in other apps meanwhile, how quickly you came back when called, how often you switched
apps, which apps your time went to, and a few sentences about what stands out. Every chart
has a table view. To draw it, `focusd` records how long each app is in front (app names and
durations only, never window titles or anything you type) in `usage.json` in its data folder,
and not while you are away from the keyboard. The tab has a button to erase it all.

**Configuration** has a control for everything you can set: whether the app is on, the LLM
recap, the away app and its timings, home apps, hotkeys, and the advanced timings. A change is
saved to `config.toml` as you make it (your comments in the file are kept) and applies at once,
except hotkeys. A value that would break a rule, such as an away app that is also a home app,
is refused with the reason, and the file is left as it was. You can still edit `config.toml`
by hand: `focusd` notices within a couple of seconds and applies it, or keeps the last good
settings and says why not.

The page needs the token, which `focusctl ui` passes on without putting it on a command line.
If you open the address by hand you will see "This page needs its key": use `focusctl ui`.
`focusctl ui --no-open` writes the launcher file and prints where it is.

## Using it from VS Code, Cursor or the Claude app (untested)

Only the terminal has been tested. The hooks belong to Claude Code, not to the terminal,
and Anthropic's docs say they fire wherever Claude Code runs, so the hooks you installed
should cover other places too. Nobody has checked that here yet, which is what this section
is for. What each place needs is its app recognised as a "home"
app, so `focusd` knows where to send you back to. Try them one at a time:

1. Start `focusd` and leave it running.
2. Open the app and start a Claude Code session in it:
   - **VS Code:** the Claude Code extension (its settings are shared with `claude` in the
     terminal, so no extra setup).
   - **Cursor:** install the same extension in Cursor, or run `claude` in Cursor's terminal.
     Cursor's own built-in agent is a different product and is not supported.
   - **Claude desktop app:** use the Code tab. The chat tab isn't Claude Code and sends no
     hooks. Anthropic's docs say the Code tab fires the same hooks, but some users report
     certain events missing there, so this one may only partly work.
3. Send a prompt, then click into another app.
4. Watch the `focusd` window. It prints `front: <bundle id>` as the front app changes, and
   `hook: UserPromptSubmit ...` when your prompt arrives.
   - **No `hook:` lines:** that app isn't sending hooks. Restart it (it only loads the hooks
     at startup) and check the extension or app version.
   - **A `note: this prompt came from ... which is not a home app` line:** copy the id it
     prints into `config.toml`, then restart `focusd`:

     ```toml
     [home]
     extra_apps = ["the.id.focusd.printed"]
     ```

Two limits to know about. Returns go to the app, not a specific window, so with several
editor windows open you land in whichever one macOS raises. And Cursor's, VS Code's and the
Claude app's ids in the built-in list were written from memory: the `front:` line shows the
real one, and if it differs from the list, please open an issue so the list can be corrected.

## Start at login (optional)

```bash
./target/release/focusctl install-agent
```

This writes a launch agent so `focusd` starts when you log in, and prints the one `launchctl`
command that starts it now. Build `focusd` and `focusctl` first (both are in
`target/release`), because it points at the `focusd` next to `focusctl`. It saves your current
`PATH`, so `claude` is found for LLM recaps. Its log, `focusd.log` in the data folder, holds
event names only, never prompt text. Stop it with `focusctl uninstall-agent` and the
`launchctl bootout` command it prints. There is no menu-bar icon yet.

## Several sessions

Every toast and recap names the project (the last folder of where Claude is running), for
example "Claude is done in api. Go back?". If two projects share a folder name, both show as
`parent/name`. The key takes you back to the app, not a particular window, so with several
windows of one app you land in whichever macOS raises.

When you return, every other session waiting in that app is settled too, so the card has one
line for each of them with what happened, for example `Also:    web - Header fixed.` (two at
most, then a count). That line is the only place their result is shown, so read it.

## Privacy

Prompts, commands and Claude's messages are redacted for likely secrets (API keys, tokens,
`password=` pairs) before `focusd` stores or shows them, so a key typed into a prompt does not
appear on the recap card. This is a pattern filter, not a guarantee. `runs.jsonl` in the data
folder keeps one metadata line per run (durations, counts, a hashed project) to improve wait
estimates, never prompt text; delete it any time.

## Shorter recaps (optional)

By default the recap is built from the hook fields: your prompt and the start of Claude's last
message, which can be long. For a three-line synthesis, let your own `claude` CLI write it:

```toml
[recap]
mode = "llm"
```

Restart `focusd`. After a few seconds the card shows this instead of the long one (the long
card is only a fallback if the summary fails):

```text
Asked: <what you asked>
Did: <what Claude did>
Needs: <what you must decide, or "nothing">
```

How it works: `focusd` runs `claude -p --model haiku` with no tools, no MCP servers and no
hooks, in an empty folder (`recap-workdir` in the data folder), and the text goes only to the
provider your Claude Code already uses. Only your prompt, the commands and file names from the
run, and the end of Claude's last message are sent, with secrets redacted; never file contents
or the transcript. It uses a little of your Claude usage per recap. If it fails (the `claude`
command isn't found on `focusd`'s PATH, a flag your version doesn't know, or a 20 second
timeout), `focusd` prints `LLM recap failed: ...` and you keep the plain card.

## An away app for the first prompt (optional)

By default an away switch goes to the app you were last called back from. To be moved
somewhere on the first prompt too, or whenever that app has quit, name an away app:

```bash
./target/release/focusd --list-apps      # bundle id <tab> name of every running app
```

```toml
[away]
apps = ["com.apple.Notes"]

[switching]
short_grace_seconds = 5
away_after_seconds = 5
away_requires_idle_seconds = 3
```

The three `[switching]` lines only make the away switch come sooner (after 5 seconds of a run
and 3 without input, in place of 30 and 8), which is handy while trying it out.

## What to check first if it misbehaves

- **Run `focusctl doctor`.** It checks, in order: `config.toml`, the installed hooks, whether
  `focusd` is running and takes the token, whether any prompt has arrived in the last 7 days,
  the `claude` command, and the launch agent. Each failure comes with the fix.

- **No box appears when Claude finishes.** Check that `focusd` printed `front: <id>` for your
  terminal and that id is a home app. If you were still in that window when Claude finished,
  nothing is shown, by design.
- **`focusd` won't start: `unknown keys: ...`.** `config.toml` has a setting the app doesn't
  know, usually a typo; correct or delete that line. A setting an older build had (such as
  `[return]`) does not stop it: `focusd` skips it and prints a `note:` saying you can delete it.
- **A box asks "Go back?" when you expected a countdown.** The `focusd` window prints a
  `log: asked_because_...` line just before it, which says why: `auto_return_is_off`,
  `another_session_is_here` (the app you are in has a Claude session of its own),
  `just_switched_away` (Claude finished within 5 seconds of moving you), `input_never_paused`
  (you typed or moved the mouse for 5 seconds straight after the countdown) or
  `the_switch_was_refused` (macOS did not bring the window forward).
- **The hotkey does nothing.** Run `focusd --test-hotkeys`. A `problem:` line means the OS
  refused the chord (another app may own it): pick another in `[hotkeys]`. Registered but no
  presses means the events are not being delivered; type `click` in the `focusd` window
  meanwhile, and report it.
- **Away app doesn't come forward.** macOS 14+ may refuse a background process activating
  another app. The core notices (it verifies after `verify_after_ms`) and falls back to a
  prompt; if so, that is the M2 finding to record.
- **Front-app changes arrive late.** `NSWorkspace` is read with a short run-loop pump and
  polled every 500 ms; a notification observer is not written.
- **Idle time looks wrong.** It comes from `CGEventSourceSecondsSinceLastEventType`.
- Everything the adapter calls needs no permission prompt. If macOS shows one, that is a bug
  against the design: report it.
