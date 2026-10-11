# ADR-0013: Converting DjVu to Strict — map the zone tree, never infer a document

- **Status:** Accepted
- **Date:** 2026-10-05
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-9-TASK.md` §7 (9D), §8 (9E), §13; ADR-0009 (PDF reading),
  ADR-0010 (tables from a grid), ADR-0012 (reading DjVu)

## Context

A reader is not a converter. `strict-ooxml-djvu` (ADR-0012) hands back a page's
**native word boxes** and their UTF-8 — no glyph advances, no weight, no slant,
no ruling lines, no tables, no reading order beyond the tree's child order. The
question this record settles is what a converter may *do* with that.

The PDF path answered a harder version of the same question by **inferring**: it
has glyphs with positions and advances, and it reconstructs paragraphs, tables
and reading order by analysis (`strict-ooxml-convert/src/semantic.rs`,
`tables.rs`, `lists.rs`). Reusing that machinery for DjVu looks attractive — one
converter, one set of rules — but it is exactly the wrong move, because the two
formats hand over different facts.

PDF hands over **evidence** (glyph geometry) from which structure must be
*derived*. DjVu hands over **structure** (a page → column → region → paragraph →
line → word tree) and no glyph geometry at all. Running structural inference on
top of a structure that already exists would invent a second, weaker opinion and
let it override the one the file states.

## Decision

### DjVu converts through its own path, not the PDF converter

`strict-ooxml-convert` grows a `djvu` module (`src/djvu.rs`) behind a `djvu`
feature. It shares the crate's *facilities* — `to_twips`, the empty document
builder, the report, `sanitize_xml_text` — and shares **none** of the PDF
structure inference. There is no `Page` trait that both readers implement; a
common trait would only be shaped by the two call sites and would hide that the
inputs are not alike. The converter is a separate function,
`convert_djvu(&mut DjvuDocument, &DjvuOptions) -> Result<Converted, ConvertError>`.

### The zone tree is mapped, not re-derived

- `mode = semantic` emits one paragraph per **line** zone; `mode = visual` emits
  one paragraph per **line** too, but adds the estimated indentation and exact
  leading taken from the line box. No mode invents a paragraph where the file
  has none.
- Zone rectangles are in points, y down (ADR-0012), the same space WML uses, so
  the flip happened once on input and nothing flips on output.
- Font size is the **median word-box height**, clamped to `6..=72` pt and written
  as half-points. The report says `djvu.box-font` `Inferred`: the box is not the
  font, and the original face and emphasis are unknown. Where only line boxes
  exist (a file with no word zones) the line height stands in, which is recorded
  the same way.
- The tree's child order **is** reading order; the converter never re-sorts it.

### Nothing is synthesised

- No glyphs, no `w:sym`, no character advances: DjVu does not carry them, and a
  WordprocessingML run is not a place to guess them.
- No tables and no lists. `TXTz` has no ruling-line or numbering construct;
  ADR-0010's "a table is a grid of ruling lines" has nothing to read. A page that
  looks tabular stays a sequence of lines, and that is stated in the report.
- Drawings outside the text zones are **not** segmented from the page image by
  geometry the file does not state. The one segmentation that *is* honest — a
  figure from the mask — is described below.

### Links, outline and metadata are carried, and each is named

- A `maparea` with a URL becomes a `w:hyperlink` on the words under its
  rectangle (`r:id`, external). An internal target (`#page=N`, `±N`, an id or a
  title) becomes a bookmark on the target page plus `w:hyperlink/@w:anchor`. A
  hotspot that covers no text is a report entry, not a silent drop.
- The `NAVM` outline becomes **flat** `w:bookmarkStart` on its target pages,
  named after the heading. Word bookmarks are flat and `NAVM` is a tree, so the
  depth is dropped — and recorded (`djvu.outline`). External and unresolvable
  entries are recorded, never guessed.
- `(metadata …)` and `(xmp …)` fill `docProps/core.xml`. A native `(metadata …)`
  key wins; XMP (`dc:title`/`dc:creator`/`dc:description`/`dc:subject`/
  `pdf:Keywords`) is the fallback. This is the case where `docProps` is not a
  loss (`O-1a`): there *is* something to carry.

### Figures come from the mask, and only from the mask

The bitonal `Sjbz` mask is the one place DjVu states ink geometry at the pixel.
The reader exposes its 8-connected components as boxes (`ink_boxes`) and can crop
an unrotated region to PNG (`region_png`). The converter:

- subtracts the text-zone hotspots, so a glyph is not a figure;
- clusters the remaining components (4-point gap) into regions;
- keeps the regions above `recovery_mixed_min_area_ratio` of the page, largest
  first, at most `recovery_max_regions`;
- asks `DjvuOptions::figure_classifier` for a `FigureKind` and a description. A
  figure becomes a picture carrying `wp:docPr/@descr` and `@title`; a region the
  model calls a `scan`, on a page that has a text provider, goes to OCR instead;
  everything else is recorded `Lost`, never described by size alone.

A page with no text layer and a classifier is classified **before** OCR: a
`scan` may be transcribed, a `photograph` is described and not transcribed. This
is the one place the ring of "never overwrite" is entered, and it is entered
along the same rule as PDF's `mixed_regions_of`.

### Recognition is opt-in and never replaces native text

`DjvuOptions::text_recovery` and `figure_classifier` are `Option`s behind the
`djvu-image` feature. Without them the output is byte-for-byte reproducible
(`SC-D11`); a page with readable native text is **never** sent to a model. A
provider error, a refusal or a timeout is a report entry (`Severity::Lost` or
`Recovered`) and the document keeps building — the conversion never fails because
a model did.

### One report and one loss rule

The DjVu converter reports into the same `ConversionReport` the PDF path uses
(`strict-ooxml-convert/src/report.rs`), so the CLI prints one report regardless
of source. A reader-level observation (the DjVu reader's `warnings`, e.g. an
unreadable `NAVM`) is folded in as a `djvu.document.*` entry, not a separate
report type. PDF's own `ReadReport` is still merged through its PDF-specific
`merge`; the DjVu reader emits no separate report. The rule is the stage's:
**lost ⇒ recorded**, inferred ⇒ named (`SC-D10`).

`Encoded` stays the PDF crate's media type — DjVu never needed it. DjVu media
travels as a `(PartId, Vec<u8>)` pair in `Converted.media`, which is the other
half of the document's `MediaIndex` exactly as it is for PDF.

## Consequences

- The converter's honesty is checkable: every paragraph traces to a line zone,
  every link to a `maparea`, every bookmark to a `maparea` or an outline entry,
  every description to a model call. Nothing is a plausible guess.
- DjVu output is *editable text at the right place*, not a faithful re-layout;
  the box geometry that is kept (indent, leading in visual mode) is exactly the
  geometry the file states.
- Two converters exist. The cost is real — a second policy surface, a second set
  of report codes — and it is paid deliberately, because a shared trait would
  have to abstract over glyphs-vs-boxes and would leak PDF assumptions into a
  format that is more truthful than PDF about its own structure.
- The figure path depends on masks: a file that carries text but no `Sjbz` yields
  no figures, and the report says so rather than inventing regions.
- `wp:docPr/@descr` from a model is the only machine-written prose in the output.
  It is opt-in, attributed (`model version` in the report) and never removes
  content that the file stated.

## Alternatives considered

1. **Reuse the PDF converter behind a page trait.** Rejected: the readers have
   different facts (glyphs vs boxes), and the trait would be shaped by its two
   callers — abstraction without a third consumer, buying coupling and losing the
   distinction that the format hands us structure.
2. **Run PDF-style structural inference (tables, columns) on the zone tree.**
   Rejected: it would override the structure the file already states with a
   weaker guess, and ADR-0010's grid model has no ruling lines to read.
3. **Synthesise glyphs/advances so the output "looks" like the page.** Rejected:
   DjVu does not carry advances, so any such number is invented, and invented
   geometry is the one thing the loss report cannot describe.
4. **Segment drawings from the rendered page image by connected components of
   arbitrary regions.** Rejected in favour of the mask: the mask is the format's
   own bitonal statement of ink; a colour-plane segmentation would be our
   opinion, not the file's.
5. **Make recognition on by default when a provider is linked.** Rejected:
   `SC-D11` requires byte-identical output with and without the feature, and a
   model must be an explicit, attributable act.

## Validation

- `strict-ooxml-convert/tests/djvu.rs` — native text to Strict XML without loss
  (`SC-D6`), determinism, section boundaries, external and internal `maparea`
  links, `NAVM` outline bookmarks, and the figure contract (classified,
  refused, scan-not-transcribed) on deterministic native fixtures.
- `strict-ooxml-djvu/tests/read.rs`, `tests/image.rs` — the device-independent
  pieces the converter relies on: zones, rotation, `ink_boxes`, `region_png`,
  XMP fallback.
- `xtool/djvu-*.py` oracle drivers and the `djvu-oracle` CI job — the
  acceptance measurements (`SC-D3`…`SC-D14`) against DjVuLibre as an external
  process, never linked.
- `fuzz_djvu` — the reader (and therefore the converter's inputs) must return
  `Err`, never panic (`SC-D2`).
- `docs/DJVU_IMPORT.md` and `strict-ooxml-djvu/PROVENANCE.md` are maintained
  alongside the code and state what is and is not carried.
