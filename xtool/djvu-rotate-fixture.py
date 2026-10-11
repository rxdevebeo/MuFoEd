#!/usr/bin/env python3
"""Synthesize rotated copies of a DjVu page for SC-D14 (no encoder needed).

DjVu stores the page rotation in the last byte of the 10-byte `INFO` chunk
(`flags & 0x7`: 1 = 0, 6 = 90, 2 = 180, 5 = 270). Rotating a page therefore
only rewrites that byte; the chunk lengths do not change, so this is a byte
patch and not a re-encode. The first `INFO` (page 1 of a bundled document) is
patched; other flag bits are preserved.

Used to exercise the zone and render oracle drivers on rotated pages.

Usage:
    python3 xtool/djvu-rotate-fixture.py --input book.djvu --out-dir DIR [--rotations 90,180,270]
"""
from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

ROTATION_FLAGS = {0: 1, 90: 6, 180: 2, 270: 5}


def patch_first_page_rotation(data: bytes, degrees: int) -> bytes:
    flag = ROTATION_FLAGS[degrees]
    out = bytearray(data)
    at = 0
    while True:
        found = out.find(b"INFO", at)
        if found < 0:
            raise SystemExit("error: no INFO chunk found")
        if found + 18 <= len(out) and struct.unpack(">I", out[found + 4 : found + 8])[0] == 10:
            flags_at = found + 8 + 9
            out[flags_at] = (out[flags_at] & 0xF8) | flag
            return bytes(out)
        at = found + 4


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--rotations", default="90,180,270")
    args = parser.parse_args()

    source = args.input.read_bytes()
    rotations = [int(part) for part in args.rotations.split(",") if part.strip()]
    for degrees in rotations:
        if degrees not in ROTATION_FLAGS:
            return f"error: unsupported rotation {degrees}" or 2
    args.out_dir.mkdir(parents=True, exist_ok=True)
    stem = args.input.stem
    for degrees in rotations:
        target = args.out_dir / f"{stem}-rot{degrees}.djvu"
        target.write_bytes(patch_first_page_rotation(source, degrees))
        print(target)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
