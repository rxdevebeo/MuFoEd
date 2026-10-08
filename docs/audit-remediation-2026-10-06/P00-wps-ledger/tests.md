# P0 tests

## T-P0-1 Ledger

```
python xtool/wps-gate/wps_ledger.py --root D:\projects\StrictLib
```

Result: `p0_measurability PASS`. Output `docs/audit-remediation-2026-10-06/wps-ledger.json`.
Every mandatory component on Clio pages 54/56/104 has `status=MEASURED`.
Geometry ≤0.25 px was **FAIL at P0 close** and is **PASS after P1** — see `P01-wps-geometry/`.

## T-P0-2 Matcher

```
python xtool/wps-gate/wps_ledger_selftest.py
```

PASS:

- split runs `L`+`15996` glue in the glyph stream
- key `H` does not match inside `HV`
- unique `(page, text, neighborhood)` pairing
- `A,C` is a whole token

## T-P0-3 Statuses

AMBIGUOUS when two hits share the same neighborhood → geometry gate FAIL.
MISSING when the key is absent → geometry gate FAIL.
MEASURED inside 0.25 px → geometry gate PASS.

## T-P0-4 Negative

Origin shifted by 1 px stays MEASURED and fails the 0.25 px gate.

## Hashes

Clio DOCX SHA-256 matches the plan.
WPS PDF SHA-256 values match `wps-absolute-baselines.json`.
