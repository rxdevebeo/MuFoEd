# Заказ на доработки по итогам приёмки аудитов StrictLib

Дата: 5 октября 2026 года. База: `b413b1594447458fac8101093b6502eacf4ff661`, ветка `master`. Основание: [итоги проверки](AUDIT_ACCEPTANCE_2026-10-05.md), `REWORK-AUDIT-2026-10.md` и `audit-2026-10-04/FIX_PLAN.md`. Этот заказ закрывает обнаруженные дефекты и недостающую приёмку уже интегрированных изменений. Коммиты и публикация требуют отдельного разрешения владельца.

**Решение по приёмке: частичная.** Текущие регрессии проходят; полное закрытие аудитов отклонено. Есть воспроизведённая порча текста, невалидный Strict на новом корпусе, неполное обтекание и невыполненные контракты проверки. Сохранить исправления, которые уже работают, и довести перечисленные ниже обязательства.

## 1. Что сохраняем как рабочую базу

- Workspace all-features locked: 1377 успешных результатов harness, 0 failures, 0 ignored. Clippy `-D warnings`, default check, feature matrix, hostile release, PDF example, lint-eof и cargo-deny проходят.
- Имеющиеся основные синтетические свидетели F00–F21 проходят runner `--all`. Это прохождение выбранных тестов, а не выполнение всех матриц и архитектурных требований карточек.
- Strict-корпус: 27 входов, 26 записанных/валидированных выходов, 1 ожидаемый отказ G-10, 0 новых схемных нарушений, 11 перенесённых нарушений источника; OPC 0.
- Оба ранее падавших теста Operations входят в прошедший workspace. Не возвращать их в очередь как текущий дефект.
- Новый `testdata/CC0_DOCX`: 100/100 SHA-256 и размеров совпали с manifest; 100/100 запись, повторное открытие и рендер первой страницы завершились. Код записи 1 означает Degraded, а не Clean: таких документов 66.
- Бюджет `max_text_box_nesting=5`, стек hostile 1 МиБ и отказ глубины 40 через `XmlDepth` сохраняются. Не повышать лимиты для прохождения.

## 2. Общий протокол исполнения

Перед работой записать HEAD, `git status`, хеши исходников и corpus manifests. Если база изменилась, определить затронутые проверки. Не перезаписывать старые отчёты аудита как будто они измеряли новый код.

Для каждого исправления: компилируемый поведенческий RED на старой реализации → производственная правка → тот же GREEN → обратный контроль конкретной логики в отдельной копии. Ошибка компиляции не RED, кроме предусмотренного планом feature-build сценария. Не менять expected/пороги после RED, не удалять тесты, не вводить ignore/xfail, не обновлять golden текущим выходом.

Артефакты: `target/audit-rework-2026-10-05/Rxx/{red,green,inverse}/`; долговечный результат и команды — `docs/audit-rework-2026-10-05/Rxx_ACCEPTANCE.md`. Receipt содержит code SHA **и хеш фактического дерева**, входные SHA-256, версии инструментов, команды/exit, количество выполненных сценариев, измерения, ограничения, хеши логов. HEAD без хеша dirty tree недостаточен.

Работы с общими parser/writer/layout-файлами выполнять последовательно. После изменения shared DOM/Source/layout повторять затронутые round-trip и workspace-проверки. Не смешивать эти изменения с RTF/DjVu и расширением редактора K00–K15.

## 3. Очередь и зависимости

| Заказ | Приоритет | Зависимости | Результат |
|---|---|---|---|
| R01 | P1 | — | U+FEFF внутри текста больше не портит символы |
| R02 | P1 | R01 | Strict-выход нового корпуса валиден в проверяемой области |
| R03 | P1 | R02 | Census раздельно измеряет схемы, потери и допустимые преобразования |
| R04 | P1 | — | Обтекание действует на все затронутые абзацы и страницы |
| R05 | P1 | R04 | Единый shaping/face/resource для layout, SVG и PDF |
| R06 | P1 | R04, R05 | Активные header/footer и ограниченный reflow соблюдают контракт |
| R07 | P1 | R04–R06 | Научные схемы F16 проходят независимый геометрический oracle |
| R08 | P1 | R01–R07 | Полные матрицы карточек и достоверные RED/GREEN/inverse receipts |
| R09 | P1 | R05, R08 | Поведенческая browser-приёмка шрифтов и loss UI |
| R10 | P1 | стабильный код R01–R09 | Покрытие и CI соответствуют требованиям обоих аудитов |
| R11 | P1 | R01–R10 | Новый базовый статус, финальный corpus/regression gate |

Проверку доказательств R08 и подготовку browser harness R09 можно готовить до исправлений, но окончательную приёмку выполнять на их результате. Приоритет P1 означает блокирование полного закрытия аудитов, а не обязательную срочность релиза.

### R01. Порча текста с U+FEFF

**Владение:** `strict-ooxml-core/src/xml/`, соответствующие WML parser/tests и writer round-trip tests. Причину установить в parser, а не маскировать заменой текста в corpus.

**Доказательство:** [минимальный DOCX](audit-review-2026-10-05/leading-feff.docx), [ожидание/выход](audit-review-2026-10-05/leading-feff.json). В `w:t` вход `U+FEFF + தமிழ்` после одного `write` превращается в `தமிழ்்`; CLI возвращает 0. U+FEFF пропадает, последний тамильский знак дублируется. В новом корпусе изменение `word/document.xml` при второй записи найдено у `025_iyothee_dass_thoughts_religion.docx` и `080_makkal_sattam_1.docx`.

**Задача:** исправить чтение текстовых событий и продвижение позиции. Проверить создание low-level reader на срезе в `XmlReader::read_parsed`; это кандидат причины, а не готовый диагноз. BOM потока и U+FEFF в текстовом узле — разные случаи. Произвольное удаление U+FEFF из содержимого не считается исправлением.

**Матрица:** UTF-8/UTF-16 BOM документа; U+FEFF в начале/середине/конце `w:t`; ASCII, кириллица, Tamil и supplementary code points; соседние runs, CDATA, entities, `xml:space`, следующий XML-тег. Assertions по точной последовательности Unicode code points.

**Приёмка:** минимальная фикстура сохраняет весь текст; нет повторных/пропавших знаков; обе corpus-регрессии проходят; fixed point всех 100 документов после предусмотренной нормализации — 100/100. Запись без редактирования не может возвращать Clean при порче текста.

### R02. Strict-сериализация нового CC0-корпуса

**Владение:** соответствующие `normalize`, WML parse/model, `strict-ooxml-write/src/`, writer tests; XS registry только после доказанного определения класса. Не записывать новые нарушения как source только для прохождения гейта.

**Доказательство:** [XSD-прогон](audit-review-2026-10-05/cc0-xsd.log), [точные примеры сообщений](audit-review-2026-10-05/cc0-xsd-examples.json). 100 выходов: 39 642 schema messages; 39 635 unmatched, 7 зарегистрированных source messages. Это сообщения валидатора, не число независимых дефектов. `IN=0` не доказывает валидность Transitional-входов: их корни этот Strict oracle пропускает.

| Класс выхода | Сообщений | Уже установленное расхождение |
|---|---:|---|
| `rFonts` | 39 013 | `w:hint="cs"` недопустим в Strict: enum `default`, `eastAsia` |
| `placeholder` | 477 | Извлечь точную причину и минимальный самостоятельный repro |
| `documentProtection` | 35 | Transitional `cryptProviderType` в Strict запрещён; проверить остальные атрибуты |
| `tr` | 10 | Строка без обязательного предшествующего `tblGrid` в таблице |
| `ins` | 7 | `w:ins` в месте, где schema ожидает `rPrChange` |
| `smallFrac` | 1 | Проверить отдельный lexical/namespace сценарий, существующий F04 его не закрывает |
| остальные DrawingML/OMML/property группы | 92 | `defRPr`, `satMod`, `gs`, `lumMod`, `ext`, `fillToRect`, `lumOff`, `rPr`, `shade`, `tint`, `alpha`, `docParts`; разнести по первопричинам |
| chart source | 7 | `lblOffset` 5, `gapWidth` 1, `overlap` 1; подтвердить перенос входных байтов |

**Задача:** сначала сделать исчерпывающую классификацию 21 local-name группы: producer/input, normalized part, writer output, generated/passthrough, primary error/cascade. Затем исправить все собственные ошибки. Для защиты документа сохранить поддерживаемую семантику; неподдерживаемая защита требует точной потери/отказа, а не молчаливого снятия ограничений. Для отсутствующей сетки таблицы восстановить согласованную grid по модели, не выкидывать строки.

**Приёмка:** все 100 выходов реально валидированы; missing/unclassified violations=0; собственных нарушений=0. Допустимые source нарушения должны подтверждаться отдельно входом и способом переноса, без широкого исключения по имени элемента. OPC остаётся 0, текст/ресурсы/структура проходят round-trip. Проверить также прежние 27 Strict и 121 Transitional документов.

### R03. Census: достоверная классификация потерь

**Владение:** `xtool/xsd-gate/census_gate.py`, `census.toml`, selftests и источники write/normalize reports по результатам классификации.

**Доказательство:** [прогон 121 документа](audit-review-2026-10-05/census.log). Все 121 выходов валидированы, XSD messages=0, но census exit=1: 4189 inventory entries по исчезнувшим элементам попадают в `unmatched`. Конец лога ошибочно называет их schema violations. `census_hits` объединяет unmatched сигналов `message` и `element`; комментарий в `report` при этом обещает, что неклассифицированный inventory не является schema failure. 4189 — число записей inventory, а не 4189 доказанных потерь и не 4189 схемных нарушений.

**Задача:** разделить `unmatched_schema`, `unclassified_element_changes`, подтверждённые скрытые потери и объявленные преобразования. Для каждой inventory группы определить переименование/регенерацию/служебный пересчёт/именованную потерю/настоящую неназванную потерю. Сопоставлять qualified names и контекст, не только local name. Удаление всех неизвестных записей из проверки запрещено. Непрояснённые изменения оставляют приёмку неполной, но не называются schema error.

**Матрица:** неизвестное XSD violation обязательно FAIL; реальная неназванная потеря — FAIL; допустимое переименование и явно объявленный пересчёт не становятся фиктивной потерей; ноль входов, missing writer output, оборванный прогон и отсутствующая schema — nonzero/unmeasurable. Не игнорировать целый каталог через совпадение одной строки отчёта.

**Приёмка:** старые 121 и новые 100 документов имеют полный inventory с проверенными dispositions; нет неразобранных групп/скрытых потерь; XSD и semantic loss измерены раздельно. Исторические цифры не перезаписывать. Прибор должен доказать отказ на каждом реальном negative control.

### R04. Обтекание и общая геометрия контейнеров — F08/F14/F15

**Владение:** `render-svg/src/layout/{floating,paragraph,paginate,table,mod}.rs` и предусмотренные планом container/exclusions модули; SVG/PDF regression tests.

**Доказательство:** [воспроизводящий скрипт](audit-review-2026-10-05/create_wrap_probe.py), [результат](audit-review-2026-10-05/wrap-probes.log). Запуск из корня: `python docs/audit-review-2026-10-05/create_wrap_probe.py`, затем `cargo +1.92.0 test -p strict-ooxml-render-svg --locked --test acceptance_wrap_probe -- --nocapture`. Скрипт временно создаёт тест в `tests/`; после выполнения удалить только созданный файл, копию оставить в артефактах.

- Square: картинка `(192,96,169,169)`, следующий абзац `FOLLOWING` начинается `(216,127.868)` внутри неё. Основной `f15_square_changes_line_intervals` одновременно проходит.
- TopAndBottom: текст baseline `109.965`, нижний край картинки `265`. Высота объекта не зарезервирована.
- Код собирает exclusions только из segments текущего абзаца; page-relative square пропускается с предупреждением. `free_spans` использует фиксированные 20 px вместо фактической высоты строки. Относительные размеры F14 и wrap должны использовать один resolved extent.

**Задача:** завершить контракт §3.3 и F15 исходного плана: page/container state, регистрация объектов до переноса затронутого текста, перенос exclusions через абзацы/границы страниц, правильные relative boxes, bounded reflow. Контуры Tight/Through остаются явно Unsupported до отдельной реализации; прямоугольник не объявлять точным контуром.

**Матрица:** host и следующие абзацы; Square/None/TopAndBottom; четыре side policies; distances; 2 объекта; фактическая высота строки; page/margin/column/paragraph/line; relative size; page break; таблица/рамка/text box; behindDoc/allowOverlap; scale. Не скрывать пересечение clip-ом.

**Приёмка:** обе новые пробы GREEN; 0 неожиданных ink intersections в поддерживаемой области; geometry error ≤0,25 px при scale=1; одинаковые media hash и object coordinates при смене только wrap; SVG/PDF согласованы. Повторные состояния/несходимость дают явную ошибку, а не последнее приближение.

### R05. Shaping и ресурс шрифта — F07

**Владение:** `render-svg/src/font/`, computed runs/text items/layout/paint и PDF text/font emission; lock/manifest при необходимости.

**Расхождение:** F07 и §3.4 требуют shaping, resource identity и browser gate. Сейчас `LayoutContext::measure` суммирует advance отдельных `char`, `BuiltinFontProvider` читает отдельные glyph widths; renderer не имеет зависимости `rustybuzz`. Основные F07-тесты проверяют строки `@font-face`/family и один fallback width, не shaping/ресурс браузера/PDF.

**Задача:** реализовать единый shaper и registry лиц согласно исходному контракту; pinned MSRV-compatible `rustybuzz`, cluster advances, resource hash и одни bytes для SVG/PDF. Не оставлять браузеру повторный несовместимый shaping. Сохранить выделяемый текст и корректную связь cluster ↔ Unicode.

**Матрица/приёмка:** полный F07; Arabic/Urdu/Tamil/Hindi и combining marks нового корпуса; ligatures/kerning; unchanged text, разные run boundaries, theme/bold/italic/CS, missing glyph/font. Hash layout=SVG=PDF, simple advance error ≤0,25 px, 0 необъявленных ink collisions; standalone SVG работает без системных fonts. Complex-script support, который не реализован, имеет честный degraded статус; это не закрывает обязательные positive scenarios.

### R06. Активные регионы страницы и сходимость — F13

**Владение:** `layout/headerfooter.rs`, `paginate.rs`, page-region state и tests.

**Расхождение:** резерв вычисляется по самому высокому из всех section references, а не только активному first/even/default. `tallest_region` измеряет без actual FieldEnv. `layout_document` делает 8 повторов по page count, но при исчерпании молча возвращает последнюю раскладку; исходный контракт требует явной ошибки несходимости и проверки region geometry.

**Задача/приёмка:** полный F13, first/even/default и наследование выбирать до измерения активного региона; PAGE/NUMPAGES/SECTIONPAGES и фактический размер региона участвуют в convergence. В ≤8 проходов достигается стабильная geometry или возвращается точная ошибка. Неактивный высокий footer не уменьшает body чужой страницы. Tests: tall region, динамическое поле с переходом разряда, смена секции, отрицательные поля/разрешённое исходное overlap. 0 неожиданных collisions; oracle использует ink/actual boxes, а не только разницу baseline 10 px.

### R07. Научные композиции — F16

**Владение:** frame grouping/origins/container graph, связанные anchors/tabs/decoding; `visual-thesis` regression harness. Зависит от исправлений геометрии и шрифтов.

**Доказательство:** `audit-2026-10-04/CLOSURE.md`: страницы 54, 56, 104 не проходят 0,25 px. На странице 56 есть сдвиги подписей порядка −359 px по x и −425 px по y; дерево не собрано. JPEG на 104 сохранены, но это не доказывает сохранность подписей. WPS-reference уже существует; отсутствие любого эталона больше не является точным описанием блокера.

**Задача:** инвентарь labels/edges/images/groups и численный oracle каждого компонента; исправить размещение и читаемость labels. Зафиксировать source/reference hashes, producer/version/export/page size. Текущий WPS — измерение против конкретного producer; он не называется Word. Для полного первоначального Word-контракта получить Word reference либо отдельно согласовать с владельцем другой профиль. Недоступность Word не блокирует устранение уже измеренных расхождений с WPS.

**Приёмка:** 0 отсутствующих/дублированных компонентов, сохранённая topology, relative positions и baselines ≤0,25 px; media неизменны; SVG/PDF. Допуск не ослаблять. До выполнения соответствующей части статус PARTIAL/BLOCKED с причиной; не хардкодить `BLOCKED` как доказательство полноты.

### R08. Матрицы, RED/inverse и публичный runner — F00–F21

**Владение:** testkit, manifest/runner/CI и tests карточек. Производственные правки — через соответствующую R-карточку.

**Расхождение:** локальные `target/audit-fixes` содержат 24 JSON receipts, только 22 GREEN карточек и служебные отрицательные receipts. RED/inverse F01–F20 в заданных каталогах не найдены. GREEN receipts показывают старый HEAD `7a8e6d3`, хотя исполнялось dirty tree, не записывают hash дерева. F07 заявляет будущие font hashes, но не содержит их. `f21_public_suite` проверяет текст manifest/имена функций/константы, а не запускает поведение. CI вызывает `--task F21`; отдельный полный workspace при этом выполняет Rust-тесты, поэтому это не утверждение, что CI вообще не тестирует код.

**Задача:** матрица «каждый обязательный сценарий FIX_PLAN → exact test → oracle → RED/GREEN/inverse → receipt». Восстановить существующие свидетельства, если они доступны, иначе честно повторить их на baseline/inverse; не создавать фиктивную историю. Исправить metadata/receipt schema. Публичная сквозная suite должна выполнять поведенческий manifest, иметь точный счётчик и обязательные внешние gates; metadata test можно сохранить отдельно.

**Приёмка:** каждый обязательный сценарий измерен либо явно BLOCKED; fail-closed при missing/zero tests/tools/fixtures; каждое собственное исправление имеет компилируемый inverse. Полная матрица F05–F19 включает cases из плана, не только основной happy path. Corpus pass и synthetic pass — разные результаты. Зелёный `--task F21` не может означать полный аудит при невыполненных зависимостях.

### R09. Browser-приёмка — F07/F20

**Владение:** viewer UI/API, browser harness, standalone font fixture; согласовать с R05.

**Расхождение:** `f20_successful_view_exposes_normalization_loss` делает TCP GET и ищет строки в HTML/JSON. JavaScript и interaction он не исполняет. F20 требует browser test с живым API; F07 — проверку реально загруженного лица.

**Матрица/приёмка:** pinned browser, actual font load/bytes/hash, отсутствие зависимости от installed fonts, selectable text; viewer clean/degraded/failed/not_run, раскрытие losses клавиатурой, escaping деталей, many issues, accepted/rejected stale sidecar. Assertions по DOM и API после выполнения JS. Если runtime отсутствует — BLOCKED этой проверки, не PASS статического HTML. Сохранить существующий API-test.

### R10. Coverage и CI обоих аудитов

**Владение:** `.github/workflows/ci.yml`, coverage harness и meaningful tests крейтов; без изменения thresholds/waivers ради закрытия.

**Расхождение:** §15 старого REWORK требует core/wml ≥85%, render-svg ≥82%; write ≥80%, pdf/render-pdf ≥75%, convert ≥70%; core `xml/escape.rs`, `opc/policy.rs`, `opc/path.rs` — 100% строк; branch core/wml ≥70%. CI измеряет лишь пять крейтов на уровне 80%, дополнительные четыре и branch gates отсутствуют. Исторический Linux wml 83,06% не достигает 85%; core целевые файлы 98,95/98,65/92,09% не достигают 100%. Эти цифры относятся к старой dirty copy, не новому HEAD.

**Задача:** измерить свежий интегрированный код на Linux, pin версию инструмента и область package-only, clean dedicated target, исключить фантомные пути. Добавить предусмотренные гейты и необходимые поведенческие tests. Сохранять numerator/denominator, flags, file list и actual tool exits. Решение менять первоначальный контракт принимает владелец отдельно.

**Приёмка:** все целевые пороги реально измерены и выполняются; package dependencies не подменяют denominator. MSRV 1.92; публичные regressions, XSD/OPC, CC0 matrix и optional-element gates в CI имеют измеряемый результат. Длительный fuzz: отдельный результат восьми целей по 3600 с; 60-секундный smoke не подменяет его. Если он не исполнялся — NOT_RUN.

### R11. Финальная приёмка и базовая линия для планирования

**Владение:** итоговая acceptance matrix, receipts, документы состояния; код на этом этапе не менять без возврата к конкретной карточке.

**Обязательные corpora:** 27 Strict; 121 существующих local Transitional; 100 `testdata/CC0_DOCX`, каждый по зафиксированному manifest/hash и с полным denominator. Новый корпус не удалять/редактировать для прохождения. Его manifest декларирует CC0 и содержит provenance; этот заказ не подтверждает права третьих лиц самостоятельной юридической проверкой.

**Обязательные измерения:** open/normalize/write/reopen/rewrite fixed point, exact semantic structure/text и media ledger, XSD/OPC, все страницы render без panic/NaN и потерянных components, SVG/PDF geometry, browser, meaningful coverage и CI. Нынешний CC0-прогон проверил render **первой страницы**; полнота страниц и faithful multilingual layout им не приняты. Для `NaN` проверять SVG markup с исключением base64 `@font-face`, а не строку во всём payload.

**Внешний CI:** последний успешный run [37195191322](https://github.com/rxdevebeo/MuFoEd/actions/runs/37195191322) относится к `7a8e6d3`; текущая master впереди remote на 6 commits. После отдельно разрешённых commit/push получить run точного итогового SHA на Ubuntu/macOS/Windows, coverage/XSD/MSRV/deny/fuzz. Старый run не является приёмкой нового кода. Не утверждать external CI PASS до завершения.

**Результат:** обновить отдельную сводку текущего состояния (старые аудиты остаются историей): что принято, на каком code/tree hash, что degraded/unsupported/blocked/not_run, подтверждённые риски и зависимости следующего плана. Различать code acceptance и полноту возможностей продукта. Старые waivers перечислить и перепроверить; не закрывать их автоматически успешным workspace.

**Общий критерий завершения:** R01–R11 выполнены по своим контрактам и evidence, отсутствуют собственные схемные нарушения/порча текста/скрытые потери/неизмеренные обязательные сценарии. Принятые ограничения имеют отдельное решение владельца. Частичный результат можно зафиксировать как базу, но он не получает статус «все аудиты закрыты».

## 4. Что не расширять в рамках заказа

Новые импортеры RTF/DjVu, UI полного редактора, K00–K15, добавление cubic/clip/pattern/Tight/Through как новых supported primitives — отдельные product work orders. AUD-99 (снятие vendored hayro) зависит от доказанного эквивалентного upstream release; локальные guards сохранять. Списки unsupported и существующие waivers не являются разрешением скрывать реальные потери либо дефекты поддерживаемого пути.
