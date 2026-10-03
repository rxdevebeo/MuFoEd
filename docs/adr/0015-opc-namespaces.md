# ADR-0015: OPC namespaces and relationship types are family-neutral

- **Status:** Accepted
- **Date:** 2026-10-03
- **Deciders:** Strict OOXML maintainers
- **Related:** AUD-20 (`REWORK-AUDIT-2026-10.md`), ADR-0007 §«Strict only» /
  §W7, ECMA-376 Part 2 / ISO/IEC 29500-2, corpus `strict01-sdk.docx`,
  `lo-strict.docx`

## Context

Earlier versions of this project treated the OPC package vocabularies
(`.rels`, `[Content_Types].xml`, core properties) as if they had a Strict
`purl.oclc.org/ooxml/package/...` spelling parallel to WordprocessingML. The
writer and the normalizer rewrote the standard
`schemas.openxmlformats.org/package/2006/...` URIs into those purl forms, and
ADR-0007 recorded the rewrite as the W7 exception.

Real Strict packages from Word (`strict01-sdk.docx`) and LibreOffice
(`lo-strict.docx`) use the openxmlformats package URIs. The purl package URIs
do not appear in ECMA-376 Part 2. A conformance detector that treated the
standard URI as "Transitional" was therefore wrong, and a writer that emitted
purl package URIs was inventing a vocabulary.

## Decision

1. **One OPC vocabulary for both families.** The canonical URIs are:

   | What | URI |
   |---|---|
   | `.rels` namespace | `http://schemas.openxmlformats.org/package/2006/relationships` |
   | `[Content_Types].xml` namespace | `http://schemas.openxmlformats.org/package/2006/content-types` |
   | core properties namespace | `http://schemas.openxmlformats.org/package/2006/metadata/core-properties` |
   | core properties rel type | `http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties` |
   | thumbnail rel type | `http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail` |

2. **Registry.** `package.relationships`, `package.metadata.coreProperties`,
   `package.contentTypes`, and `markupCompatibility` are `family_neutral`:
   `strict == transitional` (the standard URI), and `classify_namespace`
   returns `None` for them. `officeDocument.docPropsVTypes` is registered with
   its Strict/Transitional pair and is *not* family-neutral.

3. **Writer.** `.rels`, core-properties declarations, and the core/thumbnail
   relationship types are written with the standard URIs. Extended/custom
   properties relationship types use the Strict officeDocument forms
   (`…/extendedProperties`, `…/customProperties`).

4. **Normalizer repair.** The former "Transitional package → purl Strict"
   rewrite is removed. Packages that still carry the project's legacy purl
   package URIs are repaired to the standard URIs with
   `T1.namespace-repair`.

## Consequences

- Positive: written packages match Word/LibreOffice Strict OPC spelling; the
  upcoming OPC schema gate (AUD-21) can validate `.rels` and `core.xml`.
- Negative: packages written by earlier project versions need one normalization
  pass (`T1.namespace-repair`) before their OPC URIs match the corpus.
- ADR-0007's claim that Strict renamed the OPC relationships namespace is
  retracted; see the correction there.

## Alternatives considered

- **Keep purl package URIs as a project dialect.** Rejected: they are not in
  the standard and fail against real Strict corpora.
- **Treat openxmlformats package URIs as Transitional signals.** Rejected:
  every Strict package on disk would be "Mixed" or "Transitional".

## Validation

- `classify_namespace` of the standard package URIs is `None`.
- `strict-ooxml-write/tests/opc_oracle.rs` compares written URI sets to every
  `strict-ooxml-core/tests/strict/*.docx` and, for Transitional input, to
  `lo-strict.docx`.
- `rg "purl.oclc.org/ooxml/package" --type rust` is confined to the repair
  branch and its tests.
