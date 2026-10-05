#!/usr/bin/env python3
"""Snapshot available system/user font files; never claim actual WPS face resolution."""

import argparse
import hashlib
import os
from pathlib import Path
import sys

from census import write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    roots = [Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts",
             Path(os.environ.get("LOCALAPPDATA", str(Path.home() / "AppData/Local"))) / "Microsoft/Windows/Fonts"]
    records = []
    for root in roots:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*")):
            if not path.is_file() or path.suffix.lower() not in (".ttf", ".ttc", ".otf", ".fon"):
                continue
            row = {"path": str(path), "status": "Captured"}
            try:
                row.update(bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
            except OSError as exc:
                row.update(status="Blocked", reason=str(exc))
            records.append(row)
    write_json(args.output, {"version": 1, "roots": [str(root) for root in roots], "files": records,
                            "claim": "available_font_files_only_not_actual_wps_faces",
                            "limitations": ["Application-private/cloud fonts and registry-only paths may be absent",
                                            "No glyph-to-face resolution traced; visual oracle remains pending"]}, exclusive=True)
    print(f"available font files: {len(records)}; captured: {sum(row['status'] == 'Captured' for row in records)}")
    return 0 if records and all(row["status"] == "Captured" for row in records) else 1


if __name__ == "__main__":
    sys.exit(main())
