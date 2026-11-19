<!-- SPDX-License-Identifier: Apache-2.0 -->
# Releasing

Prebuilt downloads come from `.github/workflows/release.yml`. It runs when a version tag is
pushed, and nothing else starts it.

## Cutting a release (the owner)

1. Set `version` in `[workspace.package]` of `Cargo.toml`, let `Cargo.lock` follow, merge.
2. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`. The tag must equal `v` plus the
   Cargo version; the workflow stops if it does not. A suffix such as `-rc1` makes a
   pre-release.
3. The workflow makes a **draft** release and attaches, per platform, the archive and its
   `.sha256` file:

   | Platform | Archive |
   | --- | --- |
   | macOS, Apple silicon | `focus-autopilot-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
   | macOS, Intel | `focus-autopilot-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
   | Windows, x64 | `focus-autopilot-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

   Each holds `focusd`, `focusctl` (`.exe` on Windows), `LICENSE` and `READ-ME-FIRST.txt`
   (from `docs/release-readme.txt`): unzip, run `focusctl setup`, what to do about the
   unsigned-binary warning, and a link to the repository. The repository's `README.md` and
   `docs/` are not in the archive because their relative links would not resolve there.
4. Download one, check it, then publish the draft in the GitHub page.

## Checking a download

```bash
sha256sum -c focus-autopilot-vX.Y.Z-aarch64-apple-darwin.tar.gz.sha256   # macOS: shasum -a 256 -c
```

On Windows: compare `(Get-FileHash <file>.zip -Algorithm SHA256).Hash` with the `.sha256` file.
The checksum only shows the file is the one the workflow uploaded. It does not show who made it;
that needs a signature.

## What is not done: signing

The binaries are **not signed or notarized**. Expect a warning on first run: macOS Gatekeeper
("cannot be opened because the developer cannot be verified"; right-click, Open, or
`xattr -d com.apple.quarantine`), Windows SmartScreen ("Windows protected your PC"; More info,
Run anyway). Signing needs things only the owner can obtain:

- **macOS:** an Apple Developer Program membership (US$99 a year), a "Developer ID Application"
  certificate, and an app-specific password or API key for `notarytool`. Added to the workflow
  as repository secrets, used in a protected `release` environment, plus `codesign` and
  `xcrun notarytool submit --wait`. The binaries would ship as a `.dmg` or `.zip`, since a bare
  command-line binary cannot carry a stapled ticket.
- **Windows:** a code-signing certificate (an OV or EV certificate from a certificate authority,
  or an approved SignPath or Azure Trusted Signing account), its credentials as secrets, and a
  `signtool` step.

Not built either: an SBOM, build-provenance attestations, and a combined `SHA256SUMS` file.
This workflow has never run; see STATUS.md.
