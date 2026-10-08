# P12 — колонтитулы и id сносок

```powershell
cargo +1.92.0 test -p strict-ooxml-write --lib footnote_separator --locked --offline
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| T-P12-1 семантический digest колонтитула: тот же текст не даёт другой part; изменённый абзац даёт | PASS |
| T-P12-2 separator id −1 записывается; `w:footnote@id` и `w:endnote@id` на корпусе 0 | PASS |
| T-P12-3 чужой id (`was=7`) не маскируется реестром | PASS |
| Текст `word/header1.xml` и `word/header10.xml` документа 003 совпадает | PASS |

`w:headerReference@id` 138 → 78, `w:footerReference@id` 107 → 61. Это части, у которых содержимое всё ещё различается. Пакет не закрыт. Полный корпус: unclassified 784, `unmatched_schema` 1198, exit 1. D05 не PASS.
