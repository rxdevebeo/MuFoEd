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


def test_docpr_id_remap_requires_citation() -> None:
    """TZ-48 waives uniqueness remaps only when the write report cites wp:docPr."""
    registry = census_gate.load_census()
    item = next(entry for entry in registry if entry["id"] == "TZ-48")
    where = "070.docx: word/document.xml"
    label = "wp:docPr@id"
    uncited = (
        "parent=anchor|namespace=wp|parent_namespace=wp|attribute_namespace="
        "|attr=id|was=37|removed=1|named=1|cited=a:theme,wps:wsp"
    )
    cited = (
        "parent=anchor|namespace=wp|parent_namespace=wp|attribute_namespace="
        "|attr=id|was=37|removed=1|named=1|cited=a:theme,wp:docPr,wps:wsp"
    )
    if census_gate.element_item_matches(item, where, label, uncited):
        raise SystemExit("TZ-48 must not waive an uncited foreign docPr@id remap")
    if not census_gate.element_item_matches(item, where, label, cited):
        raise SystemExit("TZ-48 must match when the write report cites wp:docPr")
    hits = census_gate.census_hits(
        registry,
        {
            "message": [],
            "element": [(where, label, uncited)],
            "extension": [],
            "dropped": [],
            "unaccounted": [],
            "lossy": [],
            "picture": [],
            "mce": [],
        },
    )
    if not hits["unclassified_element_changes"]:
        raise SystemExit("uncited docPr@id remap must stay unclassified (FAIL)")


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
    if not census_gate._same_attr_value("b", "5893", "5.893%", "srcRect", "a"):
        raise SystemExit("DrawingML percentage rewrite was treated as a loss")
    for attr, raw, percent, element, ns in [
        ("b", "5893", "5.894%", "srcRect", "a"),
        ("val", "60000", "60%", "hue", "a"),
        ("x", "10000", "10%", "off", "a"),
        ("b", "5893", "5.893%", "srcRect", "w"),
    ]:
        if census_gate._same_attr_value(attr, raw, percent, element, ns):
            raise SystemExit(f"wrong percentage value/type was hidden: {element}@{attr}")
    if not census_gate._same_measure("619", "30.95pt"):
        raise SystemExit("619 twips must equal 30.95pt")
    if census_gate._same_measure("619", "31pt"):
        raise SystemExit("31pt must stay a different length")
    if not census_gate._same_measure("18", "17.99999999999983"):
        raise SystemExit("a float twip that rounds to 18 is the same length")
    if census_gate._same_measure("3125", "3124"):
        raise SystemExit("one twip must stay a different length")
    if not census_gate._same_attr_value("val", "90", "90%", "w", "w"):
        raise SystemExit("text scale 90 and 90% are one ST_TextScale")
    if census_gate._same_attr_value("val", "90", "91%", "w", "w"):
        raise SystemExit("text scale 90 must not match 91%")
    if census_gate._same_attr_value("val", "90", "90%", "sz", "w"):
        raise SystemExit("percent spelling is not a half-point size")
    if not census_gate._same_fiftieths("5000", "100%"):
        raise SystemExit("5000 fiftieths is 100%")
    if census_gate._same_fiftieths("5000", "99%"):
        raise SystemExit("5000 fiftieths is not 99%")
    if not census_gate._same_hex("44546A", "44546a"):
        raise SystemExit("hex colour case is the same value")
    if census_gate._same_hex("44546A", "44546B"):
        raise SystemExit("different hex must stay different")
    if not census_gate._same_attr_value("val", "left", "start"):
        raise SystemExit("jc left must be the start spelling")
    if census_gate._same_attr_value("w", "left", "start"):
        raise SystemExit("direction words are not a width")


def test_relationship_rename_requires_identical_resource_and_type() -> None:
    from types import SimpleNamespace

    a = "http://schemas.openxmlformats.org/drawingml/2006/main"
    r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    rel_ns = "http://schemas.openxmlformats.org/package/2006/relationships"
    oracle = SimpleNamespace(declared={"blip"})
    with tempfile.TemporaryDirectory() as tmp:
        source, output = Path(tmp) / "in.docx", Path(tmp) / "out.docx"
        def package(path, rid, target, data, kind):
            _docx(path, {
                "word/document.xml": f'<a:blip xmlns:a="{a}" xmlns:r="{r}" r:embed="{rid}"/>',
                "word/_rels/document.xml.rels": f'<Relationships xmlns="{rel_ns}"><Relationship Id="{rid}" Target="{target}" Type="{kind}"/></Relationships>',
                "word/" + target: data,
            })
        package(source, "rId5", "media/old.png", "same", r + "/image")
        package(output, "rId1", "media/new.png", "same", "http://purl.oclc.org/ooxml/officeDocument/relationships/image")
        if census_gate.vanished_elements(str(source), str(output), oracle, set()):
            raise SystemExit("resource rename was treated as a loss")
        for data, kind in [("changed", r + "/image"), ("same", r + "/hyperlink")]:
            package(output, "rId1", "media/new.png", data, kind)
            rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
            if not any(label == "a:blip@embed" for _, label, _ in rows):
                raise SystemExit(f"changed resource/type was hidden: {rows}")
        # Equal lexical IDs must not cancel before resolving their targets.
        package(output, "rId5", "media/new.png", "changed", r + "/image")
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        if not any(label == "a:blip@embed" for _, label, _ in rows):
            raise SystemExit("same rId hid changed resource bytes")


def test_process_choice_inventory_does_not_count_fallback() -> None:
    """Source AC Choice+Fallback must not invent a placement loss after write."""
    from types import SimpleNamespace

    mc = "http://schemas.openxmlformats.org/markup-compatibility/2006"
    w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    wp = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
    wps = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
    oracle = SimpleNamespace(declared={"extent", "inline", "document", "body", "p", "r", "drawing"})
    source_xml = f"""<w:document xmlns:w="{w}" xmlns:mc="{mc}" xmlns:wp="{wp}" xmlns:wps="{wps}">
      <w:body><w:p><w:r><w:drawing>
        <mc:AlternateContent>
          <mc:Choice Requires="wps"><wp:inline><wp:extent cx="1" cy="2"/></wp:inline></mc:Choice>
          <mc:Fallback><wp:inline><wp:extent cx="9" cy="9"/></wp:inline></mc:Fallback>
        </mc:AlternateContent>
      </w:drawing></w:r></w:p></w:body></w:document>"""
    written_xml = f"""<w:document xmlns:w="{w}" xmlns:wp="{wp}">
      <w:body><w:p><w:r><w:drawing>
        <wp:inline><wp:extent cx="1" cy="2"/></wp:inline>
      </w:drawing></w:r></w:p></w:body></w:document>"""
    with tempfile.TemporaryDirectory() as tmp:
        source, output = Path(tmp) / "in.docx", Path(tmp) / "out.docx"
        _docx(source, {"word/document.xml": source_xml})
        _docx(output, {"word/document.xml": written_xml})
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        extent_rows = [
            row for row in rows if row[1].endswith("extent") or "@cx" in row[1] or "@cy" in row[1]
        ]
        if extent_rows:
            raise SystemExit(f"ProcessChoice still counted Fallback extents: {extent_rows}")


def main() -> int:
    test_namespace_identity_cannot_hide_a_change()
    test_relationship_rename_requires_identical_resource_and_type()
    test_twip_and_point_are_one_measure()
    test_schema_and_inventory_are_split()
    test_decide_census_gate_matrix()
    test_context_match_rejects_wrong_parent()
    test_named_loss_requires_this_inputs_report()
    test_docpr_id_remap_requires_citation()
    test_review_negative_controls_stay_unclassified()
    test_vanished_emits_qualified_context()
    test_one_of_two_same_nodes_is_visible()
    test_attribute_value_and_resource_bytes()
    test_xsd_negative_control_still_fails()
    test_unnamed_element_loss_fails_as_ours()
    test_process_choice_inventory_does_not_count_fallback()
    test_percent_width_keeps_its_type()
    test_rounded_twip_duplicate_run_and_style()
    print("census_gate_selftest: pass")
    return 0


def test_namespace_identity_cannot_hide_a_change() -> None:
    from types import SimpleNamespace
    a = "http://schemas.openxmlformats.org/drawingml/2006/main"
    strict_a = "http://purl.oclc.org/ooxml/drawingml/main"
    oracle = SimpleNamespace(declared={"xfrm", "off"})
    with tempfile.TemporaryDirectory() as tmp:
        source, output = Path(tmp) / "in.docx", Path(tmp) / "out.docx"
        _docx(source, {"word/document.xml": f'<a:xfrm xmlns:a="{a}"><a:off x="42"/></a:xfrm>'})
        _docx(output, {"word/document.xml": f'<a:xfrm xmlns:a="{strict_a}"><a:off x="42"/></a:xfrm>'})
        if census_gate.vanished_elements(str(source), str(output), oracle, set()):
            raise SystemExit("known namespace transition was treated as loss")
        for text in [
            f'<a:xfrm xmlns:a="{strict_a}" xmlns:f="urn:foreign"><f:off x="42"/></a:xfrm>',
            f'<a:xfrm xmlns:a="{strict_a}" xmlns:f="urn:foreign"><a:off f:x="42"/></a:xfrm>',
            f'<f:xfrm xmlns:a="{strict_a}" xmlns:f="urn:foreign"><a:off x="42"/></f:xfrm>',
        ]:
            _docx(output, {"word/document.xml": text})
            if not census_gate.vanished_elements(str(source), str(output), oracle, set()):
                raise SystemExit("element/parent/attribute namespace change was hidden")


def test_percent_width_keeps_its_type() -> None:
    from types import SimpleNamespace

    oracle = SimpleNamespace(declared={"tblW", "tcW", "gridCol", "document", "body", "tbl", "tblPr", "tblGrid", "tr", "tc", "tcPr", "p"})
    with tempfile.TemporaryDirectory() as tmp:
        source, output = Path(tmp) / "in.docx", Path(tmp) / "out.docx"
        _docx(source, {"word/document.xml": (
            f"<w:document xmlns:w='{WML_T}'><w:body><w:tbl><w:tblPr>"
            "<w:tblW w:w='5000' w:type='pct'/></w:tblPr>"
            "<w:tblGrid><w:gridCol w:w='3125'/></w:tblGrid>"
            "<w:tr><w:tc><w:tcPr><w:tcW w:w='2500' w:type='dxa'/></w:tcPr><w:p/></w:tc></w:tr>"
            "</w:tbl></w:body></w:document>"
        )})
        _docx(output, {"word/document.xml": (
            f"<w:document xmlns:w='{WML_S}'><w:body><w:tbl><w:tblPr>"
            "<w:tblW w:w='100%' w:type='pct'/></w:tblPr>"
            "<w:tblGrid><w:gridCol w:w='156.25pt'/></w:tblGrid>"
            "<w:tr><w:tc><w:tcPr><w:tcW w:w='125pt' w:type='dxa'/></w:tcPr><w:p/></w:tc></w:tr>"
            "</w:tbl></w:body></w:document>"
        )})
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        labels = {label for _part, label, _detail in rows}
        if any(label in labels for label in ("w:tblW@w", "w:gridCol@w", "w:tcW@w")):
            raise SystemExit(f"equivalent widths were losses: {rows}")
        _docx(output, {"word/document.xml": (
            f"<w:document xmlns:w='{WML_S}'><w:body><w:tbl><w:tblPr>"
            "<w:tblW w:w='100%' w:type='pct'/></w:tblPr>"
            "<w:tblGrid><w:gridCol w:w='3124'/></w:tblGrid>"
            "<w:tr><w:tc><w:tcPr><w:tcW w:w='100%' w:type='pct'/></w:tcPr><w:p/></w:tc></w:tr>"
            "</w:tbl></w:body></w:document>"
        )})
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        labels = {label for _part, label, _detail in rows}
        if "w:gridCol@w" not in labels:
            raise SystemExit(f"one twip on gridCol must stay visible: {rows}")
        if "w:tcW@w" not in labels and "w:tcW@type" not in labels:
            raise SystemExit(f"dxa cell width rewritten as percent must stay visible: {rows}")


def test_rounded_twip_duplicate_run_and_style() -> None:
    from types import SimpleNamespace

    oracle = SimpleNamespace(declared={
        "document", "body", "p", "pPr", "r", "rPr", "t", "ind", "sz", "szCs",
        "styles", "style", "spacing",
    })
    with tempfile.TemporaryDirectory() as tmp:
        source, output = Path(tmp) / "in.docx", Path(tmp) / "out.docx"
        _docx(source, {"word/document.xml": (
            f"<w:document xmlns:w='{WML_T}'><w:body><w:p><w:pPr>"
            "<w:ind w:left='71.33333333333312'/></w:pPr>"
            "<w:r><w:rPr><w:sz w:val='23'/><w:sz w:val='23'/></w:rPr><w:t>A</w:t></w:r>"
            "</w:p></w:body></w:document>"
        )})
        _docx(output, {"word/document.xml": (
            f"<w:document xmlns:w='{WML_S}'><w:body><w:p><w:pPr>"
            "<w:ind w:start='71'/></w:pPr>"
            "<w:r><w:rPr><w:sz w:val='23'/></w:rPr><w:t>A</w:t></w:r>"
            "</w:p></w:body></w:document>"
        )})
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        labels = {label for _part, label, _detail in rows}
        if "w:ind@left" in labels or "w:sz" in labels or "w:sz@val" in labels:
            raise SystemExit(f"rounded indent or duplicate sz was a loss: {rows}")
        _docx(output, {"word/document.xml": (
            f"<w:document xmlns:w='{WML_S}'><w:body><w:p><w:pPr>"
            "<w:ind w:start='72'/></w:pPr>"
            "<w:r><w:rPr><w:sz w:val='23'/></w:rPr><w:t>A</w:t></w:r>"
            "</w:p></w:body></w:document>"
        )})
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        labels = {label for _part, label, _detail in rows}
        if "w:ind@left" not in labels and "w:ind@start" not in labels:
            raise SystemExit(f"a whole twip of indent must stay visible: {rows}")
        _docx(source, {"word/styles.xml": (
            f"<w:styles xmlns:w='{WML_T}'>"
            "<w:style w:type='paragraph' w:styleId='Heading1'><w:rPr><w:sz w:val='32'/></w:rPr></w:style>"
            "<w:style w:type='paragraph' w:styleId='Heading1'><w:rPr><w:sz w:val='36'/></w:rPr></w:style>"
            "</w:styles>"
        )})
        _docx(output, {"word/styles.xml": (
            f"<w:styles xmlns:w='{WML_S}'>"
            "<w:style w:type='paragraph' w:styleId='Heading1'><w:rPr><w:sz w:val='36'/></w:rPr></w:style>"
            "</w:styles>"
        )})
        rows = census_gate.vanished_elements(str(source), str(output), oracle, set())
        if any(label == "w:sz@val" for _part, label, _detail in rows):
            raise SystemExit(f"the earlier duplicate style was a loss: {rows}")


if __name__ == "__main__":
    sys.exit(main())
