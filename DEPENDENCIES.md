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

`focus-core`, `focus-platform` and `focus-headless` use the standard library and workspace crates only. The starter list
for the remaining crates is in section 5 of the design.
