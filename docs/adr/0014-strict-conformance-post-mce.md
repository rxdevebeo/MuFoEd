# ADR-0014: Strict conformance is defined post-MCE — extension namespaces do not go into a Strict package

- **Status:** Accepted
- **Date:** 2026-10-01
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-10-TASK.md` §2.3 (the schema findings), §2.4, §3 (phase
  10A), §7.4; `STAGE-8-OPEN.md` (O-14); ADR-0007 (writing), ADR-0011 (MSRV),
  ADR-0015 (coherent mutation), ADR-0016 (the writer becomes checkable)

## Context

`strict-ooxml-write` emits `w14:paraId` and `w14:textId` on every paragraph that
carries them (`strict-ooxml-write/src/body.rs:43-52`), and declares the `w14`
namespace in `document.xml` for the purpose (`parts.rs:27,32`, `package.rs:673`).
This project writes **Strict** — ISO/IEC 29500-1, ECMA-376 Part 1 — and has
gates that assert the written package is Strict:

- `a_written_package_is_strict` (`strict-ooxml-write/tests/roundtrip.rs:117-167`)
  checks namespace URIs, relationship types and content types;
- `no_written_part_carries_a_transitional_uri`
  (`normalize_roundtrip.rs:493-556`) scans for Transitional URIs.

Both gates pass on packages containing `w14` attributes, because `w14` is not a
Transitional URI. The question nobody had asked is whether `w14` belongs in a
Strict package at all. The repository's own doc comments disagreed with each
other about it (`ids.rs:32` said "Sixteen-digit paragraph identifier",
`ids.rs:49` said "Eight-digit text identifier") and neither cited anything.

## The evidence

The ECMA-376 5th edition archives were downloaded from
`ecma-international.org` (Part 1: 43 501 721 bytes; Part 4: 8 471 820 bytes),
unpacked, and both the XSD set and the RELAX NG set were searched. Part 1's PDF
(5 039 pages) and Part 4's (1 553 pages) were extracted to text and searched
too.

| Question | Answer |
|---|---|
| Type of `w14:paraId`, `w14:textId` | `w:ST_LongHexNumber` ([MS-DOCX] §2.6.2.3, §2.6.2.4) |
| `ST_LongHexNumber` | `<xsd:restriction base="xsd:hexBinary"><xsd:length value="4"/></xsd:restriction>` — four octets, **eight** hex digits. ISO/IEC 29500-1 **§17.18.50**, *ST_LongHexNumber (Eight Digit Hexadecimal Value)*; identical in `strict/wml.xsd:26` and `transitional/wml.xsd:26` |
| `ST_ShortHexNumber` | `xsd:length="2"`, four digits. §17.18.79 |
| Occurrences of `paraId`, `textId`, `w14` in ECMA-376 | **Zero** — across 23 Strict XSD, 27 Transitional XSD, both RELAX NG sets, and both PDFs' full text |
| Strict or Transitional? | **Neither.** A Microsoft extension in `http://schemas.microsoft.com/office/word/2010/wordml`, documented only in [MS-DOCX]. Corroborated by `OfficeAvailability(Office2010)` on both Open XML SDK properties — `Office2010`, not `Office2007` |
| Could `xsd:any` / `anyAttribute` admit them? | **No.** Zero `anyAttribute` and zero `xsd:any` in the Strict schema; `CT_P` declares only the five `rsid*` attributes |

And the definition that settles it — ECMA-376 Part 1 §2.1 *Document
Conformance*, clause (ii):

> After the removal of any extensions by an MCE processor as specified in
> ECMA-376-3, the part is valid against the strict W3C XML Schema (Appendix A)

**Strict conformance is defined on the post-MCE part.** A `w14` attribute is an
extension; a conforming MCE processor removes it. So the package this project
writes is not Strict-conforming *as written* — it becomes Strict only after a
processor deletes our own attributes.

Three further faults sit in those five lines:

1. `00000000`, which the writer substitutes when `text_id` is absent
   (`body.rs:50`), violates [MS-DOCX] §2.6.2.4: "Values MUST be greater than 0
   and less than `0x80000000`." Zero is not greater than zero.
2. `mc:Ignorable="w14"` is never declared, although `NS_MC` is imported
   (`package.rs:29`) and `mc` is declared (`:673`). [MS-DOCX] §2.2.4: the
   namespace prefix "MUST be specified in an Ignorable attribute".
3. `paraId` uniqueness is per **document part**, with an exception across
   `mc:AlternateContent` Choice/Fallback ([MS-DOCX] §2.6.2.3, ISO/IEC 29500-1
   §17.17.3) — a rule a validator must know or it will reject valid documents.

Meanwhile the project already disagrees with itself elsewhere:
`strict-ooxml-core/src/normalize/tables.rs:349-352` lists the `w14` namespace in
`IGNORABLE_EXTENSION_NAMESPACES`, so Transitional normalization **removes**
`w14` — while the writer adds it.

And the good news, which matters just as much: the `w:rsid*` family
(`rsidRPr`, `rsidR`, `rsidDel`, `rsidP`, `rsidRDefault`, `rsidTr`, `rsidSect`)
is declared `ST_LongHexNumber` on `CT_P`, `CT_Row` and `AG_SectPrAttributes` in
**both** Strict and Transitional. Revision-save ids are legitimate Strict
content, they are simply never validated here.

## Decision

### A Strict package contains no extension-namespace attributes

The writer stops emitting `w14:paraId` and `w14:textId` into a package it
declares Strict. This is a statement about **the document class**, not a repair
of two attributes: a Strict document does not carry extension attributes, so
the correct value is not "a well-formed `paraId`" but "none".

The model keeps both fields. Reading them is necessary — `para_id` identifies a
paragraph across saves, and an editor needs to know which paragraph it is
looking at. The decision to *write* them belongs to the writer, and it can
change (a future Transitional output would write them, plus
`mc:Ignorable="w14"` as [MS-DOCX] §2.2.4 requires).

### The extension scan is a different check from the Transitional scan

They are not the same list and they do not find the same things: `w14` is not a
Transitional URI, so a Transitional scan can never see it. `conformance_detected`
gains a third state — **Strict with extensions** — because today's two states
cannot express "the parts are Strict, and this package also carries vendor
attributes".

Gates that assert Strictness must run **over the assembled package**, not over
the model: `w14` can arrive through `Opaque*` nodes or through pass-through.

### What is actually validated

`w:rsid*` as `ST_LongHexNumber` — exactly eight hex digits, the schema's
`length` facet expressed in bytes being the same constraint as the prose's
"eight hexadecimal digit(s)". `ST_ShortHexNumber` (four digits, §17.18.79)
applies to `w:sym/@w:char`, `w:tblLook/@w:val` and `w:stylePaneFilter/@w:val`.
`paraId`/`textId` keep a uniqueness check, scoped to the document part, with
the `mc:AlternateContent` exception — useful when reading, and free while not
writing.

### The official XSDs are not vendored, and not patched on disk

Two findings, and they combine into a decision that looks like neither would
alone.

**The ECMA-376-1 5th edition has no licence grant of its own.** Its copyright
page does not exist — page 2 of the PDF is genuinely blank, with no content
stream at all — and a search for "BSD" across all 5 039 pages returns nothing. So
the BSD grant from `Ecma_Policy_on_Submission_Inclusion_and_Licensing_of_
Software.pdf`, Exhibit A, never attaches: that grant requires a legend on the
standard's copyright page. What applies is the **default copyright notice**, and
it is explicit:

> However, the content of this document itself **may not be modified in any way**,
> including by removing the copyright notice or references to Ecma International

and the FAQ on republication:

> for deliverables published under the Ecma default copyright notice it is
> required that they are **published unchanged, and up to date**

**And the Strict schema set genuinely needs a patch.** `strict/wml.xsd:599,
602, 672` declare `beforeAutospacing`, `afterAutospacing` and `nlCheck` as
`s:ST_OnOff` with `default="off"`, while Strict `ST_OnOff` is
`union(xsd:boolean)` with no `on`/`off` enumeration — that enumeration is in the
Transitional set only. Exactly three such attributes exist across all 21 Strict
XSDs. Any conformant XSD processor rejects the schema as shipped. This is not an
opinion: the harness cannot compile `wml.xsd` without working around it.

Vendoring a patched copy is therefore the one option that is both useless
(`cargo deny` cannot recognise a licence with no SPDX identifier, and the files
are not Cargo dependencies anyway) and forbidden by the notice. So:

- **The schemas are not vendored.** No ECMA byte enters the repository. A
  non-default feature downloads the archive, verifies its SHA-256, and extracts
  `OfficeOpenXML-XMLSchema-Strict.zip` into the user's cache.
- **The patch is a patched copy in that cache, not an in-memory edit.** An
  in-memory-only patch was the first design and it does not work: libxml2
  resolves schema `import`s through its own catalog, which lxml's `Resolver`
  cannot intercept, so `wml.xsd` fails to compile on `xml:space` no matter what
  the tree looks like. The patched copy therefore exists on disk — in the
  user's cache, with the pristine extraction left beside it. Nothing patched is
  ever committed.
- **One addition, not a modification.** The ECMA archive contains **no
  `xml.xsd`**, and `wml.xsd`, `dml-wordprocessingDrawing.xsd` and
  `shared-math.xsd` reference `xml:space`/`xml:lang` by `ref=` while declaring
  the import with no `schemaLocation`. This project supplies its own
  twelve-line `xml.xsd` declaring the four attributes fixed by the XML
  Namespaces specification. That is not ECMA content, so it is a complement —
  which is what the notice permits, as opposed to editing their deliverable.
- **The defect is recorded as a defect in the schema**, with file, line and
  attribute name, rather than absorbed silently into a vendored copy.
- **An offline build works from `STRICT_XSD_DIR`**, and a build with neither
  network nor cache fails with a legible error rather than validating nothing.
- **The harness may not report from a partial run.** This is in the decision
  because the first version of the measuring script caught schema-compilation
  failures and passed, and consequently reported a corpus as clean that it had
  not measured at all — it had skipped `document.xml`, `styles.xml`,
  `numbering.xml`, `settings.xml`, `hdr` and `ftr`, which are precisely the parts
  the gate exists to judge. Numbers are published only when zero schemas failed
  to compile, and the count of skipped parts is printed.

## Consequences

- "The written package is Strict" stops being a claim about namespaces and
  becomes a claim about the whole part, which is what the standard means.
- Two documents that were byte-identical before this change are byte-identical
  still; documents carrying `para_id` change. Any golden that pins those bytes
  is a fixture that was pinning a defect, and is regenerated with the reason
  recorded.
- `w14:paraId` no longer survives a round trip — but it did not survive Strict
  before either, it just did so illegally. A `lossy` record names it, per
  ADR-0007's rule that the unexpressible is recorded, not dropped.
- The contradiction with `IGNORABLE_EXTENSION_NAMESPACES` closes: the normalizer
  removes `w14`, and now so does the writer.
- `w:tr/@w14:paraId` is also permitted by [MS-DOCX] §2.2.4 and `TableRow`
  (`model/block.rs:51-59`) has no such field, so that attribute is silently
  dropped on parse. Whether to add the field is a separate question
  (`Q-E6`), because after this decision the loss becomes deliberate rather than
  accidental — and `w:tr` itself is valid Strict.
- Nobody has an authoritative statement about how Word treats `w14` in a file
  labelled Strict (`Q-E5`). That is a behavioural question, answerable only by
  measurement: open the packages before and after and confirm edits work.

## Alternatives considered

1. **Keep the attributes and declare `mc:Ignorable="w14"`.** Rejected: it is the
   Transitional interoperability mechanism, and it works by making the
   attributes *removable*. It does not make the part Strict — clause (ii)
   requires the extensions to be gone. Declaring `Ignorable` while calling the
   document Strict is the same category of mistake as declaring `Conformance:
   Strict` and shipping Transitional URIs.
2. **Keep the attributes, undeclared and unignorable.** Rejected: this is what
   the code does now, and it fails §2.1 clause (ii) while also breaking
   [MS-DOCX]'s own `Ignorable` requirement. It is the worst of the three
   because it is neither conformant nor interoperable.
3. **Validate `paraId` to `ST_LongHexNumber` and keep writing it.** Rejected:
   it produces a well-formed attribute in a document that should not carry
   attributes. A validator that only fixes well-formedness would have kept this
   bug alive indefinitely, which is the reason the invariant is `I11` ("no
   extension attributes in a Strict package") and not "8 hex digits".
4. **Make this a normalizer concern instead of a writer one.** Rejected: the
   normalizer already treats `w14` as ignorable and removable, which means the
   writer is the only component that puts it back. Fixing it in the normalizer
   would leave the writer's output non-conformant for every caller that does not
   normalize.
5. **Vendor the schemas under a permissive mirror's licence.** There are
   mirrors — `scanny/python-pptx` (MIT), `UseJunior/safe-docx` (Apache-2.0),
   `baidu/amis` (Apache-2.0) — and python-pptx has already stripped the very
   `default=` initialisers we would have stripped. Rejected: it does not escape
   the ECMA grant, it launders it. python-pptx's edit is undocumented in-repo,
   and shipping "ISO/IEC 29500-1 schema files" from a repo whose own licence
   does not mention ISO/IEC would be a claim we cannot support. The
   mirror-with-a-cleaner-licence argument turned out to be true and irrelevant:
   not vendoring is available, and it is strictly better than every variant of
   vendoring.

## Validation

- `strict-ooxml-write/tests/strict_conformance.rs` — over the assembled package:
  no attribute or element from an extension namespace in any part declared
  Strict; and the Transitional scan still green, as a separate assertion.
- `strict-ooxml-write/tests/rsids.rs` — `w:rsid*` at eight hex digits validates;
  two, six, and non-hex values are rejected with the `length` facet named.
- `strict-ooxml-convert/tests/convert.rs` — SC-5 still holds: ≥ 99.9 % of the
  characters reach the written package after the change.
- `strict-ooxml-write/tests/roundtrip.rs::a_written_package_is_strict` and
  `normalize_roundtrip.rs::no_written_part_carries_a_transitional_uri` — kept,
  and now paired with the extension scan so that a future `w15`-style
  regression cannot pass as "still Strict".
- WPS `12.1.0.28485` opens packages before and after (`Q-E5`, SC-E6 of stage 10).
