#!/usr/bin/env python3
"""Extract witness documents per remaining census class."""

from __future__ import annotations

import collections
import gzip
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
INV = ROOT / "census-inventory.json.gz"
WPS = ROOT / "wps-absolute-baselines.json"
OUT = ROOT / "plan-witnesses.json"


def parse(row: list[str]) -> dict:
    where, label, detail = row
    doc, part = where.split(": ", 1) if ": " in where else (where, "")
    fields: dict[str, str] = {}
    for piece in detail.split("|"):
        if "=" in piece:
            key, value = piece.split("=", 1)
            fields[key] = value
    return {
        "doc": doc,
        "part": part,
        "label": label,
        "parent": fields.get("parent", ""),
        "named": fields.get("named", "0"),
        "was": fields.get("was"),
        "ns": fields.get("namespace", ""),
    }


CLASSES = {
    "A_placement": lambda x: x["label"]
    in {"a:off@x", "a:off@y", "a:ext@cx", "a:ext@cy"}
    or x["label"].startswith(("a:chOff", "a:chExt")),
    "B_wp": lambda x: x["label"].startswith("wp:"),
    "C_wps": lambda x: x["ns"]
    in {
        "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    }
    or x["parent"] == "wsp"
    or x["label"].startswith("bodyPr"),
    "D_theme": lambda x: any(
        token in x["label"]
        for token in ("themeColor", "themeTint", "themeShade", "themeFill")
    ),
    "E_color": lambda x: x["label"].startswith(
        ("w:color@", "w:shd", "a:srgbClr", "a:schemeClr")
    )
    and "theme" not in x["label"],
    "F_metrics": lambda x: x["label"].startswith(
        ("w:sz@", "w:szCs", "w:spacing", "w:ind", "w:tab@", "w:w@", "w:jc")
    )
    or x["label"] in {"w:sz", "w:szCs"},
    "G_table": lambda x: any(
        x["label"].startswith(prefix)
        for prefix in (
            "w:tcW",
            "w:tblW",
            "w:gridCol",
            "w:trHeight",
            "w:top@",
            "w:bottom@",
            "w:left@",
            "w:right@",
            "w:inside",
        )
    )
    and "theme" not in x["label"],
    "H_fonts": lambda x: any(
        token in x["label"]
        for token in (
            "rFonts",
            "charset",
            "panose",
            "pitch",
            "w:sig",
            "themeFontLang",
            "hint",
        )
    ),
    "I_rel": lambda x: x["label"]
    in {
        "w:headerReference@id",
        "w:footerReference@id",
        "w:footnote@id",
        "w:endnote@id",
        "c:chart@id",
    },
    "J_ignorable": lambda x: "Ignorable" in x["label"],
    "K_sdt": lambda x: any(
        token in x["label"] for token in ("docPart", "sdtEndPr", "sdtPr", "w:alias", "w:tag")
    )
    or x["parent"] in {"sdtPr", "docPartObj", "sdt"},
    "L_pic": lambda x: "graphicData" in x["label"] or x["label"].startswith("pic:"),
    "M_settings": lambda x: "settings" in x["part"],
    "N_styles": lambda x: x["part"].endswith("styles.xml")
    or x["part"].endswith("numbering.xml"),
    "O_resource": lambda x: x["label"].startswith("resource:"),
}


def main() -> None:
    with gzip.open(INV, "rt", encoding="utf-8") as handle:
        data = json.load(handle)
    parsed = [parse(row) for row in data["unclassified_element_changes"]]

    out: dict = {
        "total_rows": len(parsed),
        "documents_with_rows": len({item["doc"] for item in parsed}),
        "classes": {},
        "clio": {},
        "wps": {},
    }

    assigned: set[int] = set()
    for name, predicate in CLASSES.items():
        hits = []
        for index, item in enumerate(parsed):
            if predicate(item):
                assigned.add(index)
                hits.append(item)
        docs = collections.Counter(item["doc"] for item in hits)
        labels = collections.Counter(item["label"] for item in hits)
        out["classes"][name] = {
            "rows": len(hits),
            "docs": len(docs),
            "named1": sum(1 for item in hits if item["named"] == "1"),
            "top_docs": docs.most_common(15),
            "top_labels": labels.most_common(12),
            "sample_was": [
                {
                    "doc": item["doc"],
                    "label": item["label"],
                    "parent": item["parent"],
                    "was": (item["was"] or "")[:80],
                    "named": item["named"],
                }
                for item in hits
                if item.get("was")
            ][:8],
        }

    rest = [parsed[index] for index in range(len(parsed)) if index not in assigned]
    out["classes"]["Z_other"] = {
        "rows": len(rest),
        "docs": len({item["doc"] for item in rest}),
        "named1": sum(1 for item in rest if item["named"] == "1"),
        "top_docs": collections.Counter(item["doc"] for item in rest).most_common(15),
        "top_labels": collections.Counter(item["label"] for item in rest).most_common(12),
        "sample_was": [],
    }

    clio = [
        item
        for item in parsed
        if "Clio" in item["doc"] or "Sarkissian" in item["doc"]
    ]
    out["clio"] = {
        "rows": len(clio),
        "top_labels": collections.Counter(item["label"] for item in clio).most_common(20),
        "class_hits": {
            name: sum(1 for item in clio if predicate(item))
            for name, predicate in CLASSES.items()
        },
    }

    if WPS.exists():
        wps = json.loads(WPS.read_text(encoding="utf-8"))
        pages = {}
        for page, payload in wps.get("pages", {}).items():
            points = payload.get("points", [])
            pages[page] = {
                "fail": [p for p in points if p.get("status") == "FAIL"],
                "ambiguous": [
                    p for p in points if p.get("status") == "AMBIGUOUS_OR_MISSING"
                ],
                "pdf_sha256": payload.get("pdf_sha256"),
                "svg_sha256": payload.get("svg_sha256"),
            }
        out["wps"] = {
            "source": "Clio Der Sarkissian mitochondrial DNA thesis pages 54/56/104",
            "tolerance_px": wps.get("tolerance_px"),
            "pages": pages,
        }

    OUT.write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"wrote {OUT}")
    for name, info in out["classes"].items():
        print(f"{name}: rows={info['rows']} docs={info['docs']}")
        for doc, count in info["top_docs"][:5]:
            print(f"  {count:4} {doc}")


if __name__ == "__main__":
    main()
