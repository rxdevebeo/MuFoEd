# Независимая приёмка возврата D01–D09

Дата: 6 октября 2026 года. Проверяется незакоммиченное дерево поверх `63be263497fa293d27734c0868c68c8540ff7f9f`. SHA HEAD не идентифицирует эти исправления. SHA-256 фактических байтов tracked-файлов: `70befa261310e3f790e00444c9ab1167f42b4fed7bb962627ca8fab5a6c16a1e`; полный перечень — `evidence/source-snapshot.json` относительно каталога `audit-rework-review-2026-10-06/`. Этот fingerprint не является реализацией D07: untracked inputs/receipts требуют отдельного manifest.

**Итог: полный аудит не принят, `full_audit=false`. D02 принят в пределах измеренных свидетелей. D01 вновь открыт; D03 принят частично и возвращён из-за воспроизводимого расхождения layout/PDF. D04 частично принят. D05–D09 открыты. WORD-COMPAT = NOT_RUN.** Исторические карточки R и приёмка 5 октября не переписаны.

## Статусы

| Карточка | Независимое решение | Основание |
|---|---|---|
| D01 | REOPENED | Текущие fmt и строгий clippy падают; MSRV 1.92 check проходит. Receipt предыдущего состояния не закрывает последующие правки. |
| D02 | ACCEPTED_MEASURED_WITNESSES | Release hostile footer 1,06 с; CC0 017/035 первая страница 1,04/0,58 с, оба render exit 0. Полный повтор времени остальных 98 документов не заявляется. |
| D03 | PARTIAL / REWORK | Unicode-проба проходит; штатные SVG/PDF pixel gates восстановлены. Новый независимый контроль `To` выявляет расхождение ширин 3,792 px при `both`, контроль `left` совпадает. |
| D04 | PARTIAL | Независимая angle-проба проходит, неизвестная защита даёт degraded с конкретными потерями. XSD/OPC 27 Strict + 121 local + 100 CC0 после последних правок не повторены. |
| D05 | PARTIAL | Удаление wildcard подтверждено исходниками и receipt; свежий census selftest PASS. Inventory действительно содержит 221 документ, 1568 групп, 16225 строк без disposition. |
| D06 | OPEN | Известные WPS отклонения остаются; критерий 0,25 px сохранён. Word отсутствует. |
| D07 | OPEN | По возврату исполнителя 14 обязательных строк F21 BLOCKED; content fingerprint и агрегатор закрытия ещё требуют завершения. |
| D08 | OPEN | По возврату исполнителя WML branch 657/1030 = 63,79% < 70%; fuzz 8×3600 с и exact-SHA CI не выполнены. Здесь эти измерения заново не воспроизводились. |
| D09 | FAIL / OPEN | Свежий workspace: 1433 passed, 1 failed, 0 ignored, exit 101; падает `stage5c_corpus`. Fmt/clippy также FAIL. |

## Новые доказанные препятствия

### D01: повторная регрессия gates

`cargo +1.92.0 fmt --all -- --check` exit 1: `strict-ooxml-write/src/passthrough.rs:755`, форматирование `bytes.windows(...).any(...)`.

`cargo +1.92.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` exit 101: `strict-ooxml-wml/src/parse/fonts.rs:117`, `clippy::single_match_else`. Исправить текущие места и повторить gates после завершения следующих производственных изменений.

`cargo +1.92.0 check --workspace --all-features --locked` exit 0 соответствует объявленному MSRV. Пробный check на 1.85 отклонён декларациями rust-version; это ошибочно выбранная версия проверки, а не дефект проекта и не свидетельство MSRV FAIL.

### D03: layout/SVG и PDF расходятся для justified simple-script

Сохранённый воспроизводитель: `audit-rework-review-2026-10-06/evidence/acceptance_return_probe.rs`. Строгий минимальный DOCX, Carlito 32 pt, текст `To`, меняется только `w:jc`:

| Выравнивание | TextItem.width | Shaped advance, используемый PDF | Разница |
|---|---:|---:|---:|
| left | 39,500 px | 39,500 px | 0 |
| both | 43,292 px | 39,500 px | 3,792 px |

Проба компилируется и падает behavioral assertion, exit 101. Это сравнение публичной placement-модели и публичного shaper, а не измерение экспортированного PDF raster. Связь с PDF проверена по исходникам: `strict-ooxml-render-pdf/src/document.rs:637`/`:683` рисует `glyph.x_em` из `shape_bundled`. Layout для Both/Justify/Distribute переключается на суммы hmtx (`layout/paragraph.rs:130`, `layout/mod.rs:551`); SVG выбирает metric-позиции по сравнению ширин (`paint/text.rs:79`). Политика не передана явно в TextItem и PDF. В том числе последняя строка такого абзаца получает отличающуюся ширину.

Нужно согласовать measurement, glyph origins, cluster mapping, caret и оба backend. Прохождение WPS text correlation не доказывает этот контракт. Не исправлять посредством порога/нового golden. Старый Unicode-дефект действительно исправлен: `office`, combining acute, Arabic и Tamil имеют корректные логические интервалы и число scalar positions; эта проба не доказывает корректное изображение Arabic/Tamil при отсутствующих glyphs.

### D05/D09: support report конфликтует с corpus contract

В свежем workspace падает `strict-ooxml/tests/stage5c_corpus.rs:86`, `no_repro_fixture_reports_an_unsupported_mechanism`. На `05-strict-math-simple` новый parser сообщает Unsupported для `w:charset`, `w:family`, `w:panose1`, `w:pitch`, `w:sig`. Причина видна в `strict-ooxml-wml/src/parse/fonts.rs:136`: все не-embed дочерние элементы font записываются как Unsupported и пропускаются.

Не подавлять эти отчёты и не удалять проверку для получения PASS. Решить судьбу подсказок: сохранить их в модели/roundtrip либо явно обосновать известную потерю и её влияние на прежний Stage-5C контракт. Если контракт требует отсутствия critical problems, одна классификация census как reported-loss его не закрывает. Добавить positive/negative control сохранения/потери подсказок и снова пройти corpus test.

## Подтверждённые улучшения

- D02: fresh release CLI; CC0 017 write 2,38 с, 035 write 0,69 с, оба write exit 1 (degraded, выход создан), render exit 0. Первоначальная попытка с несуществующим `--out-dir` была ошибкой команды; численные результаты от неё не считаются измерением renderer. Успешный запуск использует `--out`.
- D04: hue/hueOff остаются целыми `60000`, без `60%`. Unknown SID 9999 даёт outcome degraded и issues для `cryptAlgorithmSid` и `cryptProviderType`; парольная совместимость Word не проверена.
- D05: агрегирование сохранённого inventory подтверждает off@y 640, blip@embed 551, off@x 517, p@rsidP 505, hyperlink@id 478. Это проверка артефакта исполнителя, а не свежий census всех документов.

## Приёмка без Word

Применяется [отдельный протокол](WORD_INDEPENDENT_ACCEPTANCE_2026-10-05.md): structural/roundtrip/XSD/OPC, внутренние SVG/PDF controls и WPS geometry принимаются независимо. Известные WPS дефекты D06 ремонтируются сейчас: абсолютный SNP сдвиг 0,73–0,87 px, ритм клад около 2,5 px, подпись вне страницы, страницы 54 и 104 с отклонениями 3–5 px. Допуск 0,25 px сохраняется.

WORD-COMPAT остаётся NOT_RUN до пригодного Word export с input hash, версией Word/шрифтов, настройками страницы и scale. Ни WPS, ни LibreOffice, ни внутреннее совпадение SVG/PDF не объявляются Word PASS. После устранения остальных дефектов можно зафиксировать принятую часть и планировать от неё, сохранив Word-only проверки отдельным долгом. Изменение полного контракта приёмки требует решения владельца; в этой проверке оно не выполняется.

## Ограничения проверки

Исходники проверены непосредственно; граф Tier 2 имел metadata_changed, поэтому его чистое coverage не использовалось как доказательство актуальности. Это направленная приёмка возврата, не новый исчерпывающий security audit. Не запускались Linux fuzz, coverage, внешняя CI, повторные 248 XSD/OPC и полный visual corpus. Коммитов/push не было. Производственные файлы не исправлялись; временные acceptance tests удалены после сохранения в evidence.
