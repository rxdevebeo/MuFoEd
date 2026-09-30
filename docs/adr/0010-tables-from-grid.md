# ADR-0010: A table is a grid of ruling lines — and a missing rule is a merge

- **Status:** Accepted
- **Date:** 2026-09-30
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-8-OPEN.md` `O-9a`, `P-6`, `Q-20`, `Q-21`;
  `SESSION-HANDOFF-STAGE8.md` §4 (invariants 2, 6, 7), §5.3;
  `strict-ooxml-convert/src/tables.rs`

## Context

A PDF states glyph origins, advances, sizes, colours, paths and image boxes
exactly. It states nothing about paragraphs, headings or tables. Stage 8C+
(`O-9a`) has to decide whether the ruling lines on a page are a table, and the
failure modes are symmetric and both bad:

- detect nothing, and a document full of tables comes out as a flat list of
  paragraphs;
- detect everything, and a chart, a form frame or a boxed figure comes out as a
  table that *looks* right in the output and is not one — and the caller cannot
  tell without reading the report.

The first version of this increment detected a grid (four thin long paths) and
refused to build it, reporting `table.detected`. That was honest and it was not
useful, so the question became: **what would have to be true of a grid for
building a table out of it to be defensible?**

Two traps shaped the answer, and both were found by tests rather than by reading
the code:

1. **A crossing test is a point test.** The first connectivity check asked
   whether a vertical rule's y-interval overlapped a horizontal rule's x-interval
   — two intervals compared to each other, which is not the question. Two tables
   stacked on one page happened to satisfy it, merged into one figure, failed the
   frame test, and **both tables vanished silently**. The test is now: the point
   `(vertical.x, horizontal.y)` lies inside both spans.
2. **A check that fires always is worse than no check.** With a two-rule
   threshold, a single ruling line — the separator under a running header, which
   every document with a header has — was reported as a rejected table on every
   page, so `is_lossless()` was false everywhere and the CLI warned about
   documents that contain no table at all. A report nobody can act on is a
   report nobody reads.

## Decision

### The chain, and where each judgement is allowed

```text
paths -> rules -> components -> a grid of boundaries -> cells -> paragraphs
```

- **paths → rules.** A ruling line is long and thin: stroked with a pen no thicker
  than `max_rule_thickness`, or filled as a box no thicker than that. A filled
  cell is a background, not a boundary.
- **rules → components.** Two rules belong to the same figure when they meet at
  their **ends** (a producer that draws each cell's four edges) or when they
  **cross** (a producer that draws whole column and row rules through each other
  to avoid a hairline gap at a corner). Missing the second kind loses a table on
  an ordinary document.
- **components → grid.** A figure is a table when it bounds at least
  `min_columns` columns and `min_rows` rows, its **outer frame runs the whole way
  round** at `min_cover_ratio`, and at least `min_filled_cells` cells carry text.
  A frame round one paragraph is a frame; a grid with nothing written in it is a
  drawing.
- **grid → cells.** A boundary that is *missing* is a merge: no vertical between
  two columns inside a row's band means one cell spans them (`w:gridSpan`); no
  horizontal under a cell means it continues into the row below (`w:vMerge`,
  `restart` on the first row of the run, `continue` on the rest).
- **cells → paragraphs.** A cell is a small document: the same gap/indent/format
  rules as the page, with two differences. The indent is measured **from the
  cell's left edge**, not the page's, and a cell is **never** a heading however
  large its text.

All seven thresholds are public fields of `TableRules`, reachable through
`PdfOptions::tables(..)`, because a threshold is a claim about what documents look
like and a claim nobody can change is a claim nobody can check.

### A line joins a table only if the whole of it is inside the grid

A line whose baseline falls in the grid's rows but which sticks out sideways is a
paragraph beside the table far more often than it is a cell that overflowed.
Splitting it would be a guess in both directions, so it stays in the page flow.

### The geometry we write is the geometry we measured

`w:gridCol` widths are the distances between the vertical rules; `w:tblW` their
sum; `w:trHeight` the row band with rule `atLeast`, because a cell whose text
needs more room must be allowed to have it — `exact` would make the converter
choose a pagination the PDF never had; `w:tblBorders` come from the **measured**
rule thickness and the dominant rule colour, with `w:tblLayout` fixed so an engine
cannot re-fit columns the rules already sized.

### Inference is recorded; refusal is recorded with its reason

A built table writes one `table.inferred` entry (`Inferred`) naming its size, how
many cells carry text and how many merges were inferred. A figure that looked like
a table and was not built writes `table.detected` (`Unsupported`) **with the rule
that rejected it**. A figure that is not even a candidate writes nothing — that is
the second trap above, turned into a rule.

### Where this decision does not apply

This is a decision about **ink**. A source that states structure — HTML
`<table>`, a Markdown table, a `.docx` `w:tbl` — is listened to instead of
reconstructed, and none of these rules apply to it. The general form of that is
invariant 7 of the stage hand-off: *the structure of the source outranks the
geometry.*

## Consequences

- A PDF with bordered tables converts to editable tables, verified end to end: a
  3×3 grid with a spanning header and a vertically merged column comes back out of
  `write` as `w:gridSpan w:val="3"`, `w:vMerge w:val="restart"` and a bare
  `w:vMerge`, with the source column widths recovered to the twip.
- The merges are an **inference from an absence**, the only place in the converter
  where missing data is replaced by a conclusion. It holds on one assumption — a
  merged cell has no rule inside it — and it refuses rather than guesses when the
  row below does not agree (`DanglingSpan`).
- Deliberately not reconstructed, each with the reason in the module docs: a table
  continued across a page break (two figures, each with an open side), a nested
  table (becomes a sibling: the same position says «inside» and «beside»), a rule
  that covers part of a row's width, and `w:tblHeader` (nothing in the ink says
  the first row repeats, and guessing it changes pagination).
- Unmapped cost, stated plainly: the tests run against PDFs this workspace wrote
  (`Q-9` — a third-party corpus is still required). The unit tests therefore draw
  rules by hand, including the overshooting-crossing shape our own renderer never
  produces, because a self-written fixture cannot be the only witness.

## Alternatives considered

1. **Keep detecting and refusing (the first version).** Rejected: honest, and it
   leaves the most common structure in a converted document unbuilt.
2. **Infer the grid from text alignment alone** (columns of x-positions, rows of
   baselines). Rejected: it turns any multi-column page — a form, a two-column
   article, a chart with a legend — into a table, and there is no ink to contradict
   it.
3. **Build the table for every closed rectangle that has two non-empty cells.**
   Rejected: a boxed figure with a caption beside it satisfies it.
4. **A confidence score per figure instead of thresholds.** Rejected for now: a
   score without a decision rule is a number nobody acts on. If a corpus of real
   PDFs (Q-9) later shows the thresholds are systematically wrong, a score with a
   stated cut is the next step — and the thresholds are public precisely so that
   experiment is possible.

## Validation

- `strict-ooxml-convert/src/tables.rs` — 32 unit tests on hand-drawn rules: a
  filled grid, a frame round one paragraph, an empty grid, filled boxes, the
  spanning and the vertical merge, two grids on one page, rules that cross
  without meeting, text outside the grid, a lone rule (no report at all), a
  degenerate figure under `min_columns: 0` (a refusal, not a panic), and
  reproducibility of the whole plan.
- `strict-ooxml-convert/tests/tables.rs` — the full chain: model → PDF (this
  project's renderers) → convert → write → read back with our own reader and with
  `roxmltree`; SC-5 text preservation with tables included; SC-1 byte equality;
  the `TableRules` being load-bearing; a table whose producer drew no borders
  staying paragraphs; the visual mode making no structural claim.
- CLI: `strict-ooxml from-pdf` prints `tables=N` and the inference line; a
  document with no table prints `nothing inferred or lost`.
