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

Receipts always record code SHA and dirty-tree fingerprint. F21 green executes
the behavioral public suite and never claims a full audit when matrix rows are
blocked or the corpus was not required.
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
MATRIX = HERE / "matrix.toml"

EXIT_OK = 0
EXIT_FAIL = 1
EXIT_UNMEASURABLE = 2
EXIT_BLOCKED = 3

CLAIM_SYNTHETIC = "synthetic_public_suite"
CLAIM_CORPUS = "corpus_acceptance"
CLAIM_FULL = "full_audit"
CLAIM_CARD = "card"


class RunnerError(Exception):
    """A required check that did not succeed."""

    def __init__(self, code: int, detail: str, payload: dict | None = None) -> None:
        super().__init__(detail)
        self.code = code
        self.detail = detail
        self.payload = payload or {}


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


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


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


def dirty_tree_hash(repo: Path = REPO) -> str:
    """Hash status and working bytes of tracked/non-ignored untracked files."""
    try:
        porcelain = subprocess.check_output(
            ["git", "status", "--porcelain", "-z"],
            cwd=repo,
            text=False,
        )
        paths = subprocess.check_output(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=repo,
        )
        digest = hashlib.sha256(b"working-tree-content-v2\0" + porcelain)
        for name in sorted(set(paths.split(b"\0")) - {b""}):
            path = repo / os.fsdecode(name)
            digest.update(name + b"\0")
            if path.is_symlink():
                digest.update(b"symlink\0" + os.fsencode(os.readlink(path)))
            elif path.is_file():
                content = hashlib.sha256()
                with path.open("rb") as source:
                    for chunk in iter(lambda: source.read(1024 * 1024), b""):
                        content.update(chunk)
                digest.update(b"file\0" + content.digest())
            else:
                digest.update(b"absent-or-gitlink\0")
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    return digest.hexdigest()


def load_manifest() -> list[dict]:
    require_file(MANIFEST)
    with MANIFEST.open("rb") as handle:
        data = tomllib.load(handle)
    tasks = data.get("task", [])
    if not tasks:
        raise RunnerError(EXIT_UNMEASURABLE, "manifest has an empty task list")
    return tasks


def load_matrix() -> list[dict]:
    require_file(MATRIX)
    with MATRIX.open("rb") as handle:
        data = tomllib.load(handle)
    rows = data.get("scenario", [])
    if not rows:
        raise RunnerError(EXIT_UNMEASURABLE, "matrix has zero scenarios")
    return rows


def task_by_id(tasks: list[dict], task_id: str) -> dict:
    for task in tasks:
        if task.get("id") == task_id:
            return task
    raise RunnerError(EXIT_UNMEASURABLE, f"unknown task {task_id}")


def validate_receipt(payload: dict) -> None:
    """Fail-closed schema for durable receipts."""
    required = (
        "code_sha",
        "dirty_tree_hash",
        "commands",
        "phase",
        "status",
        "results",
        "claim",
        "full_audit",
        "scenario_counts",
    )
    missing = [key for key in required if key not in payload]
    if missing:
        raise RunnerError(EXIT_FAIL, f"receipt missing fields: {missing}")
    if not payload.get("code_sha") or payload["code_sha"] == "unknown":
        raise RunnerError(EXIT_UNMEASURABLE, "receipt code_sha is unknown")
    if not payload.get("dirty_tree_hash") or payload["dirty_tree_hash"] == "unknown":
        raise RunnerError(EXIT_UNMEASURABLE, "receipt dirty_tree_hash is unknown")
    counts = payload["scenario_counts"]
    if not isinstance(counts, dict):
        raise RunnerError(EXIT_FAIL, "scenario_counts must be an object")
    for key in ("executed", "passed", "failed", "blocked", "unmeasurable"):
        if key not in counts:
            raise RunnerError(EXIT_FAIL, f"scenario_counts missing {key}")
    if payload.get("full_audit") is True and payload.get("claim") != CLAIM_FULL:
        raise RunnerError(EXIT_FAIL, "full_audit requires claim=full_audit")
    if payload.get("claim") == CLAIM_FULL and payload.get("full_audit") is not True:
        raise RunnerError(EXIT_FAIL, "claim=full_audit requires full_audit=true")


def infrastructure_self_test() -> list[str]:
    """Prove the failure modes. The proof itself passes only when each one fires."""
    notes: list[str] = []
    with tempfile.TemporaryDirectory() as folder:
        repo = Path(folder)
        subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
        fixture = repo / "fixture.bin"
        fixture.write_bytes(b"initial")
        subprocess.run(["git", "add", "fixture.bin"], cwd=repo, check=True)
        fixture.write_bytes(b"changed-A")
        status_a = subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)
        hash_a = dirty_tree_hash(repo)
        fixture.write_bytes(b"changed-B")
        status_b = subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)
        hash_b = dirty_tree_hash(repo)
        if status_a != status_b or hash_a == hash_b or "unknown" in (hash_a, hash_b):
            raise RunnerError(EXIT_FAIL, "same git status hid changed working bytes")
        if dirty_tree_hash(repo) != hash_b:
            raise RunnerError(EXIT_FAIL, "working content fingerprint is unstable")
        untracked = repo / "untracked.bin"
        untracked.write_bytes(b"first")
        untracked_a = dirty_tree_hash(repo)
        untracked.write_bytes(b"other")
        if dirty_tree_hash(repo) == untracked_a:
            raise RunnerError(EXIT_FAIL, "untracked byte changes were hidden")
        notes.append("same status and untracked byte changes alter content fingerprint")

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
    expect(
        EXIT_FAIL,
        "receipt without dirty_tree_hash",
        lambda: validate_receipt(
            {
                "code_sha": "abc",
                "commands": [],
                "phase": "green",
                "status": "pass",
                "results": [],
                "claim": CLAIM_CARD,
                "full_audit": False,
                "scenario_counts": {
                    "executed": 0,
                    "passed": 0,
                    "failed": 0,
                    "blocked": 0,
                    "unmeasurable": 0,
                },
            }
        ),
    )
    expect(
        EXIT_FAIL,
        "full_audit claim without flag",
        lambda: validate_receipt(
            {
                "code_sha": "abc",
                "dirty_tree_hash": "def",
                "commands": [],
                "phase": "green",
                "status": "pass",
                "results": [],
                "claim": CLAIM_FULL,
                "full_audit": False,
                "scenario_counts": {
                    "executed": 1,
                    "passed": 1,
                    "failed": 0,
                    "blocked": 0,
                    "unmeasurable": 0,
                },
            }
        ),
    )
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
    """Names that never appeared as a harness result line (suffix match allowed)."""
    normalized = output.replace("\r\n", "\n").replace("\r", "\n")
    missing = []
    for name in tests:
        pattern = rf"(?m)^test (?:.*::)?{re.escape(name)}\b"
        if not re.search(pattern, normalized):
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
    started = measured_test_count(output)
    if started == 0:
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
        "tests_started": started,
        "tests_requested": len(tests),
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


def font_face_hashes() -> dict[str, str]:
    """SHA-256 of bundled face programs. Recorded only when files exist."""
    fonts_root = REPO / "strict-ooxml-render-svg" / "assets" / "fonts"
    if not fonts_root.is_dir():
        raise RunnerError(EXIT_UNMEASURABLE, "bundled font directory is missing")
    faces = sorted(fonts_root.rglob("*.ttf"))
    if not faces:
        raise RunnerError(EXIT_UNMEASURABLE, "zero bundled font faces")
    return {
        str(path.relative_to(REPO)).replace("\\", "/"): sha256_file(path) for path in faces
    }


def matrix_summary(rows: list[dict], card: str | None = None) -> dict:
    selected = [row for row in rows if card is None or row.get("card") == card]
    measured = [row for row in selected if row.get("status") == "measured"]
    blocked = [row for row in selected if row.get("status") == "blocked"]
    unknown = [
        row for row in selected if row.get("status") not in ("measured", "blocked")
    ]
    if unknown:
        raise RunnerError(
            EXIT_UNMEASURABLE,
            "matrix rows with unknown status: "
            + ", ".join(str(row.get("id")) for row in unknown),
        )
    return {
        "total": len(selected),
        "measured": len(measured),
        "blocked": len(blocked),
        "blocked_ids": [str(row.get("id")) for row in blocked],
        "measured_ids": [str(row.get("id")) for row in measured],
    }


def run_task(
    task: dict,
    phase: str,
    corpus: Path | None,
    require: bool,
    prior_results: list[dict] | None = None,
) -> dict:
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
        if phase in ("mutation", "inverse"):
            notes = infrastructure_self_test()
            return {
                "status": "pass",
                "corpus": corpus_state,
                "notes": notes,
                "note": "damaged fixtures stay rejected; receipt schema rejects missing dirty_tree_hash",
            }
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
            "tests_started": measured_test_count(
                (completed.stdout or "") + (completed.stderr or "")
            ),
        }

    if kind == "acceptance":
        return run_acceptance(task, phase, corpus, require, prior_results)

    tests = list(task.get("tests") or [])
    inverse_tests = list(task.get("inverse_tests") or [])
    package = task.get("package") or ""
    if phase in ("mutation", "inverse"):
        if not inverse_tests:
            raise RunnerError(
                EXIT_UNMEASURABLE,
                f"{task_id} has no inverse_tests; inverse is not a blanket invert",
            )
        if not package:
            raise RunnerError(EXIT_UNMEASURABLE, f"{task_id} has no cargo package")
        outcome = cargo_tests(package, inverse_tests, phase, task)
        if outcome["exit"] != 0:
            raise RunnerError(EXIT_FAIL, f"{task_id} inverse tests failed")
        return {
            "status": "pass",
            "corpus": corpus_state,
            "outcome": outcome,
            "scripts": [],
            "exit": outcome["exit"],
            "tests_started": outcome["tests_started"],
        }
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
    measurements: dict = {}
    if task_id == "F07" and phase == "green":
        measurements["font_face_hashes"] = font_face_hashes()
    status = "witness" if phase == "red" else "pass"
    return {
        "status": status,
        "corpus": corpus_state,
        "outcome": outcome,
        "scripts": script_results,
        "exit": outcome["exit"],
        "tests_started": outcome["tests_started"],
        "measurements": measurements,
    }


def run_acceptance(
    task: dict,
    phase: str,
    corpus: Path | None,
    require: bool,
    prior_results: list[dict] | None = None,
) -> dict:
    """Public end-to-end suite: metadata test + every non-acceptance card."""
    if phase == "red":
        raise RunnerError(
            EXIT_UNMEASURABLE,
            "F21 red is the pre-fix metadata-only suite under R08/red, not a live invert",
        )
    if phase in ("mutation", "inverse"):
        # Inverse: synthetic green must not become full_audit when matrix is incomplete.
        rows = load_matrix()
        summary = matrix_summary(rows)
        if summary["blocked"] == 0:
            raise RunnerError(
                EXIT_UNMEASURABLE,
                "inverse expected blocked matrix rows; matrix has none",
            )
        # A receipt that claims full audit while blocked rows remain is invalid
        # for acceptance reporting; the inverse proves we refuse that claim.
        return {
            "status": "pass",
            "corpus": require_corpus(corpus, require),
            "note": "inverse: full_audit refused while matrix has blocked rows",
            "blocked_ids": summary["blocked_ids"],
            "claim": CLAIM_SYNTHETIC,
            "full_audit": False,
            "scenario_counts": {
                "executed": 1,
                "passed": 1,
                "failed": 0,
                "blocked": summary["blocked"],
                "unmeasurable": 0,
            },
        }

    tasks = load_manifest()
    matrix_rows = load_matrix()
    summary = matrix_summary(matrix_rows)
    metadata_tests = list(task.get("metadata_tests") or task.get("tests") or [])
    require_tests(metadata_tests)
    package = task.get("package") or "strict-ooxml-testkit"
    metadata_outcome = cargo_tests(package, metadata_tests, phase, task)
    if metadata_outcome["exit"] != 0:
        raise RunnerError(EXIT_FAIL, "F21 metadata tests failed")

    card_results = []
    executed = 0
    passed = 0
    failed = 0
    unmeasurable = 0
    font_hashes = None
    prior_by_id = {
        item.get("id"): item for item in (prior_results or []) if item.get("id")
    }

    for card in tasks:
        if card.get("kind") == "acceptance":
            continue
        card_id = card["id"]
        if card_id in prior_by_id:
            result = prior_by_id[card_id]
            if result.get("status") not in ("pass", "witness"):
                failed += 1
                card_results.append(
                    {
                        "id": card_id,
                        "status": "fail",
                        "detail": result.get("detail") or result.get("status"),
                        "reused_from_all": True,
                    }
                )
                continue
            started = int(result.get("tests_started") or 0)
            executed += max(started, 1)
            passed += 1
            entry = {
                "id": card_id,
                "status": "pass",
                "tests_started": started,
                "reused_from_all": True,
            }
            measurements = result.get("measurements") or {}
            if "font_face_hashes" in measurements:
                font_hashes = measurements["font_face_hashes"]
                entry["font_face_hashes"] = font_hashes
            card_results.append(entry)
            continue
        try:
            result = run_task(card, "green", corpus, require)
        except RunnerError as error:
            if error.code == EXIT_UNMEASURABLE:
                unmeasurable += 1
            elif error.code == EXIT_BLOCKED:
                raise
            else:
                failed += 1
            card_results.append(
                {
                    "id": card_id,
                    "status": {
                        EXIT_BLOCKED: "blocked",
                        EXIT_UNMEASURABLE: "unmeasurable",
                    }.get(error.code, "fail"),
                    "detail": error.detail,
                }
            )
            continue
        started = int(result.get("tests_started") or 0)
        if card.get("kind") != "infrastructure" and started == 0:
            unmeasurable += 1
            card_results.append(
                {
                    "id": card_id,
                    "status": "unmeasurable",
                    "detail": "zero tests started",
                }
            )
            continue
        executed += max(started, 1)
        passed += 1
        entry = {"id": card_id, "status": "pass", "tests_started": started}
        measurements = result.get("measurements") or {}
        if "font_face_hashes" in measurements:
            font_hashes = measurements["font_face_hashes"]
            entry["font_face_hashes"] = font_hashes
        card_results.append(entry)

    corpus_state = require_corpus(corpus, require)
    blocked = summary["blocked"]
    counts = {
        "executed": executed,
        "passed": passed,
        "failed": failed,
        "blocked": blocked,
        "unmeasurable": unmeasurable,
        "matrix_measured": summary["measured"],
        "matrix_total": summary["total"],
    }
    base = {
        "corpus": corpus_state,
        "claim": CLAIM_SYNTHETIC if not require else CLAIM_CORPUS,
        "full_audit": False,
        "metadata": metadata_outcome,
        "cards": card_results,
        "matrix": summary,
        "font_face_hashes": font_hashes,
        "tests_started": executed + int(metadata_outcome.get("tests_started") or 0),
        "scenario_counts": counts,
    }
    if failed:
        failed_ids = [
            f"{card['id']}: {card.get('detail', card.get('status'))}"
            for card in card_results
            if card.get("status") == "fail"
        ]
        raise RunnerError(
            EXIT_FAIL,
            f"F21 behavioral suite failed cards: {failed} ({'; '.join(failed_ids)})",
            payload=base,
        )
    if unmeasurable:
        bad = [
            f"{card['id']}: {card.get('detail', card.get('status'))}"
            for card in card_results
            if card.get("status") == "unmeasurable"
        ]
        raise RunnerError(
            EXIT_UNMEASURABLE,
            f"F21 behavioral suite unmeasurable cards: {unmeasurable} ({'; '.join(bad)})",
            payload=base,
        )
    if executed == 0:
        raise RunnerError(
            EXIT_UNMEASURABLE,
            "F21 executed zero behavioral scenarios",
            payload=base,
        )

    full_audit = (
        require
        and corpus_state.startswith("present:")
        and blocked == 0
        and failed == 0
        and unmeasurable == 0
    )
    if full_audit:
        claim = CLAIM_FULL
    elif require:
        claim = CLAIM_CORPUS
    else:
        claim = CLAIM_SYNTHETIC
    base["claim"] = claim
    base["full_audit"] = full_audit
    base["status"] = "pass"
    return base


def empty_counts() -> dict:
    return {
        "executed": 0,
        "passed": 0,
        "failed": 0,
        "blocked": 0,
        "unmeasurable": 0,
    }


def write_receipt(directory: Path, payload: dict) -> None:
    validate_receipt(payload)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "receipt.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run one audit-fix card or all of them.")
    parser.add_argument("--task")
    parser.add_argument("--all", action="store_true")
    parser.add_argument(
        "--phase",
        required=True,
        choices=("red", "green", "mutation", "inverse"),
    )
    parser.add_argument("--receipt-dir", required=True)
    parser.add_argument("--corpus")
    parser.add_argument("--require-corpus", action="store_true")
    args = parser.parse_args(argv)
    if args.all == bool(args.task):
        sys.stderr.write("error: pass exactly one of --task and --all\n")
        return EXIT_UNMEASURABLE
    receipt_dir = Path(args.receipt_dir)
    corpus = Path(args.corpus) if args.corpus else None
    results: list[dict] = []
    claim = CLAIM_CARD
    full_audit = False
    counts = empty_counts()
    font_hashes = None
    matrix_info = None
    try:
        tasks = load_manifest()
        selected = tasks if args.all else [task_by_id(tasks, args.task)]
        code = EXIT_OK
        for task in selected:
            phase = args.phase
            prior = results if task.get("kind") == "acceptance" and args.all else None
            result = {
                "id": task["id"],
                **run_task(task, phase, corpus, args.require_corpus, prior),
            }
            results.append(result)
            if "font_face_hashes" in (result.get("measurements") or {}):
                font_hashes = result["measurements"]["font_face_hashes"]
            if result.get("font_face_hashes"):
                font_hashes = result["font_face_hashes"]
            if task.get("kind") == "acceptance":
                claim = result.get("claim", CLAIM_SYNTHETIC)
                full_audit = bool(result.get("full_audit"))
                counts = dict(result.get("scenario_counts") or counts)
                matrix_info = result.get("matrix")
            else:
                started = int(result.get("tests_started") or 0)
                counts["executed"] += max(started, 1 if result.get("status") == "pass" else 0)
                if result.get("status") in ("pass", "witness"):
                    counts["passed"] += 1
                if phase == "red":
                    # Keep the test's exit. A witness is not a successful run.
                    code = int(result.get("exit", EXIT_FAIL))
        status = "witness" if args.phase == "red" and code != EXIT_OK else "pass"
        detail = ""
        if args.phase == "red" and code == EXIT_OK and not results:
            status = "unmeasurable"
            code = EXIT_UNMEASURABLE
    except RunnerError as error:
        status = {EXIT_BLOCKED: "blocked", EXIT_UNMEASURABLE: "unmeasurable"}.get(
            error.code, "fail"
        )
        code = error.code
        detail = error.detail
        if error.payload:
            results = [{"id": args.task or "ALL", "status": status, **error.payload}]
            if error.payload.get("scenario_counts"):
                counts = dict(error.payload["scenario_counts"])
            if error.payload.get("claim"):
                claim = error.payload["claim"]
            if "full_audit" in error.payload:
                full_audit = bool(error.payload["full_audit"])
            if error.payload.get("matrix"):
                matrix_info = error.payload["matrix"]
            if error.payload.get("font_face_hashes"):
                font_hashes = error.payload["font_face_hashes"]
        else:
            results = results or []
        if status == "unmeasurable":
            counts["unmeasurable"] = max(counts.get("unmeasurable", 0), 1)
        elif status == "blocked":
            counts["blocked"] = max(counts.get("blocked", 0), 1)
        else:
            counts["failed"] = max(counts.get("failed", 0), 1)

    limitations = [
        "Private corpus files are not in this tree unless --require-corpus is set.",
        "Corpus pass and synthetic public-suite pass are different claims.",
        "A green F21 receipt with blocked matrix rows is not a full audit.",
    ]
    if matrix_info and matrix_info.get("blocked_ids"):
        limitations.append(
            "Blocked matrix rows: " + ", ".join(matrix_info["blocked_ids"])
        )

    payload = {
        "claim": claim,
        "code_sha": git_head(),
        "commands": [sys.argv],
        "detail": detail,
        "dirty_tree_hash": dirty_tree_hash(),
        "full_audit": full_audit,
        "limitations": limitations,
        "matrix": matrix_info,
        "oracles": {
            "docx": "Python zipfile + xml.etree.ElementTree, plus testkit inspect",
            "matrix": str(MATRIX.relative_to(REPO)).replace("\\", "/"),
            "pdf": "header, xref, startxref, %%EOF",
            "python": sys.version.split()[0],
        },
        "phase": args.phase,
        "results": results,
        "scenario_counts": counts,
        "status": status,
    }
    if font_hashes:
        payload["font_face_hashes"] = font_hashes
    try:
        write_receipt(receipt_dir, payload)
    except RunnerError as error:
        # Last resort: still leave a receipt for diagnosis, but fail closed.
        receipt_dir.mkdir(parents=True, exist_ok=True)
        broken = dict(payload)
        broken["status"] = "fail"
        broken["detail"] = (detail + " | " if detail else "") + error.detail
        (receipt_dir / "receipt.json").write_text(
            json.dumps(broken, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        sys.stderr.write(error.detail + "\n")
        return EXIT_FAIL
    if detail:
        sys.stderr.write(detail + "\n")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
