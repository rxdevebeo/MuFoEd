# R05 ACCEPTANCE — Unified shaping / face / resource (F07)

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `b413b1594447458fac8101093b6502eacf4ff661` (dirty tree) |
| Dirty-tree fingerprint (`git status --porcelain`) | `ce9b72df680ffb99750da24c848bed9d20d6a85d38e23908525fff96bd636526` |
| Toolchain | `cargo +1.92.0`, `rustybuzz 0.20.1`, `sha2` |
| Result | **PASS** (with honest complex-script degraded) |

Source SHA-256 (post-fix): `target/audit-rework-2026-10-05/R05/green/source-hashes.txt`.

## Bug (RED)

`LayoutContext::measure` summed per-character `FontProvider::advance_em` (hmtx). No `rustybuzz` dependency; SVG `<text>` used a single `x` and let the viewer re-shape. F07 requires one shaper + one face resource for layout / SVG / PDF.

Behavioural inverse (still in tree): `inverse_naive_per_char_sum_is_not_shaped_for_kern_pair` — for Carlito `"To"`, shaped advance ≠ naive hmtx sum.

## Fix

1. **`font/face.rs`** — `ResolvedFace` + SHA-256 `resource_hash` of the exact bundled program bytes.
2. **`font/shape.rs`** — rustybuzz shaping; cluster advances; Unicode↔cluster map; `ShapeStatus::DegradedComplexScript` for Arabic/Indic/… with `COMPLEX_SCRIPT_WARNING`.
3. **`LayoutContext::measure`** — uses `shape_text` / shaped em × size_px; records complex-script warning once.
4. **SVG paint** — multi-scalar `<text x="…">` from `unicode_x_positions_px` (ligature clusters share start x); `@font-face` CSS includes `/* resource_hash=… */`.
5. **PDF** — unchanged embed path via `face_source` (same bytes / hash as layout+SVG).
6. **Lock** — workspace `rustybuzz = "0.20.1"`, `sha2`; `Cargo.lock` updated for those packages only (+ transitive).

## GREEN

```
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --lib font::
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test f07_fonts --test f14_anchors --test f15_wrap -- --nocapture
cargo +1.92.0 test -p strict-ooxml-render-pdf --locked --lib font -- --nocapture
cargo +1.92.0 clippy -p strict-ooxml-render-svg --locked --all-targets --no-deps -- -D warnings
```

| Check | Result |
|---|---|
| `font::` unit (incl. shape/face/inverse) | pass |
| `f07_fonts` (3) | 3/3 pass |
| `f14_anchors` / `f15_wrap` | pass (f15 oracle reads first cluster `x`) |
| PDF font subset/embed lib tests | 6/6 pass |
| clippy `-D warnings` | pass |

Logs: `target/audit-rework-2026-10-05/R05/green/`.

## Inverse

`font::shape::tests::inverse_naive_per_char_sum_is_not_shaped_for_kern_pair` — log `target/audit-rework-2026-10-05/R05/inverse/shape-inverse.log`.

## Limits

- Complex scripts (Arabic/Urdu/Tamil/Hindi, …) are shaped via rustybuzz but marked **degraded**; full F07 complex-script matrix / browser load gate remain R09.
- Commit/push not performed (owner permission required).
- Golden SSIM not re-baselined in this receipt; shaping may change line breaks vs pre-R05 widths — R11 corpus gate owns that.
