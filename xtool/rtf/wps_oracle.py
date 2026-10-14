#!/usr/bin/env python3
"""Supervise isolated WPS COM workers with per-file timeout and complete accounting."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

from census import inventory, write_json


def stop_owned_worker(directory):
    marker = directory / "ownership.json"
    if not marker.is_file():
        return "no ownership marker; no WPS process terminated"
    ownership = json.loads(marker.read_text(encoding="utf-8-sig"))
    process_id = int(ownership["process_id"])
    start_ticks = int(ownership["start_ticks"])
    command = f"$rtfTimedProcess=Get-Process -Id {process_id} -ErrorAction SilentlyContinue; if ($rtfTimedProcess -and $rtfTimedProcess.StartTime.ToUniversalTime().Ticks -eq {start_ticks}) {{ Stop-Process -Id {process_id} -Force }}"
    stopped = subprocess.run(["powershell.exe", "-NoProfile", "-Command", command], capture_output=True, timeout=15, check=False)
    return f"owned-process cleanup exit {stopped.returncode}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=Path("testdata/CC0_RTF"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=90)
    parser.add_argument("--pdf", action="store_true", help="Capture PDF directly from original RTF before DOCX export")
    parser.add_argument("--only", nargs="*", help="explicit smoke inputs; all other inputs remain NotRun")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    corpus = inventory(args.corpus)
    records = [{"filename": row["filename"], "sha256": row["expected_sha256"], "status": "NotRun"} for row in corpus["files"]]
    report = {"version": 1, "engine": "WPS Writer COM", "manifest_sha256": corpus["manifest_sha256"],
              "claim": "independent_raw_references_not_importer_acceptance", "timeout_seconds": args.timeout,
              "worker_sha256": hashlib.sha256(Path(__file__).with_name("wps_export.ps1").read_bytes()).hexdigest(), "files": records}
    selected = set(args.only) if args.only is not None else {row["filename"] for row in records}
    if not selected <= {row["filename"] for row in records}:
        raise ValueError("--only contains unknown manifest input")
    if corpus["global_errors"] or corpus["summary"]["integrity_pass"] != 100:
        for record in records:
            record.update(status="Blocked", reason="corpus integrity failed")
        write_json(args.output / "oracle.json", report, exclusive=True)
        return 2
    for index, record in enumerate(records):
        if record["filename"] not in selected:
            continue
        directory = args.output / "files" / f"{index + 1:03}"
        directory.mkdir(parents=True)
        command = ["powershell.exe", "-NoProfile", "-STA", "-File", str(Path(__file__).with_name("wps_export.ps1")),
                   "-InputPath", str((args.corpus / record["filename"]).resolve()), "-OutputDirectory", str(directory)]
        if args.pdf:
            command.append("-ExportPdf")
        start = time.monotonic()
        try:
            completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=args.timeout,
                                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), check=False)
            (directory / "worker.log").write_bytes(completed.stdout)
            record["exit_code"] = completed.returncode
            result_path = directory / "result.json"
            if result_path.is_file():
                result = json.loads(result_path.read_text(encoding="utf-8-sig"))
                record.update(result)
                if record["status"] == "Captured":
                    for filename, hash_key in (("reference.docx", "docx_sha256"), ("content.txt", "content_sha256")):
                        if hashlib.sha256((directory / filename).read_bytes()).hexdigest() != record[hash_key]:
                            raise ValueError(f"worker output hash mismatch: {filename}")
                    if args.pdf and hashlib.sha256((directory / "reference.pdf").read_bytes()).hexdigest() != record["pdf_sha256"]:
                        raise ValueError("worker PDF hash mismatch")
                    if completed.returncode != 0:
                        record.update(status="Fail", reason="nonzero worker exit with captured result")
            else:
                record.update(status="Fail", reason="worker did not produce result.json")
        except subprocess.TimeoutExpired as exc:
            (directory / "worker.log").write_bytes(exc.stdout or b"")
            record.update(status="Fail", reason="worker timeout", cleanup=stop_owned_worker(directory))
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as exc:
            record.update(status="Fail", reason=str(exc))
        record["directory"] = str(directory.relative_to(args.output)).replace("\\", "/")
        record["elapsed_seconds"] = round(time.monotonic() - start, 3)
        # Atomic replacement makes progress snapshots readable during the run.
        progress = args.output / "oracle.tmp.json"
        write_json(progress, report)
        progress.replace(args.output / "oracle.json")
        print(f"{index + 1:03}/100 {record['status']} {record['filename']} ({record['elapsed_seconds']}s)", flush=True)
    write_json(args.output / "oracle.json", report)
    return 0 if all(row["status"] == "Captured" for row in records) else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"WPS oracle preparation failed: {exc}", file=sys.stderr)
        sys.exit(2)
