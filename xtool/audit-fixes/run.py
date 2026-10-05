#!/usr/bin/env python3
"""Runner for the audit-fix cards.

The interface is fixed:

    python xtool/audit-fixes/run.py --task F09 --phase red --receipt-dir target/audit-fixes/F09/red
    python xtool/audit-fixes/run.py --task F09 --phase green --receipt-dir target/audit-fixes/F09/green
    python xtool/audit-fixes/run.py --all --phase green --receipt-dir target/audit-fixes/final
    python xtool/audit-fixes/run.py --all --corpus DIR --require-corpus --receipt-dir target/audit-fixes/corpus

`red` keeps the test's own exit code and stores the assertion. It does not turn
a failure into success. `green` is nonzero when any required check failed or
could not be measured. A missing corpus with `--require-corpus` is BLOCKED.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    sys.stderr.write("error: audit-fixes needs Python 3.11 or newer (tomllib)\n")
    raise SystemExit(2)

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
MANIFEST = HERE / "manifest.toml"

EXIT_OK = 0
EXIT_FAIL = 1
EXIT_UNMEASURABLE = 2
EXIT_BLOCKED = 3


class RunnerError(Exception):
    """A required check that did not succeed."""

    def __init__(self, code: int, detail: str) -> None:
        super().__init__(detail)
        self.code = code
        self.detail = detail


def require_file(path: Path) -> None:
    """Fail when an input the runner was told to read is gone."""
    if not path.is_file():
        raise RunnerError(EXIT_UNMEASURABLE, f"deleted or missing file: {path}")


def require_tool(name: str) -> str:
    """Fail when a named executable is not on PATH. Returns its path."""
    found = shutil.which(name)
    if not found:
        raise RunnerError(EXIT_UNMEASURABLE, f"missing tool: {name}")
    return found


def require_tests(tests: list[str]) -> None:
    """An empty list is a zero measurement, not a pass."""
    if not tests:
        raise RunnerError(EXIT_UNMEASURABLE, "empty test list")


def quotient(numerator: int, denominator: int) -> float:
    """A ratio. Denominator 0 is unmeasurable, never 0 and never 1."""
    if denominator == 0:
        raise RunnerError(EXIT_UNMEASURABLE, "zero denominator")
    return numerator / denominator


def require_corpus(path: Path | None, required: bool) -> str:
    """Corpus acceptance. Absence is BLOCKED when the caller required it."""
    if not required:
        return "not_requested"
    if path is None or not path.is_dir():
        raise RunnerError(
            EXIT_BLOCKED,
            "BLOCKED: corpus directory is absent; this is not a successful skip",
        )
    documents = sorted(item.name for item in path.iterdir() if item.suffix.lower() == ".docx")
    if not documents:
        raise RunnerError(
            EXIT_BLOCKED,
            "BLOCKED: corpus has zero documents (zero denominator)",
        )
    quotient(len(documents), len(documents))
    return f"present:{len(documents)}"


def cargo_command(*arguments: str) -> list[str]:
    """`cargo` pinned to the workspace rust-version, the same floor CI uses."""
    version = ""
    manifest = REPO / "Cargo.toml"
    if manifest.is_file():
        for line in manifest.read_text(encoding="utf-8").splitlines():
            match = re.match(r'\s*rust-version\s*=\s*"([0-9][^"]*)"', line)
            if match:
                version = match.group(1)
                break
    command = ["cargo"]
    if version:
        command.append(f"+{version}")
    command.extend(arguments)
    return command


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def git_head() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=REPO,
            text=True,
            encoding="utf-8",
            errors="replace",
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def load_manifest() -> list[dict]:
    require_file(MANIFEST)
    with MANIFEST.open("rb") as handle:
        data = tomllib.load(handle)
    tasks = data.get("task", [])
    if not tasks:
        raise RunnerError(EXIT_UNMEASURABLE, "manifest has an empty task list")
    return tasks


def task_by_id(tasks: list[dict], task_id: str) -> dict:
    for task in tasks:
        if task.get("id") == task_id:
            return task
    raise RunnerError(EXIT_UNMEASURABLE, f"unknown task {task_id}")


def infrastructure_self_test() -> list[str]:
    """Prove the failure modes. The proof itself passes only when each one fires."""
    notes: list[str] = []

    def expect(code: int, label: str, action) -> None:
        try:
            action()
        except RunnerError as error:
            if error.code != code:
                raise RunnerError(
                    EXIT_FAIL,
                    f"{label}: exit {error.code}, expected {code} ({error.detail})",
                ) from error
            notes.append(f"{label}: exit {code}")
            return
        raise RunnerError(EXIT_FAIL, f"{label}: the check succeeded and should have failed")

    expect(EXIT_UNMEASURABLE, "deleted file", lambda: require_file(HERE / "no-such-input.docx"))
    expect(
        EXIT_UNMEASURABLE,
        "missing tool",
        lambda: require_tool("strict-ooxml-audit-missing-tool"),
    )
    expect(EXIT_UNMEASURABLE, "empty test list", lambda: require_tests([]))
    expect(EXIT_UNMEASURABLE, "zero denominator", lambda: quotient(0, 0))
    expect(EXIT_BLOCKED, "missing corpus", lambda: require_corpus(HERE / "no-such-corpus", True))
    if quotient(0, 3) != 0.0:
        raise RunnerError(EXIT_FAIL, "0/3 must be zero, not an error")
    notes.append("quotient 0/3 = 0")
    valid, damaged = _sample_packages()
    _accept_docx(valid)
    notes.append("valid zip/xml accepted")
    try:
        _accept_docx(damaged)
    except RunnerError as error:
        if error.code != EXIT_FAIL:
            raise
        notes.append("damaged xml rejected")
    else:
        raise RunnerError(EXIT_FAIL, "damaged xml was accepted")
    return notes


def _sample_packages() -> tuple[bytes, bytes]:
    """A tiny package and a broken one, built with the stdlib, not the production writer."""
    document = (
        "<?xml version='1.0' encoding='UTF-8'?>"
        "<w:document xmlns:w='http://example.invalid/w'>"
        "<w:body><w:p><w:t>ok</w:t></w:p></w:body></w:document>"
    )
    valid = _zip({"word/document.xml": document.encode("utf-8")})
    damaged = _zip({"word/document.xml": b"<broken"})
    return valid, damaged


def _zip(parts: dict[str, bytes]) -> bytes:
    import io

    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, payload in parts.items():
            archive.writestr(name, payload)
    return buffer.getvalue()


def _accept_docx(payload: bytes) -> None:
    try:
        with zipfile.ZipFile(io_bytes(payload)) as archive:
            names = archive.namelist()
            if "word/document.xml" not in names:
                raise RunnerError(EXIT_FAIL, "word/document.xml is absent")
            for name in names:
                if name.endswith(".xml") or name.endswith(".rels"):
                    ET.fromstring(archive.read(name))
    except RunnerError:
        raise
    except (zipfile.BadZipFile, ET.ParseError, KeyError) as error:
        raise RunnerError(EXIT_FAIL, f"package rejected: {error}") from error


def io_bytes(payload: bytes):
    import io

    return io.BytesIO(payload)


def _accept_pdf(payload: bytes) -> None:
    if not payload.startswith(b"%PDF-"):
        raise RunnerError(EXIT_FAIL, "missing %PDF- header")
    if b"xref\n" not in payload or b"startxref" not in payload or b"%%EOF" not in payload:
        raise RunnerError(EXIT_FAIL, "PDF trailer is incomplete")


def emit_and_check() -> list[dict]:
    """Materialize testkit scenarios and judge them with the stdlib."""
    require_tool("cargo")
    records: list[dict] = []
    with tempfile.TemporaryDirectory(prefix="audit-fixtures-") as tmp:
        directory = Path(tmp)
        command = cargo_command(
            "run",
            "-q",
            "--locked",
            "-p",
            "strict-ooxml-testkit",
            "--example",
            "emit_audit_fixtures",
            "--",
            "--out",
            str(directory),
        )
        completed = subprocess.run(
            command,
            cwd=REPO,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if completed.returncode != 0:
            raise RunnerError(
                EXIT_UNMEASURABLE,
                "emit_audit_fixtures failed: " + (completed.stderr or completed.stdout)[-500:],
            )
        emitted = sorted(directory.iterdir())
        if not emitted:
            raise RunnerError(EXIT_UNMEASURABLE, "emit produced zero files")
        for path in emitted:
            record = {"name": path.name, "sha256": sha256_file(path)}
            if path.name.startswith("damaged-"):
                try:
                    _accept_docx(path.read_bytes())
                except RunnerError:
                    record["result"] = "rejected"
                else:
                    raise RunnerError(EXIT_FAIL, f"{path.name} was accepted")
            elif path.suffix.lower() == ".pdf":
                _accept_pdf(path.read_bytes())
                record["result"] = "accepted"
            else:
                _accept_docx(path.read_bytes())
                record["result"] = "accepted"
            records.append(record)
    accepted = sum(1 for record in records if record["result"] == "accepted")
    quotient(accepted, len(records))
    return records


def measured_test_count(output: str) -> int:
    """How many tests the harness actually started, across every binary."""
    normalized = output.replace("\r\n", "\n").replace("\r", "\n")
    return sum(int(count) for count in re.findall(r"(?m)^running (\d+) tests?$", normalized))


def missing_test_names(output: str, tests: list[str]) -> list[str]:
    """Names that never appeared as a harness result line."""
    normalized = output.replace("\r\n", "\n").replace("\r", "\n")
    missing = []
    for name in tests:
        if not re.search(rf"(?m)^test {re.escape(name)}\b", normalized):
            missing.append(name)
    return missing


def cargo_tests(package: str, tests: list[str], phase: str, task: dict) -> dict:
    """Run named tests. Red does not invert a failing exit."""
    require_tests(tests)
    require_tool("cargo")
    arguments = cargo_command("test", "-p", package, "--locked")
    if task.get("no_default_features"):
        arguments.append("--no-default-features")
    if task.get("all_features"):
        arguments.append("--all-features")
    for feature in task.get("features") or []:
        arguments.extend(["--features", str(feature)])
    for argument in task.get("cargo_args") or []:
        arguments.append(str(argument))
    arguments.extend(["--", "--test-threads=8"])
    arguments.extend(tests)
    completed = subprocess.run(
        arguments,
        cwd=REPO,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    output = (completed.stdout or "") + (completed.stderr or "")
    if measured_test_count(output) == 0:
        raise RunnerError(
            EXIT_UNMEASURABLE,
            f"cargo test matched zero tests for {package}: {tests}",
        )
    missing = missing_test_names(output, tests)
    if missing:
        raise RunnerError(
            EXIT_UNMEASURABLE,
            f"named tests did not run for {package}: {missing}",
        )
    return {
        "command": arguments,
        "exit": completed.returncode,
        "phase": phase,
        "tail": output[-4000:],
    }


def run_scripts(scripts: list[str]) -> list[dict]:
    """Run card scripts. A nonzero exit is a failed check, not a skip."""
    outcomes = []
    for script in scripts:
        path = REPO / script
        require_file(path)
        completed = subprocess.run(
            [sys.executable, str(path)],
            cwd=REPO,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        output = (completed.stdout or "") + (completed.stderr or "")
        if completed.returncode != 0:
            raise RunnerError(
                EXIT_FAIL,
                f"script {script} exited {completed.returncode}: {output[-1500:]}",
            )
        outcomes.append({"script": script, "exit": completed.returncode})
    return outcomes


def run_task(task: dict, phase: str, corpus: Path | None, require: bool) -> dict:
    """Run one card. Returns a result dict; raises RunnerError on green failure."""
    kind = task.get("kind", "")
    task_id = task["id"]
    corpus_state = require_corpus(corpus, require)
    if kind == "infrastructure":
        if phase == "red":
            raise RunnerError(
                EXIT_UNMEASURABLE,
                "F00 has no application defect; red is not a witness",
            )
        notes = infrastructure_self_test()
        files = emit_and_check()
        completed = subprocess.run(
            cargo_command(
                "test",
                "-p",
                "strict-ooxml-testkit",
                "--lib",
                "--locked",
                "audit::tests",
                "--",
                "--test-threads=8",
            ),
            cwd=REPO,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if completed.returncode != 0:
            raise RunnerError(
                EXIT_FAIL,
                "testkit audit tests failed: " + (completed.stdout or "")[-1000:],
            )
        return {
            "status": "pass",
            "corpus": corpus_state,
            "notes": notes,
            "files": files,
            "commands": ["infrastructure_self_test", "emit_audit_fixtures", "cargo test audit::tests"],
        }
    tests = list(task.get("tests") or [])
    package = task.get("package") or ""
    if phase == "green" and not tests:
        raise RunnerError(EXIT_UNMEASURABLE, f"{task_id} has an empty test list")
    if not package:
        raise RunnerError(EXIT_UNMEASURABLE, f"{task_id} has no cargo package")
    outcome = cargo_tests(package, tests, phase, task)
    if phase == "green" and outcome["exit"] != 0:
        raise RunnerError(EXIT_FAIL, f"{task_id} green tests failed")
    scripts = [str(item) for item in (task.get("scripts") or [])]
    script_results = run_scripts(scripts) if phase == "green" and scripts else []
    if phase == "red" and outcome["exit"] == 0:
        raise RunnerError(
            EXIT_FAIL,
            f"{task_id} red phase passed; that is not a defect witness",
        )
    status = "witness" if phase == "red" else "pass"
    return {
        "status": status,
        "corpus": corpus_state,
        "outcome": outcome,
        "scripts": script_results,
        "exit": outcome["exit"],
    }


def write_receipt(directory: Path, payload: dict) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "receipt.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run one audit-fix card or all of them.")
    parser.add_argument("--task")
    parser.add_argument("--all", action="store_true")
    parser.add_argument("--phase", required=True, choices=("red", "green", "mutation"))
    parser.add_argument("--receipt-dir", required=True)
    parser.add_argument("--corpus")
    parser.add_argument("--require-corpus", action="store_true")
    args = parser.parse_args(argv)
    if args.all == bool(args.task):
        sys.stderr.write("error: pass exactly one of --task and --all\n")
        return EXIT_UNMEASURABLE
    receipt_dir = Path(args.receipt_dir)
    corpus = Path(args.corpus) if args.corpus else None
    try:
        tasks = load_manifest()
        selected = tasks if args.all else [task_by_id(tasks, args.task)]
        results = []
        code = EXIT_OK
        for task in selected:
            phase = args.phase
            if phase == "mutation" and task.get("kind") == "infrastructure":
                results.append(
                    {
                        "id": task["id"],
                        "status": "pass",
                        "note": "damaged fixtures stay rejected; F00 has no production logic to revert",
                        "notes": infrastructure_self_test(),
                    }
                )
                continue
            if phase == "mutation":
                raise RunnerError(
                    EXIT_UNMEASURABLE,
                    f"{task['id']} mutation is recorded by the card, not by a blanket invert",
                )
            result = {"id": task["id"], **run_task(task, phase, corpus, args.require_corpus)}
            results.append(result)
            if phase == "red":
                # Keep the test's exit. A witness is not a successful run.
                code = int(result.get("exit", EXIT_FAIL))
        status = "witness" if args.phase == "red" and code != EXIT_OK else "pass"
        detail = ""
    except RunnerError as error:
        status = {EXIT_BLOCKED: "blocked", EXIT_UNMEASURABLE: "unmeasurable"}.get(
            error.code, "fail"
        )
        code = error.code
        detail = error.detail
        results = []
    payload = {
        "code_sha": git_head(),
        "commands": [sys.argv],
        "detail": detail,
        "limitations": [
            "Private corpus files are not in this tree.",
            "Font face hashes are recorded by F07 and later, not by F00.",
        ],
        "oracles": {
            "docx": "Python zipfile + xml.etree.ElementTree, plus testkit inspect",
            "pdf": "header, xref, startxref, %%EOF",
            "python": sys.version.split()[0],
        },
        "phase": args.phase,
        "results": results,
        "status": status,
    }
    write_receipt(receipt_dir, payload)
    if detail:
        sys.stderr.write(detail + "\n")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
