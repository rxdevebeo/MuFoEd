# ADR-0018: Tracked-change model for `w:ins` / `w:del` / `w:moveFrom` / `w:moveTo`

- **Status:** Accepted
- **Date:** 2026-10-03
- **Deciders:** Strict OOXML maintainers
- **Related:** AUD-43 (`REWORK-AUDIT-2026-10.md`), `ADR-0004` (WML model),
  `ADR-0006` (render), `ADR-0007` (writing)

## Context

Tracked-change containers were flattened into ordinary runs and recorded as
`partial` ("tracked change container flattened"). Deleted and moved-from text
then rendered as live content, and the writer could not restore the wrappers.
Property-change history (`w:rPrChange`, `w:pPrChange`, …) is a separate problem
and stays out of the content model.

## Decision

1. **Model.** `Run::revision: Option<Revision>` and `Paragraph::revision:
   Option<Revision>`, where
   `Revision { kind: RevisionKind { Insert, Delete, MoveFrom, MoveTo }, id,
   author, date }`. Inline and block `w:ins` / `w:del` / `w:moveFrom` /
   `w:moveTo` stamp every nested run (and the paragraph mark for block wrappers).
   `w:pPr/w:rPr/w:ins|w:del` sets `Paragraph::revision` only. `w:delText` /
   `w:delInstrText` remain ordinary text / instruction content on runs whose
   revision is a deletion.

2. **Render.** Default view is **Final**: runs with `Delete` / `MoveFrom` are
   not drawn; `Insert` / `MoveTo` draw as normal text. `RenderOptions::revisions:
   RevisionView { Final, Original }` selects the opposite filter for Original.
   Visual markup of revisions (underline / strikethrough) is **out of scope**.

3. **Writer.** Consecutive runs sharing the same `Revision` are grouped into one
   wrapper with the original attributes; deleted text is emitted as `w:delText`
   (and `w:delInstrText` for instructions). Paragraph-mark revisions are written
   inside `w:pPr/w:rPr`.

4. **Property changes.** `w:rPrChange`, `w:pPrChange` and `w:ins`/`w:del` inside
   a non-paragraph-mark `w:rPr` are recorded as `partial` with message
   "property change history dropped" and are not modelled.

## Consequences

- Round-trips preserve content revisions without silent undelete.
- Callers that need "show original" set `RevisionView::Original`.
- Review markup (colours, balloons) remains a future decision.

## Alternatives considered

- Keep flattening and only record `partial` — rejected; deleted text stayed live.
- A separate `Inline::Revision` tree — rejected; every consumer would have to
  unwrap, and run-level stamping matches how Word stores markers on content.

## Validation

Unit/integration tests: model stamps runs; Final SVG omits deleted text and
keeps inserted text; Original does the opposite; writer round-trip equality of
models; XSD gate on a document with revisions does not gain new violations.
