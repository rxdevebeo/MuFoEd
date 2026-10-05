# R07 ACCEPTANCE — Scientific compositions / paragraph frames (F16)

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `c34da65346e434b266eadc3f9392e660084b53a3` (dirty tree; R01–R06 already on master) |
| Dirty-tree fingerprint (`git status --porcelain` SHA-256) | `a25879c3ecfedc9139489c8ce0270cdb524e5c078052a1c81ee942a1bdbe716a` |
| Toolchain | `cargo +1.92.0 --locked` |
| Result | **PARTIAL** |

Source SHA-256 (post-fix): `target/audit-rework-2026-10-05/R07/green/source-hashes.txt`.

| Path | SHA-256 |
|---|---|
| `strict-ooxml-render-svg/src/layout/table.rs` | `d1fba9d1ed9932690eeb83114cadaef8d1fb392fbc4364ca2f2236d939664799` |
| `strict-ooxml-render-svg/src/layout/paginate.rs` | `00ed938cde071c2a8115bb49a9cb19bef3a06530cd7481e2bb0f1d969edf5c3a` |
| `strict-ooxml-render-svg/tests/f16_frames.rs` | `e02610358830887127a6e926c3460e8ee1f0c65a49e8b8d47a889f0664a25d76` |

Inputs:

| Input | SHA-256 | Producer |
|---|---|---|
| Clio Der Sarkissian 2011.docx | `4c5b9b178bdc8c3abae865f00ab5aaa9e102f81634e53691afc3cd3073162462` | thesis source |
| `visual-thesis/wps-reference/manifest.json` | `763e7b969590c367f647d5916a872a4828424e6e85f0c1801e73cd77eca500f7` | **WPS Office 12.1.0.28485** via `kwps.Application ExportAsFixedFormat` (not Word) |
| `page-54.json` / `page-56.json` / `page-104.json` | see `source-hashes.txt` | same WPS export, y from page top, px = 96/72 × pt |

## Bug (RED)

Page-anchored `w:framePr` on **every cell of a fixed-layout table** was treated as independent stacked frames. `cell_frame_items` + `frame_resume` painted all SNPs at the shared origin `(5395, 6370)` twips.

Log: `target/audit-rework-2026-10-05/R07/red/f16_table.log`

| Case | Observed on pre-fix tree |
|---|---|
| Synthetic 2-col tree (`8994` / `11719`) | `8994` x=`362.333`, `11719` x=`361.0` (collapsed column; expected `424.733`) |
| Thesis p.56 (CLOSURE) | polymorphism labels Δx ≈ −359 px, Δy ≈ −425 px vs WPS; tree not assembled |
| Thesis p.54 (CLOSURE) | 7 text origins outside the page; primer dy ≈ −3.9 px |
| Thesis p.104 | JPEG bytes kept; composition not proven |

Inventory: `target/audit-rework-2026-10-05/R07/red/inventory.txt` (frame signatures, indents, 4-col SNP grid `946+470+931+965` twips = frame `w=3312`).

## Fix

1. **`table_uniform_frame`**: if every cell paragraph shares one `FrameProperties`, the table is one framed figure.
2. **`framed_table_items` / `place_framed_table`**: lay the table out at the frame origin with `escape_frames=false`, so column x, `trHeight` exact, and cell borders become the phylogeny edges.
3. Mixed cells (unframed + framed) still escape framed paragraphs only (`f16_page_frame_inside_a_cell_uses_the_page_origin`).

## GREEN

```
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test f16_frames --test f07_fonts --test f13_regions --test f14_anchors --test f15_wrap -- --nocapture
cargo +1.92.0 clippy -p strict-ooxml-render-svg --locked --all-targets --no-deps -- -D warnings
target/release/strict-ooxml.exe render --transitional --pages 54-56 --scale 96 <clio.docx>
target/release/strict-ooxml.exe render --transitional --pages 104 --scale 96 <clio.docx>
```

| Check | Result |
|---|---|
| `f16_frames` (6) | 6/6 pass, 0.25 px |
| `f07_fonts` / `f13_regions` / `f14_anchors` / `f15_wrap` | pass (no R04–R06 regression) |
| clippy `-D warnings` (`--no-deps`) | pass |
| CLI render pages 54–56, 104 | SVG written; CLI exit 1 is documented Transitional **lossy** `stylesWithEffects`, not a layout panic |

Logs: `target/audit-rework-2026-10-05/R07/green/`.

### Measurements vs WPS (96 px/in)

**Page 56 — SNP table (relative to `8994`, topology).** Before: `11719` at the frame stack (~x 6–361). After:

| Label | SVG x | WPS x | abs Δx | rel Δx vs 8994 | rel Δy vs 8994 (baseline − y_top) |
|---|---:|---:|---:|---:|---:|
| 8994 | 363.000 | 362.24 | 0.76 | 0 | 0 |
| 6371 | 363.000 | 362.24 | 0.76 | 0.000 | −0.147 |
| 12705 | 363.000 | 362.24 | 0.76 | 0.000 | −0.014 |
| 11719 | 424.733 | 424.00 | 0.73 | −0.027 | −0.147 |
| 14766 | 456.067 | 455.20 | 0.87 | 0.107 | −0.120 |
| 7028 | 518.133 | 517.28 | 0.85 | 0.094 | −0.093 |
| 4580 | 518.133 | 517.28 | 0.85 | 0.094 | −0.106 |

Relative SNP positions/baselines are **≤ 0.25 px**. Absolute x is ~10 twips (cell start margin) vs WPS PDF. Clade column x=`582.333` vs WPS `582.24` (Δx **0.093**). Clade-to-clade rel Δy ≈ **−2.5 px** (mixed `w:spacing` / exact 538). Tree edges: **38** SVG lines (was “lines present ≠ tree”). Caption words `Single`/`Nucleotide` start at x=`798.226` (page width 793.73) — still overflow.

**Page 54 — primer frame.** 0 text origins outside the page (was 7). `H16142` Δx **0.093**, `H16233` Δx **0.111**, rel Δy between those two **0.000**. `HVR-I` Δx **0.862** (centering vs WPS face). 12 leader/rule lines.

**Page 104 — maps.** Two JPEG, SHA-256 equal to `word/media/image6.jpeg` and `image7.jpeg` (bytes unchanged). Boxes vs WPS `LTImage`: `(447.2, 121.8, 182.4, 153.6)` vs `(447.04, 121.76, 182.4, 153.6)`; `(443.333, 353.133, 182.4, 144)` vs `(443.2, 353.12, 182.4, 144)` — **≤ 0.25 px**. Captions `A,C:` Δx **3.06**, `B,D:` Δx **5.14** (same class as CLOSURE; not the JPEG). WPS PDF also has OCR-like overlay strings; source has separate overlay frames.

Pairwise JSON: `target/audit-rework-2026-10-05/R07/green/pairwise.json`.

## Inverse

- RED `f16_shared_page_frame_table_keeps_column_offsets` on the escaped-cell path: `11719` x=`361` vs `8994` x=`362.333`. Log: `target/audit-rework-2026-10-05/R07/red/f16_table.log` and `target/audit-rework-2026-10-05/R07/inverse/collapsed_columns.log`.
- GREEN `f16_inverse_escaped_cell_frames_are_not_the_table_origin`: production path must keep Δx > 50 px.

## Limits (why not PASS)

- **No Word reference.** Oracle is WPS 12.1.0.28485. Word-profile completeness remains **BLOCKED** until a Word export of pages 54/56/104 exists; that remainder is not treated as WPS PASS.
- SNP **absolute** x vs WPS is 0.73–0.87 px (`tblCellMar` 10 twips). Tolerance 0.25 was **not** relaxed; relative topology meets it.
- Page 54 `HVR-I` and page 56 clade vertical rhythm / caption overflow still exceed 0.25 px vs WPS.
- Page 104 caption x still ~3–5 px; overlay “unreadable” strings are in the DOCX frames, not dropped JPEG.
- Full-document `to-pdf` of 328 pages was not re-run; SVG and PDF share `layout_document` / `place_pages`. Geometry assertions use `place_pages` items (same backend as PDF paint).

## Status

**PARTIAL** — F16 synthetic witnesses PASS; thesis page 56 tree is assembled with SNP relative geometry ≤ 0.25 px vs WPS; page 54 overflow closed; page 104 raster boxes match WPS. Remaining WPS residuals and the absent Word profile keep the card from PASS.
