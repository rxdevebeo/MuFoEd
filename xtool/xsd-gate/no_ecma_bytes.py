#!/usr/bin/env python3
"""G-6: not one byte of ECMA enters this repository, and the claim is checked.

`STAGE-10G-TASK.md` G1 and G8, and ADR-0014. ECMA-376-1 5th edition is published
under ECMA's default copyright notice, which forbids modifying the deliverable and
requires republication to be "unchanged, and up to date". So the schemas are
downloaded at run time and never committed, and the patched copy that the gate
actually validates against lives in the user's cache.

An assertion like that decays into a promise the first time somebody needs the
schemas to work offline, so this is a script and it runs in CI next to the gate.

What it accepts, and why:

  - ONE `.xsd` in the tree, `xtool/xsd-gate/xml.xsd`, and only when its bytes are
    the ones in `xml.xsd.sha256`. That file is OURS: it declares the four
    attributes the XML Namespaces specification fixes, which the ECMA archive
    does not ship and does not need to ship - it is the complement the notice
    permits, not an edit of their deliverable. Checking the digest as well as the
    path is what stops "our file" from quietly becoming theirs.
  - nothing else. Every other `.xsd`, anywhere, is a failure.

Exit codes: 0 clean, 1 ECMA bytes found, 2 our file changed or gone.
"""

from __future__ import annotations

import hashlib
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUR_FILE = os.path.join("xtool", "xsd-gate", "xml.xsd")
OUR_DIGEST = os.path.join(HERE, "xml.xsd.sha256")

SKIP_DIRECTORIES = {".git", "target", "node_modules", ".playwright-mcp", ".page-diff"}


def main() -> int:
    expected = open(OUR_DIGEST, encoding="utf-8").read().split()[0].strip().lower()
    actual = hashlib.sha256(open(os.path.join(REPO, OUR_FILE), "rb").read()).hexdigest()
    if actual != expected:
        print(
            f"error: {OUR_FILE} is {actual}, but xml.xsd.sha256 pins {expected}.\n"
            "       Either the file was edited without the digest being updated, or it\n"
            "       stopped being our own twelve lines. Find out which before it matters.",
            file=sys.stderr,
        )
        return 2

    found = []
    for root, directories, files in os.walk(REPO):
        directories[:] = [d for d in directories if d not in SKIP_DIRECTORIES]
        for name in files:
            if not name.lower().endswith(".xsd"):
                continue
            path = os.path.join(root, name)
            relative = os.path.relpath(path, REPO).replace("\\", "/")
            if relative != OUR_FILE.replace("\\", "/"):
                found.append((relative, os.path.getsize(path)))

    if found:
        print("error: ECMA schema bytes in the repository:", file=sys.stderr)
        for path, size in sorted(found):
            print(f"  {path} ({size} bytes)", file=sys.stderr)
        print(
            "\n       The notice forbids modifying the deliverable, and shipping a\n"
            "       patched copy would put a modified deliverable in a public tree. Keep\n"
            "       them in the cache: xtool/xsd-gate/schemas.toml pins the source and\n"
            "       xsd_gate.py fetches and patches it at run time.",
            file=sys.stderr,
        )
        return 1

    print(f"no ECMA bytes in the tree; the one .xsd is ours and matches its pinned digest")
    return 0


if __name__ == "__main__":
    sys.exit(main())
