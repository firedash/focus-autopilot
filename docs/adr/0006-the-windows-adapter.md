# ADR 0006: The Windows adapter

Date: 2026-11-17. Status: accepted; built and tested on a Windows CI runner, not yet used by
a person on a Windows desktop. Where this record and `docs/design.md` disagree, this record
wins.

The app ran on macOS only. The owner wanted to try it on Windows, where until now only the
simulator ran. These are the choices in the adapter (`focus-platform/src/windows.rs`,
`windows_overlay.rs`).

1. **Microsoft's `windows-sys` bindings, not hand-written declarations.** Every Win32 call
   goes through the bindings Microsoft publishes, which were already in the lockfile through
   tokio. The calls are still `unsafe`, each with a written justification, and still only in
   `focus-platform`.

2. **An app is an executable name; a window handle is kept with it.** The id of the app in
   front is the file name of the executable that owns the foreground window, such as
   `WindowsTerminal.exe`, and `AppRef.window` carries the window handle. The core already
   compares handles when both sides have one, so on Windows a return goes to the exact
   window the prompt was sent from, and the move back to where the user left off goes to
   the exact window they left, which macOS cannot do without a permission. When that window
   is gone, the frontmost window of the same executable is used.

3. **Which windows count as apps.** Visible, top-level windows with no owner, not tool
   windows, with a title, and not hidden by the window manager. The adapter reads the length
   of a window's title to tell a real window from an invisible helper; it never reads the
   title itself.

4. **One empty mouse event to be allowed a switch, on by default.** Windows lets a process
   bring a window forward only under conditions such as having received the last input, so
   a switch made on a timer by a background program is normally refused and the taskbar
   button flashes instead. The design said the adapter would not work around this, and that
   a forceful mode might come later, off by default. As built: the adapter first asks
   plainly. If the window did not come forward, it sends one mouse event that moves nothing
   and presses nothing, which makes this process the last to receive input, and asks again.
   This is on by default (`switching.force_foreground`), because the loop the owner asked
   for in ADR 0005 is automatic switching, and without this nearly every switch on Windows
   would become a question. It is the one place the app sends input of any kind, and it
   sends no keystroke and no click. Thread-input attachment, a synthesized Alt key and
   changing the foreground-lock timeout, the other known ways, are still not used. Turning
   the setting off restores the design's behaviour: a refused switch becomes the "go back?"
   prompt, and the focus key works there, because a hotkey press is itself input to this
   process. Side effect: the empty event counts as input for the system's idle timer.

5. **The boxes are two popup windows that cannot be clicked or focused.** Always on top,
   non-activating, tool windows (no taskbar button), layered and transparent to the mouse,
   so every click goes to whatever is underneath. They are drawn with GDI in the system's UI
   font and sized to their text. They are positioned at the top of the work area of the
   monitor that holds the foreground window, and scale with the system's scaling setting.

6. **Hotkeys use `RegisterHotKey`, tied to the shell's thread.** No keyboard hook, so no
   other key is ever seen. The defaults are `Ctrl+Alt+Shift+Space` and `Ctrl+Alt+Shift+P`. The design's
   `Ctrl+Alt+Space` was the first default; on the owner's machine the Claude desktop app
   already owned it, which is the program most users of this one will have. In that same
   self-test the pause key registered and its presses were delivered: the first thing seen
   working on a real Windows desktop. A chord
   another program owns is refused with that reason. The box shows the chord in Windows
   spelling.

7. **One thread, polled.** The windows, the hotkeys and the shell loop share one thread; the
   loop takes waiting window messages each time round, as the macOS adapter turns its run
   loop. The foreground window is polled twice a second, as on macOS.

8. **Where files go.** The data folder is `%LOCALAPPDATA%\io.github.firedash.focusautopilot`
   (per-user, not roamed between machines, since it holds the token). The home folder falls
   back to `%USERPROFILE%` when `HOME` is not set, which is where Claude Code keeps
   `.claude\settings.json`. Files are not given a restricted mode as on Unix; they rely on
   the user profile's own access rules.

9. **Not done on Windows.** Start at login (`focusctl install-agent` is macOS only; the
   design's registry value is not written). The LLM recap is untried: it runs `claude`, and
   an npm-installed `claude.cmd` may not be found. Claude Code running inside WSL cannot
   reach a `focusd` on Windows by default, because WSL's `127.0.0.1` is not Windows's. The
   home-app list for Windows was written from memory, like the macOS one.

10. **What CI proves and what it does not.** A Windows runner compiles, links and lints the
    adapter and runs its tests against a real Windows: the read-only calls answer, the
    boxes can be created, shown without taking the foreground, sized and hidden, a hotkey
    registers and a second owner is refused. It then installs the hooks, starts the real
    `focusd.exe`, sends it one hook as Claude Code would, and checks that `focusd` reported
    the front window and received the hook. It cannot show that a person sees the box in
    the right place, that a switch lands where it should, or that the empty mouse event is
    enough on a desktop someone is using.
