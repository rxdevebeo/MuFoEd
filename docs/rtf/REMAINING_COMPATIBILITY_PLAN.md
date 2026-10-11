# План оставшейся совместимости RTF

Дата: 7 октября 2026. Историческая база RTF: `a7b3c9f`, ветка `codex/rtf-import`.
Текущий интеграционный baseline: ветка `codex/rtf-import-integrated`, HEAD `5cccef4`
(merge `master` `956948f` от 8 октября; см. «RC00: повторная интеграция master»); подробности и ограничения — в [RC00_INTEGRATION.md](RC00_INTEGRATION.md).
Карточки остаются планом; RC00/RC01 частично закрыты, RC02 font-инфраструктура завершена, приёмка Blocked на внешнем harness. Таблица ниже — исторический снимок 5 октября и не описывает новый прогон.
## Цель и исходное состояние

Цель — закрыть совместимость пользовательского корпуса из 100 RTF по содержимому,
структуре и отображению через публичный импорт. Приёмка поддержанного профиля
R07 и интеграция R08 рассматриваются отдельно. Наличие документированного
unsupported элемента позволяет объяснить Degraded, но не закрывает полную
совместимость документа с этим элементом.

Основание: [ACCEPTANCE.md](ACCEPTANCE.md),
[acceptance.development.json](acceptance.development.json),
[структурные probes](structure.wps-comparison.json).
Полная очередь документов: [compatibility.backlog.json](compatibility.backlog.json).

| Проверка снимка `a7b3c9f` | Результат |
| --- | --- |
| Import / exact body text / text+media roundtrip | 100/100 |
| Media parts, сохранённые побайтно | 38 |
| Strict XSD / OPC | 100/100, 0 violations |
| Пагинация WPS | 61 Pass, 39 Fail |
| Pixel gate документов | 2 Pass, 98 Fail |
| Страницы с установленным соответствием | 244; 7 Pass, 237 Fail |
| PDF report | 48 Clean, 52 Degraded |
| `pdf.font.missing` | 47 документов, 53 различных labels family |
| `pdf.font.no_glyph` | 6 документов |
| `pdf.image.unsupported` | 3 документа: 001, 004, 006 |

Категории пересекаются: 50 документов имеют font/glyph diagnostics, 52 — хотя бы
одну из трёх PDF diagnostics. У 28 из 39 pagination failures есть
`pdf.font.missing`; это корреляция, а не доказательство причины. Все 100 import
reports остаются Degraded. Body equality не проверяет notes/comments/revisions.

Два pixel PASS (040, 046) сохраняются как положительные controls; они не означают
полную совместимость всех свойств этих документов.

## Общие контракты и правила

Источники RTF и raw WPS references неизменяемы. Каждая карточка получает минимальный
synthetic fixture, независимый reference, воспроизводимый RED, исправление,
GREEN и отрицательный контроль. Изменения serialization сравниваются по смыслу
с разрешением IDs/relationships; отсутствующее свойство не считается равным
выведенному по догадке. WPS quirk отделяется от корректного сохранения RTF.

Базовые gates сохраняются: знаменатель 100, точный текст, hashes media,
Strict/OPC, CLI 0/1/2, отсутствие silent loss, bounded reports. Нельзя автоматически
назначать observed FAIL ожидаемым результатом, отключать неизвестные schema
violations, улучшать SSIM resizing/blur или повышением порога допустимой ошибки.

Новый render contract должен включать diagnostics SVG/layout, а не только PDF.
Clean возможен только при выполнении заявленного mapping/coverage. Фактический
fallback получает Degraded и provenance даже при визуально похожем результате.

Runtime строит WML и использует единые writer/renderers. WPS остаётся oracle;
не вводится конвертация через офисное приложение, OCR или PDF в runtime.
Для сложных конструкций расширяется общая модель с проверками всех потребителей;
скрытые XML fragments не подменяют недостающий контракт.

## Интеграция с другими работами

Основной checkout на момент интеграции: `ac51b68`. RTF-интеграция собрана отдельной историей поверх него; цепочка и конфликты описаны в [RC00_INTEGRATION.md](RC00_INTEGRATION.md). Интегрированная база содержит shaping и shared resolved faces вместе с существующими P5 run/paragraph metrics и P6 table widths. Статусы другого корпуса не принимаются за доказательство RTF совместимости.

Граф `D-projects-StrictLib` относится к основному checkout, а не RTF worktree. Для существовавших файлов graph coverage показал изменившиеся метаданные; новые RTF файлы в индексе отсутствовали. Поэтому текущие выводы проверялись непосредственно по исходникам и тестам.

Работа ведётся в отдельном worktree `C:\Users\gamer\.codex\worktrees\rtf-import-integrated\StrictLib`. RC00 включал только зафиксированные RTF изменения; незакоммиченные файлы иных очередей не переносились. Изменения общих model/font/layout/writer контрактов требуют согласованной единой реализации. Коммиты остаются в отдельной ветке и не означают merge в основную ветку.
## Очередь и зависимости

| ID | Приоритет | Пакет / область ответственности | Зависимости |
| --- | --- | --- | --- |
| RC00 | P0 | Интеграция и новый baseline; manifests, shared core/render/write | — |
| RC01 | P0 | Точная диагностика и ledger; PDF report, RTF harness/oracles | RC00 |
| RC02 | P0 | Единое разрешение fonts; shared font code и PDF embedding; associated-script mapping | RC00, RC01 |
| RC03 | P1 | Complex shaping, bidi, glyph clusters; renderer + RTF language mapping | RC02 |
| RC04 | P1 | Styles и paragraph/run geometry; RTF mapping + shared P5 | RC01, RC02 |
| RC05 | P1 | Numbering/lists; RTF lists, WML numbering, writer/layout | RC04 |
| RC06 | P1 | Tables; RTF builder + shared P6/layout/writer | RC04 |
| RC07 | P1 | WMF/EMF и picture properties; shared media/render pipeline | RC00, RC01 |
| RC08 | P1 | Sections/regions/notes/fields; RTF + WML/reader/writer/layout | RC01, RC04 |
| RC09 | P2 | Остальные loss classes; RTF mapping и необходимые shared types | RC01; RC08 для fields/revisions |
| RC10 | P1 | Hostile/resource/determinism gates; все новые декодеры и builders | Каждый реализованный RC02–RC09 |
| RC11 | P1 | Corpus/profile acceptance; independent facts, thresholds, receipts | RC02–RC10 |
| RC12 | P1 | Public feature matrix, CI и release integration | RC11 |

```mermaid
flowchart LR
  R00[RC00 integration] --> R01[RC01 ledger]
  R01 --> R02[RC02 fonts]
  R02 --> R03[RC03 shaping]
  R02 --> R04[RC04 styles / metrics]
  R04 --> R05[RC05 lists]
  R04 --> R06[RC06 tables]
  R01 --> R07[RC07 metafiles]
  R04 --> R08[RC08 regions / fields]
  R01 --> R09[RC09 other losses]
  R03 & R05 & R06 & R07 & R08 & R09 --> R10[RC10 hardening]
  R10 --> R11[RC11 acceptance]
  R11 --> R12[RC12 public / CI]
```

### RC00 — интеграция и воспроизводимый baseline

Сначала сравнить уже закрытые shared changes с RTF исправлениями: core pipeline,
list writer ordering, PDF CIDSystemInfo/CMap/FontBBox/aliases, model props,
resolved faces, shaping и DrawingML geometry. Для каждого совпадения определить
единственную реализацию и сохранить regression tests. Интеграцию делать отдельным
пакетом, сохраняя историю RTF; новые renderer fixes не писать поверх старой базы.

Артефакты текущего частичного baseline и команды приведены в [RC00_INTEGRATION.md](RC00_INTEGRATION.md). Public import для 100 источников, точный текст, целостность выходов, resource supervision, Strict XSD и OPC проверены после интеграции. Body/region facts, media roundtrip, render/pagination, pixels и feature matrix по-прежнему требуют повторного прогона. WPS recapture нужен только при изменении oracle contract, а не при изменении кода.

Статус: частично выполнено. Согласованные shared contracts и зафиксированные RTF fixes интегрированы; проверенные gates не показывают регрессий. Повторный visual snapshot: 62/100 pagination Pass, 3/100 pixel Pass; это development measurements, не полный baseline/acceptance. После следующего измерения 62/100 page counts остались неизменными; один PDF hash изменился без изменения числа страниц.
### RC01 — диагностический ledger без ложных причин

Первичная интеграционная ошибка исправлена в `8e612a8`: PDF renderer теперь
различает отсутствующее семейство и отсутствующий glyph; тест на U+10FFFD
проверяет `pdf.font.no_glyph` без ложного `pdf.font.missing`. Добавлен per-feature
счётчик с bounded map (4096 имён, до 128 UTF-8 bytes каждое) и unattributed
overflow totals. Текущий [rc01-loss-ledger.json](rc01-loss-ledger.json) обновлён
после `ee0b012`:
208,166 Loss, 95 Info, 455 feature names; суммы сходятся по каждому файлу. До
RC02 было 1,327,900 Loss/459 features; `\\ab`, `\\ai`, `\\afs` сняли ровно
350,991 событий; mapping определённых `\\af` снял ещё 274,969, а 48 ссылок на
несуществующие definitions получили точные `undefined-font:*` labels. Mapping
`\\lang`/`\\langnp` и `\\langfe`/`\\langfenp` снизил остаток на 178,013 Loss;
script-font selector mapping снял ещё 274,193 события; `\\alang` снял ещё
41,568 generic Loss при сохранении неизвестных LCID как Loss. Неизвестные LCID и
неподдержанный `\\fcs` class остаются явными Loss. У четырёх документов
усечён только retained detail stream; feature counters не усечены. Namespace
`control:*` теперь даёт 164,380 loss events. Подтверждён общий parser fallback для
unhandled control words (`parser.rs:2494`), но влияние каждого слова на вывод ещё
не установлено. Ранжирование ledger задаёт triage, а не диагноз. Backlog остаётся
unreviewed.

Собрать все import loss classes из полных totals/streams, включая записи сверх
cap. Для каждого класса: mapping owner, реальные witness IDs, неизвестная причина,
проверяемая гипотеза и минимальный fixture. Раздельно хранить confirmed diagnosis,
WPS-only quirk и unsupported contract. Уточнить facts sections, styles/defaults,
list restarts, tables/merges, regions, custom marks и поля вне inline модели.

Статус: частично выполнено — totals и наблюдаемые feature streams записаны; точная пофайловая семантическая классификация остаётся открытой. Закрытие: все 100 документов учтены, все обнаруженные loss classes имеют владельца,
missing glyph не выдаётся за missing face, mutations каждого нового fact group
обнаруживаются независимо. Ledger классифицирует работу, но не утверждает exceptions.

### RC02 — associated-script formatting, реальные fonts и единый resolved face

Частично обработаны `\\ab`, `\\ai` и `\\afs`: они хранятся в существующих
WML run fields `bCs`, `iCs`, `szCs`. `\\af` разрешается в East Asian font slot,
отдельно от обычного `\\f`; font switch, group state и import → write → reopen
закреплены fixture. В корпусе эти mappings сняли 625,960 generic Loss событий,
сохранив 100/100 text и resource gates; 48 отсутствующих font definitions
продолжают честно выдавать Loss. `\\lang` family сохраняется в language slots.
`\\loch`, `\\hich`, `\\dbch`, `\\rtlch`/`\\ltrch` и `\\fcs0/1` выбирают WML font
slot; последующие `\\f`/`\\af` обновляют этот slot, а group scope восстанавливается.
Fixtures покрывают ASCII, High-ANSI, East-Asian, complex-script, unknown `\\fcs`
и import → write → reopen. `\\alang` записывается в WML `bidi` language slot; LCID
1093 сохраняется как `bn-IN`, неизвестные 1024 и 255 остаются Loss. В корпусе
после `\\alang` осталось 208,166 Loss. Затем font-table theme selectors
`\\fbimajor/minor`, `\\fdbmajor/minor`, `\\fhimajor/minor` и `\\flomajor/minor`
были связаны с соответствующими WML theme slots при сохранении fallback face;
fixture проверяет все восемь ссылок через import → write → reopen. На корпусе
снято ещё 751 Loss, осталось 207,415 Loss и 95 Info; text/resource — 100/100,
Strict XSD — 100/100 без нарушений, OPC — 100 пакетов без нарушений. Pagination
осталась 62/100 и все page counts прежние; один PDF hash изменился. Pixel gate —
3/100 Pass, 97/100 Fail (423 сопоставленные страницы: 6 Pass, 417 Fail), без
изменения comparator. Это не полная визуальная приёмка. Commit `4725086` распознал
7 font-table `\\fbidis` hints и 3 document-level `\\fbidis` controls. Для
document-level флага `\\f` уже задаёт один active face для WML slots, а дальнейший
`\\af` не переключает associated face; оба пути закреплены fixtures. Итог: 207,405
Loss, 95 Info, 445 features. Следующий приоритет RC02 — единый resolved face,
font registry и embedding; `\\fcharset` и `\\fprq` уже читаются в font table.

Использовать shared face/shaping API после RC00. Определить один resolved face
для measurement, glyph coverage, SVG emission и PDF subset: font SHA-256, face
index, variation coordinates, weight/slant и mapping provenance. Font assets
принимаются явным manifest; default остаётся deterministic bundled provider.
Локальные fonts используются только как явно выбранные, зафиксированные assets,
а не через неявный поиск по машине. Runtime API принимает owned/shared font bytes.

Новый контракт: optional font-assets registry по умолчанию отсутствует; текущий
builtin путь остаётся доступен. Missing-resource policy по умолчанию
ReportAndContinue, строгая policy — Reject. Registry key включает hash, face index
и variation coordinates, а не только family. Existing `ResolvedFace` main содержит
`&static` bytes/family и является Copy: для загружаемых assets нужен отдельный
owned handle и compatibility adapter; незаметно заменить lifetime нельзя.
`FontProvider` с одними metrics/advance не может быть единственным источником
embedding bytes. Этот shared API утверждается в RC02 до изменения потребителей.

Требуются три явно различимых результата: exact face, documented substitute,
unresolved face/glyph. По умолчанию unresolved сохраняет отчёт Degraded; строгий
профиль отклоняет такой render. Substitute не становится exact по близкому SSIM.
Права embedding и availability assets фиксируются до добавления в distributable
bundle; недоступный asset остаётся конкретным блокером, а не обещанием загрузить его.

Первые свидетели: 002 Futura/Garamond, 003/017 Georgia, 020/033 LucidaGrande,
007 Sanskrit 2003, 011 Vijaya, 018 Amar Bangla Normal. Для WPS отдельно установить
фактический resolved face: requested name в DOCX и installed-font inventory сами
по себе этого не доказывают. Проверить aliases, styled PostScript names, symbol/CJK,
missing glyph, reproducibility и font hashes. Не объявлять точность для 53 labels
без проверки: часть исходных labels относится к ошибочной диагностике RC01.

Закрытие: ни один заявленный поддержанным run не теряет glyph/text; faces и metrics
согласованы во всех backend, PDF Unicode extraction совпадает с logical text,
SVG переносит нужные faces, повторный запуск даёт одинаковые результаты.
Шесть уже исправленных alias документов остаются regression controls.

### RC03 — shaping и двунаправленный текст

Сначала использовать существующий harfrust/cluster pipeline main; не создавать
второй shaper для RTF. Проверить language/script/rtl/font slots из RTF, порядок
logical/visual clusters, combining marks, GSUB/GPOS, glyph offsets и границы
переноса. PDF должен сохранять Unicode clusters, а не только одиночные glyph IDs.

Свидетели: 007 Sanskrit, 010 Arabic, 011 Tamil, 018 Bengali; synthetic mixed
LTR/RTL с цифрами и punctuation, ligatures, combining sequences и non-BMP.
References должны измерять glyph origins/advances и подтверждать extracted text.

Закрытие: независимый cluster/geometry oracle и Unicode extraction проходят
для каждого заявленного script; отсутствие перестановки текста, разрывов clusters
и ложного Clean. Для оставшегося script — отдельная открытая карточка.

### RC04 — styles и геометрия текста

Зафиксировать cascade: document defaults → paragraph style/inheritance → character
style → direct properties; отдельно проверять явный false, plain/pard, cs slots,
language, fonts, half-points, character scale/spacing, tabs/leaders, auto/exact/
at-least spacing, keep/widow rules, hidden text и section defaults.
Общие исправления metrics брать из P5, а RTF очередь отвечает за mapping и witnesses.

Начать со style/spacing/tabs probes и 019/039 (одинаковое число страниц при pixel
FAIL), затем 052/076 и остальные документированные paragraph/layout failures.
Сходный page count не снимает ошибок line origins и paragraph placement.

Закрытие: независимые effective-style facts и line/paragraph geometry совпадают
на выбранных probes, units/defaults не угадываются, targeted pixels проходят,
ранее закрытый текст/roundtrip остаются 100/100.

### RC05 — списки и numbering

Разобрать restart/start override, level overrides, counter continuity между
параграфами/таблицами/секциями, marker fonts, suffix/tab/space, indentation и
legacy list constructs по ledger. Отрицательный list ID сохранять как корректную
source identity; случай, где WPS теряет numbering, проверять отдельно по спецификации
и второму независимому reader, а не имитировать потерю.

Закрытие: marker values/sequence/geometry и numbering XML проходят independent
facts, Strict XSD и reopen; нет двойного cached list text или потерянного restart.
Fixtures охватывают legal forward refs, missing abstracts, overrides и limits.

### RC06 — таблицы

RTF queue проверяет row/cell mapping, repeated definitions, grid, horizontal/
vertical merge, padding, alignment, widths, borders/shading, row heights,
header rows, nesting и разрывы страниц. Shared table geometry и units брать из P6.
Nested table support добавлять через bounded WML builder; до реализации явно Reject,
не flatten без loss. Свидетели определяются ledger, плюс existing vertical_merge
probe с уже подтверждёнными grid/text и fixtures mixed merged cells.

Закрытие: независимые grid/merge regions, cell text и widths совпадают;
renderer не дублирует/не теряет cells, page breaks воспроизводимы, resource bounds
сохраняются. Нельзя закрыть таблицу только совпадением concatenated text.

### RC07 — WMF/EMF и свойства изображений

Первый набор: 001, 004, 006 и portable positive/hostile fixtures. Сохранённый
payload/extent — исходный control, не доказательство rendering. Выбрать общий
bounded record decoder в typed vector primitives, используемый SVG и PDF;
растеризация допускается отдельной явно отмеченной policy с фиксированным DPI.

Проверить placeable/standard WMF, EMF coordinate transforms, object handles,
SaveDC/RestoreDC, clipping, strokes/fills/text, scaling/crop и bitmap records.
Каждый реально используемый record получает implement/test/disposition; неизвестный
влияющий record даёт Degraded/Reject, не success placeholder.
При неизвестном state-changing record отклоняется отрисовка соответствующего
picture с отчётом: нельзя пропустить команду и продолжить с недостоверным DC state.
Предлагаемые стартовые ceilings: 100000 records, 4096 live handles, 64 saved DC;
decoded bitmap bytes ≤ `max_single_uncompressed`. Это новые defaults для проверки,
а не утверждение о текущем коде; уменьшать/уточнять их по fixtures и корпусу.

Закрытие: оригинальные media bytes сохраняются, reference geometry и pixels
совпадают в обоих backend; malformed lengths/overflow/handles/records/decode bombs
отклоняются до allocation и не обходят import/render budgets.

### RC08 — секции, регионы, примечания и сложные поля

Проверить section inheritance/reset, margins/header/footer distances, columns,
first/even/odd regions, page numbering, note placement/numbering и ссылки.
Свойства из региона не должны менять body. Для custom mark сохранять glyph,
положение, формат и связь с note; unpaired marker не становится автоматической ссылкой.

Для multi-paragraph/nested fields сначала выбрать общий WML контракт с boundary
markers или block result; нынешний `Field.inlines` не представляет block result.
Reader/writer/layout/edit/report должны поддержать контракт до включения mapping.
Preserve instruction + cached result, bookmarks/hyperlink targets и relationships;
пересчёт динамических полей — отдельная явная функция, не скрытый импортный эффект.

Свидетели: regions/field_paragraphs probes, 003/015, 002 с несколькими секциями,
052 и synthetic custom notes/columns/links. Закрытие: exact region text/refs,
instructions/targets/geometry, Strict/OPC и reopen; отсутствие flatten с ложным Clean.

### RC09 — остальные классы потерь

По RC01 разложить comments, revisions, textboxes/shapes, OLE, metadata и прочие
controls на mapping tasks с witnesses. Использовать существующие WML revision/
DrawingML types, проверяя final/original view и семантику сохранения. OLE сохраняется
как opaque payload/relationships без activation; preview измеряется отдельно.
Сохраняемые авторство/даты/документные свойства не считать визуальными glyph losses,
но потерю этих данных по-прежнему показывать в import report.

Закрытие: у каждого наблюдаемого класса есть implemented semantics и independent
roundtrip test либо явное неподдержанное правило с открытым compatibility status.
Последнее достаточно для честного supported profile, но не закрывает полную цель
по документу, использующему этот класс. Blanket ignore/rename-to-info запрещён.

### RC10 — hardening и границы ресурсов

Выполняется вместе с каждым новым decoder/builder, не откладывается до конца.
Проверить debug/release hostile inputs, budgets fonts/media/records/nodes/clusters,
time/memory, cycles/forward refs, issue cap и точные totals. Нужны controls на
abort/write failure и неизменность источника. Fuzz targets имеют фиксированный seed,
corpus и time budget, crash artifacts не выдаются за successful documents.

Закрытие: ни одного panic/hang/silent fallback вне policy; import supervisor
30 s / 512 MiB и render 90 s / 1 GiB измерены на всех 100 файлах с достоверным
peak RSS. Новые публичные limits документированы и проверены на границах.

### RC11 — полная корпусная и профильная приёмка

Независимо review support matrix/expectations для всех 100 inputs: semantics,
oracle versions/hashes, fonts, expected outcome, resources и каждый допустимый Loss.
Не заполнять pending expectations из текущих failures. Структурные comparisons
проверяют эффективные свойства и все регионы, а не только выбранные style names.

Для полной corpus compatibility: 100/100 content/media/structure, 100/100 page
counts, все сопоставленные страницы проходят pixel/geometry policy, 0 silent loss,
0 unexpected/Blocked, 0 необъяснённых degradation. Известный WPS quirk с проверенной
RTF семантикой имеет отдельный reference и явный статус; общий WPS PASS тогда
не заявляется. Word compatibility не объявляется на основании WPS capture.

Действующий development pixel threshold SSIM 0.96 и structural bounds не ослаблять.
До acceptance закрепить reviewed profile/threshold version и независимые positive/
negative controls. Неполная font availability или unresolved renderer/model feature
блокирует соответствующую полную приёмку; не удалять строку из знаменателя.

Закрытие: свежий source/feature/font/oracle/binary receipt, 100 verdicts, все page
metrics, tests и воспроизводимый clean checkout. R07 supported-profile release
может быть отдельным результатом с перечисленными ограничениями, но не названием
для незакрытой полной corpus compatibility.

### RC12 — публичная интеграция и CI

Проверить no-default/base, rtf-only, default, all-features и relevant platform
combinations; CLI status/reports, viewer diagnostics, source/media/font lifetime.
Portable synthetic fixtures входят в CI на Windows/Linux/macOS без WPS.
Полный локальный корпус и WPS references запускаются воспроизводимым отдельным
acceptance job с asset/oracle access; их отсутствие означает NotRun/Blocked.

Закрытие: current-SHA CI подтверждён, existing shared queues не регрессировали,
100 inputs проходят публичный путь, frontend показывает resolved/substitute/
unsupported statuses и подробный report. Включённый workflow без успешного run
не является доказательством. Merge/release — отдельные завершающие операции
после готового reviewable diff и проверок.

## Порядок исполнения и артефакты

Первая итерация: RC00 → RC01 → RC02. Затем RC03/RC04; RC07 можно вести независимо
после общей диагностики. После RC04 идут lists/tables/regions; RC09 закрывается
по semantic impact. Любая параллельная работа требует отдельных paths/владельцев
и общего integration contract; shared `parser.rs`, model и layout не имеют двух
независимых авторов одновременно.

Для карточки: `docs/rtf/compatibility/RCxx/{STATUS.json,tests.md,witnesses.json}`,
ignored raw artifacts `target/rtf-import/compatibility/RCxx/`. Receipt фиксирует
точный commit/tree/dirty diff, options, commands/exit codes, resource/font/oracle
hashes и до/после каждого witness. Status: Planned → InProgress → Verified;
Blocked содержит конкретный отсутствующий asset/contract, а не истёкший срок.

Срок полной совместимости уточняется после RC00–RC02. Триаж 97 pixel failures
выполнен 2026-10-09 (см. [PIXEL_TRIAGE.md](compatibility/PIXEL_TRIAGE.md)): они
разбиты на доказуемые gate-механизмы, и доминирующий класс — RC04 metrics, а не
fonts. Реалистичная оценка остаётся привязанной к RC03/RC04 (shaping/metrics) и к
отдельным witnesses пагинации, а не к интеграции новой базы.

### RC02: East Asian face selection — 2026-10-07

В `ComputedRun` сохраняется отдельный East Asian font slot: direct `w:eastAsia`
или theme `w:eastAsiaTheme`. CJK-only текст выбирает этот face через общий
`chosen_family`; смешанный Latin/CJK run сохраняет основной face. Math-run
сбрасывает оба script slots. PDF collection, shaping и выбор embedded face теперь
используют тот же resolver, что SVG measurement/paint. Direct/theme SVG regression,
RTF → SVG regression и PDF embedded BaseFont regression проходят. Проверка PDF
доказывает выбор bundled substitute (Arimo/Tinos), а не наличие CJK glyph coverage.
Owned font registry, exact CJK assets, per-script splitting смешанных runs и полный
resolved-face identity contract остаются открытыми; RC02 не закрыт.

Повторно прочитаны receipts `target/rtf-import/RC02-eastasia-face-{corpus,visual,pixels}`:
100/100 Measured, text/resource/integrity Pass; pagination 62/100 Pass;
pixels 3/100 Pass, 97 Fail, 423 страницы (6 Pass, 417 Fail). Comparator и thresholds
не изменялись. Это development measurement, не visual acceptance. CLI SHA-256:
`5665763b1c104346810ee7ed692f96379f1ab954d20f508c13d718d2e1b7d683`.
XSD/OPC 100/100 без нарушений — результат предыдущего запуска, в этой сессии
эти gates не повторялись.

В текущем worktree `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
(ветка `codex/rtf-import-integrated`) повторены full PDF/public API suites с
`cargo +1.98.0 test --locked -p strict-ooxml-render-pdf -p strict-ooxml --features rtf,pdf`
и SVG `--test themes`: Pass. Логи `RC02-eastasia-face-pdf-rtf-tests.log` и
`RC02-eastasia-face-themes-tests.log` в `target/rtf-import/`.
Full SVG baseline имеет противоречивый `maps_regular_postscript_names_to_the_same_bundled_faces`:
он ожидает сохранения `Arial-BoldMT`, но unchanged resolver возвращает Carlito.
Первый повтор также не имел ignored Clio DOCX fixture; fixture восстановлен из
основной копии. Эти результаты не заменяют безусловный full-suite Pass.

Clippy на Rust 1.92 прошёл для трёх crates, `--all-targets --features rtf,pdf`,
с `-A clippy::manual_let_else -A clippy::items_after_statements
-A clippy::single_char_pattern -A clippy::replace_box -A clippy::while_let_loop
-A clippy::unnecessary_sort_by -A clippy::if_same_then_else`.
Дополнительные suppression относятся к unchanged WML/write/layout baseline;
лог `RC02-eastasia-face-clippy-msrv-final.log`. Rust 1.98 дополнительно сообщает
existing `chunks_exact_to_as_chunks` в public RTF test. Changed-file rustfmt
(`--edition 2021 --config skip_children=true`) и `git diff --check`: Pass.
`cargo fmt --all -- --check` выявляет existing formatting differences вне этого diff;
посторонние файлы не форматировались.

После восстановления fixture полный SVG suite с единственным явным исключением
maps_regular_postscript_names_to_the_same_bundled_faces завершился Pass (exit 0).
Команда: cargo +1.98.0 test --locked --no-fail-fast -p strict-ooxml-render-svg --
--skip maps_regular_postscript_names_to_the_same_bundled_faces.
Лог: target/rtf-import/RC02-eastasia-face-svg-final-tests.log.

### RC02: shared font resource contract — 2026-10-07

Добавлены FontResource/ResolvedFaceHandle: borrowed bundled или explicit shared bytes,
program SHA-256 + collection face index + sorted variation coordinates; validation
font/index/axis/range и mapping provenance. Legacy Copy face/source API сохранены.
Shaping caches используют program/instance identity; PDF collection/build разрешает
bundled face через общий cached resolver. Contract tests, cache collision regression,
полные SVG/PDF/public API suites (existing alias-test skip) и MSRV Clippy с existing
baseline suppressions проходят. RC02 остаётся InProgress: registry admission, owned
consumer pipeline, glyph policy и реальные CJK/Indic assets требуют следующих срезов.
Контракт, проверки и ограничения: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).

### RC02: bounded registry и owned consumer API — 2026-10-07

Реализован immutable FontAssetRegistry с typed pinned manifest, explicit shared
bytes и budgets; complete preflight предшествует hashing/parsing. Проверяются
hash/index/axes, metadata/duplicates, permission evidence и OS/2 embedding flags.
Lookup требует exact family/style binding, без system discovery или implicit
fallback. Owned resource metrics/coverage/advances и shape_resource используют
одни program bytes/variation location и существующий harfrust/cluster builder.
Missing CJK glyph возвращает DegradedMissingGlyph. Полные SVG/PDF/RTF suites:
418 Pass (existing alias-test skip); registry regressions 6 Pass; MSRV Clippy с
прежними baseline suppression и formatting проходят. Corpus gates в этом API-срезе
не повторялись. Registry ещё не подключён к RenderOptions/placement/SVG/PDF;
следующий срез — сохранить original request и owned handle до delivery/subset,
затем failure policy и свежий corpus run. RC02 остаётся InProgress.
Подробности: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).


### RC02: explicit assets through placement and SVG/PDF — 2026-10-07

Optional RenderOptions font_assets now preserves the selected original script-font
request and retains its registry in placed runs. SVG embeds exact program bytes with
safe resource-derived CSS names, explicit variations and no automatic optical/synthetic
instance changes. PDF collects and subsets exact program/index/all-coordinate keys;
owned programs survive options drop. TTC face 1 is verified in PDF and rejected
explicitly for current SVG delivery. Empty registries preserve established output.
Full SVG/PDF/RTF suites: 425 Pass with the previously recorded alias-test exclusion;
final SVG tests: 10 Pass. MSRV Clippy retains the existing suppressions. Fresh default
public corpus has 100/100 text/integrity/resource Pass and unchanged DOCX hashes;
Strict XSD and OPC have zero violations across 100 packages.
RC02 remains InProgress: runtime failure policy, positive licensed CJK/Indic geometry
and full visual acceptance remain pending. Registry admission is available in the
library API; the corpus CLI still exercises default bundled resources.
Current contract, limits and witness references: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).


Fresh default pagination: 62/100 Pass, 38 Fail. All 100 actual/reference PDF hashes
and page correspondences match RC02-eastasia-face. The prior pixel receipt is
carried forward by exact PDF identity: 3/100 documents Pass, 97 Fail; 423 pages,
6 Pass and 417 Fail. The existing release comparator SHA-256 matches that receipt.
This is not a new raster run or visual acceptance. Equivalence evidence:
`target/rtf-import/RC02-render-assets-pixel-equivalence.json`; fresh pagination:
`target/rtf-import/RC02-render-assets-visual-verified/results.json`.


## RC02 failure policy and licensed witnesses (2026-10-07)

Implemented bounded original-request reports and ReportAndContinue/Reject across
placement, SVG and PDF. Strict refusal survives zero detail capacity; SVG TTC
continuation omits unsupported text with a diagnostic. Renamed pinned OFL CJK SC
and Devanagari test subsets match independent native HarfBuzz geometry. CJK strict
SVG/PDF/RTF and Unicode extraction pass. Devanagari embeds but continuation CIDs
still produce replacement characters on extraction: it remains degraded and strict
rendering refuses it. Next: fix continuation extraction, review script/font profiles,
wire bounded CLI assets, and pass full visual corpus acceptance. RC02 InProgress.
Contract and limits: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).

436 tests/53 groups pass with the existing alias-test exclusion; MSRV Clippy passes
with unchanged baseline suppressions/warnings. Fresh corpus 100/100 text/integrity/
resource; XSD/OPC 100 packages, zero violations. All 100 default DOCX/PDF hashes
match the previous slice. Fresh pagination remains 62 Pass/38 Fail. Historical pixel
measurement remains 3 Pass/97 Fail documents and 6 Pass/417 Fail pages; no reraster.
Receipts: `target/rtf-import/RC02-font-policy-{corpus,visual}/results.json`,
`RC02-font-policy-{full-tests,clippy-final,xsd,opc}.log`.


### RC02: cluster ActualText — 2026-10-07

Implemented logical UTF-16 ActualText around multi-glyph clusters. Independent
MuPDF extraction now preserves the pinned Devanagari text; CJK also passes.
lopdf ignores ActualText and retains its raw continuation-CID extraction limitation.
436 tests/53 groups and MSRV Clippy pass with existing exclusions/suppressions.
Geometry and degraded complex-script status are retained. Corpus gates were not
rerun in this focused slice. Next: broader script/font profiles and complex-script
visual/placement acceptance, bounded CLI configuration, full visual corpus.
RC02 InProgress. Details: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).

### RC00: повторная интеграция master — 2026-10-08

Ветка `codex/rtf-import-integrated` повторно сведена с основной линией: `master`
`956948f` влит merge-коммитом `5cccef4` (родители `bf676d1` и `956948f`), база
прежняя — `ac51b687`. За время расхождения master получил never-crash линты,
editing/IdIndex, P07–P15 audit, table styles, charts/SmartArt и CC0-корпус;
RTF-очередь сохранила свою историю отдельной ветвью и не содержит `master`-путей.

Конфликты (8): `.github/workflows/ci.yml` (объединены RTF-шаг и fetch CC0),
`render-pdf/src/document.rs`, `render-pdf/src/font.rs`,
`render-svg/src/{layout/mod,paint/text,style,lib}.rs`, `write/src/parts.rs`.
В `paint/text.rs` и `layout/mod.rs` сохранён owned-aware `advance` (RTF), в
`document.rs` — `owned_origin_adjustments` вместе с overflow-safe счётчиком
master, в `parts.rs` принята master-версия `w:tentative` с `tentative_off`.

RTF parser/builder приведены к расширенной WML-модели: `Style` заполняет
`custom_style`, `auto_redefine`, `row`, `cell`, `conditions`; `Table` —
`grid_change: None`; autospacing стал `Option<bool>`; `Picture` — `nv_id`/`bw_mode`;
`BlipRef` — `cstate`. Значения выбраны совместимыми с прежним выводом
(`nv_id: None` сохраняет recorded `pic:cNvPr@id` loss, не меняя DOCX hashes).

Проверки после merge (Rust 1.98): `cargo +1.98.0 check --locked --workspace
--all-targets` и то же с `--all-features` — Pass; `cargo fmt --all -- --check` —
Pass. Тесты: `strict-ooxml-rtf` 30 Pass + 1 doc-test; `strict-ooxml-render-pdf`
и `strict-ooxml --features rtf,pdf` — Pass (включая `pdf.rs`, east-asia, licensed
CJK/Indic, ActualText); полный `strict-ooxml-render-svg` и `strict-ooxml-cli` с
единственным документированным `--skip
maps_regular_postscript_names_to_the_same_bundled_faces` — Pass без падений;
`strict-ooxml-write` — Pass. Corpus/visual/pixel/XSD/OPC gates в этом срезе не
перезапускались; статусы RC02+ и development-цифры не менялись. Это
интеграционная сверка, а не новый baseline совместимости.

### RC02: bounded CLI font assets — 2026-10-08

Закрыт пункт «CLI asset configuration» из FONT_RESOURCE_CONTRACT: `render` и
`to-pdf` принимают `--fonts <manifest.json>` (schema
`strictlib.font-assets.v1`). Manifest задаёт точные family/style bindings с
обязательным pinned `sha256`, путём, опциональными `face_index`/`variations`,
provenance/license и `embedding = embed-and-subset`. Читаются только указанные
файлы (без system discovery), чтение ограничено `FontRegistryLimits::default()`
до парсинга; hash mismatch, отсутствие attestation, malformed metadata или
превышение бюджета — твёрдая ошибка (exit 2) без записи вывода. Admission
делегируется общему `FontAssetRegistry`, поэтому OS/2-ограничения и дубликаты
binding проверяются идентично API. `write` отвергает `--fonts`. Registry
передаётся через `RenderOptions::font_assets`, так что SVG и PDF встраивают те
же байты программы, что и библиотечный путь; типы asset'ов реэкспортированы из
`strict-ooxml` под `svg`.

Проверки: 4 unit-теста loader'а (admission; hash/permission rejection; malformed
sha/schema; сверка witness-хешей с `fonts/witnesses/manifest.json`) и 1
process-тест, который импортирует RTF East Asian slot, встраивает pinned CJK
witness (`StrictAsset-` в SVG) и отвергает неверный hash с exit 2.
`cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings` —
Pass; при merge также исправлены RTF-участки `indexing_slicing`,
`if_same_then_else`, `items_after_statements`, ставшие ошибками из-за новых
crate-level deny из master. Corpus/visual/pixel gates не перезапускались: срез
добавляет конфигурацию, а не новое измерение корпуса. RC02 остаётся InProgress.

### RC02: reviewed profile и решение по extraction — 2026-10-08

Добавлен reviewed профиль [compatibility/RC02/profile.md](compatibility/RC02/profile.md):
Latin/Cyrillic/Greek — Supported (bundled faces, alias-regression через `pypdf`);
East Asian — Supported для конечного pinned CJK witness (native HarfBuzz geometry,
strict SVG/PDF/RTF, ActualText extraction); Devanagari — Degraded (geometry и
embedding проходят, strict отказывает на `font.script.degraded`); прочие complex
scripts — Unsupported; variable/collection faces — delivery Supported (SVG
отклоняет collections); missing request — Degraded по умолчанию и Reject в строгом
профиле. Каждая строка указывает независимое evidence и явные границы; конечный
witness не выдаётся за общую поддержку письменности.

Зафиксировано решение по reader compatibility: multi-glyph clusters несут
spec-compliant UTF-16 `/ActualText`; ActualText-aware readers (MuPDF/Adobe/PDFium)
извлекают точно (MuPDF-witness), а readers, игнорирующие ActualText (независимый
oracle `lopdf`), падают обратно на per-CID `ToUnicode` и дают replacement
characters. Это документированное ограничение reader'а, а не открытый дефект
рендерера; универсальная совместимость reader'ов не заявляется.

Два пункта приёмки переведены в **Blocked** с конкретным контрактом: полная
complex-script visual приёмка принадлежит RC03 shaping / RC04 metrics, а полный
corpus pagination/pixel gate требует poppler (`pdfinfo`/`pdftoppm`), pinned
comparator и WPS reference PDF (лежат в sibling worktree `rtf-import`). Ни то, ни
другое не является дефектом font resolution; измеренный 3/100 pixel Pass —
разрыв shaping/metrics. Font-инфраструктура RC02 (resolved face, registry, owned
shaping, exact SVG/PDF delivery, extraction, профиль, bounded CLI) завершена;
формальный `Verified` не выставлен, пока приёмка не снята внешним harness.

### RC03: shaping и bidi — 2026-10-08

Начат RC03. Второй shaper не создаётся: RTF использует существующий
harfrust/cluster pipeline. Добавлены три pinned OFL witness'а рядом с CJK и
Devanagari — Arabic, Tamil, Bengali (`strict-ooxml-render-svg/fonts/witnesses/`),
сгенерированные тем же `xtool/rtf/font_witnesses.py` (fontTools 4.60.1,
uharfbuzz 0.56.3); cjk.otf и devanagari.ttf воспроизводятся побайтно идентично,
manifest фиксирует source/fixture SHA-256, license и copyright.

`tests/font_witnesses.rs` теперь прогоняет пять скриптов и сверяет glyph id,
UTF-8 cluster, x/y advance, x/y offset и origin с независимым native HarfBuzz
TSV — все проходят. Отдельный witness проверяет: `unicode_cluster_map`
реконструирует логическую строку точно, каждый cluster — непустой и на
char-boundary, `unicode_x_positions_px` даёт ровно одну позицию на скаляр, а у
Arabic логический первый скаляр стоит правее визуального левого края (значит
произошла визуальная перестановка, а не LTR-накопление). Артефакты:
[compatibility/RC03/STATUS.json](compatibility/RC03/STATUS.json),
[tests.md](compatibility/RC03/tests.md),
[witnesses.json](compatibility/RC03/witnesses.json).

Открыто/Blocked: корпусные свидетели 007 Sanskrit, 010 Arabic, 011 Tamil,
018 Bengali требуют их реальных system faces (не забандлены, не лицензированы)
и RTF-источников из sibling worktree. Границы переноса закрыты witness'ом
`long_devanagari_token_breaks_at_cluster_boundaries`; PDF Unicode extraction
закрыт MuPDF-witness'ом для всех пяти скриптов
(`RC03-actualtext-extraction.json`); mixed LTR/RTL внутри одного run режется на
import'е по Unicode Bidirectional Algorithm (`unicode-bidi`) на
direction-homogeneous runs с `w:rtl` (тест import → write → reopen). Рендерный
line-level visual reordering mixed-direction runs в RTL-base параграфах переведён
в **Blocked** на bidi-aware независимый oracle (native HarfBuzz witness шейпит
одиночные буферы без UBA); без него корректность переупорядочивания не проверить.
RC03 остаётся InProgress.

### RC04: character geometry mapping — 2026-10-08

Начат RC04 (styles и текстовые метрики; общие metric-фиксы берутся из P5, RTF
очередь добавляет mapping и witnesses). Первый срез закрыл три ранее
отбрасываемых run/paragraph control'а и закрепил их import → Strict write →
reopen regression'ом (`strict-ooxml-rtf/tests/structure.rs`):

- `\charscalexN` → `w:rPr/w:w` (процент; `0..=600`, вне диапазона — явный Loss);
- `\expndN` → `w:rPr/w:spacing` (quarter-points → twips, `N*5`), `\expndtwN` →
  twips; переполнение — явный Loss;
- `\tlth` → `w:tab/@w:leader="heavy"`; `\tleq` и `\aexpnd` получают явный Loss
  (нет WML-эквивалента);
- `\plain` сбрасывает `scale`/`spacing` вместе со всем `RunProperties`.

Второй срез закрыл ещё отбрасываемые run-свойства (writer их уже умеет):
`\outl`/`\shad`/`\embo`/`\impr`/`\noproof`/`\snaptogrid` → соответствующие
`w:rPr` toggle; `\kernN` → `w:kern` (half-points, отрицательное — явный Loss);
`\positionN` и `\upN`/`\dnN` → знаковый `w:position`. Всё покрыто
import → Strict write → reopen regression'ом.

Артефакты: [compatibility/RC04/STATUS.json](compatibility/RC04/STATUS.json),
[tests.md](compatibility/RC04/tests.md). Проверки: RTF structure 34 + import 36
Pass; workspace clippy `-D warnings` (MSRV 1.92) и rustfmt Pass.

Добавлены keep/widow/page-break witness (`\keep`/`\keepn`/`\widctlpar`/
`\nowidctlpar`/`\pagebb`) и probe `xtool/rtf/effective_style_probe.py` против
WPS reference facts для 019/039 (receipts `RC04-effective-style-{019,039}.json`):
все geometry-relevant прямые свойства присутствуют; WPS-only имена — это
writer defaults (paragraph-mark rPr, szCs mirror, docDefaults spacing/kern), а не
пропущенные mapping. Section defaults закрыты существующим
section-reset round-trip тестом.

Открыто/Blocked: resolved-cascade effective-style equality (нужен styles.xml
resolver; direct-diff сделан) и corpus tab/leader/keep/widow/pixel geometry на
019/039, 052/076 (нужен rasterizer). RC04 остаётся InProgress.

### Разбор блокеров — 2026-10-08

1. **Rasterizer.** poppler 26.09.0 provisioned в
   `target/rtf-import/poppler/`; WPS references — в sibling worktree. Свежий
   прогон на merged HEAD (CLI `3e40d368…`): import 100/100 text Pass,
   pagination **62 Pass / 38 Fail**, pixels **3/100 документов** (6/423 страниц).
   Совпадает с прежним dev-замером; регрессий нет. Это снимает «нет tooling», но
   не снимает 97 pixel failures — это shaping/metrics (RC03/RC04), а не fonts.
2. **Corpus script faces (007/010/011/018).** Точные system faces не
   лицензируются; проверен substitute-путь: 007 с `--fonts` (requested
   `Sanskrit 2003` → pinned Devanagari witness) рендерится (90 страниц, 2
   embedded face), но честно Degraded (`font.glyph.missing` на конечном subset +
   `font.script.degraded`). Substitute не даёт exact-face покрытия.
3. **styles resolver.** Добавлен независимый
   `xtool/rtf/effective_style_resolver.py`
   (`docDefaults → pStyle chain → rStyle → direct`), receipts
   `RC04-resolved-{019,039}.json`. После нормализации Strict/Transitional имён и
   нулевого auto-spacing остаются только explicit-defaults-vs-inherited
   расхождения (наш writer пишет docDefaults явно, WPS наследует); effective-value
   mismatch на 019/039 не найден.
4. **Bidi oracle.** Добавлен независимый `xtool/rtf/bidi_oracle.py`
   (python-bidi 0.6.11 + uharfbuzz 0.56.3), receipt
   `RC03-bidi-oracle-arabic.json`: UBA visual string + LTR glyph
   origins/advances. Однако python-bidi 0.6 отдаёт только переупорядоченную
   строку, без per-character levels, поэтому geometry-oracle под стратегию
   «шейпить каждый level-run» не строится напрямую. RTF-очередь отдаёт
   renderer'у direction-homogeneous runs (import-сплит), так что renderer-level
   reordering — вне scope RTF mapping и остаётся отдельным renderer-пунктом
   (Blocked на per-level-run oracle).

### Триаж 97 pixel failures — 2026-10-09

Свежий pixel-receipt (`target/rtf-import/RC02-merge-pixels`) впервые несёт
per-page SSIM и structural measurement. По нему 97 `Fail` разбиты на доказуемые,
непересекающиеся gate-механизмы. Полный разбор, receipt и команда воспроизведения
— [compatibility/PIXEL_TRIAGE.md](compatibility/PIXEL_TRIAGE.md); классификатор
`xtool/rtf/pixel_triage.py` закреплён `xtool/rtf/test_pixel_triage.py`.

Итог: 3 Pass-контроля (040/043/046), 38 документов падают на несовпадении числа
страниц (19 «больше», 19 «меньше»; 22 рендерятся Clean), 59 — на совпавшем числе
страниц (423 страницы: 6 Pass, 417 Fail). По механизмам страниц: `line_metrics`
402, `blank_reference` 5, `glyph_appearance` 3, `extra_ink` 3, `missing_ink` 2,
`content_position` 2. Документальные primary-классы: `line_metrics` 53,
`glyph_appearance` 3, `blank_reference` 2, `missing_ink` 1 (specific-механизмы
внутри `line_metrics`-документов сохранены как `secondary`).

Доказано: 72 из 97 падающих документов рендерятся `Clean`, и 39 из 44
документов с чисто вертикальными механизмами отказа (row correlation / vertical
shift / extent) не имеют ни одного `pdf.font.*` diagnostic — значит
доминирующий отказ это line/pitch metrics (RC04), а не отсутствие faces. Отдельно
доказанные точечные классы: 035 (SimSun, 141 no-glyph → пустая страница),
5 blank-reference страниц (из них 036/048/059 — SSIM ≈ 1.0, то есть
gate-артефакт пустой страницы), 019/037/071 (structure Ok, SSIM 0.91–0.957 =
substitute face), 032/087/099 (extra ink). Владелец конкретной RTF-фичи для
доминирующего класса остаётся гипотезой до fixture; это единственная часть
триажа, не помеченная как confirmed.

### RC04: три доказанных geometry-фикса — 2026-10-09

Триаж указал доминирующий класс `line_metrics`; дальнейшие свидетели дали три
независимых доказанных дефекта, каждый закреплён import → Strict write → reopen
тестом:

1. **Отсутствующий `\deff`.** По RTF 1.9.1 default font — это font 0 (первый в
   таблице), но при отсутствии `\deff` он не попадал в
   `w:docDefaults/w:rPrDefault/w:rPr/w:rFonts`, поэтому параграфы без `\f`
   (paragraph mark, пустая строка) мерились fallback-лицом рендерера. Фикс в
   `strict-ooxml-rtf/src/parser.rs`; свидетель 013 COPYING: межстрочный разрыв
   пустого параграфа 28.4 → 27.6pt (совпал с WPS).
2. **Paragraph mark пустого параграфа.** Пустая строка получала 12pt из
   docDefaults вместо body-size (10pt), сдвигая весь остаток страницы. Теперь
   `w:pPr/w:rPr` несёт ambient run-форматирование; свидетель 054 Hotyn2: разрыв
   пустого параграфа 25.3 → 23.0pt.
3. **Script-qualified fonts.** Имена вида `Times New Roman Cyr`, `Arial CE`,
   `Calibri (Hebrew)`, `Cambria Math Cyr` не нормализовались и падали в wildcard
   Carlito. `render-svg::map_family` теперь снимает script-квалификатор до
   маппинга; свидетель 054 Hotyn2: pitch 12.2 → 11.5pt (Arimo).

Измерения (receipts `target/rtf-import/RC04-parmark-*`): import text/resource
100/100; Strict XSD 100/100 без нарушений; OPC 100 пакетов без нарушений;
pagination **62 → 65/100**; pixels **3 → 6/100 документов** и **6 → 22/458
страниц**, регрессий 0. RTF tests: structure 37 + import 36 + 1 doc-test.

Остаток после этих фиксов разобран дальше: доминирующий симптом
`row_correlation` — это не вертикальный pitch, а **advance-driven reflow**.
Свидетель 013 page 2: WPS укладывает `…тоже их получат.` до правого поля
(x1=522.9), а наши Arimo-advances переносят `получат.` на следующую строку
(x1=474.4), сдвигая весь остаток страницы на строку. Пиксельный компаратор
подтверждает, что вертикальный профиль в норме (`top +1px, bottom +0px, shift
(0,0)`), т.е. симптом назван по нарушенной границе, а не по горизонтальной
причине. RTF-шрифт `Helvetica` (`fcharset0`), WPS подставляет кириллическое лицо
с другими advances. Это предел substitute-face fidelity (RC02/RC03), а не
пропущенный RTF-mapping. Отдельно — WPS-квирк leading-whitespace (013 page 1).
Т.е. RTF-mapping часть RC04 закрыта; остаточный pixel-gap лежит в RC02/RC03
(exact faces).

Дополнительно закрыт диагностический пробел: RTF 1.0 инденты `\lin`/`\rin`/`\fin`
(corpus 27–32 документа) теперь маппятся как `\li`/`\ri`/`\fi`; геометрия не
изменилась (современные алиасы уже были рядом), но ledger перестаёт выдавать их
как Loss, и 015 получает отступ.

Следующий pagination-лид — 072 `(+37 страниц, Clean, без font/image diagnostics)`:
свидетель page 2 — ref 48 строк против наших 35, а ref page 2 начинается на
y≈35.5 (выше поля 72pt). У документа 40 `\sect`; это указывает на RC08
(секции/margins) или spacing, а не на fonts. Проверять отдельным свидетелем.

### RC08: секционные заголовки и pitch — 2026-10-09

Начата RC08 на свидетеле 072. Два доказанных дефекта закрыты (детали и receipt'ы
— [compatibility/RC08/tests.md](compatibility/RC08/tests.md)):

1. **Потеря секционных заголовков.** WPS пишет сброс перед break
   (`\sectd\sect`), Word — после (`\sect\sectd`). Немедленный сброс стирал
   заголовок/геометрию секции до её коммита; 072 писал 1 `w:headerReference` из
   40. Сброс отложен до коммита следующего `\sect` (для `\sectd\sect`) либо до
   первого не-`\sect` токена (для `\sect\sectd`); 072 теперь пишет 40 ссылок и
   заголовок виден на страницах секции.
2. **Pitch секционного body.** `\sl280\slmult1` даёт ref 16.1pt = 1.15em×1.1667
   (Arimo), а wildcard Carlito — 17.1pt. Verdana/Tahoma/HelveticaNeue теперь
   маппятся в Arimo; pitch совпал, страниц 641→638.

Остаток 072 (+34 страницы) разобран через общий layout и оказался **WPS
source-format inconsistency**, а не mapping-багом. Измерено по неизменяемым WPS
reference: WPS на RTF-пути **складывает** `before+after` (072 heading→heading
38.25pt ≈ sum 36), а на DOCX-пути берёт **max** (06-strict-math-display
body→Heading2 14.74pt ≈ max 14). Рендерер использует `max` (`paginate.rs`) —
именно это требует DOCX-baseline и весь SVG refs. Наивный `max→sum` проверен и
**откачен** (сдвинул `06-strict-math-display` extent 5→37 и уронил
`matches_wps_references`); имитировать RTF-сумму через раздувание `w:before` на
импорте нельзя — это искажает документ (DOCX-потребитель применит max и
пере-разнесёт). Поэтому остаток 072 записан как **WPS RTF-path quirk** (не
имитируется, не блокер supported profile). `\chpgn`/`\sectnum` теперь dynamic
PAGE/SECTION fields (заголовок 072 читается `Ruth (Page 2)`), на pagination/pixels
не влияет. Gates после среза: text/resource 100/100, XSD/OPC 0 нарушений,
pagination 65/100, pixels 6/100 и 22/458 без регрессий.

Далее закрыт ещё один content-loss: bookmarks
`{\*\bkmkstart Name}`/`{\*\bkmkend Name}` (616 вхождений в 7 документах, ранее
дропались как unknown destination) теперь пишутся как paired
`w:bookmarkStart`/`w:bookmarkEnd` с общим decimal `w:id` и `w:name`; якоря для
hyperlink/`REF` сохранены. Zero-width — pagination/pixels не изменились.

Ещё закрыт рендер first/even-регионов: `\titlepg` → `w:titlePg` секции, а
even-регион `\headerl`/`\footerl` включает `w:evenAndOddHeaders` (002 пишет 7
`w:titlePg` и even/default header refs — раньше и то, и другое терялось).
Заголовки в поле, поэтому pagination (65/100) и pixels (6/100, 22/458) не
изменились.

Открыты по RC08: columns (`\cols`/`\colno`/`\colw`/`\colsx`/`\colsr` — нужен
section-column renderer, shared), note placement/numbering, multi-paragraph field
block-result (shared model) и custom marks. `field-instruction-control:*`
(~40k) — форматирование внутри `\fldinst`, корректно не попадает в instruction,
но помечено Loss; вопрос реклассификации в явный ignored-formatting Info открыт
(blanket rename запрещён планом).

Ещё закрыт mapping секционных колонок: `\cols` → `w:cols/@num`, `\colsx` →
`@space`, `\colno`/`\colw`/`\colsr` → per-column `w:col` (explicit `\colw`
снимает `equalWidth`). 074/092 теперь несут свои 2-column определения и
round-trip'ятся. Shared renderer по-прежнему рисует одну колонку — column-layout
остаётся отдельным shared-build; pagination (65/100) и pixels (6/100, 22/458)
не изменились. Также `\rtlsect`/`\ltrsect` теперь задают section bidi
(`\rtlpar`/`\ltrpar` были закрыты раньше).

Финальные receipts RC08-среза: `target/rtf-import/RC08-final-{corpus,visual,pixels}`.

### RC08: нумерация и размещение примечаний — 2026-10-10

Закрыт mapping нумерации/размещения сносок и концевых примечаний. Ранее весь
семейство control-word'ов падало в generic `control:*` Loss, поэтому
`w:footnotePr`/`w:endnotePr` не писались. Теперь:

- document-level `\ftn*`/`\enddoc` и `\aftn*`/`\aenddoc` → `settings.xml`
  (`w:footnotePr`/`w:endnotePr`);
- section-level `\sftn*`/`\saftn*` (формы внутри `\sectd`, как пишет WPS) →
  `w:sectPr` текущей секции, без утечки в документные settings.

Маппятся (по таблице RTF 1.5 «Footnotes and Endnotes»): формат номера
(`\ftnnar`/`alc`/`auc`/`rlc`/`ruc`/`chi` и endnote-двойники → `w:numFmt`),
начальный номер (`\ftnstart`/`\aftnstart` → `w:numStart`), режим рестарта
(`\ftnrstcont`/`\aftnrstcont` → `continuous`, `\ftnrestart`/`\aftnrestart` →
`eachSect`, `\ftnrstpg` → `eachPage`) и позиция (`\ftnbj` → `pageBottom`,
`\ftntj` → `beneathText`, `\endnotes`/`\aendnotes` → `sectEnd`,
`\enddoc`/`\aenddoc` → `docEnd`).

Не маппится сознательно: содержимое сепараторов (`\ftnsep`/`\ftnsepc`/`\ftncn`
и endnote-двойники) остаётся `destination:*` Loss — общий model не имеет
`RunContent::Separator` (reader сейчас помечает `w:separator` Supported и теряет
его), поэтому пустая note-заглушка была бы скрытым фрагментом, а не контрактом.
`\aftnbj`/`\aftntj` (нет значения `ST_EdnPos`) и `\ftnlytwnine` (нет
Strict-эквивалента) остаются явным Loss.

Проверки: RTF structure 48 + import 36 + 1 doc-test Pass; public/PDF suite
`--features rtf,pdf` Pass; write suite Pass. Corpus receipts
`target/rtf-import/RC08-notes-{corpus,visual}`: import text/resource 100/100;
import losses 171 606 → 170 981 (−625 на 29 документах,
`\sftnbj`/`\ftnbj`/`\ftnnar`/`\aftnnrlc`/`\ftnstart`/`\ftnrstcont`/`\aenddoc`);
`strict-ooxml check` на всех 100 выходах — 100/100 `ok: strict`, 0 нарушений.
Pagination не изменилась: все 100 output переcобраны в PDF, page counts
побитово равны RC08-final; rendered PDF побайтово идентичны baseline, поэтому
pixel-замер (6/100 документов, 22/458 страниц) переносится по идентичности, без
нового растеризатора. Оговорка: WPS reference PDF больше не сохраняются под
sibling-oracle, поэтому pagination/pixel-гейты подтверждены эквивалентностью
PDF-идентичности, а не свежим raster-сравнением. RC08 остаётся InProgress:
column-layout, сепараторы, block-result fields и custom marks открыты.

### RC08: сепараторы, реклассификация полей и contextualSpacing — 2026-10-10

Три среза закрыты и закоммичены (`b5f6cfd`, `4520a46`, `06f4eba`):

1. **Сепараторы примечаний.** Общий model получил `RunContent::Separator`/
   `ContinuationSeparator`; reader больше не теряет `w:separator`/
   `w:continuationSeparator` (раньше помечал Supported и выбрасывал), writer их
   пишет. `\ftnsep`/`\ftnsepc` и endnote-двойники становятся reserved-описаниями
   `id -1/0` плюс ссылками `w:footnotePr`/`w:endnotePr`. Разбор тела сепаратора
   вскрывает форматирующие контролы внутри него: ledger +152 на 6 документах
   (честный per-control учёт вместо одиночного `destination:*`). Вывод DOCX,
   page counts и PDF побайтово не изменились. `\ftncn`/`\aftncn` остаются Loss.
2. **Реклассификация `field-instruction-control`.** Курируемый набор
   символьного форматирования внутри `\fldinst` (font/script/lang/цвет/тоглы/
   размер) → явный Info; неизвестный контрол остаётся Loss (blanket rename
   запрещён). Corpus losses 170 981 → 137 380 (−33 753).
3. **contextualSpacing.** `\contextualspace` → `w:contextualSpacing`; paginator
   подавляет before/after между соседними одно-стилевыми параграфами, если
   включено с любой стороны. Ни корпус, ни WPS-референс его не используют —
   pagination/pixels не изменились.

Проверки: RTF structure 52 + import 36 + 1 doc-test; render-svg (существующий
alias-skip) и public/PDF suite (`rtf,svg,pdf`) Pass; clippy `-D warnings` (MSRV
1.92) и changed-file rustfmt Pass. Финал: `RC08-acd-corpus`/`RC08-acd-visual`:
text/resource 100/100; `check` 100/100 `ok: strict`; все 100 page counts равны
RC08-final, все 100 PDF побайтово идентичны (pixel-замер переносится по
идентичности). Открыто: column-layout, multi-paragraph field block, custom
marks — крупные shared-build задачи.

### RC05/RC06: списки и таблицы — 2026-10-10

Карточки заведены: [compatibility/RC05](compatibility/RC05/STATUS.json) и
[compatibility/RC06](compatibility/RC06/STATUS.json). Срезы (`23f0d03`,
`e453e24`, `72e7d05`):

- **RC05.** Легаси/совместимые list-конструкции перестали быть Loss:
  `{\listtext}`/`{\pntext}` (кэшированный текст маркера, регенерируется из list
  table — иначе двойной маркер), `{\*\levelmarker}` (альтернативный маркер,
  уступает `\leveltext`), `\jclisttab`/`\levelspace`/`\levelindent`/
  `\leveltemplateid` (Word 7 / list-template compat) → Info. Уже
  маппились `\list`/`\listlevel`/`\levelnfc`/`\levelstartat`/`\levelfollow`/
  `\leveltext` → `w:abstractNum`/`w:lvl` и `\listoverride`/`\ls` → `w:num`;
  отрицательный `\listid` сохраняется как source identity. Открыто: override
  levels (`\lfolevel`) — model/renderer уже держат `w:lvlOverride`, нужен только
  parser (witness 021).
- **RC06.** `\tbllk*` → `w:tblLook` (firstRow/lastRow/firstColumn/lastColumn/
  noVBand), `\rtlrow`/`\ltrrow` → `w:bidiVisual`; char-grid/growth/band compat
  (`\cgrid`, `\snaptogridincell`, `\nocxsptable`, `\nogrowautofit`,
  `\usenormstyforlist`, `\irow`, `\irowband`, `\lastrow`) → Info. Уже
  маппились grid/merge/widths/header rows; nested tables — явный Reject.
  Открыто: **бордеры и shading** (~35k Loss; нужен `Borders` между/bar для
  paragraph `\brdrbar`/`\brdrbtw` и state-machine стилей `<edge> <style>
  \brdrw? \brdrcf?`), cell shading/diagonal, `\tblind`.

Проверки: RTF structure 55 + import 36 + 1 doc-test Pass; clippy `-D warnings`
(MSRV 1.92) и changed-file rustfmt Pass. Финал `RC0506-corpus`/`RC0506-visual`:
text/resource 100/100; `check` 100/100 `ok: strict`; import losses
137 380 → 127 228 (−10 152); все 100 page counts равны RC08-final, все 100 PDF
побайтово идентичны (pixel-замер переносится по идентичности).

### RC08: shared column layout — 2026-10-11

Закрыт крупнейший оставшийся shared-layout пункт RC08: `w:cols` теперь влияет на
раскладку, а не только round-trip'ится. `layout::column_layout` строит
per-column `(left, width)` в px: equal-width с `w:cols/@space` либо explicit
`w:col/@w`/`@space` (необъявленная ширина в explicit-наборе — равная доля
остатка; единицы не угадываются). Paginator переносит блок, не влезший в
колонку, в **следующую колонку на той же странице**; новая страница — только
когда колонки полны. Page/section-break сбрасывает на первую колонку, а
continuous-секция несёт колонки вместе с отложенной геометрией. Элементы
следующей колонки сдвигаются на column-offset, поэтому параграф, продолжившийся
из колонки в колонку, сохраняет wrap. Single-column — fast-path: пустой список
колонок, нулевые offsets, старая раскладка byte-identical. `w:br
w:type="column"` → `Flow::ColumnBreak`; endnotes идут на всю страницу.

Проверки: `f14_columns.rs` (column break, overflow, fast-path); suite
`strict-ooxml-render-svg` Pass (кроме pre-existing `font::family`
styled-PostScript теста, к RC08 не относится); clippy `-D warnings` и fmt Pass.
Corpus `target/rtf-import/RC08-columns-layout`: pagination **65/100 → 66/100**.
Единственное изменение — 092_SurviveTheTyrant (21 → 20 стр., теперь равно WPS
reference); все остальные page counts равны `RC06-final3`, регрессий нет.
Witness 074 рисует обе колонки (left 76 px / right 425 px при 96 dpi), но
остаётся 4 стр. против ref 3: его 2-column секция — continuous-break, а
continuous-break по-прежнему откладывает геометрию и колонки на следующую
страницу. Открыто: same-page continuous geometry и per-column re-wrap для
неравных колонок.

### RC08: block-result fields — 2026-10-11

`{\field ...}` с block-результатом (TOC и подобные) раньше складывался в тело с
`field:multi-paragraph-instruction-dropped` Loss: shared model держал только
inline-поля, поэтому терялись и scope поля, и границы кэшированных параграфов.
Теперь такой `\field` (как и `\flddirty`) пишется уже поддержанной
complex marker-формой: `w:fldChar begin` + `w:instrText` + `w:fldChar separate`
+ кэшированные result-параграфы (со своими props) + `w:fldChar end`, разложенной
по границам параграфов. Single-paragraph актуальный результат остаётся
`w:fldSimple`. `\flddirty` ставит `w:dirty` на begin-маркер; `\fldedit` и legacy
`{\*\datafield}` → явный Info. Renderer уже трекал `fldChar`/`instrText` и
пропускает instruction, поэтому картинка не изменилась.

Проверки: `structure.rs::{multi_paragraph_field_becomes_the_complex_marker_form,
dirty_field_marker_and_compat_controls_are_not_losses}`; suite
`strict-ooxml-rtf` (68 + 36 + doc) Pass; clippy `-D warnings` и fmt Pass.
Corpus `target/rtf-import/rc08-field-verify2`: 014 losses 830 → 796, весь корпус
83671 → 83010; `check` 014 `ok: strict`; все 100 page counts равны столбцовой
baseline (регрессий нет). Открыто по RC08: custom marks и `\ftncn`/`\aftncn`.

### RC08: custom marks и continuation notice — 2026-10-11

Оба класса в корпусе не наблюдаются (0 вхождений), поэтому закрыты как явные
неподдержанные правила, а не спекулятивно реализованы. `w:customMarkFollows`
reader раньше ронял молча; теперь ссылка сохраняется, а
`w:footnoteReference`/`w:endnoteReference` помечается `Partial` с явным
сообщением (глиф-ран custom-марки не различается в render).
`notes.rs::custom_mark_follows_is_partial_not_silent` покрывает. `\ftncn`/
`\aftncn` (continuation notice, не `\ftnsepc`) — Strict-элемента нет, остаётся
явным `destination:ftncn` Loss. RC08 после этого: колонки и field block сделаны,
остаётся только same-page continuous geometry.

Далее по порядку — RC04.

### RC09: revision ids и legacy layout-контролы — 2026-10-11

Ledger корпуса доминировали два семейства, чья bare-форма равна Strict-дефолту
(маппинг записал бы ничего), поэтому честная классификация — именованный Info,
а не Loss: ревизионные save-ID `\insrsid`/`\charrsid`/`\pararsid`
(`rsid-compat`; невизуальные merge-метаданные) и Word 6/7 + Asian layout-контролы
`\faauto`/`\adjustright`/`\aspalpha`/`\aspnum` (`word6-compat`; `w:textAlignment=
auto`, `w:adjustRightInd` on, `w:autoSpaceDE`/`autoSpaceDN` on) плюс `\hyphpar`/
`\wrapdefault`/`\sectdefaultcl`/`\linex`/`\lnbrkrule`/`\partightenfactor`
(`paragraph-compat`). Non-default формы (`\hyphpar0`, `\adjustright0`) остаются
явным `control:*` Loss — в paragraph model нет `w:suppressAutoHyphens`/
`w:adjustRightInd`.

Проверки: `structure.rs::{legacy_layout_and_revision_controls_are_info_not_losses,
non_default_legacy_layout_controls_stay_losses}`; suite `strict-ooxml-rtf` Pass;
clippy/fmt Pass. Corpus `target/rtf-import/rc09-triage`: import losses
**83010 → 11394** (−71616). Изменение только severity, поэтому DOCX байт-идентичен
(SHA-256 совпал на 014/074/092) — pagination и pixels без изменений. Карточка
[compatibility/RC09](compatibility/RC09/STATUS.json).

### RC09: slot-маппинги и метаданные — 2026-10-11

Продолжение triage. Классы, у которых есть реальный WML-слот, смаплены, а те, у
которых слота нет, классифицированы по независимому WPS-оракулу
(`RC05-wps-full/oracle/files/`):

- char: `\kerning`→`w:kern` (033/073/099 reference), `\ulc`→`w:u/@w:color`
  (028 reference `<w:u w:val="single" w:color="0000FF"/>`), `\cb`→run `w:shd`
  (как `\chcbpat`; WPS непоследователен, но семантика та же).
- table: `\trpadd{t,b,l,r}`/`\trpaddf*`→`w:tblCellMar`, `\trrh`→`w:trHeight`
  (negative = `exact`), `\trqr`/`\trql`/`\trqc`→`w:trPr/w:jc`,
  `\trautofit`→`w:tblLayout` (все — 008 reference).
- paragraph/settings: `\pardeftab`/`\deftab`→`w:defaultTabStop` (028/032),
  `{\*\panose}`→`w:panose1` (002 fontTable).
- named Info (reference пишет пусто): `\hyphpar0`/`\noline`, `\nooverflow`/`\nocwrap`,
  `\cltxlrtb`, `\hres`/`\chhres`, `\qk`/`\qnatural`, `\animtext`,
  `\trftsWidth*`/`\trwWidth*`; метаданные-дестинации `{\*\falt}`/`{\*\hyphen}`/
  `{\*\pnseclvl}`/`{\*\rsidtbl}`/`{\*\revtbl}`/`{\*\latentstyles}`/
  `{\*\expandedcolortbl}`/`{\*\xmlnstbl}`/`{\*\pgptbl}`/`{\*\pgdsctbl}`/
  `{\*\wgrffmtfilter}`/`{\*\userprops}`/`{\*\fname}`/`{\*\generator}`;
  settings/cocoa/view/drawing-grid/math/rsid-семейства, `\widowctrl`, `\additive`,
  `\pardirnatural`, `\deflang*`.

Losses **11394 → 4515**. Severity-only срезы дали SHA-256-идентичный DOCX;
pagination 66/100 без изменения номеров страниц; `check` 100/100. Тесты —
`structure.rs::{kerning_underline_colour_and_character_background_map_to_run_props,
table_padding_row_height_alignment_and_layout_map,
default_tab_stop_and_compat_controls_map_and_classify,
font_panose_maps_and_metadata_destinations_are_info,
document_settings_and_compat_controls_are_info}`. Receipts —
`target/rtf-import/rc09-slice5/`.

Остаток и почему не закрыт: `wps:leading-group-line-ending` (наш WPS-профиль
сохраняет байт как LF, WPS пишет `&#10;` — тексты расходятся, поэтому честный Loss);
`\ts` table styles (нет `w:tblStyle`-модели); picture crop `\piccrop*`/`{\*\picprop}`
(нужен `a:srcRect`); content-классы (comments/revisions/textboxes/OLE/metadata);
unknown LCID закреплены тестами; хвост низкоуверенных флагов (`\newtblstyruls`,
`\sectlinegrid`, `\horzdoc`, `\stshf*`, `\clmar*`, `\gaph`, `\fet`, shape-stroke).

### RC04: resolver-probe (заметка) — 2026-10-11

Проверен `xtool/rtf/effective_style_resolver.py` на текущем выходе 019/039 против
WPS `reference.docx` (лежат в `RC05-wps-full/oracle/files/`). Остаток — не mapping,
а корректность самого probe: `resolve` заменял свойство целиком, а не по
атрибутам, поэтому унаследованный `rFonts` при direct `<w:rFonts w:hint>` читался
как `{}`; нужна и нормализация absent-vs-default. **Исправлено 2026-10-11:**
resolver накладывает `w:rFonts` по атрибутам через всю цепочку (`overlay`), а
явный Strict-дефолт (`jc=start`, `bidi=false`, `widowControl=true`, нулевой `ind`,
`u=none`, `color=000000`) равён отсутствию; заодно исправлено падение на
`docDefaults` без `pPrDefault`/`rPrDefault`. 019: 133 → 20 диффов (остаток —
genuine WPS-quirk `\sl276` строки 1 плюс прямой `\f2`-шрифт против docDefaults);
039: 18 диффов, все — слот `w:cs` (наш Helvetica vs Times New Roman). Тесты —
`xtool/rtf/test_effective_style_resolver.py`. Pixel-часть RC04 по-прежнему
заблокирована на faces (RC02/RC03).

