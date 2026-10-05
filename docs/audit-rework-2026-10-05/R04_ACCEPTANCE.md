# R04 ACCEPTANCE — Wrap / container geometry (F08/F14/F15)

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `b413b1594447458fac8101093b6502eacf4ff661` (dirty tree) |
| Layout-tree fingerprint | `f594ef4e9641b55bdb040c0671180f53b7c5926334ab712ae3e8db85b146c9fe` |
| Probe script SHA-256 | `693c074e4a0a3edffb7fb20f6957b1515012a8cd3296b89be29c8d16c99783ed` |
| Toolchain | `cargo +1.92.0` |
| Result | **PASS** |

Source SHA-256 (post-fix): see `target/audit-rework-2026-10-05/R04/green/source-hashes.txt`.

## Bugs (RED)

Script: `python docs/audit-review-2026-10-05/create_wrap_probe.py`  
Test: `cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test acceptance_wrap_probe -- --nocapture`

| Case | Observed |
|---|---|
| Square + FOLLOWING paragraph | text `(216,127.868)` inside image `(192,96,169,169)` |
| TopAndBottom | text baseline `109.965` while object bottom `265` |
| Root causes | exclusions only from current paragraph; page-relative square skipped; `free_spans` used fixed `20` px; TopAndBottom reserved height only after the host paragraph |

Log: `target/audit-rework-2026-10-05/R04/red/acceptance_wrap_probe.log` — 1 passed, 2 failed.

## Fix

1. **Page exclusions** (`layout/exclusions.rs` + `paginate`): Square/TopAndBottom boxes are registered in page coordinates after the host paragraph and applied to later paragraphs (cleared on page break).
2. **Shared extent** (`floating::resolved_extent` / `page_exclusion`): wrap boxes use the same `sizeRel` + fallback extent resolver as paint; page/margin/column origins go through `resolve_h` / `resolve_v`.
3. **TopAndBottom**: full-width exclusion; empty free intervals advance with a spacer `Flow::Block` before text (no post-hoc `add_vspace` only).
4. **Line height**: `free_spans` intersects exclusions with the provisional line height from `resolve_line_metrics`, not a fixed 20 px.
5. **Tight/Through**: remain explicitly Unsupported (warning; rectangle is not applied as a contour).

## GREEN

```
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test acceptance_wrap_probe --test f15_wrap --test f14_anchors -- --nocapture
cargo +1.92.0 clippy -p strict-ooxml-render-svg --locked --all-targets --no-deps -- -D warnings
```

| Check | Result |
|---|---|
| `acceptance_wrap_probe` (3 tests) | 3/3 pass |
| `f15_square_changes_line_intervals` | pass (ink outside expanded box, tolerance 0.25 px) |
| `f14_anchors` (2 tests) | 2/2 pass |
| clippy `-D warnings` (`--no-deps`) | pass |

Logs: `target/audit-rework-2026-10-05/R04/green/`.  
Probe source copy: `target/audit-rework-2026-10-05/R04/artifacts/acceptance_wrap_probe.rs` (and `docs/audit-review-2026-10-05/acceptance_wrap_probe.rs`). Temporary `tests/acceptance_wrap_probe.rs` deleted after GREEN.

## Inverse

RED receipt is the behavioural inverse of the page-exclusion / TopAndBottom skip model. Restoring “current-paragraph exclusions only + fixed 20 px + post-host TopAndBottom vspace” reproduces the two failures. Notes: `target/audit-rework-2026-10-05/R04/inverse/`.

## Limits

- Commit/push not performed (owner permission required).
- Tight/Through contours remain Unsupported by design (R04 scope).
- Full F15 matrix (multi-object, table/frame hosts, behindDoc) not claimed beyond the acceptance probe + existing f14/f15 tests.
- R05 shaping / R06 header-footer convergence are out of scope.
