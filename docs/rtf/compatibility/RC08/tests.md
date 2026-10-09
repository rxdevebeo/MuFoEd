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

## Other open items

- Columns (`\cols`/`\colno`/`\column`, 2 corpus docs) and first/even/odd
  header/footer regions, note placement/numbering, multi-paragraph fields and
  custom marks are unreviewed.
