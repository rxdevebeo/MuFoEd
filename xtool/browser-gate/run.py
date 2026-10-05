#!/usr/bin/env python3
"""Orchestrate the R09 browser gate for F07/F20.

Starts ``strict-ooxml-view`` against a fixtures directory, drives a pinned
Chromium-family browser through ``run.mjs`` (CDP), and writes a JSON report.

Exit codes:
  0 — PASS
  1 — FAIL
  3 — BLOCKED (browser / node / viewer runtime missing)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
EXIT_OK = 0
EXIT_FAIL = 1
EXIT_BLOCKED = 3


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_port(port: int, proc: subprocess.Popen[str], timeout: float = 60.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            out, err = proc.communicate()
            raise RuntimeError(f"viewer exited early\nstdout={out}\nstderr={err}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError(f"viewer did not accept connections on {port}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", required=True, help="path to strict-ooxml-view binary")
    parser.add_argument("--report", required=True, help="JSON report output path")
    parser.add_argument("--fixtures-dir", required=True, help="directory with .docx fixtures")
    parser.add_argument(
        "--scenarios",
        default="f20-matrix,f07-face",
        help="comma-separated scenario ids for run.mjs",
    )
    parser.add_argument("--browser", default=os.environ.get("STRICTLIB_BROWSER"))
    args = parser.parse_args()

    viewer = Path(args.viewer)
    fixtures = Path(args.fixtures_dir)
    if not viewer.is_file():
        print(f"BLOCKED: viewer binary missing: {viewer}", file=sys.stderr)
        return EXIT_BLOCKED
    if not fixtures.is_dir():
        print(f"BLOCKED: fixtures dir missing: {fixtures}", file=sys.stderr)
        return EXIT_BLOCKED

    node = shutil.which("node")
    if not node:
        print("BLOCKED: node runtime missing", file=sys.stderr)
        return EXIT_BLOCKED

    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)

    hashes = {
        path.name: sha256_file(path)
        for path in sorted(fixtures.glob("*.docx"))
    }
    if not hashes:
        print("BLOCKED: fixtures dir has zero .docx files", file=sys.stderr)
        return EXIT_BLOCKED

    port = free_port()
    proc = subprocess.Popen(
        [str(viewer), str(fixtures), "--port", str(port), "--transitional"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        try:
            wait_port(port, proc)
        except RuntimeError as error:
            payload = {"status": "FAIL", "reason": str(error)}
            report_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
            print(error, file=sys.stderr)
            return EXIT_FAIL

        cmd = [
            node,
            str(HERE / "run.mjs"),
            "--url",
            f"http://127.0.0.1:{port}/",
            "--report",
            str(report_path),
            "--scenarios",
            args.scenarios,
        ]
        if args.browser:
            cmd.extend(["--browser", args.browser])
        completed = subprocess.run(cmd, cwd=str(REPO), check=False)
        if report_path.is_file():
            body = json.loads(report_path.read_text(encoding="utf-8"))
        else:
            body = {"status": "FAIL", "reason": "browser report missing"}
        body["fixture_sha256"] = hashes
        body["viewer"] = str(viewer)
        body["port"] = port
        body["node"] = node
        report_path.write_text(json.dumps(body, indent=2), encoding="utf-8")
        if completed.returncode == EXIT_BLOCKED:
            return EXIT_BLOCKED
        if completed.returncode != 0:
            return EXIT_FAIL
        return EXIT_OK
    finally:
        proc.kill()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


if __name__ == "__main__":
    raise SystemExit(main())
