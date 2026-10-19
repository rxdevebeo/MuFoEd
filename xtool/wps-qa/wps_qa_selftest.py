#!/usr/bin/env python3
"""Self-test of the WPS QA metrics on synthetic words (no WPS, no PDFs)."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402

import metrics  # noqa: E402
from metrics import PdfFacts, Word  # noqa: E402
import overlap as overlap_metric  # noqa: E402
import wpsqa_common  # noqa: E402
from wpsqa_common import safe_key  # noqa: E402


def layout(text: str, per_line: int, lines_per_page: int, adv: float = 30.0, lead: float = 14.0, x_shift: float = 0.0) -> PdfFacts:
    words = []
    for n, t in enumerate(text.split()):
        line_no = n // per_line
        page, line = divmod(line_no, lines_per_page)
        x = 72 + x_shift + (n % per_line) * adv
        base = 72 + line * lead
        words.append(Word(t, page, x, base - 10, x + 20, base))
    metrics._assign_lines(words)
    pages = (words[-1].page + 1) if words else 1
    return PdfFacts(pages, [(612.0, 792.0)] * pages, words, sum(len(w.text) for w in words))


TEXT = " ".join(f"w{i}" for i in range(400))
checks: list[tuple[str, bool]] = []


def check(name: str, ok: bool) -> None:
    checks.append((name, ok))


same = metrics.compare(layout(TEXT, 10, 20), layout(TEXT, 10, 20), with_raster=False)
check("identical layout scores 100", same["score"] == 100.0)

# Different wrapping: same text, 9 words per line instead of 10.
wrap = metrics.compare(layout(TEXT, 10, 20), layout(TEXT, 9, 20), with_raster=False)
check("rewrap keeps text F1 = 1", wrap["text"]["score"] == 1.0)
check("rewrap lowers line-break F1", wrap["flow"]["line_breaks"]["f1"] < 0.95)
check("rewrap keeps advance = 1", wrap["geometry"]["advance"] == 1.0)

# A uniform 10 pt horizontal shift: absolute fails, local measures do not.
shift = metrics.compare(layout(TEXT, 10, 20), layout(TEXT, 10, 20, x_shift=10), with_raster=False)
check("shift: absolute = 0", shift["geometry"]["absolute"] == 0.0)
check("shift: advance = 1", shift["geometry"]["advance"] == 1.0)
check("shift: leading = 1", shift["geometry"]["leading"] == 1.0)

# Looser leading: leading fails, advance does not.
lead = metrics.compare(layout(TEXT, 10, 20), layout(TEXT, 10, 20, lead=18), with_raster=False)
check("leading change detected", lead["geometry"]["leading"] == 0.0 and lead["geometry"]["advance"] == 1.0)

# Pagination drift: 15 lines per page instead of 20.
drift = metrics.compare(layout(TEXT, 10, 20), layout(TEXT, 10, 15), with_raster=False)
check("drift: page counts 2 vs 3", (drift["pages"]["ref"], drift["pages"]["ours"]) == (2, 3))
check("drift: same_page < 1", drift["flow"]["same_page"] < 1.0)
check("drift: pairing by words", drift["pairing"][1][1] == 1)

# Lost text: half the words missing.
half = " ".join(TEXT.split()[:200])
lost = metrics.compare(layout(TEXT, 10, 20), layout(half, 10, 20), with_raster=False)
check("lost half: recall 0.5", abs(lost["text"]["recall"] - 0.5) < 1e-9)

# Alignment with repeated words and a swap.
ref = "a b c d e f g a b c".split()
ours = "a b c e d f g a b c".split()
pairs = metrics.align(ref, ours)
check("align is monotone", all(p[0] < q[0] and p[1] < q[1] for p, q in zip(pairs, pairs[1:])))
check("align matches 9 of 10", len(pairs) == 9)

# Raster metrics: identical pages, and a blank candidate.
page = np.ones((100, 80), dtype=np.float32)
page[20:30, 10:70] = 0.0
check("raster identical ssim 1", metrics.page_raster_metrics(page, page)["ssim"] > 0.999)

# The share of a reference WPS drew in its fallback face: every style of it.
shares = {"SegoePrint": 0.5, "SegoePrint-Bold": 0.2, "ArialMT": 0.3}
check("fallback share sums the fallback styles", metrics.fallback_share(shares) == 0.7)
check("no fallback face, no share", metrics.fallback_share({"ArialMT": 1.0}) == 0.0)
# A face the document does not name is a substitution, unless it is a
# standard alias or a face of another script.
families = {"YuGothicUI-Regular": "Yu Gothic UI", "ArialMT": "Arial", "SimSun": "SimSun"}
named = {"Agency FB", "Helvetica"}
check("unnamed face is a substitution", metrics.fallback_share({"YuGothicUI-Regular": 0.7, "ArialMT": 0.3}, named, families) == 0.7)
check("aliases and script faces are not", metrics.fallback_share({"ArialMT": 0.6, "SimSun": 0.4}, named, families) == 0.0)
check("a named face is not", metrics.fallback_share({"ArialMT": 1.0}, {"Arial"}, families) == 0.0)

# Readability against the source: every word, visible, in order, no overlap.
rpage = [Word(t, 0, 10 + 40 * i, 10, 40 + 40 * i, 20) for i, t in enumerate(["one", "two", "three"])]
facts = PdfFacts(1, [(600.0, 800.0)], rpage, 11)
full = metrics.readability(["one", "two", "three"], facts, set(), 0, 0)
check("readability: all words in order", full["score"] == 100.0)
swapped = PdfFacts(1, [(600.0, 800.0)], [rpage[0], rpage[2], rpage[1]], 11)
check("readability: order counts", metrics.readability(["one", "two", "three"], swapped, set(), 0, 0)["order"] < 1.0)
check("readability: a missing-glyph box is not read", metrics.readability(["one", "two", "three"], facts, {"o"}, 0, 0)["completeness"] < 1.0)
check("readability: a missing picture counts", metrics.readability(["one", "two", "three"], facts, set(), 2, 1)["images"] == 0.5)
overlap = PdfFacts(1, [(600.0, 800.0)], [Word("one", 0, 10, 10, 60, 20), Word("two", 0, 12, 10, 62, 20), Word("three", 0, 100, 10, 140, 20)], 11)
check("readability: words over words are not legible", metrics.readability(["one", "two", "three"], overlap, set(), 0, 0)["legibility"] < 1.0)
empty = PdfFacts(1, [(600.0, 800.0)], [], 0)
check("readability: an empty page is not legible", metrics.readability(["one"], empty, set(), 0, 0)["score"] <= 15.0)
check("raster blank candidate low", metrics.page_raster_metrics(page, np.ones_like(page))["row_corr"] == 0.0)
check("diff image shape", metrics.diff_image(page, page).shape == (100, 80 * 3 + 12, 3))

# An image's box is its visible ink: the uncropped bitmap is cut by the clip.
visible = overlap_metric._visible_box((0.0, 0.0, 100.0, 100.0), [(10.0, 20.0, 90.0, 80.0)])
check("overlap: clip narrows the image box", visible == (10.0, 20.0, 90.0, 80.0))
clipped_away = overlap_metric._visible_box((0.0, 0.0, 10.0, 10.0), [(20.0, 20.0, 30.0, 30.0)])
check("overlap: a clipped-away image has no ink", clipped_away is None)
unclipped = overlap_metric._visible_box((1.0, 2.0, 3.0, 4.0), [])
check("overlap: an unclipped image keeps its box", unclipped == (1.0, 2.0, 3.0, 4.0))

check("safe_key nested", safe_key("a/b:c.docx") == "a__b_c")
check("safe_key long", len(safe_key("x" * 300 + ".docx")) <= 111)
check("safe_key rtf", safe_key("001_doc.rtf") == "001_doc")

# A directory of testdata/ holding a document is a corpus without registering it.
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    for rel in ("testdata/NEW_RTF/a.rtf", "testdata/NEW_DOCX/b.docx", "testdata/NEW_DOCX/~$b.docx", "testdata/djvu/c.djvu"):
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_bytes(b"x")
    names = [c.name for c in wpsqa_common.corpora(root)]
    check("corpora discovered", "NEW_RTF" in names and "NEW_DOCX" in names and "djvu" not in names)
    docs = list(wpsqa_common.iter_docs(root, ["NEW_DOCX", "NEW_RTF"]))
    check("corpora documents", [(d.corpus, d.key, d.is_rtf) for d in docs] == [("NEW_DOCX", "b", False), ("NEW_RTF", "a", True)])

width = max(len(n) for n, _ in checks)
for name, ok in checks:
    print(f"{name:<{width}}  {'ok' if ok else 'FAIL'}")
failed = [n for n, ok in checks if not ok]
print(f"{len(checks) - len(failed)}/{len(checks)} passed")
sys.exit(1 if failed else 0)
