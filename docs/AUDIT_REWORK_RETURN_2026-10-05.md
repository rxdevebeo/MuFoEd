# Заказ на доработки после независимой приёмки R01–R11

Дата: 5 октября 2026 года. База: `63be263497fa293d27734c0868c68c8540ff7f9f`, master. Основание: [итоги приёмки](AUDIT_REWORK_REVIEW_2026-10-05.md). Этот файл продолжает первоначальный [заказ R01–R11](AUDIT_REWORK_ORDER_2026-10-05.md): незавершённые контракты сохраняются.

**Результат очереди:** восстановить зелёные обязательные regressions, устранить новые доказанные дефекты и завершить измеримую часть приёмки без доступного Word. Не объявлять полный Word PASS. Производственные изменения, сделанные в R01–R11, не откатывать целиком; работающие исправления FEFF/wrap/regions/UI сохранять.

## Протокол

- Перед работой записать code SHA, фактические source/input hashes и исходный git status. Hash списка статусов не заменяет hash bytes дерева.
- По каждой D-карточке: behavioral RED → production/oracle fix → тот же GREEN → компилируемый inverse. Ошибка сборки и отсутствующий инструмент не RED.
- Для изменения oracle обосновать корректность формата/метрики независимыми positive/negative controls. Не обновлять expected, thresholds или golden текущим выходом для получения PASS.
- Не повышать лимиты/таймауты вместо устранения регрессий. Не править corpus inputs/manifests для прохождения.
- Сохранять артефакты `target/audit-rework-return-2026-10-05/Dxx/`, компактные receipts — `docs/audit-rework-return-2026-10-05/`. Receipt включает code/content/input hashes, версии инструментов, commands/exits, numerator/denominator и ограничения.
- Shared parser/writer/layout исправления интегрировать последовательно. Общую приёмку повторять после последней производственной правки. Коммиты/push выполнять только при отдельном разрешении владельца.

## Очередь

| Карточка | Приоритет | Зависимости | Результат |
|---|---|---|---|
| D01 | P1 | — | Fmt и полный строгий clippy проходят |
| D02 | P1 | — | Восстановлены performance и hostile budgets |
| D03 | P1 | D02 | Shaping согласован с SVG/PDF/editor/convert; regressions восстановлены |
| D04 | P1 | — | Углы и неподдерживаемая защита не портятся молча |
| D05 | P1 | D04 | Census подтверждает dispositions без wildcard bypass |
| D06 | P1 | D03 | F16 компоненты исправлены против пригодного WPS; Word статус отделён |
| D07 | P1 | D03–D06 | Матрицы, inverse и content fingerprints достоверны |
| D08 | P1 | стабильный код D01–D07 | Coverage/CI/fuzz соответствуют сохранённым контрактам |
| D09 | P1 | D01–D08 | Свежая итоговая приёмка всех corpus/regression gates |

P1 блокирует полное закрытие аудитов. Карточки можно готовить независимо по владению; конфликтующие shared файлы менять последовательно. Отсутствие Word блокирует только WORD-COMPAT, а не D01–D05/D07–D09 и не исправление известных WPS ошибок D06.

### D01. Сборочные гейты

Владение: format/lint изменения затронутых Rust-файлов, без изменения поведения.

Свежие [fmt](audit-rework-review-2026-10-05/evidence/fmt.log) и [clippy](audit-rework-review-2026-10-05/evidence/clippy.log) красные. Исправить DrawingML doc markdown, matches-like expression, rewrite_start complexity, redundant closure, match_same_arms; затем проверить остальные крейты, до которых этот прогон не дошёл. Не добавлять широкое allow и не использовать `--no-deps` как замену общего gate.

Приёмка: `cargo +1.92.0 fmt --all -- --check`, `cargo +1.92.0 clippy --workspace --all-targets --all-features --locked -- -D warnings`, MSRV check проходят на итоговом дереве. Появившиеся новые диагностики также устранены.

### D02. Резкая деградация рендера

Владение: font face/shaper caches, LayoutContext measurement и page-region resource accounting; hostile/corpus performance witnesses.

Доказательства: heavy footer неоднократно не укладывается в 10 с в debug/release, включая отдельный запуск. CC0 017 дважды timeout 90 с без SVG; 035 отдельно 82,15 с. Исторические значения первой страницы 0,885/0,539 с. `resolve_face` заново SHA-256-хеширует полный font program при каждом `measure`; проверить profiler-ом как кандидат причины, вместе с repeated shaping/field/layout loops.

Приёмка: прежний hostile budget 10 с и стек 1 МиБ сохранены; ожидаемый resource-limit error достигается своевременно. Оба CC0 документа завершают прежнюю операцию в установленном бюджете 90 с; зафиксировать controlled benchmark с одинаковой машиной, flags, inputs и эталонной базой, warm/cold отдельно. Cache keyed by face/style/variation/content не меняет glyph/resource identity. Не считать отсутствие panic успешным выполнением.

### D03. Интеграция shaping и все 14 regressions

Владение: shaped run representation, SVG/PDF paint, renderer/caret geometry, convert text recovery и соответствующие oracles/tests. Список 14 failures: [workspace-summary.json](audit-rework-review-2026-10-05/evidence/workspace-summary.json).

1. Передавать реальные shaped glyph IDs, cluster advances и offsets через layout к обоим paint backends; одинаковые face bytes сами по себе не равны одинаковой геометрии. PDF char mapping + один `show` не закрывает shaping.
2. Исправить RTL byte ranges: логические spans и визуальный порядок хранить отдельно; после reorder не оставлять `byte_end<byte_start`. [Независимая проба](audit-rework-review-2026-10-05/evidence/acceptance_shape_review.rs) должна стать GREEN. Проверить RTL/mixed direction/combining/ligatures/missing glyphs, exact Unicode round-trip, glyph offsets и selectable text.
3. Проверить per-scalar SVG x и ligatures/marks на actual browser/raster geometry. Degraded complex-script status сохранить для реально непроверенной области; неверная карта Unicode не является допустимым degraded результатом.
4. Caret/selection должны использовать согласованные shaped clusters и точную связь с Unicode; width, boundaries, hit tests и stale revisions проверяются реальным renderer. Устранить `Merged  header`, лишние paragraphs и неверную позицию таблицы в convert.
5. Обновить numbering/render/svg_oracle для допустимого `x`-списка: проверять все элементы списка, не подставлять 0 и не выбрасывать проверки collisions. Negative controls с NaN/out-of-bounds в любом элементе обязаны FAIL.
6. Восстановить SVG/PDF strict-text и strict-text-grid против сохранённых WPS bounds, structural negative controls и extent ratchet. Обосновать изменения R10 paragraphs/table goldens независимо; текущий output не golden oracle.

Приёмка: все 14 исходных failures устранены без ignore/xfail/снижения thresholds; workspace all-features locked и существующие geometry/round-trip tests проходят. SVG/PDF cluster/glyph geometry различается не более чем допускает исходный контракт 0,25 px, source text и media не меняются. Дополнительные independent RED/inverse сохранены.

### D04. Углы и параметры защиты — R02

Владение: normalizer percentage conversion и settings writer/report.

Убрать hue/hueOff из percentage conversion, проверить остальные элементы по их реальным schema types; не глобальным списком похожих имён. [Angle probe](audit-rework-review-2026-10-05/evidence/acceptance_angle_review.rs) сейчас меняет 60000 в 60%. Приёмка: angle сохраняет числовую семантику и валидность; настоящий percentage преобразуется правильно; malformed/negative/extreme values имеют ожидаемые loss/error результаты.

Unknown `cryptAlgorithmSid=9999` сейчас теряется при Clean/empty issues. [Фикстура](audit-rework-review-2026-10-05/evidence/unknown-protection.docx). Поддерживаемые алгоритмы, hashes, salt, spin, enforcement переносить семантически эквивалентно. Неизвестный SID/provider/extension требует точного named loss или отказа по контракту; не заявлять password verification сохранённой без evidence. Приёмка: этот input больше не Clean при потерянном параметре; schema-valid output не подменяет semantic acceptance; corpus protection matrix и 27/121/100 XSD/OPC повторены.

### D05. Убрать census bypass — R03

Владение: census detection/matching, registry, semantic ledger и failure controls.

Заменить whole-part `*` и namespace `a:* / wp:* / c:* / m:*` доказанными dispositions по qualified names/context и конкретному преобразованию. `pattern=w:left` не должен принимать `a:left`. Перейти от наличия local-name где-нибудь в части к обнаружению соответствующих узлов, attributes, multiplicity и semantic/resource changes. Named loss принимается только при фактическом report того же input, а не потому что запись названа named_loss в TOML.

Приёмка: [контроли](audit-rework-review-2026-10-05/evidence/census-negative.json) неизвестных изменений и неправильного namespace FAIL; удаление одного из одинаковых узлов, свойств/значения/ресурса также FAIL или имеет доказанный named disposition. Схемы и inventory по-прежнему раздельны. Все группы 121/100 разобраны, без неявного разрешения любых изменений в regenerated part.

### D06. F16 без ожидания Word

Владение: frames/anchors/table topology, captions/labels/edges и independent geometry harness. Порядок: [приёмка без Word](WORD_INDEPENDENT_ACCEPTANCE_2026-10-05.md).

Исправить все сохранённые WPS residuals: absolute SNP origin 0,73–0,87 px, clade rhythm около 2,5 px, caption overflow, page54 centering, page104 captions 3–5 px. Ledger всех компонентов и реальный SVG/PDF comparison обязателен; subset relative SNP PASS не закрывает всю композицию.

Приёмка сейчас: STRUCTURAL и WPS-PINNED проходят собственные gates с исходным допуском 0,25 px и пригодным provenance. LibreOffice с неправильными наложениями не использовать положительным golden; Word Online PNG без provenance/масштаба не numerical oracle. WORD-COMPAT остаётся NOT_RUN до архивного/нового пригодного экспорта; первоначальный full Word контракт сохраняется, если владелец отдельно его не изменит.

### D07. Матрицы и достоверные receipts — R08/R09

Владение: F00–F21 matrix/runner/schema, inverse witnesses, browser harness.

Закрыть 14 текущих blocked rows из fresh F21 receipt: F03 stage-combos; F04 bitflags/crop; F07 theme/tabs/missing-glyph; F08 RTL/justify/containers; F09 alignments/fields; F10 levels/suffix; F11 widths; F12 pagebreak/nested; F14 align/offset; F15 sides/distances; F16 Word; F17 multipage/regions; F18 positions/rotation; F19 stroke/cubic/pattern. Для out-of-scope primitives проверить честный Unsupported contract; не добавлять поддержку как незапрошенное расширение.

Word row отделить от доступной F16 structural/WPS matrix по D06; не прятать остальные строки под общей недоступностью Word. Measured row связывать с действительно исполненным scenario/test/oracle, а не только статическим status TOML. Хешировать фактические source bytes и inputs, версионировать schema. Most-card inverse gaps устранить либо точно оставить PARTIAL.

Browser runtime pin должен проверять ожидаемую version/build или доставленный binary hash; обнаруженный installed path/version — только описание среды. Отдельно доказать независимость standalone SVG от installed fonts и glyph/cluster fidelity, сохранив существующие live API/DOM/keyboard/escaping tests.

Приёмка: content fingerprint меняется при изменении bytes с неизменным `git status`; missing scenario/test/oracle/input/tool → nonzero/unmeasurable; inverse реально исполняется; runtime mismatch не PASS. `full_audit=false` сохраняется пока существует обязательный незавершённый gate.

### D08. Coverage, fuzz и exact-SHA CI — R10

Владение: meaningful WML branch witnesses, CI/tooling и evidence.

Довести WML branches 657/1030 (63,79%) до исходных ≥70% реальными scenarios; не менять threshold. Сохранить line/file floors обоих аудитов. Instrumented regressions должны проходить; `--ignore-run-fail` позволяет диагностическое измерение, но не закрывает behavioral gate. Согласовать точные JSON numerator/denominator с текстом receipts.

Выполнить предусмотренные восемь fuzz целей по 3600 с в пригодной Linux среде и сохранить bounded run evidence; 60-секундный smoke отдельно. Обеспечить воспроизводимый CC0 corpus gate в доступной CI/local среде с полным manifest/denominator; отсутствие gitignored корпуса не PASS. Exact-SHA external CI получить после отдельно разрешённого push; до этого NOT_RUN, без ссылки на старый зелёный SHA как на текущий.

Приёмка: исходные пороги и все required regressions проходят, никакие dependencies/phantom paths не подменяют область coverage, tools/flags/input hashes записаны; fuzz и текущий CI имеют реальные завершённые результаты либо итог честно PARTIAL.

### D09. Итоговая приёмка и базовая линия — R11

Владение: общий corpus/regression runner и новый status report; production bugs возвращать в соответствующую D-карточку.

Повторить на последнем интегрированном дереве workspace/all-features/locked, fmt/clippy/default/features/MSRV/deny, debug+release hostile, 27 Strict +121 local +100 CC0: open/normalize/write/reopen/rewrite, semantic text/structure/media ledger, XSD/OPC, все страницы SVG/PDF и NaN-safe scan. No stale outputs; ноль входов/оборванный run/missing measurements — nonzero. Return codes measurement tools и продуктовый результат различать.

Итог содержит по каждому gate измеренный статус, hashes и ограничения; historical R-карточки не переписывать как свежие. Waivers не автозакрывать. Word-compatible profile — отдельная ось по D06, engineering profile — отдельная ось. Общий полный заказ закрывается только при выполненных первоначальных контрактах либо документированных изменениях владельца, а не при зелёном F21 subset.
