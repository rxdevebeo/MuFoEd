#!/usr/bin/env python3
"""P0 WPS matcher selftests: uniqueness, split runs, statuses, 1 px fail."""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import wps_ledger as wl  # noqa: E402


def G(ch: str, x: float, y: float, font: str = "Test", size: float = 16.0) -> wl.Glyph:
    return wl.Glyph(ch=ch, x=x, y=y, font=font, size_px=size, transform="identity")


def glyphs(text: str, x0: float, y: float, adv: float = 8.0) -> list[wl.Glyph]:
    return [G(ch, x0 + i * adv, y) for i, ch in enumerate(text)]


def test_split_runs_glue() -> None:
    # 'L' then '15996' as separate SVG runs on the same baseline.
    stream = glyphs("L", 100, 40, 8) + glyphs("15996", 108, 40, 7)
    hits = wl.find_key_hits(stream, "L15996")
    if len(hits) != 1:
        raise SystemExit(f"T-P0-2 split runs: expected 1 hit, got {hits}")
    if abs(hits[0]["x"] - 100) > 1e-6:
        raise SystemExit("T-P0-2 split runs: origin must be the first glyph")


def test_substring_first_forbidden() -> None:
    # Page stream contains HV and standalone H. Key H must not take HV.
    hv = glyphs("HV", 10, 20, 8)
    h = glyphs("H", 10, 50, 8)
    hits = wl.find_key_hits(hv + [G(" ", 30, 20)] + h, "H")
    if len(hits) != 1 or abs(hits[0]["y"] - 50) > 1e-6:
        raise SystemExit(f"T-P0-2 substring-first leaked: {hits}")


def test_neighborhood_unique() -> None:
    ref = (
        glyphs("foo", 0, 10, 8)
        + [G(" ", 30, 10)]
        + glyphs("HVR-I", 40, 10, 8)
        + [G(" ", 90, 10)]
        + glyphs("bar", 100, 10, 8)
        + glyphs("foo", 0, 40, 8)
        + [G(" ", 30, 40)]
        + glyphs("HVR-I", 40, 40, 8)
        + [G(" ", 90, 40)]
        + glyphs("baz", 100, 40, 8)
    )
    actual = (
        glyphs("foo", 1, 11, 8)
        + [G(" ", 31, 11)]
        + glyphs("HVR-I", 41, 11, 8)
        + [G(" ", 91, 11)]
        + glyphs("bar", 101, 11, 8)
        + glyphs("foo", 1, 41, 8)
        + [G(" ", 31, 41)]
        + glyphs("HVR-I", 41, 41, 8)
        + [G(" ", 91, 41)]
        + glyphs("baz", 101, 41, 8)
    )
    page = wl.build_page_ledger(1, ["HVR-I"], ref, actual)
    comps = list(page["components"].values())
    if len(comps) != 2 or any(c["status"] != "MEASURED" for c in comps):
        raise SystemExit(f"T-P0-2 neighborhood: {comps}")
    ys = sorted(c["ref_xy"][1] for c in comps)
    if ys != [10, 40]:
        raise SystemExit(f"T-P0-2 neighborhood y {ys}")


def test_ambiguous_is_not_pass() -> None:
    def line(y: float) -> list[wl.Glyph]:
        return (
            glyphs("zzzzzzzzzzzz", 0, y, 8)
            + [G(" ", 96, y)]
            + glyphs("HVR-I", 110, y, 8)
            + [G(" ", 160, y)]
            + glyphs("qqqqqqqqqqqq", 170, y, 8)
        )

    stream = line(10) + line(30)
    page = wl.build_page_ledger(2, ["HVR-I"], stream, stream)
    statuses = {c["status"] for c in page["components"].values()}
    if statuses != {"AMBIGUOUS"}:
        raise SystemExit(f"T-P0-3 expected AMBIGUOUS, got {page['components']}")
    geom = page["geometry"]
    if geom["pass"]:
        raise SystemExit("T-P0-3 AMBIGUOUS must not PASS the geometry gate")
    if not all(p["gate"] == "FAIL" for p in geom["points"]):
        raise SystemExit("T-P0-3 each AMBIGUOUS point must FAIL")


def test_missing_status() -> None:
    ref = glyphs("L16055", 10, 10, 8)
    actual = glyphs("other", 10, 10, 8)
    page = wl.build_page_ledger(3, ["L16055"], ref, actual)
    statuses = {c["status"] for c in page["components"].values()}
    if "MISSING" not in statuses and "AMBIGUOUS" not in statuses:
        raise SystemExit(f"T-P0-3 expected MISSING, got {page['components']}")
    if page["geometry"]["pass"]:
        raise SystemExit("T-P0-3 MISSING must not PASS")


def test_one_px_shift_fails() -> None:
    ref = glyphs("L16055", 200.0, 150.0, 8)
    actual = glyphs("L16055", 201.0, 150.0, 8)
    page = wl.build_page_ledger(4, ["L16055"], ref, actual)
    comps = list(page["components"].values())
    if len(comps) != 1 or comps[0]["status"] != "MEASURED":
        raise SystemExit(f"T-P0-4 should still be MEASURED: {comps}")
    if abs(comps[0]["dx"] - 1.0) > 1e-9:
        raise SystemExit(f"T-P0-4 dx {comps[0]['dx']}")
    if page["geometry"]["pass"] or comps[0]["dx"] <= wl.TOLERANCE_PX:
        raise SystemExit("T-P0-4 1 px shift must FAIL the 0.25 px gate")
    within = page["geometry"]["points"][0]["within_tolerance"]
    if within:
        raise SystemExit("T-P0-4 within_tolerance must be false")


def test_comma_key_is_whole_token() -> None:
    stream = glyphs("A,C", 120.0, 100.0, 8) + [G(":", 144.0, 100.0)] + glyphs("all", 152.0, 100.0, 8)
    hits = wl.find_key_hits(stream, "A,C")
    if len(hits) != 1:
        raise SystemExit(f"T-P0-2 A,C token: {hits}")
    ref = glyphs("A,C", 100.0, 80.0, 8)
    actual = glyphs("A,C", 100.1, 80.05, 8)
    page = wl.build_page_ledger(5, ["A,C"], ref, actual)
    if not page["geometry"]["pass"]:
        raise SystemExit(f"T-P0-3 in-tolerance MEASURED must PASS: {page}")


def main() -> None:
    test_split_runs_glue()
    test_substring_first_forbidden()
    test_neighborhood_unique()
    test_ambiguous_is_not_pass()
    test_missing_status()
    test_one_px_shift_fails()
    test_comma_key_is_whole_token()
    print("wps_ledger_selftest: PASS")


if __name__ == "__main__":
    main()
