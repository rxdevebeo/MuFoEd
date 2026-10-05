#!/usr/bin/env python3
"""Capture independent DOCX references with an isolated LibreOffice profile.

No installed reader means Blocked for every input and exit 2, never success.
Raw DOCX references do not themselves freeze semantic expectations.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

from census import inventory, write_json


def executable_path(explicit):
    if explicit:
        path = Path(explicit).resolve()
        return path if path.is_file() else None
    for command in ("soffice", "libreoffice"):
        located = shutil.which(command)
        if located:
            return Path(located).resolve()
    for root in ("C:/Program Files", "C:/Program Files (x86)"):
        path = Path(root) / "LibreOffice/program/soffice.exe"
        if path.is_file():
            return path
    return None


def capture(corpus, output, executable, timeout):
    result = inventory(corpus)
    records = [{"filename": row["filename"], "sha256": row["expected_sha256"], "status": "Blocked"}
               for row in result["files"]]
    report = {"version": 1, "manifest_sha256": result["manifest_sha256"], "oracle": None,
              "claim": "independent_conversion_only_not_fidelity_acceptance", "files": records}
    if result["global_errors"] or result["summary"]["integrity_pass"] != 100:
        report["reason"] = "corpus integrity failed"
        return report, 2
    if not executable:
        report["reason"] = "LibreOffice executable not found; Word automation is not implemented"
        return report, 2
    try:
        # A unique profile avoids interaction with an existing desktop instance.
        profile = (output / "version-profile").resolve()
        version_command = [str(executable), f"-env:UserInstallation={profile.as_uri()}", "--headless", "--version"]
        version = subprocess.run(version_command, capture_output=True, timeout=timeout, check=True)
        report["oracle"] = {"application": "LibreOffice", "version": version.stdout.decode("utf-8", "replace").strip(),
                            "executable": str(executable), "executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                            "filter": "docx:Office Open XML Text", "timeout_seconds": timeout}
    except (OSError, subprocess.SubprocessError) as exc:
        report["reason"] = f"oracle version probe failed: {exc}"
        return report, 2
    for record in records:
        destination = (output / "files" / Path(record["filename"]).stem).resolve()
        destination.mkdir(parents=True, exist_ok=False)
        profile = (destination / "profile").resolve()
        command = [str(executable), f"-env:UserInstallation={profile.as_uri()}", "--headless",
                   "--convert-to", "docx:Office Open XML Text", "--outdir", str(destination),
                   str((corpus / record["filename"]).resolve())]
        start = time.monotonic()
        try:
            completed = subprocess.run(command, capture_output=True, timeout=timeout, check=False)
            record.update(exit_code=completed.returncode,
                          stdout=completed.stdout.decode("utf-8", "replace"),
                          stderr=completed.stderr.decode("utf-8", "replace"))
            expected = destination / (Path(record["filename"]).stem + ".docx")
            if completed.returncode == 0 and expected.is_file() and expected.stat().st_size:
                record.update(status="Captured", docx=str(expected.relative_to(output.resolve())),
                              docx_sha256=hashlib.sha256(expected.read_bytes()).hexdigest())
            else:
                record.update(status="Fail", reason="no successful nonempty DOCX result")
        except subprocess.TimeoutExpired:
            record.update(status="Fail", reason="oracle timeout")
        except OSError as exc:
            record.update(status="Fail", reason=str(exc))
        record["elapsed_seconds"] = round(time.monotonic() - start, 3)
    return report, 0 if all(record["status"] == "Captured" for record in records) else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=Path("testdata/CC0_RTF"))
    parser.add_argument("--output", type=Path, required=True, help="new run directory; never overwrite reference artifacts")
    parser.add_argument("--soffice", help="explicit executable path")
    parser.add_argument("--timeout", type=int, default=60)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    try:
        args.output.mkdir(parents=True, exist_ok=False)
        report, code = capture(args.corpus, args.output, executable_path(args.soffice), args.timeout)
        write_json(args.output / "oracle.json", report, exclusive=True)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"oracle preparation failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps({"oracle": report["oracle"], "reason": report.get("reason"),
                      "records": len(report["files"]), "captured": sum(row["status"] == "Captured" for row in report["files"])}, ensure_ascii=False))
    return code


if __name__ == "__main__":
    sys.exit(main())
