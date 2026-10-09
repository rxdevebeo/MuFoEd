#!/usr/bin/env python3
"""Announce the annotation/hyperlink gap against `djvused` (SC-D9).

SC-D9 is not implemented: our reader does not turn `ANTa`/`ANTz` `maparea`
annotations into `w:hyperlink` yet. This driver measures the gap instead of
failing it: for every file it counts the oracle's resolvable URLs and, when a
producer is supplied, our own count. With no producer it is an inventory and
always exits 0; with `--ours` it gates an equal count.

`djvused` needs a page selected before `print-ant`, so the driver reads the page
count with `... -e n` and chains `select P; print-ant` for every page in one
call. The CC0 validation corpus carries no `ANTa`/`ANTz` at all, so its oracle
total is 0; SC-D9 can only be exercised on a synthetic annotated fixture.

`djvused` is GPL, invoked as a separate process only.

Usage:
    python3 xtool/djvu-ant-oracle.py --corpus DIR --oracle /usr/bin/djvused \\
        --report out.json [--ours PRODUCER] [--resume]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import djvu_oracle_common as common  # noqa: E402

MAPAREA = re.compile(r"\(maparea\b")
PAGES = re.compile(r"(\d+)")


def oracle_mapareas(djvused: Path, source: Path) -> tuple[int | None, str]:
    count = common.run([str(djvused), "-e", "n", str(source)])
    if count.returncode != 0:
        return None, f"djvused n exit {count.returncode}"
    numbers = PAGES.findall(count.stdout.decode("utf-8", errors="replace"))
    if not numbers:
        return None, "djvused n printed no page count"
    pages = int(numbers[-1])
    # One invocation, one select per page: annotations are rare and this keeps
    # the process count at one per file.
    command = "; ".join(f"select {page}; print-ant" for page in range(1, pages + 1))
    proc = common.run([str(djvused), "-e", command, str(source)])
    if proc.returncode != 0:
        return None, f"djvused print-ant exit {proc.returncode}"
    return len(MAPAREA.findall(proc.stdout.decode("utf-8", errors="replace"))), "ok"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True, help="the djvused binary")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--ours", type=Path, default=None, help="optional producer printing {\"hyperlinks\": N}")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()

    previous = common.load_previous(args.report) if args.resume else {}
    results = []
    total_mapareas = 0
    total_hyperlinks = 0
    for source in common.iter_corpus(args.corpus):
        digest = common.sha256_file(source)
        old = previous.get(source.name)
        if common.reused(old, digest):
            results.append(old)
            total_mapareas += old.get("oracle_mapareas") or 0
            total_hyperlinks += old.get("our_hyperlinks") or 0
            continue
        count, detail = oracle_mapareas(args.oracle, source)
        row = {"file": source.name, "sha256": digest, "oracle_mapareas": count, "oracle_detail": detail}
        if count is not None:
            total_mapareas += count
        if args.ours is not None:
            produced = common.run([str(args.ours), str(source)])
            if produced.returncode == 0:
                row["our_hyperlinks"] = int(produced.stdout.decode("utf-8", errors="replace").strip() or "0")
                total_hyperlinks += row["our_hyperlinks"]
            else:
                row["our_hyperlinks"] = None
                row["producer_detail"] = f"producer exit {produced.returncode}"
        row["ok"] = args.ours is None or row.get("oracle_mapareas") == row.get("our_hyperlinks")
        print(source.name, row.get("oracle_mapareas"), flush=True)
        results.append(row)

    report = {
        "criterion": "SC-D9",
        "status": "open" if args.ours is None else "gated",
        "what": "count of maparea annotations vs our hyperlinks; not implemented until SC-D9 lands",
        "oracle": common.fingerprint(args.oracle, "djvulibre-bin"),
        "oracle_mapareas_total": total_mapareas,
        "our_hyperlinks_total": total_hyperlinks if args.ours is not None else 0,
        "deficit": total_mapareas - (total_hyperlinks if args.ours is not None else 0),
        "files": len(results),
        "ok": sum(1 for row in results if row["ok"]),
        "mismatched": [row["file"] for row in results if not row["ok"]],
        "results": results,
    }
    common.write_report(args.report, report)
    print({"files": report["files"], "oracle_mapareas": total_mapareas, "status": report["status"]})
    if args.ours is None:
        return 0
    return 0 if report["ok"] == report["files"] else 1


if __name__ == "__main__":
    common.main_guard(main)
