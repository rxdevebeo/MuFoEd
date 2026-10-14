#!/usr/bin/env python3
"""Independent bidirectional order + geometry oracle (python-bidi + uharfbuzz).

Different engines from the renderer's `unicode-bidi` + `harfrust`, so it can
validate a renderer reordering change. Produces the UBA visual string and the
left-to-right glyph origins/advances of that visual string.
"""
import argparse
import hashlib
import json
from pathlib import Path

import uharfbuzz as hb
from bidi.algorithm import get_display


def shape(data, text):
    face = hb.Face(data)
    font = hb.Font(face)
    font.scale = (face.upem, face.upem)
    hb.ot_font_set_funcs(font)
    buffer = hb.Buffer()
    buffer.add_utf8(text.encode("utf-8"))
    buffer.cluster_level = hb.BufferClusterLevel.MONOTONE_CHARACTERS
    buffer.guess_segment_properties()
    buffer.direction = "ltr"
    hb.shape(font, buffer)
    rows = [
        {"glyph": info.codepoint, "cluster": info.cluster,
         "x_advance": pos.x_advance, "x_offset": pos.x_offset, "y_offset": pos.y_offset}
        for info, pos in zip(buffer.glyph_infos, buffer.glyph_positions)
    ]
    return face.upem, rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--text", required=True)
    parser.add_argument("--base", default="auto", choices=["auto", "ltr", "rtl"])
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    base = {"auto": None, "ltr": "L", "rtl": "R"}[args.base]
    visual = get_display(args.text, base_dir=base)
    data = args.font.read_bytes()
    upem, rows = shape(data, visual)
    receipt = {
        "claim": "independent UBA visual order + geometry; not a renderer acceptance",
        "font_sha256": hashlib.sha256(data).hexdigest(),
        "logical_text": args.text,
        "visual_text": visual,
        "base": args.base,
        "upem": upem,
        "glyphs": rows,
        "total_advance": sum(row["x_advance"] for row in rows),
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("visual:", visual)
    print("glyphs:", len(rows), "advance:", receipt["total_advance"])


if __name__ == "__main__":
    main()
