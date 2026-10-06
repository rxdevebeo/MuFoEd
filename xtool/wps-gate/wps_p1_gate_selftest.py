#!/usr/bin/env python3
"""P1 WPS geometry gate selftests: page bounds + forced layout offset."""

from __future__ import annotations

import copy
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import wps_ledger as wl  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / "docs/audit-remediation-2026-10-06/wps-ledger.json"
SVG_DIR = ROOT / "target/remediation-2026-10-06/clio-svg"
REF_DIR = ROOT / "docs/audit-2026-10-04/visual-thesis/wps-reference"
PAGE_W = 793.733
PAGE_H = 1122.533


def test_p1_geometry_pass() -> None:
    ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
    if ledger.get("p1_geometry") != "PASS":
        raise SystemExit(f"T-P1-1: expected p1_geometry PASS, got {ledger.get('p1_geometry')}")
    if ledger.get("p0_measurability") != "PASS":
        raise SystemExit("T-P1-1: p0_measurability must stay PASS")


def test_captions_inside_page_bounds() -> None:
    """T-P1-2: every mandatory actual origin lies inside the SVG page box."""
    ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
    for page, rec in ledger["pages"].items():
        for cid, c in rec["components"].items():
            if c.get("status") != "MEASURED" or not c.get("actual_xy"):
                continue
            x, y = c["actual_xy"]
            if not (0.0 <= x <= PAGE_W and 0.0 <= y <= PAGE_H):
                raise SystemExit(
                    f"T-P1-2: {cid} origin ({x}, {y}) outside page {PAGE_W}x{PAGE_H}"
                )


def test_layout_offset_fails_geometry_gate() -> None:
    """T-P1-4: shifting every SVG glyph +1 px in x keeps MEASURED but fails 0.25."""
    page = 56
    pdf = wl.pdf_glyphs(REF_DIR / f"page-{page}.pdf")
    svg = wl.svg_glyphs(SVG_DIR / f"page-{page}.svg")
    shifted = []
    for g in svg:
        g2 = copy.copy(g)
        if g2.get("x") is not None:
            g2["x"] = g2["x"] + 1.0
        shifted.append(g2)
    keys = wl.CLIO_KEYS[page]
    rec = wl.build_page_ledger(page, keys, pdf, shifted)
    if not rec["measurability_ok"]:
        raise SystemExit(f"T-P1-4: offset must stay MEASURED, got {rec}")
    if rec["geometry"]["pass"]:
        raise SystemExit("T-P1-4: +1 px layout offset must FAIL the 0.25 px gate")
    failed = [p for p in rec["geometry"]["points"] if p["gate"] == "FAIL"]
    if not failed:
        raise SystemExit("T-P1-4: expected at least one FAIL point after offset")


def test_thin_margin_is_documented() -> None:
    """Guard: tightest mandatory margin must stay non-negative (≤0.25)."""
    ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
    tightest = None
    for rec in ledger["pages"].values():
        for cid, c in rec["components"].items():
            if c.get("status") != "MEASURED":
                continue
            m = max(abs(c.get("dx") or 0), abs(c.get("dy") or 0))
            margin = wl.TOLERANCE_PX - m
            if tightest is None or margin < tightest[0]:
                tightest = (margin, cid, m)
    if tightest is None or tightest[0] < 0:
        raise SystemExit(f"T-P1-1 margin: {tightest}")
    # Fragility signal for receipt notes (modern.5 historically ~0.002).
    print(f"T-P1 margin: tightest={tightest[0]:.6f} px at {tightest[1]} (abs={tightest[2]:.4f})")


def main() -> None:
    if not LEDGER.is_file():
        raise SystemExit(f"missing ledger {LEDGER}")
    if not (SVG_DIR / "page-56.svg").is_file():
        raise SystemExit(f"missing SVG under {SVG_DIR}; render Clio pages 54-104 first")
    test_p1_geometry_pass()
    test_captions_inside_page_bounds()
    test_layout_offset_fails_geometry_gate()
    test_thin_margin_is_documented()
    print("wps_p1_gate_selftest: PASS")


if __name__ == "__main__":
    main()
