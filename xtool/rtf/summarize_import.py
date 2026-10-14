#!/usr/bin/env python3
"""Pin development measurements; never freeze acceptance expectations."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess
from census import write_json


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    data = json.loads(args.results.read_text(encoding="utf-8"))
    manifest_path = Path("testdata/CC0_RTF/manifest.json")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    rows = data["files"]
    assert len(rows) == len(manifest) == 100
    assert [r["filename"] for r in rows] == [r["filename"] for r in manifest]
    sources = sorted(p for p in Path("strict-ooxml-rtf").rglob("*") if p.is_file())
    source_hashes = {str(p).replace("\\", "/"): sha(p) for p in sources}
    counts = Counter(r["status"] for r in rows)
    imported = [r for r in rows if r["status"] == "Imported"]
    result = {
        "invocation_options": data.get("options", {}),
        "version": 1, "claim": "development_measurement_not_release_acceptance",
        "git_base": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "manifest_sha256": sha(manifest_path), "results_sha256": sha(args.results),
        "results_path": str(args.results).replace("\\", "/"),
        "reference_index": "docs/rtf/wps.reference-index-v2.json",
        "reference_index_sha256": sha(Path("docs/rtf/wps.reference-index-v2.json")),
        "source_sha256": source_hashes,
        "summary": {"files": 100, "statuses": dict(counts),
                    "exact_body_text_match": sum(r["exact_body_match"] for r in imported),
                    "roundtrip_text_match": sum(r["roundtrip_text_match"] for r in imported),
                    "roundtrip_media_bytes_match": sum(r["roundtrip_media_bytes_match"] for r in imported),
                    "discarded_nuls": sum(r.get("discarded_nuls", 0) for r in imported),
                    "imported_media_parts": sum(r["media_parts"] for r in imported),
                    "writer_errors": sum(r["writer_error"] is not None for r in imported),
                    "writer_report_records": sum(r["writer_loss_count"] or 0 for r in imported),
                    "import_loss_events": sum(r["loss_count"] for r in imported),
                    "detail_caps_reached": sum(r["details_truncated"] for r in imported)},
        "import_errors": [r for r in rows if r["status"] == "ImportError"],
        "body_mismatches": [{key: r[key] for key in ("filename", "paragraphs", "expected_paragraphs", "first_mismatch")}
                            for r in imported if not r["exact_body_match"]],
        "limitations": ["Support-profile expectations remain unreviewed",
                        "Exact body text is not formatting, layout or other-region acceptance",
                        "PNG/JPEG payload checks validate signatures, not full raster decoding",
                        "Lists, styles, regions, revisions, fields and unsupported image formats have explicit losses",
                        "Nested tables are rejected rather than flattened",
                        "Performance, visual comparisons and full acceptance still pending"]}
    write_json(args.output, result, exclusive=True)
    print(json.dumps(result["summary"], ensure_ascii=True))


if __name__ == "__main__":
    main()
