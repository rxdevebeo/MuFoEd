#!/usr/bin/env python3
"""Compare word-zone rectangles against `djvutxt --detail=word` (SC-D5).

Our producer is `strict-ooxml-djvu --example zones <file>` (JSON, points from
the top-left); the oracle prints pixels from the bottom-left at the page dpi.
The driver converts ours to the oracle's system and compares each word's
rectangle (default tolerance: 1 px) and text.

Rotated pages are skipped and counted: native-zone rotation is SC-D14, still
open, and comparing it now would assert the very thing under test. `djvutxt` is
GPL, invoked as a separate process only.

Usage:
    python3 xtool/djvu-zone-oracle.py --corpus DIR --ours target/debug/examples/zones \\
        --oracle /usr/bin/djvutxt --report out.json [--tolerance 1] [--raw-dir DIR] [--resume]
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import djvu_oracle_common as common  # noqa: E402

PAGE = re.compile(r"\(\s*page\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)")
WORD = re.compile(r"\bword\b\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)\s*(.*)")


def parse_oracle(text: str) -> list[dict]:
    pages: list[dict] = []
    height = 0
    for line in text.splitlines():
        header = PAGE.search(line)
        if header:
            height = int(header.group(4))
            pages.append({"height": height, "words": []})
            continue
        word = WORD.search(line)
        if word and pages:
            tail = word.group(5).strip().rstrip(")").strip()
            if len(tail) >= 2 and tail[0] == '"' and tail[-1] == '"':
                tail = tail[1:-1]
            pages[-1]["words"].append(
                (int(word.group(1)), int(word.group(2)), int(word.group(3)), int(word.group(4)), tail)
            )
    return pages


def convert(word: dict, page: dict) -> tuple[float, float, float, float, str]:
    scale = page["dpi"] / 72.0
    left = word["x"] * scale
    top = word["y"] * scale
    width = word["w"] * scale
    height = word["h"] * scale
    return left, page["height"] - (top + height), width, height, common.compact_text(word["text"])


def close(a: float, b: float, tolerance: float) -> bool:
    return abs(a - b) <= tolerance


def compare(ours: dict, oracle_pages: list[dict], tolerance: float) -> tuple[bool, dict]:
    skipped = 0
    our_words = 0
    oracle_words = 0
    checked = 0
    for index, page in enumerate(ours["pages"]):
        if page["rotation"]:
            skipped += 1
            continue
        if index >= len(oracle_pages):
            return False, {"detail": f"page {page['page']}: oracle has no page {index + 1}", "skipped_rotated": skipped}
        expected = oracle_pages[index]["words"]
        ours_words = page["words"]
        our_words += len(ours_words)
        oracle_words += len(expected)
        if len(ours_words) != len(expected):
            if not expected and ours_words:
                return False, {
                    "detail": f"page {page['page']}: oracle printed no words (format not parsed?)",
                    "skipped_rotated": skipped,
                }
            return False, {
                "detail": f"page {page['page']}: {len(ours_words)} words, oracle {len(expected)}",
                "skipped_rotated": skipped,
            }
        for position, (mine, reference) in enumerate(zip(ours_words, expected)):
            left, bottom, width, height, text = convert(mine, page)
            ref_left, ref_bottom, ref_width, ref_height, ref_text = reference
            if not (
                close(left, ref_left, tolerance)
                and close(bottom, ref_bottom, tolerance)
                and close(width, ref_width, tolerance)
                and close(height, ref_height, tolerance)
                and text == ref_text
            ):
                return False, {
                    "detail": (
                        f"page {page['page']} word {position}: ours "
                        f"({left:.1f},{bottom:.1f},{width:.1f},{height:.1f},{text!r}) vs oracle "
                        f"({ref_left},{ref_bottom},{ref_width},{ref_height},{ref_text!r})"
                    ),
                    "skipped_rotated": skipped,
                }
            checked += 1
    return True, {"skipped_rotated": skipped, "our_words": our_words, "oracle_words": oracle_words, "checked_words": checked}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--ours", type=Path, required=True, help="the zones example binary")
    parser.add_argument("--oracle", type=Path, required=True, help="the djvutxt binary")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--tolerance", type=float, default=1.0, help="allowed pixel drift per edge (SC-D5 expects 0)")
    parser.add_argument("--raw-dir", type=Path, default=None)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()

    previous = common.load_previous(args.report) if args.resume else {}
    results = []
    for source in common.iter_corpus(args.corpus):
        digest = common.sha256_file(source)
        produced = common.run([str(args.ours), str(source)])
        ours_sha = common.hashlib.sha256(produced.stdout).hexdigest()
        old = previous.get(source.name)
        if common.reused(old, digest, ours_sha):
            results.append(old)
            continue
        row = {"file": source.name, "sha256": digest, "ours_sha256": ours_sha}
        reference = common.run([str(args.oracle), "--detail=word", str(source)])
        if produced.returncode != 0:
            row.update(ok=False, detail=f"producer exit {produced.returncode}: {produced.stderr.decode(errors='replace')[:200]}")
        elif reference.returncode != 0:
            row.update(ok=False, detail=f"oracle exit {reference.returncode}: {reference.stderr.decode(errors='replace')[:200]}")
        else:
            ours = json.loads(produced.stdout)
            oracle_text = reference.stdout.decode("utf-8", errors="replace")
            ok, detail = compare(ours, parse_oracle(oracle_text), args.tolerance)
            row.update(ok=ok, **detail)
            if not ok and args.raw_dir is not None:
                args.raw_dir.mkdir(parents=True, exist_ok=True)
                (args.raw_dir / f"{source.name}.oracle.txt").write_text(oracle_text, encoding="utf-8")
                (args.raw_dir / f"{source.name}.ours.json").write_bytes(produced.stdout)
        print(source.name, "OK" if row.get("ok") else "MISMATCH", flush=True)
        results.append(row)

    report = {
        "criterion": "SC-D5",
        "what": "word-zone rectangles equal djvutxt --detail=word (0 px expected, 1 px allowed)",
        "producer": {"name": "strict-ooxml-djvu examples/zones"},
        "oracle": common.fingerprint(args.oracle, "djvulibre-bin"),
        "tolerance_px": args.tolerance,
        "skipped_rotated_total": sum(row.get("skipped_rotated", 0) for row in results),
        "files": len(results),
        "ok": sum(1 for row in results if row.get("ok")),
        "mismatched": [row["file"] for row in results if not row.get("ok")],
        "results": results,
    }
    common.write_report(args.report, report)
    print({"files": report["files"], "ok": report["ok"], "skipped_rotated": report["skipped_rotated_total"]})
    return 0 if report["ok"] == report["files"] else 1


if __name__ == "__main__":
    common.main_guard(main)
