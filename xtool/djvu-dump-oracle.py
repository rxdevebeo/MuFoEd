#!/usr/bin/env python3
"""Compare our chunk index against `djvudump` (SC-D3, SC-D13).

Our producer is `strict-ooxml djvu-dump <file>`; the oracle is `djvudump <file>`.
Both print one `ID[:FORM] [length]` per chunk with indentation for nesting, so
the comparison is over the normalized *tree* — the ordered
`(indent-rank, id, form, length)` tuples — not over raw text: `djvudump` adds
annotations and we append `@offset`, and the two use different indent widths.

`djvudump` is GPL software, run as a separate process; nothing here links it or
reads its source. The binary hash and its `djvulibre-bin` version are recorded
in the report so a result is tied to an exact oracle.

Usage:
    python3 xtool/djvu-dump-oracle.py --corpus DIR --ours TARGET/strict-ooxml \\
        --oracle /usr/bin/djvudump --report out.json [--resume] [--raw-dir DIR]
    # SC-D13: synthesize an indirect copy with djvmcvt and compare that too
    ... --indirect-dir DIR --djvmcvt /usr/bin/djvmcvt
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import djvu_oracle_common as common  # noqa: E402

CHUNK = re.compile(
    r"^(?P<indent>[ \t]*)(?P<id>[A-Za-z0-9_]{4})(?::(?P<form>[A-Za-z0-9_]{4}))?\s*\[(?P<len>\d+)\]"
)


def normalize(text: str, first_root: bool = False) -> list[tuple]:
    raw: list[tuple[int, str, str, int]] = []
    for line in text.splitlines():
        match = CHUNK.match(line)
        if not match:
            continue
        indent = len(match.group("indent").replace("\t", "    "))
        raw.append((indent, match.group("id"), match.group("form") or "", int(match.group("len"))))
    ranks = {indent: rank for rank, indent in enumerate(sorted({r[0] for r in raw}))}
    entries = [(ranks[indent], chunk_id, form, length) for indent, chunk_id, form, length in raw]
    if first_root:
        # An indirect index resolves its components; `djvudump` prints the index
        # only, so keep just the first root subtree (FORM:DJVM + DIRM) and stop
        # at the next top-level node our reader appended.
        kept: list[tuple] = []
        roots = 0
        for entry in entries:
            if entry[0] == 0:
                roots += 1
                if roots == 2:
                    break
            kept.append(entry)
        entries = kept
    return entries


def first_divergence(ours: list[tuple], oracle: list[tuple]) -> str | None:
    for index, (left, right) in enumerate(zip(ours, oracle)):
        if left != right:
            return f"#{index}: ours {left} vs oracle {right}"
    if len(ours) != len(oracle):
        longer = "ours" if len(ours) > len(oracle) else "oracle"
        tail = ours[len(oracle) :] if len(ours) > len(oracle) else oracle[len(ours) :]
        return f"length: {len(ours)} vs {len(oracle)}; {longer} has {tail[:3]} more"
    return None


def compare_one(
    producer: list[str], oracle: list[str], work: Path, raw_dir: Path | None, first_root: bool = False
) -> dict:
    ours = common.run(producer)
    reference = common.run(oracle)
    if ours.returncode != 0:
        return {"ok": False, "detail": f"producer exit {ours.returncode}: {ours.stderr.decode(errors='replace')[:200]}"}
    if reference.returncode != 0:
        return {"ok": False, "detail": f"oracle exit {reference.returncode}: {reference.stderr.decode(errors='replace')[:200]}"}
    ours_text = ours.stdout.decode("utf-8", errors="replace")
    ours_tree = normalize(ours_text, first_root)
    oracle_tree = normalize(reference.stdout.decode("utf-8", errors="replace"))
    divergence = first_divergence(ours_tree, oracle_tree)
    row = {
        "ok": divergence is None,
        "our_chunks": len(ours_tree),
        "oracle_chunks": len(oracle_tree),
        "detail": "chunk trees match" if divergence is None else divergence,
    }
    if first_root:
        full = normalize(ours_text)
        row["resolved_pages"] = sum(1 for entry in full if entry[0] == 0) - 1
    if divergence is not None and raw_dir is not None:
        raw_dir.mkdir(parents=True, exist_ok=True)
        (raw_dir / f"{work.name}.ours.txt").write_bytes(ours.stdout)
        (raw_dir / f"{work.name}.oracle.txt").write_bytes(reference.stdout)
    return row


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--ours", type=Path, required=True, help="the strict-ooxml binary")
    parser.add_argument("--oracle", type=Path, required=True, help="the djvudump binary")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--raw-dir", type=Path, default=None)
    parser.add_argument("--indirect-dir", type=Path, default=None, help="synthesize indirect copies here (SC-D13)")
    parser.add_argument("--djvmcvt", type=Path, default=Path("djvmcvt"))
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()

    previous = common.load_previous(args.report) if args.resume else {}
    if args.indirect_dir is not None:
        args.indirect_dir.mkdir(parents=True, exist_ok=True)
    results = []
    for source in common.iter_corpus(args.corpus):
        digest = common.sha256_file(source)
        old = previous.get(source.name)
        if common.reused(old, digest):
            results.append(old)
            continue
        ours = [str(args.ours), "djvu-dump", str(source)]
        row = {"file": source.name, "sha256": digest, "mode": "direct"}
        row.update(compare_one(ours, [str(args.oracle), str(source)], source, args.raw_dir))
        if args.indirect_dir is not None:
            target = args.indirect_dir / source.name
            # djvmcvt -i <doc_in> <dir_out> <idx_fname>: component files land in
            # <dir_out> and the top-level index is written to <idx_fname>.
            made = common.run(
                [str(args.djvmcvt), "-i", str(source), str(args.indirect_dir), str(target)]
            )
            if made.returncode != 0:
                row["indirect"] = {"ok": False, "detail": f"djvmcvt exit {made.returncode}"}
            else:
                row["indirect"] = compare_one(
                    [str(args.ours), "djvu-dump", str(target)],
                    [str(args.oracle), str(target)],
                    target,
                    args.raw_dir,
                    first_root=True,
                )
                row["ok"] = row["ok"] and row["indirect"]["ok"]
        print(source.name, "OK" if row["ok"] else "MISMATCH", flush=True)
        results.append(row)

    report = {
        "criterion": "SC-D3/SC-D13",
        "what": "our chunk index equals djvudump, including a synthesized indirect copy",
        "producer": {"name": "strict-ooxml djvu-dump"},
        "oracle": common.fingerprint(args.oracle, "djvulibre-bin"),
        "normalization": "ordered (indent-rank, id, form, byte length) tuples; annotations and @offset dropped",
        "files": len(results),
        "ok": sum(1 for row in results if row["ok"]),
        "mismatched": [row["file"] for row in results if not row["ok"]],
        "results": results,
    }
    common.write_report(args.report, report)
    print({"files": report["files"], "ok": report["ok"]})
    return 0 if report["ok"] == report["files"] else 1


if __name__ == "__main__":
    common.main_guard(main)
