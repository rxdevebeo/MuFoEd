# P1 tests

## T-P1-1 Per-point geometry ≤0.25 px

```
cargo +1.92 run -p strict-ooxml-cli --release --offline -- render --transitional --pages 54-104 \
  --out target/remediation-2026-10-06/clio-svg \
  "strict-ooxml-core/tests/docx/Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx"
python xtool/wps-gate/wps_ledger.py --root <repo>
```

Result: `p0_measurability PASS`, `p1_geometry PASS`.
All mandatory MEASURED components on Clio pages 54/56/104 have `|dx|,|dy| ≤ 0.25`.
Ledger: `docs/audit-remediation-2026-10-06/wps-ledger.json`.

Tightest margin: `p104.modern.5` abs≈0.248 (0.002 px under the cap). Do **not** raise 0.25; record as fragility for P9 font work.

## T-P1-2 Captions inside page bounds

```
python xtool/wps-gate/wps_p1_gate_selftest.py
```

PASS: every mandatory `actual_xy` lies inside SVG page `793.733 × 1122.533`.

## T-P1-3 Metric / shaped regressions

```
cargo +1.92 test -p strict-ooxml-render-svg --release --offline --test f06_toggles --test f16_frames
```

PASS: 11 + 12 tests (includes Exact ascent, pBdr collapse, character spacing wrap, bold+bCs, primer stack).

## T-P1-4 Negative — forced layout offset

```
python xtool/wps-gate/wps_p1_gate_selftest.py
```

PASS: page-56 SVG glyphs shifted +1 px in x stay `MEASURED` and **FAIL** the 0.25 px geometry gate (at least one FAIL point).

Matcher-level 1 px negative remains in `wps_ledger_selftest.py` (T-P0-4).

## Hashes

Clio DOCX SHA-256 matches the plan.
WPS PDF SHA-256 values match `wps-absolute-baselines.json` / P0 witnesses (reference unchanged).
SVG SHA-256 values are the post-P1 renders listed in `witnesses.json`.

## Re-check 2026-10-08 (after the hardening)

Run by the hostq `wps` job, which renders Clio pages 54-104 with `--wps-times` (the calibration
P1 is measured with; it became opt-in in `1f31fac`) and with the defaults, then runs
`wps_ledger.py` and `wps_p1_gate_selftest.py`.

- Before the fix (`fe582d4`): p. 56 FAIL - five bold Arial 4.5 pt position labels 0.52-0.67 px
  off in dy. `9d9b372` had limited the WPS first-Exact-line rule to frames, and these labels sit
  in a table whose paragraphs share one `w:framePr`, which was laid out outside the frame flag.
- After `93b054a` (framed tables lay out under the frame rule): `p1_geometry PASS`, 26/26
  MEASURED ≤ 0.25 px; tightest still `p104.modern.5` 0.248 px.
- Defaults (no `--wps-times`): FAIL up to 3.4 px, as designed.
