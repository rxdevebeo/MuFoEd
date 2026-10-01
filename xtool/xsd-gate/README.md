# The XSD gate

`STAGE-10G-TASK.md`. This directory is the gate: an instrument that says what our
written packages are worth against the official ECMA-376 Strict schemas, as
opposed to what our own tests say about our own output.

```
python xtool/xsd-gate/xsd_gate.py
```

That is the whole command, locally and in CI. It downloads the schema set the
first time, patches a copy in your cache, writes every corpus document through
our own `write`, validates the assembled packages, prints input and output per
document, and exits non-zero while any violation of ours is left.

## Why a Python script and not a crate

There is no XSD validator in the Rust ecosystem. Writing one costs more than the
whole order, and binding libxml2 natively is a native dependency, which this
project refuses (ADR-0009 refused `pdfium-render` for exactly that reason). The
precedent is already in CI: `strict-ooxml-report --test schema` validates the
report's schema with an independent Python oracle too.

So the gate is a tool and a CI step. **It adds nothing to the crate graph.** No
`Cargo.toml` changed when it was written, `cargo deny` has no opinion to form
about it, and `lxml` lives in the CI image - which is what `requirements.txt` is
for.

## Why the schemas are not in the repository

ECMA-376-1 5th edition carries no licence grant of its own. Its copyright page
does not exist - page 2 of the PDF is genuinely blank, with no content stream at
all - and a search for "BSD" across all 5039 pages returns nothing, so the grant
in `Ecma_Policy_on_Submission_Inclusion_and_Licensing_of_Software.pdf` never
attaches: that grant needs a legend on the standard's copyright page. What
applies is ECMA's default copyright notice, and it is explicit:

> However, the content of this document itself **may not be modified in any way**,
> including by removing the copyright notice or references to Ecma International

and the FAQ on republication requires deliverables to be published "unchanged,
and up to date". Hence:

- the archive is downloaded at run time and its SHA-256 is pinned in
  [`schemas.toml`](schemas.toml);
- the **patched copy lives in your cache**, beside the pristine extraction, and
  the diff of the patch is printed on the first run;
- `no_ecma_bytes.py` walks the tree and fails on any `.xsd` that is not ours.

`cargo deny` cannot have a green opinion here and is not asked for one: the
notice has no SPDX identifier, so a licence scanner has nothing to recognise -
which is precisely why nothing is vendored for it to inspect.

## What the patch is, and why it cannot live in memory

Two things, and both were forced by a failure rather than chosen:

1. `strict/wml.xsd` declares `beforeAutospacing`, `afterAutospacing` and
   `nlCheck` as `s:ST_OnOff` with `default="off"`, while Strict `ST_OnOff` is
   `union(xsd:boolean)` - the `on`/`off` enumeration exists in the Transitional
   set only. Exactly three such attributes exist across all 21 XSDs, and without
   the fix `wml.xsd` does not compile at all. The gate verifies that those three,
   in that file, are what it patched, and stops if the set ever changes.
2. The archive has no `xml.xsd`, and `wml.xsd`, `dml-wordprocessingDrawing.xsd`
   and `shared-math.xsd` reference `xml:space`/`xml:lang` by `ref=` while
   declaring the import with no `schemaLocation`. Our own
   [`xml.xsd`](xml.xsd) declares the four attributes the XML Namespaces
   specification fixes. That is a **complement**, not an edit - the notice permits
   one and forbids the other.

The patch has to exist on disk. An in-memory-only edit was the first design and it
does not work: libxml2 resolves schema `import`s through its own catalog, which
lxml's `Resolver` cannot intercept, so `wml.xsd` fails to compile on `xml:space`
no matter what the tree looks like.

## What the gate refuses to do

**It may not report from a partial run.** The first version of this harness caught
`XMLSchemaParseError` and passed, and consequently reported the corpus clean that
it had not measured: `wml.xsd` was the schema that would not compile, so
`word/document.xml`, `styles.xml`, `numbering.xml`, `settings.xml`, `hdr` and
`ftr` were never validated - precisely the parts the gate exists to judge. Numbers
are published only when zero schemas failed to compile, and the count of skipped
parts is printed every run whether or not it is zero.

**It may not call an extension ours.** Strict conformance is defined on the
post-MCE part (ECMA-376 Part 1 §2.1 clause ii), so `mce_process()` runs first and
every message lands in one of three baskets: `schema`, `extension`, or
`mce-artifact`. The third cannot appear in a correct run, and that is the point -
a run which skipped MCE processing would be caught instead of reported clean.

**It may not judge the model.** Validation is over the assembled package. An
extension can arrive through an `Opaque*` node or through pass-through, and a gate
that looked at the model would never see it.

## Four controls it runs before it believes itself

A validator that reports zero for an invalid document reports zero for everything.
The gate proves it can tell the two apart - a minimal `w:document` must come out
clean, a `w:pPr` with `w:pBdr` before `w:tabs` must not, and `w14:paraId` must be
filed as an extension with and without `mc:Ignorable` declared.

## The registry

[`registry.toml`](registry.toml) maps every measured message to an `XS-nn` item.
An item closes when its **measured** count is zero, never because someone read
the diff; the audit that produced the file reviewed the writer and found none of
these, because nine of its eleven fixtures were written by this project.

`origin` is the field that decides what a non-zero count means. `ours` fails the
gate. `source` is markup the document's producer shipped and we carry verbatim -
diagram parts are pass-through and unmodelled, so `XS-16` is closed by marking
the source invalid, not by rewriting bytes we promised to preserve. A violation
matching no item is reported as unmatched rather than dropped: a defect with no
name is one nobody is looking for.

## Offline

```
set STRICT_XSD_DIR=D:\somewhere\OfficeOpenXML-XMLSchema-Strict
```

An offline set is used as given if it already has an `xml.xsd`; otherwise a copy is
staged into the cache and patched there, because patching a directory in place
would be modifying the deliverable. With neither network nor cache the gate fails
with a legible error rather than validating nothing.

## Files

| File | What it is |
|---|---|
| `xsd_gate.py` | the gate |
| `schemas.toml` | the schema source: URL, SHA-256, byte count, and the exact patch |
| `xml.xsd` | ours, twelve lines, not ECMA content |
| `xml.xsd.sha256` | its digest, so "ours" is checked and not assumed |
| `registry.toml` | `XS-nn`, with the origin that decides the exit code |
| `requirements.txt` | `lxml`, pinned - in the CI image, never in the crate graph |
| `no_ecma_bytes.py` | G-6, checked rather than promised |
