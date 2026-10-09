#!/usr/bin/env python3
"""Compare our page raster against `ddjvu -format=ppm` (SC-D7).

Our producer is the `codec_probe` example (`input.djvu PAGE out.png`); the oracle
is `ddjvu`. The driver decodes both images and reports per-page exact/mean/max
channel error. Pages are exact by default; an IW44 page may carry a measured
tolerance in a tolerance JSON (SC-D7's `diff-tolerance.md`), and every tolerance
must be justified in that document, never chosen to fit a failure.

Pages where both sides refuse to produce an image are skipped; one-sided refusals
are mismatches. `ddjvu` is GPL, invoked as a separate process only.

Usage:
    python3 xtool/djvu-render-oracle.py --corpus DIR --ours target/debug/examples/codec_probe \\
        --oracle /usr/bin/ddjvu --report out.json [--pages 1,3,5-7] [--tolerance tol.json] \\
        [--foreground] [--raw-dir DIR] [--resume]
"""
from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import djvu_oracle_common as common  # noqa: E402


def parse_pages(spec: str) -> list[int]:
    pages: set[int] = set()
    for part in spec.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            start, end = part.split("-", 1)
            pages.update(range(int(start), int(end) + 1))
        else:
            pages.add(int(part))
    return sorted(pages)


def tolerance_for(tolerances: dict, name: str) -> dict | None:
    if name in tolerances:
        return tolerances[name]
    if "*" in tolerances:
        return tolerances["*"]
    return tolerances.get("default")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--ours", type=Path, required=True, help="the codec_probe example binary")
    parser.add_argument("--oracle", type=Path, required=True, help="the ddjvu binary")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--pages", default="1", help="pages to compare on every file, e.g. 1,3,5-7")
    parser.add_argument("--tolerance", type=Path, default=None)
    parser.add_argument("--foreground", action="store_true", help="mask-only composite")
    parser.add_argument("--raw-dir", type=Path, default=None)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()

    pages = parse_pages(args.pages)
    tolerances = common.load_tolerances(args.tolerance)
    previous = common.load_previous(args.report) if args.resume else {}
    results = []
    for source in common.iter_corpus(args.corpus):
        digest = common.sha256_file(source)
        old = previous.get(source.name)
        if common.reused(old, digest):
            results.append(old)
            continue
        row = {"file": source.name, "sha256": digest, "pages": []}
        for page in pages:
            with tempfile.TemporaryDirectory() as scratch:
                ours_png = Path(scratch) / "ours.png"
                oracle_ppm = Path(scratch) / "oracle.ppm"
                ours_argv = [str(args.ours), str(source), str(page), str(ours_png)]
                if args.foreground:
                    ours_argv.append("--foreground")
                ours = common.run(ours_argv)
                oracle_argv = [
                    str(args.oracle),
                    "-format=ppm",
                    f"-page={page}",
                ]
                if args.foreground:
                    oracle_argv.append("-mode=foreground")
                oracle_argv += [str(source), str(oracle_ppm)]
                reference = common.run(oracle_argv)
                entry = {"page": page}
                if ours.returncode != 0 and reference.returncode != 0:
                    entry.update(skipped=True, detail="both sides have no image")
                elif ours.returncode != 0:
                    entry.update(ok=False, detail="producer has no image, oracle does")
                elif reference.returncode != 0:
                    entry.update(ok=False, detail=f"oracle exit {reference.returncode}")
                else:
                    try:
                        ow, oh, opixels = common.decode_png_rgb(ours_png.read_bytes())
                        rw, rh, rpixels = common.decode_pnm_rgb(oracle_ppm.read_bytes())
                    except (ValueError, OSError) as error:
                        entry.update(ok=False, detail=f"decode: {error}")
                    else:
                        if (ow, oh) != (rw, rh):
                            entry.update(ok=False, detail=f"size: ours {ow}x{oh}, oracle {rw}x{rh}")
                        else:
                            stats = common.diff_stats(opixels, rpixels)
                            tolerance = tolerance_for(tolerances, source.name)
                            entry.update(stats)
                            entry["ok"] = common.within(stats, tolerance)
                            entry["tolerance"] = tolerance
                            entry["detail"] = "within tolerance" if entry["ok"] else "pixel difference exceeds tolerance"
                            if not entry["ok"] and args.raw_dir is not None:
                                args.raw_dir.mkdir(parents=True, exist_ok=True)
                                (args.raw_dir / f"{source.stem}-{page}.oracle.ppm").write_bytes(oracle_ppm.read_bytes())
                                (args.raw_dir / f"{source.stem}-{page}.ours.png").write_bytes(ours_png.read_bytes())
                row["pages"].append(entry)
        row["ok"] = all(entry.get("ok", True) for entry in row["pages"])
        row["compared"] = sum(1 for entry in row["pages"] if not entry.get("skipped"))
        print(source.name, "OK" if row["ok"] else "MISMATCH", flush=True)
        results.append(row)

    compared = [entry for row in results for entry in row["pages"] if not entry.get("skipped")]
    report = {
        "criterion": "SC-D7",
        "what": "our page composite equals ddjvu; exact unless a measured tolerance applies",
        "producer": {"name": "strict-ooxml-djvu examples/codec_probe"},
        "oracle": common.fingerprint(args.oracle, "djvulibre-bin"),
        "foreground": args.foreground,
        "pages_per_file": pages,
        "files": len(results),
        "ok": sum(1 for row in results if row["ok"]),
        "pages_compared": len(compared),
        "pages_mismatched": sum(1 for entry in compared if not entry.get("ok")),
        "mismatched": [row["file"] for row in results if not row["ok"]],
        "results": results,
    }
    common.write_report(args.report, report)
    print({"files": report["files"], "ok": report["ok"], "pages_compared": report["pages_compared"]})
    return 0 if report["ok"] == report["files"] else 1


if __name__ == "__main__":
    common.main_guard(main)
