#!/usr/bin/env python3
"""Fail unless named source files meet a line-coverage floor (R10 / REWORK §15.1).

Reads a cargo-llvm-cov `--json --summary-only` export and checks each path
suffix. Prints numerator/denominator for every checked file and exits non-zero
when any file is below `--min` (default 100).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", required=True, type=Path)
    parser.add_argument(
        "--file",
        action="append",
        dest="files",
        required=True,
        help="Path suffix to require (repeatable), e.g. xml/escape.rs",
    )
    parser.add_argument("--min", type=float, default=100.0)
    args = parser.parse_args()

    data = json.loads(args.json.read_text(encoding="utf-8"))
    files = data.get("data", [{}])[0].get("files", [])
    by_suffix: dict[str, tuple[str, int, int, float]] = {}
    for fobj in files:
        fn = fobj.get("filename", "").replace("\\", "/")
        lines = fobj.get("summary", {}).get("lines", {})
        covered = int(lines.get("covered", 0))
        total = int(lines.get("count", 0))
        pct = (100.0 * covered / total) if total else 0.0
        for suffix in args.files:
            if fn.endswith(suffix.replace("\\", "/")):
                by_suffix[suffix] = (fn, covered, total, pct)

    failed = 0
    for suffix in args.files:
        if suffix not in by_suffix:
            print(f"MISSING {suffix}: not present in coverage JSON", file=sys.stderr)
            failed += 1
            continue
        fn, covered, total, pct = by_suffix[suffix]
        status = "PASS" if pct + 1e-9 >= args.min else "FAIL"
        print(f"{status} {pct:.4f}% {covered}/{total} min={args.min} {fn}")
        if status == "FAIL":
            failed += 1
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
