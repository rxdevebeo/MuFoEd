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

## Other open items

- Section column **layout** in the shared renderer (model + `w:cols` mapping are
  done; a multi-column section still paints one column).
- Note placement/numbering (`\ftn*`/`\aftn*`; most corpus uses are the OOXML
  defaults) and multi-paragraph field block results (a shared model change) and
  custom marks are unreviewed.
- `field-instruction-control:*` (~40k events) are formatting controls inside
  `\fldinst` that are correctly kept out of the instruction text but reported as
  Loss; whether to reclassify them as explicit ignored-formatting Info is open
  (the plan forbids a blanket rename).
