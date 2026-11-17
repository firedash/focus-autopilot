# Contributing

Thanks for looking. This project's promise is that a stranger can audit it in an afternoon,
so changes are judged on whether they keep it small, local and honest about what it does.

## Before you start

- Read [STATUS.md](STATUS.md) for what exists and what has actually been run on a real
  machine, and [docs/adr/](docs/adr/) for the decisions already made and why.
- For anything beyond a small fix, open an issue first so the approach can be agreed.
- Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Building and checking

Rust is pinned in `rust-toolchain.toml`; `rustup` installs it on first use.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
python3 scripts/check-repo.py
```

The macOS and Windows code only builds on those systems, but both can be type-checked from
Linux, and CI builds and tests on all three:

```bash
rustup target add aarch64-apple-darwin x86_64-pc-windows-msvc
cargo clippy --workspace --all-targets --target aarch64-apple-darwin
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc
```

If you change the statistics and configuration page, also run it in a browser (needs Python,
Node.js and Playwright):

```bash
scripts/ui-check/run.sh
```

`cargo deny check` runs the advisory, license and source rules in `deny.toml`.

## Rules the code follows

- **Decisions live in `focus-core`**, which does no I/O and has no dependencies. Shell crates
  carry out what it decides. A new behaviour is a core change with a test, not shell logic.
- **`unsafe` only in `focus-platform`**, each block with a `// SAFETY:` comment.
- **No `unwrap`, `expect`, `panic` or `todo` outside tests.** The workspace lints deny them.
- **Untrusted text stays text.** Hook payloads, app names and paths are cleaned, bounded and
  redacted on the way in, and shown with `textContent` in the page.
- **A new dependency needs a reason**, written in [DEPENDENCIES.md](DEPENDENCIES.md).
- **A new permission, network call, file write or spawned process** needs a stated reason and
  an update to [PERMISSIONS.md](PERMISSIONS.md) and [AUDIT.md](AUDIT.md). The pull request
  template asks.
- **Say what was run.** If platform code was only type-checked, say so in the pull request
  and in STATUS.md. "Compiles" and "works on a Mac" are different claims.

## Commits and sign-off

- Write commit subjects in the imperative, and say why in the body.
- Sign off your commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org/).
- Contributions are licensed under Apache-2.0, like the rest of the project.

## Conduct

Be decent to each other. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
