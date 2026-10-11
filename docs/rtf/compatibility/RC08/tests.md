# RC08 sections, regions, notes and complex fields

Card: [STATUS.json](STATUS.json). Plan:
[REMAINING_COMPATIBILITY_PLAN.md](../../REMAINING_COMPATIBILITY_PLAN.md).

## Section headers when the reset precedes the break (2026-10-09)

Opening witness: 072 `OEB-2016.1-US.rtf` (`+37` pages, `Clean`, no `pdf.font.*`
diagnostics, 40 `\sect` sections). Its page 2 rendered no header, and the written
DOCX contained 40 `header*.xml` parts but only **one** `w:headerReference`.

Cause: the RTF resets section properties with `\sectd` **before** the break
(`\sectd\sect`), the reverse of Word's `\sect\sectd`. The parser reset an
accumulator that the following `\sect` was about to commit, so the header and
geometry a section had accumulated were wiped before the commit; only the last
section (the body-level `w:sectPr`) kept its header.

Fix (parser): a `\sectd` now records a deferred reset (`section_reset_pending`).
The reset is applied:
- after the `\sect` that commits the section, when `\sectd` immediately precedes
  the break (072 order), and
- at the first following non-`\sect` control word or text, when `\sectd` follows
  the break (Word order).

Both orders now commit the section that actually preceded the break.
`structure.rs::wps_sectd_before_sect_keeps_each_section_header` pins the 072
order, and `section_reset_restores_document_geometry_not_ooxml_defaults` keeps
the Word order. 072 writes 40 header references and the header renders on every
section's pages.

## Line pitch of the section body (2026-10-09)

With the header fixed, the remaining pagination gap was `\sl280\slmult1` line
pitch: the reference body advances `16.1pt` per line, ours `17.1pt`.
`16.1 = 1.15em x (280/240)` is the **Arimo** (Arial-metric) line height, while
the unknown-family wildcard resolved Verdana to Carlito (`1.222em`). Mapping the
common sans families (`Verdana`, `Tahoma`, `Helvetica Neue`) to Arimo brings the
pitch to 16.1pt and the page count from 641 to 638.

## Chapter page numbers as dynamic fields (2026-10-09)

The header text `Ruth (Page \chpgn)` lost the number because `\chpgn` (and
`\sectnum`) fell through to the unknown-control Loss. They are now emitted as a
`PAGE` / `SECTION` field with no cached result; the renderer's page-dependent
field pass (AUD-70) computes the value for headers. 072's header now reads
`Ruth (Page 2)`, and its import losses drop from 42 to 2.
`structure.rs::chapter_page_number_becomes_a_page_field` pins the mapping.

## Not yet closed

### 072 remaining +34 pages is a WPS source-format inconsistency, not a mapping bug

Body pages now match line-for-line, so the residual is the inter-paragraph gap.
Measured at 96 DPI against the two immutable WPS references:

| witness | source | gap | `space_after + space_before` | max | WPS measured |
| --- | --- | --- | --- | --- | --- |
| 072 heading -> heading (`\sb360\sa360`, direct) | RTF | line + 720tw | 36pt (sum) | 18pt | **38.25pt ≈ sum** |
| 06-strict-math-display body -> Heading2 (style 160/280) | DOCX | line + 440tw | 22pt (sum) | 14pt | **14.74pt ≈ max** |

The same WPS Office sums `before`+`after` when rendering the **RTF** source and
takes the **max** when rendering the **DOCX** source. Our renderer uses `max`
(`paginate.rs`), which is what the immutable WPS-DOCX reference renders
(`refs/06-strict-math-display`) and the whole SVG baseline expect. A naive
`max -> sum` change was tried and **reverted**: it moved the calibrated
`06-strict-math-display` structural extent (`was 5, now 37`) and failed
`matches_wps_references`.

Consequences:

- 072's page count cannot be closed by a shared-layout spacing change without
  breaking the DOCX baseline; the two WPS oracles are mutually inconsistent.
- Emulating the RTF sum by inflating `w:before` at import would misrepresent the
  document (a DOCX consumer applies `max` and over-spaces) and violates the plan's
  "no hidden XML fragment substitutes for the contract".
- Therefore 072's remaining gap is recorded as a **WPS RTF-path quirk**: our
  model preserves the RTF `\sb`/`\sa` verbatim and the renderer applies the
  correct OOXML `max`, which is what the supported profile (DOCX-produced Strict)
  requires. It is not a defect and not a support-profile blocker.

`w:contextualSpacing` remains unimplemented in the renderer (a real gap) and is
the natural place to encode same-style suppression if a future witness needs it,
but it does not change the two witnesses above (neither uses it).

## Bookmarks (2026-10-09)

`{\*\bkmkstart Name}` / `{\*\bkmkend Name}` fell through to the
`destination:bkmkstart`/`bkmkend` Loss (616 occurrences across 7 corpus docs), so
every named anchor a hyperlink or `REF` field targets was lost. They now become
`Inline::BookmarkStart`/`BookmarkEnd` with a shared decimal `w:id` (assigned per
name) and the `w:name` taken from the destination text.
`structure.rs::bookmarks_become_paired_start_and_end_inlines` pins the pairing;
014 writes 8 start/end pairs. Bookmarks are zero-width, so pagination (65/100)
and pixels (6/100, 22/458) are unchanged.

## First/even-page regions (2026-10-09)

`\titlepg` and the even-page `\headerl`/`\footerl` regions were dropped, so
first-page and even-page headers/footers never rendered (002 carried neither
`w:titlePg` nor `w:evenAndOddHeaders`). `\titlepg` now sets the section's
`w:titlePg`, and an even region sets `w:evenAndOddHeaders` in settings; 002
writes 7 `w:titlePg` and even/default header references.
`structure.rs::titlepg_and_even_headers_are_recorded` pins it. Headers sit in
the margin, so pagination (65/100) and pixels (6/100, 22/458) are unchanged.

## Section columns (2026-10-09)

`\cols`/`\colno`/`\colw`/`\colsx`/`\colsr` were dropped, so a two-column section
lost its `w:cols` entirely. They now map to `w:cols/@num`, `@space`, and the
per-column `w:col/@w`/`@space` specs, with `equalWidth` cleared when an explicit
`\colw` appears. 074 and 092 carry their 2-column definitions
(`<w:cols w:num="2" w:equalWidth="false"><w:col w:w="5795" w:space="1657"/><w:col w:w="2228"/></w:cols>`).
`structure.rs::section_columns_map_to_w_cols` pins it. The shared renderer still
lays out a single column, so the property round-trips and validates but the
rendered columns are a follow-up shared-layout build; pagination (65/100) and
pixels (6/100, 22/458) are unchanged.

`\rtlsect`/`\ltrsect` set the section bidi flag (`\rtlpar`/`\ltrpar` were already
mapped); `structure.rs::section_bidi_controls_map` pins it.

## Section column layout (2026-10-11)

The `w:cols` mapping above was inert: the shared renderer painted one column.
`layout::column_layout` now turns `w:cols` into the per-column `(left, width)` in
px — equal-width columns with `w:cols/@space`, or the explicit `w:col/@w` and
`@space` of an unequal set (an undeclared width in an explicit set is the equal
share of what is left; the units are never guessed). The paginator breaks a block
that overflows a column to the **next column on the same page** and opens a new
page only when the columns are full; a page or section break resets to the first
column, and a continuous section carries its columns with its deferred geometry.
Items placed in a later column are shifted by the column offset, so a paragraph
that starts in one column and continues into the next keeps its wrap. A
single-column section is the fast path: the column list is empty, every offset is
zero, and the old layout is byte-identical. `w:br w:type="column"` becomes a
`Flow::ColumnBreak` and advances a column; endnotes span the whole page.

`strict-ooxml-render-svg/tests/f14_columns.rs` pins
`column_break_moves_the_rest_to_the_next_column`,
`overflowing_text_flows_into_the_second_column` and
`a_single_column_section_keeps_the_page_content_box`.

Corpus receipt `target/rtf-import/RC08-columns-layout/`: pagination **65/100 ->
66/100**. Only 092 changes (21 -> 20 pages) and now equals the WPS reference;
every other page count equals `RC06-final3`, so the single-column fast path does
not regress. 074 renders both columns (left edge 76 px, right edge 425 px at
96 dpi) but stays 4 pages against the reference's 3: its 2-column section is a
continuous break, and a continuous break defers its geometry **and its columns**
to the next page, so the columns start on page 2 rather than mid-page 1. That
same-page continuous-section change is the remaining build, recorded below.

### Remaining

A continuous section break still defers its geometry (and now its columns) to the
next page, so 074's two-column section starts on page 2 instead of mid-page 1.
An unequal-width column set also has no per-column re-wrap: a paragraph that
starts in a wide column and continues into a narrower one keeps the starting
column's wrap (074/092 do not exercise it).

## Note numbering and placement (2026-10-10)

Footnote/endnote **numbering and placement** controls were dropped as
`control:*` Loss, so `w:footnotePr`/`w:endnotePr` were never written. They now
map to the existing `NoteProperties` model per the RTF 1.5 "Footnotes and
Endnotes" table:

- **document-wide** (`\ftn*`/`\enddoc`, `\aftn*`/`\aenddoc`) land in
  `settings.xml` (`w:footnotePr`/`w:endnotePr`);
- **section-scoped** (`\sftn*`/`\saftn*`, the WPS `\sectd` forms) land in the
  running `w:sectPr` and do **not** leak into the document settings.

The mapped properties are the number format (`\ftnnar`/`alc`/`auc`/`rlc`/`ruc`/
`chi` and the endnote twins -> `w:numFmt`), the starting number
(`\ftnstart`/`\aftnstart` -> `w:numStart`), the restart mode (`\ftnrstcont`/
`\aftnrstcont` -> `continuous`, `\ftnrestart`/`\aftnrestart` -> `eachSect`,
`\ftnrstpg` -> `eachPage`) and the position (`\ftnbj` -> `pageBottom`,
`\ftntj` -> `beneathText`, `\endnotes`/`\aendnotes` -> `sectEnd`,
`\enddoc`/`\aenddoc` -> `docEnd`).

`structure.rs::document_note_numbering_and_placement_map_to_settings` and
`::section_note_properties_map_to_the_running_section` pin the mapping through
import -> Strict write -> reopen. `::recognized_note_controls_are_not_reported_
as_loss` pins that the recognized family no longer reports Loss, and
`::unsupported_note_controls_stay_explicit_losses` is the negative control:
`\aftnbj` has no `ST_EdnPos` value and `\ftnlytwnine` has no Strict equivalent,
so both keep their explicit `control:*` Loss instead of a guessed mapping.
`strict-ooxml/tests/rtf.rs::note_numbering_properties_reach_the_svg_marker`
(`rtf,svg`) is the end-to-end witness: an imported `\ftnnalc\ftnstart3`
footnote renders the marker `c` (not the default `1`).

### Note separator definitions (2026-10-10)

`\ftnsep`/`\ftnsepc` and the endnote twins are now captured. `RunContent::
Separator`/`ContinuationSeparator` were added to the shared model: the reader no
longer discards `w:separator`/`w:continuationSeparator` (it used to record them
`Supported` and drop them), and the writer emits them. The destination becomes
the reserved note definitions (`id -1` separator, `id 0` continuationSeparator)
plus their `w:footnotePr`/`w:endnotePr` references.
`structure.rs::note_separator_definitions_become_reserved_notes` pins it through
import -> Strict write -> reopen. Capturing the body unmasks the formatting
controls inside the separator (`\cgrid`, `\hyphpar`, `\adjustright`, `\itap0`,
...), so the ledger rises by 152 across 6 documents (honest per-control
accounting replaces the single `destination:ftn*` placeholder). Output DOCX,
page counts and PDF bytes are unchanged.

## Field-instruction formatting (2026-10-10)

The writer embeds run formatting in `\fldinst`; it styles the field-code display,
not the instruction text. A curated set of character-formatting controls is now
ignored as explicit **Info** (`field-instruction-formatting:<name>`) instead of
Loss; anything else inside an instruction stays `field-instruction-control:*`
Loss (no blanket rename).
`structure.rs::field_instruction_formatting_is_info_not_loss` and
`::unknown_field_instruction_control_stays_a_loss` pin both sides. Corpus import
losses 170981 -> 137380.

## Contextual spacing (2026-10-10)

`\contextualspace` -> `w:contextualSpacing`, and the paginator suppresses the
before/after gap between consecutive same-style paragraphs when either side
enables it (`structure.rs::contextual_space_maps_to_wml_contextual_spacing`,
`p5_p6.rs::t_rc08_contextual_spacing_suppresses_same_style_gap`). No corpus or
WPS reference uses it, so pagination/pixels are unchanged.

## Multi-paragraph field blocks (2026-10-11)

A `{\field ...}` whose `\fldrslt` is a **block** result (a TOC, for example) used
to be flattened into the body with a `field:multi-paragraph-instruction-dropped`
Loss: the shared model held inline fields only, so the field scope and the cached
paragraph boundaries were dropped. It is now emitted as the complex-field marker
form the model, writer and reader already supported: `w:fldChar begin` +
`w:instrText` + `w:fldChar separate` + the cached result paragraphs (with the
original paragraph props) + `w:fldChar end`, split across the result paragraph
boundaries. A single-paragraph, up-to-date result keeps the simpler
`w:fldSimple` form. `\flddirty` sets `w:dirty` on the begin marker; `\fldedit`
and the legacy `{\*\datafield}` cache are explicit Info rather than Loss.

The renderer's `FieldState` already tracked `fldChar`/`instrText` across runs and
skips the instruction text, so the marker form renders identically to the cached
result (no instruction glyphs, no page/pixel change).

`structure.rs::multi_paragraph_field_becomes_the_complex_marker_form` pins the
marker form and the `["AB", "C", "", "D"]` paragraph boundaries;
`::dirty_field_marker_and_compat_controls_are_not_losses` pins the dirty begin
marker and the Info classification. Corpus receipt
`target/rtf-import/rc08-field-verify2/`: 014's `field:*`/`control:fld*`/
`destination:datafield` Losses are gone (830 -> 796 on 014), the whole corpus
drops 83671 -> 83010 import losses, and every one of the 100 page counts equals
the column-layout baseline (no regression).

## Custom note marks (2026-10-11)

`w:customMarkFollows` (a hand-typed note mark) is unobserved in the corpus and
is not carried by the model. The reader used to drop the attribute silently; it
now keeps the reference but records `w:footnoteReference`/`w:endnoteReference`
`Partial` with an explicit message (`notes.rs::custom_mark_follows_is_partial_not_
silent`). The continuation NOTICE (`\ftncn`/`\aftncn`, distinct from the
`\ftnsepc` continuation separator already mapped) has no Strict element and stays
an explicit `destination:ftncn` Loss. Both are explicit non-support rules, not
silent fallbacks.

## Other open items

- A continuous section break still defers its geometry (and columns) to the next
  page (see "Section column layout"); same-page continuous geometry is the
  remaining shared-layout build.
