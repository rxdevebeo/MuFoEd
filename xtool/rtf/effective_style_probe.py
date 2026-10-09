#!/usr/bin/env python3
"""Compare direct paragraph/run property names between our DOCX and WPS facts.

This is a probe, not an acceptance gate: WPS expresses much formatting through
styles.xml, while the RTF importer writes it directly on paragraphs and runs, so
a name diff shows a representation difference, not automatically a defect. An
element the WPS paragraphs use and ours never use is the signal worth reading.
"""
import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import docx_facts  # noqa: E402


def local_names(xml):
    return set(re.findall(r"</?[^>]*?:([a-zA-Z]+)", xml))


def names(facts):
    paragraph, run = set(), set()
    for region in facts["regions"]:
        for entry in region["paragraphs"]:
            paragraph |= local_names(entry.get("paragraph_properties_xml", ""))
            for fragment in entry.get("run_properties_xml", []):
                run |= local_names(fragment)
    return paragraph, run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--our-docx", type=Path, required=True)
    parser.add_argument("--wps-facts", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    ours = docx_facts.inspect(args.our_docx)
    reference = json.loads(args.wps_facts.read_text(encoding="utf-8"))
    our_paragraph, our_run = names(ours)
    wps_paragraph, wps_run = names(reference)
    receipt = {
        "claim": "direct-property name diff, not effective-style equality",
        "our_docx": str(args.our_docx),
        "wps_facts": str(args.wps_facts),
        "paragraph": {
            "wps_only": sorted(wps_paragraph - our_paragraph),
            "ours_only": sorted(our_paragraph - wps_paragraph),
            "shared": sorted(our_paragraph & wps_paragraph),
        },
        "run": {
            "wps_only": sorted(wps_run - our_run),
            "ours_only": sorted(our_run - wps_run),
            "shared": sorted(our_run & wps_run),
        },
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(
        json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(receipt["paragraph"] | {"run": receipt["run"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
