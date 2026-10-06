# Заказ на доработки по возврату D01–D09

Основание: [независимая приёмка 6 октября](AUDIT_REWORK_REVIEW_2026-10-06.md). Продолжает [заказ 5 октября](AUDIT_REWORK_RETURN_2026-10-05.md); исторические R-карточки и receipts не переписывать. Текущий HEAD `63be263497fa293d27734c0868c68c8540ff7f9f`, исправления находятся в dirty tree. Полный аудит открыт.

## Первая очередь: новые доказанные blockers

1. **D03, P1 — единая геометрия simple-script при both/justify/distribute.** Владение: render-svg layout/paint/font, render-pdf document/font и соответствующие editor/convert consumers. Воспроизвести сохранённую `acceptance_return_probe.rs`: Carlito 32 pt `To`, both даёт 43,292 вместо 39,500 px. Устранить рассогласование measurement/SVG/PDF/caret, передавать выбранную геометрию явно, если нужны разные политики. Приёмка: прежний RED становится GREEN; left-control остаётся GREEN; last-line, line wrapping, pair kerning, ligatures, combining, RTL и PDF glyph origin/ToUnicode controls; существующие WPS/SSIM/extent проверки без повышения допусков. Unicode fix сохранить.
2. **D05/D09, P1 — согласовать font hints и Stage-5C контракт.** Владение: WML font parser/model, writer, support report, census, stage5c tests. `05-strict-math-simple` теперь имеет critical Unsupported по пяти hint-элементам. Предпочтительно обеспечить их сохранение; иначе обосновать конкретную потерю и допустимость изменения исходного контракта, не считать такое изменение автоматической приёмкой. Приёмка: воспроизводимый corpus RED устранён, unknown hints не теряются молча, named hints/resources/attributes учтены в census; negative control ловит действительную потерю. Удаление assertions и blanket dispositions не допускаются.
3. **D01, P1 — повторно закрыть fmt/clippy после интеграции.** Владение: writer passthrough и WML fonts. Текущие причины: fmt `passthrough.rs:755`; clippy `single_match_else` `fonts.rs:117`. Приёмка: fmt, строгий all-targets/all-features clippy и MSRV 1.92 check exit 0 на итоговых content hashes.

## Оставшаяся очередь

| Карточка | Работа | Измеримый критерий |
|---|---|---|
| D02 | Сохранить принятый ремонт, повторить свидетели после правок D03 | Heavy footer debug/release, 1 МиБ и прежний 10 с; 017/035 не возвращаются к timeout. Для полного corpus-профиля выполнить все 100 CC0. |
| D04 | Повторить XSD/OPC на 27 Strict, 121 local, 100 CC0 после всех writer/normalizer изменений | Все inputs учтены по hash; собственных schema/OPC нарушений 0; source violations и intentional refusals отдельно. Unknown protection остаётся degraded. |
| D05 | Закрыть 1568 групп / 16225 строк, сначала off/blip/rsidP/hyperlink | Каждой группе дана точная disposition с QName/path/count и behavioral witness либо report доказанной потери; unknown controls остаются unclassified; wildcard обхода нет; corpus census gate PASS. |
| D06 | Исправить перечисленные WPS geometry дефекты | Каждый компонент ≤0,25 px на пригодных WPS references, подписи внутри страницы, controls и обратные проверки проходят. WORD-COMPAT NOT_RUN сохраняется. |
| D07 | Завершить 14 F21 blocked rows: F03/F04/F07–F12/F14–F19; inverse и агрегатор | Hash меняется при изменении bytes без изменения git status. Матрица содержит commands/exits/artifacts/denominators и честные blockers. full_audit false при любом открытом обязательном gate. |
| D08 | WML branch coverage, Linux fuzz и внешняя CI | ≥70% WML branches на воспроизводимой Ubuntu job; 8 целей ×3600 с с receipts; CI на точном code SHA после отдельного разрешения на commit/push. До него CI NOT_RUN. |
| D09 | Последний последовательный acceptance run после всех production правок | Workspace all-features locked; default/features matrix; fmt/clippy/MSRV; hostile debug/release; корпуса/census/XSD/OPC; SVG/PDF и запрет nonfinite markup. Полные commands/exits/counts/input/content hashes. |

Порядок: D03 и font-hints решение → завершение D04/D05 → D06/D07 → D01 и D08 на стабильной базе → D09. Частичные измерения не подменяют corpus gate. Fuzz требует реального времени, его не сокращать. Коммиты/push только по отдельному разрешению владельца.

## Что вернуть

Новый каталог receipts с датой возврата: code SHA и content manifest (включая необходимые untracked inputs), версии инструментов, точные команды и exit, behavioral RED/GREEN/inverse, numerator/denominator, hashes артефактов и явные NOT_RUN/BLOCKED. По карточкам различать production fix, доказательство локального свидетеля и полное закрытие. Отдельно перечислить принятые без Word профили и оставшиеся Word-only пункты.

Отсутствие Word не блокирует исправление доказанных текущих дефектов и независимые gates. Полный Word PASS не объявлять.
