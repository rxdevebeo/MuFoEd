# ADR-0007: Writing Strict — one package writer, one loss report, one order function

- **Status:** Accepted (W1–W7 landed; the trade-offs below are the record)
- **Date:** 2026-09-30
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-8-TASK.md` §3 (W1–W7), §7 (SC-1, SC-3, SC-4, SC-10);
  `TZ-STRICT-OOXML-RUST.md` §3.2 (saving in Strict, removed from the MVP),
  §16.8 (licences); ADR-0001 (own ZIP writer), ADR-0004 (WML model),
  ADR-0005 (report)

## Context

Stages 1–7 could read, normalize and render a document, but not **write** one:
`TZ` §3.2 and `STAGE-6-TASK.md` §6 kept saving out of the MVP, and the only ZIP
writer in the tree built fixture packages from string literals
(`xtool/src/main.rs`). Stage 8A adds it, and the reverse direction (PDF → Strict)
cannot exist without it. Four forces shaped the design:

1. **The output must be Strict, not "mostly Strict".** Transitional and Strict
   differ in namespace URIs and in relationship types. A package that mixes them
   is read by Word as Transitional, which defeats the point of the whole project.
2. **Reproducibility is an acceptance criterion (SC-1)**, and a ZIP is a
   container whose bytes depend on entry order, timestamps, compression level and
   the *ids* handed out along the way.
3. **Nothing may be lost quietly (SC-10).** The writer walks a model that
   deliberately does not contain everything a real document can carry (chart
   parts, SmartArt, OLE embeddings, custom XML).
4. **A `Document` carries no bytes.** The WML model references media by part id;
   the bytes live in the OPC package. So the writer needs a second input, and
   must say what it does when that input is absent.

## Decision

### One entry point, one report type, one invariant

`write_package(&Document, Option<&dyn Source>, &WriteOptions) -> WriteOutput`
is the only way to produce a package. `WriteReport` is a **type alias for the
Stage-6 `NormalizationReport`** rather than a parallel struct: the «no silent
loss» invariant is the project's mechanism for SC-10 and must not exist twice,
and reusing the type means the converter crate (`strict-ooxml-convert`) reports
losses with the same vocabulary as the normalizer.
`verify_no_silent_loss(&WriteReport)` is that invariant, and it is part of the
contract rather than a debug print.

`Source` is a trait with exactly two methods — `read_media(&PartId)` and
`relationship(&PartId, &str)` — implemented for `opc::Package`. It is what lets
the writer round-trip a document *and* what a future converter needs to resolve
hyperlink targets; `None` is a supported state and means «no bytes, no
relationships», not «no document».

### Strict only

Every namespace written is the `purl.oclc.org/ooxml` one, checked byte-wise in
the output: the relationships part declares
`http://purl.oclc.org/ooxml/package/relationships` and its `Type` attributes use
`http://purl.oclc.org/ooxml/officeDocument/relationships/...`. The Transitional
pair (`schemas.openxmlformats.org/...`) is what the neighbouring session's rework
had to teach the writer to *recognise* on input; a write never emits it. There is
no «write Transitional» mode and no second pass: the output of a write is already
the target conformance.

### Order is a function, not an accident

`src/package.rs` is where everything that decides *order* lives, for one reason:
SC-1 is a byte comparison and the tempting failure modes are all orderings.

- Relationship ids are handed out by a counter in a fixed sequence
  (auxiliary parts first, then headers/footers in document order, then media in
  `MediaIndex` order), so `rId1` always names the same thing;
- media parts are named by index, and the index comes from the model's
  `MediaIndex`, not from a hash map;
- the ZIP keeps insertion order and writes a fixed timestamp, with the deflate
  level pinned;
- duplicate relationship targets are dropped keeping the first, in order.

No output decision depends on `HashMap` iteration order. This is the same rule
the parser side already follows (ADR-0004), and the reason `strict-profile`
re-writes to identical bytes twice.

### The unexpressible is recorded, not dropped

A construct the writer cannot serialize is emitted into `WriteReport` with the
location it was found at; the CLI turns a non-empty report into exit code 1. An
empty cell in a table still gets its mandatory `w:p`, because `w:tc` ends with a
paragraph and a cell without one is a document a reader rejects.

### W7: pass the parts through, do not model them

A `.docx` can carry parts this project does not model: a DrawingML chart, the
four parts of a SmartArt diagram, the workbook a chart is linked to, custom XML.
Modelling a chart means modelling DrawingML charts, and that is not this
project's work. But the *reference* to those parts lives in
`word/document.xml`, and a document that keeps a reference without the part
behind it is what Word calls unreadable content.

So the writer does the smallest honest thing, and the rule it follows is
**verbatim with one exception**:

- the parts a reference reaches are copied with their names unchanged, together
  with their `.rels` and their content types, transitively — a chart reaches its
  own `.rels`, which reaches a workbook, and stopping at the first hop would
  produce a package with a chart that cannot open its data;
- **names unchanged** is what lets a copied part's `.rels` be copied unchanged:
  its ids are referenced from *inside* the part
  (`<c:externalData r:id="rId3"/>`), so renumbering them would break the part and
  nothing in this project would notice;
- the **one** exception is the OPC relationship namespace of a copied `.rels`:
  a Transitional producer writes
  `xmlns="http://schemas.openxmlformats.org/package/2006/relationships"`, Strict
  renamed it, and a package that mixes the two makes a conformance detector
  report `unknown`. It is a declaration, not content, and rewriting it changes
  no id, type or target;
- a part's *content* is never rewritten. A Microsoft extension inside a chart
  carries a Transitional URI in an attribute **value**
  (`<dsp:dataModelExt minVer="…/drawingml/2006/diagram"/>`), and rewriting a
  value is a semantic edit this writer does not make. The consequence is stated
  rather than hidden: **a written package can contain parts that are not Strict**,
  and the Transitional-URI scan in `normalize_roundtrip.rs` holds a verbatim copy
  to the invariant that applies to it (it *is* the source's bytes) while keeping
  its teeth over everything the writer generates.

For this to work the model has to keep the reference, which it did not:
`Graphic::Chart` and `Graphic::Diagram` were unit variants, so the relationship
ids were dropped at parse time and the writer had nothing to re-point. They now
carry a `ForeignRefs` — the ids in the order the element carries them (one for
`c:chart`, four for `dgm:relIds`). A chart recognised from its
`graphicData/@uri` alone carries none, and is unwritable rather than invented.

What is still not passed through, and why:

- **a part the writer produces itself** — styles, numbering, settings, theme,
  font table, notes, headers, footers: the model is the authority, and copying the
  source's version too would give the package two parts of one name;
- **a part nothing reaches** — an orphan stays out, and that is not a loss;
- **`docProps/core.xml` and `docProps/app.xml`** — metadata *about* the document
  rather than part of it. `_rels/.rels` is written from scratch, so they are not
  in the output; copying a stale `dcterms:modified` would be a claim this writer
  cannot support, so each is **named** in the report instead (`W7.package-properties`)
  rather than dropped in silence. Copying them is a decision for a later increment;
- **a part that cannot be read** — recorded, and the rest of the pass-through
  still happens: one unreadable part must not cost the document the others.

## Consequences

- Round-trip is a **fixed point**: `write(parse(x))` re-parsed equals
  `parse(x)` on the Strict corpus (SC-3), verified structurally and by an
  independent `roxmltree` parse of every part. Getting there took 13 fixes
  contributed by a neighbouring session's rework — `.rels` namespaces, nested
  `m:rPr`, repeated `w:w` in `w:tblW`, `w:sectPr` inside a paragraph, page-border
  colour as an element rather than an attribute, `a:graphicData/@uri` as a value.
  They are all *schema* rules the writer did not know, which is the honest
  summary of what a serializer is.
- WPS `12.1.0.28485` opens a written `.docx` and converts it to PDF (SC-4).
- **Measured before and after W7**, on `strict-profile.docx` (the corpus document
  with a chart, a diagram and a workbook): 8 parts out of 21 and 1 page →
  **19 parts out of 21 and 2 pages**, with the two `[lossy]` records for `c:chart`
  and `dgm:relIds` gone. What is still missing is `docProps`, and it is now
  reported rather than dropped.
- The writer is deliberately *not* a document editor: no diff patches, no
  round-trip of unparsed parts «as is» beyond the pass-through above, no
  Transitional export.

## Alternatives considered

1. **A generic XML tree pass-through** (copy unparsed subtrees verbatim). Rejected:
   it would carry Transitional namespaces into a Strict package and make the
   conformance claim unverifiable.
2. **A separate `WriteReport` struct.** Rejected: SC-10 would then be enforced by
   two mechanisms, and a converter's losses would read differently from the
   normalizer's for no reason.
3. **`zip`/`deflate` from crates.io for writing.** Rejected: ADR-0001 already owns
   the ZIP layer, with limits, and a second implementation would be a second
   answer to «what is a valid package».
4. **Byte-stable output via a canonical XML writer (e.g. `libxml` bindings).**
   Rejected: native dependency, and the determinism requirement is already met by
   fixing the order, which is where the bugs actually were.

## Validation

- `strict-ooxml-write/tests/roundtrip.rs` — SC-3 fixed point, Strict namespaces
  detected by an independent check, every part well-formed per `roxmltree`.
- `strict-ooxml-write/tests/normalize_roundtrip.rs` — normalize(parse(serialize(x)))
  round trip, which is what caught the 13 schema rules; and the Transitional-URI
  scan, with the verbatim-copy exemption stated in the test.
- `strict-ooxml-write/tests/passthrough.rs` — W7: the chart, the four diagram parts,
  the workbook and `webSettings` are in the written package; every id the body
  carries resolves through the written `.rels` to a part that is there; a copied
  part's own ids are not renumbered; the write is a fixed point; without a source
  package the references are **refused** and no dangling `r:id` is written; an
  unreadable part is named; and the exit criterion — **2 pages before and after**,
  measured with the layout engine.
- Unit tests in `body.rs`/`xml.rs` — `xml:space="preserve"` at either end of a
  run, empty paragraph, spacing.
- SC-1 — two writes of the same `Document` are byte-identical.
- SC-4 — manual WPS run, recorded in the Stage-8 acceptance.
- `cargo run -p strict-ooxml-cli -- write <file.docx>` — exit code 1 with the
  loss list when anything could not be expressed.
