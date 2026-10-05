#!/usr/bin/env python3
"""Pin hashes and independent reference facts without assigning importer verdicts."""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import sys

from census import write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, default=Path("target/rtf-import/R01/wps-corpus"))
    parser.add_argument("--output", type=Path, default=Path("docs/rtf/wps.reference-index.json"))
    args = parser.parse_args()
    data = json.loads((args.oracle / "facts-index.json").read_text(encoding="utf-8"))
    rows = []
    for entry in data["files"]:
        row = {key: entry.get(key) for key in ("filename", "sha256", "status", "docx_sha256", "content_sha256", "counts", "facts")}
        row["artifacts"] = entry.get("directory")
        row["expected_import_outcome"] = None
        row["importer_checked"] = False
        rows.append(row)
    identities = {(entry["oracle"]["executable_sha256"], entry["oracle"]["file_version"], entry["oracle"]["application_version"])
                  for entry in data["files"] if entry.get("oracle")}
    counts = Counter()
    for entry in data["files"]:
        counts.update(entry.get("facts", {}).get("construct_counts", {}))
    font_path = args.oracle / "available-fonts.json"
    font_inventory = json.loads(font_path.read_text(encoding="utf-8"))
    elapsed = sorted(entry.get("elapsed_seconds", 0) for entry in data["files"])
    result = {"version": 1, "manifest_sha256": data["manifest_sha256"], "worker_sha256": data["worker_sha256"],
              "source_artifact_root": str(data.get("reference_root", args.oracle)).replace("\\", "/"),
              "facts_artifact_root": str(args.oracle).replace("\\", "/"),
              "claim": "independent_reference_index_not_importer_or_visual_acceptance",
              "oracle_identities": [{"executable_sha256": sha, "file_version": version, "application_version": app}
                                     for sha, version, app in sorted(identities)],
              "facts_extractor_sha256": hashlib.sha256(Path(__file__).with_name("docx_facts.py").read_bytes()).hexdigest(),
              "available_fonts_inventory_sha256": hashlib.sha256(font_path.read_bytes()).hexdigest(),
              "summary": {"files": len(rows), "captured": sum(row["status"] == "Captured" for row in rows),
                          "facts_extracted": sum((row.get("facts") or {}).get("status") == "Extracted" for row in rows),
                          "explicit_paragraphs": sum((row.get("facts") or {}).get("paragraphs", 0) for row in rows),
                          "media_parts": sum((row.get("facts") or {}).get("media_parts", 0) for row in rows),
                          "available_font_files": len(font_inventory["files"]),
                          "total_worker_seconds": round(sum(elapsed), 3),
                          "p50_worker_seconds": elapsed[(len(elapsed) - 1) // 2],
                          "p95_worker_seconds": elapsed[max(0, (95 * len(elapsed) + 99) // 100 - 1)],
                          "max_worker_seconds": elapsed[-1],
                          "word_construct_counts": dict(sorted(counts.items()))},
              "files": rows,
              "limitations": ["All importer outcomes remain pending; references do not implement RTF parsing",
                              "Detailed table grid/merge comparator and reference review pending",
                              "Available fonts do not establish actual resolved WPS faces",
                              "No visual, Strict schema, or importer performance acceptance"]}
    write_json(args.output, result, exclusive=True)
    print(json.dumps({key: value for key, value in result["summary"].items() if key != "word_construct_counts"}, sort_keys=True))
    return 0 if len(rows) == result["summary"]["captured"] == result["summary"]["facts_extracted"] == 100 and len(identities) == 1 else 1


if __name__ == "__main__":
    sys.exit(main())
