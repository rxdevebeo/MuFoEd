# Исследование: вредоносные файлы, с которыми библиотека не справится при открытии

**Дата:** 9 октября 2026
**Ветка / SHA:** `master` @ `5da50ed`
**Предмет:** путь открытия пакета `strict-ooxml` — ZIP → OPC → XML → WML, плюс
сопутствующие пути `strict-ooxml-pdf` и `strict-ooxml-view`.
**Метод:** чтение исходников с построчными ссылками, сквозной разбор пути данных
от `Package::open_*` до модели и рендера. Кода не писалось.
**Статус:** исследование завершено, правок не вносилось.

---

## 0. Краткий вывод

Библиотека хорошо закрыта по **байтам** и по **структурной целостности входа**:
лимиты на размер архива и каждой части, CRC-32, отказ от traversal, безусловный
отказ от `DOCTYPE` и любых сущностей кроме пяти предопределённых, полное
неразрешение внешних связей, бюджеты вложенности против переполнения стека.

Библиотека **не закрыта по количеству объектов**. Ни в `ResourceLimits`
(`strict-ooxml-core/src/limits.rs:25-149`), ни в `PdfLimits`
(`strict-ooxml-pdf/src/error.rs:155-221`) нет лимита на число элементов XML,
число связей, число `Override` или на общий объём работы. Байтовый потолок в
128 MiB на часть не спасает: `word/document.xml` целиком из `<w:p/>` — это
около 18 миллионов элементов в 128 MiB, и модель WML на порядки больше входа.

**Практическое следствие: файл, проходящий по всем текущим лимитам, может
исчерпать память или процессорное время.** Ниже — перечень конкретных пробелов.

---

## 1. Что закрыто

### 1.1 Байтовые лимиты и целостность ZIP

| Предохранитель | Файл:строка | Что ограничивает | Дефолт |
|---|---|---|---|
| `max_compressed_input` | `strict-ooxml-core/src/opc/mod.rs:149-162` | объём чтения архива в память при открытии | 512 MiB |
| `max_zip_entries` | `strict-ooxml-core/src/opc/zip/mod.rs:243-249` | число записей в центральном каталоге | 4096 |
| `max_parts` | `strict-ooxml-core/src/opc/zip/mod.rs:250-256` | то же, вторым барьером | 4096 |
| `max_single_uncompressed` | `strict-ooxml-core/src/opc/zip/mod.rs:76-82` | заявленный размер одной части | 128 MiB |
| `max_total_uncompressed` | `strict-ooxml-core/src/opc/zip/mod.rs:83-92` | сумма заявленных размеров | 512 MiB |
| `max_compression_ratio` | `strict-ooxml-core/src/opc/zip/mod.rs:98-115` | отношение uncompressed/compressed | 1000 |
| Потолок при чтении части | `strict-ooxml-core/src/opc/zip/mod.rs:617-622` | байты, выданные потоком | `max_single_uncompressed` |
| Усечение по заявленной длине | `strict-ooxml-core/src/opc/zip/mod.rs:599-606` | inflate не может выдать больше заявленного | — |
| CRC-32 + точная длина | `strict-ooxml-core/src/opc/zip/mod.rs:628-643` | каждая полностью прочитанная часть | — |
| Белый список методов (0, 8) | `strict-ooxml-core/src/opc/zip/mod.rs:337-341` | иначе `UnsupportedCompression` | — |
| Дубликат имени части | `strict-ooxml-core/src/opc/zip/mod.rs:72-74` | регистронезависимо | — |
| Границы каталога, переполнение смещений | `strict-ooxml-core/src/opc/zip/mod.rs:258-311, 475-505` | через `checked_*` / `.get()` | — |
| ZIP64-сентинелы | `strict-ooxml-core/src/opc/zip/mod.rs:377-417, 451-472` | позиционная корректность | — |
| Поиск EOCD в окне 64 KiB | `strict-ooxml-core/src/opc/zip/mod.rs:420-448` | объём сканирования | 65 557 Б |
| Не-UTF-8 имена отвергнуты | `strict-ooxml-core/src/opc/zip/mod.rs:312-316` | неоднозначные кодировки (CP437) | — |
| `arithmetic_side_effects` запрещён | `strict-ooxml-core/src/opc/zip/mod.rs:14` | статически, на весь модуль | — |
| `unsafe_code = "deny"` | `Cargo.toml:92` | класс ошибок памяти | — |

### 1.2 Пути и имена частей

| Предохранитель | Файл:строка | Что отвергает |
|---|---|---|
| `..`, уводящий за корень пакета | `strict-ooxml-core/src/opc/path.rs:162-170` | traversal |
| Абсолютные имена | `strict-ooxml-core/src/opc/path.rs:26-30` | абсолютный путь части |
| Обратный слэш как разделитель | `strict-ooxml-core/src/opc/path.rs:142-146` | вектор `a\b` |
| NUL и управляющие символы | `strict-ooxml-core/src/opc/path.rs:147-161` | инъекция в путь |
| Имя, схлопывающееся в корень | `strict-ooxml-core/src/opc/path.rs:33-37, 76-80` | `/`, `.`, `foo/..` |
| Percent-decode только для URI, не для имён ZIP | `strict-ooxml-core/src/opc/path.rs:84-123` | двойное декодирование |
| `PartId` сравнение ASCII без регистра | `strict-ooxml-core/src/part.rs:52-92` | подмена части сменой регистра |
| `.rels` только под `_rels/` | `strict-ooxml-core/src/opc/rels.rs:453-472` | AUD-25: чужой `/aaa.rels` не станет корневым |
| Целям подставляется написание ZIP | `strict-ooxml-core/src/opc/rels.rs:428-439` | AUD-24: подмена по регистру |

Отдельно отмечу: **на пути открытия из имени части не строится ни один путь ФС**,
извлечения на диск в `strict-ooxml-core` нет вообще. Ближайший аналог —
`media_file_name` (`strict-ooxml-render-svg/src/paint/image.rs:123-167`),
срезающий ведущий `/` и заменяющий всё вне `[A-Za-z0-9._-]` на `_`.
Записи-симлинки не проверяются (поле external attributes в
`strict-ooxml-core/src/opc/zip/mod.rs:274-356` не читается), но и не опасны:
ничего не пишется на диск.

### 1.3 XML-уровень

Парсер — собственный тонкий слой над `quick_xml` 0.41
(`strict-ooxml-core/src/xml/mod.rs`), не готовящая к применению библиотека.

| Предохранитель | Файл:строка | Что отсекает |
|---|---|---|
| `DOCTYPE` отвергается безусловно | `strict-ooxml-core/src/xml/mod.rs:374-376` | DTD, внешние сущности, XXE, SSRF, billion laughs. Не настраивается |
| Только 5 предопределённых сущностей | `strict-ooxml-core/src/xml/mod.rs:411-438` | общее сущности. Не настраивается |
| `max_xml_depth` | `strict-ooxml-core/src/xml/safety.rs:13-22`, вызов `xml/mod.rs:264-265` | вложенность — 256 |
| `max_xml_attributes_per_elem` | `strict-ooxml-core/src/xml/safety.rs:25-34`, вызов `xml/mod.rs:248` | атрибуты на элемент — 1024 (считаются и `xmlns`) |
| `max_text_len` | `strict-ooxml-core/src/xml/safety.rs:37-45`, вызов `xml/mod.rs:312-313` | суммарный текст + CDATA в части — 64 MiB |
| Неразрешённый префикс → ошибка, не паника | `strict-ooxml-core/src/xml/mod.rs:441-458` | путаница пространств имён |
| Ровно один корень, без хвоста, без обрыва | `strict-ooxml-core/src/xml/mod.rs:213-232, 236-247, 283-296` | «обрезанная часть — не более короткая часть» |

### 1.4 Внешние цели (SSRF)

`TargetMode="External"` фиксируется и **никогда не резолвится**:
`strict-ooxml-core/src/opc/rels.rs:495-513` (`resolve_target` возвращает
`Ok(None)`), `strict-ooxml-core/src/opc/path.rs:51-54`. Сетевого кода в
библиотеке нет — единственные сетевые вызовы в рабочей области это загрузка
корпуса в dev-инструменте `xtool/src/corpus.rs:398-437` и клиент Ollama в
`strict-ooxml-ocr/src/ollama.rs:181-193`, ни то ни другое не на пути `.docx`.

### 1.5 Бюджеты рекурсии WML

| Предохранитель | Файл:строка | Реакция | Дефолт |
|---|---|---|---|
| `max_block_nesting` | `strict-ooxml-wml/src/parse/mod.rs:979-990` | жёсткий отказ | 12 |
| `max_text_box_nesting` | `strict-ooxml-wml/src/parse/mod.rs:1008-1020` | деградация, `Unsupported` | 5 |
| `max_inline_nesting` | `strict-ooxml-wml/src/parse/mod.rs:1031-1052` | деградация | 16 |
| `max_group_nesting` | `strict-ooxml-wml/src/parse/drawing.rs:2065-2073` | деградация | 16 |
| `max_math_nodes` / `max_math_depth` | `strict-ooxml-wml/src/parse/math.rs:250-256, 626-628` | деградация | 4096 / 64 |
| `max_support_features` | `strict-ooxml-wml/src/model/support.rs:101-174` | свёртка в одну запись со счётчиком | 10 000 |
| `max_rel_depth` (BFS, цикло-безопасно) | `strict-ooxml-core/src/opc/mod.rs:320-347` | отказ | 32 |
| `max_render_items` | `strict-ooxml-render-svg/src/layout/mod.rs:520` | `RenderError::LimitExceeded` | 2 000 000 |
| `max_pages` | `strict-ooxml-render-svg/src/layout/paginate.rs:975-979` | `RenderError::LimitExceeded` | 10 000 |
| Потолок роста нормализатора | `strict-ooxml-core/src/normalize/transitional.rs:142-192`, проверка `393-400` | отказ | ×4 |

### 1.6 PDF-путь (`strict-ooxml-pdf`)

| Предохранитель | Файл:строка | Что ограничивает | Дефолт |
|---|---|---|---|
| `max_input_bytes` до `lopdf` | `strict-ooxml-pdf/src/document.rs:234-236` | размер файла | 256 MiB |
| Инфляция object/xref-потоков | `strict-ooxml-pdf/src/preload.rs:66-99` | сумма распакованных ObjStm/XRef | 4 × `max_content_bytes` = 128 MiB |
| `lopdf::LoadOptions::max_decompressed_size` | `strict-ooxml-pdf/src/document.rs:242-245` | потолок на поток | то же |
| Границы сканирования словарей | `strict-ooxml-pdf/src/preload.rs:39, 45` | объём работы | 65 536 / 4096 |
| `max_pages` | `strict-ooxml-pdf/src/document.rs:251-254` | страницы | 2048 |
| `max_content_bytes` на страницу | `strict-ooxml-pdf/src/document.rs:390`, `31-42` | распакованный контент | 32 MiB |
| `max_operations` / `max_glyphs` | `strict-ooxml-pdf/src/content.rs:657-675` | операторы/глифы на страницу | 2 000 000 / 500 000 |
| `max_path_points` | `strict-ooxml-pdf/src/content.rs:687-688` | точек в одном растеризованном пути | 200 000 |
| `max_font_glyphs` / `max_fonts` | `strict-ooxml-pdf/src/fonts.rs:310, 557, 632, 680` | глифов CMap / шрифтов | 65 536 / 512 |
| `max_image_bytes` на всех путях декодирования | `strict-ooxml-pdf/src/image.rs:551, 568, 675, 753, 778, 821, 831, 858, 897, 931` | декодированные байты на картинку | 64 MiB |
| Циклы `/SMask` | `strict-ooxml-pdf/src/image.rs:280-283, 314-331` | самоссылающаяся альфа | — |
| `max_form_depth` | `strict-ooxml-pdf/src/content.rs:1352` | вложенность form XObject | 12 |
| `max_cached_image_bytes` / `max_cached_form_bytes` | `strict-ooxml-pdf/src/document.rs:84, 259-260` | рост кэшей между страницами | 64 MiB каждый |
| `max_raster_pixels` (до растеризации) | `strict-ooxml-pdf/src/raster.rs:261-266` | пиксели на страницу | 4096² |
| Пропатченный растеризатор | `strict-ooxml-pdf/src/raster.rs:51` | сборка падает без vendor-патчей | — |
| Зашифрованные PDF отвергаются | `strict-ooxml-pdf/src/document.rs:248-250` | не расшифровываются | — |
| IHDR PNG до аллокации буфера | `strict-ooxml-render-pdf/src/image.rs:235-278` | враждебный заголовок PNG | `max_uncompressed` |
| `MAX_IMAGE_BYTES` при встраивании | `strict-ooxml-render-pdf/src/image.rs:113, 142` | размер медиа-части | 64 MiB |

### 1.7 HTTP-вьюер (`strict-ooxml-view`)

| Предохранитель | Файл:строка | Значение |
|---|---|---|
| `MAX_LINE_BYTES` → 431 | `strict-ooxml-view/src/http.rs:20`, `281-299` | 8 KiB |
| `MAX_HEADERS` → 431 | `strict-ooxml-view/src/http.rs:22`, `230-233` | 100 |
| `MAX_BODY_BYTES` → 413 | `strict-ooxml-view/src/http.rs:24`, `243-252` | 1 MiB |
| Таймаут ввода-вывода | `strict-ooxml-view/src/http.rs:26`, `165-166` | 10 с |
| Один запрос на соединение | `strict-ooxml-view/src/http.rs:157-176` | slowloris / keep-alive |
| Однопоточный accept, неблокирующий poll | `strict-ooxml-view/src/http.rs:135-154` | 1 |
| Только loopback | `strict-ooxml-view/src/main.rs:122` | `127.0.0.1` |
| Экранирование JSON в одной точке | `strict-ooxml-view/src/main.rs:378-397` | инъекция в страницу |
| `unwrap`/`panic`/индексация запрещены вне тестов | `strict-ooxml-view/src/main.rs:17-28`, `lib.rs:7-18` | паника на путях библиотеки |

---

## 2. Что не закрыто

### 2.1 Бомба из количества элементов — лимита нет вообще

**Где:** `strict-ooxml-core/src/limits.rs:25-149`, проверка в
`strict-ooxml-core/src/xml/mod.rs:236-281`.

`ResourceLimits` не содержит `max_elements`, `max_nodes` или равнозначного.
`max_xml_depth` ограничивает глубину, `max_text_len` суммирует только
`Text`/`CData` (`strict-ooxml-core/src/xml/mod.rs:312`), `check_attributes`
(`strict-ooxml-core/src/xml/safety.rs:25`) считает атрибуты, но не элементы.
Поиск по рабочей области: `max_elements|max_nodes|max_elements_per_part` —
совпадений нет; единственное `max_math_nodes` относится к одной формуле.

**Почему это проходит:** часть в 128 MiB из `<w:p/>` (7 байт на элемент) —
около 18 млн элементов при глубине 1 и нулевом тексте. Ни один текущий лимит
не срабатывает. Модель WML на такой части будет на порядки больше по памяти,
чем 128 MiB, потому что каждый элемент становится узлом модели со строками.

**Как выглядел бы фикс:** `max_elements_per_part: u64` с проверкой в
`XmlReader::open_element` рядом с существующей `safety::check_depth`
(`strict-ooxml-core/src/xml/mod.rs:264`), плюс `max_nodes` в парсере WML.

### 2.2 Атрибуты считаются, но не их байты

**Где:** `strict-ooxml-core/src/xml/safety.rs:25-34`,
`strict-ooxml-core/src/xml/mod.rs:308-319, 386-407`.

`check_attributes` ограничивает число атрибутов на элементе, а `check_text`
суммирует только текстовые узлы. Каждый атрибут при этом превращается в пару
`String` (`xml/mod.rs:397-404`) без учёта байт. 1024 атрибута по ~100 KiB
значения на одном элементе — около 100 MiB `String` при одном пропущенном лимите.
Значение атрибута не имеет отдельного ограничения длины нигде.

**Фикс:** второй аккумулятор в `safety`, питаемый из `own_start` рядом с
существующим `check_text`.

### 2.3 Связи и `Override` без счётчика

**Где:** `strict-ooxml-core/src/opc/rels.rs:487-527`,
`strict-ooxml-core/src/opc/content_types.rs:40-79`.

`parse_relationships` (`rels.rs:514`) и разбор `[Content_Types].xml`
(`content_types.rs:53, 68`) растут как `Vec`/`HashMap` без ограничения числа
элементов. Размер ограничен только размером части. 128 MiB из
`<Relationship/>` — сотни тысяч связей, по пять аллокаций на каждую; то же
для `Override` и `Default`.

**Фикс:** `max_relationships_per_part` и `max_content_type_entries` в
`ResourceLimits`.

### 2.4 Нормализатор обходит все лимиты `XmlReader`

**Где:** `strict-ooxml-core/src/normalize/transitional.rs:302-305`.

`normalize` создаёт сырой `Reader::from_reader` мимо `XmlReader`: без отказа от
`DOCTYPE`, без `max_xml_depth`, `max_xml_attributes_per_elem`, `max_text_len`.
Единственный предохранитель — потолок роста ×4
(`strict-ooxml-core/src/normalize/transitional.rs:393-400`), то есть вход
128 MiB даёт до 512 MiB сырого XML без учёта структуры.

Дополнительно:

- `collect_subtree` (`transitional.rs:1501-1517`) буферизует целое поддерево
  `w:pict` / `mc:AlternateContent` как owned-события без потолка размера.
- `Event::DocType` проходит насквозь: `into_owned_event`
  (`transitional.rs:1530`) и `rewrite_event` → `other => write_passthrough`
  (`transitional.rs:805`).
- `check_invariants` (`transitional.rs:1216-1262`) проверяет только
  пространства имён и `mc:Ignorable` — `DOCTYPE` не проверяет.

Нормализатор работает на `[Content_Types].xml` и на каждой `.rels`-части при
`open_archive` (`strict-ooxml-core/src/opc/mod.rs:186-224`) и на каждой
проверяемой части (`opc/mod.rs:640-647`).

**Открытый вопрос:** достижим ли выход нормализатора с `DOCTYPE` до записи на
диск. Дальше по пути байты всё же читает `XmlReader`, который отвергнет
`DOCTYPE`, поэтому сквозной путь, вероятно, обрывается. Проверка требует
исполняемого теста, а не чтения кода — в §4 этот пункт не закрыт.
Фаззинг-таргет `fuzz/fuzz_targets/fuzz_normalize.rs:30-39` проверяет
well-formedness выхода через `XmlReader`, но не проверяет отсутствие `DOCTYPE`
явно.

### 2.5 Нет бюджета времени или CPU

**Где:** вся рабочая область.

Единственные таймауты — тестхарнес `strict-ooxml-testkit/src/harness.rs:17-20`
(10 с, стек 1 MiB, только для тестов) и IO-таймаут HTTP-вьюера. В production
библиотека не ограничивает ни wall-clock, ни число операций. Любой
квадратичный или суперлинейный путь в парсере/рендере не имеет предохранителя.

**Фикс:** `Deadline` в `ResourceLimits`, проверяемый в `XmlReader::next_event`
и в циклах раскладки; либо явная документированная обязанность вызывающего
ограничить время.

### 2.6 PDF: бюджеты постраничные, не на документ

**Где:** `strict-ooxml-pdf/src/document.rs:251-254`,
`strict-ooxml-pdf/src/error.rs:155-241`.

Все потолки PDF — на страницу, на поток, на кэш. Суммарного потолка на
документ нет. Если вызывающий проходит все страницы: 2048 страниц × 32 MiB
контента × 2 млн операторов — порядка 64 GiB работы. Фаззинг-таргет
`fuzz/fuzz_targets/fuzz_pdf.rs` ограничивает обход 32 страницами в харнессе,
в библиотеке такого ограничения нет.

**Фикс:** `max_total_content_bytes`, накапливаемый в `PdfDocument` по вызовам
`page()`.

### 2.7 PDF: `DCTDecode` без потолка по пикселям

**Где:** `strict-ooxml-pdf/src/image.rs:426-437`, `jpeg_size` (`image.rs:947-973`).

Для `DCTDecode` байты кодека идут дальше нетронутыми, размеры берутся из
маркера SOF. Потолок `width × height` не проверяется нигде на этом пути:
`max_image_bytes` (`error.rs:171`) списывается на кэш, но не на геометрию.
JPEG на 200 байт, заявляющий 65535 × 65535, проходит и попадает в медиа-часть
`.docx`; нижестоящий растеризатор, декодирующий эту часть, платит около 12 GiB.

**Фикс:** проверка `width as u64 * height as u64` против `max_raster_pixels`
перед созданием `Encoded::Jpeg`.

### 2.8 PDF: активное содержимое не читается и не репортится

**Где:** `strict-ooxml-pdf/src/` — поиск `JavaScript|Launch|OpenAction|EmbeddedFiles|GoToR`
не даёт совпадений.

`/JavaScript`, `/Launch`, `/URI`, `/OpenAction`, `/EmbeddedFiles`, `/GoToR` не
упоминаются нигде в крейте. Это не поверхность исполнения (ничего не выполняется),
но документ с активным содержимым молча репортится как чистый.

**Фикс:** перечислять словари действий в `PdfDocument::open`
(`document.rs:233`) и записывать их в `ReadReport`.

### 2.9 Вьюер: отдельная поверхность

**Где:** `strict-ooxml-view/`.

- **Абсолютные пути корпуса отдаются любому локальному клиенту.**
  `corpus_json` (`main.rs:265-274`) включает `path`, заполняемое из
  `dir.display()` (`main.rs:514`). Аутентификации нет, CORS-заголовков нет,
  биндинг только loopback. Низкий риск, но для локального инструмента,
  который может смотреть каталог с чужими документами, это утечка.
- **Кэш рендера не вытесняется.** `Cache::insert`
  (`catalog.rs:449-451`) перезаписывает по имени, но ничего не удаляет при
  появлении нового корпуса или документа (`main.rs:194-199`).
- **Рендер синхронный, с дефолтными лимитами и без таймаута.**
  `catalog::render` (`catalog.rs:176-199`) вызывает `RenderOptions::default()`,
  то есть `ResourceLimits::default()` и `max_pages = 10_000`. Один
  патологический `.docx` в наблюдаемом корпусе блокирует однопоточный сервер.

### 2.10 Мелочи

- **`.xlsx`/`.pptx` не поддерживаются.** Библиотека только WML. `.xlsx`
  отвергается `check_main_content_type` (`strict-ooxml-core/src/opc/mod.rs:513-533`)
  при `StrictOnly`, но при `Permissive` доходит до связи `officeDocument` и
  падает позже в WML-парсере с гораздо менее конкретной ошибкой. Лучше
  отказывать рано по content type и при `Permissive`.
- **Флаг шифрования ZIP (general purpose bit 0) игнорируется** —
  `let _ = flags;` в `strict-ooxml-core/src/opc/zip/mod.rs:283, 345`.
  Зашифрованная запись распаковывается как обычный DEFLATE и падает позже как
  `InvalidZip`/CRC, а не как понятный отказ «encrypted». Сравните с PDF-путем,
  где `document.rs:248-250` отказывает явно.
- **Имя из local header не сверяется с центральным каталогом** —
  `zip/mod.rs:185-209` читает `name_len`/`extra_len`, но не само имя.
  Polyglot-архивы открываются чисто; последствий для traversal нет, так как
  путь из имени не строится.
- **Пробелы покрытия фаззингом.** Восемь таргетов в `fuzz/fuzz_targets/`
  (`fuzz_convert`, `fuzz_docx_full`, `fuzz_normalize`, `fuzz_pdf`, `fuzz_relpath`,
  `fuzz_wml`, `fuzz_xml`, `fuzz_zip`); нет отдельной цели на `write_package`,
  нет цели на `strict-ooxml-edit`, нет цели на HTTP-слой вьюера, нет цели на
  `strict-ooxml-pdf::image::decode_from` напрямую.

---

## 3. Сводная таблица приоритетов

| № | Пробел | Где | Оценка |
|---|---|---|---|
| 1 | Нет лимита на число элементов XML | `strict-ooxml-core/src/limits.rs` | высокая |
| 2 | Нормализатор обходит лимиты `XmlReader`, `DOCTYPE` проходит насквозь | `normalize/transitional.rs:302, 805, 1530` | средне-высокая |
| 3 | Нет бюджета времени/CPU | вся область | средняя |
| 4 | Нет счётчиков связей и `Override` | `opc/rels.rs:514`, `opc/content_types.rs:68` | средняя |
| 5 | Байты атрибутов не учитываются | `xml/safety.rs:25` | средняя |
| 6 | Нет суммарного бюджета PDF на документ | `strict-ooxml-pdf/src/document.rs:251` | средне-высокая |
| 7 | `DCTDecode` без потолка по пикселям | `strict-ooxml-pdf/src/image.rs:426` | средне-высокая |
| 8 | Рендер вьюера без таймаута, кэш не вытесняется | `view/src/catalog.rs:176, 449` | средняя |
| 9 | Активное содержимое PDF не репортится | `strict-ooxml-pdf/src/` | низкая (инфо) |
| 10 | Утечка абсолютных путей во вьюере | `view/src/main.rs:265` | низкая |
| 11 | Флаг шифрования ZIP игнорируется | `opc/zip/mod.rs:283` | низкая |
| 12 | Имя local header не сверяется с каталогом | `opc/zip/mod.rs:185` | низкая |

---

## 4. Что осталось невыясненным

Перечисленное ниже не удалось установить чтением исходников.

1. **Достижим ли `DOCTYPE` из нормализатора до записи на диск.** Нужен
   исполняемый тест на пути `normalize` / `write_package`.
2. **Может ли `collect_subtree` (`transitional.rs:1501-1517`) буферизовать
   целую часть.** В исходнике ограничения нет; достижимость 128 MiB в одном
   поддереве статически не установлена.
3. **Приёмка фаззинга на 24 часа не выполнена.** `docs/fuzz-protocol.md:59-65`
   содержит явную отсрочку: сессии 24 часов отложены до этапа 7. Утверждение об
   отсутствии неограниченных аллокаций в `docs/adr/0001-zip-backend.md:72-74`
   и `docs/adr/0002-xml-backend.md:79-80` опирается на часовые прогоны CI и
   proptest, а не на полноценную сессию.
4. **`fuzz/artifacts/` не исследованы.** Каталог в `.gitignore`. На диске
   лежат три утечки `fuzz_convert` и одна `fuzz_pdf`;
   `docs/HARDENING_2026-10-07.md:166-168` помечает утечку `fuzz_convert`
   в 13 байт как открытую. Живых ли это утечек — по артефактам не судил.
5. **Пиковый RSS при открытии максимально допустимого пакета.** Байтовые
   лимиты стоят на заявленных и потоковых размерах; число одновременных
   полноразмерных копий (буфер архива + копия части + `Cow::Owned`
   нормализатора) нигде не измеряется.
6. **Внутренние лимиты `hayro`.** `strict-ooxml-pdf/src/raster.rs:47-51`
   проверяет наличие `hayro::PATCHED_LIMITS`, но вендорный код не читался —
   какие именно лимиты держит растеризатор, не установлено.
7. **Ни `SECURITY.md`, ни отдельной модели угроз в репозитории нет.** Материал
   по теме — `docs/TZ-STRICT-OOXML-RUST.md` §12.2 и ADR 0001/0002; ни один из
   них не перечисляет классы атак из §2.

---

## 5. Связанные документы

- `docs/core-limits-audit.md` — обоснование текущих значений лимитов ZIP и
  решения по `max_compression_ratio`
- `docs/REWORK-CORE-LIMITS.md` — исходная постановка по лимитам ядра
- `docs/HARDENING_2026-10-07.md` — предыдущая серия ужесточений, включая
  бомбу в object stream PDF
- `docs/fuzz-protocol.md` — протокол фаззинга и его отсрочка
- `docs/adr/0001-zip-backend.md`, `docs/adr/0002-xml-backend.md` — решения по
  бэкендам и заявленные гарантии