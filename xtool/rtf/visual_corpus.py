#!/usr/bin/env python3
"""Measure PDF pagination against hash-verified WPS exports; no expectations edits.

Pixel fidelity is a separate check. Equal page counts never mean visual PASS.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

from census import write_json
from public_corpus import measured


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def page_count(path):
    result = subprocess.run(["pdfinfo", str(path)], capture_output=True, text=True,
                            timeout=15, check=True)
    match = re.search(r"^Pages:\s+(\d+)\s*$", result.stdout, re.MULTILINE)
    if not match:
        raise ValueError("pdfinfo did not report page count")
    return int(match[1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--imported", required=True, type=Path)
    parser.add_argument("--oracle", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--timeout", default=90, type=int)
    parser.add_argument("--rss-mib", default=1024, type=int)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    index = args.oracle / "oracle.json"
    source = json.loads(index.read_text(encoding="utf-8"))
    imports = json.loads((args.imported / "results.json").read_text(encoding="utf-8"))
    imported = {row["filename"]: row for row in imports["files"]}
    rows = []
    receipt = {"claim": "pagination_measurement_not_pixel_fidelity_acceptance",
               "cli_sha256": sha(args.cli), "oracle_sha256": sha(index),
               "import_receipt_sha256": sha(args.imported / "results.json"),
               "limits": {"timeout_seconds": args.timeout, "rss_mib": args.rss_mib},
               "files": rows}
    for reference in source["files"]:
        row = {"filename": reference["filename"], "pagination": "Blocked",
               "pixels": "NotMeasured"}
        rows.append(row)
        try:
            stem = Path(reference["directory"]).name
            docx = args.imported / (stem + ".docx")
            pdf = args.output / (stem + ".pdf")
            wps_pdf = args.oracle / reference["directory"] / "reference.pdf"
            if sha(docx) != imported[reference["filename"]]["output_sha256"]:
                raise ValueError("imported DOCX hash differs from receipt")
            if sha(wps_pdf) != reference["pdf_sha256"]:
                raise ValueError("reference PDF hash differs from WPS receipt")
            row.update({"docx_sha256": sha(docx), "reference_pdf_sha256": sha(wps_pdf),
                        "reference_pages": page_count(wps_pdf)})
            row.update(measured([str(args.cli.resolve()), "to-pdf", str(docx),
                                 "--out", str(pdf)], args.output / (stem + ".log"),
                                args.timeout, args.rss_mib * 1024 * 1024))
            row["render_status"] = {0: "Clean", 1: "Degraded"}.get(row["exit_code"], "Failed")
            if row["exit_code"] not in {0, 1} or row["resource_verdict"] != "Pass":
                raise ValueError("render failed or resource measurement did not pass")
            row.update({"actual_pages": page_count(pdf), "pdf_sha256": sha(pdf)})
            row["pagination"] = "Pass" if row["actual_pages"] == row["reference_pages"] else "Fail"
        except Exception as error:
            row["error"] = str(error)
        write_json(args.output / "results.json", receipt)
        print(row["filename"], row["pagination"], row.get("actual_pages"),
              row.get("reference_pages"), row.get("error", ""), flush=True)
    return 0 if all(row["pagination"] == "Pass" for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
