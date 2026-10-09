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

- 072 is still `+34` pages (638 vs 604). Body pages now match line-for-line. The
  residual is the gap between adjacent heading paragraphs: the WPS reference
  measures `line + 18 + 18` (it adds `w:before` and `w:after`), while the
  renderer's `pending_after.max(space_before)` gives `line + 18`. A naive
  `max -> sum` change was tried and **reverted**: it moved the calibrated
  `06-strict-math-display` structural extent (`was 5, now 37`) and failed
  `matches_wps_references`. The reconciliation is a shared-layout item: the two
  WPS-derived expectations differ, most likely because Word's
  `contextualSpacing`/same-style suppression applies to the baseline documents.
  Implementing `w:contextualSpacing` (currently ignored by the renderer) is the
  prerequisite; it belongs to the shared P5 metrics/layout queue, not RTF
  mapping.
- First/even/odd regions, note placement/numbering, multi-paragraph fields and
  custom marks are unreviewed.
