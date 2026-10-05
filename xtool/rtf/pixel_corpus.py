#!/usr/bin/env python3
"""Compare every matched page at 96 DPI, without resizing or threshold edits."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from census import write_json


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rendered", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--compare", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    receipt_path = args.rendered / "results.json"
    source = json.loads(receipt_path.read_text(encoding="utf-8"))
    rows = []
    result = {"claim": "development_pixel_measurement_not_reviewed_support_profile", "dpi": 96,
              "pagination_receipt_sha256": sha(receipt_path),
              "comparison_binary_sha256": sha(args.compare), "files": rows}
    for file in source["files"]:
        row = {"filename": file["filename"], "pages": [], "pixels": "Blocked"}
        rows.append(row)
        try:
            if file["pagination"] != "Pass":
                row["pixels"] = "Fail"
                row["reason"] = "page count differs; page correspondence not inferred"
            else:
                stem = file["filename"][:3]
                ref_pdf = args.oracle / "files" / stem / "reference.pdf"
                actual_pdf = args.rendered / (stem + ".pdf")
                if sha(ref_pdf) != file["reference_pdf_sha256"] or sha(actual_pdf) != file["pdf_sha256"]:
                    raise ValueError("PDF hashes differ from pagination receipt")
                directory = args.out / stem
                directory.mkdir()
                for page in range(1, file["actual_pages"] + 1):
                    rasters = []
                    for kind, pdf in (("reference", ref_pdf), ("actual", actual_pdf)):
                        prefix = directory / (f"{page:04d}-" + kind)
                        completed = subprocess.run(["pdftoppm", "-f", str(page), "-l", str(page),
                            "-r", "96", "-png", "-singlefile", str(pdf), str(prefix)],
                            capture_output=True, text=True, timeout=30, check=True)
                        raster = prefix.with_suffix(".png")
                        rasters.append(raster)
                        (directory / f"{page:04d}-{kind}.log").write_text(completed.stderr, encoding="utf-8")
                    comparison = subprocess.run([str(args.compare.resolve()), *map(str, rasters)],
                                                capture_output=True, text=True, timeout=30, check=True)
                    verdict = comparison.stdout.split("\t", 1)[0]
                    if verdict not in {"Pass", "Fail"}:
                        raise ValueError("unknown pixel comparison result")
                    row["pages"].append({"page": page, "verdict": verdict,
                        "reference_png_sha256": sha(rasters[0]), "actual_png_sha256": sha(rasters[1]),
                        "measurement": comparison.stdout.strip()})
                row["pixels"] = "Pass" if all(p["verdict"] == "Pass" for p in row["pages"]) else "Fail"
                row["render_status"] = file["render_status"]
        except Exception as error:
            row["error"] = str(error)
        write_json(args.out / "results.json", result)
        print(row["filename"], row["pixels"], len(row["pages"]), row.get("error", ""), flush=True)
    return 0 if all(r["pixels"] == "Pass" for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
