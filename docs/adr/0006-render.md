# ADR-0006: SVG rendering — font metrics, style cascade, media and SSIM references

- **Status:** Accepted (with one documented waiver: external SSIM references)
- **Date:** 2026-09-28
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-4-TASK.md` §4–§9; `TZ-STRICT-OOXML-RUST.md` §5.1–§5.4, §6,
  §11, §14, §15 (Stage 4), §17; ADR-0004, ADR-0005

## Context

Stage 4 turns the immutable DOM (`strict-ooxml-wml`) into SVG pages. The forces:

1. **No system fonts.** Layout needs glyph advances, but depending on installed
   fonts would make output machine-dependent; the TZ §14 NFR and the project's
   determinism requirement forbid that.
2. **Where to compute the cascade.** `STAGE-4-TASK.md` §3 leaves open whether
   effective paragraph/run properties are computed in `wml` or in the renderer.
3. **Media bytes.** A [`Document`] is self-contained but does not hold part
   bytes; image embedding needs `Package::read_part`.
4. **Visual acceptance.** `STAGE-4-TASK.md` §11 requires SSIM ≥ 95% against
   *approved* references whose provenance §9.3 leaves open (Word/LibreOffice).
5. **Output size / safety.** Pathological documents must not exhaust memory.

## Decision

### Font metrics: a bundled deterministic provider (open question 1)

Layout depends on a `FontProvider` trait returning [`FontMetrics`]. The default
is `BuiltinFontProvider`: a **data-only** model with per-character-class advance
ratios (narrow/wide/digit/uppercase/CJK) and per-family width scaling
(monospace/sans/serif). No font binaries are bundled. A system-font provider is
left behind the `system-fonts` feature and is explicitly **outside** the Stage-4
acceptance criteria. Consequences: byte-identical output across OSes and CI, at
the cost of exact glyph metrics (a documented limitation; real metrics tables
would be a data-only addition later).

### Style cascade lives in the render crate (open question 2)

`strict-ooxml-render-svg::style` computes `ComputedParagraph`/`ComputedRun`
from `docDefaults` (built-in defaults) → paragraph style `basedOn` chain → the
style → direct `pPr`/`rPr`, and for runs the character style chain plus `rPr`.
`TriState::Absent` inherits. The Stage-2 `wml` DOM is **not** changed: the
cascade is a rendering concern and keeping it here avoids destabilising the
DOM and its golden tests (ADR-0004).

The Stage-2 `StyleTable` retains the resolved `based_on_chain`; `docDefaults`
are not in the DOM, so effectively the built-in defaults play that role. A
future `docDefaults` capture is additive.

### Media resolution: `MediaSource` + explicit modes (open question 4)

`render(document, options)` keeps the `STAGE-4-TASK.md` §6 signature and uses
`RenderOptions::media`:

- `EmbedDataUri` (**default**): bytes are Base64-inlined (`data:` URI); the SVG
  is self-contained. A tiny in-crate Base64 encoder avoids a dependency.
- `ExternalFiles`: `<image xlink:href="imageN.png">`; `media_file_name` exposes
  the mapping for the caller to write the files.
- `None`: a placeholder rectangle.

Because a bare `Document` cannot read bytes, `render` uses placeholders and a
second entry point `render_with_media(document, options, Some(&package))` (used
by the meta-crate and CLI) inlines/links the real bytes. This satisfies the
specified API while remaining functional.

### Pagination and scope

Page geometry comes from the last `sectPr`; page breaks are explicit
(`w:br type=page`), `pageBreakBefore`, and content overflow. `keepLines` is
honoured; `keepNext`, multi-column flow, footnotes, fields, headers/footers,
`wp:anchor`, EMF/WMF and math are **out of scope** for Stage 4 (recorded by the
Stage-3 support model). Output is capped (`MAX_PAGES`, `MAX_ITEMS`) and every
coordinate is finite.

### SSIM references and the waiver (open question 3)

The acceptance criterion “SSIM ≥ 95% against approved references” needs an
external rasterizer and externally produced reference images (Word/LibreOffice).
Neither is available in the current environment. Decision:

- The SSIM metric is implemented in `tests/ssim.rs` (pure Rust, no deps) and
  self-tested (identical images → 1.0, perturbed → < 1.0).
- The harness entry point rasterizes through a caller-provided grayscale buffer
  list; wiring a specific rasterizer (`resvg`) and committing approved
  references is **deferred** and recorded as a waiver in `docs/stage-4-report.md`
  (mirroring the Stage-2 fuzz waiver, M8). Structural/deterministic/validity
  oracles are fully enforced now; the SSIM gate is the only waived item.

This mirrors the `STAGE-4-TASK.md` §12 risk note: fix the reference source and
tolerances or revisit the criterion with the customer.

## Alternatives considered

1. **System fonts / `fontdb`.** Rejected as the default: breaks determinism and
   CI reproducibility.
2. **Compute the cascade in `wml`.** Rejected for Stage 4: it changes the DOM
   contract and Stage-2 golden tests; the cascade is a rendering concern.
3. **Raster SVG→PNG and compare to committed references in CI with `resvg`.**
   Deferred with the waiver: no approved references and rasterizer fonts make
   the comparison non-reproducible today.
4. **Hand-written JSON/XML for SVG.** Rejected: the writer is small but the
   escaping/coordinate rules are easier to keep correct with a dedicated module
   and an independent XML parser as oracle.

## Validation

- `tests/render.rs` — structure, formatting, page geometry, selection, scale,
  explicit page break, multi-page overflow, no `NaN`/`inf`.
- `tests/svg_oracle.rs` — independent `roxmltree` XML/SVG parse and structural
  invariants (viewBox, finite/within-bounds coordinates); corrupted SVG
  rejected (self-check).
- `tests/determinism` (inside `render.rs`) — two runs byte-identical; output is
  independent of insertion order.
- `tests/images.rs` — data URI / external file / placeholder modes.
- `tests/golden.rs` — commit-checked SVG snapshots (format lock).
- `tests/corpus.rs` — no panics on the Stage-1 corpus; Transitional refused.
- `tests/ssim.rs` — metric self-test; external-reference harness (waiver).
- `benches/render.rs` — 10/100/500-page layouts.
- CI: SVG-validity oracle, corpus no-panic, `cargo-deny`, coverage of
  `strict-ooxml-render-svg` ≥ 80% lines.
