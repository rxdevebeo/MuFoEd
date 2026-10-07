# P7 — themeColor / themeTint / themeShade

```powershell
cargo +1.92.0 test -p strict-ooxml-write --locked --offline --test p7_p8
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --offline --test p7_p8
cargo +1.92.0 build -p strict-ooxml-cli --release --locked --offline
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build `
  --quiet-messages --keep-written target/census-written-p7p8-final `
  --inventory-out target/census-inventory-p7p8-final.json
```

| ID | Результат |
|---|---|
| T-P7-1 themeColor/tint/shade на run, tblBorders, tcBorders, themeFill | PASS |
| T-P7-1 paint: accent1 → `#4472c4` в SVG | PASS |
| T-P7-1 paint: `w:shd@themeFill=accent1` → `#4472c4` | PASS |
| T-P7-2 lexical drop themeColor при том же hex → XML без themeColor | PASS |
| T-P7-3 accent1→accent2 в XML и paint (`#ed7d31`) | PASS |
| `w:u` без `w:val` с `themeColor=accent2` | PASS |
| Contoso styles themeColor/themeFill counts source=written (1277/784) | PASS (`t_p7_contoso_styles_theme_token_counts_match_source`) |

Отрицательный контроль: accent1 и accent2 — разные слоты и разный paint. Lexical drop themeColor при том же hex не объявлен declared_transform. Допуск 0,25 px не поднимался.

Сдача 2026-10-07: корневой `STATUS.json` → `m3_status=ACCEPT` / `m3_slice_p7_p8=ACCEPT`.

Страница свидетеля: `visual/contoso-page1.svg`. Color sample: `visual/color-sample.json`.

Полный корпус, 221 документ: метки theme* **1027 → 0**.
