# ADR-0006: SVG rendering — font metrics, style cascade, media and SSIM references

- **Status:** Accepted (superseded the SSIM/font-metrics waiver; see
  `STAGE-4-RENDER-FIDELITY.md`)
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-4-TASK.md` §4–§9; `TZ-STRICT-OOXML-RUST.md` §5.1–§5.4, §6,
  §11, §14, §15 (Stage 4), §17; ADR-0004, ADR-0005

## Context

Stage 4 turns the immutable DOM (`strict-ooxml-wml`) into SVG pages. The forces:

1. **No system fonts.** Layout needs glyph advances, but depending on installed
   fonts would make output machine-dependent; the TZ §14 NFR and the project's
   determinism requirement forbid that.
2. **Where to compute the cascade.** `STAGE-4-TASK.md` §3 leaves open whether
   effective paragraph/run properties are computed in `wml` or in the renderer.
3. **Media bytes.** A [`Document`] is self-contained but does not hold part
   bytes; image embedding needs `Package::read_part`.
4. **Visual acceptance.** `STAGE-4-TASK.md` §11 requires SSIM ≥ 95% against
   *approved* references whose provenance §9.3 leaves open (Word/LibreOffice).
5. **Output size / safety.** Pathological documents must not exhaust memory.

## Decision

### Font metrics: a bundled metric-compatible provider (open question 1)

Layout depends on a `FontProvider` trait. `BuiltinFontProvider` reads real glyph
advances and line metrics from **bundled metric-compatible open fonts** —
Carlito (Calibri), Caladea (Cambria), Arimo (Arial/Helvetica), Tinos (Times New
Roman) and Cousine (Courier New) — parsed with the maintained `skrifa` parser
(`STAGE-4-RENDER-FIDELITY.md` S4F.1/S4F.3). Font binaries are compiled into the
library via `include_bytes!`, so output is byte-identical across OSes and CI and
never consults system fonts. `map_family` substitutes the proprietary family
names and the same name is emitted in the SVG `font-family` so the rasterizer
resolves the bundled face. Unknown families fall back to the deterministic
character-class model (`FontMetrics::advance_em`). A system-font provider stays
behind the `system-fonts` feature and is outside the acceptance criteria.

### Style cascade lives in the render crate (open question 2)

`strict-ooxml-render-svg::style` computes `ComputedParagraph`/`ComputedRun`
from `docDefaults` (built-in defaults) → paragraph style `basedOn` chain → the
style → direct `pPr`/`rPr`, and for runs the character style chain plus `rPr`.
`TriState::Absent` inherits. The Stage-2 `wml` DOM is **not** changed: the
cascade is a rendering concern and keeping it here avoids destabilising the
DOM and its golden tests (ADR-0004).

The Stage-2 `StyleTable` retains the resolved `based_on_chain`; `docDefaults`
are parsed but **not** stored in the DOM, so the renderer's built-in defaults
play that role (`S4F-REWORK-2` B-1b). This is a documented limitation: a
document whose only spacing comes from `w:docDefaults/w:pPrDefault` does not get
it applied. It does not change pagination where a line grid is present, because
the grid rule (`S4F.1` above) snaps `lineRule="auto"` lines independently of the
multiplier, so `strict-profile` keeps its 2 pages; a future `docDefaults` capture
is additive.

### Media resolution: `MediaSource` + explicit modes (open question 4)

`render(document, options)` keeps the `STAGE-4-TASK.md` §6 signature and uses
`RenderOptions::media`:

- `EmbedDataUri` (**default**): bytes are Base64-inlined (`data:` URI); the SVG
  is self-contained. A tiny in-crate Base64 encoder avoids a dependency.
- `ExternalFiles`: `<image xlink:href="imageN.png">`; `media_file_name` exposes
  the mapping for the caller to write the files.
- `None`: a placeholder rectangle.

Because a bare `Document` cannot read bytes, `render` uses placeholders and a
second entry point `render_with_media(document, options, Some(&package))` (used
by the meta-crate and CLI) inlines/links the real bytes. This satisfies the
specified API while remaining functional.

### Pagination and scope

Page geometry comes from the last `sectPr` (universal measures such as
`545.30pt` are converted to twips); `w:docGrid/@w:linePitch` snaps line heights
to the document grid — each `lineRule="auto"` line occupies the smallest whole
number of grid units that fits its (possibly explicit) auto line height, and
the extra leading is centered on the natural line box, matching WPS even with
single spacing (`w:line="240"`, `S4F-REWORK-2` B-1); page breaks are explicit
(`w:br type=page`),
`pageBreakBefore`, and content overflow. `spacing/@w:before` is applied at the
top of a page (Word/WPS default). `keepLines` is honoured; `keepNext`,
multi-column flow, footnotes, fields, headers/footers, `wp:anchor` and math are
**out of scope** for Stage 4 (recorded by the Stage-3 support model). Inline
drawings we cannot rasterize (charts/diagrams) reserve their declared extent so
pagination matches the producer. Output is capped (`MAX_PAGES`, `MAX_ITEMS`) and
every coordinate is finite.

### SSIM references: resvg + pinned WPS (open question 3)

The acceptance criterion “SSIM ≥ 95% against approved references” is enforced:

- References are produced **once** by a pinned engine, WPS Office
  `12.1.0.28485` (`kwpsconvert.exe word2photo`), and committed under
  `strict-ooxml-core/tests/strict/refs/<doc>/page_N.png`. WPS is not required in
  CI.
- `tests/ssim.rs` rasterizes our SVG with `resvg` using the bundled fonts,
  converts both sides to grayscale and computes a windowed (11×11 Gaussian) mean
  SSIM; the gate is the **worst** page score (≥ 0.95) plus a **structural
  invariant** and the **page-count invariant**.
- The structural invariant (`S4F-REWORK-2` B-3/R-1) is rasterizer-independent
  and catches what integral SSIM tolerates on sparse text: candidate/reference
  ink coverage within `[0.5, 2.0]`×; row- and column-ink profile correlations
  ≥ 0.9 / ≥ 0.85; vertical and horizontal profile alignment ≤ 2 px; and an ink
  centroid drift ≤ 2.5 px on either axis. A blank page or a ≥ 3 px shift on
  **either** axis is rejected (SSIM alone tolerated a horizontal shift to
  ~10 px).
- Per-pixel comparison applies to the text references (`strict-text`,
  `strict-text-grid`). The chart/diagram document `strict-profile` is
  page-count checked only: Stage 4 cannot rasterize DrawingML charts, and a
  real Strict **text** document is not available under a compatible license, so
  real-Strict pixel fidelity is a **recorded exception** (see
  `STAGE-4-ACCEPTANCE.md` O1), not a closed item.

## Alternatives considered

1. **System fonts / `fontdb`.** Rejected as the default: breaks determinism and
   CI reproducibility.
2. **Compute the cascade in `wml`.** Rejected for Stage 4: it changes the DOM
   contract and Stage-2 golden tests; the cascade is a rendering concern.
3. **Raster SVG→PNG and compare to committed references in CI with `resvg`.**
   Adopted (`STAGE-4-RENDER-FIDELITY.md`): references are pinned and committed,
   and the bundled fonts make rasterization reproducible in CI.
4. **`ttf-parser` for font metrics.** Rejected: unmaintained
   (RUSTSEC-2026-0192) and blocked by `cargo-deny`; `skrifa` (fontations) is
   used instead.
5. **Hand-written JSON/XML for SVG.** Rejected: the writer is small but the
   escaping/coordinate rules are easier to keep correct with a dedicated module
   and an independent XML parser as oracle.

## Validation

- `tests/render.rs` — structure, formatting, page geometry, selection, scale,
  explicit page break, multi-page overflow, no `NaN`/`inf`.
- `tests/svg_oracle.rs` — independent `roxmltree` XML/SVG parse and structural
  invariants (viewBox, finite/within-bounds coordinates); corrupted SVG
  rejected (self-check).
- `tests/determinism` (inside `render.rs`) — two runs byte-identical; output is
  independent of insertion order.
- `tests/images.rs` — data URI / external file / placeholder modes.
- `tests/golden.rs` — commit-checked SVG snapshots (format lock).
- `tests/corpus.rs` — no panics on the Stage-1 corpus; Transitional refused.
- `tests/ssim.rs` — windowed SSIM self-test and the `resvg`-based gate against
  the committed WPS references (worst page ≥ 0.95 + page count); deterministic
  rasterization self-check.
- `benches/render.rs` — 10/100/500-page layouts.
- CI: SVG-validity oracle, corpus no-panic, SSIM gate, `cargo-deny`, coverage of
  `strict-ooxml-render-svg` ≥ 80% lines.

## Stage 5A.1a — header/footer decoration (additive)

- `layout/headerfooter.rs` lays each referenced header/footer part out once into
  a region (origin = content left edge, `y = 0`) and decorates every page after
  pagination. Selection honours `w:titlePg` (first page → `first`) and
  `settings.evenAndOddHeaders` (even pages → `even`); otherwise `default`.
- Geometry uses `w:pgMar/@w:header` and `@w:footer` (default 720 twips); the
  footer region's top is `page_height − footer_offset − region_height`. Paint
  order per page is deterministic: header, then body, then footer.
- Per the Stage-5 open-question default (§9 q4), headers/footers do **not**
  reduce the body's available height in this increment; they are painted inside
  the top/bottom margins. Multi-section header selection follows the existing
  single-section renderer (the last section's geometry is used).
- Known limitation (deferred to a later 5A increment): a header/footer taller
  than its margin band is not split across pages; it overflows into the body.
- Validation: `tests/headers.rs` (default/first/even, `pgMar` offset delta,
  footer placement, per-page presence).

## Stage 5A.1b — footnotes, endnotes and computed fields (additive)

- `notes.rs` numbers the referenced footnotes/endnotes in document order
  (`NoteNumbering`), with `ST_NumberFormat` formatting (decimal, decimalZero,
  roman, letter, bullet, none). Defaults: footnotes decimal, endnotes
  `lowerRoman`; `w:footnotePr`/`w:endnotePr` (section over `settings`) set
  format/start.
- Footnote references render as superscript markers and reserve a bottom-of-page
  area (`layout/paginate.rs`): a separator line plus the note bodies, laid out by
  `layout_blocks_inline` with the note's number substituted for `w:footnoteRef`.
  When the accumulated notes exceed the page, the remaining notes are deferred to
  the next page as a continuation (full-width separator). Endnotes are flowed
  after the body with an endnote separator.
- Computed fields (`fields.rs`): `fldSimple` and `fldChar` sequences are folded
  in the renderer; PAGE/NUMPAGES/SECTIONPAGES become placeholders resolved at
  placement. NUMPAGES/SECTIONPAGES use a bounded two-pass layout
  (`layout_document` → `layout_once`) that iterates until the page count is
  stable; other fields keep their cached result.
- Validation: `tests/notes.rs` (marker + area, endnote at end, continuation),
  `tests/fields.rs` (PAGE/NUMPAGES, complex field, cache fallback, `\* roman`),
  unit tests in `notes.rs`/`fields.rs`.

## Stage 5A.1c — complex tables (additive)

- `layout/table.rs` now builds a two-phase row/cell model. `gridSpan` positions
  and sizes the merged cell; `vMerge` (restart/continue) suppresses continuation
  cells and lets the restart cell own the merged region's shading, borders and
  content, spanning the summed row heights. Nested tables are laid out inline
  inside their cell.
- Tables emit `Flow::TableRow` (items, height, `header`) instead of opaque
  blocks, so `layout/paginate.rs` breaks the table between rows across pages and
  repeats rows marked `w:tblHeader` at the top of each continued page
  (`set_table_headers`/`repeat_table_headers`).
- Known limitation: a single row taller than a page is placed whole and may
  overflow (rows are not split at line boundaries); a merged region that crosses
  a page break draws its background/borders on the first page.
- Validation: `tests/tables.rs` (gridSpan, vMerge region height, nested table,
  page break, repeated header row).

## Stage 5A.1d — theme resolution (additive)

- `style.rs` threads the document `Theme` through the cascade. `apply_fonts`
  prefers a direct `w:ascii`/`w:hAnsi` family, then a theme reference
  (`minorHAnsi` → minor Latin, `majorEastAsia` → major East-Asian, …), so
  `w:asciiTheme`/`w:hAnsiTheme` resolve to the actual typeface (and are mapped to
  the bundled metric-compatible face by `map_family`).
- `apply_run_props` resolves `w:color/@w:themeColor` against the colour scheme and
  applies `w:themeShade` (multiply) or `w:themeTint` (lighten); a direct `w:val`
  still applies when no theme slot resolves.
- Validation: `tests/themes.rs` (minor/major theme fonts, direct override,
  accent resolution, shade), unit tests in `notes.rs`/`fields.rs`/`model/theme.rs`.

## Stage 5A.1e — full list numbering (additive)

- `numbering.rs` precomputes a marker for every numbered body paragraph in
  document order (`NumberingMarkers`, keyed by `SourceLocation`) so the result is
  stable across the two-pass field layout. The evaluator keeps per-`numId`
  counters, increments the current level and restarts deeper levels when a higher
  level is used (unless `w:lvlRestart w:val="0"`), and applies
  `lvlOverride`/`startOverride`.
- `w:lvlText` supports `%n` substitution with each referenced level's number
  format (decimal/roman/letter, via `NumberFormat`); bullet levels render their
  literal glyph. Level indentation (`w:pPr/w:ind`) is applied to a numbered
  paragraph that has no direct indentation, positioning the marker at the
  hanging offset and the text at the level start.
- `layout/paragraph.rs` looks the marker up by paragraph location instead of the
  previous single-level heuristic.
- Known limitations: `w:lvlJc` marker alignment, `w:suff`, style-linked
  numbering (`numStyleLink`/`styleLink`) and `numId=0` removal are not modelled.
- Validation: `tests/numbering.rs` (multi-level + restart, start override,
  bullet, indentation), unit tests in `numbering.rs`.


## Stage 5B - floating DrawingML and page borders (additive)

- New floating layer: `layout/floating.rs` resolves `wp:anchor` positions
  (`relativeFrom` page/margin/column/paragraph/line with `align`/`posOffset`,
  EMU->px) and inserts the object items into the page where its host paragraph
  was placed. `behindDoc` objects go to the page front, others to the back
  (before headers/footers); `relativeHeight` orders each group. `wrapNone` is an
  overlay, `wrapTopAndBottom` reserves vertical flow space; `wrapSquare/Tight/
  Through` are overlaid by bounding box (`Partial`).
- `paint/graphics.rs` renders shapes (preset-geometry subset, solid/gradient/
  pattern/none fills with theme colours, strokes with `prstDash`), groups
  (compose `off/ext/chOff/chExt` with child scale) and text boxes
  (`w:txbxContent` laid out as blocks with `bodyPr` insets/anchor). Anchored
  pictures apply `rot`/`flip`; `srcRect` is parsed but not cropped (`Partial`).
  `paint/shapes.rs` emits arbitrary `<path>` with a translate/rotate/scale
  transform.
- `layout/pageborders.rs` draws the four `w:pgBorders` edges with `offsetFrom`
  (page/text), `space`, `val`->dash, `sz`, colour/theme and `zOrder` (behind or
  front of the text).
- `RenderOptions::floating` (default `true`) and the CLI `--no-floating` disable
  the floating layer.
- Validation: `tests/floating.rs`, `tests/strict_profile.rs`,
  `strict-ooxml/tests/stage5b_corpus.rs`; the 5A/Stage-4 SSIM gate stays green.

## Stage 5C - math layout, the math font and inline drawings (additive)

- New `math/` module: a deterministic two-pass TeX-style layout
  (`layout_inline`, `layout_display`) that turns a formula into a size triple
  plus paint items, and a deterministic SVG emission of the stretchy
  constructs (delimiters, radical sign, large operators) as vector paths, so a
  character the bundled face does not cover still yields the right structure.
- **Block geometry follows the nominal math font, not the substitute.**
  `map_family` puts STIX Two Math behind `Cambria Math`, and the two are not
  metric-compatible. The *layout* therefore uses the nominal font's line
  metrics (Cambria Math `hhea`: ascender 1901, descender 483, 2048 units/em)
  for the formula row grid, the fraction box and the display sizes of the large
  operators — the same substitution principle the text cascade already uses
  when it puts Carlito's advances behind Calibri. Glyph shapes and advances
  still come from the substitute. `STAGE-5C-REWORK-1` C3.
- A display formula (`m:oMathPara`) is a self-contained block: the paragraph's
  `w:spacing` is **not** added around it, and the line it interrupts is emitted
  only when it carries content. Both were emitting a whole empty line of
  leading per display formula.
- A formula line keeps the paragraph's natural height and grows only by the
  formula's own box, capped at 1.60×.
- `w:docGrid` snaps the line height only for `w:type="lines"`,
  `linesAndChars` and `snapToChars`; `w:type="default"` — also the value when
  the attribute is absent — is *no* document grid (ISO/IEC 29500-1 §17.6.6).
- An inline drawing (`wp:inline` with a shape or a group) is placed **in the
  text line** with its bottom on the baseline, and the inline-image path no
  longer claims it: an extent alone no longer turns a shape into an image
  placeholder (`STAGE-5C-REWORK-1` D3).
- `map_family` learns the LibreOffice `Liberation Sans/Serif/Mono` families
  (→ `Arimo`/`Tinos`/`Cousine`, the same metric-compatible designs under their
  OFL names) so a LibreOffice Strict package renders black text instead of
  falling back to the rasterizer's grey (D1).
- Validation: `tests/math.rs`, `tests/ssim.rs` (SSIM ≥ 0.95 + structural
  invariant against pinned WPS references for `strict-stage5c`, `05`, `06`,
  `07`, `10`), `tests/floating.rs`, `strict-ooxml/tests/stage5c_corpus.rs`.
