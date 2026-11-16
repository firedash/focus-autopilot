# Security

Focus Autopilot watches your coding agent's hooks and can bring windows to the front, so a
flaw here matters. Reports are welcome.

## Reporting a vulnerability

Please report privately, through GitHub's private vulnerability reporting on this repository
(the "Report a vulnerability" button under the Security tab). Do not open a public issue for
something that could put users at risk.

- You should get a first response within 7 days.
- We will agree a disclosure date with you and credit you if you want that.

Useful things to include: the version or commit, your OS, what you did, what happened, and
what you expected.

## What counts

The threats the project defends against, and how, are in [THREAT_MODEL.md](THREAT_MODEL.md).
In scope, for example:

- A web page or another user's process getting the receiver on `127.0.0.1` to accept an event,
  read statistics or change a setting.
- A hook payload, app name or config value that makes the app crash, hang, or show or run
  something it should not.
- `focusctl` damaging Claude Code's `settings.json` or leaving it in a state `uninstall-hooks`
  cannot undo.
- Prompt text, commands, paths or the token ending up in a log, in `runs.jsonl`, in
  `usage.json`, or anywhere it can be read by another user.

Out of scope: software already running as you. It can read the token and the config file
directly, and no local app can defend against that.

## Supported versions

There are no releases yet. Fixes go to `main`.
