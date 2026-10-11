"""Build reproducible styled math faces; keep source Unicode and real STIX glyphs.

Requires fontTools. The derived family is renamed under the source SIL OFL.
Only cmap/name/style metadata change; outlines and MATH construction tables stay.
"""
import hashlib
import json
import unicodedata
from pathlib import Path
from fontTools.ttLib import TTFont

root = Path(__file__).resolve().parents[2]
source = root / "strict-ooxml-render-svg/assets/fonts/stix/STIXTwoMath-Regular.otf"
output = source.parent / "strict-math"
output.mkdir(exist_ok=True)
manifest = {"source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "faces": []}
styles = [
    ("Regular", None, False, False),
    ("Italic", "MATHEMATICAL ITALIC ", False, True),
    ("Bold", "MATHEMATICAL BOLD ", True, False),
    ("BoldItalic", "MATHEMATICAL BOLD ITALIC ", True, True),
]


def alphabet(cmap, prefix, legacy=()):
    result = {}
    for code in list(range(0x1D400, 0x1D800)) + list(range(0x2100, 0x2150)):
        name = unicodedata.name(chr(code), "")
        if not (name.startswith(prefix) or name.startswith(legacy)):
            continue
        if name.startswith(prefix) and name[len(prefix):].startswith(("ITALIC ", "SCRIPT ", "FRAKTUR ")):
            continue
        decomposition = unicodedata.decomposition(chr(code)).split()
        if len(decomposition) == 2 and decomposition[0] == "<font>" and code in cmap:
            result[int(decomposition[1], 16)] = cmap[code]
    return result


for profile, (style, prefix, bold, italic) in (
    (profile, style) for profile in ["", "Script", "Fraktur", "DoubleStruck"] for style in styles
):
    font = TTFont(source, recalcTimestamp=False)
    cmap = font.getBestCmap()
    replacements = {}
    if prefix:
        replacements.update(alphabet(cmap, prefix))
        if italic and not bold:
            replacements[ord("h")] = cmap[0x210E]  # italic h lives in Letterlike Symbols
        if bold and italic:
            for index in range(10):  # mathematical digits are upright, also in bold italic
                replacements[ord("0") + index] = cmap[0x1D7CE + index]
    if profile == "Script":
        replacements.update(alphabet(cmap, "MATHEMATICAL BOLD SCRIPT " if bold else "MATHEMATICAL SCRIPT ", () if bold else ("SCRIPT CAPITAL ", "SCRIPT SMALL ")))
    elif profile == "Fraktur":
        replacements.update(alphabet(cmap, "MATHEMATICAL BOLD FRAKTUR " if bold else "MATHEMATICAL FRAKTUR ", () if bold else ("BLACK-LETTER CAPITAL ",)))
    elif profile == "DoubleStruck":
        replacements.update(alphabet(cmap, "MATHEMATICAL DOUBLE-STRUCK ", ("DOUBLE-STRUCK CAPITAL ", "DOUBLE-STRUCK SMALL ")))
    for table in font["cmap"].tables:
        if table.isUnicode():
            table.cmap.update(replacements)
    family = "Strict Math" + (f" {profile}" if profile else "")
    subfamily = "Bold Italic" if style == "BoldItalic" else style
    full = f"{family} {subfamily}"
    postscript = f"StrictMath{profile}-{style}"
    for name_id, value in {1: family, 2: subfamily, 3: postscript, 4: full, 6: postscript, 16: family, 17: subfamily}.items():
        for platform, encoding, language in [(3, 1, 0x409), (1, 0, 0)]:
            font["name"].setName(value, name_id, platform, encoding, language)
    cff = font["CFF "].cff
    cff.fontNames = [postscript]
    cff.topDictIndex[0].FamilyName = family
    cff.topDictIndex[0].FullName = full
    cff.topDictIndex[0].Weight = "Bold" if bold else "Regular"
    font["OS/2"].usWeightClass = 700 if bold else 400
    font["OS/2"].fsSelection = (font["OS/2"].fsSelection & ~(1 | 32 | 64 | 512)) | (1 if italic else 0) | (32 if bold else 0) | (64 if not bold and not italic else 0)
    font["head"].macStyle = (font["head"].macStyle & ~3) | (1 if bold else 0) | (2 if italic else 0)
    font["post"].italicAngle = -12 if italic else 0
    path = output / f"{postscript}.otf"
    font.save(path)
    manifest["faces"].append({"file": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "remapped_characters": len(replacements)})
(output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print(json.dumps(manifest, indent=2))
