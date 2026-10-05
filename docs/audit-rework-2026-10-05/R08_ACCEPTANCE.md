# R08 ACCEPTANCE — Matrices, RED/inverse receipts, public runner (F00–F21)

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `a439b5751fdbb60482c1cb2d98e98338c386e2bb` (dirty tree; R01–R07 already on master) |
| Dirty-tree fingerprint (`git status --porcelain` SHA-256) | `fc1e699e55be6d5a383225539d361297d429c13b909df24750a28ae5c01ad0b9` |
| Toolchain | `cargo +1.92.0 --locked`, Python 3.11+ |
| Result | **PARTIAL** |

Source SHA-256 (post-fix): `target/audit-rework-2026-10-05/R08/green/source-hashes.txt`.

| Path | SHA-256 |
|---|---|
| `xtool/audit-fixes/run.py` | `54205e51784d0dbfd63d6525222ce065d068b034bd98b8d5c20436f12e65bc1a` |
| `xtool/audit-fixes/manifest.toml` | `10a741fa1ad456e5004e0a63ffeb430efaf9811323007303001298c3da03543e` |
| `xtool/audit-fixes/matrix.toml` | `58c09e0580899783dd505e3584d9964a2907af43b2daf5900586eed6d8e9f495` |
| `strict-ooxml-testkit/tests/f21_public.rs` | `d4ba2f7defa7c15d4509794ce1079b2e6ada6db6dd131a94943eb4d645c2697c` |
| `strict-ooxml-render-svg/tests/f10_lists.rs` | `67c624f12060f517ec672f9fa36deecd8257133441f5239f8c4bc8e6d1e96140` |
| `strict-ooxml-convert/tests/f18_geometry.rs` | `e826513a0d3f550f610c4b98a19044f8c3dc6615427666c9388bfea3f4bae053` |

## Bug (RED)

1. **Receipt schema.** GREEN receipts under `target/audit-fixes` recorded only `code_sha` (old HEAD `7a8e6d3`) and no dirty-tree fingerprint. Snapshot: `target/audit-rework-2026-10-05/R08/red/schema_gaps.json`.
2. **Missing RED/inverse dirs.** Inventory showed GREEN-only for F01–F21; RED/inverse F01–F20 absent in designated dirs. Snapshot: `target/audit-rework-2026-10-05/R08/red/phase_inventory.json`. No fabricated historical receipts were invented (`red/HISTORY.md`).
3. **F21 metadata-only.** Pre-fix `--task F21` ran a single Rust test that checked manifest text/names/constants (`running 1 test` / metadata). Receipt: `red/f21_pre_fix/receipt.json`, log `red/f21_pre_fix.log`.
4. **F07 font hashes.** GREEN receipts claimed font hashes “recorded by F07 and later” but did not contain them.
5. **Multi-x oracle false FAIL.** After R05, SVG `<text x="…">` is a space-separated cluster list. `f10_lists` / `f18_geometry` parsed the whole attribute as `f64` → `0.0`. Pre-fix asserts: `red/F10_assert.log`, `red/F18_assert.log`, `red/oracle_multi_x.md`.

## Fix

1. **`run.py`**: `dirty_tree_hash`, `claim` / `full_audit`, `scenario_counts`, fail-closed `validate_receipt`, `inverse` phase alias, F07 `font_face_hashes`, acceptance runner for F21.
2. **`matrix.toml`**: every FIX_PLAN required scenario → test/oracle → `measured` or honest `blocked`.
3. **`manifest.toml`**: expanded measured matrix tests for F05–F09/F13/F14/F16; `inverse_tests` for F07/F16; F21 `kind = "acceptance"`.
4. **`f21_public.rs`**: metadata gate renamed to `f21_public_manifest_metadata`; asserts runner/matrix wiring; does **not** claim full audit alone.
5. **CI**: job renamed to “Public synthetic audit suite (F21)” with comment that green ≠ full audit when matrix blocked or corpus not required.
6. **Card oracles**: F10/F18 read first cluster `x` (same rule as F15).

Production layout/write/parser fixes from R01–R07 were not reverted.

## GREEN

```
python xtool/audit-fixes/run.py --task F00 --phase green --receipt-dir target/audit-rework-2026-10-05/R08/green/F00
python xtool/audit-fixes/run.py --task F07 --phase green --receipt-dir target/audit-rework-2026-10-05/R08/green/F07
python xtool/audit-fixes/run.py --task F21 --phase green --receipt-dir target/audit-rework-2026-10-05/R08/green/F21
cargo +1.92.0 test -p strict-ooxml-testkit --locked --test f21_public
```

| Check | Result |
|---|---|
| F00 infrastructure + receipt schema self-test | pass |
| F07 green + 18 bundled `font_face_hashes` | pass |
| F21 behavioral suite | **pass**, `claim=synthetic_public_suite`, `full_audit=false` |
| F21 counters | executed=64, passed=21 cards, failed=0, blocked_matrix=15, unmeasurable=0 |
| Metadata test | 1/1 pass |
| Matrix | 69 measured / 15 blocked / 84 total (`matrix_coverage.csv`) |

Logs/receipts: `target/audit-rework-2026-10-05/R08/green/`.

## Inverse

| Control | Result | Path |
|---|---|---|
| F00 receipt schema rejects missing `dirty_tree_hash` / bogus `full_audit` | pass | `inverse/F00/` |
| F07 `inverse_naive_per_char_sum_is_not_shaped_for_kern_pair` | pass | `inverse/F07/` (+ R05 log) |
| F16 `f16_inverse_escaped_cell_frames_are_not_the_table_origin` | pass | `inverse/F16/` (+ R07 log) |
| F21 refuses `full_audit` while matrix has blocked rows | pass | `inverse/F21/` |

Cards without compilable `inverse_tests` in the public manifest: **BLOCKED** for inverse (`inverse_coverage.csv`). Production inverses for R01–R07 remain under their R-card trees; they were not re-faked here.

## Matrix coverage summary

| Band | Measured | Blocked | Notes |
|---|---:|---:|---|
| F00–F04 primary + available matrix | yes | F03 stage combos; F04 bit/crop variants | Gates via `test_f01_gates.py` |
| F05–F06 | full existing tests in manifest | — | |
| F07 | 3 witnesses + face hashes + shape inverse | theme/tab/glyph/browser | Remainder R09 |
| F08–F09 | expanded | RTL/justify/decimal/fields | |
| F10–F12, F17–F20 | primary only | deeper FIX_PLAN matrices | |
| F13–F16 | expanded (incl. R06/R07 tests) | F15 side/distance; F16 Word schemes | |
| F21 | metadata + behavioral suite | — | claim never `full_audit` with blockers |

Corpus pass is a separate `--require-corpus` claim; this green F21 did not request a corpus.

## Limits (why not PASS)

- **15 blocked matrix rows** remain (honest FIX_PLAN gaps / R09 browser / Word reference).
- **Inverse BLOCKED** for most cargo cards lacking `inverse_tests` in the public manifest.
- Historical RED receipts for F01–F20 were **not** restored from missing archives; only real pre-fix snapshots and R01–R07 evidence are cited.
- External CI of this dirty tree was not run; CI workflow text was updated locally only.
- Commit/push not performed (owner permission required).

## Status

**PARTIAL** — receipt schema, public behavioral F21 with exact counters and non-full-audit claim, matrix inventory, font-hash recording, and oracle fixes for multi-x SVG are done. Full FIX_PLAN matrix closure and universal inverse witnesses remain open.
