#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Repository rules that a compiler doesn't enforce (design section 13). Run in CI.

1. The files a stranger needs in order to trust the project exist.
2. Every source file carries an SPDX license header.
3. Every direct third-party dependency has an entry in DEPENDENCIES.md.
4. Only focus-platform may contain `unsafe`; every other crate forbids it.

Exits non-zero and lists what is wrong.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REQUIRED = [
    "LICENSE", "README.md", "STATUS.md", "DEPENDENCIES.md", "SECURITY.md", "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md", "THREAT_MODEL.md", "PERMISSIONS.md", "AUDIT.md", "deny.toml",
    "Cargo.lock", "rust-toolchain.toml", ".github/CODEOWNERS", ".github/dependabot.yml",
    ".github/pull_request_template.md", ".github/workflows/ci.yml",
]
SOURCE_SUFFIXES = {".rs", ".toml", ".py", ".sh", ".js", ".css", ".yml"}
SKIP_DIRS = {"target", ".git", "node_modules"}
# Generated, or a format with no comment syntax.
NO_HEADER = {"Cargo.lock", "clippy.toml", "rustfmt.toml", "rust-toolchain.toml"}

problems = []


def sources():
    for path in ROOT.rglob("*"):
        if not path.is_file() or SKIP_DIRS & set(path.relative_to(ROOT).parts):
            continue
        if path.suffix in SOURCE_SUFFIXES and path.name not in NO_HEADER:
            yield path


for name in REQUIRED:
    if not (ROOT / name).is_file():
        problems.append(f"missing required file: {name}")

for path in sources():
    head = "\n".join(path.read_text(encoding="utf-8").splitlines()[:3])
    if "SPDX-License-Identifier: Apache-2.0" not in head:
        problems.append(f"no SPDX header in the first three lines: {path.relative_to(ROOT)}")

listed = (ROOT / "DEPENDENCIES.md").read_text(encoding="utf-8")
for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
    section = None
    for line in manifest.read_text(encoding="utf-8").splitlines():
        header = re.match(r"\[(.+)\]", line.strip())
        if header:
            section = header.group(1)
            continue
        dependency = re.match(r"([A-Za-z0-9_-]+)\s*=", line)
        if not dependency or not section or not section.endswith("dependencies"):
            continue
        name = dependency.group(1)
        if "path =" in line or name.startswith("focus"):
            continue
        if not re.search(rf"(?<![A-Za-z0-9_-]){re.escape(name)}(?![A-Za-z0-9_-])", listed):
            problems.append(f"{name} ({manifest.parent.name}) has no entry in DEPENDENCIES.md")

for lib in sorted((ROOT / "crates").glob("*/src/*.rs")):
    if lib.name not in ("lib.rs", "main.rs"):
        continue
    crate = lib.parent.parent.name
    forbids = "#![forbid(unsafe_code)]" in lib.read_text(encoding="utf-8")
    if crate == "focus-platform":
        continue
    if not forbids:
        problems.append(f"{crate} must set #![forbid(unsafe_code)] in {lib.name}")

if problems:
    print("\n".join(problems))
    sys.exit(1)
print("repository checks passed")
