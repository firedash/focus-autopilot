# Threat model

What could go wrong with an app that listens for your coding agent's events, can bring
windows to the front, and serves a settings page; what the app does about each; and where
that is tested. [AUDIT.md](AUDIT.md) says where each piece lives in the code.

## What is protected

- Your keystrokes going to the window you meant them for.
- Your Claude Code settings file.
- The text of your prompts, Claude's replies, commands and file paths.
- The app's own settings and the record of how you spend your time.

## Who is assumed hostile

- A web page open in your browser, which can send requests to `127.0.0.1`.
- Another user on the same machine, who can connect to the loopback port and list processes.
- The content of hook payloads: prompts, agent output, file and app names are all untrusted text.
- A dependency, CI action or build step (supply chain).

**Not defended against:** software already running as you. It can read the token and the
config file directly, and no local app can prevent that.

## Threats and mitigations

| Threat | Mitigation | Tested by |
| --- | --- | --- |
| A web page or another user forges hook events to trigger switches | Loopback bind only; a random 256-bit token compared in constant time; `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (blocks DNS rebinding); a hook with any `Origin` header is refused; JSON content type required; rate limit of 200 a second | `focus-receiver/src/check.rs` tests; `tests/http.rs` over a real socket |
| A web page reads your statistics or changes your settings through the page's API | Everything above, plus: `Origin`, if present, must be this server's own; `Sec-Fetch-Site`, if present, must be `same-origin`; writes must be `application/json`, which a cross-site form cannot send; no CORS headers are ever sent | `the_api_refuses_strangers_and_never_reaches_the_backend` |
| The page's token leaks | It is passed after the `#` of the address, which browsers do not send to servers, and removed from the address bar and history on load; `focusctl ui` hands it over through a private file, not a command line other users could list; the API never returns it | `focusctl/src/ui.rs` tests; `the_config_reply_never_carries_the_token` |
| The page is used to change something it should not | Only the settings in an explicit list can be changed; the token, port and salt are not in it; every change is type- and range-checked and the whole config validated before anything is written | `focus-runner/src/settings.rs` tests |
| Text from a prompt, app name or error runs as code in the page | Every string is set with `textContent`; the page has no inline script or style and a Content-Security-Policy of `default-src 'none'` with only same-origin script, style and fetch; it cannot be framed | `the_page_has_nothing_inline_for_the_policy_to_block`; the browser check reports any policy violation |
| Huge, deeply nested or malformed payloads | 8 MB body limit (64 KB for the API); typed parsing of known fields only; unknown field types read as absent; connections that stall are cut off | `payload.rs` tests; `a_body_over_8_mb_is_refused` |
| A switch makes keystrokes land in the wrong window | Every automatic switch, back or away, is announced by a countdown the focus key stops; when the countdown ends the switch waits for a pause in typing, and after 5 seconds of continuous input it becomes a question; two settings turn the automatic switches off altogether; the box on screen cannot take keyboard focus; the app never sends keys or clicks | `focus-core/tests/full_loop.rs`, `returns_and_sessions.rs`, `away_and_back.rs`, `manual_mode.rs` |
| A hook, or another program with the token, chooses which window is brought forward | The Antigravity events carry no app name: `focusd` takes the app to return to from its own `[antigravity] app_name` and matches it against the running apps; a payload field that names an app is ignored; an unmatched or ambiguous name does nothing and never falls back to the foreground window. Anyone who has the token can still forge a "finished" event for that one configured app | `the_app_to_return_to_comes_from_the_configuration_not_the_hook`; `a_payload_cannot_name_the_app_to_bring_forward`; `unknown_or_ambiguous_app_never_falls_back_to_the_foreground_window` |
| The Antigravity hook changes how Antigravity behaves | Its command always exits 0 with a passive reply (`allow` for tools, an empty object elsewhere, `stop` for Stop), never edits tool arguments, injects steps or reads transcripts; failures print one generic line to stderr. That `allow` leaves Antigravity's own approval policy alone is taken from its documentation and has not been observed ([ADR 0007](docs/adr/0007-antigravity-attention-returns.md)) | `focusctl/tests/antigravity.rs`; a real-session check, not done |
| The installer damages Claude Code's settings | Dry run; backup; atomic write; symlinks followed; non-JSON refused; only entries with this app's hook URL are touched; uninstall restores the original bytes | `focusctl/src/files.rs`, `hooks.rs` tests |
| Secrets in prompts or commands are shown or stored | Likely secrets are redacted at the receiver before anything is stored and again on the recap card; logs written to a file hold event names only; history and usage files hold no prompt text, commands or paths | `focus-core/tests/sessions_and_secrets.rs`; `payload.rs` tests |
| A folder or app name is injected into an OS command | OS calls are made directly; the programs ever started (`claude`, and for `focusctl ui` the desktop's opener: `open`, `explorer` or `xdg-open`) get an argument list, never a shell string | [AUDIT.md](AUDIT.md), Processes |
| A feature quietly needs Accessibility or screen access | None is used; [PERMISSIONS.md](PERMISSIONS.md) states it and changes to it need a maintainer's review | review |
| A malicious dependency or CI action | Lockfile committed; toolchain pinned; `cargo deny` gates advisories, licenses and sources; CI actions pinned to full commits with a read-only token; every dependency listed with its reason in DEPENDENCIES.md | `deny.toml`; `scripts/check-repo.py`; `.github/workflows/ci.yml` |

## Known gaps

- Redaction is a pattern filter. A secret written as plain prose ("my password is ...") or
  attached to a short flag (`-pSecret`) is not caught.
- The app keeps app names and time per app on disk when that setting is on. It is local and
  owner-readable only, but it is a record; it can be turned off and erased.
- No fuzz targets yet, no `strace` network-and-file test, no signed releases.
- The macOS adapter has run on one Mac and the Windows adapter on one Windows desktop, each
  by the same person. [STATUS.md](STATUS.md) lists what was seen on each and what was not.
- On Windows the app sends one empty mouse event when Windows refuses a switch
  ([PERMISSIONS.md](PERMISSIONS.md)). It is the only input the app sends on either system.
- On Windows the data files rely on the user profile's access rules; they are not given a
  restricted mode as they are on macOS and Linux.
