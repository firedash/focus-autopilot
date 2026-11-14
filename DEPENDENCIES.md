# Dependencies

Every direct dependency needs an entry here: purpose, license, and why the standard library
isn't enough. CI will fail when one is added without an entry.

| Crate | Used by | Purpose | License | Why not std |
| --- | --- | --- | --- | --- |
| serde, serde_json | focus-receiver | Typed parsing of hook payloads | MIT/Apache-2.0 | Parsing untrusted JSON by hand is the riskier choice |
| toml | focus-receiver | Reading config.toml | MIT/Apache-2.0 | Same |
| subtle | focus-receiver | Constant-time token comparison | BSD-3-Clause | std has no constant-time compare |
| sha2 | focus-receiver | Salted project keys for run history | MIT/Apache-2.0 | std has no hash suited to this |
| getrandom | focus-receiver | Random token and salt | MIT/Apache-2.0 | std has no OS randomness API |
| hyper, hyper-util, http-body-util | focus-receiver | Loopback HTTP server | MIT | A hand-written HTTP parser would be the weakest part |
| tokio | focus-receiver | Runtime for hyper (single thread, no `rt-multi-thread`) | MIT | hyper needs an executor |
| toml_edit | focusctl | Edit config.toml without losing the user's comments | MIT/Apache-2.0 | Same |
| serde_json | focus-runner | Reading and writing `runs.jsonl` | MIT/Apache-2.0 | Same |
| serde_json `preserve_order` | focusctl | Keep key order when merging settings.json | MIT/Apache-2.0 | Same |
| objc2, objc2-foundation, objc2-app-kit, objc2-core-graphics | focus-platform (macOS only) | `NSWorkspace`, `NSRunningApplication`, idle time, sound | MIT/Apache-2.0/Zlib | OS APIs; typed bindings instead of raw FFI |
| Carbon framework (system) | focus-platform (macOS only) | Global hotkeys via `RegisterEventHotKey`, hand-written FFI with a safety comment on each `unsafe` block | Apple system framework | No permission needed, unlike key-event monitoring; not in `objc2` |
| tempfile | focusctl (tests) | Temporary folders for file tests | MIT/Apache-2.0 | Tests only |

`focus-core` uses the standard library only, with no dependencies at all. `focus-platform`
uses the standard library everywhere except macOS, where it uses the `objc2` crates above and
declares a few functions from the system Carbon framework by hand (`RegisterEventHotKey` and
the event calls around it), because the `objc2` crates have no binding for them; it needs no
permission. `focus-runner`, `focus-headless` and `focusd` use workspace crates plus
`serde_json` (runner only). The starter list is in section 5 of the design.
