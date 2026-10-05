#!/usr/bin/env python3
"""Independent selected XML facts for RTF/WPS probes, including differences.

IDs are resolved to definitions/content; namespace and true/1 differences are
serialization only. Missing properties stay missing, never inferred as equal.
"""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

from census import write_json
from docx_facts import local
from public_corpus import paragraphs, visible


def attr(node, key, default=None):
    return next((v for k, v in node.attrib.items() if local(k) == key), default) if node is not None else default


def child(node, name):
    return next((n for n in node if local(n.tag) == name), None) if node is not None else None


def props(node):
    if node is None:
        return {}
    result = {}
    for n in node:
        name = local(n.tag)
        if name in {"b", "i", "bCs", "iCs"}:
            result[name] = attr(n, "val", "true") not in {"false", "0", "off"}
        elif name in {"sz", "szCs", "numFmt", "start", "lvlText", "suff", "basedOn"}:
            result[name] = attr(n, "val")
        elif name in {"spacing", "ind", "tab"}:
            result[name] = {local(k): {"left": "start", "right": "end"}.get(v, v)
                            for k, v in n.attrib.items()}
        elif name in {"tabs", "pPr", "rPr"}:
            result[name] = (
                [props_wrapper(c) for c in n] if name == "tabs" else props(n))
    return result


def props_wrapper(node):
    wrapper = ET.Element("wrapper")
    wrapper.append(node)
    return props(wrapper)


def twips(value):
    """Strict universal measures and Transitional twips denote the same length."""
    if value is None:
        return None
    units = {"pt": Decimal(20), "in": Decimal(1440), "cm": Decimal(1440) / Decimal("2.54"),
             "mm": Decimal(1440) / Decimal("25.4"), "pc": Decimal(240), "pi": Decimal(240)}
    if value[-2:] in units:
        return str(Decimal(value[:-2]) * units[value[-2:]])
    return str(Decimal(value))


def inspect(path):
    with zipfile.ZipFile(path) as package:
        def root(name):
            return ET.fromstring(package.read(name)) if name in package.namelist() else None
        document = root("word/document.xml")
        body = child(document, "body")
        styles = root("word/styles.xml")
        definitions = {} if styles is None else {
            attr(s, "styleId"): s for s in styles if local(s.tag) == "style"}
        style_names = {key: attr(child(s, "name"), "val") for key, s in definitions.items()}
        result = {"body": paragraphs(body), "styles": {}, "paragraph_properties": [],
                  "fields": [], "notes": {}, "note_references": [], "headers": [],
                  "tables": [], "numbering": []}
        for key, s in definitions.items():
            name = style_names[key]
            if name not in {"Normal", "Title", "Emphasis"}:
                continue
            result["styles"][name] = {"type": attr(s, "type"),
                "based_on": style_names.get(attr(child(s, "basedOn"), "val")),
                "run": props(child(s, "rPr"))}
        numbers = root("word/numbering.xml")
        abstracts = {} if numbers is None else {
            attr(n, "abstractNumId"): n for n in numbers if local(n.tag) == "abstractNum"}
        instances = {} if numbers is None else {
            attr(n, "numId"): attr(child(n, "abstractNumId"), "val")
            for n in numbers if local(n.tag) == "num"}
        for p in body.iter():
            if local(p.tag) != "p":
                continue
            pp = child(p, "pPr")
            result["paragraph_properties"].append(props(pp))
            np = child(pp, "numPr")
            if np is not None:
                abstract = abstracts.get(instances.get(attr(child(np, "numId"), "val")))
                level = attr(child(np, "ilvl"), "val", "0")
                definition = next((l for l in abstract if local(l.tag) == "lvl" and attr(l, "ilvl") == level), None) if abstract is not None else None
                result["numbering"].append({"text": visible(p), "level": level,
                                            "definition": props(definition)})
        for n in body.iter():
            if local(n.tag) == "fldSimple":
                result["fields"].append(attr(n, "instr", "").strip())
            if local(n.tag) == "instrText":
                result["fields"].append((n.text or "").strip())
        note_lookup = {}
        for kind in ("footnote", "endnote"):
            table = root("word/" + kind + "s.xml")
            if table is None:
                continue
            normal = [n for n in table if attr(n, "type", "normal") == "normal"]
            result["notes"][kind] = [paragraphs(n) for n in normal]
            note_lookup[kind] = {attr(n, "id"): paragraphs(n) for n in normal}
        for p_index, p in enumerate(n for n in body.iter() if local(n.tag) == "p"):
            prefix = ""
            for n in p:
                for ref in n.iter():
                    kind = local(ref.tag).removesuffix("Reference")
                    if local(ref.tag) in {"footnoteReference", "endnoteReference"}:
                        result["note_references"].append({"kind": kind, "paragraph": p_index,
                            "preceding_text": prefix, "note": note_lookup.get(kind, {}).get(attr(ref, "id")),
                            "custom_mark": attr(ref, "customMarkFollows", "0") in {"1", "true"}})
                prefix += visible(n)
        rels = root("word/_rels/document.xml.rels")
        targets = {} if rels is None else {attr(r, "Id"): attr(r, "Target") for r in rels}
        for ref in body.iter():
            if local(ref.tag) in {"headerReference", "footerReference"}:
                target = targets.get(attr(ref, "id"))
                part = target.lstrip("/") if target and target.startswith("/") else "word/" + (target or "")
                r = root(part)
                result["headers"].append({"kind": local(ref.tag), "type": attr(ref, "type"),
                                          "paragraphs": paragraphs(r) if r is not None else None})
        for table in (n for n in body.iter() if local(n.tag) == "tbl"):
            result["tables"].append([[{"paragraphs": paragraphs(cell),
                "vertical_merge": attr(child(child(cell, "tcPr"), "vMerge"), "val", "continue")
                    if child(child(cell, "tcPr"), "vMerge") is not None else None,
                "width_twips": twips(attr(child(child(cell, "tcPr"), "tcW"), "w"))}
                for cell in row if local(cell.tag) == "tc"]
                for row in table if local(row.tag) == "tr"])
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, default=Path("docs/rtf/structure.fixture-inputs.json"))
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--actual", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    rows = []
    for fixture in json.loads(args.fixtures.read_text(encoding="utf-8"))["files"]:
        case = fixture["case"]
        source = args.references / case / "input.rtf"
        reference = args.references / case / "reference.docx"
        actual = args.actual / (case + ".docx")
        if source.read_bytes() != bytes.fromhex(fixture["input_hex"]):
            raise ValueError("probe source changed: " + case)
        expected, observed = inspect(reference), inspect(actual)
        diffs = [key for key in expected if expected[key] != observed[key]]
        rows.append({"case": case, "input_hex": fixture["input_hex"],
                     "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                     "reference_sha256": hashlib.sha256(reference.read_bytes()).hexdigest(),
                     "actual_sha256": hashlib.sha256(actual.read_bytes()).hexdigest(),
                     "different_fact_groups": diffs, "reference": expected, "actual": observed})
        print(case, ", ".join(diffs) or "selected facts equal")
    write_json(args.out, {"claim": "selected_structural_facts_not_full_fidelity", "files": rows})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
