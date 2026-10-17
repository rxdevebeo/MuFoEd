#!/usr/bin/env python3
"""Overlap metric: ink that sits on other ink where it should not.

Where `metrics.py` answers "how close is our PDF to WPS's", this answers a
different question about one PDF on its own: **where does its ink land on other
ink?** — text over text, an image over text, an image over an image, or ink
outside the page — the overlaps nobody asked for.

Every item carries a bounding box in points, top-down (the same convention as
`metrics.Word`): text words from the text layer, images and vector paths from
the page's objects (`get_bounds`), so the boxes are the real ink, not a guess.
Pairs are classified by their kinds and reported with the intersection
rectangle and the share of the smaller box it covers; a document is summarised
by class.

It is a QA instrument, like the rest of the harness: it measures and reports,
it never decides pass/fail. Run it over `refs` (WPS) and `ours` to see which
overlaps are ours and which the reference has too.

    python xtool/wps-qa/overlap.py find <pdf> [page…]
    python xtool/wps-qa/overlap.py report --label L [--corpus C …] [--side ours|ref|both]
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import metrics  # noqa: E402
import wpsqa_common as common  # noqa: E402
from wpsqa_common import Doc  # noqa: E402

try:
    import pypdfium2 as pdfium
    from pypdfium2 import raw as pdfium_raw
except ImportError:  # pragma: no cover - the harness requires pdfium
    pdfium = None

# A pair counts as an overlap only when the smaller box is covered by at least
# this share. Below it, adjacent words that share a few tenths of a point of
# their em box are touching, not overlapping.
MIN_RATIO = 0.15
# Below this many square points the intersection is glyph-box noise.
MIN_AREA_PT2 = 1.0
# Grid cell for the sweep, in points. Bodies of text are dense; comparing every
# pair is quadratic, and a bucket sweep is linear in practice.
CELL_PT = 48.0

OBJ_TEXT = 1
OBJ_PATH = 2
OBJ_IMAGE = 3
OBJ_FORM = 5

# Classes, in report order.
CLASSES = (
    "text-text",
    "text-over-lines",
    "text-image",
    "image-image",
    "text-image-bg",
    "text-path",
    "image-path",
    "off-page-text",
    "off-page-image",
)


@dataclass
class Item:
    kind: str  # "text" | "image" | "path"
    page: int
    x0: float
    y0: float  # top
    x1: float
    y1: float  # bottom
    order: int  # paint order within the page (0 = first drawn)
    text: str = ""

    @property
    def area(self) -> float:
        return max(0.0, self.x1 - self.x0) * max(0.0, self.y1 - self.y0)


def _overlap(a: Item, b: Item) -> tuple[float, tuple[float, float, float, float]]:
    left, top = max(a.x0, b.x0), max(a.y0, b.y0)
    right, bottom = min(a.x1, b.x1), min(a.y1, b.y1)
    if right <= left or bottom <= top:
        return 0.0, (0.0, 0.0, 0.0, 0.0)
    inter = (right - left) * (bottom - top)
    smaller = min(a.area, b.area)
    ratio = inter / smaller if smaller > 0 else 0.0
    return (inter, (left, top, right, bottom)) if ratio > 0 else (0.0, (0.0, 0.0, 0.0, 0.0))


def _classify(a: Item, b: Item, page_area: float) -> str:
    kinds = {a.kind, b.kind}
    if kinds == {"text"}:
        return "text-text"
    if kinds == {"image"}:
        return "image-image"
    if kinds == {"text", "image"}:
        image = a if a.kind == "image" else b
        # An image that covers most of the page is a background/letterhead: text
        # over it is expected, and kept apart from a stray image over body text.
        if page_area > 0 and image.area >= 0.4 * page_area:
            return "text-image-bg"
        return "text-image"
    if kinds == {"text", "path"}:
        return "text-path"
    if kinds == {"image", "path"}:
        return "image-path"
    return "text-text" if a.kind == "text" else "image-image"


def _buckets(items: list[Item]) -> dict[tuple[int, int], list[int]]:
    buckets: dict[tuple[int, int], list[int]] = defaultdict(list)
    for index, item in enumerate(items):
        cx, cy = int(item.x0 // CELL_PT), int(item.y0 // CELL_PT)
        buckets[cx, cy].append(index)
    return buckets


def _candidate_pairs(items: list[Item]):
    """Index pairs whose boxes could overlap (same or neighbouring grid cell)."""
    buckets = _buckets(items)
    seen: set[tuple[int, int]] = set()
    for (cx, cy), members in buckets.items():
        # Compare within the bucket and the four already-visited neighbours.
        for dx, dy in ((0, 0), (1, 0), (0, 1), (1, 1), (1, -1)):
            other = buckets.get((cx + dx, cy + dy))
            if not other:
                continue
            if (dx, dy) == (0, 0):
                for ii in range(len(members)):
                    for jj in range(ii + 1, len(members)):
                        a, b = members[ii], members[jj]
                        seen.add((a, b) if a < b else (b, a))
            else:
                for a in members:
                    for b in other:
                        seen.add((a, b) if a < b else (b, a))
    yield from seen


def page_items(
    pdf_path: str, page_index: int, with_paths: bool, words_by_page: dict[int, list]
) -> tuple[tuple[float, float], list[Item]]:
    """Every boxed item of one page: text words, images and (optionally) paths."""
    doc = pdfium.PdfDocument(pdf_path)
    try:
        page = doc[page_index]
        width, height = page.get_size()
        items: list[Item] = []
        for word in words_by_page.get(page_index, ()):
            items.append(Item("text", page_index, word.x0, word.top, word.x1, word.bottom, -1, word.text))
        order = 0
        for obj in page.get_objects(max_depth=3):
            kind = {OBJ_IMAGE: "image", OBJ_PATH: "path", OBJ_TEXT: None}.get(obj.type)
            if kind is None:
                continue
            if kind == "path" and not with_paths:
                continue
            try:
                left, bottom, right, top = obj.get_bounds()
            except Exception:  # noqa: BLE001 - a degenerate object has no box
                continue
            items.append(Item(kind, page_index, left, height - top, right, height - bottom, order))
            order += 1
        page.close()
        return (width, height), items
    finally:
        doc.close()


def _read_words(pdf_path: str, max_pages: int) -> dict[int, list]:
    """Text words per page, through the harness's own decoder, bounded to the
    pages the run analyzes (a 5000-page document must not read all of itself)."""
    doc = pdfium.PdfDocument(pdf_path)
    try:
        pages = len(doc)
        limit = pages if max_pages <= 0 else min(pages, max_pages)
        by_page: dict[int, list] = {}
        for index in range(limit):
            page = doc[index]
            height = page.get_size()[1]
            textpage = page.get_textpage()
            chars: list[str] = []
            boxes: list = []
            for i in range(textpage.count_chars()):
                code = pdfium_raw.FPDFText_GetUnicode(textpage.raw, i)
                generated = pdfium_raw.FPDFText_IsGenerated(textpage.raw, i) == 1
                ch = chr(code) if 0 < code < 0x110000 and not (0xD800 <= code < 0xE000) else " "
                if generated or ch in "\r\n\t\u00ad":
                    ch = " " if ch != "\u00ad" else ""
                if ch == "":
                    continue
                chars.append(ch)
                if ch.isspace():
                    boxes.append(None)
                else:
                    left, bottom, right, top = textpage.get_charbox(i)
                    boxes.append((left, height - top, right, height - bottom))
            joined = "".join(chars)
            words: list = []
            for match in metrics.WORD_RE.finditer(joined):
                span = [b for b in boxes[match.start() : match.end()] if b is not None]
                if not span:
                    continue
                words.append(
                    metrics.Word(
                        metrics._norm(match.group()),
                        index,
                        min(b[0] for b in span),
                        min(b[1] for b in span),
                        max(b[2] for b in span),
                        max(b[3] for b in span),
                    )
                )
            metrics._assign_lines(words)
            by_page[index] = words
            textpage.close()
            page.close()
        return by_page
    finally:
        doc.close()


def analyze_page(
    pdf_path: str, page_index: int, with_paths: bool, min_ratio: float, words_by_page: dict[int, list]
) -> dict:
    (width, height), items = page_items(pdf_path, page_index, with_paths, words_by_page)
    page_area = width * height
    # Deduplicate text words that pdfium reports twice for one glyph box: keep
    # the first of identical boxes so a doubled word is one overlap, not four.
    overlaps: list[dict] = []
    for i, j in _candidate_pairs(items):
        a, b = items[i], items[j]
        inter, rect = _overlap(a, b)
        if inter < MIN_AREA_PT2:
            continue
        smaller = min(a.area, b.area)
        ratio = inter / smaller if smaller > 0 else 0.0
        if ratio < min_ratio:
            continue
        overlaps.append(
            {
                "class": _refine(a, b, _classify(a, b, page_area)),
                "ratio": round(ratio, 3),
                "area": round(inter, 2),
                "rect": [round(v, 2) for v in rect],
                "a": _item_ref(a),
                "b": _item_ref(b),
            }
        )
    off_page = []
    tol = 1.0
    for item in items:
        if item.x0 < -tol or item.y0 < -tol or item.x1 > width + tol or item.y1 > height + tol:
            off_page.append({"class": f"off-page-{item.kind}", **_item_ref(item)})
    return {
        "page": page_index + 1,
        "size": [round(width, 1), round(height, 1)],
        "overlaps": overlaps,
        "off_page": off_page,
    }


def _item_ref(item: Item) -> dict:
    ref = {
        "kind": item.kind,
        "box": [round(item.x0, 2), round(item.y0, 2), round(item.x1, 2), round(item.y1, 2)],
        "order": item.order,
    }
    if item.text:
        ref["text"] = item.text[:40]
    return ref


def _refine(a: Item, b: Item, klass: str) -> str:
    """Split "text-text" by baseline: same line is overprint, another line is
    colliding lines (negative or near-zero leading), which needs its own fix."""
    if klass != "text-text":
        return klass
    height = min(a.y1 - a.y0, b.y1 - b.y0)
    if height > 0 and abs(a.y1 - b.y1) > 0.35 * height:
        return "text-over-lines"
    return "text-text"


def analyze(pdf_path: str, with_paths: bool = False, min_ratio: float = MIN_RATIO, max_pages: int = 0) -> dict:
    """Findings of a whole PDF: per page, plus a rollup by class."""
    doc = pdfium.PdfDocument(pdf_path)
    try:
        pages = len(doc)
    finally:
        doc.close()
    limit = pages if max_pages <= 0 else min(pages, max_pages)
    words_by_page = _read_words(pdf_path, max_pages)
    by_class: Counter = Counter()
    docs = []
    for index in range(limit):
        page = analyze_page(pdf_path, index, with_paths, min_ratio, words_by_page)
        for overlap in page["overlaps"]:
            by_class[overlap["class"]] += 1
        for item in page["off_page"]:
            by_class[item["class"]] += 1
        if page["overlaps"] or page["off_page"]:
            docs.append(page)
    return {"pdf": pdf_path, "pages": pages, "analyzed_pages": limit, "by_class": dict(by_class), "findings": docs}


def overlay(pdf_path: str, page_index: int, findings: dict, out_path: str, dpi: int = 96) -> str:
    """The page with each overlap outlined; the same picture the report links."""
    import numpy as np
    from PIL import Image, ImageDraw

    gray = metrics.rasterize(pdf_path, page_index, dpi)
    rgb = np.repeat((np.clip(gray, 0, 1) * 255).astype(np.uint8)[..., None], 3, axis=-1)
    image = Image.fromarray(rgb)
    scale = dpi / 72.0
    draw = ImageDraw.Draw(image)
    colors = {
        "text-text": (220, 30, 30),
        "text-over-lines": (230, 0, 120),
        "text-image": (200, 0, 200),
        "image-image": (230, 130, 0),
        "text-image-bg": (120, 120, 120),
        "text-path": (0, 150, 90),
        "image-path": (150, 90, 0),
    }
    for overlap in findings.get("overlaps", []):
        x0, y0, x1, y1 = (v * scale for v in overlap["rect"])
        draw.rectangle([x0, y0, x1, y1], outline=colors.get(overlap["class"], (0, 0, 0)), width=3)
    for item in findings.get("off_page", []):
        x0, y0, x1, y1 = (v * scale for v in item["box"])
        draw.rectangle([x0, y0, x1, y1], outline=(30, 80, 220), width=3)
    Path(out_path).parent.mkdir(parents=True, exist_ok=True)
    image.save(out_path, optimize=True)
    return out_path


# ---------------------------------------------------------------------------
# report: aggregate a label's store by class
# ---------------------------------------------------------------------------


def _pdf_for(store: Path, side: str, label: str, doc: Doc) -> Path | None:
    if side == "ref":
        path = store / "refs" / doc.corpus / f"{doc.key}.pdf"
    else:
        path = store / "ours" / label / doc.corpus / f"{doc.key}.pdf"
    return path if path.is_file() else None


def _worst_pages(findings: dict, limit: int) -> list[dict]:
    scored = sorted(
        findings.get("findings", []),
        key=lambda page: -sum(o["ratio"] for o in page["overlaps"]),
    )
    return scored[:limit]


def cmd_report(args) -> int:
    store: Path = args.store
    out_root = store / "overlap" / args.label
    docs = list(common.iter_docs(args.data_root, args.corpus))
    if args.limit:
        docs = docs[: args.limit]
    sides = ["ours", "ref"] if args.side == "both" else [args.side]

    rows: list[dict] = []
    totals: dict[str, dict[str, dict[str, int]]] = {}  # side -> corpus -> class -> count
    offenders: dict[str, dict] = {}  # side -> list of per-doc summaries
    for side in sides:
        totals[side] = {}
        offenders[side] = []
        for n, doc in enumerate(docs, 1):
            pdf = _pdf_for(store, side, args.label, doc)
            if pdf is None:
                continue
            try:
                result = analyze(str(pdf), args.with_paths, args.min_ratio, args.max_pages)
            except Exception as exc:  # noqa: BLE001
                print(f"overlap[{side}]: {doc.ident}: {type(exc).__name__}: {exc}", flush=True)
                continue
            counts = result["by_class"]
            worst = 0.0
            pages = []
            for page in result["findings"]:
                if page["overlaps"]:
                    worst = max(worst, max(o["ratio"] for o in page["overlaps"]))
                    pages.append(page["page"])
            corpus_totals = totals[side].setdefault(doc.corpus, defaultdict(int))
            for klass, count in counts.items():
                corpus_totals[klass] += count
                rows.append(
                    {
                        "side": side,
                        "corpus": doc.corpus,
                        "key": doc.key,
                        "class": klass,
                        "count": count,
                        "worst_ratio": round(worst, 3),
                        "pages": " ".join(str(p) for p in pages[:20]),
                    }
                )
            if counts:
                offenders[side].append(
                    {
                        "corpus": doc.corpus,
                        "key": doc.key,
                        "total": sum(counts.values()),
                        "by_class": counts,
                        "worst_ratio": round(worst, 3),
                        "pages": pages,
                        "pdf": str(pdf),
                    }
                )
            # Overlay the worst pages of the documents that have findings.
            if counts and not args.no_images:
                for page in _worst_pages(result, args.images_per_doc):
                    out = out_root / side / "img" / doc.corpus / f"{doc.key}-p{page['page']}.png"
                    overlay(str(pdf), page["page"] - 1, page, str(out))
            if args.verbose and (counts or n % 50 == 0):
                print(f"overlap[{side}] [{n}/{len(docs)}] {doc.ident} {counts}", flush=True)

    (out_root).mkdir(parents=True, exist_ok=True)
    with (out_root / f"results-{args.label}.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["side", "corpus", "key", "class", "count", "worst_ratio", "pages"])
        writer.writeheader()
        writer.writerows(rows)

    summary = {
        "label": args.label,
        "side": args.side,
        "min_ratio": args.min_ratio,
        "with_paths": args.with_paths,
        "totals": {side: {c: dict(v) for c, v in cs.items()} for side, cs in totals.items()},
        "documents": {side: sorted(offenders[side], key=lambda d: -d["total"])[:args.top] for side in sides},
    }
    common.write_json(out_root / "summary.json", summary)
    report_path = out_root / "report.md"
    write_markdown(report_path, summary, sides)
    print(f"overlap[{args.label}]: {report_path}")
    print(f"  overlays: {out_root / '<side>' / 'img'}")
    for side in sides:
        grand = Counter()
        for corpus_counts in totals[side].values():
            grand.update(corpus_counts)
        print(f"  {side}: {dict(grand)}")
    return 0


def write_markdown(path: Path, summary: dict, sides: list[str]) -> None:
    lines = [f"# Overlap report — {summary['label']}", ""]
    lines.append(f"min_ratio={summary['min_ratio']} with_paths={summary['with_paths']}")
    lines.append("")
    for side in sides:
        lines.append(f"## side: {side}")
        lines.append("")
        grand = Counter()
        for corpus_counts in summary["totals"][side].values():
            grand.update(corpus_counts)
        lines.append("### totals by class")
        lines.append("")
        lines.append("| class | count |")
        lines.append("|--|--|")
        for klass, count in grand.most_common():
            lines.append(f"| {klass} | {count} |")
        lines.append("")
        lines.append("### by corpus")
        lines.append("")
        all_classes = [k for k, _ in grand.most_common()]
        lines.append("| corpus | " + " | ".join(all_classes) + " |")
        lines.append("|--" * (len(all_classes) + 1) + "|")
        for corpus, counts in sorted(summary["totals"][side].items()):
            cells = " | ".join(str(counts.get(k, 0)) for k in all_classes)
            lines.append(f"| {corpus} | {cells} |")
        lines.append("")
        lines.append("### worst documents")
        lines.append("")
        lines.append("| document | total | worst | classes |")
        lines.append("|--|--|--|--|")
        for doc in summary["documents"][side]:
            classes = ", ".join(f"{k}:{v}" for k, v in sorted(doc["by_class"].items(), key=lambda kv: -kv[1]))
            lines.append(f"| `{doc['corpus']}/{doc['key'][:48]}` | {doc['total']} | {doc['worst_ratio']} | {classes} |")
        lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def cmd_find(args) -> int:
    result = analyze(args.pdf, args.with_paths, args.min_ratio, args.max_pages)
    if args.pages:
        wanted = set(args.pages)
        result["findings"] = [p for p in result["findings"] if p["page"] in wanted]
    print(json.dumps(result, ensure_ascii=False, indent=1))
    return 0


def main(argv: list[str] | None = None) -> int:
    # Document names carry any script; a redirected stdout would otherwise raise
    # on the first non-ASCII ident under a legacy console encoding.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_find = sub.add_parser("find", help="analyze one or more pages of one PDF")
    p_find.add_argument("pdf")
    p_find.add_argument("pages", type=int, nargs="*", help="1-based pages (default: all)")
    p_find.add_argument("--with-paths", action="store_true")
    p_find.add_argument("--min-ratio", type=float, default=MIN_RATIO)
    p_find.add_argument("--max-pages", type=int, default=30)

    for name, fn in (("report", cmd_report),):
        p = sub.add_parser(name)
        p.set_defaults(fn=fn)
        p.add_argument("--store", type=Path, default=common.default_store())
        p.add_argument("--data-root", type=Path, default=common.main_copy_root())
        p.add_argument("--label", required=True, help="our render label (store/ours/<label>)")
        p.add_argument("--side", choices=["ours", "ref", "both"], default="ours")
        p.add_argument("--corpus", action="append")
        p.add_argument("--limit", type=int, default=0)
        p.add_argument("--min-ratio", type=float, default=MIN_RATIO)
        p.add_argument("--with-paths", action="store_true")
        p.add_argument("--max-pages", type=int, default=0, help="pages per document (0 = all)")
        p.add_argument("--top", type=int, default=40)
        p.add_argument("--images-per-doc", type=int, default=2)
        p.add_argument("--no-images", action="store_true")
        p.add_argument("--verbose", action="store_true")

    p_find.set_defaults(fn=cmd_find)
    args = parser.parse_args(argv)
    if hasattr(args, "store"):
        args.store = args.store.resolve()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
