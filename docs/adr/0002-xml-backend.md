# ADR-0002: `quick-xml` tokenizer with an owned limiting wrapper

- **Status:** Accepted
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `TZ-STRICT-OOXML-RUST.md` §9, §12.2, §13, decision Г.3; `STAGE-1-TASK.md` §4.2, task S1.9–S1.10

## Context

The XML layer must be streaming (no DOM by default), namespace-aware, and
hostile-input safe. In particular it must never process DTDs or external
entities (XXE/SSRF, "billion laughs"), must bound nesting depth, attribute count
and text length, and must surface unknown prefixes as `UnboundPrefix` errors
rather than panics (`TZ` §9, §12.2, §13).

Writing an XML tokenizer from scratch is feasible but expensive and itself a
source of security bugs. We want to reuse a well-tested tokenizer while owning
every security-relevant policy decision.

## Decision

Use **`quick-xml` in low-level event mode** as the byte-to-event tokenizer, and
build our **own namespace-resolution and safety layer** on top of it
(`xml/qname.rs`, `xml/ns_stack.rs`, `xml/safety.rs`).

Specifically:

- **Reject `DOCTYPE` before tokenization / at the first token.** DTD and
  external entity resolution are unconditionally disabled; there is no opt-in.
  Entity references beyond the five predefined XML entities are rejected.
- Namespace prefix → URI resolution, scope stack and `QName` construction are
  **ours**, not delegated to `quick-xml`.
- Enforce `max_xml_depth`, `max_xml_attributes_per_elem` and `max_text_len`
  from `ResourceLimits` in the wrapper, mapping every violation to
  `LimitExceeded { kind, limit, actual }`.
- We expose only our own `XmlEvent`/`QName`/`Attr` types so the `quick-xml`
  dependency does not leak into the public API and can be swapped.

Risk containment (see `STAGE-1-TASK.md` §4.2, §12 risks): the DOCTYPE ban lives
in our limiter, not in `quick-xml` configuration; the wrapper is fuzzed by
`fuzz_xml`.

## Consequences

Positive:

- battle-tested tokenization without trusting it with security policy;
- security-relevant behavior (DTD/entities/limits) is explicit and testable;
- `quick-xml` is not part of the public API, so it can be replaced later
  without a breaking change;
- UTF-8/UTF-16 handling can ride on `encoding_rs` and `quick-xml`.

Negative / costs:

- a dependency whose upgrade/security advisories must be tracked
  (`cargo-deny`);
- we must keep our namespace stack correct even where `quick-xml` offers its own
  helpers (we intentionally do not use them);
- if `quick-xml` cannot be made to reject a construct early enough, the fallback
  is a custom tokenizer (decision recorded in the risk table, `STAGE-1-TASK.md`
  §12).

## Alternatives considered

1. **Hand-written tokenizer.** Rejected for Stage 1: higher up-front cost and
   greater chance of parser-level vulnerabilities; revisit only if `quick-xml`
   proves insufficient.
2. **`xml-rs`.** Rejected: pull-parser ergonomics are fine but its
   configuration/limits story and dependency weight are weaker fits than
   `quick-xml` low-level events.
3. **`roxmltree` / DOM builders.** Rejected outright: building a tree by default
   contradicts the streaming requirement (`TZ` §9.1) and raises DoS risk.

## Validation

- unit tests: UTF-8/UTF-16 BOM, namespace scope, nested prefixes, CDATA,
  `xml:space`, unbound prefix, and DOCTYPE rejection (`STAGE-1-TASK.md` §9.1);
- limit tests: each of depth/attrs/text → the matching `LimitKind` (§9.1);
- property test: arbitrary scope sequences resolve correctly (§9.3);
- `fuzz_xml` for 24 h without panics (§9.4, acceptance criterion §10.5).
