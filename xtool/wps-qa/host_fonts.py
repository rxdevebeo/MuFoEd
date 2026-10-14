"""Inventory of the fonts installed on the machine that renders the WPS references.

    python xtool/wps-qa/host_fonts.py [--out PATH] [DIR ...]

For every font file (TrueType/OpenType, collections face by face) it records
the family and style names, the `OS/2` Windows and typographic boxes, the
`hhea` box, `fsType`, and the advance widths of the Latin, Greek, Cyrillic and
general-punctuation characters. Only numbers are read: no outlines, no font
bytes leave the machine. The QA uses it to explain a reference (which families
the host lacked, so WPS substituted them) and to give the renderer the
metrics of the proprietary families it has no metric-compatible face for.
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import sys
from pathlib import Path

RANGES = [(0x20, 0x24F), (0x370, 0x3FF), (0x400, 0x52F), (0x1E00, 0x1EFF), (0x2000, 0x206F), (0x20A0, 0x20CF), (0x2100, 0x215F), (0x2190, 0x21FF), (0x2200, 0x22FF), (0x25A0, 0x25FF)]
SCHEMA = "strictlib.host-fonts.v1"


def _u16(b, o):
    return struct.unpack_from(">H", b, o)[0]


def _i16(b, o):
    return struct.unpack_from(">h", b, o)[0]


def _u32(b, o):
    return struct.unpack_from(">I", b, o)[0]


def _tables(b: bytes, base: int) -> dict[str, tuple[int, int]]:
    n = _u16(b, base + 4)
    out = {}
    for i in range(n):
        r = base + 12 + 16 * i
        out[b[r : r + 4].decode("latin-1")] = (_u32(b, r + 8), _u32(b, r + 12))
    return out


def _names(b: bytes, t) -> dict[int, str]:
    if "name" not in t:
        return {}
    o, _ = t["name"]
    count, strings = _u16(b, o + 2), o + _u16(b, o + 4)
    best: dict[int, tuple[int, str]] = {}
    for i in range(count):
        r = o + 6 + 12 * i
        pid, eid, lang, nid, ln, off = struct.unpack_from(">HHHHHH", b, r)
        raw = b[strings + off : strings + off + ln]
        if pid == 3 and eid in (0, 1, 10):
            text = raw.decode("utf-16-be", "replace")
            rank = 0 if lang == 0x409 else 1
        elif pid == 1 and eid == 0:
            text = raw.decode("mac_roman", "replace")
            rank = 2
        else:
            continue
        if nid not in best or rank < best[nid][0]:
            best[nid] = (rank, text)
    return {k: v for k, (_, v) in best.items()}


def _cmap(b: bytes, t) -> dict[int, int]:
    o, _ = t["cmap"]
    n = _u16(b, o + 2)
    subtables = []
    for i in range(n):
        pid, eid, off = struct.unpack_from(">HHI", b, o + 4 + 8 * i)
        subtables.append((pid, eid, o + off))
    # Unicode full repertoire first, then BMP, then symbol.
    order = {(3, 10): 0, (0, 4): 1, (0, 6): 1, (3, 1): 2, (0, 3): 3, (0, 1): 3, (3, 0): 4}
    subtables.sort(key=lambda s: order.get((s[0], s[1]), 9))
    for pid, eid, at in subtables:
        fmt = _u16(b, at)
        if fmt == 4:
            segs = _u16(b, at + 6) // 2
            ends, starts = at + 14, at + 16 + 2 * segs
            deltas, ranges = starts + 2 * segs, starts + 4 * segs
            out = {}
            for s in range(segs):
                end, start = _u16(b, ends + 2 * s), _u16(b, starts + 2 * s)
                delta, ro = _i16(b, deltas + 2 * s), _u16(b, ranges + 2 * s)
                for c in range(start, end + 1):
                    if c == 0xFFFF:
                        continue
                    if ro == 0:
                        g = (c + delta) & 0xFFFF
                    else:
                        ga = ranges + 2 * s + ro + 2 * (c - start)
                        g = _u16(b, ga) if ga + 2 <= len(b) else 0
                        g = (g + delta) & 0xFFFF if g else 0
                    if g:
                        out[c] = g
            if (pid, eid) == (3, 0):
                # Symbol fonts map U+F0xx; the characters are their low bytes.
                out = {(c & 0xFF if 0xF000 <= c <= 0xF0FF else c): g for c, g in out.items()}
            return out
        if fmt == 12:
            groups = _u32(b, at + 12)
            out = {}
            for i in range(groups):
                start, end, gid = struct.unpack_from(">III", b, at + 16 + 12 * i)
                if start > 0x2FFFF:
                    continue
                for c in range(start, min(end, 0x2FFFF) + 1):
                    out[c] = gid + c - start
            return out
    return {}


def face(b: bytes, base: int) -> dict | None:
    t = _tables(b, base)
    if not all(k in t for k in ("head", "hhea", "hmtx", "cmap")):
        return None
    names = _names(b, t)
    head = t["head"][0]
    upm = _u16(b, head + 18)
    mac_style = _u16(b, head + 44)
    hh = t["hhea"][0]
    hasc, hdesc, hgap = _i16(b, hh + 4), _i16(b, hh + 6), _i16(b, hh + 8)
    nmetrics = _u16(b, hh + 34)
    hmtx = t["hmtx"][0]
    out = {
        "family": names.get(16) or names.get(1, ""),
        "subfamily": names.get(17) or names.get(2, ""),
        "legacy_family": names.get(1, ""),
        "full_name": names.get(4, ""),
        "postscript": names.get(6, ""),
        "units_per_em": upm,
        "hhea": [hasc, hdesc, hgap],
        "bold": bool(mac_style & 1),
        "italic": bool(mac_style & 2),
    }
    if "OS/2" in t:
        o = t["OS/2"][0]
        out["weight"] = _u16(b, o + 4)
        out["fs_type"] = _u16(b, o + 8)
        fs_sel = _u16(b, o + 62)
        out["bold"] = bool(fs_sel & 32) or out["bold"]
        out["italic"] = bool(fs_sel & 1) or out["italic"]
        out["use_typo"] = bool(fs_sel & 128)
        out["typo"] = [_i16(b, o + 68), _i16(b, o + 70), _i16(b, o + 72)]
        out["win"] = [_u16(b, o + 74), _u16(b, o + 76)]
        out["avg_char_width"] = _i16(b, o + 2)
    cmap = _cmap(b, t)

    def advance(g: int) -> int:
        i = min(g, nmetrics - 1)
        return _u16(b, hmtx + 4 * i)

    adv = {}
    for lo, hi in RANGES:
        for c in range(lo, hi + 1):
            g = cmap.get(c)
            if g:
                adv[c] = advance(g)
    out["advances"] = adv
    out["glyphs_in_cmap"] = len(cmap)
    return out


def read_file(path: Path) -> list[dict]:
    b = path.read_bytes()
    bases = [0]
    if b[:4] == b"ttcf":
        bases = [_u32(b, 12 + 4 * i) for i in range(_u32(b, 8))]
    faces = []
    for i, base in enumerate(bases):
        try:
            f = face(b, base)
        except (struct.error, IndexError, KeyError):
            f = None
        if f:
            f["file"] = path.name
            f["face_index"] = i
            faces.append(f)
    return faces


def font_dirs() -> list[Path]:
    dirs = []
    if os.name == "nt":
        dirs.append(Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts")
        local = os.environ.get("LOCALAPPDATA")
        if local:
            dirs.append(Path(local) / "Microsoft" / "Windows" / "Fonts")
    else:
        dirs += [Path("/usr/share/fonts"), Path.home() / ".fonts", Path.home() / ".local/share/fonts"]
    return [d for d in dirs if d.is_dir()]


def inventory(dirs: list[Path]) -> dict:
    faces = []
    for d in dirs:
        for p in sorted(d.rglob("*")):
            if p.suffix.lower() in (".ttf", ".otf", ".ttc", ".otc") and p.is_file():
                try:
                    faces.extend(read_file(p))
                except OSError:
                    continue
    return {"schema": SCHEMA, "dirs": [str(d) for d in dirs], "faces": faces}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=Path("host-fonts.json"))
    parser.add_argument("dirs", nargs="*", type=Path)
    args = parser.parse_args(argv)
    data = inventory(args.dirs or font_dirs())
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    print(f"host fonts: {len(data['faces'])} faces -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
