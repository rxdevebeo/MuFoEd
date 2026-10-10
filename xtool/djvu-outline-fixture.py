#!/usr/bin/env python3
"""Add a `NAVM` outline to a DjVu file, for the SC-D9 outline oracle.

The CC0 validation corpus carries no `NAVM`, so SC-D9 L3 can only be exercised
on a synthetic fixture. `djvused` writes the outline chunk (`set-outline`); it
is GPL software run as a separate process, never linked.

The expression passed to `--outline` is the same notation `djvused print-outline`
emits: a `(bookmarks …)` wrapper around `("title" "url" child…)` nodes.

Usage:
    python3 xtool/djvu-outline-fixture.py --input book.djvu --out outline.djvu \\
        --djvused /usr/bin/djvused [--outline "(bookmarks …)"]
"""
from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

DEFAULT_OUTLINE = """(bookmarks
 ("Chapter" "#page=1"
  ("Section" "#page=2"))
 ("Appendix" "#page=3"))
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--djvused", type=Path, default=Path("djvused"))
    parser.add_argument("--outline", default=DEFAULT_OUTLINE, help="the outline S-expression")
    args = parser.parse_args()

    outline = args.outline if args.outline.endswith("\n") else args.outline + "\n"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(args.input.read_bytes())
    with tempfile.NamedTemporaryFile(
        "w", suffix=".outline", delete=False, encoding="utf-8"
    ) as handle:
        handle.write(outline)
        outline_path = handle.name
    try:
        proc = subprocess.run(
            [str(args.djvused), str(args.out), "-e", f"set-outline {outline_path}", "-s"],
            capture_output=True,
            timeout=120,
        )
    finally:
        Path(outline_path).unlink(missing_ok=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", errors="replace"))
        return 1
    print(args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
