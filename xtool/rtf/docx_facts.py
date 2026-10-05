#!/usr/bin/env python3
"""Independent XML facts from captured office DOCX, without StrictLib code.

Paragraph text uses Final view and excludes field instructions/textboxes from
its enclosing paragraph. Raw ZIP/XML hashes preserve evidence for other views.
"""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import sys
import xml.etree.ElementTree as ET
import zipfile

from census import write_json

WML = {"http://schemas.openxmlformats.org/wordprocessingml/2006/main", "http://purl.oclc.org/ooxml/wordprocessingml/main"}
MAX_PART = 128 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024


def local(tag):
    return tag.rsplit("}", 1)[-1]


def is_word(node, name):
    return node.tag in {"{" + namespace + "}" + name for namespace in WML}


def paragraph_text(node):
    if any(is_word(node, name) for name in ("del", "moveFrom", "txbxContent", "instrText", "delText", "pPr", "rPr", "sdtPr")):
        return ""
    if is_word(node, "t"):
        return node.text or ""
    if is_word(node, "tab"):
        return "\t"
    if is_word(node, "cr"):
        return "\n"
    if is_word(node, "br"):
        kind = next((value for key, value in node.attrib.items() if local(key) == "type"), "textWrapping")
        return {"page": "\f", "column": "\v"}.get(kind, "\n")
    if is_word(node, "noBreakHyphen"):
        return "\u2011"
    if is_word(node, "softHyphen"):
        return "\u00ad"
    return "".join(paragraph_text(child) for child in node)


def paragraph_records(root, *, path="", in_deleted=False):
    if is_word(root, "txbxContent"):
        return
    deleted = in_deleted or is_word(root, "del") or is_word(root, "moveFrom")
    if is_word(root, "p") and not deleted:
        properties = next((child for child in root if is_word(child, "pPr")), None)
        yield {"path": path, "text": paragraph_text(root),
               "paragraph_properties_xml": ET.tostring(properties, encoding="unicode") if properties is not None else None,
               "run_properties_xml": [ET.tostring(node, encoding="unicode") for node in root.iter() if is_word(node, "rPr")],
               "raw_xml_sha256": hashlib.sha256(ET.tostring(root, encoding="utf-8")).hexdigest()}
        return
    counts = Counter()
    for child in root:
        name = local(child.tag)
        index = counts[name]
        counts[name] += 1
        yield from paragraph_records(child, path=f"{path}/{name}[{index}]", in_deleted=deleted)


def inspect(path):
    regions = []
    media = []
    part_hashes = {}
    font_names = set()
    constructs = Counter()
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        if len(names) != len(set(names)):
            raise ValueError("duplicate ZIP part name")
        if sum(info.file_size for info in infos) > MAX_TOTAL or any(info.file_size > MAX_PART for info in infos):
            raise ValueError("oracle ZIP exceeds inspection budget")
        if "word/document.xml" not in names or "[Content_Types].xml" not in names:
            raise ValueError("missing DOCX main/content-types part")
        for name in sorted(names):
            data = archive.read(name)  # CRC is checked by zipfile.
            part_hashes[name] = hashlib.sha256(data).hexdigest()
            if name.startswith("word/media/"):
                media.append({"part": name, "bytes": len(data), "sha256": part_hashes[name]})
            if not name.startswith("word/") or not name.endswith(".xml"):
                continue
            root = ET.fromstring(data)
            for node in root.iter():
                if node.tag.startswith("{") and node.tag.split("}", 1)[0][1:] in WML:
                    constructs[local(node.tag)] += 1
                if is_word(node, "rFonts"):
                    font_names.update(value for key, value in node.attrib.items() if local(key) in ("ascii", "hAnsi", "eastAsia", "cs"))
            kind = local(root.tag)
            if kind not in ("document", "hdr", "ftr", "footnotes", "endnotes", "comments"):
                continue
            paragraphs = list(paragraph_records(root, path=kind))
            regions.append({"part": name, "kind": kind, "view": "Final", "paragraphs": paragraphs,
                            "text_sha256": hashlib.sha256(json.dumps([row["text"] for row in paragraphs], ensure_ascii=False,
                                                                   separators=(",", ":")).encode("utf-8")).hexdigest()})
            for index, node in enumerate(root.iter()):
                if is_word(node, "txbxContent"):
                    textbox = list(paragraph_records_children(node, path=f"textbox[{index}]"))
                    regions.append({"part": name, "kind": "textbox", "view": "Final", "paragraphs": textbox})
    return {"version": 2, "docx_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "claim": "independent_office_reference_facts_not_import_acceptance", "regions": regions,
            "media": media, "part_sha256": part_hashes, "declared_fonts": sorted(font_names),
            "word_construct_counts": dict(sorted(constructs.items())),
            "limitations": ["Properties remain oracle XML; comparisons must select supported semantics explicitly",
                            "Final-view text excludes deleted/moveFrom text and field instructions",
                            "Font declarations do not prove the actual resolved/rendered face",
                            "w:sym and automatic reference markers require dedicated glyph/text mapping",
                            "Tables and merges counted; detailed grid/merge oracle still requires an adapter",
                            "No PDF/visual reference extracted"]}


def paragraph_records_children(node, path):
    for index, child in enumerate(node):
        yield from paragraph_records(child, path=f"{path}/{local(child.tag)}[{index}]")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--output", type=Path, help="New facts directory; captured references stay in --oracle")
    args = parser.parse_args()
    report = json.loads((args.oracle / "oracle.json").read_text(encoding="utf-8"))
    output = args.output or args.oracle
    if args.output:
        output.mkdir(parents=True, exist_ok=False)
        report["reference_root"] = str(args.oracle)
        write_json(output / "oracle.json", report, exclusive=True)
        fonts = args.oracle / "available-fonts.json"
        if fonts.exists():
            (output / "available-fonts.json").write_bytes(fonts.read_bytes())
    for row in report["files"]:
        if row["status"] != "Captured":
            continue
        directory = args.oracle / row["directory"]
        try:
            facts = inspect(directory / "reference.docx")
            if facts["docx_sha256"] != row["docx_sha256"]:
                raise ValueError("captured reference hash changed")
            destination = output / row["directory"] / "facts.json"
            destination.parent.mkdir(parents=True, exist_ok=True)
            write_json(destination, facts, exclusive=True)
            row["facts"] = {"status": "Extracted", "sha256": hashlib.sha256(destination.read_bytes()).hexdigest(),
                            "construct_counts": facts["word_construct_counts"],
                            "paragraphs": sum(len(region["paragraphs"]) for region in facts["regions"]),
                            "media_parts": len(facts["media"]), "declared_fonts": facts["declared_fonts"],
                            "region_text_sha256": {region["part"]: region["text_sha256"] for region in facts["regions"] if "text_sha256" in region}}
        except (OSError, ValueError, ET.ParseError, zipfile.BadZipFile, RuntimeError) as exc:
            row["facts"] = {"status": "Fail", "reason": str(exc)}
    write_json(output / "facts-index.json", report, exclusive=True)
    extracted = sum(row.get("facts", {}).get("status") == "Extracted" for row in report["files"])
    print(f"facts extracted: {extracted}/{len(report['files'])}")
    return 0 if extracted == 100 else 1


if __name__ == "__main__":
    sys.exit(main())
