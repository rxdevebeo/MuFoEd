# ADR-0004: WordprocessingML Strict DOM, event parser and two-phase assembly

- **Status:** Accepted
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-2-TASK.md` §4–§9; `TZ-STRICT-OOXML-RUST.md` §3.3, §5.1, §6, §7,
  §15 (Stage 2); ADR-0002, ADR-0003

## Context

Stage 1 delivers `strict-ooxml-core`: an OPC package reader, a streaming
namespace-aware `XmlReader` with owned events, a namespace registry and a unified
`StrictError`. Stage 2 must turn the parts of a Strict package into a typed,
immutable document model ("DOM") that Stage 3 (Feature Report) and Stage 4
(rendering) consume.

`STAGE-2-TASK.md` §4 fixes the main forces:

1. **No intermediate generic DOM.** Build the typed model directly from
   `XmlReader` events (memory / speed / exact types / control of unknown
   elements).
2. **Table-driven dispatch.** `(namespace, local) → handler` is data
   (`ElementSpec`), not `if`-chains; unknown elements become `Opaque` nodes
   recorded in the support model, never errors.
3. **Name interning (P9).** Store interned symbols / `Arc<str>` instead of a
   `String` per name.
4. **Two-phase assembly.** Parse parts into raw typed structures, then resolve
   styles, numbering and relationships into an immutable `Document`.
5. **Immutable model.** `Document` is `Send + Sync` after resolve.
6. **Error model.** Reuse `StrictError`; `Permissive` input yields a model with
   support-model entries rather than an outright failure.

Stage-1 explicitly deferred P6 (persistent `quick-xml::Reader`), P7 (double
attribute `Vec`) and P9 (string per name) to Stage 2 (`docs/perf-baseline.md`).

## Decision

### Crate and layers

A new crate `strict-ooxml-wml` depends only on `strict-ooxml-core` (plus optional
`rayon` under the `parallel` feature). It is organised as:

- `model/` — pure data (no parser logic), all nodes carry a `SourceLocation`.
- `parse/` — event-driven recursive-descent parser over `XmlReader`, an
  `Interner`, `ParseContext` and `ElementSpec` dispatch tables.
- `resolve/` — the second phase: style cascade, `numId → abstractNumId`, and
  `r:embed`/`r:id` → `PartId`.

### Event-driven recursive descent (not a bespoke state machine)

The parser is a recursive-descent consumer of the existing pull API: each
`parse_*` function is handed the already-consumed `StartElement` and reads
events until the matching `EndElement` (depth-tracked). This keeps the mapping
from schema to code local and reviewable. Deep input cannot overflow the stack
unboundedly: the `XmlReader`'s `max_xml_depth` (default 256) bounds nesting
before the parser recurses, and recursion depth is additionally checked against
the same limit.

Dispatch is data-driven through `parse::dispatch`:

- `body_element(local) -> BodyElement` for block-level children;
- `inline_element(local) -> InlineElement` for run/inline children;
- `prop_element(context, local) -> PropElement` for `pPr`/`rPr`/`tblPr`/`tcPr`/`trPr`.

Names the tables do not know yield `Opaque` nodes and a `FeatureUse`.

### Interning: `Arc<str>` handles, not raw `Sym` in the DOM

`parse::interner::Interner` deduplicates `(namespace, local)` names and value
strings and hands out `Arc<str>`. The DOM stores `Arc<str>` (wrapped in
semantic newtypes such as `StyleId`) rather than a `u32` symbol that would force
every consumer to hold a borrow of the parser's interner (which conflicts with
the immutable, self-contained `Document` required by §4.5). Text content, which
is rarely repeated, is stored as `String` as drafted in §6.

This satisfies P9 (no `String` per *name*; shared allocations) while keeping
`Document: Send + Sync + 'static`.

### Two-phase assembly

```text
Package ──parse──► parts:
                    document.xml  → RawBody (+ sections)
                    styles.xml    → StyleTable (unresolved basedOn)
                    numbering.xml → NumberingTable (unresolved num→abstract)
                    settings.xml  → Settings
                    rels          → relationship map
                            │
                        resolve
                            ▼
                       Document (immutable)
```

`strict_ooxml_wml::parse::parse_document(&Package, &ConformancePolicy, limits)`
runs both phases and returns `Result<Document>`. `styles`/`numbering`/`settings`
are independent and, under `feature = "parallel"`, are parsed concurrently with
`rayon`.

### Fail-soft under `Permissive`

Malformed construct handling matches the policy: `StrictOnly` propagates the
`StrictError`; `Permissive` records a `FeatureUse { status: Partial/Unsupported }`
with the offending `SourceLocation`, substitutes the schema default where one
exists, and continues. The strict namespace check (`purl.oclc.org/ooxml/...`)
is applied regardless of policy — the Stage-2 parser never accepts Transitional
markup (normalization is Stage 6).

### Public surface

- `strict-ooxml-wml` exposes `parse_document`, the `model` types, `SupportModel`
  and `Document::support()/support_debug()`.
- The new `strict-ooxml` meta-crate provides `StrictDocument` as the Stage-2
  entry point (`open_path`, `open_reader`, `document()`, `support()`); the
  rendering/report methods are added in Stages 3–4.

## Consequences

Positive:

- The typed model is exact and self-contained (`Send + Sync + 'static`); no
  parser state leaks into consumers.
- Unknown markup is never lost: it becomes `Opaque` and a `FeatureUse`, which is
  exactly the Stage-3 input.
- Table-driven dispatch makes the optional-element coverage measurable against
  a single inventory (`coverage/wml-elements.toml`).
- Recursive descent composes cleanly with the existing depth limit and keeps
  location bookkeeping O(1).

Negative / costs:

- Recursive descent uses the call stack; it is safe only because the XML depth
  limit is enforced first. This is documented and covered by
  `tests/misc.rs::depth_limit_is_enforced_without_panic` (a document nested past
  `ResourceLimits::max_xml_depth` yields `LimitExceeded`, never a panic/overflow).
- `Arc<str>` per distinct name still allocates once per distinct name (not per
  occurrence), which is the accepted P9 trade-off.
- Two phases mean raw structures exist briefly; they are dropped before
  `Document` is returned.

## Alternatives considered

1. **Intermediate generic DOM then convert.** Rejected per §4.1: extra memory,
   imprecise types, and unknown-element handling would be deferred too late.
2. **Hand-written flat state machine (explicit stack).** Rejected: harder to
   review against the schema and no measurable win once the depth limit bounds
   stack use.
3. **`u32` symbols stored in the DOM with an interner kept on `Document`.**
   Rejected: forces `Document` to carry the interner and complicates `Sync`;
   `Arc<str>` gives the sharing benefit without the coupling.
4. **`String` for every name (Stage-1 style).** Rejected: P9 explicitly assigns
   interning to Stage 2.
5. **Reuse `strict-ooxml-cli` as the meta entry point.** Rejected: the TZ §5.2
   graph requires a library meta-crate above `wml`.

## Validation

- Golden-DOM tests (`tests/golden/*.txt`) over synthetic Strict documents
  covering paragraphs, runs, properties, tables, lists, sections and inline
  drawings.
- Property tests: arbitrary event streams never panic; `dump(parse(x))` is
  stable.
- Corpus test over `tests/samples/` (no panic; Transitional rejected under
  `StrictOnly`).
- `fuzz_wml`: `Package` → `parse_document` never panics.
- `criterion` bench `wml_parse` for 10/100/500-page documents.
- CI gate: optional-element coverage from `coverage/wml-elements.toml` ≥ 90%.
- Deep-nesting limit: `tests/misc.rs::depth_limit_is_enforced_without_panic`.
- Line coverage of `strict-ooxml-wml` ≥ 80% (CI `coverage` job).
- `tests/misc.rs::model_variant_sizes_are_bounded` bounds `Block`/`Inline` sizes;
  this is why the `large_enum_variant` allow is acceptable (REWORK M9).
- Independent WML oracle: `tests/corpus_oracle.rs` derives real lexical forms
  from the public corpus with an independent ZIP reader and checks that the
  parser applies them. It is the regression guard for schema attribute names
  (`CT_TabStop` position in `w:pos`, alignment in `w:val`) and for fractional
  measurements (`ST_MeasurementOrPercent`/`ST_SignedTwipsMeasure`), which are
  rounded to the model's whole-twip representation rather than dropped
  (STAGE-2-WORK-ORDER D-1/D-2/P-1). It runs in the CI test job.

## Stage 5A.1a — headers and footers (additive)

- The DOM gains `HeaderFooter` (`part`, `is_header`, `blocks`, `location`) and
  `Document::headers_footers`; `HeaderFooterRef` gains a resolved `part`.
  `DocumentSource` gains `footnotes`/`endnotes`/`theme` part ids. All additions
  are additive (pre-1.0, ADR-0004 §4).
- Header (`w:hdr`) and footer (`w:ftr`) parts are located by **relationship
  type** (`RelType::Header`/`Footer`), never by file name, and reused with the
  body block parser (`parse_block_children`); the part root must be in the WML
  Strict namespace under a detected-Strict package.
- A relationship that resolves to a part absent from the package is
  `StrictError::MissingReferencedPart { part, location }` (an error with a
  location, never a panic). Unresolved or mistyped relationships are recorded as
  `Partial` and left unresolved.
- Validation: `tests/sections.rs` (parsing, missing part, wrong rel type),
  `tests/golden.rs` (`section`), plus render coverage in ADR-0006.

## Stage 5A.1b — footnotes, endnotes and fields (additive)

- New `notes` model: `Note`/`NoteKind`/`NoteTable` (keyed by `w:id`, with the
  reserved separator ids `-1`/`0`) and `NoteProperties`
  (`w:footnotePr`/`w:endnotePr`). `Document` gains `footnotes`/`endnotes`;
  `Settings` and `SectionProperties` gain note properties.
- `footnotes.xml`/`endnotes.xml` are discovered by relationship type and parsed
  with the body block parser; `w:footnoteRef`/`w:endnoteRef` are modelled as the
  new `RunContent::NoteRef`. `w:footnoteReference`/`w:endnoteReference` are now
  `Supported` (they were placeholder `Unsupported` in Stages 2–4).
- Fields stay flat (`w:fldChar`/`w:instrText`/`w:fldSimple`) and are grouped in
  the renderer; the parser records `w:fldSimple`/`w:instrText` as `Supported`
  for computed fields (PAGE/NUMPAGES/SECTIONPAGES) and `Partial` for cached
  fields, so the "not computed" state is never silent.
- Validation: `tests/notes.rs` (parts, marker, numbering properties), unit tests
  in `model/notes.rs`; rendering in ADR-0006.

## Stage 5A.1d — themes (additive)

- New `theme` model (`Theme`/`ThemeFonts`/`FontSet`/`ThemeColors`) parsed from
  `theme/theme1.xml` (DrawingML Strict namespace). `Document.theme` holds it.
- `Fonts` gains theme references (`asciiTheme`/`hAnsiTheme`/`eastAsiaTheme`/
  `cstheme`); `RunProperties` gains `color_theme` (`w:themeColor` + tint/shade).
- `Theme::font` resolves `major*`/`minor*` + script suffix;
  `Theme::colors::resolve` maps `dark1`/`text1`/`accent1`/`hyperlink`/… to the
  `dk1`/`lt1`/`accent1`/`hlink`/… slots. Effects/fills/line styles (`a:fmtScheme`)
  are recorded `Partial` and not resolved (5A scope).
- Validation: `tests/theme.rs`, unit tests in `model/theme.rs`/`parse/theme.rs`;
  cascade application in ADR-0006.

