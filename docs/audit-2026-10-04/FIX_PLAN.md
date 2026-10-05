# План исправления StrictLib: задачи, RED-тесты и приемка

Дата: 4 октября 2026 года. Основание: [AUDIT.md](AUDIT.md), находки **A01–A21**. Исходная реализация аудита: `7a8e6d37d4d96543db00a9c4f2be51f879397c1f`. Это постановка работ, а не отчет об уже написанных или пройденных регрессиях.

Архитектура, порядок и критерии зафиксированы здесь. Исполнитель пишет тесты и код по этим контрактам. Он не выбирает другой способ исправления, не ослабляет пороги, не подменяет ожидаемые данные текущим выходом. Несовместимость постановки с фактическим кодом оформляет как конкретное расхождение с доказательством; до пересмотра постановки соответствующая задача остается незавершенной.

## 1. Цель и границы

Сделать надежной уже реализованную цепочку **чтение DOCX/PDF → модель → запись Strict → повторное чтение → SVG/PDF/просмотрщик** в проверяемом подмножестве. Сохранность текста, ресурсов, геометрии, Strict-валидность и учет потерь — самостоятельные обязательства, проверяемые разными оракулами.

Закрыть все A01–A21. Импорт новых форматов, полноценный UI-редактор, распознавание намерения автора поврежденного оглавления и расширение OCR не входят в эту работу. Неподдерживаемые механизмы должны иметь точное сообщение, существенную потерю и соответствующий статус; такое сообщение не заменяет исправления поддерживаемого механизма.

Полноту сложных научных схем проверять по инвентарю компонентов и их взаимным положениям. Наличие всех букв или целого JPEG не доказывает сохранность композиции. Замечания, оказавшиеся свойством исходника, оставить отрицательными контролями: намеренное размытие, символы `_` вместе с single underline, исходный слитый TOC-абзац. Фактическое лицо шрифта в пользовательском просмотрщике пока не измерено: A11 исправляется по воспроизводимому контракту ресурса/метрик, а не по предположению о конкретном системном fallback.

## 2. Обязательный протокол RED → GREEN

1. Для каждой карточки сначала добавить основной регрессионный тест и минимальную фикстуру. Использовать существующие публичные входы, CLI, независимый XML/SVG/PDF разбор. Тест должен **компилироваться и выполняться на коде до исправления**. Для новых внутренних типов сначала проверять наблюдаемый выход; отсутствие нового метода не является доказательством бага.
2. Зафиксировать RED: команда, SHA/хеш дерева, имя теста, фактическое и ожидаемое значение, exit и лог. Падение должно быть на утверждении о нужном дефекте. Ошибка пути, отсутствующий корпус/шрифт, загрузка схем, panic тестового helper или `assert!(false)` не засчитываются.
3. Затем менять производственный код. Не менять ожидаемые значения и пороги после RED. Если независимый эталон ошибочен, сначала оформить исправление самого эталона и повторить RED.
4. Проверить GREEN тем же тестом и целевой матрицей; затем сделать обратный контроль: на временной копии вернуть прежнюю логику конкретного дефекта, сохранив новый тест. Он обязан снова упасть. Не изменять рабочее дерево пользователя для этой проверки.
5. Сохранять материалы в `target/audit-fixes/<Fnn>/{red,green,mutation}/` и итоговый `receipt.json`. В receipt: входные SHA-256, SHA кода, тесты/команды, исходы, измерения, версии оракулов, хеши шрифтов, известные ограничения. Эти пути — артефакты выполнения, не исходники.

Для A07 RED — отказ компиляции поддерживаемой feature-комбинации; это единственное исключение из требования runtime assertion. Для A04/A05 RED — неверный результат проверяемого прибора, а не намеренное падение валидного приложения.

Обязательный основной RED каждого исправления должен быть воспроизводим на исходной реализации аудита. После зависимых исправлений он также запускается до новой работы. Если предшествующая карточка уже устранила этот дефект, приложить исходный RED и текущий GREEN, отметить «закрыт зависимостью» и выполнить оставшуюся матрицу. Не создавать искусственное падение ради отдельной карточки.

Нельзя использовать `ignore`, `should_panic`, xfail, успешный skip при отсутствии инструмента, мок результата renderer, новую waiver для A01–A21 или автоматическое обновление golden. Ноль проверенных объектов никогда не PASS. Fixture ожидаемых координат не должна вызывать производственный resolver ширин/секций/рамок для вычисления своего oracle.

## 3. Архитектурные решения

### 3.1. Сохранить границы существующих крейтов

Не выделять новый layout-crate в рамках исправлений. Единый layout остается в `strict-ooxml-render-svg`; SVG и PDF потребляют одни размещенные страницы. `strict-ooxml-write` — единственный писатель пакета; не добавлять отдельный «починенный writer» в CLI. `strict-ooxml-convert` строит обычную WML-модель, без специального DOCX формата и скрытой сериализации PDF-страниц.

`Document` продолжает ссылаться на медиа; байты передаются существующим `Source`/`MediaBag`. Бинарные данные не переносить в DOM. Ресурсы и ссылки проверять после повторного открытия результата.

### 3.2. Общий результат стадий

В `strict-ooxml-core` добавить типы без зависимости от serde/UI:

```text
PipelineStage = Input | Normalize | Convert | Write | Render
PipelineOutcome = Clean | Degraded | Failed
PipelineIssue { stage, id, severity, part?, page?, location?, count, detail }
PipelineSummary { issues, outcome }
```

Сохранить существующие stage reports. Добавить адаптеры их объединения, не переписывать каждый крейт на новый report. Не дедуплицировать события разных стадий по одному id. Поля без исходной информации оставлять отсутствующими, не выдумывать координаты. Stable sort: stage, part, page, location, id, detail. Inferred и подтвержденная lossless-нормализация — Clean; Lost/Unsupported/Lossy и Recovered с непроверенной точностью — Degraded; fatal/инвариант/невозможность записи — Failed. Raw stage severity также сохраняется.

CLI: **0 Clean, 1 Degraded с созданным результатом, 2 Failed**. Исходный результат с существенной потерей можно сохранить для проверки, но нельзя объявлять чистым. Проверять и учет удалений, и фактический список потерь: равенство счетчиков не означает отсутствие потерь. Sidecar `--report-out PATH` содержит version=1, вход/выход SHA-256 и все стадии; если явно запрошенный sidecar не записан, exit=2. JSON-представление строится в report/CLI-слое, без serde в core. Расширение существующего support-report делать обратно совместимыми добавочными полями.

### 3.3. Единая геометрия контейнеров

В layout добавить внутренние типы в новых `layout/container.rs` и `layout/exclusions.rs`:

```text
LayoutContainer { id, page, origin, bounds, clip?, parent? }
LineInterval { start, end }
PageRegions { header, body, footer, columns }
Exclusion { owner_id, page, contour, distances, side_policy }
```

Все размеры — f64 px при установленном scale; входные twips/EMU преобразуются один раз. Начало локального контейнера и координаты страницы не смешивать. Правый конец строки определяется границей контейнера, а не `cursor + paragraph_width`. Body, table cell, frame и text box используют один расчет интервалов. Paint не принимает решения о переносах.

Плавающие объекты позиционировать и регистрировать их exclusions **до расчета затронутых строк**. Для paragraph/line anchors использовать ограниченный перерасчет по стабильным идентификаторам: максимум 8 проходов, сравнение геометрии ≤0,01 px, обнаружение повторного состояния. При отсутствии сходимости — явная ошибка layout, без молчаливого последнего приближения. Несвязанные страницы не перераскладывать без причины. Общие resource limits остаются действующими.

### 3.4. Шрифт измерения равен шрифту отображения

Добавить `ResolvedFace { id, resource_hash, family, style, bytes }` и font resource registry. Выбирать лицо по письменности, theme и свойствам run; наличие `cs` не переключает латиницу/кириллицу автоматически. Неизвестные/CSS-имена в детерминированном режиме заменять известным встроенным лицом с диагностикой, а не измерять приблизительно и оставлять исходное имя браузеру.

Для SVG доставлять используемые bundled faces через встроенные `@font-face` с уникальным именем по ресурсу; standalone SVG должен работать без установки шрифтов. PDF использует те же байты лица. Для точного размещения кластера добавить shaping на Rust-библиотеке `rustybuzz`, закрепить совместимую с MSRV 1.92 зависимость в Cargo.lock; shaping, glyph advances и выбранное лицо общие для layout/SVG/PDF. Новый shaper не становится причиной смены golden без независимого пересчета. SVG-текст сохраняется доступным для выделения; явные позиции кластеров предотвращают повторное расхождение браузерного shaping. Math fallback проверяется отдельно.

### 3.5. Каскад, таблицы, декорации

Разделить применение run-свойств стиля и прямого форматирования: XOR только для toggle-семантики style chain; direct On/Off — присваивание. `pPr/rPr` хранит свойства знака абзаца и не служит второй безусловной накладкой на видимый текст. Отступы наследовать по отдельным полям с сохранением различия «не задано»/«явный 0».

Таблица рассчитывает логические колонки и merge regions до pagination. `TableLayout` хранит размеры колонок, строки и диапазоны объединений; `TableFragment` материализуется для конкретной страницы. Continuation не исчезает: фон и внешние границы принадлежат фрагменту объединения, текст — своему диапазону. Декорации абзаца/ячейки рисуются из окончательных фрагментов; границы и подложки не создаются до решения о разрыве.

### 3.6. PDF-страницы и visual mode

Intermediate section properties прикрепляются к граничному абзацу предыдущей секции; последняя секция хранится в завершающем body sectPr. Для пустой страницы создавать пустой абзац-якорь. Не выводить секции второй раз циклом writer по массиву: это создало бы дубли уже существующих paragraph sectPr.

Visual PDF использует page-anchored frames для текста и anchors для изображений с нулевыми полями и координатами от верхнего левого угла нормализованного page box. Baseline переводится через ascent выбранного лица; сохранять метаданные исходной геометрии для отчета, не использовать межстрочный интервал как замену абсолютному y. Semantic mode остается поточным и допускает перепагинацию, сохраняя границы смены геометрии секций.

Каждый PDF item получает идентификатор `(source_page, ordinal)` и ровно одно disposition: `emitted`, `consumed_by_structure`, `reported_loss`. Расход вектора для определения таблицы не считается сохранением, пока соответствующая граница таблицы не появилась в выходе. Не накладывать растровый снимок целой страницы поверх дублирующего редактируемого текста.

## 4. Порядок выполнения

Карточки выполнять **последовательно** в этой таблице. В колонке зависимости указаны необходимые контракты; порядковый номер обязателен даже для технически независимых задач. Общие файлы нельзя менять параллельно.

| Карточка | Находки | Зависимости | Результат |
|---|---|---|---|
| F00 | общая инфраструктура | — | независимые фикстуры и receipts |
| F01 | A04, A05 | F00 | достоверные измерения и fail-closed gates |
| F02 | A07 | F01 | поддерживаемая feature pdf собирается |
| F03 | A06 | F01 | единый outcome и отчеты всех стадий |
| F04 | A09 | F01, F03 | валидная Strict-сериализация |
| F05 | A01 | F04 | сохранение границ и геометрии PDF-секций |
| F06 | A10 | F00 | корректный каскад |
| F07 | A11 | F02, F06 | единое лицо, shaping и доставка шрифта |
| F08 | A20 | F07 | единые контейнеры и интервалы строк |
| F09 | A19 | F08 | tab alignment и leaders |
| F10 | A16 | F06, F09 | списки без коллизий |
| F11 | A12 | F08 | невырожденные размеры таблиц |
| F12 | A14 | F06, F08 | декорации блочных областей |
| F13 | A15 | F07, F08, F12 | области страницы участвуют в pagination |
| F14 | A13 | F08, F13 | разрешение координат и размеров anchors |
| F15 | A21 | F14 | обтекание до переноса строк |
| F16 | A18 | F12, F15 | рамки и составные композиции |
| F17 | A17 | F11, F12, F13 | фрагментация вертикальных merge |
| F18 | A03 | F05, F07, F16 | абсолютная геометрия PDF visual |
| F19 | A02 | F03, F14, F18 | сохранение/учет PDF-векторов |
| F20 | A08 | F03, F19 | потери доступны в API и UI |
| F21 | A01–A21 | все | сквозная приемка и corpus gate |

## 5. Карточки исполнения

Каждая карточка включает основной тест RED, дополнительные регрессии и итоговый observable contract. Общие пороги из раздела 6 обязательны во всех карточках.

### F00. Подготовить независимый комплект регрессий

**Постановка:** перенести минимальные механизмы аудита в генерируемые фикстуры `strict-ooxml-testkit` и `xtool/audit-fixes/`; не включать закрытый пользовательский корпус в публичный репозиторий. Testkit остается независимым от производственных крейтов. Добавить manifest тестов A01–A21 и runner receipts. Ожидаемые значения — вручную/независимым оракулом, с объяснением единиц.

**Проверки инфраструктуры:** валидная фикстура проходит независимый ZIP/XML-разбор; умышленно поврежденная отвергается; удаленный файл, отсутствующий инструмент, пустой список тестов и нулевой знаменатель завершают runner неуспешно. Это проверка test infrastructure, не исправление A01–A21 и не требование искусственного RED на приложении.

**Приемка:** один переносимый минимальный сценарий для каждого подтвержденного механизма; создаются исходники DOCX/PDF, не только DOM. Внешние корпусные файлы — отдельный локальный профиль с хеш-манифестом. Отсутствие корпуса дает BLOCKED для корпусной приемки, не успешный skip. F00 не создает код будущих resolver/report типов заранее ради компиляции тестов.

### F01. Исправить приборы сохранности и XSD/census gates — A04/A05

**Владение:** `strict-ooxml-convert/tests/convert.rs`, test-only text oracle, `xtool/xsd-gate/{xsd_gate,census_gate}.py`, Python regression suite и CI wiring.

**Код:** строгий Unicode scalar sequence для синтетических тестов; дополнительно LCS recall=`LCS(want,have)/len(want)` и precision для corpus, без удаления всех пробелов. Нормализация ограничена CRLF→LF, ее режим фиксируется. Обход текста включает таблицы и вложенные контейнеры; headers/footers считаются отдельными областями. Gate считает expected/validated/missing/refused/invalid/uncovered. Непредусмотренный missing, unknown violation и нулевой validated блокируют PASS. Реестр — классификация, а не фильтр ошибок. Known-source violations показываются отдельно и не дают документу schema-clean. Непокрытый root требует конкретного альтернативного validator или делает полную Strict-приемку BLOCKED.

**Основные RED:** `f01_text_metric_rejects_total_substitution`: want=`abc`, have=`Z`, recall=0, не 1; `f01_xsd_missing_output_fails`: один ожидаемый документ без результата → nonzero и validated=0; `f01_census_unknown_violation_fails`: невалидный Strict settings из аудита, отсутствующий в registry → nonzero.

**Матрица:** `abc→c` recall=1/3; `abc→ab`, `ac`, `bc`; `abc→cba` не 100%; лишний текст снижает precision; пробелы/таблица/Unicode; частично отсутствующий output; валидный единичный output; stale waiver и отказ с неверной причиной. Валидный контроль должен проходить, поврежденный — падать после самого исправления.

**Приемка:** ни один unmatched schema error не дает schema-clean/PASS; «closed» для правила без упражняющей фикстуры заменяется NOT_EXERCISED. Все три исходных ложных успеха воспроизводимо устранены. Опубликован точный знаменатель.

### F02. Закрыть feature pdf — A07

**Владение:** `strict-ooxml/Cargo.toml`, feature cfg/reexports в фасаде, CI feature matrix.

**Решение:** `pdf = ["dep:strict-ooxml-render-pdf", "svg"]`. Существующая RenderOptions/layout зависимость становится прямым контрактом feature; не дублировать RenderOptions и не выделять новый крейт.

**Основной RED:** `cargo +1.92.0 check -p strict-ooxml --no-default-features --features pdf --locked` сейчас не собирается. Добавить compile/use пример открытия минимального DOCX и вызова render_pdf с экспортируемым RenderOptions.

**Матрица/приемка:** no-default, svg-only, pdf-only, write-only, convert-only, default, all-features собираются; pdf-only пример выполняется. CLI собирается без дополнительных ручных feature-флагов. Зависимости/lock совместимы с MSRV; нет cfg, скрывающего публичную функцию вместо исправления.

### F03. Учет потерь всей цепочки — A06

**Владение:** core pipeline types, адаптеры существующих reports, CLI write/from-pdf/render, report schema additive extension.

**Основной RED:** `f03_normalization_loss_changes_exit`: минимальный VML-loss DOCX из аудита → выход записан, normalization.lossy>0, exit=1 вместо 0. `f03_writer_loss_changes_pdf_exit`: PDF-конверсия и отдельная минимальная writer-loss фикстура/интеграционный harness, чистый convert report + потеря медиа writer → stage Write присутствует, outcome Degraded.

**Матрица:** только normalization, только conversion, только write, две стадии одновременно, input fatal, нарушение no-silent-loss, inferred-only, recovered, clean; JSON сохраняет ids/counts/locations и обе стадии одинакового id. Нельзя мокировать конечный exit. На source-confirmed ветке writer требуется живое воспроизведение, не только чтение условного оператора.

**Приемка:** 0/1/2 следуют разделу 3.2; writer report выводится и при from-pdf; sidecar валиден, хеш выхода совпадает, failure записи sidecar проверен. Финальные файлы записываются атомарной заменой после успешной подготовки; partial package не объявляется результатом.

### F04. Strict scalars и порядок numbering — A09

**Владение:** normalizer scalars, parse/model для соответствующих settings/math/numbering, `write/{parts,drawing,order}.rs`.

**Решение:** нормализовать lexical values до writer и проверять canonical Strict output. `srcRect`: integer в тысячных процента → десятичное число с `%`, без float drift, например 1253→`1.253%`, 0→`0%`. `smallFrac`: on/1/true→true, off/0/false→false; неизвестное значение — явная ошибка, не невалидный XML. `suff`: допустимы **tab, space, nothing**, значение nothing не заменять. Сериализация CT_Lvl строго через имеющийся order table: start, numFmt, lvlRestart, pStyle, isLgl, suff, lvlText, lvlPicBulletId, lvlJc, pPr, rPr.

Legacy `stylePaneFormatFilter/@val` декодировать в именованные boolean атрибуты. Маски: 0001 allStyles; 0002 customStyles; 0004 latentStyles; 0008 stylesInUse; 0020 headingStyles; 0040 numberingStyles; 0080 tableStyles; 0100 directFormattingOnRuns; 0200 directFormattingOnParagraphs; 0400 directFormattingOnNumbering; 0800 directFormattingOnTables; 1000 clearFormatting; 2000 top3HeadingStyles; 4000 visibleStyles; 8000 alternateStyleNames. Зарезервированный 0010 и неверный hex — отказ с причиной. Если присутствуют именованные атрибуты, legacy val не накладывается поверх них. Это следует правилу приоритета [Microsoft Open Specifications](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/6b4f3e2f-281a-412c-a486-c41b5f773b09); допустимые атрибуты/sequence проверены по закрепленному локальному Strict XSD.

**Основные RED:** `f04_settings_bitmap_is_strict`: исходное val=0001 → allStyles=true и нет val, XSD=0; `f04_crop_percentage_is_strict`: ненулевой crop → все 4 атрибута Strict percentages; `f04_level_suffix_order_is_strict`: lvlText+lvlJc+suff=nothing → suff перед lvlText, numbering XSD=0; `f04_math_onoff_is_strict`: smallFrac=off → false, XSD=0.

**Матрица:** отдельные битовые флаги и 2002, mixed val/named, значения crop со знаком/дробью и все нули; tab/space/nothing; smallFrac true/false/absent; два successive writes байт-детерминированы и второй normalize не вносит изменений. Схемный oracle должен отвергать старую lexical/order форму.

**Приемка:** минимальные выходы и все 121 вход корпуса проверены XSD+OPC, неизвестные ошибки блокируют F21. Не удалять settings/crop/numbering для получения валидности. Уточнение исходного аудита: suff=nothing само по себе валидно; независимая проверка выявила неправильный порядок suff после lvlText/lvlJc.

### F05. Секции PDF при round trip — A01

**Владение:** `convert/{semantic,visual,geometry}.rs`; общий helper завершения страницы, интеграционные convert/write тесты.

**Основной RED:** `f05_visual_preserves_two_page_sections`: две страницы 612×792 и 842×595 pt → записать/повторно прочитать → две секции с обеими геометриями; visual SVG/PDF ровно две страницы. Проверить XML paragraph sectPr + final body sectPr, не только sections.len до записи.

**Матрица:** 3 страницы, пустая средняя, пустая последняя, последняя с таблицей, диапазон страниц, одинаковая геометрия соседних страниц, semantic/visual. При необходимости пустой граничный параграф не создает дополнительной пустой страницы. В semantic точное число физических страниц не фиксируется, но соответствие блока и геометрии секции проверяется.

**Приемка:** страницы не получают размер последней секции задним числом; section boundaries не дублируются writer. Текст/медиа/порядок сохранены после записи, не только в DOM конвертера.

### F06. Разделить стиль и прямые run toggles — A10

**Владение:** `render-svg/style.rs`, нужные computed props, style regression tests.

**Основной RED:** `f06_direct_italic_is_not_xor`: pPr/rPr i + видимый run i → SVG italic; аналогично b. Исходная реализация выключает флаг двойным XOR.

**Матрица:** style basedOn toggles, character style, direct On/Off, Absent, одинаковые флаги на mark/run, пустой paragraph mark, mixed Latin/Cyrillic/CS, caps/vanish, изоляция соседнего run. Assertions на computed и фактическом SVG/PDF.

**Приемка:** direct свойства задают состояние; mark formatting не применяется второй раз ко всем буквам; toggle chain имеет независимую таблицу ожидаемых состояний. Исправление не сводится к удалению pPr/rPr из входного файла.

### F07. Согласовать лицо, shaping и ресурс — A11

**Владение:** `font/*`, `style.rs`, текстовые layout/paint items, SVG emitter и PDF font/text emission. Существующие math paths сохраняются.

**Основные RED:** `f07_font_resource_travels_with_svg`: standalone SVG заголовка/кода должен содержать ресурс выбранного лица; текущий SVG не содержит его. `f07_cs_font_does_not_override_cyrillic`: Latin/Cyrillic run с отдельным cs=Segoe UI → выбран ascii/hAnsi/theme для соответствующей письменности, не безусловный cs. `f07_unknown_face_has_consistent_fallback`: неизвестная CSS family → конкретный bundled face для измерения и доставки.

**Матрица:** theme major/minor, bold/italic, split colored code runs vs единый run, пробел/реальная tab, №, отсутствующий glyph, resource missing/corrupt, отключенные системные шрифты, SVG standalone/viewer/PDF. Проверять payload hash, выбранное лицо, shaped cluster positions и независимые hmtx-контроли простого текста. Разрыв на runs без изменения свойств не должен менять длину строки.

**Приемка:** resource_hash layout=SVG=PDF; простой advance error ≤0,25 px; 0 незаявленных коллизий соседних фрагментов по ink. Браузерный gate проверяет font load завершенным и выбранный resource; нет зависимости от шрифтов машины. Сохранность selectable text проверяется отдельно. Доставка лица обязательна даже если локальная машина уже имеет такой шрифт.

### F08. Исправить интервалы строк — A20

**Владение:** новые container types, `layout/{mod,paragraph}.rs`, адаптация вызовов из cells/text boxes.

**Основной RED:** `f08_first_line_indent_keeps_right_boundary`: контейнер 600 px, firstLine=160 px и короткие words → каждый размещенный glyph/segment находится внутри правой границы 600 px; на текущем коде начало доходит до 713,242 px. Проверять также ожидаемый перенос первого слова, а не только clip.

**Матрица:** firstLine 0/+/−, hanging, start/end, center/right/justify, RTL, второй перенос, tab/inline object у границы, cell/frame/text box, отступ больше ширины, один непрерывный длинный token. Для слишком узкого интервала — определенный overflow report, без фиктивной ширины 1 px.

**Приемка:** firstLine изменяет start первой строки и доступную длину, но не end; никакого «исправления» clip, скрывающего текст. Правило intentional hanging и long-token overflow явно проверено.

### F09. Выравнивание табуляторов и leaders — A19

**Владение:** paragraph segments/line builder, effective tabs cascade, field/hyperlink flattening.

**Решение:** TabbedSegment заканчивается следующим tab/break; до размещения измерить весь сегмент выбранным face. Right/center/decimal рассчитываются относительно stop; decimal anchor — выбранный separator из свойств/локали, явно закрепленный фикстурой. Leader рисуется в отдельном gap, не добавляется к извлекаемому тексту. Clear удаляет унаследованный stop, bar рисует вертикальную линию и не становится обычным cursor jump.

**Основной RED:** `f09_right_tab_aligns_segment_end`: стоп=320 px, `12` заканчивается на 320±0,25, присутствует dot leader; текущий код начинает `12` на 320. Второй RED: правый stop у границы 720 px → номер отдельного TOC/PAGEREF остается на baseline записи.

**Матрица:** left/right/center/decimal/bar/clear/default, dot/dash/underscore, несколько fonts/runs, 1/2/3 цифры, nested field+hyperlink, длинный title, RTL, tab за контейнером, source merged entries.

**Приемка:** 0 неверных anchors/leaders/ненужных переносов; лидер не перекрывает текст и не меняет сохраненный text sequence. Не реконструировать слитый источник в несколько записей. Пересчет всего TOC по новой пагинации не вводится как скрытый дополнительный scope.

### F10. Геометрия списков и partial indentation — A16

**Владение:** paragraph effective indentation, numbering marker layout.

**Решение:** property-level precedence: paragraph/style resolved baseline → numbering-level defaults для списка → direct pPr поля. Явный 0 сохраняется, отсутствие других полей не отменяет inheritance. Маркер — отдельный line item с заранее измеренными bounds и suffix; не вставлять его после раскладки основного текста. Marker и first-text имеют отдельные anchors. Marker занимает hanging-область до first-text; если declared anchors совпали, его правый край ставится в `text_start - suffix_gap`, без изменения explicit text_start. Space gap равен advance пробела лица маркера, tab gap определяется следующей доступной stop, nothing gap=0; при отсутствии разрешенной hanging-области и невозможности непересекающегося размещения вернуть layout issue. Не переносить все explicit 0 в наследуемый start.

**Основной RED:** `f10_partial_zero_indent_does_not_overlap_marker`: level left+hanging, direct start=0 и отсутствующий firstLine → source 0 сохранен, marker и text не пересекаются. Текущий код дает одинаковый x у 5/5 пунктов.

**Матрица:** bullet/decimal, numbering restart, уровни, multi-digit, suffix tab/space/nothing, marker font, styled и direct indentation, narrow cell, RTL, длинный item с переносами.

**Приемка:** 0 marker/text ink collisions и сохраненный порядок; ожидаемые marker/text anchors и hanging следующей строки заданы численно. Конфликт источника виден в report; нельзя убрать bullets для прохождения.

### F11. Предпочтительная ширина таблицы — A12

**Владение:** `layout/table.rs`, internal TableLayout/width solver.

**Решение:** tblW dxa=0/auto/отсутствие — отсутствие положительной target width, не ширина 1 px. Положительный grid — приоритетный набор пропорций; zero/absent target использует grid sum с ограничением контейнера. При отсутствии grid использовать constraints ячеек/минимальные содержательные ширины. Positive dxa и pct разрешать относительно текущего контейнера. Невозможные constraints возвращают диагностику, не отрицательные/NaN колонки.

**Основной RED:** `f11_zero_table_width_uses_grid`: target dxa0, grid=9355 twips → ширина 623,667 px в достаточно широком контейнере и сохраненные пропорции, не 1 px.

**Матрица:** auto/nil/dxa0/positive/pct, grid0/missing, mixed cell widths, spans, nested table, контейнер уже/шире grid, RTL. Различать предпочтительную ширину и явное невозможное переполнение.

**Приемка:** конечные положительные ширины, сумма соответствует resolved target ≤0,25 px; нормальные колонки не схлопываются до одного символа. Вход не изменен; удаление tblW не является исправлением.

### F12. Подложки и границы абзацев — A14

**Владение:** computed paragraph decorations, line/block fragments, paint слои SVG/PDF.

**Решение:** переносить эффективные shd/pBdr в computed model. Подложка принадлежит окончательному paragraph fragment, учитывает indent/container и рисуется до текста; paragraph border не заменять underline. На переносе страницы создавать декорацию каждого fragment, без непрерывного прямоугольника через край страницы.

**Основной RED:** `f12_paragraph_shading_reaches_output`: grey F7F7F7 на code paragraph → SVG/PDF rectangle охватывает весь block и все строки; текущий renderer имеет 0 таких подложек при явных shd.

**Матрица:** direct/style/table-derived shading, multiline, page break, padding/borders, пустой абзац, nested cell/frame, single underline вместе с `_` как неизменяемый контроль.

**Приемка:** 100% ожидаемых decorations, 0 лишних/потерянных; ошибка bounds ≤0,25 px. Текст, underscore glyphs и underline не удаляются и не дублируются.

### F13. Header/body/footer как области pagination — A15

**Владение:** `layout/{headerfooter,paginate,mod}.rs`, page regions, dynamic field passes.

**Решение:** измерить активные header/footer до body pagination. При обычных неотрицательных полях body_start=max(top margin, header bottom), body_end=min(page-bottom margin, footer top). First/even/default references и наследование выбираются до измерения. Добавочный gap не выдумывать: использовать существующие исходные расстояния. Явно отрицательные поля и разрешенное source overlap имеют отдельную диагностику/контракт, не общий auto-fix.

**Основной RED:** `f13_body_respects_tall_header`: top=header=37,8 px, обычный однострочный header → первый body ink ниже header ink; текущие baseline 47,8/48,9 близки и не обеспечивают раздельные области. Дополнительно footer fixture с body у низа.

**Матрица:** tall/multiline header, first/even/odd, несколько секций, PAGE/NUMPAGES/SECTIONPAGES, увеличение total pages, поля 0/отрицательные, картинки/таблицы в header/footer.

**Приемка:** 0 неожиданных region ink collisions; total-page зависимые поля и region bounds сходятся в максимум 8 проходов, иначе явная ошибка. Только финальное количество страниц печатается. Принудительное увеличение исходных полей без расчета региона не засчитывается.

### F14. Позиционирование и относительные размеры anchors — A13

**Владение:** `layout/floating.rs`, container resolver, parse/model нужных wp14 размеров, normalized VML integration.

**Решение:** один resolver page/margin/column/paragraph/line/character containers. Alignment считается в rectangle выбранного relativeFrom, не всегда по whole page. SizeRelH/V с процентом — в указанном reference rectangle; положительная относительная величина приоритетнее fallback extent. Transform group применяется один раз; clip/overflow принадлежит контейнеру, не скрытой подмене размера.

**Основной RED:** `f14_relative_width_replaces_fallback_extent`: width=94,1% от страницы 642,2 px, fallback extent=768 px → actual width=604,3102 px. Изменение ширины page пропорционально меняет width, fixed extent не побеждает.

**Матрица:** page/margin/column align left/center/right, offsets +/−, процент по высоте, nested group, normalized VML, host на следующей странице, missing/invalid relative value. Zero explicitly invalid не маскировать позитивным числом без issue.

**Приемка:** origin/extent error ≤0,25 px; заголовок рядом с объектом не выходит из-за неверного reference box; SVG/PDF одинаковы. Пока exclusions появляются только в F15, эта карточка закрывает координаты/размеры, не заявляет faithful wrapping.

### F15. Реальное обтекание плавающих рисунков — A21

**Владение:** exclusions engine, paragraph intervals, anchor placement/pagination. Paint больше не является первым местом появления объекта.

**Решение:** Square использует rectangle, расширенный distL/R/T/B; side both/left/right/largest ограничивает свободные intervals. Tight/Through используют нормализованный polygon: Tight исключает внутренность contour, Through допускает внутренние свободные области по polygon winding. None не исключает текст, TopAndBottom резервирует высоту. Если polygon/transform не поддержан, сообщить Unsupported/Degraded, не называть rectangle точным Tight.

**Основной RED:** `f15_square_changes_line_intervals`: минимальный portrait fixture из visual-routing-marketing-book → Square и None имеют разные переносы; Square не содержит text ink внутри расширенной области изображения. Сегодня SVG побайтно одинаковы.

**Матрица:** 4 side policies, distances, картинка в середине/у края, host+следующие абзацы, 2 объекта, page break, column, behindDoc/allowOverlap, Tight/Through contour, трансформация. Увеличение distance монотонно уменьшает свободный interval, а не сдвигает ресурс.

**Приемка:** 0 незаявленных ink intersections в поддерживаемом wrap; позиция и media hash одинаковы при смене только wrap; свободные интервалы ≤0,25 px. 32 начала text внутри портрета из корпуса — baseline индикатор, а не финальный ink oracle. Бесконечный reflow исключен лимитами и проверкой сходимости.

### F16. Абзацные рамки и научные композиции — A18

**Владение:** frame grouping/classification, computed frame props, container/exclusion integration, parse support status уточняется после приемки.

**Решение:** соседние абзацы одинаковой effective frame signature в одной секции объединять в один FrameGroup; signature включает anchors, x/y/align, width/height/rule, spacing/wrap. Group разрывают изменение signature/section/явный page break/другой родитель. Не превращать каждое из 14 924 повторений framePr в отдельный frame. Внутри frame layout обычный; frame position разрешается в reference container. exact/minimum/auto height имеют разные правила, overflow exact явно диагностируется. Around использует тот же exclusions engine.

**Основной RED:** `f16_frame_translation_moves_all_children`: x=3000,y=2000 twips → origin=(200;133,333) px; смена x→4500 сдвигает детей на 100 px. Текущий код игнорирует shift. Второй тест: два соседних framed paragraphs не наложены и образуют одну область.

**Матрица:** одинаковые/разные signatures, sections, zero page margins, page/margin/column anchors, exact/auto height, frame around/none, picture+external labels, normalized VML, nested frame/text box, ordered text и clipping report.

**Приемка:** grouping 0 mismatches; origin/extent и относительные положения компонентов ≤0,25 px; 0 потерь/дубликатов текста/изображений. Инвентарь схем 54/56/104 задается до fix по исходным XML и независимому Word-эталону. Нет эталона — полнота сложной схемы BLOCKED, а не PASS по числу text/line. Целые JPEG карты не меняются для исправления подписей.

### F17. Фрагменты вертикальных объединений — A17

**Владение:** TableLayout/MergeRegion, `layout/{table,paginate}.rs`, fragment painting.

**Решение:** геометрия merge region логическая, page fragments отдельные. На каждой странице создать фон и внешние borders пересечения региона с fragment. Не рисовать внутреннюю горизонтальную границу через merge, не дублировать restart text; повторные заголовки таблицы — отдельные copies с отдельным provenance. Слишком высокая row делится только там, где source разрешает; невозможный unsplittable размер явно reported.

**Основной RED:** `f17_vmerge_has_continuation_on_next_page`: синтетическая 3-row merge с forced split → фон/правая граница присутствуют на обеих страницах, нет out-of-page paint. Локальная проба должна получить fragments **102,5+63,25 px** вместо одной фигуры 165,75 px и двух пропусков.

**Матрица:** merge через 2/3 страницы, gridSpan+vMerge, несколько regions, repeat header, nested content, restart у края, empty continuation, row keep/cantSplit.

**Приемка:** сумма высот fragment=логическая высота ±0,25; 0 отсутствующих continuation regions/дубликатов текста; 0 выхода за страницу, 0 gap внешнего border >0,25 px. Ширина колонок неизменна между fragments. Снятие vMerge с исходника не засчитывается.

### F18. Геометрия PDF visual — A03

**Владение:** `convert/{visual,geometry,semantic}.rs` общие media positioning helpers и page frames.

**Основной RED:** `f18_pdf_baseline_and_x_survive_write`: PDF text x=72 pt, baseline=92 pt сверху → visual → write/reopen → SVG x=96 px и baseline=122,667±0,25. Сегодня x=192,y=108,8. Off-center image сохраняет свой rectangle вместо центрированного inline paragraph.

**Матрица:** 3 строки в разных частях, нулевой/ненулевой page box origin, landscape, выбранные страницы, изображение справа/слева, несколько размеров лица. Нормализованная crop/rotation geometry считается один раз; неподдержанная transform имеет explicit issue. Поддерживаемые quadrants rotation проверяются численно.

**Приемка:** координаты, размеры, baseline в допуске, нет двойного поля; visual сохраняет число выбранных страниц и text sequence, XSD/OPC=0. Semantic не принуждается к абсолютной геометрии и не теряет свое назначение.

### F19. PDF-векторы и их исчерпывающий учет — A02

**Владение:** conversion item ledger, `semantic/visual/media/recover` integration, mapping на существующий DrawingML custom geometry. Не писать второй PDF parser.

**Решение:** поддержать доступное в PDF reader подмножество path move/line/cubic/close, fill/stroke и affine transform; разместить как page-anchored DrawingML shape. При semantic table reconstruction пометить использованные ruling paths и проверить выходные borders. Rectangular clipping переносить; сложные clip/pattern/unsupported paint — отдельный reported_loss с source item id. Silent omission запрещен на любой странице, в том числе с читаемым текстом. Raster fallback всей страницы с дублирующим текстом запрещен.

**Основные RED:** `f19_text_and_vector_is_not_lossless_when_vector_missing`: красный filled rectangle + текст, оба режима → либо сохраненный shape, либо Degraded с конкретным vector id (первый этап учета). Финальный обязательный `f19_supported_rectangle_survives`: rectangle реально присутствует после write/reopen/SVG/PDF, fill/box совпадают, текст не дублируется. Одного warning для этого поддерживаемого rectangle недостаточно.

**Матрица:** stroke line, fill+stroke, cubic, mixed image/vector/text, labels, consumed table ruling, unsupported pattern/clip, embed_images=false, пустая page, несколько vectors.

**Приемка:** каждый значимый source vector имеет ровно один disposition; 0 необъясненных удалений/двойного emission; supported primitives geometry ≤0,25 px, цвет точно по fixture. Unsupported остаются явной границей с Degraded, их сохранность не объявляется выполненной. В итоговом отчете раздельны «A02 silent-loss устранен» и список еще не отображаемых primitives.

### F20. Потери в просмотрщике — A08

**Владение:** `strict-ooxml-view/src/{catalog,main}.rs`, frontend document view, tests API/UI.

**Решение:** DocumentView включает pipeline outcome/issues из F03. API добавляет поля, сохраняет summary механизмов отдельно. UI при успешном render показывает компактную сводку потерь, раскрываемые stage/id/count/part/page/detail и отличает unavailable report от clean. Если stage не запускалась в viewer, она отмечается not_run; sidecar принимается только при совпадении выходного SHA, чужой/stale sidecar не подменяет live report.

**Основной RED:** `f20_successful_view_exposes_normalization_loss`: живой VML-loss DOCX → HTTP успешен, API содержит normalize loss и Degraded, UI показывает число/причину. Текущий DocumentView оставляет note=None и не передает LossRecord.

**Матрица:** clean, lossy, failed open, special-character detail escaping, много issues, keyboard expansion, render-only partial, matched/stale sidecar.

**Приемка:** API JSON валиден; UI не скрывает потерю за общим supported/partial summary, не сообщает о стадиях, которых не выполняла. Обязателен browser test с живым API; скриншот без assertions не заменяет тест.

### F21. Сквозная приемка и закрытие аудита

**Владение:** test manifest/CI, corpus runner, receipt consolidation, AUDIT/REPORT updates. Менять производственный код на этом этапе только возвратом к конкретной незавершенной карточке.

**Проверки:** все RED witnesses повторены с исправлениями и обратными controls; workspace test all-features, feature matrix, XSD+OPC, convert/write/reopen/render round trips, standalone browser font gate, SVG/PDF geometry, локальный корпус 121 DOCX и минимальный PDF-набор. Synthetic public suite обязателен в CI и не зависит от закрытого корпуса. Local corpus acceptance — отдельный обязательный результат, не объявляемый пройденным по публичному CI.

**Приемка:** 21/21 находок имеют implementation/test/receipt ссылки либо явно BLOCKED с невыполненным критерием; нет незаметно снятых assertions/новых waiver. Для A02 отдельно остается список unsupported primitives. Для F16 без Word reference нельзя закрыть полноту сложных схем. Частичная реализация не получает общий статус «все баги исправлены».

## 6. Общие оракулы, пороги и команды

### 6.1. Четыре независимых слоя

| Слой | Oracle | Обязательное ожидание |
|---|---|---|
| Пакет | independent ZIP/XML, OPC gate | ссылки/parts/media согласованы, 0 OPC violations |
| Strict | pinned ECMA XSD через lxml | 0 violations у принимаемых outputs, unknown не фильтруются |
| Семантика | вручную заданный текст/структура, media hashes | exact synthetic text/order; 0 пропавших/дублированных компонентов |
| Геометрия | SVG/PDF независимый parse, pinned browser/raster masks | anchors/intervals/baselines/extents ≤0,25 px при scale=1; 0 неожиданных ink collisions |

Порог масштабируется пропорционально scale. XML twip rounding отдельно ≤0,5 twip; нельзя незаметно ослабить pixel budget через округление в десятые pt. Resource hashes exact там, где не предусмотрена явная lossless перекодировка; при перекодировке сравнивать декодированные пиксели и dimensions. Цвет synthetic fill exact; небольшая antialiasing-разница не дает права потерять компонент. Whole-page SSIM применяется дополнительно, но не может закрывать плохую локальную геометрию.

Для synthetic font/geometry suite использовать bundled fonts и вручную заданные небольшие координаты; независимый parser не импортирует production geometry. Для browser — закрепленный test engine, дождаться fonts loaded, проверить resource/face. Если runtime отсутствует, соответствующая приемка BLOCKED. Сборка приложения от browser инструмента не зависит.

Corpus Word reference хранить вне публичного исходника с manifest: input hash, Word version, export settings, page size, reference hash. Reference создается из исходного DOCX, не текущего SVG и не диагностической копии. Если export другой версии меняет layout, создавать отдельный профиль, не перезаписывать прежний golden автоматом. Повторный ручной осмотр пользователем дополняет измерения.

### 6.2. Команды

Исполнитель добавляет runner `xtool/audit-fixes/run.py` с фиксированным интерфейсом:

```powershell
python xtool/audit-fixes/run.py --task F09 --phase red --receipt-dir target/audit-fixes/F09/red
python xtool/audit-fixes/run.py --task F09 --phase green --receipt-dir target/audit-fixes/F09/green
python xtool/audit-fixes/run.py --all --phase green --receipt-dir target/audit-fixes/final
python xtool/audit-fixes/run.py --all --corpus strict-ooxml-core/tests/docx --require-corpus --receipt-dir target/audit-fixes/corpus
cargo +1.92.0 test --workspace --all-features --locked
cargo +1.92.0 check -p strict-ooxml --no-default-features --features pdf --locked
```

Runner phase red не инвертирует exit теста и не считает любой отказ успехом: он сохраняет отказ и его assertion как свидетельство. Итог RED receipt принимается отдельно только при совпадении нужного дефекта. Green возвращает nonzero при любом failed/unmeasurable required check. Реальные команды xsd_gate/census/OPC фиксируются runner из существующих CLI интерфейсов; root/schema/cache не выводятся из случайного cwd. Не объявлять эти команды выполненными до реализации runner.

### 6.3. Передача следующему исполнителю

По завершении карточки сообщать: что изменено, список имен тестов, фактический RED→GREEN→mutation, targeted matrix, измененные файлы, незакрытые ограничения. Следующая карточка начинается после выполнения критериев предыдущей. Код общего helper не считается завершением всех его потребителей без сквозных тестов.

Не делать commit без отдельного разрешения пользователя. Не изменять корпус для прохождения приемки. Не устанавливать Word/платные средства автоматически: доступность независимого эталона отражается в status. Отсутствие эталона не препятствует остальным synthetic исправлениям.

## 7. Полнота покрытия и доказательства постановки

| Аудит | Карточка | Основной RED свидетель |
|---|---|---|
| A01 | F05 | PDF 2 pages → write/reopen sections/page geometry |
| A02 | F19 | text+vector: silent loss, затем actual rectangle |
| A03 | F18 | x=96, baseline=122,667 вместо 192/108,8 |
| A04 | F01 | abc→Z не имеет recall=1 |
| A05 | F01 | missing outputs и unmatched invalid не PASS |
| A06 | F03 | normalization/writer loss меняет exit и stage report |
| A07 | F02 | pdf-only compilation/use |
| A08 | F20 | live API/UI содержит normalize loss |
| A09 | F04 | bitmap/crop/math lexical + suffix sequence XSD |
| A10 | F06 | повтор direct italic/bold не выключает оформление |
| A11 | F07 | standalone font payload и script-correct resolution |
| A12 | F11 | dxa0+grid не дает 1 px |
| A13 | F14 | relative width/anchor reference box |
| A14 | F12 | paragraph grey rectangle есть в output |
| A15 | F13 | header/footer bounds ограничивают body |
| A16 | F10 | partial explicit0 не отменяет hanging и не накладывает bullet |
| A17 | F17 | merge region имеет continuation fragments |
| A18 | F16 | frame shift переносит всех детей |
| A19 | F09 | right tab выравнивает конец; leader/TOC baseline |
| A20 | F08 | positive firstLine не перемещает правую границу |
| A21 | F15 | Square и None различаются в строках, collision=0 |

Исходные измерения: [основной аудит](AUDIT.md), [Лайм](visual-lime/DIAGNOSIS.md), [Programming first steps](visual-programming/DIAGNOSIS.md), [Cookie](visual-cookie/DIAGNOSIS.md), [Simple Conditions](visual-conditions/DIAGNOSIS.md), [Thesis](visual-thesis/DIAGNOSIS.md), [Routing/Marketing/Book](visual-routing-marketing-book/DIAGNOSIS.md). Дополнительная проверка при постановке: `plan-scalar-check.docx` из food_template показывает suffix=nothing и порядок `start,numFmt,pStyle,lvlText,lvlJc,suff,pPr`; локальный CT_Lvl требует suff до lvlText.

Структурные границы сверены через graph Tier 2: project D-projects-StrictLib, generation `2026-10-04T12:46:37Z`. Coverage candidate paths не имеет recorded gaps, но freshness=metadata_changed; существенные участки convert/write/layout/CLI/viewer перечитаны непосредственно. Corpus и внешние schemas проверены файловым чтением. Полнота всего graph не заявляется. Числа из аудита — baseline measurements, предложенные новые регрессии **еще не написаны и не имеют RED/GREEN receipts**.
