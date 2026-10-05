# RTF: первый приоритет после baseline (исторический snapshot)

Актуальные расширения и проверки: [ACCEPTANCE.md](ACCEPTANCE.md).

Все перечисленные ниже NUL/text failures теперь разобраны: [COMPATIBILITY_RESOLUTION.md](COMPATIBILITY_RESOLUTION.md).

2026-10-05. Baseline commit: `6b6d6d2`, ветка `codex/rtf-import`.
Работа продолжается в отдельном worktree. Исправления включены в коммит завершения данного этапа.

## Реализовано

- `sect` завершает абзац, включая пустой после `par`, и сохраняет section properties на границе. `sectd` сбрасывает свойства; `sbknone/page/col/even/odd` сохраняют тип секции. Разрыв внутри незавершённой строки таблицы отклоняется. Секции учитываются в node budget.
- `ansicpg0` больше не блокирует ASCII/Unicode-only документы. Высокие байты по-прежнему отклоняются: системная кодировка не угадывается.
- Новый opt-in `UnicodeParameterPolicy::AllowUnsignedAndReport` принимает положительные UTF-16 units до 65535 с Info для каждого исправленного параметра. Default остаётся StrictSigned; значения вне 16-bit range отклоняются в обоих режимах.
- `fcharset2` сохраняет font-encoded byte как `w:sym` с точным именем шрифта и кодом глифа. Это не перевод Wingdings в обычный Unicode. Проверены font/glyph roundtrip и scalar budget.

## Независимые проверки

`xtool/rtf/priority_probes.py` создал 17 минимальных входов и независимо открыл их в WPS. Все 17 captured; source/reference hashes закреплены в [priority.wps-probes.json](priority.wps-probes.json). Raw artifacts: `target/rtf-import/priority/wps-probes-v1/`.

WPS подтверждает отдельный абзац на `sect`, ASCII/Unicode с `ansicpg0`, unsigned surrogate pairs и Wingdings FC как PUA F0FC в текстовом представлении с шрифтом Wingdings. Наш package сохраняет эквивалентный font glyph через `w:sym`; сравнение корпуса выражает glyph byte в том же PUA представлении. Это не проверка изображения глифа или наличия шрифта.

WPS превращает `\-` в обычный дефис, но сохраняет Unicode `u173` как U+00AD. Импортёр сохраняет `\-` как soft hyphen, поэтому файлы 002/017 остаются различными. Raw LF в простом тексте и после tab WPS игнорирует: причины расхождений 005/058/059/075 ещё не установлены полностью.

## Корпус из 100 файлов

| Режим | Imported | Ошибки | Exact WPS body | Text/media roundtrip |
| --- | --- | --- | --- | --- |
| Baseline | 95 | 5 | 83 | 95/95 |
| Новый default | 97 | 3 | 88 | 97/97 |
| Новый explicit unsigned compatibility | 99 | 1 | 90 | 99/99 |

Источники и результат закреплены в [default summary](import.priority-default-summary.json) и [compatibility summary](import.priority-compat-summary.json). WPS facts v2 не изменены; whitespace и paragraph boundaries не нормализованы. В обоих новых прогонах 22 media parts, 0 writer errors и 0 writer report records.

31 Rust behavioral tests + 1 doctest, clippy all targets `-D warnings`, 21 Python harness tests проходят. Новые 99 DOCX проходят Strict XSD и OPC gates без violations: [validation.priority-receipt.json](validation.priority-receipt.json). Baseline schema receipt относится к старым 95 outputs.

## Следующие узкие задачи

1. Файл 006: явные `\'00` внутри текста. WPS их удаляет; default importer отклоняет XML-invalid text. Нужна явно выбранная и диагностируемая политика, без скрытого удаления NUL.
2. Файл 073: horizontal merges и окончательные пустые paragraphs в ячейках. Текст сохранён, но структура paragraph sequence отличается. Не закрывать это простым исключением пустых абзацев.
3. Файлы 005/058/059/075: минимализировать producer-specific line behavior. Простой raw LF сам по себе расхождение не воспроизводит.
4. Файлы 061/100: проверить inline Apple/Cocoa controls отдельными fixtures; WPS corpus output включает их spelling в видимый текст. Не копировать такое поведение без отдельного доказательства и явного режима.

Оставшиеся mismatches: 002, 005, 017, 058, 059, 061, 073, 075, 100. Default errors: 006, 038, 041; compatibility error: 006. Это измерения разработки, а не полная совместимость или release acceptance.
