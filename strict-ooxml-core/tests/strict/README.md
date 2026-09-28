# Real Strict OOXML corpus (`tests/strict/`)

Real, externally produced **WordprocessingML Strict** documents
(`purl.oclc.org/ooxml/...` namespaces), as opposed to the synthetic Strict
packages built in memory by the tests and the Transitional corpus in
`../samples/`.

## Contents

| File | Origin | License | Status |
|---|---|---|---|
| `strict-profile.docx` | [kklimuk/docx-cli](https://github.com/kklimuk/docx-cli) — `tests/fixtures/strict-profile.docx` | MIT (© 2026 Kirill Klimuk) | committed |
| `strict-text.docx` | Built for this project (Strict namespaces) | MIT OR Apache-2.0 (project) | committed |
| `strict-text-grid.docx` | `strict-text` + `w:docGrid` + a Cambria run | MIT OR Apache-2.0 (project) | committed |
| `refs/` | WPS Office reference renders (see `refs/README.md`) | project | committed |

`strict-profile.docx` is redistributed under the MIT License of its source
repository; the original license text is preserved in the source project. It is
used here only as a test fixture.

`strict-text.docx` is a small text-only Strict document authored for this
repository (Calibri/Arial/Times/Courier runs, justification, centering) so the
renderer can be compared pixel-for-pixel against an external engine.

> Note: an additional real Strict fixture was found in
> `Esword618/unioffice` (`document/testdata/strict.docx`), but that repository is
> **AGPL-3.0**, which is incompatible with this project's MIT/Apache-2.0
> licensing, so it is **not** committed (local use only).

## Status

`strict-profile.docx` **parses**: the Stage-2 parser now skips a whitespace,
comment or processing-instruction prolog before the root element
(`REWORK-WML-1.md`, finding C-3). It is exercised end to end (open, Feature
Report, SVG render) by `strict-ooxml/tests/strict_corpus.rs`, and the
declaration/newline regression is covered by
`strict-ooxml-wml/tests/prolog.rs`.

The document reports `unsupported` for mechanisms it does not model (charts,
diagrams, anchored drawings and various `settings.xml` extensions), so `check`
exits `1`; opening succeeds and `render` emits two pages (the chart/diagram
extents are reserved for pagination).

## Visual fidelity (S4F)

`strict-ooxml-render-svg/tests/ssim.rs` rasterizes our SVG with `resvg` using the
bundled metric-compatible fonts and compares it with the committed WPS
references: `strict-text` and `strict-text-grid` must reach **SSIM ≥ 0.95** plus
the structural invariant (ink coverage, row-ink correlation, ≤ 2 px vertical
drift), and every document must render the **same number of pages** as its
reference. `strict-profile` is page-count only (its DrawingML charts cannot be
rasterized by Stage 4). See `refs/README.md`.
