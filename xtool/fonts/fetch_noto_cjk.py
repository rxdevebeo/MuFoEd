"""Fetch the Noto Sans CJK pack and write its font manifest.

    python xtool/fonts/fetch_noto_cjk.py [--dest testdata/fonts/noto-cjk]

Downloads the Regular and Bold collections of Noto Sans CJK (SIL OFL 1.1) from
`notofonts/noto-cjk` at a pinned commit, checks each file against its pinned
SHA-256 and writes `manifest.json` (`strictlib.font-assets.v1`) beside them:
one binding per language face, each marked as a `fallback` for its language,
so `to-pdf --fonts <dest>/manifest.json` draws CJK text that the requested
fonts (SimSun, MS Mincho, Batang, ...) would have drawn. The files are not
bundled with the library (40 MB); `testdata/` is ignored by git.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

COMMIT = "165c01b46ea533872e002e0785ff17e44f6d97d8"
BASE = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{COMMIT}"
FILES = {
    "NotoSansCJK-Regular.ttc": "b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a",
    "NotoSansCJK-Bold.ttc": "faa5f3656a78b2e2d450d27fe8382c778bc2b6bb5ea29c986664a6a435056ceb",
    "LICENSE": "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
}
SOURCES = {
    "NotoSansCJK-Regular.ttc": f"{BASE}/Sans/OTC/NotoSansCJK-Regular.ttc",
    "NotoSansCJK-Bold.ttc": f"{BASE}/Sans/OTC/NotoSansCJK-Bold.ttc",
    "LICENSE": f"{BASE}/LICENSE",
}
# Face order of both collections (name ID 4 at that commit). Simplified
# Chinese first: Han text in a run that names no language falls to the first.
FACES = [(2, "SC", "zh-cn"), (3, "TC", "zh-tw"), (4, "HK", "zh-hk"), (0, "JP", "ja"), (1, "KR", "ko")]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetch(name: str, dest: Path) -> None:
    path = dest / name
    pin = FILES.get(name)
    if path.exists() and (pin is None or sha256(path) == pin):
        return
    print(f"fetch {SOURCES[name]}", flush=True)
    tmp = path.with_suffix(path.suffix + ".part")
    with urllib.request.urlopen(SOURCES[name], timeout=600) as response, tmp.open("wb") as out:
        while block := response.read(1 << 20):
            out.write(block)
    if pin is not None and sha256(tmp) != pin:
        tmp.unlink()
        raise SystemExit(f"{name}: SHA-256 does not match the pin")
    tmp.replace(path)


def manifest() -> dict:
    assets = []
    for index, tag, language in FACES:
        for file, bold in (("NotoSansCJK-Regular.ttc", False), ("NotoSansCJK-Bold.ttc", True)):
            assets.append(
                {
                    "family": f"Noto Sans CJK {tag}",
                    "bold": bold,
                    "path": file,
                    "sha256": FILES[file],
                    "face_index": index,
                    "fallback": language,
                    "source": SOURCES[file],
                    "license_id": "OFL-1.1",
                    "license_reference": "LICENSE",
                }
            )
    return {"schema": "strictlib.font-assets.v1", "assets": assets}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dest", type=Path, default=Path("testdata/fonts/noto-cjk"))
    args = parser.parse_args(argv)
    args.dest.mkdir(parents=True, exist_ok=True)
    for name in SOURCES:
        fetch(name, args.dest)
    out = args.dest / "manifest.json"
    out.write_text(json.dumps(manifest(), indent=2) + "\n", encoding="utf-8")
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
