#!/usr/bin/env python3
"""Publish a census summary and an explicitly unreviewed expectations template."""

import argparse
import json
from pathlib import Path
import sys

from census import expectation_template, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--census", type=Path, default=Path("target/rtf-import/R01/corpus/census.json"))
    parser.add_argument("--docs", type=Path, default=Path("docs/rtf"))
    parser.add_argument("--refresh-pending", action="store_true", help="replace only a wholly unreviewed pending template")
    args = parser.parse_args()
    data = json.loads(args.census.read_text(encoding="utf-8"))
    summary = {"version": data["version"], "manifest_sha256": data["manifest_sha256"],
               "limits": data["limits"], "global_errors": data["global_errors"], "summary": data["summary"],
               "claim": "lexical_presence_only_not_import_acceptance",
               "files": [{"filename": row["filename"], "sha256": row.get("sha256"), "integrity": row["integrity"],
                          "lexical_status": row.get("lexical", {}).get("status"),
                          "warnings": row.get("lexical", {}).get("warnings", []),
                          "feature_candidates": sorted(row.get("lexical", {}).get("feature_hits", {}))}
                         for row in data["files"]]}
    pending = args.docs / "expectations.pending.json"
    if pending.exists():
        existing = json.loads(pending.read_text(encoding="utf-8"))
        if not args.refresh_pending or existing.get("status") != "PendingIndependentOracle" or any(
                row.get("reviewed") is not False or row.get("expected_import_outcome") is not None
                or row.get("reviewed_unsupported")
                or any(row.get(key) != "Blocked" for key in ("text_oracle", "structure_oracle", "visual_oracle"))
                for row in existing["files"]):
            raise ValueError("will not replace reviewed/non-pending expectations")
    write_json(pending, expectation_template(data), exclusive=not pending.exists())
    write_json(args.docs / "census.summary.json", summary)
    print(f"published {len(data['files'])} rows; expectations remain PendingIndependentOracle")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"snapshot failed: {exc}", file=sys.stderr)
        sys.exit(2)
