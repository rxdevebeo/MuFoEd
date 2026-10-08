# P9 — шрифты и hints

```powershell
cargo +1.92.0 test -p strict-ooxml-wml --test styles_settings --locked --offline -- parses_all_settings_elements
cargo +1.92.0 test -p strict-ooxml-wml --lib --locked --offline -- theme_percentages
cargo +1.92.0 test -p strict-ooxml-write --test fonts --test p9_fonts --test loss_gate --locked --offline
cargo +1.92.0 test -p strict-ooxml-write --lib --locked --offline -- rfonts_
cargo +1.92.0 test -p strict-ooxml --test stage5c_corpus --locked --offline
python xtool/xsd-gate/census_gate_selftest.py
cargo +1.92.0 build -p strict-ooxml-cli --release --locked --offline
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build --quiet-messages `
  --keep-written target/census-written-p9-final `
  --inventory-out target/census-inventory-p9-final.json
```

| ID | Результат |
|---|---|
| T-P9-1 fontTable panose/charset/family/pitch/sig сохраняются; повторный `w:rFonts` накладывается, а не затирает ascii/hAnsi | PASS |
| T-P9-2 `stage5c_corpus` 4/4, assertions не удалялись | PASS |
| T-P9-3 очищенный charset Calibri и charset темы `86` отсутствуют в записанном XML | PASS |
| `hint=cs` не пишется, `w:cs` остаётся, отчёт цитирует `w:rFonts@hint` | PASS |
| `hint=eastAsia` и `hint=default` пишутся | PASS |
| `themeFontLang` eastAsia/bidi сохраняются; bidi не выдумывается | PASS |
| `a:objectDefaults` копируется вместе с `a:lnDef` и `a:sym`; `spcPct`/`miter` становятся `100%`/`400%` | PASS |
| census selftest: TZ-49 принимает только цитированный `hint=cs`; charset без waiver остаётся unclassified; overlay не прячет сброшенный ascii | PASS |

Отрицательный контроль: тихий сброс charset виден. `hint=eastAsia` не попадает под TZ-49. Сброс `hint=cs` без цитаты остаётся unclassified. Сброс `w:ascii` после overlay остаётся строкой census.

Полный корпус, primary labels: **235 → 0**. `unmatched_schema` остаётся 1198. Unclassified 4493 → 4153. D05 не PASS.

Страница Stage-5C: `visual/05-strict-math-simple-page1.svg`.
