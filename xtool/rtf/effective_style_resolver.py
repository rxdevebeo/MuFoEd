#!/usr/bin/env python3
"""Independent effective paragraph/run property resolver for a DOCX.

Resolves the cascade docDefaults -> paragraph style (basedOn chain) -> character
style -> direct properties, for a fixed set of geometry-relevant properties, and
diffs two documents paragraph by paragraph. This is a probe for the RC04
resolved-cascade item, not an acceptance gate: it resolves only the properties it
knows and labels everything it does not.
"""
import argparse
import json
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

TRANS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
STRICT = "http://purl.oclc.org/ooxml/wordprocessingml/main"
PARAGRAPH_PROPS = ("jc", "ind", "spacing", "tabs", "numPr", "pBdr", "widowControl", "keepLines", "keepNext", "bidi")
RUN_PROPS = ("rFonts", "sz", "szCs", "b", "i", "u", "color", "strike", "vertAlign", "position", "spacing", "w", "kern")


def tag(local):
    return local.rsplit("}", 1)[-1]


def get(document, name):
    for namespace in (TRANS, STRICT):
        found = document.find("{" + namespace + "}" + name)
        if found is not None:
            return found
    return None


def local_child(element, name):
    for child in element:
        if tag(child.tag) == name:
            return child
    return None


def value(element):
    if element is None:
        return None
    return sorted(
        (tag(key), val) for key, val in element.attrib.items()
        if tag(key) not in ("rsidR", "rsidRPr", "rsidP", "rsidTr")
    )


def props(element, names):
    if element is None:
        return {}
    out = {}
    for name in names:
        found = local_child(element, name)
        if found is not None:
            out[name] = value(found)
    return out


def load_styles(package):
    defaults = {"pPr": {}, "rPr": {}}
    styles = {}
    try:
        root = ET.fromstring(package.read("word/styles.xml"))
    except KeyError:
        return defaults, styles
    doc_defaults = get(root, "docDefaults")
    if doc_defaults is not None:
        for kind, name in (("pPr", "pPrDefault"), ("rPr", "rPrDefault")):
            wrapper = get(doc_defaults, name)
            inner = wrapper.find("./{" + TRANS + "}" + kind) or wrapper.find("./{" + STRICT + "}" + kind)
            if wrapper is not None:
                inner = get(wrapper, kind)
                if inner is not None:
                    defaults[kind] = props(inner, PARAGRAPH_PROPS if kind == "pPr" else RUN_PROPS)
    for style in root:
        if tag(style.tag) != "style":
            continue
        style_id = None
        for key, val in style.attrib.items():
            if tag(key) == "styleId":
                style_id = val
        style_type = None
        for key, val in style.attrib.items():
            if tag(key) == "type":
                style_type = val
        if style_id is None:
            continue
        based = get(style, "basedOn")
        based_on = None
        if based is not None:
            for key, val in based.attrib.items():
                if tag(key) == "val":
                    based_on = val
        ppr = get(style, "pPr")
        rpr = get(style, "rPr")
        styles[style_id] = {
            "type": style_type,
            "basedOn": based_on,
            "pPr": props(ppr, PARAGRAPH_PROPS) if ppr is not None else {},
            "rPr": props(rpr, RUN_PROPS) if rpr is not None else {},
        }
    return defaults, styles


def chain(style_id, styles):
    seen = []
    while style_id and style_id not in seen and style_id in styles:
        seen.append(style_id)
        style_id = styles[style_id]["basedOn"]
    return list(reversed(seen))


def resolve(style_id, styles, defaults, kind):
    merged = dict(defaults[kind])
    for identifier in chain(style_id, styles):
        merged.update(styles[identifier][kind])
    return merged


def style_id_of(container, key):
    element = local_child(container, key)
    if element is None:
        return None
    for attr, val in element.attrib.items():
        if tag(attr) == "val":
            return val
    return None


def document_paragraphs(package):
    root = ET.fromstring(package.read("word/document.xml"))
    body = get(root, "body")
    out = []
    if body is None:
        return out
    for index, paragraph in enumerate(
        node for node in body.iter() if tag(node.tag) == "p"
    ):
        ppr = get(paragraph, "pPr")
        pstyle = style_id_of(ppr, "pStyle") if ppr is not None else None
        direct = props(ppr, PARAGRAPH_PROPS) if ppr is not None else {}
        runs = []
        for run in paragraph:
            if tag(run.tag) != "r":
                continue
            rpr = get(run, "rPr")
            rstyle = style_id_of(rpr, "rStyle") if rpr is not None else None
            runs.append((rstyle, props(rpr, RUN_PROPS) if rpr is not None else {}))
        out.append({"index": index, "pStyle": pstyle, "direct": direct, "runs": runs})
    return out


def effective(path):
    with zipfile.ZipFile(path) as package:
        defaults, styles = load_styles(package)
        paragraphs = document_paragraphs(package)
    resolved = []
    for paragraph in paragraphs:
        ppr = resolve(paragraph["pStyle"], styles, defaults, "pPr")
        ppr.update(paragraph["direct"])
        runs = []
        for rstyle, direct in paragraph["runs"]:
            rpr = resolve(rstyle, styles, defaults, "rPr")
            rpr.update(direct)
            runs.append(rpr)
        resolved.append({"index": paragraph["index"], "pPr": ppr, "runs": runs})
    return resolved


def diff(left, right):
    justification = {"start": "left", "end": "right", "both": "justify"}

    def canonical(scope, name, entry):
        attrs = {key: val for key, val in entry[name]}
        if name == "jc":
            attrs["val"] = justification.get(attrs.get("val"), attrs.get("val"))
        if name == "spacing":
            attrs = {
                key: val
                for key, val in attrs.items()
                if not (key in ("beforeLines", "afterLines") and val == "0")
            }
        if name == "rFonts":
            attrs.pop("hint", None)
        return json.dumps(attrs, sort_keys=True)

    def flatten(rows):
        out = []
        for row in rows:
            for name in PARAGRAPH_PROPS:
                if row["pPr"].get(name) is not None:
                    out.append((row["index"], "pPr", name, canonical("pPr", name, row["pPr"])))
            for run in row["runs"]:
                for name in RUN_PROPS:
                    if run.get(name) is not None:
                        out.append((row["index"], "rPr", name, canonical("rPr", name, run)))
        return out

    left_rows = flatten(left)
    right_rows = flatten(right)
    left_map = {(a, b, c): d for a, b, c, d in left_rows}
    right_map = {(a, b, c): d for a, b, c, d in right_rows}
    differences = []
    for key in sorted(set(left_map) | set(right_map), key=lambda k: (k[0], k[1], k[2])):
        a = left_map.get(key)
        b = right_map.get(key)
        if a != b:
            differences.append({"paragraph": key[0], "scope": key[1], "prop": key[2],
                                "ours": a, "wps": b})
    return differences


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--our-docx", type=Path, required=True)
    parser.add_argument("--wps-docx", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    differences = diff(effective(args.our_docx), effective(args.wps_docx))
    receipt = {"claim": "resolved effective properties, fixed property subset",
               "our_docx": str(args.our_docx), "wps_docx": str(args.wps_docx),
               "differences": differences, "difference_count": len(differences)}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("differences:", len(differences))
    for entry in differences[:20]:
        print(entry["paragraph"], entry["scope"], entry["prop"], "| ours:", entry["ours"], "| wps:", entry["wps"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
