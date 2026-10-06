#!/usr/bin/env python3
"""F01: a missing output and an unmatched schema error must not PASS.

Default (what F21 runs) is the decide_gate matrix: no lxml, no schema compile.
`--full` adds the oracle cases and is what the xsd-gate CI job runs.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import xsd_gate  # noqa: E402

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
WML = "http://purl.oclc.org/ooxml/wordprocessingml/main"


def test_decide_matrix() -> None:
    code, _ = xsd_gate.decide_gate(
        documents=1, validated=0, missing=1, unmatched=0, our_total=0, source_total=0
    )
    if code == 0:
        raise SystemExit("missing output was a pass")
    code, _ = xsd_gate.decide_gate(
        documents=0, validated=0, missing=0, unmatched=0, our_total=0, source_total=0
    )
    if code == 0:
        raise SystemExit("zero documents was a pass")
    code, _ = xsd_gate.decide_gate(
        documents=1, validated=1, missing=0, unmatched=1, our_total=0, source_total=0
    )
    if code == 0:
        raise SystemExit("unmatched violation was a pass")
    code, summary = xsd_gate.decide_gate(
        documents=1, validated=1, missing=0, unmatched=0, our_total=0, source_total=2
    )
    if code != 0 or "schema-clean" not in summary or "not schema-clean" not in summary:
        raise SystemExit(f"source violations were called clean: {code} {summary}")
    code, summary = xsd_gate.decide_gate(
        documents=1, validated=1, missing=0, unmatched=0, our_total=0, source_total=0
    )
    if code != 0 or "schema-clean" not in summary:
        raise SystemExit(f"clean corpus did not pass: {code} {summary}")
    if xsd_gate.item_state({"origin": "ours"}, 0) != "NOT_EXERCISED":
        raise SystemExit("a zero without a fixture was called closed")
    if xsd_gate.item_state({"origin": "ours", "fixture": "x"}, 0) != "closed":
        raise SystemExit("an exercised zero was not closed")


def _docx(path: Path, parts: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as archive:
        for name, text in parts.items():
            archive.writestr(name, text.encode("utf-8"))


def test_missing_output_command() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _docx(
            root / "corpus" / "one.docx",
            {
                "word/document.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:document xmlns:w='{WML}'><w:body><w:p/></w:body></w:document>"
                )
            },
        )
        (root / "written").mkdir()
        completed = subprocess.run(
            [
                sys.executable,
                str(HERE / "xsd_gate.py"),
                "--corpus",
                str(root / "corpus"),
                "--written",
                str(root / "written"),
                "--quiet-messages",
            ],
            cwd=REPO,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        output = (completed.stdout or "") + (completed.stderr or "")
        if completed.returncode == 0:
            raise SystemExit("f01_xsd_missing_output_fails: exit 0\n" + output[-1500:])
        if "validated=0" not in output:
            raise SystemExit("missing output did not report validated=0\n" + output[-1500:])


def test_unknown_settings_violation() -> None:
    config = xsd_gate.load_config()
    directory = xsd_gate.locate_schemas(config)
    oracle = xsd_gate.Oracle(directory)
    if oracle.failures:
        raise SystemExit("schemas failed to compile")
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "settings.docx"
        _docx(
            path,
            {
                "word/settings.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:settings xmlns:w='{WML}'>"
                    "<w:stylePaneFormatFilter w:val='0001'/>"
                    "</w:settings>"
                )
            },
        )
        measured = xsd_gate.validate_package(str(path), oracle)
        hits = xsd_gate.registry_hits(xsd_gate.load_registry(), measured.messages)
        code, summary = xsd_gate.decide_gate(
            documents=1,
            validated=1,
            missing=0,
            unmatched=hits["unmatched"],
            our_total=0,
            source_total=0,
        )
        if hits["unmatched"] < 1 or code == 0:
            raise SystemExit(
                "f01_census_unknown_violation_fails: "
                f"unmatched={hits['unmatched']} code={code} {summary} "
                f"messages={measured.messages[:4]}"
            )


def main() -> int:
    test_decide_matrix()
    if "--full" in sys.argv[1:]:
        test_unknown_settings_violation()
        test_missing_output_command()
    print("f01 gates: pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
