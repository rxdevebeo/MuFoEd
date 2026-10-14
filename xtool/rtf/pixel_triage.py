#!/usr/bin/env python3
"""Split the 97 pixel failures into provable, disjoint mechanism classes.

Reads the fresh per-page pixel receipt (with a per-page SSIM + structural
measurement) and the pagination receipt it was derived from, then classifies
every document that fails the pixel gate. The classifier is a pure function of
the two receipts (plus optional render logs for auxiliary font/image evidence),
so the result is reproducible and unit-testable.

The output is a triage, not a fix and not a support profile. A document class
proves the *gate mechanism* that rejected the document; the semantic owner
(which RC card, and when still open which RTF feature) is recorded separately
and marked a hypothesis until a minimal fixture confirms it.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

from census import write_json

SCHEMA = "strictlib.rtf-pixel-triage.v1"
CLAIM = "gate_mechanism_triage_not_semantic_root_cause_or_support_profile"
PASS = "Pass"
FAIL = "Fail"

# A page whose structure is `Ok` but whose SSIM is below threshold: the layout
# is right and the glyphs are not the reference glyphs (a substituted face).
GLYPH_APPEARANCE = "glyph_appearance"

# Page-level gate mechanisms, checked in the order the comparator reports them.
# `structural_fidelity_with` prints the *first* violated bound, so the first
# message fragment to match names the mechanism.
PAGE_KINDS = (
    ("reference ink", "blank_reference"),
    ("row-ink correlation", "row_correlation"),
    ("column-ink correlation", "column_correlation"),
    ("vertical shift", "vertical_shift"),
    ("horizontal shift", "horizontal_shift"),
    ("top ink edge", "extent_top"),
    ("bottom ink edge", "extent_bottom"),
    ("content centroid shift", "content_centroid"),
    ("centroid shift", "centroid"),
)

# Fine-grained page kinds folded into one document class. A document class is
# the coarse mechanism; the per-page kinds stay in the receipt as evidence.
CLASS_OF_PAGE = {
    "blank_reference": "blank_reference",
    "missing_ink": "missing_ink",
    "extra_ink": "extra_ink",
    GLYPH_APPEARANCE: "glyph_appearance",
    "row_correlation": "line_metrics",
    "vertical_shift": "line_metrics",
    "extent_top": "line_metrics",
    "extent_bottom": "line_metrics",
    "column_correlation": "horizontal_metrics",
    "horizontal_shift": "horizontal_metrics",
    "content_centroid": "content_position",
    "centroid": "content_position",
}

# Document classes, most specific first. The dominant metrics class is last.
CLASS_ORDER = (
    "missing_ink",
    "blank_reference",
    "extra_ink",
    "glyph_appearance",
    "content_position",
    "horizontal_metrics",
    "line_metrics",
)

OWNERS = {
    "pagination_page_count": "RC04 layout/metrics; RC03 for complex-script runs",
    "line_metrics": "RC04 line/paragraph metrics (semantic feature is a hypothesis)",
    "horizontal_metrics": "RC04 run-width metrics and tabs",
    "content_position": "RC04 paragraph geometry",
    "glyph_appearance": "RC02 substitute face / RC03 shaping",
    "missing_ink": "RC02/RC03 glyph coverage",
    "extra_ink": "RC06 borders/tables or RC07 image placeholder",
    "blank_reference": "gate blank-page handling; owner open until a trailing-page witness exists",
}

PROVEN = {
    "pagination_page_count": "confirmed: render page count differs from the WPS reference receipt",
    "line_metrics": "mechanism confirmed by the structural bound; semantic owner is a hypothesis",
    "horizontal_metrics": "mechanism confirmed by the structural bound; semantic owner is a hypothesis",
    "content_position": "mechanism confirmed by the structural bound; semantic owner is a hypothesis",
    "glyph_appearance": "confirmed: structure Ok but SSIM below threshold (glyph appearance differs)",
    "missing_ink": "confirmed: candidate ink collapsed while reference ink is present",
    "extra_ink": "confirmed: candidate ink more than twice the reference ink",
    "blank_reference": "confirmed: WPS reference page ink is below the gate's MIN_INK_FRACTION",
}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def page_kind(measurement):
    """Map one comparator line to a single page-level gate mechanism."""
    if "structure=Ok" in measurement:
        return GLYPH_APPEARANCE
    segment = measurement.split("\t", 1)[1] if "\t" in measurement else measurement
    match = re.search(r'structure=Err\("([^"]*)"', segment)
    message = match.group(1) if match else segment
    if "candidate ink" in message:
        return "missing_ink" if "<<" in message else "extra_ink"
    for needle, name in PAGE_KINDS:
        if needle in message:
            return name
    raise ValueError(f"unclassified page measurement: {measurement!r}")


def _pages(row):
    pages = row.get("pages") or []
    return pages if isinstance(pages, list) else [pages]


def _diagnostics(log_path):
    if not log_path.exists():
        return {}
    text = log_path.read_text(encoding="utf-8", errors="replace")
    return {
        "no_glyph": text.count("[pdf.font.no_glyph]"),
        "missing_face": text.count("[pdf.font.missing]"),
        "image_unsupported": text.count("[pdf.image.unsupported]"),
        "faces": sorted(set(re.findall(r"the bundled face for '([^']+)'", text))),
    }


def classify(pixels, visual, logs=None):
    """Return the triage model for a pixel + pagination receipt pair."""
    visual_by_name = {row["filename"]: row for row in visual["files"]}
    classes = {}
    page_mechanisms = {}
    files = []
    for row in pixels["files"]:
        name = row["filename"]
        pagination = visual_by_name[name]
        pages = _pages(row)
        diagnostics = _diagnostics(Path(logs) / (name[:3] + ".log")) if logs else {}
        entry = {
            "id": name[:3],
            "filename": name,
            "pixels": row["pixels"],
            "pagination": pagination["pagination"],
            "render_status": pagination.get("render_status"),
            "actual_pages": pagination.get("actual_pages"),
            "reference_pages": pagination.get("reference_pages"),
            "diagnostics": diagnostics,
        }
        if row["pixels"] == PASS:
            entry["primary"] = "pass_control"
            entry["page_kinds"] = {}
            entry["measured_pages"] = len(pages)
            entry["pass_pages"] = len(pages)
        elif not pages:
            entry["primary"] = "pagination_page_count"
            entry["page_count_delta"] = (pagination.get("actual_pages") or 0) - (
                pagination.get("reference_pages") or 0
            )
            entry["page_kinds"] = {}
        else:
            failing = [p for p in pages if p["verdict"] == FAIL]
            kinds = {}
            for page in failing:
                fine = page_kind(page["measurement"])
                kinds[fine] = kinds.get(fine, 0) + 1
            entry["page_kinds"] = kinds
            entry["measured_pages"] = len(pages)
            entry["pass_pages"] = len(pages) - len(failing)
            # The dominant failing mechanism is the document's primary class;
            # every page contributes to exactly one class counter below.
            per_class = {}
            for fine, count in kinds.items():
                cls = CLASS_OF_PAGE[fine]
                per_class[cls] = per_class.get(cls, 0) + count
                page_mechanisms[cls] = page_mechanisms.get(cls, 0) + count
            entry["primary"] = max(
                per_class, key=lambda c: (per_class[c], -CLASS_ORDER.index(c))
            )
            entry["secondary"] = sorted(
                (c for c in per_class if c != entry["primary"]),
                key=lambda c: (-per_class[c], CLASS_ORDER.index(c)),
            )
        files.append(entry)
        classes.setdefault(entry["primary"], {"documents": []})
        classes[entry["primary"]]["documents"].append(name)

    measured = sum(entry.get("measured_pages", 0) for entry in files)
    failing_pages = sum(page_mechanisms.values())
    failed = [entry for entry in files if entry["primary"] != "pass_control"]
    pagination = [entry for entry in failed if entry["primary"] == "pagination_page_count"]
    evidence = {
        "pagination_ours_more": sum(1 for e in pagination if e["page_count_delta"] > 0),
        "pagination_ours_fewer": sum(1 for e in pagination if e["page_count_delta"] < 0),
        "failed_with_glyph_loss": sum(1 for e in failed if e["diagnostics"].get("no_glyph", 0) > 0),
        "failed_with_image_loss": sum(1 for e in failed if e["diagnostics"].get("image_unsupported", 0) > 0),
        "failed_render_clean": sum(1 for e in failed if e["render_status"] == "Clean"),
    }
    summary = {
        "documents": len(files),
        "pixel_pass": len(classes.get("pass_control", {}).get("documents", [])),
        "pixel_fail": len(failed),
        "pagination_page_count": len(pagination),
        "matched_page_pixel": len(failed) - len(pagination),
        "measured_pages": measured,
        "passing_pages": measured - failing_pages,
        "failing_pages": failing_pages,
    }
    return {
        "claim": CLAIM,
        "summary": summary,
        "evidence": evidence,
        "classes": classes,
        "page_mechanisms": page_mechanisms,
        "owners": OWNERS,
        "proven": PROVEN,
        "class_order": list(CLASS_ORDER),
        "files": files,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pixels", type=Path, required=True,
                        help="directory holding the fresh per-page pixel results.json")
    parser.add_argument("--visual", type=Path, required=True,
                        help="pagination receipt directory the pixel run was derived from")
    parser.add_argument("--logs", type=Path, default=None,
                        help="optional render-log directory for font/image diagnostics")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    pixels_path = args.pixels / "results.json"
    visual_path = args.visual / "results.json"
    pixels = json.loads(pixels_path.read_text(encoding="utf-8"))
    visual = json.loads(visual_path.read_text(encoding="utf-8"))
    if sha(visual_path) != pixels["pagination_receipt_sha256"]:
        raise SystemExit("pixel receipt was not derived from the given pagination receipt")

    model = classify(pixels, visual, args.logs)
    model.update({
        "schema": SCHEMA,
        "dpi": pixels["dpi"],
        "pixels_receipt_sha256": sha(pixels_path),
        "pagination_receipt_sha256": sha(visual_path),
        "comparison_binary_sha256": pixels["comparison_binary_sha256"],
    })
    args.out.mkdir(parents=True, exist_ok=True)
    write_json(args.out / "triage.json", model)
    s = model["summary"]
    print(f"documents={s['documents']} pass={s['pixel_pass']} fail={s['pixel_fail']} "
          f"pagination={s['pagination_page_count']} matched={s['matched_page_pixel']} "
          f"pages={s['measured_pages']} passing={s['passing_pages']} failing={s['failing_pages']}")
    print("documents by primary class:")
    for name in model["class_order"]:
        group = model["classes"].get(name)
        if group:
            print(f"  {name}: {len(group['documents'])} documents")
    print("pages by mechanism:")
    for name, count in sorted(model["page_mechanisms"].items(),
                              key=lambda kv: -kv[1]):
        print(f"  {name}: {count} pages")
    print("evidence:", model["evidence"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
