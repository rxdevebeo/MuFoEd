# P8 — явные цвета `w:color@val` / `a:schemeClr`

```powershell
cargo +1.92.0 test -p strict-ooxml-write --locked --offline --test p7_p8
cargo +1.92.0 test -p strict-ooxml-render-svg --locked --offline --test p7_p8
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build `
  --quiet-messages --keep-written target/census-written-p7p8-final `
  --inventory-out target/census-inventory-p7p8-final.json
```

| ID | Результат |
|---|---|
| T-P8-1 hex `AbCdEf` roundtrip; смена на `AbCdEe` видна в XML | PASS |
| T-P8-2 `auto` и `000000` сохраняют лексику | PASS |
| T-P8-3 `a:schemeClr` на lnRef/fillRef/effectRef/fontRef не становится `phClr` | PASS |
| T-P8-3 paint: `bg1` → `#ffffff` (lt1) | PASS |
| `fontRef idx="minor"` сохраняется как текст | PASS |

Отрицательный контроль: реальное изменение hex не маскируется case-fold. Stored schemeClr vals не заменяются на `phClr`.

Страница свидетеля: `visual/018-page1.svg`. Color sample: `visual/color-sample.json`.

Полный корпус: `w:color@val` + `a:schemeClr@val` **297 → 0**. Residual 21 строк (`a:srgbClr` в `a14:hiddenFill` под `a:noFill`, `a:sysClr`, один `w:shd`) — вне primary labels плана; не закрыты disposition’ом.

Сдача 2026-10-07: корневой `STATUS.json` → `m3_status=ACCEPT` / `m3_slice_p7_p8=ACCEPT`.
