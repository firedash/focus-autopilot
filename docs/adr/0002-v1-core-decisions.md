# ADR 0002: Choices made for the V1 items in focus-core

Date: 2026-11-14. Status: accepted, open to revision with real traces.

1. **Events carry more.** `AgentEvent` gains `project` (the salted hash of `cwd`, computed by
   the shell so the core needs no hashing). `UserPromptSubmit` gains `permission_mode` and
   `prompt_chars`, the prompt's full length before the receiver truncates it to 500
   characters; without it the over-1,000 bucket could never be told apart. `PreToolUse`
   gains `effort`.

2. **History reaches the core as input.** The shell reads `runs.jsonl` and passes
   `Input::History`. The core adds each completed run itself, so estimates improve without
   a restart. Only runs that ended with a Stop count; stale and failed runs say nothing about
   how long such a run takes. Sending an empty history clears it (the tray's Clear history).

3. **When a run is recorded.** Once, when it can no longer change: 60 s after its Stop if
   nobody is waiting on it (so a Stop-hook continuation can't follow), or when it goes
   stale, is replaced by a new prompt, or its session ends. A run first seen mid-way is never
   recorded, because its start time and prompt are unknown. A run that is still waiting for
   the user to come back is recorded when they do.

4. **Tier boundaries.** Under 30 s is Short, 30 s to under 5 min is Medium, 5 min and over is
   Long. With the design's floor values, a test command (120 s) gives Medium and a sub-agent
   (300 s) gives Long. Backlog item E3 says a test command "moves the run to the Long tier";
   the behaviour is the same, because Medium and Long share the 10 s away threshold.

5. **The return queue (H1).** A return settles the sessions waiting at the window the user
   came back to. A session waiting at a different window stays queued and gets its own
   return next. Because the user is then in a home app, not an away app, that return is the
   one-key prompt, not an automatic switch.

6. **Declare-long.** Applies to the running session whose home window is in front. It sets
   the Long tier, skips the elapsed-time and idle guards (pressing the key is input, and it
   is the user asking), and re-arms a switch cancelled earlier with "Not now". The home
   window, away app, pause and nobody-waiting guards still apply, as do the countdown and the
   input-quiet wait.

7. **Immersive apps** get a countdown of at least 5 s and a warning line on the toast.

8. **Ignore patterns.** `*` stays within one path segment, `**` crosses segments, `?` is one
   character, and a pattern that matches a folder also covers everything inside it. `\` and
   `/` are treated alike. Events from a matching folder are dropped before anything else,
   with no log line.

9. **LLM recap.** The core builds the prompt text and redacts it; the shell runs the CLI and
   reports the reply with the id it was given. The reply replaces the basic card only if it
   is for the latest card, arrives within `llm_timeout_seconds`, and has at least one
   non-empty line; at most three lines are kept and cleaned like any outside text.

10. **Redaction is deliberately narrow about bare words.** `password=x`, `token: x`,
    `--password x`, `API_KEY = x`, bearer tokens, known key prefixes and PEM blocks are
    hidden. A bare word such as "token" in a sentence is not treated as announcing a secret,
    so "fix the token refresh bug" is left alone.

11. **Run statistics** join the recap as one line (duration, tool calls, sub-agents,
    background tasks still running), which keeps the card within six lines.
