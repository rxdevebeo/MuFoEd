# Baseline status — 2026-10-05 (post R01–R11)

Short companion to [`R11_ACCEPTANCE.md`](R11_ACCEPTANCE.md). Historical audits stay history; this is the current planning floor.

## Snapshot

| Field | Value |
|---|---|
| HEAD | `3d073ba6716380a96407a92b5a96d4fdab49bafd` (R10 on `master`) |
| Dirty FP | `2f8b8d7e0f9a1a0c42e373468bb753ae30378b3fb0612a0962919db9754468e6` |
| vs `origin/master` | 16 commits ahead (`7a8e6d3…`) |
| External CI exact SHA | **NOT_RUN** |
| Overall audits | **PARTIAL** — not closed |

## Card rollup

PASS: R01, R02, R03, R04, R05 (degraded complex scripts), R06, R09.  
PARTIAL: R07 (no Word oracle), R08 (`full_audit=false`, 15 blocked), R10 (wml branch <70%; fuzz/CC0 CI NOT_RUN).  
R11: **PARTIAL** baseline recorded.

## Accepted now

Integrated fixes for FEFF text, CC0 Strict owned-schema=0, census schema/inventory split, Square/TopAndBottom wrap, shared shaping, active header/footer convergence, F16 synthetics vs WPS, honest F21 receipts, browser loss/font gate, §15 line coverage floors (Linux).

## Still open / degraded / blocked

- Word F16 oracle; 15 matrix blockers; Tight/Through unsupported.
- Complex scripts degraded; CC0 full-page + multilingual layout not accepted.
- wml branch 63.79%; fuzz 3600s NOT_RUN; exact-SHA CI NOT_RUN.
- `docs/waivers.toml` entries remain open unless individually exited — **not** auto-closed.

## Next plan inputs

Owner push → CI on exact SHA; branch coverage or waiver; Word reference; matrix/inverse closure; all-pages CC0 + refreshed Strict/Transitional gates.
