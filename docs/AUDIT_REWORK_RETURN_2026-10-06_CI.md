# Приёмка последних изменений и уточнённый заказ после зелёного CI

6 октября 2026. HEAD `9fec5cbe45d32f484e4d71e891d796920df1a585`. Tracked tree чистое; локальные корпусные и fuzz-артефакты untracked сохранены. Это обновление к [приёмке до исправлений](AUDIT_REWORK_REVIEW_2026-10-06.md) и [заказу](AUDIT_REWORK_RETURN_2026-10-06.md); исторические выводы не переписаны.

## Что теперь подтверждено

[CI 37454090543](https://github.com/rxdevebeo/MuFoEd/actions/runs/37454090543) действительно SUCCESS на точном текущем HEAD. Успешны все восемь выполнявшихся jobs: test Ubuntu/Windows/macOS, MSRV 1.92, cargo-deny, coverage, XSD, fuzz smoke. `fuzz nightly` SKIPPED: этот job выполняется только по schedule. Покрытие строк в coverage job не означает прохождения branch ≥70%.

- **D01 принят:** fmt/clippy/MSRV проходят в текущем CI.
- **Предыдущий Stage-5C дефект закрыт:** known font hints сохраняются в модели и writer; порядок `CT_Font` исправлен. Независимый локальный запуск `stage5c_corpus`, `fonts`, `d03_geometry_probe`: 15/15 PASS, exit 0.
- **Прежняя эвристика D03 устранена:** `TextItem.advance` явно выбирает Metric/Shaped; SVG, PDF и caret читают её. Однако D03 остаётся открытым из-за нового дефекта ниже.
- **D04, Strict schema/OPC часть подтверждена текущим CI:** XSD дважды documents=27, validated=26, refused=1, missing=0, unmatched=0, ours=0, source=11. Это не schema-clean всех выходов. OPC gate и его negative control зелёные. Полная актуальная приёмка локального/CC0 корпуса отдельно.
- **D07 продвинулся:** в matrix осталась одна blocked строка `F16-complex-word-schemes`; F21 остаётся synthetic public suite. Полный audit не принят.
- Замена rustybuzz на harfrust покрывается текущими межплатформенными тестами. XSD oracle сокращает компиляцию только в settings negative-control; обычный corpus gate создаёт полный Oracle. F01 oracle cases вынесены в отдельный CI шаг `--full`, они не исчезли из проверки.

## P1: новый доказанный дефект D03 — Metric PDF теряет буквы

Места: `strict-ooxml-render-pdf/src/document.rs:303` (`collect_fonts`) и `:646` (`show_metric`); `strict-ooxml-render-pdf/src/font.rs:120` (`add_cluster`).

Сборщик шрифта всегда включает glyphs shaped-результата. Для Carlito `office` шейпер объединяет `ffi` в ligature. В `add_cluster` многосимвольный Unicode сохраняется в cluster mapping, отдельные `f`/`i` в `font.chars` не появляются. Затем Metric paint рисует по scalar через `font.chars` и пропускает их.

Независимая проба публичного PDF API подаёт одну placement-страницу с `office`, Carlito, отдельно Shaped и Metric:

| Политика | PdfReport |
|---|---|
| Shaped | losses=[] |
| Metric | pdf.font.no_glyph: f и i не нарисованы |

Тест компилируется и падает behavioral assertion именно на Metric, exit 101. Glyphs в bundled face существуют: ошибка в подготовке subset/CID mapping, не в отсутствии шрифта. Проба не использует extraction как oracle: lopdf извлёк пустую строку в обоих случаях, поэтому этот результат не объявляется отдельным дефектом текста.

Воспроизводитель и лог сохранены в `audit-rework-review-2026-10-06-ci/evidence/`. Чтобы повторить, скопировать `acceptance_metric_probe.rs` в `strict-ooxml-render-pdf/tests/` и выполнить `cargo +1.92.0 test -p strict-ooxml-render-pdf --test acceptance_metric_probe --locked -- --nocapture`. Временный тест из рабочего crate удалён после сохранения. Production-файлы при проверке не менялись.

## Заказ на оставшиеся доработки

1. **D03, P1: исправить font collection под выбранную политику.** Владение: render-pdf document/font и tests. Для Metric включать scalar glyphs/CIDs и scalar ToUnicode, для Shaped сохранять shaped glyphs/clusters. Приёмка: сохранённый RED → GREEN; `office`, `ffi`, combining marks, mixed Metric/Shaped на одном face; проверить реальный glyph output и Unicode mapping независимым PDF oracle. Старые d03_geometry, SVG/PDF WPS и pixel gates остаются зелёными. Не подавлять missing-glyph report.
2. **D05: закончить census.** Последний локальный `census.log`: documents=221, validated=221, missing=0, unmatched_schema=0, ours=0, но **11711 unclassified changes, FAIL**. Это прогресс относительно 16225; не PASS. Артефакт относится к предшествующему промежуточному дереву, повторить на финальном SHA. Не добавлять blanket dispositions.
3. **D06: измерить реальную WPS геометрию после frame/table исправлений.** Synthetic frame tests и зелёные text-class gates не доказывают устранение всех старых SNP/клад/Clio отклонений. Нужен новый ledger пригодных references, страницы 54/56/104, подписи внутри страницы и каждый компонент ≤0,25 px. Word-only строка остаётся BLOCKED/NOT_RUN.
4. **D08: закрыть branch/fuzz остатки.** STATUS.md сообщает WML 678/1042 = 65,07% <70%; это историческое промежуточное измерение, проверить итоговый SHA. Локальные `fuzz_pdf.log` и `fuzz_convert.log` заканчиваются exit 1 с ASan leak artifacts; triage/reproduce/minimize, установить production/dependency/harness причину. Обеспечить восемь отдельных receipts по 3600 с с SHA/command/exit/input artifacts. Зелёный 60-секундный smoke их не закрывает.
5. **D04/D07/D09: финальный corpus и согласованные receipts.** Завершить актуальные XSD/OPC/census для 27 Strict +121 local +100 CC0, входы/refusals/source violations считать отдельно. Content hashes должны идентифицировать входы и код. STATUS.json всё ещё указывает базовый 63be263, CI NOT_RUN и commits=none, тогда как текущий HEAD опубликован и CI SUCCESS; обновить текущий итоговый receipt, не исторические карточки. После production fix повторить обязательные gates и связать с точным финальным SHA.

## Текущая база для планирования

Зелёный межплатформенный CI принят. D01 и font-hints regression закрыты; D02 performance свидетели сохраняют прежнюю ограниченную приёмку. D03 открыт на новом PDF дефекте; D04 частичен по корпусам; D05/D06/D08/D09 открыты; D07 имеет одну Word-only blocked строку и требует актуальных receipts. **full_audit=false, WORD-COMPAT=NOT_RUN**. Снятие Word-зависимого требования без пригодного экспорта не выполнялось. Коммитов и push в этой проверке не было.
