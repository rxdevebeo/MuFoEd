"""Measures how close our PDF is to the WPS reference PDF - not pixel by pixel.

Both PDFs are read through the same library (pdfium) and rasterized by the
same rasterizer, so a difference is a difference in layout, never in the tool
that looked at it.

Four families of measures, each a number in [0, 1] plus raw values:

* **pages** - page count and page size agreement.
* **text** - multiset F1 of the words on all pages: text we lost (glyphs not
  drawn, parts not rendered) or invented.
* **flow** - where the words land in the reading order: the fraction of
  matched words on the same page, and the agreement of *line breaks* and
  *page breaks* between consecutive matched words. A line break is compared
  as an event ("the next word starts a new line"), so a drift accumulated
  above does not punish every later line again.
* **geometry** - the mean of three shares: matched words on the same page
  within `GEOM_TOL_PT` (absolute), and two local measures that do not inherit
  a drift from above - the left-edge step between adjacent words on one line
  (advance) and the baseline step between adjacent lines (leading).
* **raster** - per page pair (pages paired through matched words, not by
  index): SSIM at a low resolution, which sees blocks and not glyphs, and the
  correlation of the row and column ink profiles.

Words are matched with a patience diff: words unique in both documents are
anchors, the longest increasing subsequence of anchors fixes the alignment,
and the gaps between anchors are matched by a bounded `SequenceMatcher`.
"""

from __future__ import annotations

import bisect
import difflib
import math
import re
import unicodedata
from collections import Counter
from dataclasses import dataclass, field

import numpy as np
import pypdfium2 as pdfium
import pypdfium2.raw as pdfium_c

RASTER_DPI = 36
IMAGE_DPI = 72
GEOM_TOL_PT = 4.0
LOCAL_TOL_PT = 1.0
LOCAL_TOL_FRAC = 0.05
LINE_TOL_FRAC = 0.5  # a word starts a new line when its baseline moves > 0.5 x its height
MAX_GAP_DIFF = 3000  # gaps longer than this on either side are not diffed
WORD_RE = re.compile(r"\w+", re.UNICODE)

WEIGHTS = {"pages": 0.15, "text": 0.25, "flow": 0.25, "geometry": 0.20, "raster": 0.15}


@dataclass
class Word:
    text: str
    page: int
    x0: float
    top: float
    x1: float
    bottom: float
    line: int = 0  # line ordinal within the page

    @property
    def height(self) -> float:
        return max(0.5, self.bottom - self.top)


@dataclass
class PdfFacts:
    pages: int
    sizes: list[tuple[float, float]]
    words: list[Word]
    text_chars: int
    path: str = ""
    raster_cache: dict = field(default_factory=dict)


def _norm(text: str) -> str:
    return unicodedata.normalize("NFKC", text).casefold()


def read_pdf(path: str) -> PdfFacts:
    doc = pdfium.PdfDocument(path)
    try:
        sizes: list[tuple[float, float]] = []
        words: list[Word] = []
        total_chars = 0
        for index in range(len(doc)):
            page = doc[index]
            width, height = page.get_size()
            sizes.append((round(width, 1), round(height, 1)))
            textpage = page.get_textpage()
            count = textpage.count_chars()
            chars: list[str] = []
            boxes: list[tuple[float, float, float, float] | None] = []
            for i in range(count):
                code = pdfium_c.FPDFText_GetUnicode(textpage.raw, i)
                generated = pdfium_c.FPDFText_IsGenerated(textpage.raw, i) == 1
                ch = chr(code) if 0 < code < 0x110000 and not (0xD800 <= code < 0xE000) else " "
                if generated or ch in "\r\n\t­":
                    ch = " " if ch != "­" else ""
                if ch == "":
                    continue
                chars.append(ch)
                if ch.isspace():
                    boxes.append(None)
                else:
                    left, bottom, right, top = textpage.get_charbox(i)
                    boxes.append((left, height - top, right, height - bottom))
            total_chars += sum(1 for ch in chars if not ch.isspace())
            joined = "".join(chars)
            for match in WORD_RE.finditer(joined):
                span = [b for b in boxes[match.start() : match.end()] if b is not None]
                if not span:
                    continue
                words.append(
                    Word(
                        _norm(match.group()),
                        index,
                        min(b[0] for b in span),
                        min(b[1] for b in span),
                        max(b[2] for b in span),
                        max(b[3] for b in span),
                    )
                )
            textpage.close()
            page.close()
        _assign_lines(words)
        return PdfFacts(len(sizes), sizes, words, total_chars, path)
    finally:
        doc.close()


def _assign_lines(words: list[Word]) -> None:
    """Numbers lines within each page in content-stream order."""
    line = 0
    prev: Word | None = None
    for w in words:
        if prev is None or prev.page != w.page:
            line = 0
        elif _new_line(prev, w):
            line += 1
        w.line = line
        prev = w


def _new_line(prev: Word, w: Word) -> bool:
    return abs(w.bottom - prev.bottom) > LINE_TOL_FRAC * min(prev.height, w.height) or w.x0 < prev.x0 - 1.0


# ---------------------------------------------------------------------------
# Word alignment (patience diff)
# ---------------------------------------------------------------------------


def _lis(seq: list[int]) -> list[int]:
    """Indices into `seq` of a longest strictly increasing subsequence."""
    tails: list[int] = []
    tails_idx: list[int] = []
    parent = [-1] * len(seq)
    for i, value in enumerate(seq):
        pos = bisect.bisect_left(tails, value)
        if pos == len(tails):
            tails.append(value)
            tails_idx.append(i)
        else:
            tails[pos] = value
            tails_idx[pos] = i
        parent[i] = tails_idx[pos - 1] if pos > 0 else -1
    out: list[int] = []
    k = tails_idx[-1] if tails_idx else -1
    while k != -1:
        out.append(k)
        k = parent[k]
    return out[::-1]


def align(ref: list[str], ours: list[str]) -> list[tuple[int, int]]:
    """Monotone pairs (i_ref, j_ours) of equal words."""
    pairs: list[tuple[int, int]] = []
    _align_range(ref, ours, 0, len(ref), 0, len(ours), pairs, depth=0)
    pairs.sort()
    return pairs


def _align_range(ref, ours, a0, a1, b0, b1, out, depth):
    # Trim equal heads and tails.
    while a0 < a1 and b0 < b1 and ref[a0] == ours[b0]:
        out.append((a0, b0))
        a0 += 1
        b0 += 1
    while a0 < a1 and b0 < b1 and ref[a1 - 1] == ours[b1 - 1]:
        out.append((a1 - 1, b1 - 1))
        a1 -= 1
        b1 -= 1
    if a0 >= a1 or b0 >= b1:
        return
    count_a = Counter(ref[a0:a1])
    count_b = Counter(ours[b0:b1])
    pos_b = {}
    for j in range(b0, b1):
        w = ours[j]
        if count_b[w] == 1 and count_a.get(w) == 1:
            pos_b[w] = j
    anchors = [(i, pos_b[ref[i]]) for i in range(a0, a1) if ref[i] in pos_b]
    if anchors and depth < 64:
        keep = _lis([j for _, j in anchors])
        chain = [anchors[k] for k in keep]
        prev_a, prev_b = a0, b0
        for i, j in chain:
            _align_range(ref, ours, prev_a, i, prev_b, j, out, depth + 1)
            out.append((i, j))
            prev_a, prev_b = i + 1, j + 1
        _align_range(ref, ours, prev_a, a1, prev_b, b1, out, depth + 1)
        return
    if (a1 - a0) <= MAX_GAP_DIFF and (b1 - b0) <= MAX_GAP_DIFF:
        sm = difflib.SequenceMatcher(None, ref[a0:a1], ours[b0:b1], autojunk=False)
        for block in sm.get_matching_blocks():
            for k in range(block.size):
                out.append((a0 + block.a + k, b0 + block.b + k))


# ---------------------------------------------------------------------------
# Raster
# ---------------------------------------------------------------------------


def rasterize(path: str, page: int, dpi: int) -> np.ndarray:
    """One page as gray levels in [0, 1] (1 = white)."""
    doc = pdfium.PdfDocument(path)
    try:
        bitmap = doc[page].render(scale=dpi / 72.0, grayscale=True)
        return bitmap.to_numpy().astype(np.float32) / 255.0
    finally:
        doc.close()


def _raster(path: str, facts: PdfFacts, page: int) -> np.ndarray:
    cache = facts.raster_cache
    if page not in cache:
        cache[page] = rasterize(path, page, RASTER_DPI)
    return cache[page]


def _fit(img: np.ndarray, shape: tuple[int, int]) -> np.ndarray:
    """Crops or pads (white) to `shape`, anchored at the top-left corner."""
    out = np.ones(shape, dtype=np.float32)
    h = min(shape[0], img.shape[0])
    w = min(shape[1], img.shape[1])
    out[:h, :w] = img[:h, :w]
    return out


def _corr(a: np.ndarray, b: np.ndarray) -> float:
    if a.std() < 1e-9 and b.std() < 1e-9:
        return 1.0
    if a.std() < 1e-9 or b.std() < 1e-9:
        return 0.0
    return float(np.corrcoef(a, b)[0, 1])


def page_raster_metrics(ref_img: np.ndarray, our_img: np.ndarray) -> dict:
    from skimage.metrics import structural_similarity

    ours = _fit(our_img, ref_img.shape)
    ssim = float(structural_similarity(ref_img, ours, data_range=1.0, win_size=7))
    ink_r = 1.0 - ref_img
    ink_o = 1.0 - ours
    sum_r, sum_o = float(ink_r.sum()), float(ink_o.sum())
    ink_ratio = (sum_o / sum_r) if sum_r > 1e-6 else (1.0 if sum_o <= 1e-6 else math.inf)
    return {
        "ssim": round(ssim, 4),
        "row_corr": round(_corr(ink_r.sum(axis=1), ink_o.sum(axis=1)), 4),
        "col_corr": round(_corr(ink_r.sum(axis=0), ink_o.sum(axis=0)), 4),
        "ink_ratio": round(ink_ratio, 3) if math.isfinite(ink_ratio) else None,
    }


def diff_image(ref_img: np.ndarray, our_img: np.ndarray) -> np.ndarray:
    """RGB: reference | ours | overlay (reference ink red, ours blue)."""
    ours = _fit(our_img, ref_img.shape)
    # Ink only in the reference turns red, ink only in ours turns blue, shared
    # ink stays dark.
    overlay = np.stack([np.minimum(1.0, ours + (1 - ref_img)), np.minimum(ref_img, ours), np.minimum(1.0, ref_img + (1 - ours))], axis=-1)
    overlay = np.where((ref_img[..., None] > 0.97) & (ours[..., None] > 0.97), 1.0, overlay)
    gap = np.full((ref_img.shape[0], 6, 3), 0.6, dtype=np.float32)
    gray3 = lambda g: np.repeat(g[..., None], 3, axis=-1)  # noqa: E731
    return np.concatenate([gray3(ref_img), gap, gray3(ours), gap, overlay.astype(np.float32)], axis=1)


# ---------------------------------------------------------------------------
# Comparison
# ---------------------------------------------------------------------------


def _f1(tp: int, n_ref: int, n_ours: int) -> tuple[float, float, float]:
    precision = tp / n_ours if n_ours else (1.0 if n_ref == 0 else 0.0)
    recall = tp / n_ref if n_ref else (1.0 if n_ours == 0 else 0.0)
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return precision, recall, f1


def _break_f1(ref_words: list[Word], our_words: list[Word], pairs: list[tuple[int, int]], kind: str) -> dict:
    """Agreement of line/page breaks between consecutive matched words.

    Only pairs adjacent in both documents are compared, so an inserted or
    deleted word does not count as a break.
    """
    tp = fp = fn = agree = total = 0
    for (i0, j0), (i1, j1) in zip(pairs, pairs[1:]):
        if i1 != i0 + 1 or j1 != j0 + 1:
            continue
        r0, r1, o0, o1 = ref_words[i0], ref_words[i1], our_words[j0], our_words[j1]
        if kind == "page":
            rb, ob = r1.page != r0.page, o1.page != o0.page
        else:
            rb = r1.page != r0.page or r1.line != r0.line
            ob = o1.page != o0.page or o1.line != o0.line
        total += 1
        agree += rb == ob
        tp += rb and ob
        fp += ob and not rb
        fn += rb and not ob
    _, _, f1 = _f1(tp, tp + fn, tp + fp)
    return {"pairs": total, "ref_breaks": tp + fn, "our_breaks": tp + fp, "f1": round(f1, 4), "agreement": round(agree / total, 4) if total else None}


def _close(a: float, b: float) -> bool:
    """Local distances agree within 1 pt or 5 %, whichever is larger."""
    return abs(a - b) <= max(LOCAL_TOL_PT, LOCAL_TOL_FRAC * abs(b))


def _geometry(ref_words: list[Word], our_words: list[Word], pairs: list[tuple[int, int]]) -> dict:
    """Three views of word placement.

    * `absolute` - words on the same page within `GEOM_TOL_PT` on both axes.
      Words on another page are left to `flow`, so a pagination drift is not
      punished twice.
    * `advance` - for two adjacent words on one line in both renders, the
      distance between their left edges: font metrics, spacing, justification.
    * `leading` - for two adjacent words that start a new line in both renders,
      the baseline step: line height and paragraph spacing.

    `advance` and `leading` are local, so they do not inherit a shift
    accumulated earlier on the page.
    """
    dx: list[float] = []
    dy: list[float] = []
    within = 0
    for i, j in pairs:
        r, o = ref_words[i], our_words[j]
        if r.page != o.page:
            continue
        ddx, ddy = abs(o.x0 - r.x0), abs(o.bottom - r.bottom)
        dx.append(ddx)
        dy.append(ddy)
        within += ddx <= GEOM_TOL_PT and ddy <= GEOM_TOL_PT
    adv_ok = adv_n = lead_ok = lead_n = 0
    for (i0, j0), (i1, j1) in zip(pairs, pairs[1:]):
        if i1 != i0 + 1 or j1 != j0 + 1:
            continue
        r0, r1, o0, o1 = ref_words[i0], ref_words[i1], our_words[j0], our_words[j1]
        if r0.page != r1.page or o0.page != o1.page:
            continue
        same_r, same_o = r0.line == r1.line, o0.line == o1.line
        if same_r and same_o:
            adv_n += 1
            adv_ok += _close(o1.x0 - o0.x0, r1.x0 - r0.x0)
        elif not same_r and not same_o:
            lead_n += 1
            lead_ok += _close(o1.bottom - o0.bottom, r1.bottom - r0.bottom)
    parts = {
        "absolute": within / len(dx) if dx else None,
        "advance": adv_ok / adv_n if adv_n else None,
        "leading": lead_ok / lead_n if lead_n else None,
    }
    present = [v for v in parts.values() if v is not None]
    return {
        "same_page_words": len(dx),
        "median_dx_pt": round(float(np.median(dx)), 2) if dx else None,
        "p90_dx_pt": round(float(np.percentile(dx, 90)), 2) if dx else None,
        "median_dy_pt": round(float(np.median(dy)), 2) if dy else None,
        "p90_dy_pt": round(float(np.percentile(dy, 90)), 2) if dy else None,
        "tol_pt": GEOM_TOL_PT,
        "advance_pairs": adv_n,
        "leading_pairs": lead_n,
        **{k: (round(v, 4) if v is not None else None) for k, v in parts.items()},
        "score": round(sum(present) / len(present), 4) if present else 0.0,
    }


# The face WPS draws a family with when the machine does not have it: Segoe
# Print, whatever the family was (inter, EB Garamond, Century Schoolbook,
# even Cousine). Such a reference shows the host's substitution, not the
# document, and its score is reported apart.
WPS_FALLBACK_FONTS = ("SegoePrint",)
# Faces WPS draws although the document does not name them, and rightly:
# the standard aliases (Helvetica -> Arial, an RTF run without a font ->
# Times New Roman), symbols, math, and the system faces of other scripts.
# Any other face the document does not name is a substitution for a family
# the host lacks (Agency FB -> Yu Gothic UI, Blackadder ITC -> Gabriola).
LEGIT_UNNAMED = frozenset(
    "".join(ch for ch in name.lower() if ch.isalnum())
    for name in (
        "Times New Roman", "Arial", "Courier New", "Symbol", "Wingdings", "Webdings", "Marlett",
        "Segoe UI Symbol", "Segoe UI Emoji", "Segoe UI Historic", "Cambria Math", "Sans Serif Collection",
        "SimSun", "NSimSun", "SimSun-ExtB", "SimSun-ExtG", "MS Gothic", "MS PGothic", "MS UI Gothic",
        "Yu Gothic", "Malgun Gothic", "Microsoft YaHei", "Microsoft YaHei UI", "Microsoft JhengHei",
        "Microsoft JhengHei UI", "PMingLiU-ExtB", "MingLiU-ExtB", "MingLiU_HKSCS-ExtB", "MingLiU_MSCS-ExtB",
        "Nirmala UI", "Nirmala Text", "Ebrima", "Gadugi", "Leelawadee UI", "Myanmar Text", "Javanese Text",
        "Mongolian Baiti", "Microsoft Himalaya", "Microsoft Yi Baiti", "Microsoft Tai Le",
        "Microsoft New Tai Lue", "Microsoft PhagsPa", "Sylfaen",
    )
)
FALLBACK_SHARE = 0.10


def font_shares(path: str) -> dict[str, float]:
    """Share of the visible characters of `path` drawn in each font (by
    base name, without the subset tag)."""
    import ctypes

    doc = pdfium.PdfDocument(path)
    counts: Counter = Counter()
    buf = ctypes.create_string_buffer(256)
    flags = ctypes.c_int()
    try:
        for index in range(len(doc)):
            page = doc[index]
            textpage = page.get_textpage()
            for i in range(textpage.count_chars()):
                code = pdfium_c.FPDFText_GetUnicode(textpage.raw, i)
                if code <= 32 or pdfium_c.FPDFText_IsGenerated(textpage.raw, i) == 1:
                    continue
                pdfium_c.FPDFText_GetFontInfo(textpage.raw, i, buf, len(buf), ctypes.byref(flags))
                counts[buf.value.decode("latin-1").split("+")[-1]] += 1
            textpage.close()
            page.close()
    finally:
        doc.close()
    total = sum(counts.values())
    return {name: round(n / total, 4) for name, n in counts.most_common()} if total else {}


def fallback_share(shares: dict[str, float], named: set[str] | None = None, families: dict[str, str] | None = None) -> float:
    """Share of the reference drawn in a face the document did not ask for.

    Segoe Print always counts (WPS's face for any missing family). With the
    document's `named` families (`named_families`), so does every face whose
    family the document never names: WPS also set Agency FB in Yu Gothic UI
    and Blackadder ITC in Gabriola. `families` maps a PostScript name to its
    family (the host inventory); without it the name's stem is compared."""
    total = 0.0
    folded = {_fold(n) for n in named or ()}
    folded.discard("")
    for font, share in shares.items():
        if font.startswith(WPS_FALLBACK_FONTS):
            total += share
        elif folded and not _is_named(font, folded, families or {}) and not _is_legit(font, families or {}):
            total += share
    return round(total, 4)


def _is_legit(postscript: str, families: dict[str, str]) -> bool:
    family = families.get(postscript)
    if family is not None:
        return _fold(family) in LEGIT_UNNAMED
    stem = _fold(postscript.split("-")[0])
    return any(stem.startswith(name) for name in LEGIT_UNNAMED if len(name) >= 5)


def _fold(name: str) -> str:
    return "".join(ch for ch in name.lower() if ch.isalnum())


def _is_named(postscript: str, folded: set[str], families: dict[str, str]) -> bool:
    family = families.get(postscript)
    if family is not None:
        return _fold(family) in folded
    stem = _fold(postscript.split("-")[0])
    for suffix in ("psmt", "mt", "ps"):
        if stem.endswith(suffix) and len(stem) > len(suffix) + 3:
            stem = stem[: -len(suffix)]
            break
    return any(stem == n or (len(n) >= 4 and (stem.startswith(n) or n.startswith(stem))) for n in folded)


def named_families(path: str) -> set[str]:
    """Every font family a DOCX or RTF names: its font table, run fonts and
    theme fonts (`fallback_share`)."""
    import re
    import zipfile

    names: set[str] = set()
    if path.lower().endswith(".rtf"):
        with open(path, "rb") as f:
            text = f.read().decode("latin-1")
        start = text.find("{\\fonttbl")
        if start >= 0:
            depth, end = 0, start
            for end in range(start, min(len(text), start + 200_000)):
                depth += {"{": 1, "}": -1}.get(text[end], 0)
                if depth == 0:
                    break
            table = text[start:end]
            for entry in re.findall(r"\\f\d+[^;]*;", table):
                entry = re.sub(r"\{\\\*\\panose [^}]*\}", "", entry)
                for alt in re.findall(r"\{\\\*\\falt ([^}]*)\}", entry):
                    names.add(alt.strip())
                entry = re.sub(r"\{\\\*\\falt [^}]*\}", "", entry)
                entry = re.sub(r"\\[a-z]+-?\d* ?", "", entry)
                names.add(entry.strip(" {};"))
        return {n for n in names if n}
    try:
        z = zipfile.ZipFile(path)
    except (OSError, zipfile.BadZipFile):
        return names
    for part in z.namelist():
        if not (part.startswith("word/") and part.endswith(".xml")) and not part.startswith("word/theme/"):
            continue
        try:
            xml = z.read(part).decode("utf-8", "ignore")
        except (KeyError, OSError, zipfile.BadZipFile):
            continue
        names.update(re.findall(r'<w:font w:name="([^"]+)"', xml))
        names.update(re.findall(r'<w:altName w:val="([^"]+)"', xml))
        names.update(re.findall(r'w:(?:ascii|hAnsi|eastAsia|cs)="([^"]+)"', xml))
        names.update(re.findall(r'typeface="([^"]+)"', xml))
    return {n.strip() for n in names if n.strip()}


def compare(ref: PdfFacts, ours: PdfFacts, with_raster: bool = True) -> dict:
    out: dict = {}
    # -- pages -------------------------------------------------------------
    n_r, n_o = ref.pages, ours.pages
    same_size = sum(
        1 for a, b in zip(ref.sizes, ours.sizes) if abs(a[0] - b[0]) <= 1.5 and abs(a[1] - b[1]) <= 1.5
    )
    page_count_score = max(0.0, 1.0 - abs(n_o - n_r) / max(1, n_r))
    size_score = same_size / max(1, min(n_r, n_o)) if min(n_r, n_o) else 0.0
    out["pages"] = {
        "ref": n_r,
        "ours": n_o,
        "delta": n_o - n_r,
        "same_size": same_size,
        "ref_size": ref.sizes[0] if ref.sizes else None,
        "our_size": ours.sizes[0] if ours.sizes else None,
        "score": round(0.8 * page_count_score + 0.2 * size_score, 4),
    }
    # -- text --------------------------------------------------------------
    rt = [w.text for w in ref.words]
    ot = [w.text for w in ours.words]
    common = sum((Counter(rt) & Counter(ot)).values())
    precision, recall, f1 = _f1(common, len(rt), len(ot))
    out["text"] = {
        "ref_words": len(rt),
        "our_words": len(ot),
        "precision": round(precision, 4),
        "recall": round(recall, 4),
        "score": round(f1, 4),
    }
    # -- flow --------------------------------------------------------------
    pairs = align(rt, ot)
    same_page = sum(1 for i, j in pairs if ref.words[i].page == ours.words[j].page)
    page_shift = [ours.words[j].page - ref.words[i].page for i, j in pairs]
    lines = _break_f1(ref.words, ours.words, pairs, "line")
    pbreaks = _break_f1(ref.words, ours.words, pairs, "page")
    coverage = len(pairs) / len(rt) if rt else (1.0 if not ot else 0.0)
    same_page_frac = same_page / len(pairs) if pairs else 0.0
    line_f1 = lines["f1"] if lines["ref_breaks"] or lines["our_breaks"] else 1.0
    flow_score = (same_page_frac + line_f1) / 2 if pairs else 0.0
    out["flow"] = {
        "matched": len(pairs),
        "coverage": round(coverage, 4),
        "same_page": round(same_page_frac, 4),
        "mean_abs_page_shift": round(sum(abs(s) for s in page_shift) / len(page_shift), 3) if page_shift else None,
        "max_page_shift": max(page_shift, key=abs) if page_shift else None,
        "line_breaks": lines,
        "page_breaks": pbreaks,
        "score": round(flow_score, 4),
    }
    out["geometry"] = _geometry(ref.words, ours.words, pairs)
    # -- page pairing and raster -------------------------------------------
    pairing: list[tuple[int, int]] = []
    votes: dict[int, Counter] = {}
    for i, j in pairs:
        votes.setdefault(ref.words[i].page, Counter())[ours.words[j].page] += 1
    for p in range(n_r):
        if p in votes:
            pairing.append((p, votes[p].most_common(1)[0][0]))
        elif p < n_o:
            pairing.append((p, p))
    out["pairing"] = pairing
    if with_raster and pairing:
        per_page = []
        for p, q in pairing:
            m = page_raster_metrics(_raster(ref.path, ref, p), _raster(ours.path, ours, q))
            m.update({"ref_page": p + 1, "our_page": q + 1})
            per_page.append(m)
        ssim = [m["ssim"] for m in per_page]
        rows = [max(0.0, m["row_corr"]) for m in per_page]
        cols = [max(0.0, m["col_corr"]) for m in per_page]
        # Pages of the reference with no counterpart in our render score 0.
        missing = max(0, n_r - len(per_page))
        mean = lambda xs: (sum(xs) / (len(xs) + missing)) if xs or missing else 0.0  # noqa: E731
        raster_score = (mean(ssim) + mean(rows) + mean(cols)) / 3
        worst = min(per_page, key=lambda m: m["ssim"] + m["row_corr"])
        out["raster"] = {
            "dpi": RASTER_DPI,
            "mean_ssim": round(sum(ssim) / len(ssim), 4),
            "min_ssim": round(min(ssim), 4),
            "mean_row_corr": round(sum(rows) / len(rows), 4),
            "mean_col_corr": round(sum(cols) / len(cols), 4),
            "worst_page": worst,
            "pages": per_page,
            "score": round(raster_score, 4),
        }
    elif with_raster:
        out["raster"] = {"score": 0.0, "pages": []}
    # -- composite ---------------------------------------------------------
    total = 0.0
    weight = 0.0
    for key, w in WEIGHTS.items():
        if key in out and out[key].get("score") is not None:
            total += w * out[key]["score"]
            weight += w
    out["score"] = round(100 * total / weight, 1) if weight else 0.0
    out["grade"] = grade(out["score"])
    return out


def grade(score: float) -> str:
    if score >= 90:
        return "A"
    if score >= 75:
        return "B"
    if score >= 50:
        return "C"
    return "D"
