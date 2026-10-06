#!/usr/bin/env python3
"""WPS absolute-origin ledger and matcher (plan P0).

Coordinates are glyph origins, page top-left, 96 dpi. A point is MEASURED only
when (page, text, neighborhood) selects exactly one reference and one actual
hit, or when the text is unique on the page. Substring-first matching is
forbidden. AMBIGUOUS is never PASS.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any
from xml.etree import ElementTree as ET

PT_TO_PX = 4.0 / 3.0
TOLERANCE_PX = 0.25
Y_TOL_PX = 1.25
BULLETS = "■□▪▫●○"
BREAK_CHARS = set(" \t\n\r_:;")
TOKEN_STRIP = ".;:\"'()[]{}"
ALNUM = re.compile(r"[A-Za-z0-9]+")

CLIO_DOC = (
    "strict-ooxml-core/tests/docx/"
    "Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx"
)
CLIO_SHA256 = "4c5b9b178bdc8c3abae865f00ab5aaa9e102f81634e53691afc3cd3073162462"

# Mandatory Clio keys from the visual plan. Each unique neighborhood pair is
# one component; unique-on-page text is one component.
CLIO_KEYS: dict[int, list[str]] = {
    54: ["HVR-I", "L15996", "H16142", "L16055", "L16117", "H16233"],
    56: ["-M", "H", "8994", "6371", "11719", "14766", "7028"],
    104: ["A,C", "B,D", "modern"],
}


class Glyph(dict):
    """ch, x, y, font, size_px, transform."""


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def core_text(text: str) -> str:
    return text.lstrip(BULLETS)


def norm_neighbor(text: str | None) -> str | None:
    if text is None:
        return None
    parts = ALNUM.findall(text)
    if not parts:
        return None
    return "".join(parts).lower()


def neighbors_compatible(a: str | None, b: str | None) -> bool:
    na, nb = norm_neighbor(a), norm_neighbor(b)
    if na == nb:
        return True
    if na is None or nb is None:
        return False
    if min(len(na), len(nb)) < 3:
        return False
    return na.startswith(nb) or nb.startswith(na)


def _new_token(g: Glyph) -> dict[str, Any]:
    return {
        "text": g["ch"],
        "x": g["x"],
        "y": g["y"],
        "font": g.get("font"),
        "size_px": g.get("size_px") or 12.0 * PT_TO_PX,
        "transform": g.get("transform") or "identity",
        "last_x": g["x"],
    }


def glue_tokens(glyphs: list[Glyph]) -> list[dict[str, Any]]:
    """Glue split runs; break on space, underscore, colon, and word gaps."""
    tokens: list[dict[str, Any]] = []
    cur: dict[str, Any] | None = None
    for g in glyphs:
        ch = g["ch"]
        if ch in BREAK_CHARS or ch in BULLETS:
            if cur is not None:
                tokens.append(cur)
                cur = None
            if ch in BULLETS:
                tokens.append(
                    {
                        "text": ch,
                        "x": g["x"],
                        "y": g["y"],
                        "font": g.get("font"),
                        "size_px": g.get("size_px") or 12.0 * PT_TO_PX,
                        "transform": g.get("transform") or "identity",
                        "last_x": g["x"],
                    }
                )
            continue
        if cur is None:
            cur = _new_token(g)
            continue
        same_line = abs(g["y"] - cur["y"]) <= Y_TOL_PX
        gx, lx = g["x"], cur["last_x"]
        size = cur["size_px"]
        # ~1em: keep primer labels and haplogroup HV together; spaces still break.
        max_dx = max(1.05 * size, 8.0)
        if gx is None and same_line:
            close = True
        elif gx is None or lx is None:
            close = False
        else:
            dx = gx - lx
            close = 0 <= dx <= max_dx
        if same_line and close:
            cur["text"] += ch
            if gx is not None:
                cur["last_x"] = gx
        else:
            tokens.append(cur)
            cur = _new_token(g)
    if cur is not None:
        tokens.append(cur)
    cleaned: list[dict[str, Any]] = []
    for tok in tokens:
        tok["text"] = tok["text"].strip(TOKEN_STRIP)
        if tok["text"]:
            cleaned.append(tok)
    return cleaned


NB_WINDOW = 12


def glyph_stream(glyphs: list[Glyph]) -> tuple[list[Glyph], str, list[int | None]]:
    """Skip underscores so split runs concatenate. Keep spaces as boundaries."""
    kept: list[Glyph] = []
    chars: list[str] = []
    index_map: list[int | None] = []
    for g in glyphs:
        ch = str(g["ch"])
        if ch == "_":
            continue
        if ch.isspace():
            chars.append(" ")
            index_map.append(None)
            continue
        index_map.append(len(kept))
        kept.append(g)
        chars.append(ch)
    return kept, "".join(chars), index_map


def is_whole_match(text: str, index: int, key: str) -> bool:
    """Short keys must not match inside a longer identifier (H in HVR-I / HV)."""
    after = index + len(key)
    if len(key) >= 3:
        return True
    before_ok = (not key[0].isalnum()) or index == 0 or not text[index - 1].isalnum()
    after_ok = (
        (not key[-1].isalnum())
        or after >= len(text)
        or not text[after].isalnum()
    )
    return before_ok and after_ok


def nonspace_window(text: str, start: int, end: int, toward: str, n: int) -> str | None:
    chars: list[str] = []
    if toward == "left":
        j = start - 1
        while j >= 0 and len(chars) < n:
            if not text[j].isspace():
                chars.append(text[j])
            j -= 1
        chars.reverse()
    else:
        j = end
        while j < len(text) and len(chars) < n:
            if not text[j].isspace():
                chars.append(text[j])
            j += 1
    return "".join(chars) or None


def find_key_hits(glyphs: list[Glyph], key: str) -> list[dict[str, Any]]:
    """All whole occurrences. Never returns only the first of several."""
    stream, text, index_map = glyph_stream(glyphs)
    hits: list[dict[str, Any]] = []
    start = 0
    while True:
        i = text.find(key, start)
        if i < 0:
            break
        if " " in text[i : i + len(key)]:
            start = i + 1
            continue
        if is_whole_match(text, i, key):
            gi = index_map[i]
            if gi is None:
                start = i + 1
                continue
            left = nonspace_window(text, i, i + len(key), "left", NB_WINDOW)
            right = nonspace_window(text, i, i + len(key), "right", NB_WINDOW)
            g = stream[gi]
            hits.append(
                {
                    "text": key,
                    "left": left,
                    "right": right,
                    "x": g["x"],
                    "y": g["y"],
                    "font": g.get("font"),
                    "size_px": g.get("size_px"),
                    "transform": g.get("transform") or "identity",
                    "i": i,
                }
            )
        start = i + 1
    return hits


def pair_hits(
    reference: list[dict[str, Any]], actual: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """Pair by unique (text, neighborhood). Never take the first of many."""
    used_act: set[int] = set()
    pairs: list[dict[str, Any]] = []

    if len(reference) == 1 and len(actual) == 1:
        pairs.append(_pair(reference[0], actual[0], "unique_on_page"))
        return pairs

    # Unique neighborhood on both sides. Prefer the actual hit whose painted
    # size is closest to the reference when several neighborhoods still match
    # (Clio page-54 false L+16055 vs real Lucida on another page).
    for ri, ref in enumerate(reference):
        cand = [
            (ai, act)
            for ai, act in enumerate(actual)
            if ai not in used_act
            and neighbors_compatible(ref["left"], act["left"])
            and neighbors_compatible(ref["right"], act["right"])
        ]
        if not cand:
            continue
        if len(cand) > 1:
            ref_size = ref.get("size_px")
            if ref_size:
                cand.sort(
                    key=lambda item: abs((item[1].get("size_px") or ref_size) - ref_size)
                )
                best = abs((cand[0][1].get("size_px") or ref_size) - ref_size)
                cand = [
                    item
                    for item in cand
                    if abs((item[1].get("size_px") or ref_size) - ref_size) <= best + 1e-6
                ]
        if len(cand) != 1:
            continue
        ai, act = cand[0]
        # reverse uniqueness
        reverse = [
            rj
            for rj, other in enumerate(reference)
            if neighbors_compatible(other["left"], act["left"])
            and neighbors_compatible(other["right"], act["right"])
        ]
        if reverse != [ri]:
            continue
        used_act.add(ai)
        pairs.append(_pair(ref, act, "neighborhood"))

    paired_ref_idx = {id(p["_ref_obj"]) for p in pairs}

    for ref in reference:
        if id(ref) in paired_ref_idx:
            continue
        pairs.append(
            {
                "text": ref["text"],
                "left": ref["left"],
                "right": ref["right"],
                "ref_xy": [ref["x"], ref["y"]],
                "actual_xy": None,
                "font": {"reference": ref.get("font"), "actual": None},
                "transform": {
                    "reference": ref.get("transform") or "identity",
                    "actual": None,
                },
                "match": None,
                "status": "MISSING",
                "_ref_obj": ref,
            }
        )
    unused_actual = [act for ai, act in enumerate(actual) if ai not in used_act]
    if unused_actual and any(p["status"] == "MISSING" for p in pairs):
        # Remaining same-text hits are ambiguous, not first-match.
        for p in pairs:
            if p["status"] == "MISSING":
                p["status"] = "AMBIGUOUS"
                p["actual_xy"] = None
        for act in unused_actual:
            pairs.append(
                {
                    "text": act["text"],
                    "left": act["left"],
                    "right": act["right"],
                    "ref_xy": None,
                    "actual_xy": [act["x"], act["y"]],
                    "font": {"reference": None, "actual": act.get("font")},
                    "transform": {
                        "reference": None,
                        "actual": act.get("transform") or "identity",
                    },
                    "match": None,
                    "status": "AMBIGUOUS",
                    "_ref_obj": None,
                }
            )
    return pairs


def _pair(ref: dict[str, Any], act: dict[str, Any], match: str) -> dict[str, Any]:
    rx, ry = ref["x"], ref["y"]
    ax, ay = act["x"], act["y"]
    dx = None if ax is None or rx is None else ax - rx
    dy = None if ay is None or ry is None else ay - ry
    measured = ax is not None and ay is not None
    return {
        "text": ref["text"],
        "left": ref["left"],
        "right": ref["right"],
        "ref_xy": [rx, ry],
        "actual_xy": [ax, ay],
        "dx": dx,
        "dy": dy,
        "font": {"reference": ref.get("font"), "actual": act.get("font")},
        "transform": {
            "reference": ref.get("transform") or "identity",
            "actual": act.get("transform") or "identity",
        },
        "match": match,
        "status": "MEASURED" if measured else "MISSING",
        "svg_page": act.get("svg_page"),
        "_ref_obj": ref,
    }


def infer_role(text: str, y: float | None, left: str | None, right: str | None) -> str:
    blob = " ".join(x for x in (left, text, right) if x)
    if "Figure" in blob or (left is None and y is not None and y < 120):
        return "figure_label"
    if text in {"-M", "H", "A,C", "B,D"} or (text.isalpha() and len(text) <= 3):
        return "haplogroup_or_legend"
    if text.isdigit() or (text[:1].isalpha() and text[1:].isdigit()):
        return "snp_or_primer"
    return "body"


def evaluate_geometry(pairs: list[dict[str, Any]], tolerance: float = TOLERANCE_PX) -> dict[str, Any]:
    """Geometry gate: only MEASURED points may PASS; AMBIGUOUS is never PASS."""
    results = []
    all_ok = True
    for p in pairs:
        status = p["status"]
        row = {
            "text": p["text"],
            "status": status,
            "dx": p.get("dx"),
            "dy": p.get("dy"),
            "within_tolerance": False,
            "gate": "FAIL",
        }
        if status != "MEASURED":
            all_ok = False
            row["gate"] = "FAIL"
            results.append(row)
            continue
        dx, dy = p.get("dx"), p.get("dy")
        if dx is None or dy is None:
            all_ok = False
            row["gate"] = "FAIL"
            results.append(row)
            continue
        within = max(abs(dx), abs(dy)) <= tolerance
        row["within_tolerance"] = within
        row["gate"] = "PASS" if within else "FAIL"
        if not within:
            all_ok = False
        results.append(row)
    return {"pass": all_ok, "tolerance_px": tolerance, "points": results}


def measurability_ok(pairs: list[dict[str, Any]], required_keys: list[str]) -> bool:
    if not pairs:
        return False
    if any(p["status"] != "MEASURED" for p in pairs if p.get("ref_xy") is not None):
        return False
    measured = {p["text"] for p in pairs if p["status"] == "MEASURED"}
    return all(key in measured for key in required_keys)


def svg_glyphs(path: Path) -> list[Glyph]:
    tree = ET.parse(path)
    out: list[Glyph] = []
    for node in tree.iter():
        tag = node.tag.split("}")[-1]
        if tag != "text":
            continue
        text = node.text or ""
        xs = [float(x) for x in (node.get("x") or "0").split()]
        y = float(node.get("y") or "0")
        font = node.get("font-family")
        size_raw = node.get("font-size")
        size_px = float(size_raw.split()[0]) if size_raw else 12.0 * PT_TO_PX
        transform = node.get("transform") or "identity"
        for i, ch in enumerate(text):
            x = xs[i] if len(xs) == len(text) else (xs[0] if i == 0 else None)
            out.append(
                Glyph(ch=ch, x=x, y=y, font=font, size_px=size_px, transform=transform)
            )
    return out


def pdf_glyphs(path: Path) -> list[Glyph]:
    try:
        import pdfplumber
    except ImportError as exc:
        raise SystemExit("pdfplumber is required to read WPS reference PDFs") from exc
    out: list[Glyph] = []
    with pdfplumber.open(path) as pdf:
        page = pdf.pages[0]
        for char in page.chars:
            size_px = float(char.get("size") or 12.0) * PT_TO_PX
            matrix = char.get("matrix") or [1, 0, 0, 1, char.get("x0", 0), char.get("y0", 0)]
            x = matrix[4] * PT_TO_PX
            y = (page.height - matrix[5]) * PT_TO_PX
            font = char.get("fontname")
            transform = "identity"
            if matrix[:4] != [1, 0, 0, 1]:
                transform = ",".join(str(v) for v in matrix[:4])
            for ch in char["text"]:
                out.append(
                    Glyph(ch=ch, x=x, y=y, font=font, size_px=size_px, transform=transform)
                )
    return out


def build_page_ledger(
    page: int,
    keys: list[str],
    reference_glyphs: list[Glyph],
    actual_glyphs: list[Glyph],
    pdf_sha: str | None = None,
    svg_sha: str | None = None,
) -> dict[str, Any]:
    components: dict[str, Any] = {}
    all_pairs: list[dict[str, Any]] = []
    for key in keys:
        ref_hits = find_key_hits(reference_glyphs, key)
        act_hits = find_key_hits(actual_glyphs, key)
        if not ref_hits:
            pairs = [
                {
                    "text": key,
                    "left": None,
                    "right": None,
                    "ref_xy": None,
                    "actual_xy": None
                    if not act_hits
                    else [act_hits[0]["x"], act_hits[0]["y"]],
                    "dx": None,
                    "dy": None,
                    "font": {"reference": None, "actual": None},
                    "transform": {"reference": None, "actual": None},
                    "match": None,
                    "status": "MISSING",
                    "_ref_obj": None,
                }
            ]
        else:
            pairs = pair_hits(ref_hits, act_hits)
        measured_index = 0
        for pair in pairs:
            pair.pop("_ref_obj", None)
            if pair["status"] == "AMBIGUOUS" and pair.get("ref_xy") is None:
                cid = f"p{page}.{_cid(key)}.extra{measured_index}"
                measured_index += 1
            else:
                cid = f"p{page}.{_cid(key)}.{measured_index}"
                measured_index += 1
            y = None if pair.get("ref_xy") is None else pair["ref_xy"][1]
            pair["role"] = infer_role(key, y, pair.get("left"), pair.get("right"))
            pair["neighborhood"] = {"left": pair.get("left"), "right": pair.get("right")}
            components[cid] = pair
            all_pairs.append(pair)
    return {
        "pdf_sha256": pdf_sha,
        "svg_sha256": svg_sha,
        "components": components,
        "measurability_ok": measurability_ok(all_pairs, keys),
        "geometry": evaluate_geometry(all_pairs),
    }


def _cid(key: str) -> str:
    raw = key[1:] if key.startswith("-") else key
    prefix = "neg_" if key.startswith("-") else ""
    body = re.sub(r"[^A-Za-z0-9]+", "_", raw).strip("_") or "key"
    return prefix + body


def load_all_svg_glyphs(dest_svg: Path) -> dict[int, list[Glyph]]:
    """Load every rendered page. WPS and our page numbers can diverge."""
    out: dict[int, list[Glyph]] = {}
    if not dest_svg.is_dir():
        return out
    for path in sorted(dest_svg.glob("page-*.svg")):
        try:
            number = int(path.stem.split("-", 1)[1])
        except ValueError:
            continue
        out[number] = svg_glyphs(path)
    return out


def actual_hits_across_pages(
    svg_by_page: dict[int, list[Glyph]], key: str
) -> list[dict[str, Any]]:
    """All whole-key hits in every SVG page, tagged with svg_page."""
    hits: list[dict[str, Any]] = []
    for number in sorted(svg_by_page):
        for hit in find_key_hits(svg_by_page[number], key):
            tagged = dict(hit)
            tagged["svg_page"] = number
            hits.append(tagged)
    return hits


def build_clio_ledger(root: Path) -> dict[str, Any]:
    dest_svg = root / "target/remediation-2026-10-06/clio-svg"
    ref_dir = root / "docs/audit-2026-10-04/visual-thesis/wps-reference"
    doc = root / CLIO_DOC
    svg_by_page = load_all_svg_glyphs(dest_svg)
    pages: dict[str, Any] = {}
    for number, keys in CLIO_KEYS.items():
        pdfpath = ref_dir / f"page-{number}.pdf"
        # Same-number SVG is only a hint for hashes; matching searches all pages
        # because WPS and our section pagination disagree (Clio ~104 vs ~322).
        svgpath = dest_svg / f"page-{number}.svg"
        reference_glyphs = pdf_glyphs(pdfpath)
        components: dict[str, Any] = {}
        all_pairs: list[dict[str, Any]] = []
        for key in keys:
            ref_hits = find_key_hits(reference_glyphs, key)
            act_hits = actual_hits_across_pages(svg_by_page, key)
            if not ref_hits:
                pairs = [
                    {
                        "text": key,
                        "left": None,
                        "right": None,
                        "ref_xy": None,
                        "actual_xy": None
                        if not act_hits
                        else [act_hits[0]["x"], act_hits[0]["y"]],
                        "dx": None,
                        "dy": None,
                        "font": {"reference": None, "actual": None},
                        "transform": {"reference": None, "actual": None},
                        "match": None,
                        "status": "MISSING",
                        "svg_page": act_hits[0].get("svg_page") if act_hits else None,
                        "_ref_obj": None,
                    }
                ]
            else:
                pairs = pair_hits(ref_hits, act_hits)
            measured_index = 0
            for pair in pairs:
                pair.pop("_ref_obj", None)
                if "svg_page" not in pair:
                    pair["svg_page"] = None
                    if pair.get("actual_xy") is not None:
                        for act in act_hits:
                            if (
                                abs(act["x"] - pair["actual_xy"][0]) < 1e-6
                                and abs(act["y"] - pair["actual_xy"][1]) < 1e-6
                            ):
                                pair["svg_page"] = act.get("svg_page")
                                break
                if pair["status"] == "AMBIGUOUS" and pair.get("ref_xy") is None:
                    cid = f"p{number}.{_cid(key)}.extra{measured_index}"
                else:
                    cid = f"p{number}.{_cid(key)}.{measured_index}"
                measured_index += 1
                y = None if pair.get("ref_xy") is None else pair["ref_xy"][1]
                pair["role"] = infer_role(key, y, pair.get("left"), pair.get("right"))
                pair["neighborhood"] = {
                    "left": pair.get("left"),
                    "right": pair.get("right"),
                }
                components[cid] = pair
                all_pairs.append(pair)
        pages[str(number)] = {
            "pdf_sha256": sha256_file(pdfpath),
            "svg_sha256": sha256_file(svgpath) if svgpath.is_file() else None,
            "svg_pages_loaded": len(svg_by_page),
            "components": components,
            "measurability_ok": measurability_ok(all_pairs, keys),
            "geometry": evaluate_geometry(all_pairs),
        }
    required_measured = True
    for rec in pages.values():
        if not rec["measurability_ok"]:
            required_measured = False
    return {
        "document": CLIO_DOC.replace("\\", "/"),
        "source_sha256": sha256_file(doc) if doc.is_file() else None,
        "expected_source_sha256": CLIO_SHA256,
        "reference": "WPS, not Word",
        "coordinates": "absolute glyph origin, top-left, 96 dpi",
        "tolerance_px": TOLERANCE_PX,
        "pages": pages,
        "p0_measurability": "PASS" if required_measured else "FAIL",
        "p1_geometry": "PASS"
        if all(p["geometry"]["pass"] for p in pages.values())
        else "FAIL",
        "note": "AMBIGUOUS is never PASS. P0 closes on MEASURED; P1 on ≤0.25 px.",
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build the Clio WPS ledger")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="JSON output path (default: docs/audit-remediation-2026-10-06/wps-ledger.json)",
    )
    args = parser.parse_args(argv)
    out = args.out or (
        args.root / "docs/audit-remediation-2026-10-06/wps-ledger.json"
    )
    ledger = build_clio_ledger(args.root)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(ledger, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print("wrote", out)
    print("p0_measurability", ledger["p0_measurability"])
    print("p1_geometry", ledger["p1_geometry"])
    for page, rec in ledger["pages"].items():
        rows = [
            (cid, c["text"], c["status"], c.get("dx"), c.get("dy"))
            for cid, c in rec["components"].items()
        ]
        print("page", page, rows)
    return 0 if ledger["p0_measurability"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
