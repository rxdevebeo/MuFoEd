#!/usr/bin/env python3
"""Run R01 fixtures, binary-skip mutation, and deterministic full-corpus census."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

from census import write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("target/rtf-import/R01/validation"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    records = []

    def run(name, command, cwd=root):
        start = time.monotonic()
        result = subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
        (output / f"{name}.log").write_bytes(result.stdout)
        records.append({"name": name, "command": command, "exit_code": result.returncode,
                        "elapsed_seconds": round(time.monotonic() - start, 3)})
        print(f"{name}: exit {result.returncode}", flush=True)
        return result

    unit = run("fixtures", [sys.executable, "-m", "unittest", "discover", "-s", "xtool/rtf", "-p", "test_*.py", "-v"])
    mutation = output / "mutation"
    mutation.mkdir()
    source = (root / "xtool/rtf/census.py").read_text(encoding="utf-8")
    if source.count("i += parameter") != 1:
        raise ValueError("mutation target must occur exactly once")
    (mutation / "census.py").write_text(source.replace("i += parameter", "i += 0  # mutation: parse binary as syntax"), encoding="utf-8")
    shutil.copyfile(root / "xtool/rtf/test_census.py", mutation / "test_census.py")
    negative = run("binary_mutation", [sys.executable, "-m", "unittest", "test_census.ScannerTests.test_binary_masks_syntax", "-v"], cwd=mutation)
    corpus_a = output / "census-a.json"
    corpus_b = output / "census-b.json"
    first = run("corpus_a", [sys.executable, "xtool/rtf/census.py", "--output", str(corpus_a)])
    second = run("corpus_b", [sys.executable, "xtool/rtf/census.py", "--output", str(corpus_b)])
    deterministic = corpus_a.is_file() and corpus_b.is_file() and corpus_a.read_bytes() == corpus_b.read_bytes()
    corpus = json.loads(corpus_a.read_text(encoding="utf-8")) if corpus_a.is_file() else None
    mutation_proved = negative.returncode != 0 and b"FAIL: test_binary_masks_syntax" in negative.stdout and b"AssertionError" in negative.stdout
    passed = unit.returncode == first.returncode == second.returncode == 0 and mutation_proved and deterministic
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=True).stdout.strip()
    tracked_diff = subprocess.run(["git", "diff", "--binary", "HEAD"], cwd=root, capture_output=True, check=True).stdout
    sources = {str(path.relative_to(root)).replace("\\", "/"): hashlib.sha256(path.read_bytes()).hexdigest()
               for path in sorted((root / "xtool/rtf").iterdir()) if path.suffix in (".py", ".ps1")}
    receipt = {"version": 1, "scope": "R01 census harness only; importer and semantic oracles not tested",
               "passed": passed, "base_commit": head, "tracked_dirty_diff_sha256": hashlib.sha256(tracked_diff).hexdigest(),
               "harness_source_sha256": sources, "python": sys.version, "python_executable": sys.executable,
               "commands": records, "mutation_proved": mutation_proved, "deterministic_census": deterministic,
               "manifest_sha256": corpus["manifest_sha256"] if corpus else None,
               "summary": corpus["summary"] if corpus else None,
               "limitations": ["Initial LibreOffice probe unavailable; WPS oracle is tracked in a separate run",
                                "No importer, Strict XSD, visual fidelity, or peak RSS validation in R01"]}
    write_json(output / "receipt.json", receipt, exclusive=True)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
