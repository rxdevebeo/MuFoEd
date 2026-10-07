# P10 — бинарные ресурсы

```powershell
cargo +1.92.0 test -p strict-ooxml-write --test p10_media --locked --offline
cargo +1.92.0 test -p strict-ooxml-write --lib --locked --offline -- locked_canvas_markup_is_emitted_with_placement
python xtool/xsd-gate/census_gate_selftest.py
cargo +1.92.0 build -p strict-ooxml-cli --release --locked --offline
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build --quiet-messages `
  --keep-written target/census-written-p10-final `
  --inventory-out target/census-inventory-p10-final.json
```

| ID | Результат |
|---|---|
| T-P10-1 SHA картинки совпадает с исходным файлом: заливка `a:blipFill` в First-Steps, JPEG locked canvas в 014, пустой `font6.odttf` в 068 | PASS |
| T-P10-2 переименование и вторая копия тех же байт не дают строку `resource:` | PASS |
| T-P10-3 другие байты (recompress) и пропавший уникальный digest остаются строкой `resource:` | PASS |
| запись locked canvas не подменяет картинку на header: `rId8` остаётся header, JPEG висит на своём `r:embed` | PASS |
| census selftest | PASS |

Отрицательный контроль: изменённые байты PNG и отсутствующий GIF остаются unclassified. Каталог zip (`word/`) не считается ресурсом. Повтор одинакового SHA не считается потерей.

Срез 14 свидетелей: resource 64 → 0, unclassified 739, exit 1. Полный корпус 221: resource labels **84 → 0**, `unmatched_schema` 1198, unclassified 4153 → 4032, exit 1. D05 не PASS.

`census.toml` не менялся. Пиксельное сравнение не нужно: байты PNG не пережимались. Страница с заливкой-картинкой: `visual/first-steps-page1.svg`.
