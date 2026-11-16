# ADR 0004: The statistics and configuration page

Date: 2026-11-16. Status: accepted. Where this record and `docs/design.md` disagree, this
record wins.

The app gained a page with two tabs. **Statistics** shows where time went while Claude
worked; **Configuration** changes the settings without editing `config.toml` by hand. These
are the choices behind it.

1. **It is a local web page served by `focusd`, not a native window.** The receiver already
   listens on `127.0.0.1`; the page is three static files and a small JSON API on that same
   port, opened with `focusctl ui`. The design planned a Tauri webview for this. A page was
   chosen because it could be built and tested end to end without a Mac, in a real browser,
   with the fake platform behind it; the native macOS windows in this project were written
   blind and that cost several rounds of fixes. The same files can be loaded in a Tauri window
   later with no change. The cost: there is no menu-bar icon to open it from, and it lives in
   a browser tab.

2. **The page gets the same defences as the hook endpoint, and more.** A page that can change
   settings is a bigger target than one that only receives events. So: loopback only; the
   `Host` check that blocks DNS rebinding; the token on every API call; `Origin` must be
   absent or this server's own; `Sec-Fetch-Site` must be `same-origin` when a browser sends
   it; writes must be JSON, which a cross-site form cannot send; no CORS headers. The static
   files need no token because they hold no data. `/hook` is unchanged and still refuses any
   `Origin`, including this page's.

3. **The token reaches the page after the `#`.** Browsers never send that part to a server,
   and the page removes it from the address bar and history as it loads, keeping it in
   session storage. `focusctl ui` does not put the token on a command line, where another
   user could read it from the process list: it writes a private file that redirects to the
   page, and asks the browser to open that file.

4. **Nothing inline, nothing external, text as text.** The page has no inline script or
   style and loads nothing from another origin, enforced by a Content-Security-Policy of
   `default-src 'none'` with same-origin script, style and fetch only. Every string from
   outside (app names, paths, errors) is set with `textContent`. No framework, no build step,
   no npm: plain HTML, CSS and JavaScript, as the design's code standards ask.

5. **What can be changed is an explicit list.** `settings::KEYS` names every setting the page
   may change, with its type and range. The token, the port and the history salt are not in
   it. A change is applied to the text of `config.toml` with `toml_edit`, so comments and
   layout survive; the whole result is parsed and validated; only then is it written, through
   a temporary file. A refused change writes nothing and comes back as a sentence.

6. **Settings apply live, from the page or from the file.** A saved change is handed to the
   running core at once. A hand edit of `config.toml` is noticed within two seconds and
   applied the same way; if it is invalid, the last good config stays and the reason is
   printed. This replaces ADR 0003 item 22 in part: the file is now re-read while running. An
   unknown key is still an error rather than a warning. Hotkeys are the exception: they are
   registered once at startup, so a change needs a restart, and the page says so.

7. **Time per app is recorded, by default, locally.** To show where waiting time goes the
   shell records how long each app is in front: app ids, names and milliseconds, in half-hour
   slots in `usage.json`, owner-readable only. It never records window titles, URLs, file
   names or input. Time is not counted while nobody has touched the machine for five minutes
   or across a gap such as sleep. It is on by default because the Statistics tab is empty
   without it, it can be turned off on the Configuration tab (`stats.track_app_time`), and the
   Statistics tab can erase it together with the run history. This is new data the original
   design did not keep; PERMISSIONS.md and THREAT_MODEL.md say so.

8. **Half-hour slots in UTC, days in the viewer's time zone.** The browser sends its offset
   and the server groups slots into local days. Half an hour was chosen so that time zones at
   :30 fall on slot boundaries. No time-zone library is needed.

9. **Insights are computed on the server, sparingly.** The sentences on the Statistics tab
   come from a pure function with tests, not from the page, so every number shown is tested.
   Each needs enough data to mean something (two returns before quoting an average, five
   prompts before naming a busiest hour), at most six are shown, and with thin data the page
   says less instead of guessing.

10. **Charts.** One column chart of Claude's working time per day (one series, one colour, no
    legend) and one list of apps with thin stacked bars (two series: while Claude worked, and
    the rest). Both have a table view and the same tooltip on hover and on keyboard focus. The
    two series colours were checked with a palette validator for colour-blind separation and
    contrast against the light and the dark surface.

11. **`focusctl ui` starts one more program.** The design allowed two spawned programs,
    `focusctl` itself and `claude`. Opening the page uses the desktop's opener (`open` on
    macOS), with the file path as a single argument and no shell. AUDIT.md lists it.

12. **Checked in a browser, by script.** `scripts/ui-check/run.sh` seeds made-up data, starts
    `focus-headless`, and drives the page in Chromium through 41 checks: the key handling,
    each kind of control, refused changes, the file on disk, the running app's log, hand
    edits, the charts' table views and tooltips, and clearing the statistics. It also fails on
    any console error or policy violation. It is not part of CI yet, because CI has no
    browser installed.

13. **Not done.** A menu-bar icon; changing hotkeys without a restart; statistics per
    project (the history keeps only a salted hash of the folder, by design); exporting the
    data.
