# Performance baseline — Stage 1 rework

Measurements for `REWORK-STAGE-1`. Reference host: Windows x86_64, Rust 1.91,
`--release`. The specification reference is `TZ-STRICT-OOXML-RUST.md` §14
(Linux x86_64, zero of 100-page document ≤ 2 s).

## R1 — XML location bookkeeping (K1, P0)

Synthetic document: `<w:document>` wrapping `n` × `<w:p><w:r><w:t>x</w:t></w:r></w:p>`.
"events" = 7 per `n` (plus the two root tags).

| n | bytes | events | Before (REWORK §3.2) | After (criterion) | Speedup |
|---|---|---|---|---|---|
| 1 000 | 34 KB | 7 002 | — | 1.05 ms | — |
| 8 000 | 272 KB | 56 002 | 2 651 ms | 10.4 ms | ~255× |
| 32 000 | 1.09 MB | 224 002 | 67 896 ms | 45.6 ms | ~1489× |
| 128 000 | 4.35 MB | 896 002 | — | 197 ms | — |

Deep nesting (`depth=200`): 97 µs.

**Result:** growth is linear (4× input → ~4.3× time); 1 MiB parses in ~46 ms,
well under the ≤ 150 ms target. Regression barrier:
`strict-ooxml-core/tests/perf.rs` (generous 2 s threshold, ~0.26 s in debug).

## Alloc/copy reductions (P3/P4)

- `XmlReader::from_vec(Vec<u8>)` takes ownership; the UTF-8 fast path performs no
  copy (previously `bytes.to_vec()` on every part).
- `ZipArchive` stores `Arc<Vec<u8>>`; constructing it no longer copies the whole
  archive (`Arc<[u8]>::from(Vec)` did).

## CRC-32 (P5)

`crc32_update` now uses a compile-time 256-entry lookup table instead of a
bit-at-a-time loop; CRC runs over every decompressed byte, so this matters for
media-heavy parts.

## Conformance scanning (P2/R7)

`root_namespace` streams a bounded 64 KiB prefix of each part and stops at the
first start element, falling back to a full read only when the prefix is
inconclusive. Large `document.xml` parts are no longer fully decompressed just
to be classified.

## Deferred with rationale (P6/P7/P8/P9)

- **P6 (persistent `quick-xml::Reader`).** After R1, per-event reader
  reconstruction is not a dominant cost (measured `xml_scan` throughput is
  healthy). A persistent reader without `unsafe`/self-reference needs a windowed
  design; deferred to Stage 2, where DOM parsing will drive the access pattern
  and a benchmark can justify the change.
- **P7 (double attribute `Vec`).** Small, per-element; deferred to Stage 2
  together with name interning (P9), which will restructure attribute handling.
- **P8 (`NsStack::resolve` linear scan).** Namespace stacks are shallow; no
  measurable impact. Revisit only if documents with many local `xmlns`
  declarations appear.
- **P9 (`String` per name).** Accepted for Stage 1 per ADR-0003; interning is a
  Stage-2 (DOM) concern.

## Stage 2 — `wml_parse` (criterion)

`strict-ooxml-wml/benches/wml_parse.rs` measures `Package::open_reader` +
`parse_document` over synthetic Strict documents of 10/100/500 "pages"
(10 paragraphs per page). Windows x86_64, Rust 1.91, `--release`:

| Pages | Paragraphs | Time |
|---|---|---|
| 10 | 100 | ~0.43 ms |
| 100 | 1 000 | ~4.84 ms |
| 500 | 5 000 | ~29.4 ms |

Growth is linear; a 100-page document parses in well under the TZ §14 budget
(≤ 2 s). Name/value interning (P9) and the O(1) location cache from Stage 1 are
both exercised by this benchmark.

Stage-2 follow-ups on the deferred items: P6 (persistent `quick-xml::Reader`)
remains deferred; P7 (double attribute `Vec`) and P9 (interning) are resolved in
`strict-ooxml-wml` (`parse::interner`, `PartParser`).
