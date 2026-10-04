# ADR-0009: Reading PDF — `lopdf` for objects, our own for meaning

- **Status:** Accepted
- **Date:** 2026-09-30
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-8-TASK.md` §2 (dependencies), §5 (C1–C9), §7 (SC-2, SC-5,
  SC-9); ADR-0001 (own ZIP layer), ADR-0002 (own XML layer), ADR-0004, ADR-0008

## Context

The reverse direction needs a PDF reader, and the project's tradition (ADR-0001,
ADR-0002) is to own the layer where the *meaning* is decided. PDF splits neatly
in two:

- **object plumbing** — cross-reference tables, object streams, stream filters
  (`FlateDecode`, `LZWDecode`, `ASCIIHex`, `ASCII85`, `RunLength`), and the
  content-stream tokenizer;
- **meaning** — what a page is, where a glyph sits, which font program a code
  belongs to, whether an image's samples are 8-bit RGB or a 16-bit mask, how much
  of a Bézier a polyline may flatten.

Rewriting the first is unpaid work with no correctness argument; delegating the
second would put this project's acceptance criteria in someone else's release
schedule. `mupdf` is AGPL and out; `pdfium-render` needs a native binary.

The budget is not a detail here: a PDF is attacker-controlled input, and the
work order demands `Err` rather than a panic on hostile files (SC-2).

## Decision

### `lopdf` 0.45 for the object model, everything else ours

`lopdf` gives objects, the xref, stream decoding and the content-stream
*tokenizer*. The graphics-state machine, the text state machine, the font layer
(`ToUnicode`, `/Differences`, `/Widths`, `/W`, `/DW`, CID fonts), image decoding
and curve flattening are written here, in `strict-ooxml-pdf`.

`lopdf` 0.45 specifically (not 0.37): it is MIT, and the version line that
matters is the one with the fixes this reader depends on. Its MSRV (1.88) is part
of why the workspace floor is where it is (ADR-0011).

### Coordinates: points, y from the top

Output is in points with **y measured downwards from the top of the page**,
because that is the space the WML model is written in (twips from the top margin)
and a converter that flipped every coordinate on the way out would be a second
place to get the flip wrong. PDF's y-up space is an intermediate step inside the
interpreter and nowhere else — including for paths and images, which arrive
through the same `height - y` conversion.

### A budget on the way in, not on the way out

`PdfLimits` caps pages, decompressed bytes per page, operators, glyphs per page,
decoded image bytes, path points and fonts. The caps are checked *before* the
work they bound, including the flattened-point count of a path. Errors are
values: no `unwrap`, no `expect`, no `panic!` on a library path.

### Distrust the producer's numbers, and say which ones you distrusted

Two rules that came out of bugs rather than taste:

- **A glyph whose character cannot be mapped is dropped and counted, never
  replaced.** `U+FFFD` does not go into the page's text, because a substituted
  character in a converted document is a silent lie; `ReadReport::unmapped_glyphs`
  is the honest form of the same fact.
- **A width the font did not state is a different kind of number from one it
  did.** `Width::{Stated(f64), Estimated}` exists because a missing `/DW` was
  being reported as `Stated(1000.0)` — plausible and wrong, and impossible to
  tell apart from a real value downstream. Same reason `Glyph::width` is carried
  at all (Q-15): the reader knows the advance, and a consumer that has to
  recompute it is a consumer that can get it differently.

Encryption is not supported (RC4/AES): `Err`, no panic.

### The geometry oracle lives in the reader

A round trip through our own writer proves that two halves of this project agree
with each other, which is not evidence about either. So the oracle for geometry
is `strict-ooxml-pdf/tests/geometry.rs`, driven from the reader's own numbers,
and the writer's test that used to hand-roll a content-stream tokenizer was
deleted in favour of it. The rule for the future: **the reader is the only place
where «what does this number mean here» is a question with an answer.**

Curves are flattened to polylines at `FLATTEN_TOLERANCE_PT = 0.125`, a quarter of
the SC-9 budget, because the consumer of the path is a *different* renderer and a
quarter of the budget spent here is a quarter the consumer never gets.

## Consequences

- `strict-ooxml-pdf` is the only crate that understands PDF, and it is the oracle
  for every claim about a PDF's contents — including claims made by the writer
  that produced it.
- Reading is honest about what it does not know: unmapped glyphs, estimated
  widths, unsupported image filters and layouts (`Reject::{UnsupportedFilter,
  UnsupportedLayout, Incomplete, Broken, TooLarge}`) all have names and reach the
  report. `/JPXDecode` is carried (`hayro-jpeg2000` → 8-bit `Encoded::Raw`); a
  damaged JPX stream is `Broken`, not `UnsupportedFilter`.
- No third-party PDF corpus is committed yet (`Q-9`): the geometry tests run
  against PDFs this workspace wrote. That is a real gap — a producer's quirks
  (hex strings, inline dictionaries, a `cm` covering the page) were only found by
  hand — and it is why §4 of the hand-off lists a licensed corpus as required
  before this reader is called finished.
- 39 tests in the crate; the whole phase is 8C in `STAGE-8-OPEN.md`.

## Alternatives considered

1. **`mupdf` / bindings.** Rejected: AGPL-3.0, incompatible with the project
   licence (TZ §16.8).
2. **`pdfium-render`.** Rejected: a ~20 MB native binary fetched from an external
   registry, which breaks determinism, `cargo-deny` and the three-OS CI matrix —
   and a separate text extractor would still be needed for the geometry.
3. **`pdf-extract`.** Rejected: no glyph geometry, and glyph geometry is the
   entire point of a converter that reconstructs layout.
4. **Writing the xref/filter layer too.** Rejected: no correctness argument, and
   ADR-0001/0002 already own the analogous layers elsewhere for the same reason —
   not because reimplementation is a virtue in itself, but because those layers
   had no library that did the job under this project's licence and limits.

## Validation

- `strict-ooxml-pdf/tests/geometry.rs` — positions at 0.01 pt, text and total
  advance, against a PDF written by `strict-ooxml-render-pdf`.
- Unit tests in `content.rs` — matrix composition against a known product (Q-13:
  the row/column mix-up that shifted every glyph and squared every size), `cm`
  chains, flattening tolerance, the graphics-state machine.
- Unit tests in `fonts.rs` — `ToUnicode` `bfrange` sections by absolute cursor
  (Q-12), `/Differences`, `Width::Estimated` for a missing `/DW` (Q-16).
- `strict-ooxml-convert/tests/convert.rs` — SC-5 end to end: ≥ 99.9 % of the
  characters in the PDF's text layer reach the written package.
- The reader is also the *input* side of the round trip in
  `strict-ooxml/tests/stage5b_corpus.rs` and the CLI's `inspect`/`from-pdf`.
