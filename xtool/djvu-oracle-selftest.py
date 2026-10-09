#!/usr/bin/env python3
"""Self-test the oracle drivers against mock oracles (no DjVuLibre needed).

Builds a throwaway corpus and a set of mock producers/oracles that emit the
documented output shapes, then runs each driver and asserts it passes on a
match and fails on a mismatch. This checks the drivers' plumbing (parsing,
comparison, JSON evidence, resume), not DjVuLibre itself.

Usage: python3 xtool/djvu-oracle-selftest.py
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
IS_WINDOWS = os.name == "nt"


def make_mock(root: Path, name: str, body: str) -> Path:
    """Write a runnable mock: a shebang script on POSIX, a .cmd shim on Windows."""
    script = root / f"{name}.py"
    script.write_text(body, encoding="utf-8")
    if IS_WINDOWS:
        shim = root / f"{name}.cmd"
        shim.write_text(f'@python "%~dp0{name}.py" %*\r\n', encoding="ascii")
        return shim
    script.chmod(0o755)
    return script


READ_PRODUCER = """
import sys
print("FORM:DJVM [100] @0")
print("  DIRM [20] @8")
print("  FORM:DJVU [72] @36")
print("    INFO [10] @44")
"""

READ_ORACLE = """
import sys
print("    FORM:DJVM [100] ")
print("        DIRM [20] ")
print("        FORM:DJVU [72] ")
print("            INFO [10] ")
"""

BAD_ORACLE = READ_ORACLE.replace("INFO [10]", "INFO [11]")

ZONES_PRODUCER = """
import json, sys
print(json.dumps({"file": "a.djvu", "pages": [
    {"page": 1, "width": 100, "height": 100, "dpi": 72, "rotation": 0,
     "words": [{"x": 10.0, "y": 10.0, "w": 20.0, "h": 5.0, "text": "hi"}]}
]}))
"""

ZONES_ORACLE = """
import sys
print("(page 0 0 100 100")
print('(word 10 85 20 5 "hi")')
print(")")
"""

ZONES_BAD = ZONES_ORACLE.replace("10 85 20 5", "15 85 20 5")

PNG_HELPER = """
import struct, zlib
def png(w, h, rgb):
    raw = b"".join(b"\\x00" + bytes(rgb[y * w * 3:(y + 1) * w * 3]) for y in range(h))
    def chunk(t, d):
        c = t + d
        return struct.pack(">I", len(d)) + c + struct.pack(">I", zlib.crc32(c) & 0xffffffff)
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return b"\\x89PNG\\r\\n\\x1a\\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
"""

RENDER_PRODUCER = PNG_HELPER + """
import sys
out = sys.argv[3]
open(out, "wb").write(png(2, 2, bytes([0,0,0, 255,255,255, 128,128,128, 10,20,30])))
"""

RENDER_ORACLE = """
import sys
out = sys.argv[-1]
rgb = bytes([0,0,0, 255,255,255, 128,128,128, 10,20,30])
open(out, "wb").write(b"P6\\n2 2\\n255\\n" + rgb)
"""

RENDER_BAD = RENDER_ORACLE.replace("10,20,30", "10,20,31")

ANT_ORACLE = """
import sys
print('(maparea "" (rect 0 0 10 10) (url "https://example.invalid/a"))')
print('(maparea "" (rect 0 20 10 10) (url "https://example.invalid/b"))')
"""

ANT_OURS = "print('1')"


def run(driver: str, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(HERE / driver), *args],
        capture_output=True,
        text=True,
    )


def expect(label: str, proc: subprocess.CompletedProcess, code: int) -> None:
    if proc.returncode != code:
        raise SystemExit(f"FAIL {label}: exit {proc.returncode} != {code}\n{proc.stdout}\n{proc.stderr}")


def main() -> int:
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        corpus = root / "corpus"
        corpus.mkdir()
        (corpus / "a.djvu").write_bytes(b"AT&T" + b"x" * 64)

        dump_ok = make_mock(root, "djvu_dump_ok", READ_PRODUCER)
        dump_bad = make_mock(root, "djvu_dump_bad", READ_ORACLE.replace("DIRM [20]", "DIRM [21]"))
        oracle_dump = make_mock(root, "djvudump", READ_ORACLE)
        oracle_dump_bad = make_mock(root, "djvudump_bad", BAD_ORACLE)
        zones = make_mock(root, "zones", ZONES_PRODUCER)
        oracle_zones = make_mock(root, "djvutxt", ZONES_ORACLE)
        oracle_zones_bad = make_mock(root, "djvutxt_bad", ZONES_BAD)
        probe = make_mock(root, "codec_probe", RENDER_PRODUCER)
        oracle_render = make_mock(root, "ddjvu", RENDER_ORACLE)
        oracle_render_bad = make_mock(root, "ddjvu_bad", RENDER_BAD)
        oracle_ant = make_mock(root, "djvused", ANT_ORACLE)
        ours_ant = make_mock(root, "ant_ours", ANT_OURS)

        # SC-D3: a match passes, a divergent tree fails.
        report = root / "dump.json"
        expect("dump ok", run(
            "djvu-dump-oracle.py", "--corpus", str(corpus), "--ours", str(dump_ok),
            "--oracle", str(oracle_dump), "--report", str(report)), 0)
        expect("dump mismatch", run(
            "djvu-dump-oracle.py", "--corpus", str(corpus), "--ours", str(dump_bad),
            "--oracle", str(oracle_dump_bad), "--report", str(report)), 1)
        assert json.loads(report.read_text())["ok"] == 0

        # SC-D5: a match passes, a >1 px drift fails.
        expect("zone ok", run(
            "djvu-zone-oracle.py", "--corpus", str(corpus), "--ours", str(zones),
            "--oracle", str(oracle_zones), "--report", str(report)), 0)
        expect("zone mismatch", run(
            "djvu-zone-oracle.py", "--corpus", str(corpus), "--ours", str(zones),
            "--oracle", str(oracle_zones_bad), "--report", str(report)), 1)

        # SC-D7: a match passes, one pixel off fails.
        expect("render ok", run(
            "djvu-render-oracle.py", "--corpus", str(corpus), "--ours", str(probe),
            "--oracle", str(oracle_render), "--report", str(report)), 0)
        expect("render mismatch", run(
            "djvu-render-oracle.py", "--corpus", str(corpus), "--ours", str(probe),
            "--oracle", str(oracle_render_bad), "--report", str(report)), 1)

        # SC-D9: inventory exits 0; a gated producer with a different count fails.
        expect("ant inventory", run(
            "djvu-ant-oracle.py", "--corpus", str(corpus), "--oracle", str(oracle_ant),
            "--report", str(report)), 0)
        assert json.loads(report.read_text())["oracle_mapareas_total"] == 2
        expect("ant gated mismatch", run(
            "djvu-ant-oracle.py", "--corpus", str(corpus), "--oracle", str(oracle_ant),
            "--ours", str(ours_ant), "--report", str(report)), 1)

    print("selftest: all oracle drivers behaved as expected")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
