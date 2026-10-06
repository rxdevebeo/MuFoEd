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
            (
                "doc.docx: docProps/app.xml",
                "ep:Pages",
                "parent=Properties|namespace=ep|removed=1|named=1",
            ),
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
    if census_gate.element_item_matches(
        prefix_item, "doc.docx: word/document.xml", "a:prstGeom", "parent=spPr"
    ):
        raise SystemExit("a:* is not a disposition")
    if census_gate.element_item_matches(
        prefix_item, "doc.docx: word/document.xml", "w:left", "parent=tblBorders"
    ):
        raise SystemExit("a:* must not match w: labels")
    if census_gate.element_item_matches(
        item, "doc.docx: word/document.xml", "a:left", "parent=tblBorders"
    ):
        raise SystemExit("w:left must not accept a:left")
    star = {"id": "TZ-35", "elements": ["*"], "parts": ["word/settings.xml"], "parents": []}
    if census_gate.element_item_matches(
        star, "fake.docx: word/settings.xml", "w:trackRevisions", "parent=settings"
    ):
        raise SystemExit("* must not dispose an unknown settings change")


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
        rows = census_gate.vanished_elements(
            str(source), str(written), oracle, {"Pages", "ep:Pages"}
        )
        labels = {label for _part, label, _detail in rows}
        details = {detail for _part, _label, detail in rows}
        if "ep:Pages" not in labels:
            raise SystemExit(f"expected ep:Pages in {labels}")
        if "w:left" not in labels or "w:right" not in labels:
            raise SystemExit(f"expected w:left/w:right in {labels}")
        if not any(detail.startswith("parent=tblBorders") for detail in details):
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


def test_named_loss_requires_this_inputs_report() -> None:
    item = {
        "id": "TZ-22",
        "disposition": "named_loss",
        "elements": ["ep:Pages", "Pages"],
        "parents": ["Properties"],
        "parts": ["docProps/app.xml"],
    }
    if census_gate.element_item_matches(
        item,
        "doc.docx: docProps/app.xml",
        "ep:Pages",
        "parent=Properties|namespace=ep|removed=1|named=0",
    ):
        raise SystemExit("named_loss matched a report that did not name Pages")
    if not census_gate.element_item_matches(
        item,
        "doc.docx: docProps/app.xml",
        "ep:Pages",
        "parent=Properties|namespace=ep|removed=1|named=1",
    ):
        raise SystemExit("named_loss must match when this input's report names Pages")


def test_review_negative_controls_stay_unclassified() -> None:
    """The 2026-10-05 controls: unknown settings, DrawingML, and a:left."""
    registry = census_gate.load_census()
    rows = [
        ("fake.docx: word/settings.xml", "w:trackRevisions", "parent=settings|namespace=w|removed=1|named=0"),
        ("fake.docx: word/document.xml", "a:blipFill", "parent=graphic|namespace=a|removed=1|named=0"),
        ("fake.docx: word/document.xml", "a:left", "parent=tblBorders|namespace=a|removed=1|named=0"),
    ]
    hits = census_gate.census_hits(
        registry,
        {
            "message": [],
            "element": rows,
            "extension": [],
            "dropped": [],
            "unaccounted": [],
            "lossy": [],
            "picture": [],
            "mce": [],
        },
    )
    unclassified = {row[1] for row in hits["unclassified_element_changes"]}
    if unclassified != {"w:trackRevisions", "a:blipFill", "a:left"}:
        raise SystemExit(f"negative controls were disposed: {hits['unclassified_element_changes']}")
    if hits["counts"]["TZ-23"] or hits["counts"]["TZ-35"] or hits["counts"]["TZ-42"]:
        raise SystemExit(
            f"wildcard or wrong-namespace item matched: "
            f"TZ-23={hits['counts']['TZ-23']} TZ-35={hits['counts']['TZ-35']} "
            f"TZ-42={hits['counts']['TZ-42']}"
        )


def test_one_of_two_same_nodes_is_visible() -> None:
    config = xsd_gate.load_config()
    oracle = xsd_gate.Oracle(xsd_gate.locate_schemas(config))
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "in.docx"
        written = root / "out.docx"
        body = (
            "<?xml version='1.0' encoding='UTF-8'?>"
            f"<w:document xmlns:w='{WML_T}'><w:body>"
            "<w:p><w:r><w:t>a</w:t></w:r></w:p>"
            "<w:p><w:r><w:t>b</w:t></w:r></w:p>"
            "</w:body></w:document>"
        )
        kept = (
            "<?xml version='1.0' encoding='UTF-8'?>"
            f"<w:document xmlns:w='{WML_S}'><w:body>"
            "<w:p><w:r><w:t>a</w:t></w:r></w:p>"
            "</w:body></w:document>"
        )
        _docx(source, {"word/document.xml": body})
        _docx(written, {"word/document.xml": kept})
        rows = census_gate.vanished_elements(str(source), str(written), oracle, set())
        hit = [row for row in rows if row[1] == "w:p" and "removed=1" in row[2]]
        if len(hit) != 1:
            raise SystemExit(f"one of two w:p was hidden: {rows}")


def test_attribute_value_and_resource_bytes() -> None:
    config = xsd_gate.load_config()
    oracle = xsd_gate.Oracle(xsd_gate.locate_schemas(config))
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "in.docx"
        written = root / "out.docx"
        _docx(
            source,
            {
                "word/document.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:document xmlns:w='{WML_T}'><w:body><w:p>"
                    "<w:pPr><w:jc w:val='center'/></w:pPr><w:r><w:t>a</w:t></w:r>"
                    "</w:p></w:body></w:document>"
                ),
                "word/media/image1.png": "png-a",
            },
        )
        _docx(
            written,
            {
                "word/document.xml": (
                    "<?xml version='1.0' encoding='UTF-8'?>"
                    f"<w:document xmlns:w='{WML_S}'><w:body><w:p>"
                    "<w:pPr><w:jc w:val='both'/></w:pPr><w:r><w:t>a</w:t></w:r>"
                    "</w:p></w:body></w:document>"
                ),
                "word/media/image1.png": "png-b",
            },
        )
        rows = census_gate.vanished_elements(str(source), str(written), oracle, set())
        labels = {label for _part, label, _detail in rows}
        if "w:jc@val" not in labels:
            raise SystemExit(f"jc value change was hidden: {rows}")
        if "resource:word/media/image#.png" not in labels:
            raise SystemExit(f"resource byte change was hidden: {rows}")
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
        unclassified = {row[1] for row in hits["unclassified_element_changes"]}
        if "w:jc@val" not in unclassified or "resource:word/media/image#.png" not in unclassified:
            raise SystemExit(f"property/resource change was disposed: {unclassified}")


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


def test_twip_and_point_are_one_measure() -> None:
    if not census_gate._same_measure("619", "30.95pt"):
        raise SystemExit("619 twips must equal 30.95pt")
    if census_gate._same_measure("619", "31pt"):
        raise SystemExit("31pt must stay a different length")
    if not census_gate._same_hex("44546A", "44546a"):
        raise SystemExit("hex colour case is the same value")
    if census_gate._same_hex("44546A", "44546B"):
        raise SystemExit("different hex must stay different")
    if not census_gate._same_attr_value("val", "left", "start"):
        raise SystemExit("jc left must be the start spelling")
    if census_gate._same_attr_value("w", "left", "start"):
        raise SystemExit("direction words are not a width")


def main() -> int:
    test_twip_and_point_are_one_measure()
    test_schema_and_inventory_are_split()
    test_decide_census_gate_matrix()
    test_context_match_rejects_wrong_parent()
    test_named_loss_requires_this_inputs_report()
    test_review_negative_controls_stay_unclassified()
    test_vanished_emits_qualified_context()
    test_one_of_two_same_nodes_is_visible()
    test_attribute_value_and_resource_bytes()
    test_xsd_negative_control_still_fails()
    test_unnamed_element_loss_fails_as_ours()
    print("census_gate_selftest: pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
