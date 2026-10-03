# ADR-0017: Per-part NormalizationReport, merge on read

- **Status:** Accepted
- **Date:** 2026-10-03
- **Deciders:** Strict OOXML maintainers
- **Related:** AUD-30, AUD-33, AUD-36 (`REWORK-AUDIT-2026-10.md`),
  `TZ-STRICT-OOXML-RUST.md` §10.9–§10.10, ADR-0005 (Feature Report)

## Context

`TransitionalNormalizer` accumulated one shared `NormalizationReport` behind a
mutex held for the whole of `normalize_part`. Two consequences:

1. **Double counting.** OPC opens `.rels` twice under AUD-23 (raw T0 signal,
   then through the normalizer). Each pass added the same `T2.reltype` hits, so
   `Manual.docx` reported `T2.reltype x16` for eight real mappings. Any part
   read more than once inflated the report.
2. **Serialisation under `parallel`.** The mutex covered the entire rewrite, so
   the `rayon::join` path could not actually normalize two parts at once.

The Feature Report (AUD-31) also needs a stable, order-independent view of
what the pipeline did — not a log of how many times a part happened to be
touched.

## Decision

1. **Per-part storage.** `TransitionalNormalizer` holds
   `Mutex<BTreeMap<PartId, NormalizationReport>>`. Each `normalize_part` call
   writes into a **local** `NormalizationReport`. When the call finishes
   (success, borrow-back, or error after partial work), it performs one
   `map.insert(part, local)` — a **replacement**, not an addition. The mutex
   is held only for that insert (and for the small
   `note_unexpected_main_content_type` update).

2. **Merge on `report()`.** `report()` walks the map in `PartId` order and
   merges part reports: `applied` counts are summed (`saturating_add`), the
   first `from`/`to` mapping wins, `losses` are concatenated, `removed_nodes`
   / `reported_nodes` are summed, the first `fatal` wins.
   `conformance_detected` is **not** set by the normalizer; the caller that
   knows package-level detection fills it (AUD-31).

3. **Replacement is sound.** A ZIP archive is immutable for the life of a
   `Package`. Re-reading a part always yields the same bytes, so replacing the
   per-part report cannot discard a different outcome. This is the justification
   for `insert` rather than merge-on-write.

4. **`PartialEq` on `NormalizationReport`.** Equality compares the observable
   fields so tests can assert that order of reads and number of re-reads do
   not change `report()`.

5. **`bidi` inherited from styles is out of scope (AUD-33).** The T4 `bidi`
   warning considers only `w:bidi` / `w:bidiVisual` present in the same
   `w:pPr` / `w:tblPr` as the `w:jc` being mapped. Style-inherited bidi is
   not consulted; that limitation is accepted here so the streaming rewrite
   does not need a style cascade.

## Consequences

- Positive: `Manual.docx` reports `T2.reltype x8`; re-reading parts is
  idempotent for the report; under `parallel` parts can rewrite concurrently
  (AUD-36).
- Positive: Feature Report assembly (AUD-31) gets a deterministic merge.
- Negative: a caller that inspects the report mid-open sees only parts
  touched so far — the same as before, but now per-part rather than a running
  sum that could double-count.

## Alternatives considered

- **Merge-on-write with dedup keys.** Rejected: needs a stable key per
  transform instance; replacement is simpler and correct under ZIP immutability.
- **Thread-local report without a map.** Rejected: `report()` must be
  order-independent across threads; a `BTreeMap` keyed by `PartId` gives that
  for free.

## Validation

- Core: normalize one part three times → `report()` equals one read; two
  different part orders → equal `report()`.
- CLI: `normalize Manual.docx` → `T2.reltype x8`.
- Property: on the local Transitional corpus, after a full walk of every part,
  a second full walk leaves `report()` unchanged. (`Package::open` alone does
  not necessarily touch every part, so the equality is against the post-walk
  report, not the post-open report.)
- AUD-36: under `--features parallel`, `report()` equals the sequential run.
