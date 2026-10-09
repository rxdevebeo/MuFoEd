# Pixel-failure triage (97 Fail)

Mechanism triage of the development pixel gate: the 97 `Fail` documents are split
into provable, disjoint gate mechanisms so the open work can be scheduled per RC
card. This is a triage, **not** a support profile and **not** a fix; a class proves
*which bound rejected the document*, and the semantic owner is a hypothesis until
a minimal fixture confirms it.

> Follow-up (2026-10-09): the triage's dominant `line_metrics` class yielded three
> proven geometry fixes (RTF default font, empty-paragraph paragraph mark, font
> script-suffix mapping). The corpus is now 6/100 documents and 22/458 pages
> pixel-Pass (from 3/100, 6/423), pagination 65/100, with 0 regressions. See
> [RC04/tests.md](RC04/tests.md) and the receipts `target/rtf-import/RC04-parmark-*`.
> The per-class counts below describe the original receipt and are kept as the
> triage baseline. A follow-up witness (013 page 2) shows the residual
> `row_correlation` failures are **advance-driven reflow** from substitute-face
> advance differences, not vertical pitch: the symptom class is named after the
> violated bound, not the horizontal mechanism that caused it.

Reproduced from the fresh per-page receipt (the first pixel run that carries a
per-page SSIM + structural measurement), joined to the pagination receipt it was
derived from:

| Input | SHA-256 |
| --- | --- |
| `target/rtf-import/RC02-merge-pixels/results.json` | `9d86c86af140aa32ed40b91aa166ad7345940e956e28caa4776d6d552eaffb85` |
| `target/rtf-import/RC02-merge-visual/results.json` | `3c9b27a0e442fed91402e9da33e8a1756a373eb9d8298d00189306aff4b50f2f` |
| comparator (`compare_png`, 96 DPI) | `399d0af501ec525a4446e877f195deb90820f153660e86b12d7e8983f6ff1c14` |

Command:

```
python xtool/rtf/pixel_triage.py \
  --pixels target/rtf-import/RC02-merge-pixels \
  --visual target/rtf-import/RC02-merge-visual \
  --logs   target/rtf-import/RC02-merge-visual \
  --out    target/rtf-import/RC04-pixel-triage
```

Machine receipt: `target/rtf-import/RC04-pixel-triage/triage.json`
(`strictlib.rtf-pixel-triage.v1`). The classifier is pinned by
`xtool/rtf/test_pixel_triage.py`.

## Top-level split

The gate is also a pagination gate: a document only reaches per-page pixel
comparison when our page count matches the WPS reference. The 97 failures are
therefore disjoint at the top:

| Class | Documents | Meaning |
| --- | --- | --- |
| `pagination_page_count` | 38 | page count differs from WPS; no page correspondence, pixel comparison not applicable |
| matched-page pixel classes | 59 | page count matches; ≥1 page fails the pixel/structural gate |
| (pixel Pass controls) | 3 | 040, 043, 046 |

Page-level accounting over the 62 matched documents: **423 measured pages, 6 Pass,
417 Fail** — identical to the receipt totals.

## Matched-page mechanisms

Document primary class (the dominant failing mechanism per document; every
failing page counts once in the page column):

| Mechanism | Documents | Failing pages | Proven | Owner (hypothesis) |
| --- | --- | --- | --- | --- |
| `line_metrics` | 53 | 402 | structural bound | RC04 line/paragraph metrics |
| `blank_reference` | 2 | 5 | reference ink < `MIN_INK_FRACTION` | gate/blank-page handling |
| `glyph_appearance` | 3 | 3 | structure Ok, SSIM < 0.96 | RC02 substitute face / RC03 |
| `missing_ink` | 1 | 2 | candidate ink collapsed | RC02/RC03 glyph coverage |
| `extra_ink` | 3 (secondary) | 3 | ink ratio > 2.0 | RC06 borders / RC07 placeholder |
| `content_position` | 2 (secondary) | 2 | centroid bound | RC04 paragraph geometry |

Pages-by-mechanism sums to 417 exactly; documents-by-primary-class sum to 59.
Documents whose primary class is `line_metrics` but which also carry a specific
mechanism (009/031/059 blank page; 032/087/099 extra ink; 033 missing ink;
051/064 content position) are listed with that mechanism under `secondary` in the
receipt.

## What the triage proves

1. **The failures are layout/metrics, not font availability.** 72 of the 97
   failing documents render `Clean` (no PDF diagnostic), and 39 of the 44
   documents whose failing pages are *exclusively* vertical-mechanism (row
   correlation / vertical shift / ink extent) carry **zero** `pdf.font.*`
   diagnostics. The dominant
   mechanism is a small vertical line-pitch/offset divergence (402 of 417 pages;
   `shift_y` mostly 0–4 px with `corr_y` far below 0.9), which is a metrics
   problem owned by RC04, not a missing face. This confirms the plan's split: the
   development 3/100 pixel Pass is RC03/RC04 shaping/metrics, not RC02 fonts.
2. **Pagination mismatch is a second, independent mechanism.** The 38 documents
   split evenly into 19 where our render has *more* pages and 19 where it has
   *fewer*; 22 of the 38 render `Clean`, so most are pure metric/layout
   differences. 13 carry `pdf.font.no_glyph` and 3 `pdf.image.unsupported`.
   Extreme outliers (002 −150 pages, 072 +37, 096 −25, 084 +20) need their own
   witnesses before attribution but are provably pagination, not pixel.
3. **Four specific, directly-proven sub-classes exist** and each has a witness:
   - `missing_ink`: 035 (SimSun `宋体`, 141 no-glyph events) renders a blank page
     where WPS has ink — a lost-glyph page, not a metric drift.
   - `blank_reference`: 5 pages (009:140, 031:11, 036:2, 048:1, 059:14) where the
     WPS reference page ink is below the gate floor; 036/048/059 score SSIM ≈ 1.0,
     i.e. both pages are blank, so this is a trailing/blank-page handling case,
     not a rendering defect.
   - `glyph_appearance`: 019, 037, 071 have a structurally-correct page whose SSIM
     is only 0.91–0.957 — the layout is right and the substituted face is not the
     reference face.
   - `extra_ink`: 032, 087, 099 draw more than twice the reference ink on a page
     (border/table or image placeholder).

## Confirmed diagnosis vs open hypothesis

- **Confirmed:** the gate mechanism and its measured numbers for every class; the
  page-count direction for every pagination document; the blank-reference floor;
  the candidate-blank glyph-loss page; the structure-Ok/low-SSIM pages.
- **Hypothesis (needs a fixture):** the specific RTF feature behind the
  `line_metrics` majority — candidate owners are paragraph `\sl`/`\sa`/`\sb`
  line spacing, `\fN` face metrics via the bundled substitute, and run `\fs`
  sizes. RC04 already owns this line of work (`compatibility/RC04/`).
- **Not decided here:** the `blank_reference` documents 036/048/059 are a gate
  artifact (a blank reference page cannot satisfy `MIN_INK_FRACTION`); whether to
  special-case blank pages in the comparator is a gate question, and the plan
  forbids weakening a bound, so it is recorded rather than "fixed" by threshold.

## Next witnesses

1. `line_metrics`: pick one zero-diagnostic document (e.g. 013) and compare the
   WPS reference line origins against ours to separate a constant offset from a
   line-pitch difference; then map the responsible RTF control.
2. Pagination outliers: 002 (−150) and 072 (+37) are the largest single-mechanism
   deltas; section/table/content-presence witnesses belong to RC05/RC06/RC08.
3. `extra_ink`: confirm 032/087/099 against the DOCX (border vs picture
   placeholder) — RC06/RC07.
4. `blank_reference`: decide the comparator's blank-page contract without
   relaxing the ink-ratio bounds.
