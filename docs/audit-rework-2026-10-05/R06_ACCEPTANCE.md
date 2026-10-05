# R06 ACCEPTANCE — Active header/footer regions + convergence (F13)

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `b413b1594447458fac8101093b6502eacf4ff661` (dirty tree) |
| Dirty-tree fingerprint (`git status --porcelain` SHA-256) | `c1e4f2dba489778ef47f54c446b83fe291b7bf0d8e5eb0363047bd5180539629` |
| Toolchain | `cargo +1.92.0 --locked` |
| Result | **PASS** |

Source SHA-256 (post-fix): `target/audit-rework-2026-10-05/R06/green/source-hashes.txt`.

| Path | SHA-256 |
|---|---|
| `strict-ooxml-render-svg/src/layout/headerfooter.rs` | `1483f5be71dfc2286770fc71879d32a6932b341fa1a935af454705dbb276282e` |
| `strict-ooxml-render-svg/src/layout/paginate.rs` | `dc6ad6347b5dcecbd9e347d24a735a3b10315566cc7810c50d345ee6974c5f4d` |
| `strict-ooxml-render-svg/src/layout/mod.rs` | `a2cc3afa50416e917ebe43d7d1d1e9eab1db65b7e94cf242caadd3d75c62c92b` |
| `strict-ooxml-render-svg/src/error.rs` | `78d7e121b1bcf6d5fd1070c3aeb84c8fc96ab8396e7c7ff347d56d2df69cf8b7` |
| `strict-ooxml-render-svg/tests/f13_regions.rs` | `3d4d510516e316ebf6d2884546233e7a1dc3bbd72bbc52166ca907227dee6913` |

## Bug (RED)

`body_reserve` took `tallest_region` over **all** section header/footer refs, measured **without** `FieldEnv`, and applied that extra to every page of the section. `layout_document` retried up to 8 times on **page count only** and returned the last layout if totals never stabilized.

Log: `target/audit-rework-2026-10-05/R06/red/f13_regions.log` — 4 failed / 3 passed on the sharpened oracles (then 3 failed after digit-rollover fixture tuning).

| Case | Observed on pre-fix tree |
|---|---|
| Inactive tall default footer + `titlePg` first footer | Page 1 body only fit `FLOW0`/`FLOW1` (tallest footer stole capacity) |
| Even tall header + short odd default | Odd body baseline `230.8` (even height reserved on odd pages) |
| Negative `w:pgMar/@w:top` | No diagnostic; extras still invented |
| PAGE/NUMPAGES/SECTIONPAGES fixture | After shortening the page, totals ≥10 and ink already clear (no RED collision); still required as GREEN FieldEnv/oracle)

## Fix

1. **`geometry_for`** no longer inflates margins from the tallest unused ref.
2. **`page_body_reserve`** selects first/even/default (plus inheritance) **before** measuring; measurement uses `FieldEnv` (`PAGE` / `NUMPAGES` / `SECTIONPAGES` / `SECTION`).
3. **`Paginator::apply_page_regions`** runs at page start / page break / section break so extras are per-page.
4. Negative top/bottom or header/footer distances: warning `render.negative-page-margin: ...`; body not auto-shifted.
5. **Convergence:** fingerprint is page count + per-section counts + quantized header/footer heights. ≤8 passes; otherwise `RenderError::DidNotConverge` (not the last layout).

## GREEN

```
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test f13_regions -- --nocapture
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --lib layout::paginate -- --nocapture
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test f07_fonts --test f14_anchors --test f15_wrap --test headers --test fields --test sections_render -- --nocapture
cargo +1.92.0 clippy -p strict-ooxml-render-svg --locked --all-targets --no-deps -- -D warnings
```

| Check | Result |
|---|---|
| `f13_regions` (7) | 7/7 pass (ink/actual `TextItem` boxes, 0.25 px) |
| paginate F13 unit (2) | 2/2 pass |
| `f07_fonts` / `f14_anchors` / `f15_wrap` | pass (no R04/R05 regression) |
| `headers` / `fields` / `sections_render` | pass |
| clippy `-D warnings` (`--no-deps`) | pass |

Logs: `target/audit-rework-2026-10-05/R06/green/`.

## Inverse

- `f13_inactive_tall_footer_does_not_shrink_other_page` — restoring tallest-of-all refs fails (page 1 cannot hold `FLOW10`). Log: `target/audit-rework-2026-10-05/R06/inverse/inactive_tall_footer.log`.
- `f13_fingerprint_includes_region_size_not_only_page_count` — same page count with a taller footer is **not** converged. Log: `target/audit-rework-2026-10-05/R06/inverse/fingerprint.log`.

## Limits

- Commit/push not performed (owner permission required).
- Digit-rollover **collision** was not a RED on the old painter (decorate already relayouts with FieldEnv); the test now guards totals + ink after FieldEnv-aware **reserve**.
- Exhausting all 8 passes is covered by `DidNotConverge` display + the 8-pass constant; a live oscillating fixture was not constructed.
- Tight/Through wrap and F16 remain out of scope.
