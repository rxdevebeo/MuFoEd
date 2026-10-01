# ADR-0012: Reading DjVu — `djvu-rs` subcrates for plumbing, our own for the text layer

- **Status:** Accepted
- **Date:** 2026-10-01
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-9-TASK.md` §2 (dependencies), §2.2 (patents and trademark),
  §3 (the format as it actually is), §4 (J1–J10), §5 (Z1–Z9); ADR-0001 (own ZIP
  layer), ADR-0002 (own XML layer), ADR-0009 (PDF reading — the same split),
  ADR-0011 (MSRV)

## Context

Importing DjVu needs a reader, and ADR-0009 already fixed the shape of the
argument for PDF: split the format into **object plumbing** and **meaning**,
delegate the first, own the second.

DjVu splits more cleanly than PDF did, because DjVu's *plumbing* is genuinely
alien — an IFF-85 container, an adaptive binary arithmetic coder (ZP), a
Burrows–Wheeler-plus-MTF scheme (BZZ), a JB2 symbol dictionary and a
Dubuc–Deslauriers–Lemire wavelet coder (IW44). None of that has a counterpart
anywhere in this tree, and every bit of it has to be bit-exact against files
produced by encoders nobody in this project will ever run.

DjVu's *meaning*, by contrast, is the most valuable thing about it, and it is
where the format is **wrong in the sources**. Half of what circulates about DjVu
is not the format: there is no `SIZZ` chunk (it is `INFO`), no `DANT` chunk
(no "page has text" bit — the presence of `TXTa`/`TXTz` *is* the test), no
`CID ` chunk, no `RLE`, no `n*3±1` length formulas, and the text record is not
`(len, version, flags)` with eight flag bits. An implementation written from the
secondary sources would fail on real files while looking correct.

The licensing constraint is sharper here than for PDF. DjVuLibre is
**GPL-2.0-only** — not "v2 or later" — and every DjVu decoder in existence for
two decades is either it or a binding to it.

## Decision

### Subcrates, not the umbrella crate

`matyushkin/djvu-rs` 0.40 is MIT, edition 2024, MSRV 1.88, and is a clean-room
implementation of the published specification. We take its **subcrates**:

| Crate | Role |
|---|---|
| `djvu-iff` | IFF-85 container: `FORM`, chunk walk, the odd-**offset** pad rule |
| `djvu-zp` | ZP adaptive binary coder (transitive) |
| `djvu-bzz` | BZZ (transitive) |
| `djvu-bitmap`, `djvu-pixmap` | packed 1-bit and RGBA buffers |
| `djvu-jb2`, `djvu-iw44` | the two image codecs, behind feature `djvu-image` |

**One direct dependency.** Its own tree is `thiserror` plus `wide` (Zlib OR
Apache-2.0 OR MIT, already in `deny.toml`'s allow-list) and an optional `rayon`
we do not enable. MSRV 1.89 against our floor of 1.92: the floor does not move
(ADR-0011), and `ci.yml`'s `msrv (1.92)` keeps verifying it as a fact.

The umbrella `djvu-rs` crate is not taken despite carrying everything we need:
it has **47 dependencies**, including `tesseract`, `tract-onnx`, `wasm-bindgen`,
`libc`, `image` and `tokio`. Auditing that tree under `cargo-deny` costs more
than the work we would save.

### The text layer is ours, entirely

`TXTa`/`TXTz` are ours to parse: `BE24` text length, UTF-8, then a recursive
zone tree of 17-byte records (`ztype`, five `BE16` biased by `+0x8000`,
`BE24` text length, `BE24` child count). So are the **deltas** — the part that
is genuinely non-obvious and that the reference decoder is the only
documentation for: `x`, `y` and `text_start` resolve against the *previous
sibling*, else the parent, else absolutely, and the arithmetic differs by
`ztype`. `PAGE`/`PARAGRAPH`/`LINE` resolve from the previous block's lower-left;
`COLUMN`/`REGION`/`WORD`/`CHARACTER` from its lower-right.

The three guards the reference decoder applies are **mandatory**, not
defensive padding: a zone with zero width *or* height is rejected, `text_start`
must be non-negative, and `text_start + text_length` must not exceed the header's
count. A single corrupt byte shifts every subsequent zone, and those three
checks are the only thing that turns that into `Err` instead of a plausible
document full of text in the wrong places.

Zone rectangles are output in **points, y downwards from the top** — the same
coordinate system ADR-0009 chose for PDF, and the same reason: it is the space
the WML model is written in, so the flip happens once, on input, and nowhere on
output.

### Coordinates the format does not give us

`TXTz` yields a tree of word rectangles and their text. It does **not** yield
glyph positions, per-character advances, or bold/italic — the mask carries
shapes with no codepoint mapping. So this reader emits **word boxes**, and a
consumer that needs character geometry cannot have it. See ADR-0013 for what
the converter does instead of inventing it.

`INFO.flags & 0x7` says the page is rotated. Whether the zone coordinates are
stored before or after that rotation is **not stated in the specification**, and
it is settled by measurement on rotated files (`SC-D14`), not by assumption.
Until that measurement exists the assumption is stated in the report as
`Inferred`.

### A budget on the way in

`DjvuLimits` mirrors `PdfLimits`: pages, chunks, chunk bytes, document bytes,
BZZ block, zones, zone depth, decoded text bytes, component files, ink pixels,
JB2 symbols — each checked *before* the work it bounds. A DjVu file is
attacker-controlled input and the work order demands `Err`, not a panic (SC-D2).

`fuzz_djvu` is added to both fuzz jobs. It is the **first** fuzz target on any
reader in this project — `fuzz_pdf` never arrived — so a reader fuzz target is
also a small piece of debt this stage pays off.

### Provenance is recorded per module, or the clean-room claim is unverifiable

The implementation derives from the published specification (DjVu Reference v3,
November 2005, status "Released"), from public wavelet and arithmetic-coding
literature, and from MIT crates. ADR-0012 carries a table: **module → source**.
Two rules come with it:

- **Do not read DjVuLibre sources while writing a codec.** LizardTech Original
  Code is under GPL-2.0; reading it and then writing makes provenance arguable
  in a way that is expensive to disprove later.
- Nobody transcribes an algorithm from DjVuLibre. If someone has read it, that
  module goes to counsel before it ships.

The provenance table is the only artefact that survives the stage, and it is the
reason the decision below is checkable rather than merely stated.

### No DjVu writer, and that is a patent boundary

`STAGE-9-TASK.md` §10 excludes writing DjVu. That is usually a scope decision;
here it is also the reason the patent exposure is bounded. US 6 058 214 expired
2019-01-19 (35 U.S.C. 154(a)(2): twenty years from the 1999-01-19 filing), and
its sole independent claim is an **encoding** method — the patent text itself
says the result "can be decoded according to normal wavelet techniques regardless
of the mask". A read-only implementation does not practise that claim at any
term. The owner accepted the residual risk on 2026-10-01 without commissioning a
freedom-to-operate search (`docs/waivers.toml`, `DJVU-PATENT`), and the three
unresolved items are written into that waiver rather than left in this file.

The "DjVu" word mark is cancelled in the US (serial 75449497, cancelled
2011-01-28 for a missed Section 8). It is used **referentially only**, with an
affiliation disclaimer; the crate name `strict-ooxml-djvu` is referential in the
same way `strict-ooxml-pdf` already is (`DJVU-TM`).

## Consequences

- `strict-ooxml-djvu` is the only crate that understands DjVu, and the oracle for
  every claim about a DjVu file's contents. The text layer in particular is the
  one thing no external oracle in this project can check at the DOM level — see
  the next record.
- The reader produces structure, not glyphs. Everything downstream of a word box
  is a mapping decision, which moves the inference burden from a lossy guess
  (PDF: glyphs → paragraphs) to a declared rule (DjVu: zones → paragraphs). That
  is the whole reason the stage is worth doing.
- Reading is honest about what it does not know: no glyph advances, no weight or
  slant, no vector ruling lines, no tables, no lists. Each has a name in the
  report rather than a plausible substitute.
- Indirect documents are the least covered path in the entire format: **no public
  corpus of them exists**. `SC-D13` is why `djvmcvt -i` is in the corpus plan.
- No FTO search was commissioned. This is a real gap, accepted in writing, and
  `docs/waivers.toml`'s `exit` names the point at which it stops being
  acceptable.
- The corpus is uncommitted (`testdata/` is gitignored), so the permanent guards
  against what the corpus finds live in committed tests, as they did for PDF
  (`tests/resources.rs`).

## Alternatives considered

1. **DjVuLibre, or bindings to it.** Rejected: **GPL-2.0-only**, the same class
   of decision that ruled out `mupdf` in ADR-0009. Also: the reference library
   does not contain the layer-separation code or the highest-ratio JB2 mode
   (LizardTech's own licensing page), so binding it would not even have given us
   a complete decoder.
2. **The umbrella `djvu-rs` crate.** Rejected: 47 dependencies for functionality
   we use a third of, and a `cargo-deny` tree to audit for it. Its subcrates are
   the same project, same author, same licence.
3. **`djvu` (bugzmanov).** Rejected: GPL-3.0-or-later. Its README describes
   itself as AI slop whose algorithms were studied from `djvu.js` and DjVuLibre —
   which is a second, independent reason, and the licence is enough on its own.
4. **`sndjvu_format`.** Rejected despite MIT-or-Apache and a lower MSRV: last
   released 2023-01, 37 % documented, and it is a deliberate "transfer format"
   abstraction that *hides* the codec bytes — which is precisely the layer we
   wanted to delegate rather than re-specify.
5. **Shell out to `djvutxt` / `ddjvu`.** Rejected for the same reason as
   `pdfium-render` in ADR-0009: a native binary, GPL, breaking determinism,
   `cargo-deny` and the three-OS CI matrix. Kept as an **oracle** for
   acceptance, which is a different job.
6. **Write ZP, BZZ, JB2 and IW44 ourselves.** Rejected: no correctness argument.
   ADR-0009 already carved out the precedent in the other direction — those
   layers are not reimplemented where a library does the job under this
   project's licence and limits, and here one does.

## Validation

- `strict-ooxml-djvu/tests/chunks.rs` — the chunk walk against `djvudump`
  output, line for line, including the pad-by-offset rule and the `AT&T`/`SDJV`
  magic between chunks (`SC-D3`).
- `strict-ooxml-djvu/tests/zones.rs` — the zone tree against
  `djvutxt --detail=word`, rectangles to the pixel, on the bundled DjVu
  specification itself (`archive.org/details/DjVu3Spec`), which is the one file
  whose correct text is known (`SC-D4`, `SC-D5`).
- `tests/indirect.rs` — `DIRM` offset array and `INCL` resolution against
  `djvudump`, on files produced by `djvmcvt -i` (`SC-D13`).
- Unit tests in the zone parser — each delta branch, each guard, UTF-8 boundary
  splitting of a zone's byte range, `max_zone_depth` and `max_zones` refusals.
- `tests/rotation.rs` — the measurement that closes the pre/post-rotation
  question (`SC-D14`). Until it exists, this record's coordinate claim is an
  assumption and says so.
- `cargo run -p strict-ooxml-djvu --example corpus_report -- testdata/djvu` —
  a bug-finding instrument, not a fixture; a test that skipped a missing
  directory could not fail.
- The provenance table in this record is maintained alongside the code.
