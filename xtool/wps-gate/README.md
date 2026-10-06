# WPS geometry gate (P0)

Absolute glyph-origin matcher for WPS PDF vs our SVG. Tolerance is **0.25 px**.
`AMBIGUOUS` is never PASS.

```
python xtool/wps-gate/wps_ledger_selftest.py
python xtool/wps-gate/wps_ledger.py --root <repo>
```

Ledger output: `docs/audit-remediation-2026-10-06/wps-ledger.json`.

Reading WPS reference PDFs needs `pdfplumber`. Selftests use synthetic glyphs only.
