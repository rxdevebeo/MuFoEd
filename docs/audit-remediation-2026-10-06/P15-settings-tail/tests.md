# P15 — settings, styles, numbering и хвост

```powershell
cargo +1.92.0 test -p strict-ooxml-core --lib word2012_lvl_tentative_becomes_the_strict_attribute --locked --offline
cargo +1.92.0 test -p strict-ooxml-write --lib a_text_wrapping_break_keeps_its_type --locked --offline
cargo +1.92.0 test -p strict-ooxml-write --lib parts::tests --locked --offline
python xtool/xsd-gate/census_gate_selftest.py
```

| ID | Результат |
|---|---|
| `w15:tentative` на `w:lvl` становится `w:tentative`; собственное имя атрибута больше не считается «уже присутствующим» | PASS |
| Algorithmics: в `word/numbering.xml` одиннадцать `w:tentative="1"` | PASS |
| T-P15 `restartNumberingAfterBreak=0` только с citation `ext:restartNumberingAfterBreak`; `was=1` и молчаливый сброс не маскируются | PASS |
| Битовая маска `stylePaneFormatFilter` `1721` совпадает с записанными флагами; неполная маска и бит `0x0010` остаются строкой | PASS |
| Переписанное пространство имён custom properties — не потеря; удалённое property остаётся строкой | PASS |
| `w:br w:type="textWrapping"`, `w:evenAndOddHeaders w:val="false"`, `w:autoHyphenation w:val="true"`, пустой `w:docDefaults`, `tblStyleRow/ColBandSize` | PASS |

Срез трёх свидетелей (Algorithmics, curso, Digital Marketing): documents=3, unmatched_schema=0, unclassified=0, exit 0.

Цель пакета на полном корпусе — `unclassified_element_changes=0`. Измерено 784. Пакет не закрыт. `unmatched_schema` 1198. Полный прогон exit 1. D05 не PASS. M4 не закрыт.
