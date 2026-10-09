#!/usr/bin/env python3
"""Add one `maparea` URL annotation to a DjVu page, for the SC-D9 oracle.

The CC0 validation corpus carries no `ANTa`/`ANTz`, so SC-D9 can only be
exercised on a synthetic fixture. `djvused` writes the annotation chunk; its
`set-ant` command is GPL software run as a separate process, never linked.

The rectangle is in native page pixels from the bottom-left; a page-covering
rectangle matches the text zones so the link maps to text.

Usage:
    python3 xtool/djvu-annotate-fixture.py --input book.djvu --out annotated.djvu \\
        --url https://example.invalid/djvu-link [--page 1] [--rect "0 0 20000 20000"]
"""
from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--djvused", type=Path, default=Path("djvused"))
    parser.add_argument("--page", type=int, default=1)
    parser.add_argument("--rect", default="0 0 20000 20000")
    parser.add_argument("--url", required=True)
    args = parser.parse_args()

    ant = f'(maparea "" (rect {args.rect}) (url "{args.url}"))\n'
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(args.input.read_bytes())
    with tempfile.NamedTemporaryFile(
        "w", suffix=".ant", delete=False, encoding="utf-8"
    ) as handle:
        handle.write(ant)
        ant_path = handle.name
    try:
        proc = subprocess.run(
            [
                str(args.djvused),
                str(args.out),
                "-e",
                f"select {args.page}; set-ant {ant_path}",
                "-s",
            ],
            capture_output=True,
            timeout=120,
        )
    finally:
        Path(ant_path).unlink(missing_ok=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", errors="replace"))
        return 1
    print(args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
