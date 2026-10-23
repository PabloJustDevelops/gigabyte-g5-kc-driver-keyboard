#!/usr/bin/env python3
"""Fail if `PKGBUILD`'s `source` array and the tree have drifted apart.

The array is maintained by hand — one line per panel source file, doc, unit and
rule — and every sum in it is `SKIP`, so a file that is never added is not a
build error: it is a package that quietly ships without it. `docs/
VISUAL-REWORK.md` was exactly that, for a week. The check is cheap and the
failure is loud, so CI runs it on every push (see the `python` job in
`.github/workflows/ci.yml`).

Two rules, both needed:

* everything in the array exists (a rename that forgets the array);
* everything that *should* ship is in the array (a new file that forgets the
  array) — `SHIPPED` below is what "should ship" means.

Usage: python tools/check_packaging.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Glob patterns for the files the package ships, relative to the repo root.
# Deliberately explicit: a broad `gui/**` would drag in `gui/target` and
# `gui/tools`, which are not packaging inputs (the panel is built from source
# inside the PKGBUILD, and measure.py is a development script).
SHIPPED = (
    "src/*.py",
    "systemd/*",
    "systemd/system-sleep/*",
    "polkit/*",
    # NOT kernel/*.c: the module build leaves g5kbd.mod.c next to its source,
    # and a glob would call that a file which should ship.
    "kernel/g5kbd.c",
    "kernel/Makefile",
    "kernel/*.rules",
    # One cargo workspace at the root (Cargo.lock included: makepkg builds with
    # --frozen, so the lock file has to travel).
    "Cargo.toml",
    "Cargo.lock",
    "crates/*/Cargo.toml",
    "crates/*/src/*.rs",
    "crates/*/tests/*.rs",
    "crates/*/testdata/*",
    "gui/Cargo.toml",
    "gui/src/*.rs",
    "gui/src/backend/*.rs",
    "gui/tests/*.rs",
    "gui/fonts/*",
    "gui/g5kbd-gui.desktop",
    "gui/icons/*",
    "README.md",
    "docs/*.md",
)

# Documentation the install script copies, by absolute install path. Keeping
# this next to the PKGBUILD check catches the other half of the same mistake:
# a doc that travels with makepkg but not with `sudo ./install.sh`.
INSTALL_DOCS = ("README.md", "docs/FAN-RESEARCH.md", "docs/WINDOWS-RESEARCH.md",
                "docs/ROADMAP.md", "docs/VISUAL-REWORK.md")


def source_array(text: str) -> list[str]:
    """The quoted entries of `source=(...)` in a PKGBUILD.

    Scanned by hand rather than matched with one regex: the array's closing paren
    shares a line with its last entry, and entries are commented in place.
    """
    if "source=(" not in text:
        raise SystemExit("PKGBUILD: no source=(...) array found")
    index = text.index("source=(") + len("source=(")
    depth = 1
    quoted = False
    entries: list[str] = []
    current: list[str] = []
    while index < len(text) and depth > 0:
        char = text[index]
        if quoted:
            if char == '"':
                quoted = False
                entries.append("".join(current))
                current = []
            else:
                current.append(char)
        elif char == '"':
            quoted = True
        elif char == "#":  # a comment runs to the end of the line
            newline = text.find("\n", index)
            index = len(text) if newline < 0 else newline
            continue
        elif char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        index += 1
    return entries


def shipped_files() -> set[str]:
    found: set[str] = set()
    for pattern in SHIPPED:
        found.update(
            str(p.relative_to(ROOT)) for p in ROOT.glob(pattern) if p.is_file()
        )
    return found


def main() -> int:
    pkgbuild = (ROOT / "PKGBUILD").read_text(encoding="utf-8")
    listed = source_array(pkgbuild)
    expected = shipped_files()

    problems = []
    for entry in listed:
        if not (ROOT / entry).is_file():
            problems.append(f"PKGBUILD lists {entry}, which is not in the tree")
    for entry in sorted(expected - set(listed)):
        problems.append(f"{entry} should ship, but PKGBUILD does not list it")
    for entry in sorted(set(listed) - expected):
        problems.append(f"PKGBUILD lists {entry}, which is not a packaged input")

    install = (ROOT / "install.sh").read_text(encoding="utf-8")
    for doc in INSTALL_DOCS:
        if doc not in install:
            problems.append(f"install.sh does not install {doc}")

    if problems:
        print("packaging check failed:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print(f"packaging: {len(listed)} sources listed, {len(expected)} shipped, in sync")
    return 0


if __name__ == "__main__":
    sys.exit(main())
