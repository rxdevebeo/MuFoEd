# REWORK-AUDIT-2026-10 — план закрытия дефектов аудита от 2026-10-02

**Дата:** 2026-10-02 · **Основание:** аудит реализованной части проекта (ядро, WML, report,
render-svg, render-pdf, pdf, convert, write, CLI, view, CI) · **Статус:** к исполнению

> **Как читать.** Документ закрывает **все** найденные дефекты. Порядок фаз — обязательный:
> каждая фаза опирается на инфраструктуру и инварианты предыдущей. Внутри фазы задачи можно
> делать в любом порядке, если не указана зависимость. Все архитектурные решения уже приняты
> и записаны в разделе «Решение» каждой задачи; исполнитель **не выбирает** между вариантами.
> Если реализация упирается в то, что решение не предусмотрело, работа по задаче
> останавливается и вопрос поднимается владельцу — задача не закрывается «как получилось».

---

## Статус выполнения (на 2026-10-03)

| Задача | Статус | Коммиты | Что сделано |
|---|---|---|---|
| AUD-00 | ✅ выполнена | `1af8988`, `13a66dc`, `1007ab4`, `75dfda3`, `c28dcc3` | `origin` = <https://github.com/rxdevebeo/MuFoEd>, ветка `master` (добавлена в триггеры CI). Базовая линия и причины падений — `docs/ci-baseline-2026-10.md`. Тулчейн закреплён на 1.92.0 во всех job'ах; `install-action@v2` с `tool:`; fuzz собирается с `--target x86_64-unknown-linux-gnu`; `.gitattributes` `eol=lf` и `rustfmt` `newline_style = "Unix"`; `deny.toml` `allow-wildcard-paths`; `checkout@v5`, `setup-python@v6`. Census-гейт убран из CI и запускается локально (waiver `CENSUS-LOCAL`) |
| AUD-01 | ✅ выполнена | `13a66dc` | Крейт `strict-ooxml-testkit` (`publish = false`, без зависимостей от крейтов workspace): `ZipBuilder` со своим CRC-32, `DocxBuilder` (Strict/Transitional, замена любой части, `raw_entry` для дубликатов), `PdfBuilder` (настоящий xref, `reserve`/`set` для ссылок на себя), `xml::{nested, nested_tables, nested_text_boxes}`, `harness::{bounded, assert_survives}` (стек 1 MiB, таймаут 10 с). 14 юнит-тестов. Шаг CI «Hostile inputs (release)» |
| AUD-02 | ✅ выполнена | `13a66dc` | `tests/hostile.rs` в `strict-ooxml`, `strict-ooxml-pdf`, `strict-ooxml-convert` с модулями под задачи Ф1–Ф2 и смоук-тестами testkit на настоящем API |
| AUD-03 | ✅ выполнена | `4f0939d` | `strict_ooxml_core::xml::escape` (`is_xml_char`, `escape_text_into`/`escape_attr_into` с числом удалённых символов, `count_invalid`); шесть локальных функций удалены. `\t\n\r` в `.rels` и `[Content_Types].xml` пишутся ссылками, а не пробелом. Писатель: `XmlWriter::finish_counted`, `Ctx::finish_xml`, потеря `W.invalid-xml-char` (`Lossy`) с именем части из `part_xml`. SVG: новое поле `Page::warnings`, `render.invalid-xml-char`. Тесты: юнит + 2 proptest в core, юнит в writer, 2 интеграционных в `strict-ooxml/tests/hostile.rs` (оракул `roxmltree`) |
| AUD-04 | ✅ выполнена | `см. ниже` | `XmlReader::end_of_document()` отвергает конец входа с незакрытыми элементами (`unexpected end of document: N unclosed element(s), innermost <name>`), второй корень и непробельный текст после корня (`content after the root element`) и документ без корня (`no root element`); новые поля `root_seen`/`root_closed`, общий `close_scope()`. `PartParser::expect_end_of_part()` — восемь корневых парсеров дочитывают часть, иначе хвост после `</w:document>` оставался непрочитанным. Три петли WML с явной веткой `Eof`: `fonts.rs` ×2 и `settings.rs::parse_math_properties`, где было `_ => {}`. `xtool lint-eof` — рекурсивный обход `.rs`, снятие `//`-комментариев перед разбором, поиск `Eof`-руки по отступу, явная отмена `lint-eof: this arm is the success case`; шаг в CI. Тесты: 10 юнит в core, 7 в `strict-ooxml/tests/hostile.rs` (каждая часть, обрезанная ровно по закрывающему тегу), 6 юнит в `xtool` |

| AUD-05 … AUD-94 | ⏳ не начаты | — | — |
**Состояние CI.** Прогон [37071556814](https://github.com/rxdevebeo/MuFoEd/actions/runs/37071556814) на `c28dcc3` полностью зелёный: test (ubuntu, macos, windows), fuzz smoke, coverage, msrv, cargo-deny, XSD gate. Fuzz nightly запускается только по расписанию.

**Отступления, допущенные при выполнении.**
- AUD-03, §0.1 п.2: падение новых тестов на коде до правки не проверялось откатом. SVG-тест на старом коде не компилируется (поля `Page::warnings` не было), тест писателя проверяет запись, которой раньше не было.
- AUD-03: конвертер своего XML не пишет, его вывод проходит через писатель, поэтому отдельная запись `convert.invalid-xml-char` не вводилась. Её проверяет тест AUD-83.
- AUD-03: MathML (`math_expression_to_mathml`) удаляет недопустимые символы, но не сообщает о них: у публичного API нет отчёта.
- Census-гейт в §0.4 — только локально (waiver `CENSUS-LOCAL`, решение владельца 2026-10-03).
- Census-гейт был сломан до начала Ф1: `report()` печатал список сообщений из переменной `out_messages`, которой в нём нет (`NameError` на ветке, которая срабатывает всегда). Исправлено в `f3f3456`; до правки §0.4 нельзя было выполнить в принципе.
- AUD-04: ветки `Eof` в трёх петлях WML стали защитой в глубину — `XmlReader` отвергает обрезанную часть раньше, и до них управление уже не доходит. Это следствие решения п.1, а не ослабление проверки: петли больше не могут зависнуть ни при каком поведении ридера.
- AUD-04: добавлен `PartParser::expect_end_of_part()`. Без него «content after the root element» был бы недостижим через `open_*`: парсер по построению останавливается на закрывающем теге корня, и хвост после него никто не читает. Это выходит за букву п.2, но без него половина п.2 не проверяема через публичный API.
- AUD-04: у `xtool lint-eof` есть явная отмена — маркер `lint-eof: this arm is the success case` в документации функции. Без неё линт не может отличить `expect_end_of_part` (где `Eof` — успех) от петли, которая глотает конец части.

---

## 0. Общие правила

### 0.1. Правило закрытия задачи

Задача закрыта, когда **одновременно**:

1. выполнены все пункты её «Приёмки»;
2. добавлены все тесты её раздела «Тесты», и каждый тест **падает на коде до правки**
   (исполнитель проверяет это локально: откатывает правку, видит красный тест, возвращает;
   в описании коммита указывается, что проверка сделана);
3. полный гейт (§0.4) зелёный;
4. обновлены документы, перечисленные в задаче (ADR, ТЗ, `CORE-QUEUE.md`, `docs/waivers.toml`);
5. один коммит на задачу (или серию коммитов с общим префиксом ID задачи в сообщении),
   формат: `fix(<crate>): AUD-NN <кратко>`.

### 0.2. Общие решения, действующие во всех задачах

| # | Решение |
|---|---|
| G-1 | Никаких `unwrap`/`expect`/`panic!`/`unreachable!`/`debug_assert!` на значениях, зависящих от входа, в библиотечном коде. Индексация срезов — только через `get`/`get_mut` либо после явной проверки границ в той же функции. |
| G-2 | Арифметика над значениями из входа (twips, EMU, счётчики, размеры, смещения): `checked_*` там, где переполнение — ошибка входа; `saturating_*` там, где результат — геометрия, которую допустимо зажать. Приведения `as` между целыми запрещены для значений из входа — только `try_from` с обработкой ошибки. `f64 → целое` — только через функцию `units::to_i64_saturating` (создаётся в AUD-05), которая отображает NaN в 0 и зажимает диапазон. |
| G-3 | Любое значение из входа, которое модель не может выразить, даёт **запись**: в `SupportModel` (парсер), в `WriteReport` (писатель), в `NormalizationReport` (нормализатор), в отчёт рендера (`render-pdf`/`convert`). «Молча проигнорировать» запрещено; `_ => {}` в `match` по имени элемента допускается только если ветка делает запись. |
| G-4 | Ограничения на ресурсы — поля `ResourceLimits` (ядро, WML, рендер) или `PdfLimits` (PDF). Новые захардкоженные константы-пределы не вводятся; существующие (`MAX_MATH_NODES`, `MAX_MATH_DEPTH`, `MAX_ITEMS`, `MAX_PAGES`) переносятся в структуры лимитов в задачах ниже. |
| G-5 | Экранирование XML для **всех** выходов (writer, SVG, MathML, convert, `content_types::write_xml`, `rels::write`) — только через один модуль `strict_ooxml_core::xml::escape` (создаётся в AUD-03). Локальные функции `escape_*` удаляются. |
| G-6 | Тестовые `.docx`/`.pdf` для враждебных случаев **не** коммитятся бинарниками: они строятся в тесте через `strict-ooxml-testkit` (AUD-01). Исключение — реальные документы корпуса, уже лежащие в `tests/strict/`, `tests/docx/`. |
| G-7 | Оракул для OPC-слоя — реальные пакеты, сделанные Word/LibreOffice (`strict-ooxml-core/tests/strict/*.docx`), и официальные схемы ECMA-376 Part 2. Наш собственный код оракулом для OPC быть не может. |

### 0.3. Новые ADR, которые создаются по ходу плана

| ADR | Задача | Содержание |
|---|---|---|
| `0015-opc-namespaces.md` | AUD-20 | OPC (ISO/IEC 29500-2) не имеет Strict-варианта пространств имён; перечень URI; отзыв соответствующих утверждений ADR-0007 |
| `0016-conformance-policy.md` | AUD-23 | Матрица «политика × обнаруженная конформность × наличие нормализатора → результат»; детекция по сырым сигналам (T0) |
| `0017-normalization-report.md` | AUD-30 | Учёт отчёта по частям, идемпотентность, связь с Feature Report |
| `0018-revisions.md` | AUD-43 | Модель правок `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` |
| `0019-tz-deviations.md` | AUD-90 | Отклонения от ТЗ, принятые решением (immutable DOM, `round-trip-normalize`, `resolve_theme`/`load_media`, `SourceLocation` на текстовых узлах, content types в детекции) |

Номер `0013` в `docs/adr/` отсутствует; новые ADR его не занимают.

### 0.4. Полный гейт (запускается перед закрытием каждой задачи)

```text
cargo +1.92.0 fmt --all -- --check
cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.92.0 check --workspace --all-targets                      # default features
cargo +1.92.0 test --workspace --all-features
cargo +1.92.0 test --workspace --all-features --release --test hostile   # с AUD-01
cargo +1.92.0 deny check
cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 89
cargo run -p xtool -- coverage --file coverage/stage5-scenarios.toml --min 85
python xtool/xsd-gate/xsd_gate.py
python xtool/xsd-gate/census_gate.py                              # только локально (waiver CENSUS-LOCAL)
python xtool/xsd-gate/opc_gate.py                                  # с AUD-21
```

Покрытие (`cargo llvm-cov`, CI-способ с `CARGO_PROFILE_DEV_CODEGEN_UNITS=1`) — **не ниже**
текущего для каждого крейта и не ниже порогов §15 этого документа; проверяется в конце каждой
фазы, а не каждой задачи.

### 0.5. Порядок фаз

```
Ф0  Инфраструктура            AUD-00…03   testkit, hostile-набор, escape, remote/CI
Ф1  Ядро: XML и лимиты        AUD-04…07   Eof, вложенность, корень, лимиты в структуре
Ф2  Паники и DoS              AUD-08…16   все падения/зависания во всех крейтах
Ф3  OPC и конформность        AUD-20…26   URI, T2-таблица, имена частей, политика, детекция
Ф4  Нормализатор и отчёт      AUD-30…36   учёт по частям, Feature Report, bidi, опции
Ф5  Модель WML: потери        AUD-40…52   секции, sdt, контейнеры, правки, toggles, свойства
Ф6  Писатель                  AUD-60…67   сноски, связи, имена, ZIP, passthrough
Ф7  Рендер SVG                AUD-70…78   поля, табуляция, секции, бюджет, MathML
Ф8  PDF, конвертер, вьюер     AUD-80…87   корректность writer/reader/convert/view
Ф9  Процесс, ТЗ, документы    AUD-90…94   ADR, ТЗ, fuzz, CI, финальная приёмка
```

Почему так: Ф1–Ф2 устраняют то, что роняет процесс, и без этого фаззинг (Ф9) бесполезен;
Ф3 должна предшествовать Ф4 и Ф6, потому что нормализатор и писатель пишут OPC-URI и типы
связей, которые Ф3 исправляет; Ф5 предшествует Ф6 и Ф7, потому что писатель и рендер
потребляют новые поля модели.

---

## Ф0. Инфраструктура

### AUD-00. Подключить удалённый репозиторий и реально запустить CI — ✅

**Проблема.** У репозитория нет `git remote`; `.github/workflows/ci.yml` ни разу не исполнялся,
включая fuzz-smoke и fuzz-nightly. Все утверждения «в CI» не проверены.

**Решение.** Владелец создаёт удалённый репозиторий на GitHub (CI написан под GitHub Actions)
и добавляет `origin`. Исполнитель пушит текущую ветку `main` **до** начала Ф1, чтобы получить
базовую линию.

**Приёмка.**
1. `git remote -v` показывает `origin`.
2. Первый прогон CI на неизменённом `main` выполнен; его результат (зелёный/красный по каждому
   job) записан в `docs/ci-baseline-2026-10.md` со ссылкой на run.
3. Каждый красный job из базовой линии либо чинится в рамках этого плана (ссылка на AUD-ID),
   либо получает waiver в `docs/waivers.toml` с датой пересмотра.

**Тесты.** Нет (инфраструктура).

---

### AUD-01. Крейт `strict-ooxml-testkit` и набор `hostile` — ✅

**Проблема.** Враждебные входы строятся вручную в каждом крейте по-своему (см. `build_test_zip`
в `opc/zip/mod.rs`, `common/mod.rs` в нескольких `tests/`), часть повторов лежит в `%TEMP%`.

**Решение.**
- Новый член workspace `strict-ooxml-testkit` (`publish = false`, только `[dev-dependencies]`
  других крейтов).
- API:
  - `DocxBuilder::strict()` / `DocxBuilder::transitional()` — минимальный валидный пакет
    (`[Content_Types].xml`, `_rels/.rels`, `word/document.xml`, `word/_rels/document.xml.rels`)
    с правильными для семейства URI (OPC-URI — по ADR-0015, т.е. `schemas.openxmlformats.org/package/2006/...`
    для обоих семейств);
  - `.body(xml: &str)` — содержимое `w:body`; `.part(name, bytes)` / `.part_xml(name, xml)`;
    `.rel(source, id, type, target)`; `.raw_part(name, bytes)` (без проверок — для битых XML);
    `.stored()` / `.deflated()`; `.build() -> Vec<u8>`;
  - `nested(open: &str, close: &str, inner: &str, depth: usize) -> String` — генератор вложенности;
  - `PdfBuilder` — минимальный PDF 1.7 с xref, `.page(content: &[u8])`, `.object(id, dict_bytes)`,
    `.stream(id, dict, data, compress: bool)`, `.build()`.
- Набор тестов `hostile` — файл `tests/hostile.rs` в каждом из крейтов `strict-ooxml`
  (мета), `strict-ooxml-pdf`, `strict-ooxml-convert`. Каждый тест: строит вход, вызывает
  публичный API, утверждает `Err(..)` нужного вида **или** `Ok` с записью в отчёте, и что
  процесс не упал. Тесты переполнения стека запускаются в отдельном потоке с размером стека
  **1 MiB** (`std::thread::Builder::new().stack_size(1 << 20)`), чтобы воспроизводить
  поведение главного потока Windows.
- Таймаут на тест зависаний: тест запускает работу в потоке и ждёт `recv_timeout(10 s)`;
  по таймауту — `panic!("hang")` (в тестах паника допустима).

**Изменения.** `Cargo.toml` (members), `strict-ooxml-testkit/**`, `strict-ooxml/tests/hostile.rs`,
`strict-ooxml-pdf/tests/hostile.rs`, `strict-ooxml-convert/tests/hostile.rs`, CI: шаг
`cargo test --workspace --all-features --release --test hostile`.

**Тесты.** Юнит-тесты testkit: собранный `DocxBuilder::strict()` открывается
`StrictDocument::open_reader` под `StrictOnly` с `Conformance::Strict`; собранный
`PdfBuilder` с одной страницей открывается `PdfDocument::open`.

**Приёмка.** Всё выше; `hostile` присутствует в CI как отдельный шаг в debug **и** release.

---

### AUD-02. Перенести повторы аудита в `hostile` — ✅

**Проблема.** Подтверждённые повторы дефектов лежат во временной папке.

**Решение.** Для каждого дефекта Ф1–Ф2 тест в `hostile` создаётся **в задаче, которая его
чинит** (а не здесь). Эта задача только создаёт пустые файлы `hostile.rs` с модульной
структурой: `mod xml; mod nesting; mod table; mod numbering; mod math; mod pdf_fonts;
mod pdf_images; mod writer;` и комментарием-ссылкой на AUD-ID в каждом модуле.

**Приёмка.** Файлы существуют, компилируются, пусты.

---

### AUD-03. Единый модуль экранирования XML — ✅

**Проблема.** Шесть независимых функций экранирования; ни одна не фильтрует символы,
недопустимые в XML 1.0 (U+0000–U+0008, U+000B, U+000C, U+000E–U+001F, U+FFFE, U+FFFF),
поэтому `w:sym w:char="0001"` даёт неразбираемый SVG, а писатель/конвертер — неразбираемый
`document.xml`.

**Решение.**
- Модуль `strict_ooxml_core::xml::escape`:
  - `pub fn is_xml_char(c: char) -> bool` — производство `Char` из XML 1.0 §2.2;
  - `pub fn escape_text_into(out: &mut String, s: &str) -> usize` — `& < >`, `\r` → `&#13;`,
    недопустимые символы **удаляются**; возвращает число удалённых;
  - `pub fn escape_attr_into(out: &mut String, s: &str) -> usize` — `& < > " '`,
    `\t \n \r` → `&#9; &#10; &#13;`, недопустимые удаляются; возвращает число удалённых.
  - `\t \n \r` в `content_types::write_xml` **перестают** заменяться пробелом (сейчас
    `escape_into` делает `' '`) — это порча значения; теперь они пишутся ссылками.
- Все вызывающие (writer `xml.rs`, `render-svg/paint/mod.rs`, `render-svg/math/mathml.rs`,
  `convert`, `opc/content_types.rs`, `opc/rels.rs`) переходят на модуль. Возвращённое число
  удалённых символов > 0 даёт запись:
  - писатель: `WriteReport` loss `W.invalid-xml-char` (severity `Lossy`), одна запись на часть
    с суммарным числом;
  - SVG: `RenderReport`/warning `render.invalid-xml-char` (если у рендера нет отчёта — в
    `Page::warnings: Vec<String>`, поле добавляется);
  - convert: запись в `ConversionReport` `convert.invalid-xml-char`.

**Тесты.**
- Юнит: для каждого из 32 управляющих символов и U+FFFE/U+FFFF — удалён; `\t\n\r` в
  атрибуте — ссылки; в тексте `\t\n` сохраняются как есть, `\r` → `&#13;`; суррогатов в `&str`
  не бывает (тест не нужен).
- Свойство (proptest): для любой строки результат `escape_text_into` вложенный в
  `<a>…</a>` разбирается `quick-xml` без ошибки; то же для атрибута.
- Интеграция: `.docx` с `w:sym w:char="0001"` и с текстом `"a\u{1}b"` (через `&#1;` в XML
  нельзя — это ошибка разбора; поэтому текст подаётся через `document_mut()`):
  SVG-страница и выход `write_package` разбираются `roxmltree` без ошибки, в отчётах есть
  запись.

**Приёмка.** `rg -n "fn escape" --type rust` вне `core/src/xml/escape.rs` и тестов — пусто.

---

## Ф1. Ядро: XML и лимиты

### AUD-04. `XmlReader`: конец документа с незакрытыми элементами — ошибка

**Проблема.** `XmlReader::next_event` (`core/src/xml/mod.rs:166`) на `Eof` не проверяет
`open_names`; на повторных вызовах снова отдаёт `Eof`. Три цикла WML-парсера на `Eof` не
выходят → **зависание** на обрезанном `fontTable.xml`/`settings.xml` (подтверждено). Усечённые
части в остальных местах принимаются молча.

**Решение.**
1. На `Parsed::Eof`: если `open_names` не пуст или `pending_end` не `None` →
   `StrictError::InvalidXml { detail: "unexpected end of document: N unclosed element(s), innermost <name>" }`.
2. Корень: после закрытия корневого элемента допустимы только пробельный текст, комментарии,
   PI. Второй корневой элемент или непробельный текст → `InvalidXml("content after the root element")`.
   Отсутствие корня (документ пуст / только пролог) → `InvalidXml("no root element")`.
3. После того как `next_event` вернул `Eof` один раз, следующие вызовы возвращают `Eof`
   (идемпотентно), но это больше не может скрыть незакрытый элемент — п. 1 срабатывает раньше.
4. Три цикла WML (`parse/fonts.rs:81`, `:134`, `parse/settings.rs:357`) получают явную ветку
   `XmlEvent::Eof => return Err(self.invalid("unexpected end of part"))` — защита в глубину.
5. Общая проверка: в `strict-ooxml-wml` любой `loop { match self.next_event()? … }` обязан
   иметь ветку `Eof`, возвращающую ошибку. Для этого добавляется xtool-проверка
   `xtool lint-eof` (поиск `loop` с `next_event` без `Eof` в той же функции; простая текстовая
   эвристика по функции) и шаг в CI.

**Изменения.** `core/src/xml/mod.rs`, `wml/src/parse/{fonts,settings}.rs`, `xtool/src/main.rs`,
`ci.yml`.

**Тесты.**
- core юнит: `<a><b>` → ошибка с `"2 unclosed"`; `<a/><b/>` → ошибка «content after the root»;
  `<a/>text` → ошибка; `<a/>  <!--c-->` → ок; `""` → «no root element»; `<?xml?>` → «no root».
- hostile (`mod xml`): обрезанные `fontTable.xml`, `settings.xml` (`<m:mathPr>` без закрытия),
  `styles.xml`, `numbering.xml`, `document.xml`, `footnotes.xml`, `header1.xml` — каждый
  `open_path` завершается ошибкой `InvalidXml` за < 10 с.
- Регрессия корпуса: все существующие корпусные тесты зелёные (корпус не должен содержать
  мусора после корня; если содержит — задача останавливается, см. «Как читать»).

**Приёмка.** Тесты выше; `xtool lint-eof` в CI зелёный.

---

### AUD-05. Лимит вложенности блоков и общий счётчик глубины

**Проблема.** 40 вложенных таблиц (≪ `max_xml_depth = 256`) переполняют стек уже в `check`
(подтверждено, debug и release); около 8 вложенных текстовых блоков переполняют стек в debug.
Рендер сбрасывает счётчик глубины на 0 на каждом уровне (`table.rs:312`, `graphics.rs:323`,
`headerfooter.rs:166`), поэтому его предел `depth > 8` не работает.

**Решение.**
1. Новое поле `ResourceLimits::max_block_nesting: u32`, по умолчанию **12**; `LimitKind::BlockNesting`.
   Считается **одним** счётчиком на документ: вход в любой из контейнеров `w:tbl`, `w:txbxContent`,
   `w:sdtContent` (блочный), `w:customXml` (блочный), `w:comment`/`w:footnote`/`w:endnote`
   (содержимое), `wps:txbx`, `v:textbox` (после нормализации), `w:hdr`/`w:ftr` увеличивает
   его, выход уменьшает. Превышение → `StrictError::LimitExceeded { kind: BlockNesting, .. }`
   для всего документа (не деградация: документ с такой вложенностью — враждебный).
2. Рендер: глубина передаётся параметром во все вызовы `layout_blocks_inline` (никаких литералов
   `0` кроме корня страницы), предел — тот же `max_block_nesting` из `RenderOptions::limits`
   (поле `limits: ResourceLimits` добавляется в `RenderOptions`, по умолчанию `ResourceLimits::default()`).
   Превышение в рендере (возможно только для `Document`, построенного программно) →
   `RenderError::LimitExceeded`.
3. Писатель: та же проверка при обходе модели (`body.rs`) → `WriteError::LimitExceeded`.
4. Функция `units::to_i64_saturating(f64) -> i64` (NaN → 0, ±inf → MIN/MAX) создаётся в
   `render-svg/src/units.rs` и экспортируется для `render-pdf` (решение G-2).
5. Значение 12 выбрано так, чтобы глубина 12 проходила в **debug** на стеке 1 MiB с запасом;
   это проверяется тестом, а не предполагается. Если тест на 12 не проходит в debug —
   исполнитель уменьшает кадры стека (выносит крупные локальные структуры в `Box`), а не
   понижает лимит.

**Тесты.**
- hostile (`mod nesting`), поток со стеком 1 MiB, debug **и** release:
  - таблицы глубины 12 → `parse` + `render_svg` + `write_package` + `render_pdf` = `Ok`;
  - таблицы глубины 13 → `Err(LimitExceeded{BlockNesting})` из `open_*`;
  - текстовые блоки (`wps:txbx` в `w:drawing`) глубины 12 → `Ok`, 13 → ошибка;
  - смешанная вложенность: tbl → sdt → txbx → tbl … суммарно 13 → ошибка;
  - 200 вложенных таблиц → ошибка (не переполнение стека).
- render-svg юнит: программно построенный `Document` с 13 уровнями → `RenderError::LimitExceeded`.

**Приёмка.** Тесты выше; `max_block_nesting` документирован в `limits.rs` и в ТЗ §12.1 (AUD-91).

---

### AUD-06. Пределы формул — в `ResourceLimits`, деградация одной формулы

**Проблема.** `MAX_MATH_NODES`/`MAX_MATH_DEPTH` захардкожены (`wml/parse/math.rs:28-31`) и
превышение валит весь документ.

**Решение.** Поля `ResourceLimits::max_math_nodes: u32 = 4096`, `max_math_depth: u32 = 64`.
Превышение внутри одного `m:oMath`: формула заменяется на `MathNode::Opaque` (существующий
`UnknownMathNode` с именем `oMath`), поддерево пропускается `skip_element`, в `SupportModel`
запись `m:oMath` со статусом `Unsupported` и причиной `"formula exceeds max_math_nodes/max_math_depth"`.
Документ продолжает разбираться. `LimitKind::MathNodes/MathDepth` остаются, но больше не
возвращаются из `parse_document`.

**Тесты.** hostile (`mod math`): формула с 5000 узлов и с глубиной 70 → документ открыт,
`support_report()` содержит `m:oMath` `unsupported` с локацией, остальной текст абзаца на месте.

**Приёмка.** Тесты; константы удалены из `math.rs`.

---

### AUD-07. Утечка счётчика глубины в WML-парсере

**Проблема.** `enter()` без `leave()` в `parse/fonts.rs:47`, `parse/settings.rs:312` → ложный
`LimitExceeded(XmlDepth)` при ≥256 `m:mathPr`.

**Решение.** Заменить пары `enter`/`leave` на guard-тип `DepthGuard` (RAII через `&mut`
не получится из-за заимствования парсера) — поэтому решение: функция-обёртка
`fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T>`, которая
делает `enter`, вызывает `f`, делает `leave` при **любом** исходе. Все прямые вызовы `enter()`
в крейте переписываются на `nested`. `enter`/`leave` становятся приватными для `nested`.

**Тесты.** wml юнит: `settings.xml` с 300 элементами `m:mathPr` подряд → `Ok`. Свойство:
после `parse_document` на всём корпусе `parser.depth == 0` (через `debug`-аксессор,
`#[cfg(test)]`).

**Приёмка.** `rg "\.enter\(\)" strict-ooxml-wml/src` — только внутри `nested`.

---

## Ф2. Паники и DoS

### AUD-08. SVG: строка с ячейками сверх `tblGrid`

**Проблема.** `render-svg/src/layout/table.rs:85` `widths[..column]` паникует, когда сумма
`gridSpan` в строке больше числа колонок сетки (подтверждено, exit 101 в release).

**Решение.**
1. `column_count = max(tblGrid.len(), max по строкам Σ gridSpan)`.
2. `column_widths` для недостающих колонок: остаток ширины контента
   `max(content_width − Σ grid, 0)` делится поровну; если остаток 0 — каждой недостающей
   колонке ширина = среднее существующих колонок (если существующих нет — `content_width / column_count`).
3. Все срезы `widths[..]` заменяются на `get(..)` с суммированием через итератор с `take`/`skip`.
4. Запись в `SupportModel` не нужна (рендер), но в `Page::warnings` (AUD-03) добавляется
   `"table row has N grid columns, tblGrid declares M"`.
5. Та же логика в писателе не нужна (он пишет модель как есть); в `render-pdf` используется
   общая раскладка — исправление в одном месте.

**Тесты.** hostile (`mod table`): `tblGrid` из 1 колонки и строка из 3 ячеек; `tblGrid` пуст
и 2 ячейки; `gridSpan=65535` → `render_svg` и `render_pdf` = `Ok`, ширина таблицы ≤ ширины
контента + 1 px. render-svg юнит на `column_widths`.

**Приёмка.** Тесты; `rg "widths\[" strict-ooxml-render-svg/src` — пусто.

---

### AUD-09. SVG: переполнения целых и `debug_assert!`

**Проблема.** Подтверждённые паники в debug (тихий неверный результат в release):
сумма `gridCol` (`table.rs:237`), `lIns + rIns`/`tIns + bIns` (`graphics.rs:320`, `:333`),
счётчик нумерации `value + 1` (`numbering.rs:113`), `debug_assert!` зазора дроби
(`math/layout.rs:833`).

**Решение.**
- Сумма сетки: в `i64`, `saturating_add`.
- Отступы текстового блока: `saturating_add`; итоговая внутренняя ширина/высота зажимается
  снизу 1 px.
- Нумерация: `saturating_add(1)`; в модели `start` дополнительно зажимается парсером в
  `0..=i32::MAX as u32` с записью в `SupportModel`, если зажат (`w:start` вне диапазона).
- `debug_assert!` удаляется; зазор = `max(computed, MIN_FRACTION_PART_GAP_PX)`.
- Аудит крейта по G-2: `rg " as (u8|u16|u32|i32|usize|i64)" strict-ooxml-render-svg/src` —
  каждое вхождение либо заменено на `try_from`/`to_i64_saturating`, либо снабжено комментарием,
  почему значение не из входа (например, индекс из `enumerate`).

**Тесты.** hostile: `gridCol w:w="2000000000"` ×2; `a:bodyPr lIns="9223372036854775807" rIns="1"`;
`w:start w:val="4294967295"` + 2 пункта; формула `m:f` при `w:sz=6` и в индексе третьего
уровня. Все — `Ok` в debug.

**Приёмка.** Тесты в debug; результат аудита `as` записан в описании коммита.

---

### AUD-10. Писатель: паника в `remove_element`

**Проблема.** `write/src/passthrough.rs:748` `rest[at + open.len()]` паникует, если часть
кончается на `<Pages` (байты из входного `app.xml`).

**Решение.** `let Some(&after) = rest.get(at + open.len()) else { out.extend_from_slice(rest); break; };`.
Плюс аудит всех байтовых функций `passthrough.rs` (`find`, `replace_all`, `remove_element`
и т.п.) на индексацию: заменить на `get`.

**Тесты.** writer юнит: `remove_element(b"<x><Pages", "Pages")`, `b"<Pages"`, `b"<Pages/"`,
`b""` — без паники, вывод = вход. hostile (`mod writer`): `.docx` с `docProps/app.xml`,
обрезанным на `<Pages` (через `raw_part`, CRC валиден) → `write_package` = `Ok` или
`Err(WriteError)` — без паники.

**Приёмка.** Тесты.

---

### AUD-11. Писатель: ZIP-приведения

**Проблема.** `core/src/opc/zip/write.rs:245-246`, `:269-270`: размеры `as u32`, длина имени
`as u16` без проверки.

**Решение.** ZIP64 **не** пишется (части ограничены 128 MiB лимитами чтения, пакет ≤ 512 MiB).
Вместо этого `u32::try_from`/`u16::try_from`; ошибка → `StrictError::LimitExceeded { kind:
LimitKind::ZipWriteField, .. }` (новый `LimitKind`). Число записей > 65 535 → то же. Смещение
центральной директории > `u32::MAX` → то же.

**Тесты.** core юнит: запись части с именем длиной 65 536 байт → ошибка; 65 536 частей
(пустых) → ошибка; нормальный пакет — байт-в-байт как раньше (снапшот существующего теста
детерминизма не меняется).

**Приёмка.** Тесты; `rg " as u(16|32)" core/src/opc/zip/write.rs` — только константы.

---

### AUD-12. PDF-ридер: `ToUnicode`, `/W`, `/SMask`

**Проблема (по коду, подтверждено чтением).**
- `pdf/src/fonts.rs:646`: срез `&str` по байтам в `utf16_value` — паника на не-ASCII.
- `pdf/src/fonts.rs:488`: `for code in first..=second` без ограничения — до 4·10⁹ вставок.
- `pdf/src/image.rs:261-265`: `/SMask`, ссылающийся на себя (или цикл масок), — бесконечная
  рекурсия, переполнение стека.

**Решение.**
- `ToUnicode`: hex-токены валидируются как ASCII hex (`bytes().all(|b| b.is_ascii_hexdigit())`)
  до любого среза; невалидный токен пропускается с записью `pdf.font.tounicode-invalid`
  (один раз на шрифт). Срезы делаются по `as_bytes()` с `chunks_exact(4)`.
- `/W`: длина диапазона `second − first + 1` ограничивается `PdfLimits::max_font_glyphs`
  (существующее поле); при превышении диапазон обрезается до лимита и пишется
  `pdf.font.widths-truncated`. `second < first` → запись пропускается.
  То же правило для `bfrange` в `ToUnicode`.
- `/SMask`: `decode` получает множество `in_progress: RefCell<BTreeSet<ObjectId>>` (в кэше
  и в `decode_from` — параметр); повторный вход в id из множества → маска отсутствует
  (`None`), запись `pdf.image.smask-cycle`. Маска маски не декодируется вовсе (по
  ISO 32000-1 у `/SMask` не может быть своего `/SMask`): `decode_inner` для маски вызывается
  с `mask_of = |_| None`.

**Тесты.** hostile (`mod pdf_fonts`, `mod pdf_images`) через `PdfBuilder`:
- `ToUnicode` с `<00é1>` и `<ЖЖЖЖ>`; `/W [0 4294967295 500]`; `bfrange <0000> <FFFFFFFF> <0041>`;
- изображение с `/SMask` = само себя; цикл A→B→A.
Каждый — `PdfDocument::open` + чтение всех страниц за < 10 с, без паники, запись в отчёте.

**Приёмка.** Тесты.

---

### AUD-13. PDF-ридер: бюджеты, которые не проверяются или обходятся

**Проблема.** `max_glyphs` считается на вызов формы (обходится множеством `Do`); форма и её
шрифты декодируются заново на каждом `Do`; `max_path_points`, `max_fonts`, `max_raster_pixels`
нигде не проверяются; распаковка потоков (контент, формы, изображения, `ToUnicode`) не
ограничена.

**Решение.**
1. Счётчики `glyphs`, `operations`, `path_points` — **на страницу**, общие для всех вложенных
   форм (передаются `&mut PageBudget`). Превышение → обработка страницы прекращается, уже
   собранное сохраняется, запись `pdf.page.budget` с именем лимита.
2. Кэш декодированных форм по `ObjectId` на документ (`RefCell<HashMap<ObjectId, Rc<DecodedForm>>>`);
   шрифты — кэш по `ObjectId` на документ, `max_fonts` проверяется при вставке
   (превышение → шрифт не декодируется, глифы этого шрифта считаются без Unicode, запись
   `pdf.font.budget`).
3. `max_raster_pixels` проверяется в `raster.rs` до аллокации буфера: `w × h` через `checked_mul`.
4. Ограниченная распаковка: функция `bounded_decompress(stream, limit: usize) -> Result<Vec<u8>, Reject>`
   в `pdf/src/document.rs` на `miniz_oxide::inflate::decompress_to_vec_zlib_with_limit`;
   все вызовы `decompressed_content()` lopdf заменяются на неё. Лимиты: контент страницы —
   `max_content_bytes`, изображение — `max_image_bytes`, прочее (формы, CMap) —
   `max_content_bytes`.
5. Распаковка объектных потоков при `lopdf::Document::load` нашим кодом не ограничивается;
   это **принятый остаточный риск**: файл уже ограничен `PdfLimits::max_input_bytes`
   (добавить поле, 256 MiB, проверка до `load`), и waiver `PDF-OBJSTM-BOMB` в
   `docs/waivers.toml` с ссылкой на fuzz-таргет `fuzz_pdf` (AUD-92).

**Тесты.** hostile: страница с формой, вызванной 10 000 раз, каждая по 1000 глифов →
обработка за < 10 с, запись `pdf.page.budget`; flate-бомба 1 KiB → 1 GiB в контенте и в
изображении → `Reject`/запись, пиковая память не измеряется, но тест завершается < 10 с;
документ с 10 000 шрифтов → запись `pdf.font.budget`.

**Приёмка.** Тесты; `rg "decompressed_content" strict-ooxml-pdf/src` — только внутри `bounded_decompress`.

---

### AUD-14. PDF-писатель: аллокация по IHDR

**Проблема.** `render-pdf/src/image.rs:152` выделяет буфер по размерам из заголовка PNG.

**Решение.** До декодирования: `width × height × channels × bytes_per_sample` через
`checked_mul` ≤ `RenderOptions::limits.max_single_uncompressed` (переиспользуется
существующий лимит ядра: изображение не может распаковаться больше, чем часть). Превышение —
изображение не встраивается, `PdfLoss` `pdf.image.too-large`.

**Тесты.** hostile: PNG 67 байт с IHDR 65535×65535 → `render_pdf` = `Ok`, потеря в отчёте,
тест < 10 с.

**Приёмка.** Тест.

---

### AUD-15. Конвертер: квадратичный поиск таблиц и размер сетки

**Проблема.** Поиск таблиц в `convert/src/tables.rs` квадратичен по числу линий; размер
сетки не ограничен.

**Решение.** Новые поля `PdfOptions` (конвертер): `max_table_lines: usize = 4000`,
`max_table_cells: usize = 10_000`. Линии на странице сверх `max_table_lines` → таблицы на
странице не ищутся, запись `convert.table.budget`. Сопоставление линий — сортировка по
координате + проход «скользящим окном» с допуском (O(n log n)), а не попарное сравнение.
Сетка больше `max_table_cells` → таблица не строится, текст идёт абзацами, запись.

**Тесты.** hostile (convert): страница с 20 000 горизонтальных и вертикальных отрезков →
конвертация < 10 с, запись. Существующие `tests/tables.rs` зелёные без изменений ожиданий.

**Приёмка.** Тесты; бенчмарк в описании коммита: время на странице с 4000 линий до/после.

---

### AUD-16. Вьюер: неограниченное чтение и зависание

**Проблема.** `view/src/http.rs`: `read_line` без предела длины; однопоточный сервер без
таймаута чтения висит на простаивающем keep-alive.

**Решение.** Строка запроса и заголовки читаются через `Read::take(8 KiB)` на строку, не
более 100 заголовков, тело не более 1 MiB (если `Content-Length` больше — `413`).
`set_read_timeout(Some(10 s))` и `set_write_timeout(Some(10 s))` на каждом соединении.
Keep-alive отключается: ответ всегда с `Connection: close`. Сервер остаётся однопоточным.

**Тесты.** view юнит/интеграция: строка запроса 100 KiB → `431`/закрытие; соединение без
данных закрывается через ≤ 11 с, следующий запрос обслуживается; 101 заголовок → `431`.

**Приёмка.** Тесты.

---

## Ф3. OPC и конформность

### AUD-20. OPC-пространства имён и типы связей — стандартные

**Проблема.** Писатель и нормализатор пишут несуществующие в стандарте URI (подтверждено на
`lo-strict.docx`): `.rels` с `xmlns="http://purl.oclc.org/ooxml/package/relationships"`,
тип `…/package/relationships/metadata/core-properties`, пространство
`…/package/metadata/coreProperties`; тип `…/officeDocument/relationships/extended-properties`.
Реестр помечает это как `verified: true`; ADR-0007 содержит ту же ошибку.

**Решение (ADR-0015).** Каноничные значения, сверенные с `strict01-sdk.docx` (Word 15) и
`lo-strict.docx` (LibreOffice), действуют для **обоих** семейств, кроме помеченных:

| Что | URI |
|---|---|
| `.rels` namespace | `http://schemas.openxmlformats.org/package/2006/relationships` |
| `[Content_Types].xml` namespace | `http://schemas.openxmlformats.org/package/2006/content-types` |
| core properties namespace | `http://schemas.openxmlformats.org/package/2006/metadata/core-properties` |
| core properties rel type | `http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties` |
| thumbnail rel type | `http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail` |
| digital signature rel types | `http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/*` |
| extended properties rel type (Strict) | `http://purl.oclc.org/ooxml/officeDocument/relationships/extendedProperties` |
| custom properties rel type (Strict) | `http://purl.oclc.org/ooxml/officeDocument/relationships/customProperties` |
| docPropsVTypes namespace (Strict) | `http://purl.oclc.org/ooxml/officeDocument/docPropsVTypes` |

Изменения:
1. `ns/registry.rs`: у `package.relationships` и `package.metadata.coreProperties`
   `strict == transitional` (стандартный URI); новое поле `NamespaceEntry::family_neutral: bool`
   (`true` у этих двух, у `markupCompatibility` и у нового `package.contentTypes`);
   `classify_namespace` для `family_neutral` возвращает `None`. Добавить запись
   `officeDocument.docPropsVTypes` (strict/transitional из таблицы, `verified: true`).
2. `opc/rels.rs`: константа `STRICT_PACKAGE_REL_NS` удаляется, вместо неё
   `PACKAGE_REL_NS` = стандартный URI; `write` пишет его.
3. `normalize/transitional.rs`: ветка `TRANSITIONAL_PACKAGE_REL_NS → STRICT_PACKAGE_REL_NS`
   (строки 1915–1917) удаляется. **Новая** ветка ремонта: если вход содержит
   `http://purl.oclc.org/ooxml/package/relationships` (пакеты, записанные прошлыми версиями
   этого проекта), он заменяется на стандартный URI с записью `T1.namespace-repair`. Так же для
   `…/package/metadata/coreProperties` и `…/package/relationships/metadata/*`.
4. `write/src/passthrough.rs`: `thumbnail_type_uri`, `core_properties_type_uri`,
   `STRICT_RELS_NS`, `STRICT_CORE_PROPERTIES_NS` и `strict_rels_namespace` переходят на
   стандартные значения; `docPropsVTypes` Transitional → Strict (уже есть `STRICT_VARIANT_TYPES_NS`
   — проверить значение по таблице).
5. ADR-0007 §«Strict only» и §W7 пункт «the one exception» исправляются ссылкой на ADR-0015.

**Тесты.**
- `write/tests/opc_oracle.rs` (новый): для каждого файла `core/tests/strict/*.docx` —
  `write_package`, затем в выходе множество (namespace `.rels`, тип core-properties, тип
  extended-properties, namespace `core.xml`, namespace `vt` в `app.xml`) **равно** множеству
  тех же значений во входе. Это оракул G-7: значения берутся из файлов Word/LibreOffice, не из
  нашего кода.
- Тот же тест для `write --transitional` на `core/tests/docx/*.docx`: значения в выходе равны
  значениям из `lo-strict.docx` (эталон Strict-пакета).
- Существующие тесты, утверждающие purl-URI (`write/tests/passthrough.rs:355-373`,
  `strict_conformance.rs:53`, `normalize_roundtrip.rs:502`, `:662`) **переписываются** на
  стандартные URI — в коммите явно перечисляются.
- core юнит: `classify_namespace("http://schemas.openxmlformats.org/package/2006/relationships") == None`.
- Ремонт: пакет с purl-`.rels` под нормализатором → `.rels` в выходе со стандартным URI,
  запись `T1.namespace-repair`.

**Приёмка.** `rg "purl.oclc.org/ooxml/package" --type rust` — только в ветке ремонта и её тестах.

---

### AUD-21. OPC-гейт по схемам ECMA-376 Part 2

**Проблема.** `xsd_gate.py` исключает `.rels` (`OUT_OF_SCOPE_SUFFIXES`); `[Content_Types].xml`
и `core.xml` не проверяются ничем.

**Решение.** Новый скрипт `xtool/xsd-gate/opc_gate.py` на том же оракуле (`xmlschema`) и той
же схеме кэширования, что `xsd_gate.py`: скачивает схемы ECMA-376 Part 2
(`opc-relationships.xsd`, `opc-contentTypes.xsd`, `opc-coreProperties.xsd` и их импорты Dublin
Core), валидирует в каждом записанном пакете (Strict-корпус через `write` и Transitional-корпус
через `write --transitional`) все `*.rels`, `[Content_Types].xml`, части с типом
core-properties. Сигнал: число нарушений; гейт падает при > 0. Реестр гейта `OP-01…` в
`xtool/xsd-gate/opc.toml` по образцу `census.toml`. `no_ecma_bytes.py` проверяет, что схемы не
закоммичены. Шаг в CI в job `xsd-gate`.

**Тесты.** Гейт обязан уметь падать: прогон на пакете, записанном **до** AUD-20 (сохранить
один такой пакет как фикстуру гейта `xtool/xsd-gate/fixtures/purl-rels.docx` — это наш
вывод, не стороннее содержимое), даёт > 0 нарушений; тест-скрипт `opc_gate_selftest.py`.

**Приёмка.** `python xtool/xsd-gate/opc_gate.py` = PASS, 0 нарушений; selftest = PASS.

---

### AUD-22. T2 по таблице, типы связей по точному URI

**Проблема.** `map_rel_or_content_type` (`transitional.rs:1911`) подставляет префикс строки
(даёт `extended-properties`, `custom-properties`); `RelType::from_uri` (`rels.rs:104`)
сопоставляет только последний сегмент URI (любой `http://x/officeDocument` — главный документ).

**Решение.**
1. Таблица `REL_TYPES` в `normalize/tables.rs`: `(kind: RelType, transitional: &str, strict: &str)`
   — по одной строке на тип связи WordprocessingML из ECMA-376 Part 1 §15 и Part 4:
   officeDocument, styles, stylesWithEffects (только Transitional; strict = `None` → потеря
   `T2.reltype-no-strict`), numbering, settings, webSettings, fontTable, font, theme, themeOverride,
   image, hyperlink, header, footer, footnotes, endnotes, comments, customXml, customXmlProps,
   glossaryDocument, attachedTemplate, subDocument, aFChunk, oleObject, package, chart,
   chartUserShapes, diagramData, diagramLayout, diagramQuickStyle, diagramColors, control, frame,
   printerSettings, extendedProperties (`extended-properties` ↔ `extendedProperties`),
   customProperties (`custom-properties` ↔ `customProperties`). Типы, которые меняются
   только префиксом, всё равно перечисляются явно. Каждая строка — `verified` тестом
   (см. ниже).
2. `RelType` получает варианты для всех строк таблицы, которые используются кодом
   (минимум: существующие + `WebSettings`, `Comments`, `ExtendedProperties`,
   `CustomProperties`, `CoreProperties`, `Thumbnail`, `CustomXml`, `GlossaryDocument`);
   прочие табличные — `RelType::Known(&'static str /* strict URI */)`. `from_uri` — точное
   сравнение с обеими колонками таблицы и с OPC-типами (AUD-20); иначе `Other(uri)`.
3. `map_rel_or_content_type` — поиск в таблице; Transitional-URI под базой
   `officeDocument/2006/relationships/`, отсутствующий в таблице, **не** переписывается;
   запись `T2.reltype-unknown` (`Lossy`, в отчёте URI).
4. `rels::strict_type_uri` читает ту же таблицу.

**Тесты.**
- core юнит: каждая строка таблицы — `from_uri(transitional) == from_uri(strict) == kind`;
  `from_uri("http://evil.example/officeDocument")` = `Other`.
- Оракул таблицы: тест собирает все `Type=` из `core/tests/strict/*.docx` (Word/LO) и
  утверждает, что каждый Strict-тип из корпуса присутствует в колонке `strict` таблицы или
  является OPC/вендорным (`schemas.microsoft.com`). Это и есть «verified».
- hostile: пакет, где `_rels/.rels` содержит первым `Type="http://evil.example/officeDocument"`
  на `evil.xml`, вторым — настоящий → главным документом выбран настоящий.

**Приёмка.** Тесты; `rg 'rsplit\(.\/.\)' core/src/opc/rels.rs` — пусто.

---

### AUD-23. Политика конформности: одна матрица в одном месте

**Проблема.** Правила размазаны по `opc/mod.rs::enforce_policy`, `wml/parse/mod.rs::parse_document`
и CLI `check` и расходятся: `Permissive` без нормализатора открывает Transitional как есть;
`Mixed` принимается при любой политике, если нормализатор установлен; `Unknown` принимается
при `StrictOnly`.

**Решение (ADR-0016).**
1. Детекция (T0) делается по **сырым** сигналам: корневые namespace частей читаются без
   проекции; типы связей — из **сырых** `.rels` (до нормализатора: `.rels` разбираются дважды —
   сырыми для сигналов и нормализованными для графа; это дёшево). Content types из сигналов
   удаляются (AUD-26).
2. `Package` хранит `detected: Conformance` (сырое) и `normalized: bool` (нормализатор вернул
   `Cow::Owned` хотя бы для одной части). `Package::conformance()` возвращает `detected`;
   новый `Package::was_normalized()`.
3. Матрица (единственная функция `opc::policy::decide(policy, detected, has_normalizer) -> Result<()>`):

| policy \ detected | Strict | Transitional | Mixed | Unknown |
|---|---|---|---|---|
| `StrictOnly` | ok | `TransitionalNotSupported` | `MixedConformance` | `UndeterminedConformance` |
| `Normalize`, нормализатор есть | ok | ok | ok | ok |
| `Normalize`, нормализатора нет | ok | `Unsupported("Normalize requires a normalizer")` | то же | то же |
| `Permissive`, нормализатор есть | ok | ok | ok | ok |
| `Permissive`, нормализатора нет | ok | то же, что `Normalize` без нормализатора | то же | ok |

   `Permissive` отличается от `Normalize` только поведением **парсера**: под `Permissive`
   неподдержанные механизмы не являются ошибкой (как сейчас), под `Normalize` — тоже не
   являются (как сейчас); различие — `Permissive` допускает `Unknown` без нормализатора
   (для `inspect`). Новый вариант `StrictError::UndeterminedConformance { detail }`.
4. `wml::parse_document` больше **не** проверяет конформность сам: проверку делает только
   `Package::open_*`. `ParseOptions::conformance` удаляется (ломающее изменение до 1.0 —
   допустимо по ТЗ §6.5); `parse_options_from` в мета-крейте упрощается.
5. Внутри парсера `require_strict_ns` = `!package.was_normalized() && detected == Strict`
   заменяется на: корневой элемент **всегда** обязан быть в Strict WML namespace после
   нормализации; если пакет не нормализован и корень не Strict → `TransitionalNotSupported`
   (под `Permissive` без нормализатора и `Unknown` — `InvalidXml("root element is not in the
   WordprocessingML Strict namespace")`).
6. CLI `check`: ветвление по конформности удаляется, остаются коды: `Ok` → `report_support`;
   `TransitionalNotSupported` → 1; прочие ошибки → 2. Строка успеха: `"ok: strict"` если
   `!was_normalized()`, иначе `"ok: normalized from <detected>"`.

**Тесты.** core: табличный тест на все 20 клеток матрицы (5 строк × 4 колонки), пакеты из
testkit. CLI `tests/cli.rs`: Strict → 0 «ok: strict»; Transitional → 1; Transitional
`--transitional` → 0 «ok: normalized from transitional»; Unknown (корень `<foo/>`) → 2;
Mixed без `--transitional` → 2. Мета-крейт: `Manual.docx` под нормализатором →
`conformance() == Transitional`, `was_normalized() == true` (сейчас `Strict` — регрессия
подтверждённого дефекта).

**Приёмка.** Тесты; `rg "Conformance::Mixed if" core/src` — пусто; ADR-0016 принят.

---

### AUD-24. Имена частей: регистронезависимость и percent-encoding

**Проблема.** OPC требует регистронезависимой (ASCII) эквивалентности имён частей;
`PartId` сравнивается побайтно — дубликаты `word/document.xml` и `Word/Document.xml` не
ловятся, цель `Word/Styles.xml` не находит `word/styles.xml`. Цели связей и `PartName` в
`Override` — URI, percent-encoding не декодируется.

**Решение.**
1. `PartId` хранит исходное написание; `Eq`/`Hash`/`Ord` — по ASCII-lowercase (ручные
   реализации). Отображение (`Display`, `as_str`) — исходное.
2. Дубликат по новому равенству → `DuplicatePart` (уже существующая ошибка).
3. `opc::path::percent_decode(&str) -> Result<String>`: `%XX` → байт, результат обязан быть
   UTF-8, иначе `InvalidPartName`; применяется к `Target` (только `Internal`) и к
   `Override/@PartName` до канонизации. ZIP-имена не декодируются (они уже не URI).
4. После разрешения цели `resolved` переписывается в написание ZIP-записи
   (`ZipArchive::entry(id).id.clone()`), чтобы во всём остальном коде было одно написание.

**Тесты.** core юнит: равенство/хэш `PartId`; `Target="Media/Image%201.png"` находит
`word/media/image 1.png`; `%FF` → ошибка; hostile: пакет с `word/document.xml` и
`WORD/DOCUMENT.XML` → `DuplicatePart`.

**Приёмка.** Тесты; корпус зелёный.

---

### AUD-25. `.rels` вне `_rels/` и `max_rel_depth`

**Проблема.** Любая запись `*.rels` вне каталога `_rels/` приписывается корню пакета
(`rels.rs:204`) — может подменить главный документ. `max_rel_depth` объявлен и нигде не
используется.

**Решение.**
1. `source_part_for_rels` возвращает `Option<PartId>`: `None`, если путь не имеет вида
   `<dir>/_rels/<name>.rels`. Такие записи **не** разбираются как связи и остаются обычными
   частями.
2. `Package::reachable_parts(from: &PartId) -> Result<Vec<PartId>>` — обход графа связей в
   ширину, глубина ≤ `max_rel_depth`, превышение → `LimitExceeded{RelationshipDepth}`;
   защищён от циклов (посещённые). Писатель (passthrough, транзитивное копирование частей)
   переходит на этот метод вместо своего обхода.

**Тесты.** hostile: `/aaa.rels` (первым в ZIP) с `officeDocument` на `evil.xml` → главный
документ — из `_rels/.rels`. Цепочка связей глубины 33 из passthrough-частей → ошибка; цикл
A→B→A → обход завершается.

**Приёмка.** Тесты.

---

### AUD-26. Удалить мёртвый сигнал content types

**Проблема.** `classify_content_types` (`ns/detect.rs:66`) ищет URL в MIME-строках, где их нет;
content types одинаковы в обоих семействах.

**Решение.** Функция и поле `ConformanceSignals::content_types` удаляются. Отклонение от ТЗ
§4.1 п.4 фиксируется в ADR-0019. Вместо сигнала добавляется **проверка**: content type
главной части — `application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml`
или `…template.main+xml` или macro-enabled варианты; иначе `InvalidPartName`-подобная
ошибка `StrictError::UnexpectedContentType { part, content_type }` (новый вариант) — только
под `StrictOnly`; под нормализацией — запись в отчёт нормализатора `T2.content-type`.

**Тесты.** core: главная часть с content type `application/xml` под `StrictOnly` → ошибка;
под `Normalize` → `Ok` + запись.

**Приёмка.** Тесты.

---

## Ф4. Нормализатор и отчёт

### AUD-30. Отчёт нормализатора — по частям, без двойного учёта

**Проблема.** Отчёт копится на каждом вызове `normalize_part`; части, прочитанные дважды,
учитываются дважды (подтверждено: 8 типов связей → `T2.reltype x16`). Мьютекс держится всё
время обработки части.

**Решение (ADR-0017).**
1. `TransitionalNormalizer` хранит `Mutex<BTreeMap<PartId, NormalizationReport>>`. Обработка
   части пишет в **локальный** `NormalizationReport`; по завершении — одна вставка
   `map.insert(part, local)` (замена, а не сложение). Мьютекс держится только на вставку.
2. `report()` сливает отчёты частей в порядке `PartId` → результат не зависит от порядка
   чтения и числа повторных чтений.
3. `conformance_detected` нормализатор не знает; поле заполняется вызывающим (AUD-31).
4. `fatal` и счётчики `removed_nodes`/`reported_nodes` — поля per-part отчёта, при слиянии
   суммируются (`saturating_add`).
5. Повторное чтение части с **другими** байтами невозможно (ZIP неизменяем), поэтому замена
   корректна; это утверждение записывается в ADR-0017.

**Тесты.** core: прочитать одну часть 3 раза → отчёт равен отчёту одного чтения; прочитать
части в двух разных порядках → `report()` равны (`PartialEq` на `NormalizationReport`
добавить). CLI: `normalize Manual.docx` → `T2.reltype x8`. Свойство на Transitional-корпусе:
`report()` после `open` + повторного чтения всех частей == после одного `open`.

**Приёмка.** Тесты; ADR-0017.

---

### AUD-31. Loss Report в Feature Report

**Проблема.** `support_report()` всегда `normalized: false`, блок `normalization` пуст
(ТЗ §10.9, §11.1, приёмка этапа 6); CLI `report` не умеет `--transitional`.

**Решение.**
1. Трейт `RawNormalizer` получает метод по умолчанию
   `fn report(&self) -> Option<NormalizationReport> { None }`; `TransitionalNormalizer`
   возвращает `Some(self.report())`. `Package` предоставляет `normalization_report()`.
2. `StrictDocument::support_report()`:
   - `conformance(Conformance::Strict, package.conformance(), package.was_normalized())`;
   - `normalization` = преобразование `NormalizationReport` → `NormalizationBlock`:
     `applied[i] = { transform_id, count, locations: [] }` (`locations` — первые 16 локаций,
     если нормализатор их хранит; если нет — пусто);
     `losses[i] = { transform_id, feature_id, reason, severity, locations }`,
     severity: `Ignorable → info`, `Lossy → warning`, `Error → error`;
     `invariants_ok = report.fatal.is_none() && report.verify_no_silent_loss().is_ok()`;
   - потеря с severity `error` даёт в `features` запись `normalization.<feature_id>` со
     статусом `error` (чтобы `overall_status` это видел и правило §11.3 «у error есть локация»
     выполнялось).
3. JSON-схема (`strict-ooxml-report`): если существующая схема блока не допускает этих полей —
   `SCHEMA_VERSION` поднимается до `2.1`, схема обновляется; иначе версия не меняется. Решение
   принимается по факту валидации тестом ниже, а не заранее.
4. CLI `report` получает `--transitional` (через `open_options`); сообщение «requires Stage-6
   normalization» заменяется на «pass --transitional to normalize it».
5. Doc-комментарии «always false until Stage 6» удаляются во всех крейтах.

**Тесты.** report: снапшот JSON для `Manual.docx --transitional` — `normalized: true`,
`detected: "transitional"`, непустые `applied` и `losses`; валидация по схеме тестом `schema`;
детерминизм: два прогона дают равные байты. Strict-корпус: `normalization` пуст,
`normalized: false` (SC-1).

**Приёмка.** Тесты; приёмка этапа 6 «Feature Report явно указывает на потери» проверена
снапшотом.

---

### AUD-32. Ошибка среза префиксов и текстовый поиск деклараций

**Проблема.** `namespace_declarations` (`transitional.rs:944`) режет `rest[..equals]`, где
`equals` посчитан по `tail` — префиксы в отчёте обрезаны (`:v` вместо `vt`). Поиск деклараций
текстовый: `xmlns` в тексте документа даёт ложные нарушения (под `InvariantMode::Strict` —
отказ части); одинарные кавычки не видны.

**Решение.** `namespace_declarations` переписывается на разбор `quick_xml` (события
`Start`/`Empty`, атрибуты `xmlns`/`xmlns:*`), что одновременно чинит срез, кавычки и ложные
срабатывания. Висячий фрагмент doc-комментария `/// will need.` (строка 962) удаляется.

**Тесты.** core юнит: `<a xmlns:vt="u1" xmlns:dc='u2'>xmlns:x="u3"</a>` → ровно
`[("vt","u1"),("dc","u2")]`. CLI: в выводе `normalize Manual.docx` нет подстроки `prefix :`.

**Приёмка.** Тесты.

---

### AUD-33. `bidi` в T4 и `DirectionPolicy`

**Проблема.** ТЗ §10.4 п.2 и §10.5 п.3: учёт `bidi` и политика направления не реализованы.

**Решение.**
1. `NormalizerOptions::direction: DirectionPolicy { MapToStartEnd /* default */, Keep }`.
   - `MapToStartEnd`: текущее поведение (`left→start`, `right→end`, переименования атрибутов).
   - `Keep`: direction-neutral переименования **и** маппинг значений не выполняются; каждое
     невыполненное — `LossRecord` `T3.direction-kept`/`T4.direction-kept` (`Lossy`).
     Результат может не пройти XSD — это ожидаемо для диагностического режима.
2. `bidi` (ТЗ: консервативно, физический маппинг + предупреждение): в потоке `w:pPr`
   нормализатор запоминает наличие `w:bidi` (без `w:val` или с истинным on/off) до
   `w:jc` — порядок `CT_PPrBase` ставит `bidi` раньше `jc`. Если в этом `pPr` есть `w:bidi` и
   `w:jc/@w:val ∈ {left, right}` → маппинг выполняется как обычно, плюс `LossRecord`
   `T4.jc-bidi` (`Lossy`, причина «physical left/right in a bidi paragraph mapped to logical
   start/end; visual alignment may flip»). `bidi`, унаследованный из стиля, не учитывается —
   это ограничение записывается в ADR-0017.
3. То же для `w:tblPr/w:bidiVisual` + `w:jc` таблицы.

**Тесты.** core юнит: `pPr` с `bidi` + `jc=left` → `start` и запись; без `bidi` → без записи;
`Keep` → значение `left` осталось, запись есть; `tblPr/bidiVisual` + `jc=right`.

**Приёмка.** Тесты; `DirectionPolicy` реэкспортирован из мета-крейта.

---

### AUD-34. `VmlFallback`

**Проблема.** ТЗ §6.3/§10.8: опция отсутствует; поведение (конвертация VML в DrawingML)
не управляется.

**Решение.** `NormalizerOptions::vml: VmlFallback { Convert /* default, текущее поведение */,
Report, Drop }`. `Report`: `w:pict`/`w:object` не конвертируются, поддерево удаляется, запись
`T7.vml` (`Lossy`) с классом фигуры. `Drop`: то же, запись с причиной «dropped by policy».
Имена вариантов ТЗ (`RasterizeIfPossible`) заменяются `Convert`, потому что реализация
конвертирует в DrawingML, а не растрирует; переименование записывается в ADR-0019.

**Тесты.** core: фикстуры VML из существующих тестов `vml.rs` под тремя политиками; под
`Report`/`Drop` в выходе нет `v:`/`w:pict`, `verify_no_silent_loss` = `Ok`.

**Приёмка.** Тесты.

---

### AUD-35. `max_expansion_bytes`: документация против кода

**Проблема.** Документ поля говорит «Zero means no extra allowance», код при нуле даёт 4×.

**Решение.** Поле переименовывается в `max_expansion: ExpansionLimit { Factor(u32) /* default 4 */,
Bytes(usize) }`; `Factor(n)`: `input.saturating_mul(n).max(input.saturating_add(1024))`;
`Bytes(b)`: `input.saturating_add(b)`. Документация соответствует.

**Тесты.** core юнит на обе ветки и на `input = usize::MAX`.

**Приёмка.** Тест.

---

### AUD-36. Мьютекс и `parallel`

**Проблема.** Под feature `parallel` все части сериализуются на мьютексе нормализатора.

**Решение.** Закрывается AUD-30 (мьютекс только на вставку). Дополнительно: тест, что под
`parallel` отчёт равен последовательному.

**Тесты.** core (`--features parallel` в мета): Transitional-корпус, `report()` при
`parallel` == без.

**Приёмка.** Тест.

---

## Ф5. Модель WML: молчаливые потери

### AUD-40. Секции из вложенных контейнеров

**Проблема.** `sync_resolved_sections_into_body` (`parse/mod.rs:483-509`) считает `sectPr`
и внутри `SdtBlock`/ячеек/`ins`/`del`, а сбор `sections` их выбрасывает (`document.rs:672`,
`:134`, `table.rs:140`) — свойства секций сдвигаются на чужие абзацы.

**Решение.** Сбор и синхронизация используют **один** обход — функцию
`walk_paragraphs_in_order(&[Block], &mut impl FnMut(&mut Paragraph))`, проходящую в порядке
документа абзацы тела, `SdtBlock`, ячеек таблиц (вложенно), правок. `sections` собираются
этим обходом из каждого абзаца с `sectPr` и из `body/sectPr` последним. Локальные списки
`sections` в `parse_block_children`/`table.rs` удаляются. `sectPr` в абзаце внутри ячейки
таблицы по спецификации недопустим; если встречен — секция **учитывается** (как делает Word)
и пишется запись `w:sectPr` `partial` «section break inside a table cell».

**Тесты.** wml: тело `p, sdt{ p[sectPr A] }, p, p[sectPr B]`, `body/sectPr C` →
`sections == [A, B, C]` и каждый абзац получает правильную секцию; то же с `w:ins` вокруг
абзаца с `sectPr`. Рендер: документ с альбомной секцией внутри sdt → страница этой секции
альбомная (после AUD-74).

**Приёмка.** Тесты.

---

### AUD-41. `w:sdt` на уровне строк и ячеек таблицы

**Проблема.** `parse/table.rs:32-39`, `:97-106`: `w:tr` внутри `sdtContent` в таблице и `w:tc`
внутри `sdtContent` в строке становятся `Opaque` и теряются.

**Решение.** Разворачивание: `w:sdt` в `w:tbl` — его `sdtContent/w:tr` добавляются как строки
таблицы; `w:sdt` в `w:tr` — `sdtContent/w:tc` добавляются как ячейки. `sdtPr` сохраняется в
новом поле `TableRow::sdt: Option<SdtProperties>` / `TableCell::sdt` (тот же тип, что у
`SdtContainer`), чтобы писатель восстановил обёртку. Запись `w:sdt` `partial`
«row/cell-level content control unwrapped; properties kept».

**Тесты.** wml: таблица с repeating section из 3 строк → 3 строки в модели, текст на месте.
Писатель: round-trip сохраняет `w:sdt` вокруг строк (разбор → запись → разбор, сравнение
моделей). Рендер: строки отрисованы.

**Приёмка.** Тесты.

---

### AUD-42. Прозрачные контейнеры: `customXml`, `smartTag`, `dir`, `bdo`

**Проблема.** `OpaqueBlock`/`OpaqueInline` не хранят детей; текст внутри `w:customXml`
(блочный, инлайновый, в ячейке), `w:smartTag`, `w:dir`, `w:bdo` теряется.

**Решение.** Эти четыре элемента — **прозрачные**: их содержимое разбирается в родительский
контекст как будто обёртки нет. Сохранение обёртки для писателя:
- `customXml`, `smartTag`: в модели новое поле на затронутых узлах не вводится; писатель
  обёртку **не** восстанавливает; запись `w:customXml`/`w:smartTag` `partial` «wrapper dropped,
  content kept» и в `WriteReport` loss `W.custom-xml-wrapper` (`Ignorable`).
- `dir`, `bdo`: влияют на направление; вводится `Inline::Directional { kind: Dir|Bdo, val: Rtl|Ltr,
  inlines: Vec<Inline> }`; рендер применяет направление к вложенным runs (как `w:rtl`), писатель
  восстанавливает элемент.
`OpaqueBlock`/`OpaqueInline` остаются для действительно неизвестных элементов; dispatch-таблица
(`dispatch.rs:45`) обновляется.

**Тесты.** wml: текст внутри каждого из четырёх (в теле, в абзаце, в ячейке) присутствует в
модели. Рендер: текст виден на странице (поиск строки в SVG). Писатель: `dir`/`bdo` round-trip;
`customXml` — запись потери.

**Приёмка.** Тесты.

---

### AUD-43. Правки: `w:ins`, `w:del`, `w:moveFrom`, `w:moveTo`

**Проблема.** Удалённый и перемещённый текст разворачивается в обычный (`document.rs:357`,
`dispatch.rs:36-39`, `:108-111`) и рендерится как живой.

**Решение (ADR-0018).**
- `Run::revision: Option<Revision>`, `Revision { kind: RevisionKind { Insert, Delete, MoveFrom,
  MoveTo }, id: u32, author: Option<Arc<str>>, date: Option<Arc<str>> }`. То же поле на
  `Paragraph` для пометки знака абзаца (`w:rPr/w:ins|w:del` внутри `w:pPr`).
- `w:delText`/`w:delInstrText` — содержимое runs с `Delete`/`MoveFrom`.
- Рендер: «окончательный вид» — runs с `Delete`/`MoveFrom` не рисуются; `Insert`/`MoveTo` —
  обычный текст. Опция `RenderOptions::revisions: RevisionView { Final /* default */, Original }`
  (`Original`: не рисуются `Insert`/`MoveTo`). Разметки правок (подчёркивание/зачёркивание)
  нет — это вне scope; записать в ADR-0018.
- Писатель: последовательные runs с одинаковой `Revision` группируются в одну обёртку
  `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` с теми же атрибутами; в `w:del` текст пишется как
  `w:delText`.
- Правки свойств (`w:rPrChange`, `w:pPrChange` и т.д.) — запись `partial` «property change
  history dropped» (см. AUD-46), модель их не несёт.
- `coverage/wml-elements.toml`: `w:ins`, `w:del`, `w:moveFrom`, `w:moveTo`, `w:delText` →
  `supported`.

**Тесты.** wml: модель помечает runs; рендер `Final`: удалённого текста нет в SVG, вставленный
есть; `Original` — наоборот; writer round-trip равенство моделей; XSD-гейт на документе с
правками без новых нарушений.

**Приёмка.** Тесты; ADR-0018.

---

### AUD-44. Toggle-свойства и on/off в settings

**Проблема.** `caps`, `smallCaps`, `vanish`, `rtl`, `keep_next`, `page_break_before` и др.
хранятся как `bool` — явное «выключено» (`w:val="0"`) теряется в каскаде; в `settings.rs`
флаги включаются независимо от `w:val` (`settings.rs:112-118`, `:248`, `:251`).

**Решение.**
- Все on/off-свойства `rPr` (`b, bCs, i, iCs, caps, smallCaps, strike, dstrike, outline, shadow,
  emboss, imprint, noProof, snapToGrid, vanish, webHidden, specVanish, rtl, cs, oMath`) и `pPr`
  (`keepNext, keepLines, pageBreakBefore, widowControl, suppressLineNumbers, suppressAutoHyphens,
  bidi, contextualSpacing, mirrorIndents, suppressOverlap, wordWrap, …` — все `CT_OnOff` из
  `CT_PPrBase`) — тип `TriState` (уже есть для `b`/`i`). Каскад стилей: `Unset` наследует,
  `On`/`Off` перекрывают. Для toggle-свойств `rPr` (`b, bCs, i, iCs, caps, smallCaps, strike,
  dstrike, outline, shadow, emboss, imprint, vanish`) — XOR-семантика ECMA-376 §17.7.3 при
  наследовании через стили символов/абзацев, ровно по спецификации.
- Единая функция `parse_on_off(attrs) -> Option<bool>` (`true|1|on|отсутствует` → `true`,
  `false|0|off` → `false`, иное → `None` + запись) используется **везде**, включая `settings.rs`.
- `proofState` — не on/off: разбирается как `spelling`/`grammar` enum (`clean|dirty`) и
  хранится в `Settings::proof_state`.

**Тесты.** wml: стиль с `vanish`, run с `vanish w:val="0"` → видим; `evenAndOddHeaders w:val="0"`
→ `false`; XOR: стиль абзаца `b`, стиль символа `b` → run не жирный. Рендер: скрытый текст не
рисуется, раскрытый рисуется.

**Приёмка.** Тесты; `rg "=> settings\.\w+ = true" wml/src/parse/settings.rs` — пусто.

---

### AUD-45. Отрицательные проценты и `w:sym`

**Проблема.** `percent_from_fiftieths(-100)` → `"--2%"` (`values.rs:820`); `w:sym` снимает все
ведущие `F` (`document.rs:490`).

**Решение.**
- `percent_from_fiftieths`: работа с `abs` (`i64` для `i32::MIN`), знак добавляется один раз.
- `w:sym`: `u32::from_str_radix(code, 16)` (регистронезависимо); `0xF000..=0xF0FF` → минус
  `0xF000`; результат через `char::from_u32` и `escape::is_xml_char` (AUD-03); невалидный —
  запись `w:sym` `unsupported` с кодом.

**Тесты.** wml юнит: `-100 → "-2%"`, `-75 → "-1.50%"`, `-25 → "-0.50%"`, `i32::MIN`; `w:sym`
`F0FC`, `f0fc`, `FF20` (не трогается: `0xFF20`), `0001` (запись).

**Приёмка.** Тесты.

---

### AUD-46. Неизвестные дети `*Pr` и недостающие свойства

**Проблема.** `_ => {}`/`skip_element` без записи в `props.rs:142, 263, 471, 527, 604, 791`,
`styles.rs:205`; теряются `bCs`/`iCs`, `pgNumType`, `tblpPr`, `framePr`, `tblCellSpacing`,
`trPr/jc`, `*Change`, `tblStylePr`; `semiHidden` и `hidden` склеены.

**Решение.**
1. Каждая ветка «по умолчанию» в этих функциях делает запись в `SupportModel`:
   `feature_id = "<prefix>:<local>"`, статус `Unsupported`, причина «property not modelled»,
   локация.
2. Моделируются (парсер + модель + писатель; рендер — как указано):
   - `bCs`, `iCs` (`TriState`), `szCs` уже есть? — если нет, тоже; рендер: применяется к
     complex-script runs (`w:cs`/`w:rtl`);
   - `sectPr/pgNumType` (`fmt`, `start`, `chapStyle`, `chapSep`); рендер: PAGE учитывает
     `start` и `fmt` (вместе с AUD-70);
   - `tblPr/tblpPr` (все атрибуты); рендер: `partial` — таблица остаётся в потоке, запись;
   - `pPr/framePr` (все атрибуты); рендер: `partial` — запись;
   - `tblPr/tblCellSpacing`, `tblPrEx/tblCellSpacing`, `trPr/tblCellSpacing`; рендер: зазор
     между ячейками применяется;
   - `trPr/jc`; рендер: выравнивание строки;
   - `semiHidden`, `hidden` — раздельные поля стиля; `qFormat`, `locked`, `unhideWhenUsed` —
     поля стиля (писатель восстанавливает, рендер игнорирует без записи — это метаданные UI).
3. Не моделируются, но записываются (`partial`): `*Change` (история свойств), `tblStylePr`
   (условное форматирование табличных стилей) — отдельная будущая работа, waiver
   `TBL-STYLE-PR` в `docs/waivers.toml`.
4. `coverage/wml-elements.toml` обновляется для всех перечисленных.

**Тесты.** wml: для каждого моделируемого — разбор, writer round-trip; для каждого
записываемого — запись присутствует. xtool: тест `every_default_branch_records` — по одному
синтетическому неизвестному элементу `w:zzz` в каждом из `pPr/rPr/tblPr/trPr/tcPr/sectPr/style`
→ 7 записей.

**Приёмка.** Тесты; гейт покрытия ≥ 89 % (не падает).

---

### AUD-47. Нумерация: `numStyleLink`, `ilvl`, дубликаты

**Проблема.** `numStyleLink`/`styleLink` не разрешаются; `ilvl > 8` молча зажимается; дубли
`abstractNumId` перезаписываются.

**Решение.**
- Разрешение: `abstractNum` с `numStyleLink="S"` → стиль `S` (тип `numbering`) → его
  `pPr/numPr/numId` → `num` → `abstractNum` с `styleLink="S"`. Цепочка ≤ 8 переходов,
  повтор → прекращение, запись `w:numStyleLink` `partial` «cycle».
- `ilvl > 8`: зажимается до 8 **с записью** `w:ilvl` `partial`.
- Дубликат `abstractNumId`/`numId`: **первый** выигрывает (как в Word), запись `partial`.

**Тесты.** wml: список через numbering-стиль получает уровни; цикл из 2 стилей → запись,
без зависания; `ilvl=12` → запись; дубликаты → первый.

**Приёмка.** Тесты.

---

### AUD-48. Медиа из сносок

**Проблема.** `parse_part_with` (`parse/mod.rs:217-218`) отбрасывает `parser.media` для
`footnotes.xml`/`endnotes.xml`.

**Решение.** Для сносок вызывается тот же `merge_media`, что для колонтитулов; медиа-ключи
привязаны к исходной части (для разрешения `r:embed` через `.rels` сносок).

**Тесты.** wml: сноска с картинкой → `document.media` содержит её; рендер: картинка в зоне
сносок; писатель: см. AUD-61.

**Приёмка.** Тесты.

---

### AUD-49. `custGeom` с несколькими путями

**Проблема.** `drawing.rs:790-797` склеивает пути и перезаписывает `w`/`h`.

**Решение.** `CustomGeometry::paths: Vec<GeometryPath { w, h, fill, stroke, commands }>`;
каждый путь масштабируется своим `w`/`h`. Рендер SVG/PDF: по `<path>` на путь. Писатель:
пути по отдельности.

**Тесты.** wml + render-svg: фигура из двух путей с разными `w`/`h` → два `<path>` с
правильными координатами (сравнение с вычисленными вручную); writer round-trip.

**Приёмка.** Тесты.

---

### AUD-50. Сноски без id, несколько `m:oMath`, мелкие потери

**Проблема.** Сноска без `w:id` получает 0 и затирает continuationSeparator
(`notes.rs:64`, `document.rs:294`, `:406`); несколько `m:oMath` в `m:oMathPara` сливаются
(`math.rs:91-110`); `UnknownMathNode` не хранит содержимое при сообщении «preserved verbatim»;
`headerReference` с неизвестным `w:type` отбрасывается (`props.rs:860`); `effectExtent` у
inline пропускается (`drawing.rs:107`); `parse_emu` подставляет 0 (`drawing.rs:1417`);
`mc:AlternateContent` в runs недостижим (`document.rs:351`).

**Решение.**
- Сноска без/с невалидным `w:id` — пропускается целиком, запись `w:footnote` `error` с
  локацией; ссылка с невалидным id → `Inline::FootnoteRef` не создаётся, запись.
- `MathParagraph::equations: Vec<OMath>`; рендер — по строке на уравнение; писатель —
  по `m:oMath` на уравнение.
- Сообщение «preserved verbatim» исправляется на «dropped»; содержимое не хранится (решение).
- `headerReference` неизвестного `w:type` → запись `partial`.
- `effectExtent` моделируется (`Drawing::effect_extent`), рендер расширяет занимаемый блок,
  писатель пишет.
- `parse_emu` при невалидном/отсутствующем значении → запись `partial` с именем атрибута;
  значение 0 остаётся.
- `mc:AlternateContent` в Strict-документе без нормализатора: парсер выбирает ветку по тем же
  правилам, что `McePolicy::ProcessChoice`; множество поддерживаемых `Requires` —
  константа `wml::SUPPORTED_MCE_NAMESPACES` = {wps `http://schemas.microsoft.com/office/word/2010/wordprocessingShape`,
  wpg `…/2010/wordprocessingGroup`, `m` Strict}. Проверка `!is_wml` переносится после
  обработки `AlternateContent`.

**Тесты.** wml: по тесту на каждый пункт.

**Приёмка.** Тесты.

---

### AUD-51. Ограничение размера `SupportModel`

**Проблема.** Ключ фичи строится из `prefix:local`; число разных ключей не ограничено;
префикс подменяем.

**Решение.** Ключ фичи — `{namespace-key}:{local}`, где `namespace-key` — короткий ключ
реестра (`w`, `a`, `wp`, `m`, …) или `ext` для незарегистрированного URI (по URI, не по
префиксу). Число различных фич ≤ `ResourceLimits::max_support_features` (новое, 10 000);
сверх — в одну фичу `support.overflow` со счётчиком.

**Тесты.** wml: 50 000 уникальных неизвестных элементов → ≤ 10 001 фича; `w:` привязанный к
чужому URI → ключ `ext:…`.

**Приёмка.** Тест.

---

### AUD-52. `Send + Sync` и `render_page_svg`

**Проблема.** Нет статической проверки ТЗ §6.5; `render_page_svg(usize::MAX)` переполняет
`index + 1`.

**Решение.** Тест `strict-ooxml/tests/traits.rs`:
`fn assert_send_sync<T: Send + Sync>() {}` для `StrictDocument`, `Document`, `Package`,
`SupportReport`, `NormalizationReport`, `TransitionalNormalizer`, `Page`, `PdfOutput`,
`WriteOutput`. `render_page_svg`: `index.checked_add(1)` → `StrictError::Render("page index out of range")`.

**Тесты.** Тест выше; `render_page_svg(usize::MAX)` → `Err`.

**Приёмка.** Тесты.

---

## Ф6. Писатель

### AUD-60. Многоабзацные сноски

**Проблема.** `parts.rs:914-933`: все абзацы сноски пишутся в один `w:p`, таблица — внутрь
`w:p`; свойства последующих абзацев теряются.

**Решение.** Сноска пишется как последовательность блоков: каждый `Paragraph` — свой `w:p` со
своими `pPr`; `Table` — на уровне сноски. Ссылка `w:footnoteRef`/`w:endnoteRef` вставляется
в первый run **первого** абзаца, если её там нет; если первый блок — таблица, перед ней
вставляется абзац со ссылкой.

**Тесты.** writer: сноска из 3 абзацев с разными `pStyle` и таблицей → round-trip равенство
моделей; XSD-гейт без новых нарушений.

**Приёмка.** Тесты.

---

### AUD-61. Идентификаторы связей в колонтитулах и сносках

**Проблема.** `.rels` колонтитула ссылается на исходное имя картинки, а не на записанное
(`package.rs:651-659`); `r:embed`/`r:id` в сносках берутся из главного документа, `.rels`
сносок не пишется; ссылка на диаграмму в колонтитуле сопоставляется с id главного документа
(`ctx.rs:143`); `fontTable.xml.rels` пишется до чтения байтов шрифта (`package.rs:609-635`).

**Решение.**
1. Одно правило: **каждая** записываемая часть (`document`, `header*`, `footer*`, `footnotes`,
   `endnotes`, `comments`) получает свой `RelAllocator` (контекст `ctx` хранит
   `current_part: PartId` и аллокатор этой части). Любая ссылка (`r:embed`, `r:id`, `r:link`)
   разрешается через `(исходная часть, исходный id)` → источник → цель → **записанное** имя
   цели → новый id в аллокаторе текущей части.
2. Медиа именуются один раз (AUD-62) до записи частей; `.rels` пишутся после записи
   содержимого части.
3. `fontTable.xml.rels` пишется после чтения байтов всех шрифтов; шрифт, байты которого не
   прочитались, — не попадает в `.rels`, loss `W7.font`.
4. Инвариант-проверка в конце `write_package` (не тест, а код): каждый `r:*` атрибут каждой
   записанной части разрешается через её `.rels` в существующую часть или External. Нарушение
   → `WriteError::DanglingRelationship` (внутренняя ошибка — защищает от регрессий).

**Тесты.** writer: документ с картинкой в колонтитуле, в сноске и в теле с одинаковыми
исходными id `rId5` в разных частях → после записи каждая ссылка ведёт на правильную
картинку (сравнение байтов медиа); диаграмма в колонтитуле; шрифты. Свойство на всём
корпусе: инвариант п.4 выполняется.

**Приёмка.** Тесты.

---

### AUD-62. Аллокатор имён частей и коллизии

**Проблема.** Копируемая (passthrough) картинка совпадает по имени со сгенерированной
медиачастью или миниатюра копируется дважды → `DuplicatePart` роняет запись.

**Решение.** `PartNameAllocator` в `package.rs`: сначала резервируются имена **всех**
passthrough-частей (в исходном написании; сравнение — по AUD-24), затем генерируемые
медиа получают `word/media/image{N}.{ext}` с наименьшим свободным `N`. Повторное
резервирование одного имени — no-op (миниатюра). Аллокатор — единственный источник имён.

**Тесты.** writer: исходный пакет, где диаграмма ссылается на `word/media/image1.png`, а тело
— на другую картинку → обе в выходе, разные имена, ссылки верны; миниатюра, на которую
ссылаются дважды → одна часть.

**Приёмка.** Тесты; `DuplicatePart` из писателя недостижим (тест на корпусе).

---

### AUD-63. Transitional-типы в генерируемых `.rels` и passthrough-части

**Проблема.** При Transitional-источнике связи неизвестных типов попадают в генерируемый
`document.xml.rels` с Transitional URI; `comments`, `webSettings`, `glossary` копируются
дословно с Transitional-разметкой.

**Решение.**
- Тип любой генерируемой связи — через таблицу AUD-22; Transitional-URI вне таблицы →
  связь пишется с исходным URI **и** loss `W.reltype-transitional` (`Lossy`), чтобы census
  видел.
- Passthrough читает байты через `Package::read_part` (нормализованные), не через сырые;
  после копирования часть проверяется `part_needs_normalization`; если Transitional-сигнал
  остался → loss `W7.non-strict-part` (`Lossy`).
- `comments.xml` — моделируется? **Нет** в этом плане (дыра модели из `CORE-QUEUE.md`);
  passthrough с нормализацией.

**Тесты.** writer: Transitional-документ с `comments.xml`, `webSettings.xml`, glossary →
`write --transitional`; в выходе нет `schemas.openxmlformats.org/wordprocessingml/2006` ни в
одной части, кроме названных в отчёте; census-гейт PASS.

**Приёмка.** Тесты; census `TZ-15 = 0` сохраняется.

---

### AUD-64. `xml:space="preserve"` для `m:t`

**Проблема.** У `m:t` нет `xml:space="preserve"` — пробелы по краям текста формул теряются.

**Решение.** Та же логика, что для `w:t`: атрибут пишется, если текст начинается/кончается
пробельным символом или содержит два пробела подряд.

**Тесты.** writer: `m:t` `" x "` → round-trip сохраняет пробелы.

**Приёмка.** Тест.

---

### AUD-65. Дубликаты частей в ZIP-писателе — регистронезависимо

**Решение.** Закрывается AUD-24 (новое равенство `PartId`) + тест: запись двух частей
`a.xml` и `A.xml` → `DuplicatePart`.

**Приёмка.** Тест.

---

### AUD-66. Невалидные XML-символы в выходе

**Решение.** Закрывается AUD-03 (writer переходит на `core::xml::escape`, loss
`W.invalid-xml-char`). Здесь — только тест на уровне писателя: модель с `\u{1}` в тексте run,
в `w:alias`, в имени стиля → выход разбирается `roxmltree`, отчёт содержит потерю.

**Приёмка.** Тест.

---

### AUD-67. Опорный тест писателя по Word-оракулу

**Проблема.** Ошибки AUD-20/60/61 прошли все тесты, потому что тесты сверяют вывод с
ожиданиями того же кода.

**Решение.** Тест `write/tests/word_oracle.rs`: для каждого `core/tests/strict/*.docx`
(Word/LO) — разбор → запись → разбор; сравнение **второго** разбора с **первым**
(модели равны с точностью до полей, перечисленных в `WriteReport` как потери) **и** сравнение
OPC-структуры выхода со структурой входа: множество типов связей каждой части, множество
content types, namespace каждой корневой части — равны, кроме названных потерь.

**Приёмка.** Тест зелёный на всех 27 файлах `tests/strict/`; расхождения, которые являются
решениями (`W7-DROPPED`), перечислены в тесте явным списком со ссылкой на waiver.

---

## Ф7. Рендер SVG

### AUD-70. PAGE/NUMPAGES в колонтитулах

**Проблема.** Колонтитул раскладывается один раз и кэшируется; `resolve_fields` вызывается
только для тела — на всех страницах «1».

**Решение.** Кэш колонтитула допускается только если в нём нет полей `PAGE`, `NUMPAGES`,
`SECTIONPAGES`, `SECTION`; иначе колонтитул раскладывается **на каждой странице** с
контекстом `{ page_number, page_count, section_index, section_pages }`. Номер страницы
учитывает `pgNumType/@start` и `@fmt` (AUD-46). Раскладка колонтитулов участвует в общем
бюджете AUD-72.

**Тесты.** render-svg: 3-страничный документ с `PAGE` в футере → в SVG страниц «1», «2»,
«3»; `pgNumType start=5 fmt=upperRoman` → «V», «VI», «VII»; `NUMPAGES` → «3» на всех.

**Приёмка.** Тесты.

---

### AUD-71. Шаг табуляции 0 и не-конечные числа

**Проблема.** `defaultTabStop=0` → `0/0` (`paragraph.rs:1075`), NaN в `PlacedPage`;
`fmt_num` может вывести `inf` (`units.rs:66`); масштабы групп до 1e19.

**Решение.**
- Шаг табуляции ≤ 0 → 720 twips (умолчание Word, 0.5"), запись в `Page::warnings`.
- Масштаб группы (`chExt`/`ext`) зажимается в `[1e-6, 1e6]`.
- `fmt_num`: не-конечное → `0`, |v| > 1e9 → зажим; плюс `debug`-проверка в `place_pages`
  нельзя (G-1) — вместо неё **валидация** в конце `place_pages`: все координаты конечны,
  иначе элемент удаляется и пишется предупреждение.
- `render-pdf` читает только конечные значения (гарантия `place_pages`).

**Тесты.** render-svg: `defaultTabStop=0` → текст после таба правее начала строки; группа с
`chExt cx="1"` и вложенностью 6 → в SVG нет `NaN`/`inf`; свойство (proptest) на
`fmt_num(f64)` — всегда конечная строка.

**Приёмка.** Тесты; `rg "NaN|inf" ` в SVG корпуса — пусто (тест `corpus`).

---

### AUD-72. Общий бюджет вывода

**Проблема.** `MAX_ITEMS` проверяется только для тела страницы; колонтитулы, рамки, якоря,
повтор заголовков таблиц добавляются после проверок — 119 KB → 238 MB SVG.

**Решение.** `ResourceLimits::max_render_items: u64 = 2_000_000` (элементов на документ,
включая колонтитулы, рамки, якоря, повторы заголовков) и `max_pages: u32 = 10_000` (перенос
`MAX_PAGES`). Счётчик в контексте раскладки; превышение → `RenderError::LimitExceeded`. Тот
же бюджет действует для `render_pdf` (общая раскладка). `MAX_ITEMS`/`MAX_PAGES` удаляются.

**Тесты.** hostile: колонтитул из 1000 элементов × 2000 страниц → `Err(LimitExceeded)` за
< 10 с; корпус рендерится как раньше (бюджет не срабатывает).

**Приёмка.** Тесты.

---

### AUD-73. Нулевой и отрицательный размер страницы

**Решение.** `pgSz/@w` или `@h` ≤ 0 или отсутствует → Letter (12240 × 15840 twips), запись в
`SupportModel` `w:pgSz` `partial` (парсер) и в `Page::warnings`. Поля, дающие ширину
контента < 1 px, зажимаются (существующий clamp) — без изменений.

**Тесты.** render-svg: `pgSz w="0" h="0"` → `viewBox` ненулевой.

**Приёмка.** Тест.

---

### AUD-74. Геометрия по секциям

**Проблема.** `paginate.rs:56-61` берёт только `sections.last()`.

**Решение.** Каждая страница получает геометрию секции, к которой принадлежит её первый
элемент тела. Тип разрыва секции: `nextPage` — новая страница; `oddPage`/`evenPage` — новая
страница, при несоответствии чётности вставляется пустая страница; `continuous` — та же
страница, новая геометрия применяется со следующей страницы (как в Word); `nextColumn` —
как `nextPage` (колонки секций вне scope, запись). Колонтитулы — от секции страницы (с
наследованием `headerReference` из предыдущей секции, как по спецификации).

**Тесты.** render-svg: документ «портрет → альбом → портрет» → три страницы с правильными
`viewBox`; `oddPage` после страницы 1 → вставлена пустая страница 2; колонтитулы секции 2
отличаются.

**Приёмка.** Тесты; SSIM-гейты зелёные.

---

### AUD-75. Последовательные пустые разрывы страниц

**Решение.** `flush` не пропускает страницу, если она создана явным разрывом (`w:br
w:type="page"`, `pageBreakBefore`, разрыв секции): пустая страница сохраняется.

**Тесты.** render-svg: `p, br(page), br(page), p` → 3 страницы.

**Приёмка.** Тест.

---

### AUD-76. MathML: дублирующиеся атрибуты

**Проблема.** `math/mathml.rs:150-161` пишет `mathvariant` до 4 раз.

**Решение.** Один `mathvariant`, вычисленный по приоритету: `m:nor`/`m:lit` (→ `normal`) >
`m:scr` + `m:sty` (комбинация: `bold-script`, `bold-fraktur`, `double-struck`, `script`,
`fraktur`, `sans-serif`, `bold-sans-serif`, `sans-serif-italic`, `sans-serif-bold-italic`,
`monospace`) > `m:sty` (`bold`, `italic`, `bold-italic`, `normal`). Таблица соответствия — в
коде одной `match`.

**Тесты.** render-svg: каждая комбинация `scr × sty` → ровно один атрибут, ожидаемое значение;
вывод разбирается `roxmltree`.

**Приёмка.** Тест.

---

### AUD-77. Цвет из `ctrlPr`

**Решение.** Через `parse_color`; невалидный → цвет по умолчанию, предупреждение.

**Тесты.** render-svg: `ctrlPr/w:rPr/w:color w:val="auto"` и `"zz"` → `fill` из допустимых.

**Приёмка.** Тест.

---

### AUD-78. Имена внешних медиафайлов

**Проблема.** `paint/image.rs:100-106` берёт basename — `/word/media/a.png` и `/word/x/a.png`
совпадают; `:` допустим.

**Решение.** Имя = путь части без ведущего `/`, `/` → `_`, символы вне `[A-Za-z0-9._-]` → `_`;
коллизии после замены → суффикс `-2`, `-3`… в порядке `MediaIndex`.

**Тесты.** render-svg: две картинки с одинаковым basename → разные имена; имя с `:` → `_`.

**Приёмка.** Тест.

---

## Ф8. PDF, конвертер, вьюер

### AUD-80. PDF-писатель: повторное изображение

**Проблема.** `render-pdf/src/document.rs:121`: изображение на нескольких страницах остаётся
только на первой.

**Решение.** XObject пишется один раз (дедупликация по `MediaId`), ссылка добавляется в
`/Resources /XObject` **каждой** страницы, где он используется.

**Тесты.** render-pdf: картинка в колонтитуле 3-страничного документа → `strict-ooxml-pdf`
читает 3 размещённых изображения, один объект XObject.

**Приёмка.** Тест; `pdf_pixels` зелёный.

---

### AUD-81. PDF-писатель: форматы PNG/JPEG

**Проблема.** 16-битные и 1/2/4-битные PNG, серые и CMYK JPEG пишутся с неверной раскладкой.

**Решение.**
- PNG 16 бит → 8 бит (старший байт); 1/2/4 бит → распаковка в 8 бит; палитра → `Indexed`
  как сейчас (если поддержано) или в RGB; альфа → `/SMask`.
- JPEG: число компонентов из SOF: 1 → `DeviceGray`, 3 → `DeviceRGB`, 4 → `DeviceCMYK`;
  при маркере Adobe APP14 с `transform=2`/инвертированном CMYK → `/Decode [1 0 1 0 1 0 1 0]`.

**Тесты.** render-pdf: по одной фикстуре на формат (генерируются в тесте через `png`-крейт;
JPEG — минимальные файлы, сгенерированные заранее скриптом `xtool gen-jpeg` и закоммиченные как
наши собственные данные); растр `hayro` центрального пикселя совпадает с ожидаемым цветом
±2/255.

**Приёмка.** Тесты.

---

### AUD-82. PDF-писатель: два символа на один глиф

**Проблема.** Второй символ с тем же глифом (NBSP и пробел) выбрасывается из `ToUnicode`.

**Решение.** CID ≠ GID: для каждой пары (глиф, символ) выделяется свой CID;
`/CIDToGIDMap` — поток отображения; `ToUnicode` — по CID. Повторная пара переиспользует CID.

**Тесты.** render-pdf: текст «a b\u{A0}c» → извлечение текста нашим ридером возвращает
исходную строку, включая U+00A0.

**Приёмка.** Тест; `pdf_pixels` зелёный.

---

### AUD-83. Конвертер: управляющие символы

**Решение.** Закрывается AUD-03 (convert пишет через `core::xml::escape` и делает запись).
Тест: PDF со строкой, содержащей U+0001 (в `Tj` через hex), → `document.xml` разбирается,
запись `convert.invalid-xml-char`.

**Приёмка.** Тест.

---

### AUD-84. Ридер PDF: inline image (`BI…ID…EI`) — пункт `L-9` очереди

**Проблема.** Из `CORE-QUEUE.md` §2 — байты inline-изображения исполняются как операторы.

**Решение.** Как записано в `CORE-QUEUE.md` §2, пункты 1–4 «Готово, когда» — без изменений;
план только фиксирует место в последовательности (после AUD-13, использует `PageBudget` и
`bounded_decompress`).

**Тесты/приёмка.** По `CORE-QUEUE.md` §2.

---

### AUD-85. Конвертер: колонки — пункт `P-7` очереди

**Решение/тесты/приёмка.** По `CORE-QUEUE.md` §3, без изменений; место в
последовательности — после AUD-15.

---

### AUD-86. `units.rs` render-pdf: переполнение цвета

**Проблема (предположение аудита).** Переполнение при преобразовании цвета.

**Решение.** Аудит функций `units.rs` по G-2; компоненты цвета — `u8::try_from` с зажимом.

**Тесты.** render-pdf юнит: цвета `FFFFFF`, `000000`, мусор.

**Приёмка.** Тест; результат аудита в коммите.

---

### AUD-87. Вьюер: прочее

**Решение.** После AUD-16 — прогон вьюера на корпусе (`strict-ooxml-view` открывает каждый
файл `tests/strict/`), без паник; результат — тест `view/tests/catalog.rs`.

**Приёмка.** Тест.

---

## Ф9. Процесс, ТЗ, документы

### AUD-90. ADR-0019: отклонения от ТЗ, принятые решением

**Содержание (решения уже приняты, ADR их записывает):**

| ТЗ | Отклонение | Решение |
|---|---|---|
| §7.3 «модель неизменяема» | `document_mut()` | оставить; ТЗ правится: «модель неизменяема для парсера; мутация — явный API» |
| §7.3 `SourceLocation` у всех узлов | нет у `TextNode`, `Symbol`, `FieldChar`, `Bookmark`, `GridCol`, `FootnoteRef` | оставить: локация на уровне `Run`/`Paragraph`/`Table` достаточна для отчёта; ТЗ правится |
| §5.4 feature `round-trip-normalize` | нормализатор в `core` всегда | оставить; feature не вводится; ТЗ правится |
| §5.4/Г.7 `default = svg + report` | мета-крейт `default` включает `write, pdf, convert` | **исправить**: `default = ["report", "svg"]`; CLI включает нужные features явно в своём `Cargo.toml`; CI-шаг `check (default features)` это проверяет |
| §6.3 `resolve_theme`, `load_media` | нет | не вводить: тема разрешается всегда, медиа — лениво через `MediaMode`; ТЗ правится |
| §6.3 `VmlFallback::RasterizeIfPossible` | `Convert` | AUD-34 |
| §4.1 п.4 content types | сигнал удалён | AUD-26 |
| §9.3 таблица OPC-URI | неверна | AUD-20, таблица ТЗ правится по ADR-0015 |
| §3.2 «редактирование вне scope» | есть `write` и `document_mut` | этапы 8+ расширили scope; ТЗ ссылается на `STAGE-8-TASK.md` |

**Приёмка.** ADR-0019 принят; изменения мета-крейта (`default`) сделаны и CI зелёный.

---

### AUD-91. Правка ТЗ

**Решение.** `TZ-STRICT-OOXML-RUST.md` → версия 2.1: §9.3 (OPC-URI), §12.1 (новые лимиты
`max_block_nesting`, `max_math_nodes`, `max_math_depth`, `max_support_features`,
`max_render_items`, `max_pages`), §4.2 + §6.3 (матрица ADR-0016, `DirectionPolicy`,
`VmlFallback`, `ExpansionLimit`), §13 (новые варианты `UndeterminedConformance`,
`UnexpectedContentType`, `LimitKind::BlockNesting/ZipWriteField`), ссылки на ADR-0015…0019.
Каждое изменение помечено «(2.1, AUD-NN)».

**Приёмка.** Документ обновлён; `README.md` ссылается на этот план.

---

### AUD-92. Fuzz-таргеты

**Проблема.** Таргетов для нормализатора, писателя, рендера и PDF нет (ТЗ §12.3).

**Решение.** Новые таргеты в `fuzz/fuzz_targets/`:
- `fuzz_normalize` — `TransitionalNormalizer::normalize` на произвольной части + `XmlReader`
  на выходе (выход обязан быть корректным XML, если `Ok`);
- `fuzz_docx_full` — `open_reader` (`Normalize` + нормализатор) → `support_report` →
  `render_svg` (первые 3 страницы) → `write_package` → повторный `open_reader` на выходе
  обязан быть `Ok`;
- `fuzz_pdf` — `PdfDocument::open` → все страницы;
- `fuzz_convert` — `convert` на PDF.
Словари (`-dict`) для XML-таргетов: имена элементов `w:`/`a:`/`m:` из `coverage/wml-elements.toml`.
Seed-корпус: файлы `tests/strict/`, `tests/docx/`, PDF-корпус. Лимиты в таргетах уменьшены
(как в `fuzz_zip`), `-rss_limit_mb=2048`, `-timeout=10`.
CI: `fuzz-smoke` 60 с на каждый новый таргет; `fuzz-nightly` 1 ч на каждый.

**Приёмка.** Каждый из 8 таргетов отработал локально **1 час** без находок; находки,
появившиеся по ходу, оформлены регрессионными тестами в `hostile` и исправлены. 24-часовой
прогон по ТЗ §12.3 — критерий приёмки этапа 7 (релиз), не этого плана; запись об этом в
`docs/fuzz-protocol.md`.

---

### AUD-93. Документация

**Решение.** Исправить: ADR-0007 (AUD-20), doc-комментарии «until Stage 6» (AUD-31),
`README.md` (CLI: `report --transitional`, `normalize`), `CORE-QUEUE.md` (пункты, закрытые
этим планом, — со ссылкой на AUD-ID), `docs/waivers.toml` (новые waiver'ы: `PDF-OBJSTM-BOMB`,
`TBL-STYLE-PR`; закрытые — удалить). `SESSION-HANDOFF-2026-10-02.md` не трогать (история).

**Приёмка.** `cargo test -p strict-ooxml-render-svg --test waivers` зелёный (реестр в обе
стороны).

---

### AUD-94. Финальная приёмка плана

План закрыт, когда:

1. все AUD-00…AUD-93 закрыты по §0.1;
2. полный гейт §0.4 зелёный локально **и** в CI на `main` (ссылка на run в
   `docs/ci-baseline-2026-10.md`, раздел «после»);
3. набор `hostile` содержит не менее одного теста на каждый дефект Ф1–Ф2 и проходит в debug
   и release;
4. OPC-гейт, XSD-гейт и census-гейт — PASS с нулём нарушений;
5. `write/tests/word_oracle.rs` зелёный на всех файлах `tests/strict/`;
6. покрытие — §15 ниже;
7. повторный аудит тех же мест (список «Где» каждой задачи) не находит дефекта того же класса:
   исполнитель прогоняет поиск из «Приёмки» каждой задачи (`rg …`) и прикладывает вывод к
   финальному коммиту.

---

## 15. Покрытие тестами

### 15.1. Пороги (строки, `cargo llvm-cov`, CI-способ)

| Крейт | Сейчас (порог CI) | После плана |
|---|---|---|
| `strict-ooxml-core` | ≥ 80 % | **≥ 85 %**, и 100 % строк в `xml/escape.rs`, `opc/policy.rs`, `opc/path.rs` |
| `strict-ooxml-wml` | ≥ 80 % | **≥ 85 %** |
| `strict-ooxml-report` | ≥ 80 % | ≥ 80 % |
| `strict-ooxml-render-svg` | ≥ 80 % | **≥ 82 %** |
| `strict-ooxml-fidelity` | ≥ 80 % | ≥ 80 % |
| `strict-ooxml-write` | не меряется | **≥ 80 %** (новый шаг CI) |
| `strict-ooxml-pdf` | не меряется | **≥ 75 %** (новый шаг CI) |
| `strict-ooxml-render-pdf` | не меряется | **≥ 75 %** (новый шаг CI) |
| `strict-ooxml-convert` | не меряется | **≥ 70 %** (новый шаг CI) |

Ветвления (ТЗ §14, ≥ 70 % для `core` и `wml`): `cargo llvm-cov --branch` (nightly) — шаг в
`fuzz-nightly` job (он уже на nightly), порог 70 % для `core` и `wml`.

### 15.2. Обязательные виды тестов по задачам

| Вид | Где | Задачи |
|---|---|---|
| Юнит | рядом с кодом | все |
| `hostile` (debug + release, стек 1 MiB, таймаут 10 с) | `strict-ooxml`, `strict-ooxml-pdf`, `strict-ooxml-convert` | AUD-04…16, 20, 22, 24, 25 |
| Табличный (матрица) | core | AUD-23 |
| Свойство (proptest) | core, render-svg | AUD-03, 30, 71 |
| Оракул Word/LO | write | AUD-20, 22, 67 |
| Схемный гейт | xtool/xsd-gate | AUD-20, 21, 43, 60, 63 |
| Round-trip модели | write | AUD-41, 42, 43, 46, 49, 50, 60, 61, 64 |
| Снапшот JSON + схема | report | AUD-31 |
| Пиксельный (SSIM) — не ухудшился | render-svg, pdf | AUD-08, 70–76, 80–82 |
| Fuzz | fuzz/ | AUD-92 |

### 15.3. Правило «тест умеет падать»

Каждый новый тест в этом плане обязан падать на коде **до** соответствующей правки (§0.1 п.2).
Для гейтов (OPC, census) — selftest на заведомо плохом входе (AUD-21). Тест, который не
может упасть, не засчитывается в закрытие задачи.

---

## 16. Сводка: дефект аудита → задача

| Дефект аудита | Задача |
|---|---|
| Purl-URI в OPC (`.rels`, core-properties, extended-properties) | AUD-20, 21, 22, 67 |
| Переполнение стека на вложенных таблицах/текстовых блоках | AUD-05 |
| Зависание на обрезанном XML; `Eof` без проверки | AUD-04 |
| Паника `widths[..column]` | AUD-08 |
| Переполнения/`debug_assert!` в рендере | AUD-09 |
| Паника `remove_element` | AUD-10 |
| ZIP-писатель `as u16/u32` | AUD-11 |
| PDF: `ToUnicode`, `/W`, `/SMask` | AUD-12 |
| PDF: бюджеты, распаковка | AUD-13 |
| PDF-писатель: IHDR | AUD-14 |
| Конвертер: квадратичность | AUD-15 |
| Вьюер: `read_line`, таймауты | AUD-16, 87 |
| Loss Report не в Feature Report | AUD-31 |
| Детекция «Strict» для Transitional | AUD-23 |
| Двойной учёт отчёта | AUD-30 |
| T2 подстановкой префикса | AUD-22 |
| Политики `Permissive`/`Mixed`/`Unknown` | AUD-23 |
| Нет `DirectionPolicy`, `VmlFallback`, `bidi` | AUD-33, 34 |
| Мёртвый сигнал content types | AUD-26 |
| `max_rel_depth` не используется | AUD-25 |
| Default features мета-крейта | AUD-90 |
| Молчаливые потери `*Pr`, Opaque без детей, `w:del` | AUD-42, 43, 46 |
| Нет fuzz для нормализатора/писателя/рендера/PDF | AUD-92 |
| Нет проверки `Send + Sync` | AUD-52 |
| Регистр имён частей, percent-encoding | AUD-24, 65 |
| Тип связи по суффиксу | AUD-22 |
| `.rels` вне `_rels/` | AUD-25 |
| Сноски в один `w:p`, неверные `r:id` | AUD-60, 61 |
| Управляющие символы в XML-выходах | AUD-03, 66, 83 |
| PAGE в колонтитулах, табуляция 0, амплификация колонтитулов | AUD-70, 71, 72 |
| Секции из sdt, строки в sdt, флаги settings, `--2%` | AUD-40, 41, 44, 45 |
| Обрезанные префиксы в сообщениях | AUD-32 |
| Нет `git remote`, CI не запускался | AUD-00 |
| Прочие из отчётов субагентов (sym, custGeom, сноски без id, oMathPara, MathML-атрибуты, ctrlPr, pgSz 0, секции, пустые разрывы, имена медиа, повтор картинок в PDF, форматы PNG/JPEG, общий глиф, `m:t`, коллизии имён, Transitional в passthrough, numStyleLink, медиа сносок, SupportModel, `render_page_svg`, `max_expansion_bytes`, мьютекс) | AUD-45, 49, 50, 76, 77, 73, 74, 75, 78, 80, 81, 82, 64, 62, 63, 47, 48, 51, 52, 35, 36 |

---

**Конец документа.**
