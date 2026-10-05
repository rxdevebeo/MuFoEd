# R11 ACCEPTANCE - Final baseline and audit closure status

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `3d073ba6716380a96407a92b5a96d4fdab49bafd` |
| Branch | `master` |
| Dirty-tree fingerprint (`git status --porcelain` SHA-256) | `2f8b8d7e0f9a1a0c42e373468bb753ae30378b3fb0612a0962919db9754468e6` |
| `origin/master` | `7a8e6d37d4d96543db00a9c4f2be51f879397c1f` |
| Commits ahead of `origin/master` | 16 |
| CC0 manifest SHA-256 | `68b0aead98882ac1d85bdaf24eda015a6eae06b01fd1f582b2f1321e86ed5745` |
| Toolchain (card family) | `cargo +1.92.0 --locked` |
| Result | **PARTIAL** |

Dirty porcelain at measurement (before writing this card):

```
?? docs/audit-review-2026-10-05/ci-last-green.json
?? docs/audit-review-2026-10-05/ci-runs.json
?? docs/audit-rework-2026-10-05/R07/
```

Artifacts: `target/audit-rework-2026-10-05/R11/` (`identifiers.txt`, `git-status-porcelain.txt`, `commands.txt`, `receipt.json`, `waiver-ids.txt`).  
Companion summary: [`BASELINE_STATUS_2026-10-05.md`](BASELINE_STATUS_2026-10-05.md).

**No production code was changed in R11.** Commit/push not performed (owner permission required).

## Verdict

**PARTIAL** — R01–R10 deliver integrated fixes and measured local evidence, but the order’s full-closure criterion is not met. External CI for exact HEAD is **NOT_RUN**. Residual PARTIAL cards (R07/R08/R10), blocked matrix rows, unsupported primitives, and open waivers remain. This baseline is a planning floor, **not** “all audits closed”.

## R01–R10 outcomes (from acceptance cards only)

| Card | Result | One-line limit / residual |
|---|---|---|
| R01 | **PASS** | FEFF text corruption fixed; does not claim R02 XSD closure |
| R02 | **PASS** | CC0 Strict outs: ours=0 unmatched=0; 7 source XS-16 chart carries; 27 Strict + 121 Transitional full XSD not re-run in that receipt |
| R03 | **PASS** | Schema vs inventory separated; 121 census clean; waived TZ dispositions are named transforms, not silent losses |
| R04 | **PASS** | Square/TopAndBottom exclusions across paragraphs; Tight/Through remain **Unsupported** |
| R05 | **PASS** (honest degraded) | Unified shaping/face hash; complex scripts marked **degraded** |
| R06 | **PASS** | Active header/footer + ≤8 converge / `DidNotConverge`; no live oscillating fixture |
| R07 | **PARTIAL** | F16 synthetics PASS vs WPS; **no Word oracle** (BLOCKED); LibreOffice ref + Word Online preview under `R07/` untracked |
| R08 | **PARTIAL** | F21 behavioral suite pass with `full_audit=false`; 69 measured / **15 blocked**; inverse BLOCKED for most cards |
| R09 | **PASS** | Live CDP browser F07/F20; missing browser/Node → BLOCKED (not PASS) |
| R10 | **PARTIAL** | All §15 **line** floors PASS on Linux; **wml branch 63.79% < 70%**; fuzz 3600s and CC0 in CI **NOT_RUN** |

Sources: `docs/audit-rework-2026-10-05/R01_ACCEPTANCE.md` … `R10_ACCEPTANCE.md`. No PASS invented beyond those cards.

## Accepted vs degraded / unsupported / blocked / not_run

### Accepted (code / measured local evidence)

- U+FEFF mid-document text preservation (R01) + CC0 fixed_point 100/100 on that path.
- CC0 Strict write: owned schema violations 0; unmatched 0; 7 intentional source chart carries (R02).
- Census: schema and inventory dispositions separated; 121 Transitional measured clean (R03).
- Square / TopAndBottom wrap across following paragraphs (R04 probe + f14/f15).
- Shared rustybuzz shaping + face resource hash for layout/SVG/PDF (R05).
- Active header/footer body reserve + convergence contract (R06).
- F16 synthetic frames; thesis table topology vs WPS within stated tolerances (R07 partial acceptance of synthetics only).
- Honest F21 receipts (`full_audit=false` when matrix blocked) (R08).
- Browser DOM loss UI + Carlito face load (R09).
- §15 package **line** coverage floors + core file 100% line gates on Linux (R10).

### Degraded

- Complex-script shaping: rustybuzz path marked degraded (R05).
- CC0 write outcomes historically include many Degraded (exit 1) documents — Clean is not claimed for lossy Transitional→Strict.
- R07 absolute/caption residuals vs WPS remain (relative SNP geometry accepted within 0.25 px where stated).

### Unsupported (by design / out of order scope)

- Tight / Through wrap contours (R04; not claimed as rectangles).
- Cubic / clip / pattern as new supported primitives (order §4).
- RTF / DjVu importers; full editor K00–K15 (order §4).
- AUD-99 vendored hayro removal pending equivalent upstream.

### Blocked

- R07 Word-profile geometric oracle (pages 54/56/104) until Word export exists.
- R08: 15 FIX_PLAN matrix rows; public-manifest inverse for most cargo cards.
- R09 host without Chromium-family browser or Node.
- Owner commit/push → blocks external CI on exact HEAD.

### NOT_RUN

- External CI for HEAD `3d073ba6716380a96407a92b5a96d4fdab49bafd` (Ubuntu/macOS/Windows, coverage, XSD, MSRV, deny, fuzz).
- Fuzz 3600s × 8 (schedule `fuzz-nightly`; R10 acceptance did not run it).
- CC0 corpus matrix **in CI** (`testdata/` gitignored; local-only, same class as census).
- Full-page / all-pages CC0 render gate (historical probe: **first page only**).
- Faithful multilingual layout acceptance for CC0 (explicitly not accepted by order / AUDIT_ACCEPTANCE).
- R02 note: full 27 Strict + 121 Transitional XSD re-validation not in R02 receipt (R11 records that gap; not re-executed here).

## Code acceptance vs product completeness

| Axis | Status |
|---|---|
| **Code acceptance** | Integrated R01–R10 fixes on `master` HEAD `3d073ba…`; local RED/GREEN/inverse receipts exist per card; workspace regressions were the working base at order start and were not re-litigated as open defects here. |
| **Product completeness** | Not complete: Word F16 oracle missing; 15 matrix rows blocked; Tight/Through unsupported; complex scripts degraded; wml branch coverage below contract; long fuzz and exact-SHA external CI unproven; CC0 full-page render and multilingual fidelity not accepted. |

A green harness or synthetic F21 suite does **not** equal full audit closure when `full_audit=false` or required corpora/CI are missing.

## Corpus gates (what was measured vs not)

| Corpus | Required by R11 | Measured in R01–R10 / prior acceptance | Not measured / not accepted |
|---|---|---|---|
| **27 Strict** | yes | Historical AUDIT_ACCEPTANCE / order base: 26 written/validated, 1 expected G-10 refuse, OPC 0; R02 did not re-run full Strict XSD matrix | Fresh all-pages render + exact-SHA CI XSD for current HEAD |
| **121 local Transitional** | yes | R03 census PASS (`unmatched_schema=0`, `unclassified_element_changes=0`); prior XSD 0 messages on outs | Exact-SHA CI census; public F21 `--require-corpus` |
| **100 `testdata/CC0_DOCX`** | yes | Manifest SHA match; R01 fixed_point 100/100; R02 XSD ours=0 unmatched=0 source=7; R03 inventory unclassified=0; first-page render 100/100 historically | **All-pages** render; NaN scan of full multi-page SVG; CI shipping of corpus; third-party rights beyond declared CC0/provenance |

Order reminder: current CC0 probe checked **first-page** render; full-page completeness and faithful multilingual layout are **not** accepted by that measurement.

## External CI

| Claim | Status |
|---|---|
| Last green run [37195191322](https://github.com/rxdevebeo/MuFoEd/actions/runs/37195191322) | **success** on `7a8e6d37d4d96543db00a9c4f2be51f879397c1f` (older than current HEAD by 16 commits) |
| Exact HEAD `3d073ba…` on Ubuntu/macOS/Windows | **NOT_RUN** / blocked until owner push |
| In-progress scheduled CI observed | run `37293517543` still on `7a8e6d3…` — not an acceptance of R01–R10 code |
| Coverage / XSD / MSRV / deny / fuzz for exact SHA | **NOT_RUN** |

**Do not claim external CI PASS for this baseline.**

## Old waivers — re-listed, not auto-closed

Registry: `docs/waivers.toml` (revision meta `2026-10-04`). Workspace PASS / R01–R10 local greens **do not** close these. Owner decision required per entry `exit` text.

IDs present in registry (29 entries; includes historically `closed` rows such as `07-AMBER` that remain documented):

`A-1-LIMITS`, `A-5B-3`, `5C-C4`, `5C-C3`, `5C-AMBER`, `07-AMBER`, `CHARTS`, `CHARTS-2`, `PDF-LIMITS-TEXT`, `PDF-LIMITS-MIXED`, `PDF-LIMITS-GRAPHICS`, `PDF-AMBER-5B`, `PDF-AMBER-5C`, `STAGE5C-P1-LAYOUT`, `A-1`, `A-2`, `A-5B-2`, `COVERAGE-5C`, `FUZZ`, `CENSUS-LOCAL`, `DJVU-PATENT`, `COVERAGE-ELEMENTS`, `XSD-CORPUS`, `DJVU-TM`, `TBL-STYLE-PR`, `VERTJC`, `8A-CHART`, `W7-DROPPED`, `PDF-OBJSTM-BOMB`

Full list: `target/audit-rework-2026-10-05/R11/waiver-ids.txt`.

Notable process waivers still relevant to this order: `FUZZ` (long fuzz), `CENSUS-LOCAL` (local corpus), `XSD-CORPUS`, coverage-related entries, DjVu patent/TM, chart/layout ambers.

## Audit closure statement

Per `AUDIT_REWORK_ORDER_2026-10-05.md` §3 R11 and `AUDIT_ACCEPTANCE_2026-10-05.md`:

- Overall audit closure remains **PARTIAL**.
- This document establishes a **new planning baseline** on HEAD `3d073ba…` + dirty FP `2f8b8d7e…`.
- It does **not** grant status «все аудиты закрыты» / “all audits closed”.

## Residual blockers for the next plan

1. **External CI** on exact HEAD after owner commit/push (multi-OS + coverage §15 + XSD/OPC + deny + MSRV).
2. **wml branch coverage** 63.79% → ≥70% (or explicit owner waiver); threshold must not be lowered silently.
3. **R07 Word oracle** for thesis pages 54/56/104 (LibreOffice/WPS/Word Online preview are not a Word PASS).
4. **R08 matrix**: clear or own the 15 blocked rows; expand public inverse coverage; keep `full_audit=false` until done.
5. **Corpus completeness**: all-pages CC0 render (NaN-safe), 27 Strict + 121 Transitional refresh on current tree, fuzz 3600s evidence, and separate product work for Tight/Through / complex-script fidelity.

## Protocol note

R11 is documentation/baseline only. No threshold lowering, no golden updates, no CC0 corpus edits, no production code changes without returning to a specific R-card.
