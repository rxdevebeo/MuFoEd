# ADR-0005: Feature Report — JSON stack, locations, severity and aggregation

- **Status:** Accepted
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-3-TASK.md` §3–§9; `TZ-STRICT-OOXML-RUST.md` §5.1–§5.4, §6,
  §11, §15 (Stage 3), §17; ADR-0004

## Context

Stage 2 (`strict-ooxml-wml`) produces a `SupportModel`: a `BTreeMap<Arc<str>,
FeatureUse>` where each `FeatureUse` aggregates a mechanism (`w:tbl`, `wp:anchor`,
…), the most severe `SupportStatus`, the first message, the first
`SourceLocation` and an occurrence `count`.

Stage 3 must turn that into a **Feature Report** (`TZ` §11, schema v2) that is
deterministic, valid against a versioned JSON Schema and traceable: every
`partial`/`unsupported`/`error` mechanism must carry at least one location.
`STAGE-3-TASK.md` §9 leaves four decisions open; this record fixes them.

Forces:

1. The report is a **public, versioned contract** consumed by tools and CI; the
   schema must be the source of truth and the report a stable, byte-identical
   serialization.
2. `TZ` §11.1 shows `locations` as an **array**; the Stage-2 model stores a
   single optional location.
3. `TZ` §11.2 fixes the status/severity vocabularies and the `overall_status`
   aggregation order `error > unsupported > partial > supported`.
4. Stage 3 must **not** parse Transitional documents (normalization is Stage 6,
   `STAGE-3-TASK.md` §2.2).
5. New dependencies need a decision and a `cargo-deny` story.

## Decision

### JSON stack (open question 5)

Use `serde` (derive) + `serde_json` for serialization. Deriving `Serialize` on
the model removes the class of “hand-written writer emits invalid JSON” bugs and
keeps field order equal to declaration order. The report is emitted with
`serde_json::to_string_pretty` followed by a single `\n` (UTF-8, `\n` line
endings); pretty output is deterministic because serde emits fields in struct
order and every collection is a sorted `Vec`.

`jsonschema` is added as a **dev-dependency only** (validation in tests). It is
never linked into the library or the CLI, so production builds stay lean. It is
the independent oracle of `TZ` §11 validness (`STAGE-3-TASK.md` §8.2).

### Schema version (open question 6)

Start at `"2.0"` exactly as `TZ` §11.1, because the shape matches the v2 report
(nested `tool`/`conformance`/`summary`/`normalization` blocks). The literal is
single-sourced in `schema.rs` (`SCHEMA_VERSION`) and pinned in the schema by
`const`.

### Multiple locations (open question 1)

Extend `FeatureUse` to `locations: Vec<SourceLocation>` while keeping the
aggregate `count` (total occurrences). Policy:

- **Cap `N = 8`** distinct locations per feature (`MAX_LOCATIONS_PER_FEATURE`),
  bounding report size on large documents.
- **Primary location = first** captured (document/part order); `locations[0]` is
  what the Stage-2 `debug_summary()` prints.
- Deduplicate identical locations; merge keeps the earlier feature’s order so
  the result is independent of parse scheduling (`parallel` merges
  styles → numbering → settings in fixed order).
- This is a pre-1.0 breaking change to `FeatureUse` (documented; `location` is
  replaced by `locations` + `first_location()`), which `STAGE-3-TASK.md` §2.2
  explicitly allows.

### Status, severity and `overall_status` (open question 2, 3)

Vocabularies (`TZ` §11.2): feature status `supported | partial | unsupported |
ignored | error`; severity `info | warning | error`; overall `supported |
partial | unsupported`.

Default severity table (encoded in `severity.rs` **and** in the schema so the
two cannot drift):

| status | severity |
|---|---|
| `supported` | `info` |
| `partial` | `warning` |
| `unsupported` | `warning` |
| `ignored` | `info` |
| `error` | `error` |

`overall_status` is “worst of”: any `unsupported`/`error` feature →
`unsupported`; else any `partial` → `partial`; else `supported`. This matches
`TZ` §11.2 (`error > unsupported > partial > supported`).

`status: error` is **reserved**: in Stage 3 the parser never yields it, so
`summary.error` is always `0`. It is produced only by future report-assembly
failures (e.g. a normalization invariant violation in Stage 6). The enum and
schema accept it now so the contract is stable.

**Critical problem** (used by `check`, §7.2): a feature with `status` equal to
`unsupported` or `error` — i.e. an `error`/`unsupported` blocker. `partial` is a
warning, not a blocker.

### Feature semantics: a problem/attention view (option a)

The Stage-2 parser records a mechanism in the `SupportModel` only when it needs
attention (unknown/foreign, partial, deliberately ignored) or when a value could
not be applied. It never records fully supported core markup (no production path
calls `record(…, Supported, …)`; `Supported` is only the default of a record
that is never created for it).

Consequently the Feature Report is a **problem/attention view, not a coverage
map**:

- `features` enumerates the mechanisms the parser recorded — normally the ones
  needing attention. Fully supported core markup is **not** enumerated.
- `summary.supported` counts only *explicitly recorded* supported mechanisms and
  is therefore normally `0` in production; it is **not** an indicator that "no
  markup is supported". `summary` is a set of counters over `features`, not
  coverage percentages.
- `overall_status == supported` means "no `partial`/`unsupported`/`error`
  mechanism was recorded" (no reported limitation), **not** "every mechanism was
  verified as supported".

This choice is recorded as option **(a)** of the Stage-3 acceptance finding F1:
document the semantics rather than populate `supported` for every recognised
element. Emitting a real coverage map (option (b)) would require the Stage-2
parser to record `supported` mechanisms against the `coverage/wml-elements.toml`
inventory and to bound the resulting report size; that is deferred to a later
stage and is **not** part of Stage 3.

Consequences: the human-readable renderer prints an explicit
`note: only mechanisms with a notable status are recorded; fully supported
markup is not enumerated`, and the JSON Schema documents the same in the
`features`/`summary`/`overall_status` descriptions. `check`'s exit codes remain
driven by `unsupported`/`error` blockers and are unaffected.

### Transitional input (open question 4)

**Refuse.** Stage 3 opens with `ConformancePolicy::StrictOnly`; a Transitional
package yields `StrictError::TransitionalNotSupported` and no report is
produced. Building an “empty features” report for Transitional would falsely
suggest the document is understood; without Stage-6 normalization the parser
cannot claim that. `check` maps this to exit code `1`; `report` maps it to exit
code `2` (generation failed).

### CLI contract (TZ decision G.8 refinement, `STAGE-3-TASK.md` §7.2)

- `check <file>`: `0` — Strict, parsed, no `unsupported`/`error` feature; `1` —
  at least one `unsupported`/`error` feature **or** Transitional under
  `StrictOnly`; `2` — damaged input, undetermined conformance or internal error.
- `report <file> [--json|--text] [--out <path>]`: full report; `0` on success,
  `2` on any failure (damaged, Transitional, undetermined).

### `locations` representation

JSON `locations` are strings `"<part>:<line>:<column>"` (the `Display` of
`SourceLocation`, `TZ` §7.4), matching the `TZ` §11.1 example. The schema pins
the pattern; the model keeps the string so the report is self-contained.

## Consequences

Positive:

- The report is a pure function of `(SupportModel, Conformance, tool metadata)`;
  no unordered map reaches the output, so serialization is byte-stable.
- Schema and code share the severity table via a schema `if/then`; drift makes
  the schema-validation test fail (independent oracle).
- Location cap bounds report size while preserving ≥ 1 location for every
  partial/unsupported/error feature.

Negative / costs:

- `serde`/`serde_json` become runtime dependencies; `jsonschema` a (large)
  dev-dependency. Mitigated: `cargo-deny` reviewed and green with the existing
  allow-list, `jsonschema` is dev-only and built with `default-features = false`
  (no HTTP/file resolvers).
- `FeatureUse` is a breaking change (single `location` → `locations`). Pre-1.0
  SemVer allows it; Stage-2 tests are updated.

## Alternatives considered

1. **Hand-written JSON writer.** Rejected: high risk of invalid JSON/escaping
   bugs and no derive-level field-order guarantee (`STAGE-3-TASK.md` §4).
2. **Keep a single location.** Rejected: `TZ` §11 mandates an array and
   traceability is better with bounded multiple locations; the task’s
   recommendation is N locations.
3. **Structured `location` objects `{part,line,column}`.** Rejected: `TZ` §11.1
   shows strings; strings keep the schema and report compact. Structured data
   remains available through `strict-ooxml-wml`’s `SupportModel`.
4. **`unsupported → error` severity.** Rejected: would make almost every real
   Strict document “critical”; the task keeps `unsupported` at `warning` with a
   separate “blocker” concept for exit codes.
5. **Emit an empty report for Transitional under `Permissive`.** Rejected: it
   would claim understanding without normalization (`TZ` §15 Stage 4/6).

## Validation

- `strict-ooxml-report/tests/schema.rs` — every generated/golden report is valid
  against `schema/support-report.schema.json` with the independent `jsonschema`
  validator; a deliberately corrupted report is **rejected** (oracle
  self-check).
- `strict-ooxml-report/tests/oracle.rs` — report invariants against the input
  `SupportModel` (same `feature_id` set, same counts, ≥ 1 location for
  partial/unsupported, summary/overall consistency), plus a corrupted-report
  negative control.
- `strict-ooxml-report/tests/determinism.rs` — two runs and shuffled insertion
  orders produce identical bytes.
- `strict-ooxml-report/tests/golden.rs` — golden JSON and text for synthetic
  Strict documents.
- `strict-ooxml-cli/tests/cli.rs` — `check` codes `0/1/2`, `report --json/--text/
  --out`.
- CI: schema-validation step, CLI tests, `cargo-deny`, coverage of
  `strict-ooxml-report` ≥ 80% lines.
