# ADR-0008: One layout, many backends — and the cost of a second consumer

- **Status:** Accepted
- **Date:** 2026-09-30
- **Deciders:** Strict OOXML maintainers
- **Related:** `STAGE-8-TASK.md` §1.1, §4 (P1–P7), §7 (SC-6); ADR-0006 (SVG
  rendering), ADR-0004 (WML model)

## Context

The layout engine lives in the crate named `strict-ooxml-render-svg` and was
`pub(crate)`. Stage 8B needs the same pagination to emit PDF, and there are two
ways to get it:

1. render SVG, then convert SVG → PDF with a third-party tool;
2. expose the layout as an API and write a second backend over it.

Option 1 loses information by construction: an SVG rasterizes text or demands
manual font substitution, so the PDF would carry pictures of letters — and
`SC-7` («текст в PDF живой») makes that a failure, not a trade-off. It also drags
a `usvg` rasterizer into the graph. The order says the same thing
(`STAGE-8-TASK.md` §4).

The cost of option 2 is that the layout stops being one consumer's private
detail. Anything in it that is *really* SVG — a path string, a px coordinate, a
font hint — becomes a contract with someone outside the crate.

## Decision

### `place_pages` is the API; `Item` is the vocabulary

`render_svg::place_pages(&Document, &RenderOptions, Option<&dyn MediaSource>) ->
Vec<PlacedPage>` is public, and `PlacedPage { width_px, height_px, items }`
carries `Item::{Text, Rect, Line, Image, Path}` — five primitives that name no
output format. `strict-ooxml-render-pdf` is a second backend over exactly these
values.

The consequence that matters is not convenience, it is that **the two backends
cannot disagree about pagination**: they are handed the same page list. SC-6's
«число страниц совпадает» is therefore true by construction rather than by
testing, and the test is there to catch the day someone gives the PDF backend its
own pagination.

`TextItem::field` is public for the same reason: a backend that draws text needs
to know a fragment is a page number so it can resolve the marker.

### One conversion point for units

`place_pages` speaks px; the PDF speaks pt. `render-pdf::units::px_to_pt` is the
only place that converts. If the engine ever hands out points, that function is
deleted rather than kept alongside a second conversion — two conversion points is
one too many, and the Q-2 rule exists so the decision is not re-litigated per
call site.

### `PathItem` carries its outline as a string — a conscious trade

A DrawingML custom geometry reaches the layout as SVG path data, and `PathItem`
keeps that string for the PDF backend to parse back
(`render-pdf/src/path.rs` is the only reader). The alternative is a structured
path type in the layout, which would have meant touching the Stage-4/5B renderer
and its SSIM gate to serve a second consumer.

This is the one place where the backend-neutral vocabulary is not neutral, and it
is recorded as a debt with a rule attached: **do not repeat it for HTML or
Markdown.** Those sources have their own structure, and a path string in a
converter whose input is `<rect>` would be a string nobody asked for.

### The cost, and what it obliges

A second consumer means every change to `layout` is a change to two backends.
The obligations that follow, all of them in `STAGE-8-OPEN.md`:

- any change to `render-svg::layout` must pass `strict-ooxml-render-pdf --test pdf`
  in the same commit (page count and `MediaBox` must match the SVG);
- adding a field to `ImageItem` (Q-3) updates both backends in one commit;
- image resource names come from the part id, and a collision is currently
  unchecked (Q-4) — the PDF backend must switch to a counter at the first one.

The engine is still changing (a neighbouring session's 5C rework touches
`math/layout.rs` and the gate thresholds), so `Q-1` keeps the layout under
observation rather than declaring it stable.

### Deferred: the crate name

`strict-ooxml-render-svg` is a misnomer for a crate that now hosts two backends
and a public layout API. Renaming it to `strict-ooxml-render` is **deferred to
before publication on crates.io** (stage 7): it is cosmetic debt with no runtime
consequence, and renaming a crate that another session is mid-edit in would cost
more than the name does.

## Consequences

- Adding a backend is a consumer of a stable vocabulary, not a fork of the
  layout. `strict-ooxml-render-pdf` is 28 unit + 8 integration tests and touches
  no layout file.
- The layout is public API, so its shape is now a compatibility promise: the
  five `Item` variants, the `PlacedPage` fields and the `px` convention.
- A bug in the layout hits both backends at once. That is the intended coupling —
  one layout, one pagination — but it means layout regressions are *not* an
  SVG-only concern any more.
- Pixel fidelity for PDF is not gated yet (`O-2`): the shared `ssim.rs` harness is
  red on a neighbouring session's rework, so phase 8B verified «same pages, same
  `MediaBox`» and left the per-pixel comparison for the rasterizer increment.

## Alternatives considered

1. **`svg2pdf` / an SVG→PDF library.** Rejected: text comes out as curves or
   needs font substitution, and `SC-7` requires selectable text; it also adds a
   rasterizer to the dependency graph.
2. **A structured path type in the layout.** Rejected *for now*: it is the
   better long-term shape and the wrong cost right now (it destabilizes the
   Stage-4/5B renderer and its gate). Recorded as debt rather than forgotten.
3. **Keep the layout private and fork the pagination for PDF.** Rejected outright:
   two paginators is exactly the divergence `SC-6` exists to prevent.
4. **Emit PDF from the SVG string.** Rejected with option 1.

## Validation

- `strict-ooxml-render-pdf/tests/*` — page count and `MediaBox` equal to the SVG
  backend's, checked on four corpus documents (`--test pdf`).
- `strict-ooxml-render-svg/tests/ssim.rs` — the SVG-side fidelity gate; both
  backends are required to survive it.
- `strict-ooxml-pdf/tests/geometry.rs` — the reader's geometry against the writer's
  own output, at 0.01 pt (ADR-0009).
- `cargo clippy --workspace --all-targets --all-features` — the shared vocabulary
  has to compile under the same lint policy in both crates.
