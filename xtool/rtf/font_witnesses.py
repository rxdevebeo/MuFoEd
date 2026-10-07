#!/usr/bin/env python3
"""Build narrow renamed OFL witnesses and independent HarfBuzz geometry.

Requires fontTools (version recorded in manifest) and uharfbuzz 0.56.3. Input programs are pinned;
this script does not fetch them. These subsets are test assets, not fallbacks.
"""
import argparse
import hashlib
import json
from pathlib import Path
import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont
import uharfbuzz as hb

SOURCES = [
    ("cjk", "NotoSansCJKsc-Regular.otf", "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
     "f8d157532fbfaeda587e826d4cd5b21a49186f7c", "notofonts/noto-cjk", "Sans/OTF/SimplifiedChinese", "CJK-OFL.txt",
     "StrictLib Witness CJK SC", "漢字中文测试日本語한국어AV0123"),
    ("devanagari", "NotoSansDevanagari-Regular.ttf", "385e78e6359a9d88a0f243d53b1209d7548361ba2194e2b9ec779bcaa7e8949d",
     "ffebf8c1ee449e544955a7e813c54f9b73848eac", "notofonts/noto-fonts", "hinted/ttf/NotoSansDevanagari", "Devanagari-OFL.txt",
     "StrictLib Witness Devanagari", "कर्मनमस्तेहिन्दीक्षि0123"),
]

def digest(data):
    return hashlib.sha256(data).hexdigest()

def shape(data, text):
    face = hb.Face(data)
    font = hb.Font(face)
    font.scale = (face.upem, face.upem)
    hb.ot_font_set_funcs(font)
    buffer = hb.Buffer()
    buffer.add_utf8(text.encode("utf-8"))
    buffer.cluster_level = hb.BufferClusterLevel.MONOTONE_CHARACTERS
    buffer.guess_segment_properties()
    buffer.language = "und"
    hb.shape(font, buffer)
    rows = [(i.codepoint, i.cluster, p.x_advance, p.x_offset, p.y_offset)
            for i, p in zip(buffer.glyph_infos, buffer.glyph_positions)]
    assert all(row[0] != 0 for row in rows), "oracle produced .notdef"
    return face.upem, rows

def build(source, output):
    output.mkdir(parents=True, exist_ok=True)
    manifest = {"schema": "strictlib.font-witness.v1", "license": "OFL-1.1",
                "fonttools": fontTools.__version__, "uharfbuzz": hb.__version__,
                "harfbuzz": hb.version_string(), "purpose": "test-only finite coverage, no runtime fallback", "assets": []}
    for key, filename, pinned, commit, repo, folder, license_file, family, text in SOURCES:
        original = (source / filename).read_bytes()
        assert digest(original) == pinned, filename + " hash mismatch"
        font = TTFont(source / filename, recalcTimestamp=False)
        copyright_text = "\n".join(sorted({n.toUnicode() for n in font["name"].names if n.nameID == 0}))
        options = subset.Options()
        options.layout_features = ["*"]
        options.name_IDs = [0, 1, 2, 3, 4, 6, 13, 14, 16, 17]
        options.name_languages = ["*"]
        options.recalc_timestamp = False
        sub = subset.Subsetter(options=options)
        sub.populate(text=text)
        sub.subset(font)
        names = {1: family, 2: "Regular", 3: family.replace(" ", "") + "-Regular-v1",
                 4: family + " Regular", 6: family.replace(" ", "") + "-Regular",
                 16: family, 17: "Regular"}
        for record in font["name"].names:
            if record.nameID in names:
                record.string = names[record.nameID].encode(record.getEncoding())
        if "CFF " in font:
            cff = font["CFF "].cff
            cff.fontNames = [names[6]]
            cff.topDictIndex[0].FamilyName = family
            cff.topDictIndex[0].FullName = names[4]
        fixture = output / (key + Path(filename).suffix)
        font.save(fixture)
        data = fixture.read_bytes()
        original_upem, original_rows = shape(original, text)
        upem, rows = shape(data, text)
        assert upem == original_upem
        assert [r[1:] for r in rows] == [r[1:] for r in original_rows], "subset changed geometry"
        original_names = TTFont(source / filename).getGlyphOrder()
        subset_names = font.getGlyphOrder()
        assert [original_names[r[0]] for r in original_rows] == [subset_names[r[0]] for r in rows], "subset changed substitutions"
        (output / (key + ".txt")).write_text(text, encoding="utf-8")
        (output / (key + ".tsv")).write_text(str(upem) + "\n" + "\n".join("\t".join(map(str, row)) for row in rows) + "\n", encoding="utf-8")
        (output / license_file).write_bytes((source / license_file).read_bytes())
        (output / (key + "-COPYRIGHT.txt")).write_text(copyright_text + "\n", encoding="utf-8")
        manifest["assets"].append({"id": key, "family": family, "source_url": f"https://github.com/{repo}/blob/{commit}/{folder}/{filename}",
            "source_sha256": pinned, "source_bytes": len(original), "fixture": fixture.name, "fixture_sha256": digest(data),
            "fixture_bytes": len(data), "text": text, "license_file": license_file,
            "modifications": "finite text subset with all layout features and closure; renamed family/PostScript/CFF names; unchanged timestamps",
            "oracle": "native HarfBuzz, default features, und, guessed properties, MONOTONE_CHARACTERS; original/subset glyph names and geometry identical"})
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    build(args.source, args.output)
