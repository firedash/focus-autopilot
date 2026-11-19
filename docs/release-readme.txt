Focus Autopilot: read me first
==============================

This folder holds two programs, focusd and focusctl (focusd.exe and focusctl.exe on
Windows), and the licence. It has no installer.

1. Unzip the whole folder somewhere you will keep it, and open a terminal in it.
   (Moving it later is fine; run setup again from the new place.)

2. Run setup. It adds the hooks to Claude Code, starts focusd in the background, checks the
   result and ends with one line: "Ready" or what to fix. It is safe to run again.

     macOS:     ./focusctl setup
     Windows:   .\focusctl.exe setup

3. Restart any Claude Code session that was already open. If you use the Claude desktop app's
   Code tab, fully quit the app and open it again. Then give Claude a task and switch to
   another window. When Claude is done, a box counts down and brings you back.

   The focus key is Ctrl+Alt+Shift+Space on Windows (Control+Option+Command+Space on a Mac).
   While a countdown runs it cancels the switch ("snooze" on the way back, "not now" on the
   way out). If you turned off "Bring me back by itself", the box waits and the key takes you.
   If another program already holds the key, setup says so when it starts focusd.

To check again later: focusctl doctor. To remove the hooks: focusctl uninstall-hooks.
Setup does not make focusd start at login. After a restart of your computer, run setup
again (macOS can also start it at login: focusctl install-agent).

These programs are not signed, so the system may warn you the first time:
  macOS:    "cannot be opened because the developer cannot be verified". Run
            xattr -d com.apple.quarantine focusd focusctl
            in this folder, then try again.
  Windows:  "Windows protected your PC". Choose "More info", then "Run anyway".
            Setup also starts focusd.exe in the background, and that start may raise its own
            SmartScreen prompt. This has not been tested; the same choice applies.

Check the download: the .sha256 file next to the archive on the releases page holds its
SHA-256 checksum. It shows the file is the one that was published, not who made it.

Everything else (what it does, what it never does, the settings, the quickstarts) is in the
project's repository: https://github.com/firedash/focus-autopilot
The README.md and the docs folder there are not in this archive, which is why their links
are not included here.
