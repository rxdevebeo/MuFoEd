# ADR-0003: Owned XML events and a self-positioning `XmlReader`

- **Status:** Accepted
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-1-TASK.md` §6.5, §11; ADR-0002

## Context

`STAGE-1-TASK.md` §6.5 sketches a zero-copy pull reader:

```rust
pub struct XmlReader<'a> { /* ... */ }
pub fn new(bytes: &'a [u8], limits: &ResourceLimits) -> Result<Self>;
pub fn next_event(&mut self) -> Result<Option<XmlEvent<'a>>>;
```

A lifetime-parameterised, zero-copy reader is a poor fit for two Stage-1
requirements:

1. **Encoding.** `TZ` §9.1 requires UTF-8 and UTF-16 (BOM). UTF-16 must be
   transcoded to UTF-8 before tokenization, so events cannot borrow the
   caller's original bytes in that case.
2. **Namespace resolution.** Resolving a prefix to a URI requires a mutable
   scope stack, and the resolved URI can originate from an escaped attribute
   value. Returning `&'a str` borrowed simultaneously from the input and the
   scope stack couples the two and makes the borrow checker fight every scope
   push/pop.

`quick-xml`'s own slice reader can yield zero-copy events only when the
`encoding` feature is disabled; with it enabled, events borrow the reader's
internal buffer and thus cannot outlive a single `read_event` call anyway.

## Decision

The Stage-1 XML layer uses an **owning** event model and a reader that owns its
decoded buffer:

- `XmlReader` owns the decoded UTF-8 bytes and parses one event per call by
  reconstructing a `quick-xml` slice reader at the current offset (with
  `check_end_names = false`; end-tag matching is done by our own scope stack).
- `XmlEvent`, `QName`, `Attr` own their strings (`String`) and namespace URIs
  are interned as `Arc<str>`. `next_event(&mut self) -> Result<XmlEvent>` has no
  external lifetime.
- `XmlReader::new` takes an explicit `PartId` so that error locations are
  meaningful.

This is a deviation from §6.5 and is recorded here as required by §11.

## Consequences

Positive:

- no self-referential reader and no lifetime entanglement with the scope stack;
- UTF-16/legacy encodings are handled uniformly;
- namespace URIs can be shared cheaply via `Arc<str>`;
- straightforward to fuzz and to reason about.

Negative:

- one allocation (name / text) per event; acceptable for Stage 1, where the
  consumer is conformance detection and later a DOM builder that allocates
  anyway;
- a future zero-copy fast path would be an additive, non-breaking optimisation.

## Alternatives considered

1. **`XmlReader<'a>` with `&'a [u8]` (as drafted).** Rejected: cannot represent
   transcoded UTF-16 without unsafe self-reference or an external owning buffer
   the API cannot express.
2. **`quick-xml` `NsReader`.** Rejected: ADR-0002 commits to our own namespace
   resolution and safety policy.
3. **Enabling `quick-xml`'s `encoding` feature.** Rejected: events then borrow
   the reader's internal buffer, which cannot be returned across calls with our
   API.

## Validation

- unit tests for UTF-8/BOM/UTF-16, namespace scope, nested prefixes, CDATA,
  `xml:space`, unbound prefix and DOCTYPE rejection (`STAGE-1-TASK.md` §9.1);
- limit tests for depth / attributes / text length (§9.1);
- `fuzz_xml` for 24 h without panics (§9.4).

## Addendum (REWORK-STAGE-1)

Following the Stage-1 review, two implementation details were tightened without
changing this decision:

1. **Location bookkeeping is amortized O(1).** `XmlReader` maintains running
   `line`/`column` counters advanced once per byte, replacing the former O(n²)
   "scan from offset 0 on every event" approach (rework R1 / K1). See
   `docs/perf-baseline.md`.
2. **`XmlReader::from_vec(Vec<u8>)`** lets callers hand over ownership of an
   already-decoded buffer, so the UTF-8 fast path performs no copy; `new(&[u8])`
   remains as a convenience that copies once (rework P3).

