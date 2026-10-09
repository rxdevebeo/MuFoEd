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

## Not yet closed

- 072 is still `+34` pages (638 vs 604). Body pages now match line-for-line, so
  the residual is section-start paragraph spacing (the reference book-title gaps
  are ~70-79pt vs our ~52-59pt); it needs a focused paragraph-spacing witness.
- `\chpgn` is an explicit Loss, so the header reads `Ruth (Page )` instead of
  `Ruth (Page 2)`. Per the plan, a dynamic field must be recomputed by an
  explicit renderer function, not faked at import.
- First/even/odd regions, note placement/numbering, multi-paragraph fields and
  custom marks are unreviewed.
