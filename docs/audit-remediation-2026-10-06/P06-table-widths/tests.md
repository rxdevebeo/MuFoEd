# P6 — ширины таблиц

```powershell
cargo +1.92.0 test -p strict-ooxml-write --locked --offline --test p5_p6
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --offline --test p5_p6
cargo +1.92.0 test -p strict-ooxml-render-pdf --locked --offline --test p6_table_page
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| T-P6-1 gridCol/tcW: dxa пишется в пунктах и читается обратно в твипы | PASS |
| T-P6-1 `w:tblGridChange` сохраняет прежнюю сетку | PASS |
| T-P6-2 синтетика: ячейки 3000 и 1500 twips → 200 px и 100 px | PASS |
| T-P6-2 RM0090: колонки 1917 и 2579 twips, заголовок по центру, HCLK на 108 twips внутри второй колонки | PASS |
| T-P6-2 PDF одной страницы таблицы: `visual/rm0090-table-page.pdf` | PASS |
| T-P6-3 колонка на 1 twip уже — другая ширина в модели и в XML | PASS |
| `w:tblPrEx` граница sz 4 / space 0 переживает следующий `w:trPr` | PASS |
| ширина границы: абзац sz 12 и таблица sz 8; отрицательный EighthsPoint 11 | PASS |

Один твип при 96 dpi — это 1/15 px, меньше допуска 0.25 px. Поэтому T-P6-3 проверяется компаратором переписи и моделью, а не промахом пикселя. `3125` и `3124` не совпадают; `5000` fiftieths и `100%` совпадают только при `w:type="pct"`. Допуск 0.25 px не поднимался.

Страница свидетеля: `visual/rm0090-page2.svg`. PDF одной страницы: `visual/rm0090-table-page.pdf`.

Полный корпус, 221 документ: `w:tcW@w`, `w:tblW@w`, `w:gridCol@w` уже были 0; ширины границ **12 → 0**.

Повтор 2026-10-07: SVG T-P6-2 на `limited_pages(2)` ok; PDF `p6_table_page` ok (~23 s). **ACCEPT**.
