# ADR-0016: One conformance-policy matrix, decided from raw (T0) signals

- **Status:** Accepted
- **Date:** 2026-10-03
- **Deciders:** Strict OOXML maintainers
- **Related:** AUD-23 (`REWORK-AUDIT-2026-10.md`), AUD-22 (exact relationship-type
  matching, which makes a resolving `officeDocument` relationship always carry
  a conformance signal), `ADR-0005` (Feature Report severities)

## Context

Whether a package may be opened under a given `ConformancePolicy` used to be
decided three times, and the three implementations disagreed:

1. `opc::mod::enforce_policy` (now deleted), inside `Package::open_archive`.
2. `wml::parse::parse_document`'s own gate on `package.conformance()`.
3. The CLI `check` command's branching on conformance and whether a
   normalizer was installed.

The disagreements were not cosmetic:

- `Permissive` with no normalizer let a Transitional package through
  unexamined — the parser's gate was supposed to catch it downstream, but it
  only fired for `Conformance::Transitional`, not for whatever the parser's
  own, looser "match by local name" fallback decided to accept.
- `Mixed` was accepted under *any* policy, including `StrictOnly`, as long as
  a normalizer happened to be installed — a package carrying both Strict and
  Transitional signals is not something `StrictOnly` should ever silently
  wave through.
- `Unknown` was accepted under `StrictOnly` — "no signal at all" was treated
  as "Strict" rather than as what it is: undetermined.
- Detection itself was taken **after** the normalizer had already touched the
  part being inspected, so a Transitional document could come out of
  detection as `Strict` once a `RawNormalizer` had rewritten its root
  namespace — the policy was deciding on the wrong input.

## Decision

1. **Detection reads raw (T0) signals, never projected ones.**
   `Package::open_archive` computes the root namespace of the main document
   part without ever routing it through the normalizer's namespace registry
   (`raw_root_namespace`, replacing the old `root_namespace` that projected
   through the registry before classifying — the direct cause of the
   "Strict for Transitional" regression). Relationship-type signals come from
   the **raw** bytes of `.rels`: the file is parsed twice — once raw, purely
   for the T0 signal, and once through the normalizer to build the
   `RelationshipGraph` the rest of the package actually uses. Parsing a
   `.rels` file twice is cheap; getting the signal from the wrong side of the
   normalizer is not recoverable after the fact.

2. **`Package` tracks what it detected and whether it changed anything.**
   `Package::conformance()` returns the T0-detected `Conformance` and never
   changes after `open_*` returns. A new `Package::was_normalized()` reports
   whether the installed `RawNormalizer` returned `Cow::Owned` — i.e. changed
   something — for any part touched while opening or while reading parts
   afterward. It is backed by an `AtomicBool` rather than a `Cell<bool>`
   because `Package` must stay `Sync` for the `rayon::join` parallel parsing
   path (feature `parallel`).

3. **One matrix, one function.** `opc::policy::decide(policy, detected,
   has_normalizer) -> Result<()>` is the only place a `ConformancePolicy` is
   weighed against a detected `Conformance` and whether a normalizer is
   configured:

   | policy \ detected | Strict | Transitional | Mixed | Unknown |
   |---|---|---|---|---|
   | `StrictOnly` | ok | `TransitionalNotSupported` | `MixedConformance` | `UndeterminedConformance` |
   | `Normalize`, has normalizer | ok | ok | ok | ok |
   | `Normalize`, no normalizer | ok | `Unsupported` | `Unsupported` | `Unsupported` |
   | `Permissive`, has normalizer | ok | ok | ok | ok |
   | `Permissive`, no normalizer | ok | `Unsupported` | `Unsupported` | ok |

   `Permissive` differs from `Normalize` in exactly one cell: without a
   normalizer it still accepts `Unknown`, so `inspect`-style callers can look
   at a package whose conformance cannot be determined at all. Everywhere
   else the two policies agree on *whether* a package opens; they differ only
   in how the parser treats unsupported mechanisms once it does (unchanged by
   this ADR). `StrictError::UndeterminedConformance { detail }` is new, for
   the one cell that used to be silently treated as Strict.

   `Package::open_archive` calls `decide` exactly once, right after T0
   detection. Nothing else re-implements any cell of this table.

4. **The WML parser no longer checks conformance itself.** `ParseOptions`
   drops its `conformance: ConformancePolicy` field — the only decision left
   for a parser to make about conformance is "is the root element in the
   right namespace", not "is this package allowed to be opened at all", and
   that question has already been answered by the time `parse_document` is
   called. This is a breaking change to a pre-1.0 crate, which the project's
   TZ explicitly allows. The meta-crate's `parse_options_from` is simplified
   to match.

5. **The root-namespace check is unconditional, and keys on what actually
   happened, not on the detected label.** `expect_root_ns`'s old guard —
   `!package.was_normalized() && package.conformance() == Conformance::Strict`
   — only ever checked the root namespace when conformance had already been
   labelled `Strict`; anything else fell through to a looser "match the root
   by local name" fallback. The root element is now always required to sit in
   the WordprocessingML Strict namespace, normalized or not. On a mismatch:
   if the package was not normalized and was detected `Transitional`, the
   error is `TransitionalNotSupported` (the package needs `--transitional` /
   a normalizer, not a bug report); otherwise it is `InvalidXml("root element
   is not in the WordprocessingML Strict namespace")` — covering a
   conformance-`Unknown` package opened under `Permissive` with no
   normalizer, and the pre-existing case of a package independently detected
   `Strict` (via its relationship type) whose root happens to sit in some
   unrelated namespace.

6. **The CLI's `check` branching collapses to the matrix's result.** Since
   `Package::open_path` already applied `decide`, by the time it returns `Ok`
   there is nothing left to branch on. `check` keeps exactly three outcomes:
   `Ok(document)` → `report_support`; `Err(TransitionalNotSupported)` → exit
   code 1 (with the existing `--transitional` hint); any other `Err` → exit
   code 2. The success line distinguishes an already-Strict input from a
   normalized one: `"ok: strict"` when `!was_normalized()`, otherwise `"ok:
   normalized from <detected>"` (lower-cased, e.g. `"ok: normalized from
   transitional"`).

## Consequences

- Positive: there is exactly one place that can get the policy matrix wrong,
  and it is executable and tested cell-by-cell. Conformance detection is
  correct for a Transitional package even when a normalizer is installed and
  rewrites that same part moments later.
- Positive: `StrictOnly` can no longer be fooled into accepting `Mixed` or
  `Unknown` content; `Permissive` can no longer be fooled into accepting
  Transitional content with no normalizer to actually normalize it.
- Negative / breaking (pre-1.0, accepted per the TZ): `ParseOptions` no
  longer has a `conformance` field; any caller constructing it as a struct
  literal with that field must drop it. A `Permissive` caller that relied on
  opening Transitional packages without a normalizer (there was exactly one
  such production code path — none; only tests and corpus-fidelity checks did
  this) must now install a normalizer, even a `NoopNormalizer`, to keep doing
  so.
- Because AUD-22 made relationship-type matching exact, a package whose main
  document relationship actually resolves always carries a Strict or
  Transitional signal of its own (`ns::registry::classify_relationship`
  matches both URI families), so true `Conformance::Unknown` is unreachable
  for any package that successfully opens its main document — it is reachable
  only via a main-document relationship that does not resolve to a
  recognized URI at all.

## Alternatives considered

- **Keep three decision points but make them agree.** Rejected: agreement
  would have to be re-verified by hand on every future change to any of the
  three; a single function is the only way to make "three places implement
  this" structurally impossible.
- **Detect conformance after normalization, documented as intentional.**
  Rejected: the whole point of `ConformancePolicy::Normalize` is to accept
  Transitional input and turn it into Strict; detecting "Strict" because the
  normalizer already did its job defeats the purpose of recording what the
  input actually was (`was_normalized()`, the Loss Report, `--transitional`'s
  CLI messaging all depend on knowing the T0 state).
- **Let `Permissive` keep opening Transitional without a normalizer.**
  Rejected: nothing downstream of `open_*` can safely assume Transitional
  content was ever rewritten; the WML parser's own gate on
  `TransitionalNotSupported` existed only to patch over this, in exactly one
  of three places that needed to agree with each other.

## Validation

- `opc::policy::tests::every_cell_of_the_matrix` executes all 20 cells (5
  policy/normalizer rows × 4 detected-conformance columns) and asserts both
  `Ok`/`Err` and, for each error cell, the exact `StrictError` variant.
- `strict-ooxml-core/tests/opc_integration.rs`:
  `conformance_is_detected_from_raw_bytes_even_with_a_normalizer_installed`
  pins the regression this ADR closes (a Transitional document under
  `Normalize` with a `TransitionalNormalizer` installed detects as
  `Transitional`, not `Strict`, and `was_normalized()` is `true`);
  `relationship_type_signal_is_read_before_the_normalizer_rewrites_it` proves
  the raw/normalized double-parse of `.rels`; `permissive_without_a_normalizer_no_longer_opens_transitional_as_is`
  and `permissive_without_a_normalizer_still_opens_unknown_for_inspect` pin
  the one cell that still differs between `Permissive` and `Normalize`.
- `strict-ooxml-wml/tests/prolog.rs`:
  `foreign_namespace_root_is_rejected_even_when_detected_as_strict` pins the
  unconditional root-namespace check.
- `strict-ooxml-cli/tests/cli.rs`: `check_strict_returns_zero`,
  `check_transitional_returns_one`,
  `check_transitional_with_flag_normalizes_and_returns_zero` (`"ok: normalized
  from transitional"`), `check_rejects_an_unrecognized_office_document_relationship`
  (an unresolved `officeDocument` relationship is `Unknown` under
  `StrictOnly` → `UndeterminedConformance` → exit 2).
- `rg "Conformance::Mixed if" strict-ooxml-core/src` is empty: no remaining
  code path treats `Mixed` as acceptable outside `opc::policy::decide`.
