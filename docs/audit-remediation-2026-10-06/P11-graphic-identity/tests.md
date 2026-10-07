# P11 — graphicData URI и pic identity

```powershell
cargo +1.92.0 test -p strict-ooxml-write --test p11_graphics --locked --offline
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| T-P11-1 известная пара URI picture/lockedCanvas/chart/diagram (transitional → purl) | PASS |
| T-P11-2 URI другого vocabulary не считается той же парой | PASS |
| T-P11-3 `bwMode` пишется явно, включая `auto`; сброс значения по умолчанию не принимается | PASS |
| First-Steps: `id="150"`, `bwMode="auto"`, `cstate="print"` | PASS |

Полный корпус 221: `a:graphicData@uri` 109 → 0, `pic:spPr@bwMode` 74 → 0, `spPr@bwMode` 17 → 0, `cNvPr@id` 1195 → 0, `a:blip@cstate` 46 → 0. `unmatched_schema` 1198. unclassified 4032 → 784. Exit 1. D05 не PASS.
