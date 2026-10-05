# RTF importer: состояние реализации

2026-10-05. Worktree: `C:/Users/gamer/.codex/worktrees/rtf-import/StrictLib`, ветка `codex/rtf-import`, база `7a8e6d37d4d96543db00a9c4f2be51f879397c1f`. Изменения основного checkout не переносились. Коммитов нет.

## Что работает

Создан workspace crate `strict-ooxml-rtf`. Публичный контракт: `import(&[u8], &RtfOptions) -> Result<RtfImported, RtfError>`, где результат содержит owned WML document, отдельные media payloads и import report. Runtime не использует WPS, сеть, файловую систему или OCR. Workspace manifest и lock изменены только для этого crate.

- Итеративный lexer: группы, escapes, hex, signed parameters, атомарный `binN`, header/trailer validation, bounded stack/tokens/input.
- Scoped run/paragraph state, `plain`/`pard`, основные toggles, size, vertical alignment, bidi, alignment, indentation, spacing; единичная section geometry.
- Unicode: signed 16-bit `uN`, scoped `ucN`, fallback, surrogate pairs, `upr/ud`; invalid encoding по умолчанию fatal, replacement opt-in с loss.
- `ansicpg` и используемый font charset; font/color tables, faces в run properties. Symbol/OEM и неявный system ACP не подменяются Windows-1252.
- Простые таблицы: строки/ячейки/empty cells, widths, повторные и переиспользуемые row definitions, vertical merge markers. Разные grids отмечаются loss; nested tables блокируются.
- Inline PNG/JPEG из hex/binary, отдельные media parts, goal sizes/scale, byte budgets, signature checks. Неизвестные image formats и crop controls отмечаются loss. Это не полная raster validation.
- Неизвестные controls сохраняют окружающий текст и получают named loss. Ignorable destinations пропускаются целиком. Issue cap не сбрасывает loss/info totals и имеет `details_truncated`.

## Измеренный результат

`cargo +1.92.0 test -p strict-ooxml-rtf`: 27 behavioral tests и 1 doctest проходят. `clippy --all-targets -- -D warnings` проходит. Начальный компилируемый stub дал 8 assertion failures; затем все 8 исходных контрактов стали GREEN. Логи: `target/rtf-import/R02-{red,green}.log`. Compile failure первого scaffold не считается RED evidence.

Корпусный пример `examples/corpus.rs` проверил ровно 100 manifest inputs. Последний измеренный прогон: `target/rtf-import/R06-corpus-v3/`; pinned source/result/reference hashes и список расхождений: [import.development-summary.json](import.development-summary.json).

| Проверка | Результат |
| --- | --- |
| Import | 95 Imported, 5 ImportError |
| Точное body text сравнение с WPS facts v2 | 83/100, без нормализации |
| Writer + Strict reader body text roundtrip | 95/95 импортированных |
| Roundtrip imported media bytes | 95/95 документов, 22 media parts |
| Writer errors / report records | 0 / 0 |
| Import loss events / capped detail lists | 1 323 565 / 8 файлов |
| Strict XSD для созданных packages | 95/95, 0 violations |
| OPC XSD для созданных packages | 95/95, 0 violations |

Большое число loss events включает повторные unsupported control tokens, а не количество уникальных дефектов. Нулевая потеря writer не снимает потерь importer. Пять ошибок: XML-incompatible text в 006, `ansicpg0` в 007, non-signed Unicode parameters в 038/041 и используемый неподдержанный font charset в 054. Это development observations, не автоматически утверждённые expected failures.

Оставшиеся 12 успешных inputs имеют body mismatch. Список сохранён с first mismatch и paragraph counts. Среди причин и кандидатов: section breaks, поля/области, soft hyphen и особенности трактовки raw line endings/opaque Cocoa data в WPS. Причины ещё требуют минимальных independently checked fixtures; исключения не добавлены в expectations.

## Исправление независимого oracle

DOCX facts v1 ошибочно учитывал `w:tab` внутри `w:pPr/w:tabs` как видимый текст. Добавлен независимый XML regression test; `pPr/rPr/sdtPr` исключены из text traversal. Facts v2 пересчитаны из прежних 100 raw DOCX с проверкой их исходных SHA-256. Capture не повторялся и DOCX не изменялись.

Актуальный индекс: [wps.reference-index-v2.json](wps.reference-index-v2.json). Facts: `target/rtf-import/R01/wps-facts-v2/`; raw DOCX: `target/rtf-import/R01/wps-corpus/`. Старый v1 индекс сохранён для истории; его text digests не использовать. Python harness: 21 tests GREEN после исправления. Старые числа 39/42/50 exact text matches получены по дефектным facts v1 и не являются текущей оценкой.

## Что остаётся открытым

R02 имеет работающие API и bounded parser, R03/R04/R05/R06 имеют частичную реализацию. Их полное acceptance не объявляется. Reviewed support-profile expectations R01 ещё не заморожены.

Не завершены: полноценные styles/lists, multiple sections, horizontal/validated vertical merges, headers/footers/notes/comments/textboxes, tracked revisions, полевые инструкции/ссылки, WMF/EMF/shapes/OLE. Эти конструкции имеют loss/rejection, а не обещание fidelity. FontTable metadata сохраняется лишь частично через run fonts; actual resolved fonts не проверены.

Strict XSD gate `xsd-gate 1.0.0` применён к 95 outputs с явными `--corpus` и `--written`, pointing at `R06-corpus-v3`: 2133 schema drivers compiled, 0 failures, 0 uncovered roots и 0 violations. ECMA archive SHA-256 закреплён в штатном gate; schema cache изолирован под worktree target. OPC parts, которые ECMA Part 1 gate отдельно исключает, проверены `opc-gate 1.0.0`: 95 packages, 0 violations. Oracle: lxml 6.0.1, libxml2 2.11.9, Python 3.13.5. Логи: `target/rtf-import/R06-xsd-corpus.log`, `R06-opc.log`. Первый `R06-xsd.log` выбрал штатный corpus из-за default `--corpus` и **не** используется как RTF evidence.

Locked tests/clippy и Python harness logs: `target/rtf-import/R06-{tests,clippy,harness-tests}.log`. Визуальные comparisons, performance/peak RSS, supervised per-file crash/timeout accounting и release acceptance не закрыты. Facade/CLI/viewer/CI/core pipeline integration в основной checkout не выполнялась.
