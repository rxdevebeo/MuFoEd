# ADR-0001: Own ZIP reader on top of `miniz_oxide`

- **Status:** Accepted
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `TZ-STRICT-OOXML-RUST.md` §8, §12.2, decision Г.2; `STAGE-1-TASK.md` §4.1, task S1.3–S1.5

## Context

The library must read OPC packages (`.docx`), i.e. ZIP archives whose input is
always treated as potentially hostile (`TZ` §2.3). Off-the-shelf ZIP crates
(`zip`, `rc-zip`, …) expose convenience semantics (automatic name handling,
eager decompression, implicit allocation) that make it hard to reason about and
audit resource consumption *before* decompression begins.

Requirements driving this decision:

- enforce `max_zip_entries`, `max_total_uncompressed`, `max_single_uncompressed`
  and `max_compression_ratio` **before** and **during** decompression
  (`TZ` §12.1–§12.2);
- reject path traversal, absolute paths, backslash abuse and duplicate part
  names;
- treat a damaged central directory as an error, never a panic;
- keep the parsing code small enough for a security review and `cargo-fuzz`.

## Decision

Implement a **dedicated ZIP reader inside `opc::zip`** using only
`miniz_oxide` (pure-Rust DEFLATE) plus `memchr` for scanning. We parse the
End-Of-Central-Directory record, the central directory and per-entry local
headers manually, and we expose raw stored/deflated byte streams rather than
materialized archives.

Scope for Stage 1:

- compression methods `0` (store) and `8` (deflate); any other method →
  `UnsupportedCompression`;
- ZIP64 is **desirable but not an acceptance blocker** (decision Г.9); when
  present it must be parsed correctly or rejected with a clear error;
- all size arithmetic uses checked operations;
- the reader is isolated behind the crate-internal API and is fuzzed by
  `fuzz_zip` (`STAGE-1-TASK.md` §9.4).

## Consequences

Positive:

- exact, auditable control over limits and allocation;
- no hidden "read everything into memory" behavior;
- fuzz surface is small and owned by us;
- deterministic, dependency-light error reporting.

Negative / costs:

- we own ZIP correctness (EOCD, ZIP64, encodings, duplications) and its tests;
- slightly more code than delegating to an existing crate;
- must validate against real Word/LibreOffice output and the ZIP spec corpus.

## Alternatives considered

1. **`zip` crate.** Rejected: convenience-oriented, less control over
   pre-decompression limits, broader API surface than we need.
2. **`rc-zip`.** Rejected: async/pull framing and its own opinions add risk and
   indirection for a synchronous, security-first reader.
3. **`flate2`/miniz C bindings.** Rejected: native code weakens the "no C,
   pure Rust, fuzz-friendly" property stated in `STAGE-1-TASK.md` §3.

## Validation

- unit tests for EOCD with/without ZIP64, store/deflate, empty archive, bad CRC,
  truncated file (`STAGE-1-TASK.md` §9.1);
- property test for ZIP round-trip (§9.3);
- `fuzz_zip` running `Package::open_reader` over raw bytes for 24 h without
  panics (§9.4, acceptance criterion §10.5).
