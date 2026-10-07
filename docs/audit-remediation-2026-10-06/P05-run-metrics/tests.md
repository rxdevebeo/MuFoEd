# P5 — метрики абзаца и run

```powershell
cargo +1.92.0 test -p strict-ooxml-write --locked --offline --test p5_p6
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --offline --test p5_p6
cargo +1.92.0 test -p strict-ooxml-render-pdf --locked --offline --test p6_table_page
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| T-P5-1 sz/szCs half-point и `w:w` → `N%` | PASS |
| T-P5-1 `w:sdtPr/w:rPr` sz 28 | PASS |
| T-P5-1 `w:sdtEndPr/w:rPr` sz 20, отрицательный sz 19 | PASS |
| T-P5-2 `after="12pt"` пишется как `w:after="240"` | PASS |
| T-P5-3 tab `pos` сохраняется, `left` пишется как `start` | PASS |
| T-P5-4 синтетика: sz 24 → 16 px; 240 twips after → +16 px; `w:w` 50% → ширина ×0.5 | PASS |
| T-P5-4 RM0090: заголовок LCD-TFT, sz 48 → 32 px, ширина 143.953125 px, зазор до `16.1` 85.212890625 px | PASS |
| `w:ind@left` 720 → `w:start` 720; отрицательный 719 | PASS |
| `w:leftChars` / `w:rightChars`, `beforeLines` / `afterLines`, autospacing `false`, run spacing 0 | PASS |
| `m:sSubPr/m:ctrlPr/w:rPr/w:sz` 22 остаётся под `m:sSubPr`; отрицательный 21 | PASS |

Отрицательный контроль half-point: sz 24 и sz 23 дают разный `w:sz`. Отступ 720 и 719 — разные `w:start`. Допуск краски 0.25 px не поднимался.

Страница свидетеля: `visual/rm0090-page1.svg`.

Полный корпус, 221 документ, 0 отказов: метки пакета **197 → 15**. Оставшиеся 15 — `w:sz` / `w:szCs` внутри `v:group` в `070_Innovations_and_New_Technologies.docx`. Диспозиция реестра для них не добавлялась. `w:spacing`, `w:ind`, `w:tab` и `w:w@val` на корпусе равны 0, включая уроки SoftUni 1–8.

Повтор 2026-10-07 (после early-stop `PageSelection::Range` в пагинаторе): write `p5_p6` 13 ok; SVG `p5_p6` `--test-threads=1` 5 ok. **ACCEPT** (остаток 15 VML sz вне paint-scope).
