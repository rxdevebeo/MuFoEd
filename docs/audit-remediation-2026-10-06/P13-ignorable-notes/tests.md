# P13 — mc:Ignorable на колонтитулах и сносках

```powershell
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| T-P13-1 известный набор префиксов Office на hdr/ftr/footnotes/endnotes/glossary совпадает с TZ-50 | PASS |
| T-P13-2 произвольный префикс и строка без citation не совпадают | PASS |

TZ-46 не расширялся: `was=` относится ко всему item, и общий список сломал бы `pic:cNvPr@id`. TZ-50 — отдельный item с точными строками префиксов.

Полный корпус: пять меток Ignorable 388 → 0. `unmatched_schema` 1198. unclassified 784. Exit 1. D05 не PASS.
