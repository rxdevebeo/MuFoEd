#!/usr/bin/env python3
"""R03 census classification selftests and negative controls."""

from __future__ import annotations

import os
import sys
import tempfile
import zipfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import census_gate  # noqa: E402
import xsd_gate  # noqa: E402

HERE = Path(__file__).resolve().parent
WML_T = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
WML_S = "http://purl.oclc.org/ooxml/wordprocessingml/main"
EP = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"


def _docx(path: Path, parts: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as archive:
        for name, text in parts.items():
            archive.writestr(name, text.encode("utf-8"))


def test_schema_and_inventory_are_split() -> None:
    registry = census_gate.load_census()
    signals = {
        "message": [("doc.docx: word/settings.xml", "stylePaneFormatFilter", "not a valid value")],
        "element": [
            ("doc.docx: docProps/app.xml", "ep:Pages", "parent=Properties"),
            ("doc.docx: word/document.xml", "w:left", "parent=tblBorders"),
            ("doc.docx: word/document.xml", "w:mysteryLoss", "parent=p"),
        ],
        "extension": [],
        "dropped": [],
        "unaccounted": [],
        "lossy": [],
        "picture": [],
        "mce": [],
    }
    hits = census_gate.census_hits(registry, signals)
    if not hits["unmatched_schema"]:
        raise SystemExit("schema message was not unmatched_schema")
    if any(row[1] == "w:mysteryLoss" for row in hits["unclassified_element_changes"]) is False:
        raise SystemExit("unknown inventory row was not unclassified_element_changes")
    if any(row[1] in ("ep:Pages", "w:left") for row in hits["unclassified_element_changes"]):
        raise SystemExit("declared/named inventory was left unclassified")
    if hits["unmatched"] != hits["unmatched_schema"]:
        raise SystemExit("unmatched alias must equal unmatched_schema only")
    if "w:mysteryLoss" in {row[1] for row in hits["unmatched_schema"]}:
        raise SystemExit("inventory leaked into unmatched_schema")


def test_decide_census_gate_matrix() -> None:
    code, summary = census_gate.decide_census_gate(
        documents=1,
        validated=1,
        missing=0,
        unmatched_schema=2,
        unclassified_element_changes=0,
        our_total=0,
    )
    if code == 0 or "schema" not in summary:
        raise SystemExit(f"unmatched schema must fail as schema: {code} {summary}")

    code, summary = census_gate.decide_census_gate(
        documents=1,
        validated=1,
        missing=0,
        unmatched_schema=0,
        unclassified_element_changes=5,
        our_total=0,
    )
    if code == 0:
        raise SystemExit(f"unclassified must fail: {summary}")
    if "unclassified_element_changes" not in summary or "not a schema error" not in summary:
        raise SystemExit(f"unclassified must be labeled non-schema: {summary}")
    if "schema violation" in summary:
        raise SystemExit(f"unclassified must not be called schema violation: {summary}")

    code, summary = census_gate.decide_census_gate(
        documents=1,
        validated=1,
        missing=0,
        unmatched_schema=0,
        unclassified_element_changes=0,
        our_total=3,
    )
    if code == 0:
        raise SystemExit(f"owned hits must fail: {summary}")

    code, summary = census_gate.decide_census_gate(
        documents=1,
        validated=1,
        missing=0,
        unmatched_schema=0,
        unclassified_element_changes=0,
        our_total=0,
    )
    if code != 0:
        raise SystemExit(f"clean census must pass: {code} {summary}")

    code, _ = census_gate.decide_census_gate(
        documents=0,
        validated=0,
        missing=0,
        unmatched_schema=0,
        unclassified_element_changes=0,
        our_total=0,
    )
    if code == 0:
        raise SystemExit("zero documents must be unmeasurable")


def test_context_match_rejects_wrong_parent() -> None:
    item = {
        "id": "TZ-23",
        "elements": ["w:left", "left"],
        "parents": ["tblBorders", "tcBorders"],
        "parts": [],
    }
    if not census_gate.element_item_matches(
        item, "doc.docx: word/document.xml", "w:left", "parent=tblBorders"
    ):
        raise SystemExit("tblBorders left must match TZ-23")
    if census_gate.element_item_matches(
        item, "doc.docx: word/document.xml", "w:left", "parent=p"
    ):
        raise SystemExit("left under p must not match table-edge disposition")
    prefix_item = {"id": "TZ-42", "elements": ["a:*"], "parents": [], "parts": []}
    if not census_gate.element_item_matches(
        prefix_item, "doc.docx: word/document.xml", "a:prstGeom", "parent=spPr"
    ):
        raise SystemExit("a:* must match qualified DrawingML labels")
    if census_gate.element_item_matches(
        prefix_item, "doc.docx: word/document.xml", "w:left", "parent=tblBorders"
    ):
        raise SystemExit("a:* must not match w: labels")


def test_vanished_emits_qualified_context() -> None:
    config = xsd_gate.load_config()
    oracle = xsd_gate.Oracle(xsd_gate.locate_schemas(config))
    if oracle.failures:
        raise SystemExit("schemas failed to compile")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "in.docx"
        written = root / "out.docx"
        _docx(
            source,
            {
                "docProps/app.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<Properties xmlns='{EP}'><Pages>1</Pages><Company>X</Company></Properties>"
                ),
                "word/document.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:document xmlns:w='{WML_T}'><w:body><w:tbl>"
                    "<w:tblPr><w:tblBorders><w:left w:val='single'/><w:right w:val='single'/>"
                    "</w:tblBorders></w:tblPr><w:tblGrid/><w:tr><w:tc><w:p/></w:tc></w:tr>"
                    "</w:tbl></w:body></w:document>"
                ),
            },
        )
        _docx(
            written,
            {
                "docProps/app.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<Properties xmlns='{EP}'><Company>X</Company></Properties>"
                ),
                "word/document.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:document xmlns:w='{WML_S}'><w:body><w:tbl>"
                    "<w:tblPr><w:tblBorders><w:start w:val='single'/><w:end w:val='single'/>"
                    "</w:tblBorders></w:tblPr><w:tblGrid/><w:tr><w:tc><w:p/></w:tc></w:tr>"
                    "</w:tbl></w:body></w:document>"
                ),
            },
        )
        rows = census_gate.vanished_elements(str(source), str(written), oracle, set())
        labels = {label for _part, label, _detail in rows}
        details = {detail for _part, _label, detail in rows}
        if "ep:Pages" not in labels:
            raise SystemExit(f"expected ep:Pages in {labels}")
        if "w:left" not in labels or "w:right" not in labels:
            raise SystemExit(f"expected w:left/w:right in {labels}")
        if "parent=tblBorders" not in details:
            raise SystemExit(f"expected parent=tblBorders in {details}")
        hits = census_gate.census_hits(
            census_gate.load_census(),
            {
                "message": [],
                "element": [(f"t.docx: {part}", label, detail) for part, label, detail in rows],
                "extension": [],
                "dropped": [],
                "unaccounted": [],
                "lossy": [],
                "picture": [],
                "mce": [],
            },
        )
        if hits["unmatched_schema"]:
            raise SystemExit("fixture inventory must not be schema unmatched")
        if hits["unclassified_element_changes"]:
            raise SystemExit(
                f"fixture inventory should be disposed: {hits['unclassified_element_changes']}"
            )


def test_xsd_negative_control_still_fails() -> None:
    """F01 unknown settings violation remains a schema failure path."""
    config = xsd_gate.load_config()
    oracle = xsd_gate.Oracle(xsd_gate.locate_schemas(config))
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "settings.docx"
        _docx(
            path,
            {
                "word/settings.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:settings xmlns:w='{WML_S}'>"
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
                f"negative schema control passed: unmatched={hits['unmatched']} "
                f"code={code} {summary}"
            )


def test_unnamed_element_loss_fails_as_ours() -> None:
    registry = [
        {
            "id": "TZ-X",
            "origin": "ours",
            "signal": "element",
            "disposition": "unnamed_loss",
            "elements": ["w:doNotWrapTextWithPunct", "doNotWrapTextWithPunct"],
        }
    ]
    hits = census_gate.census_hits(
        registry,
        {
            "message": [],
            "element": [
                (
                    "doc.docx: word/settings.xml",
                    "w:doNotWrapTextWithPunct",
                    "parent=compat",
                )
            ],
            "extension": [],
            "dropped": [],
            "unaccounted": [],
            "lossy": [],
            "picture": [],
            "mce": [],
        },
    )
    if hits["counts"]["TZ-X"] != 1:
        raise SystemExit("ours element loss was not counted")
    code, summary = census_gate.decide_census_gate(
        documents=1,
        validated=1,
        missing=0,
        unmatched_schema=0,
        unclassified_element_changes=0,
        our_total=hits["counts"]["TZ-X"],
    )
    if code == 0:
        raise SystemExit(f"unnamed owned loss must fail: {summary}")


def main() -> int:
    test_schema_and_inventory_are_split()
    test_decide_census_gate_matrix()
    test_context_match_rejects_wrong_parent()
    test_vanished_emits_qualified_context()
    test_xsd_negative_control_still_fails()
    test_unnamed_element_loss_fails_as_ours()
    print("census_gate_selftest: pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
