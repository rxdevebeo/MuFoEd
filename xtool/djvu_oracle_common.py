#!/usr/bin/env python3
"""Shared plumbing for the DjVu oracle drivers.

Every driver compares one artefact of our reader against a pinned external
oracle (the DjVuLibre CLI, invoked as a separate process; its source is never
read). This module keeps the parts they must agree on in one place:

* a stable JSON evidence shape (`run`, `fingerprint`, `write_report`);
* input/output hashing so `--resume` can reuse rows whose input is unchanged;
* image decoding (PNG that we write, PNM that `ddjvu` writes) for pixel checks.

Nothing here imports a DjVu library or links against the oracle.
"""
from __future__ import annotations

import hashlib
import json
import struct
import subprocess
import sys
import zlib
from pathlib import Path

DJ_EXTENSIONS = (".djvu", ".djv")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def package_version(package: str) -> str | None:
    """The distro version of an oracle package, or None off Debian/Ubuntu."""
    try:
        proc = subprocess.run(
            ["dpkg-query", "-W", "-f=${Version}", package],
            capture_output=True,
            timeout=30,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    version = proc.stdout.decode("utf-8", errors="replace").strip()
    if proc.returncode != 0 or not version:
        return None
    return version


def fingerprint(binary: Path, package: str | None) -> dict:
    """What identifies the oracle: its bytes and, if asked, its package version."""
    info = {"path": str(binary), "sha256": sha256_file(binary) if binary.is_file() else None}
    if package:
        info["package"] = package
        info["package_version"] = package_version(package)
    return info


def run(argv: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
    """Run an oracle or producer, never raising on a non-zero exit."""
    return subprocess.run([str(a) for a in argv], capture_output=True, timeout=timeout)


def iter_corpus(root: Path) -> list[Path]:
    return sorted(
        path
        for path in root.iterdir()
        if path.is_file() and path.suffix.lower() in DJ_EXTENSIONS
    )


def compact_text(text: str) -> str:
    """Drop only whitespace and the advisory structural separators and NUL.

    Everything else — accents, combining marks, punctuation — is significant, so
    a difference there is a real difference, not formatting.
    """
    return "".join(c for c in text if not c.isspace() and c not in "\x00\x1d\x1f")


def load_previous(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return {row["file"]: row for row in payload.get("results", [])}


def reused(row: dict | None, digest: str, native_digest: str | None = None) -> bool:
    """A previous row stands when the input and, if recorded, our side is unchanged."""
    if not row or not row.get("ok"):
        return False
    if row.get("sha256") != digest:
        return False
    if native_digest is not None and row.get("ours_sha256") != native_digest:
        return False
    return True


def write_report(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def summarize(criterion: str, description: str, results: list[dict]) -> dict:
    ok = sum(1 for row in results if row.get("ok"))
    return {
        "criterion": criterion,
        "what": description,
        "files": len(results),
        "ok": ok,
        "mismatched": [row["file"] for row in results if not row.get("ok")],
        "results": results,
    }


# --- image decoding -------------------------------------------------------


def decode_png_rgb(data: bytes) -> tuple[int, int, bytes]:
    """Decode an 8-bit, non-interlaced PNG to RGB8 (gray/RGB/gray+A/RGBA)."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, header, idat = 8, None, bytearray()
    while pos + 8 <= len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if header is None:
        raise ValueError("PNG has no IHDR")
    width, height, depth, color, _compression, _filter, interlace = header
    if depth != 8 or interlace != 0:
        raise ValueError(f"unsupported PNG depth={depth} interlace={interlace}")
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color)
    if channels is None:
        raise ValueError(f"unsupported PNG color type {color}")
    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    rgb = bytearray()
    previous = bytearray(stride)
    at = 0
    for _ in range(height):
        filter_type = raw[at]
        at += 1
        line = bytearray(raw[at : at + stride])
        at += stride
        for x in range(stride):
            left = line[x - channels] if x >= channels else 0
            up = previous[x]
            corner = previous[x - channels] if x >= channels else 0
            if filter_type == 1:
                line[x] = (line[x] + left) & 0xFF
            elif filter_type == 2:
                line[x] = (line[x] + up) & 0xFF
            elif filter_type == 3:
                line[x] = (line[x] + ((left + up) >> 1)) & 0xFF
            elif filter_type == 4:
                estimate = left + up - corner
                pa, pb, pc = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
                predictor = left if (pa <= pb and pa <= pc) else (up if pb <= pc else corner)
                line[x] = (line[x] + predictor) & 0xFF
            elif filter_type != 0:
                raise ValueError(f"unsupported PNG filter {filter_type}")
        if channels == 3:
            rgb += line
        elif channels == 1:
            for value in line:
                rgb += bytes((value, value, value))
        elif channels == 2:
            for i in range(0, stride, 2):
                value = line[i]
                rgb += bytes((value, value, value))
        else:
            for i in range(0, stride, 4):
                rgb += line[i : i + 3]
        previous = line
    return width, height, bytes(rgb)


def _pnm_tokens(data: bytes, needed: int) -> tuple[list[bytes], int]:
    tokens: list[bytes] = []
    at = 0
    while len(tokens) < needed:
        while at < len(data) and data[at : at + 1].isspace():
            at += 1
        if at < len(data) and data[at : at + 1] == b"#":
            while at < len(data) and data[at : at + 1] != b"\n":
                at += 1
            continue
        start = at
        while at < len(data) and not data[at : at + 1].isspace():
            at += 1
        if start == at:
            raise ValueError("truncated PNM header")
        tokens.append(data[start:at])
    return tokens, at + 1


def decode_pnm_rgb(data: bytes) -> tuple[int, int, bytes]:
    """Decode a binary PNM (P6 RGB or P5 gray) to RGB8, as `ddjvu` writes it."""
    tokens, at = _pnm_tokens(data, 4)
    magic = tokens[0]
    if magic not in (b"P5", b"P6"):
        raise ValueError(f"unsupported PNM {magic!r}")
    width, height, maxval = int(tokens[1]), int(tokens[2]), int(tokens[3])
    if maxval != 255:
        raise ValueError(f"unsupported PNM maxval {maxval}")
    channels = 3 if magic == b"P6" else 1
    raw = data[at : at + width * height * channels]
    if len(raw) != width * height * channels:
        raise ValueError("truncated PNM data")
    if channels == 1:
        rgb = bytearray()
        for value in raw:
            rgb += bytes((value, value, value))
        return width, height, bytes(rgb)
    return width, height, raw


def diff_stats(left: bytes, right: bytes) -> dict:
    """Exact/mean/max channel difference between two RGB8 buffers."""
    if len(left) != len(right):
        raise ValueError("image sizes differ")
    total = 0
    worst = 0
    equal = 0
    for a, b in zip(left, right):
        delta = abs(a - b)
        total += delta
        if delta > worst:
            worst = delta
        if delta == 0:
            equal += 1
    pixels = len(left)
    return {
        "channels": pixels,
        "mean_channel_error": round(total / pixels, 6) if pixels else 0.0,
        "max_channel_error": worst,
        "equal_fraction": round(equal / pixels, 6) if pixels else 1.0,
    }


def load_tolerances(path: Path | None) -> dict:
    """Per-file `{mean_channel_error, max_channel_error}` limits, or {} for exact."""
    if path is None:
        return {}
    payload = json.loads(path.read_text(encoding="utf-8"))
    return payload.get("tolerances", payload)


def within(row_stats: dict, tolerance: dict | None) -> bool:
    if tolerance is None:
        return row_stats["max_channel_error"] == 0
    return (
        row_stats["mean_channel_error"] <= tolerance.get("mean_channel_error", 0)
        and row_stats["max_channel_error"] <= tolerance.get("max_channel_error", 0)
    )


def main_guard(function) -> None:
    try:
        raise SystemExit(function())
    except KeyboardInterrupt:
        sys.exit(130)
