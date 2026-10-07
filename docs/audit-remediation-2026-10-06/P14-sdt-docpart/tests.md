# P14 — docPart и sdtEndPr

```powershell
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| T-P14-1 `w:docPartGallery`, `w:docPartObj`, `w:docPartUnique`, `w:sdtEndPr` сохраняются, а не списываются | PASS, метки 0 |
| T-P14-2 `w:alias` без citation не совпадает ни с одним item | PASS |

`w:dataBinding` (12) и `w:text` внутри `sdtPr` (9) остаются unclassified: у них `named=0`, и TZ-32 требует citation. Полный корпус: unclassified 784, `unmatched_schema` 1198, exit 1. D05 не PASS.
