# Импорт RTF в StrictLib

Дата: 4 октября 2026 года. Статус: план; импортёр и его тестовый harness ещё не реализованы.

## 1. Цель и решение

Добавить самостоятельный workspace-крейт `strict-ooxml-rtf`, который читает RTF-байты и строит существующую `strict_ooxml_wml::model::Document`. Результат пригоден для сохранения единственным существующим writer в Strict DOCX, редактирования в пределах возможностей `strict-ooxml-edit` и вывода существующими SVG/PDF renderer.

RTF содержит структуру и форматирование, поэтому основной путь — прямое преобразование свойств в WML. Не вводить OCR, PDF-посредник, запуск Word/LibreOffice в runtime или отдельную модель для renderer. Независимые приложения нужны только для тестовых эталонов.

Приёмка строится на пользовательском корпусе из 100 файлов и минимальных синтетических fixtures. Полная совместимость со всеми расширениями RTF и идентичная Word пагинация не входят в первоначальное обещание. Поддержка каждой конструкции определяется матрицей и измерением, а не успешным открытием файла.

Связанные решения: [редактирование](EDITING_PLAN.md), [исправления A01–A21](audit-2026-10-04/FIX_PLAN.md). Writer остаётся единственным; байты медиа не переносятся в DOM; исходные отчёты стадий сохраняются.

## 2. Исходный корпус и подтверждённые факты

Корпус: `testdata/CC0_RTF/`; манифест: `testdata/CC0_RTF/manifest.json`.

Проверка 2026-10-04 непосредственно по файловой системе:

| Измерение | Результат |
|---|---:|
| RTF-файлов | 100 |
| Записей манифеста / уникальных имён | 100 / 100 |
| Общий размер | 52 185 834 байта |
| Минимальный / максимальный размер | 204 / 8 352 331 байт |
| Несовпадений размера или SHA-256 с манифестом | 0 |

Манифест содержит `filename`, `identifier`, `original_filename`, `title`, `language`, `license_url`, `byte_size`, `download_url`, `sha256`. Ссылки на лицензии — метаданные происхождения, а не результат отдельного правового аудита. Не перекачивать корпус, не менять исходники и не включать содержимое документов в публичные артефакты автоматически.

Эта проверка подтверждает идентичность входов. Валидность RTF, набор конструкций, наличие повреждений и качество импорта ещё не измерены. `testdata` исключена из графового индекса: корпус проверяется напрямую.

В R01 зафиксировать все 100 входов и полный учёт: отсутствующий файл, лишний RTF, повтор имени, несовпадение размера/хеша — ошибка подготовки корпуса. Не сокращать знаменатель до успешно прочитанных файлов.

## 3. Границы параллельной работы

Основная область владения RTF-очереди: `strict-ooxml-rtf/**`, этот документ, новые `docs/rtf/**` и `xtool/rtf/**`. Эти каталоги — планируемые, кроме данного документа. Корпус читается без изменений.

Runtime-зависимости: `strict-ooxml-wml`, `strict-ooxml-core`, `encoding_rs`, `thiserror`. Writer, reader и renderer используются как dev-зависимости для интеграционных проверок; JSON/hashing/oracle orchestration остаются в harness. Соблюдать workspace MSRV и lint policy.

Вести реализацию в отдельном worktree/ветке. Текущие незакоммиченные исправления renderer, writer, convert, facade, CLI и viewer не отменять и не переносить механически в новый worktree. В receipt указывать точную базу и дерево исходников: HEAD недостаточен при локальных изменениях.

`Cargo.toml`, `Cargo.lock`, CI, facade, CLI и viewer — общие точки интеграции. Подключать их согласованным небольшим изменением после стабилизации библиотечного контракта. До этого использовать примеры и тесты нового крейта. Не расширять `strict-ooxml-convert`: он сейчас обслуживает PDF.

Изменения общих WML/core типов оформлять отдельной задачей с потребителями и проверками. Для базового импорта ожидается достаточно текущих публичных типов; это проверяется scaffold и fixtures, а не объявляется доказанным для всех 100 файлов. Возникающие пробелы модели не обходить скрытыми XML-фрагментами.

Коммиты — только после отдельного разрешения пользователя. Текущий этап меняет документацию, без создания крейта или подключения workspace.

## 4. Предлагаемый публичный контракт

Ниже проект API, а не существующие символы:

```rust
pub fn import(bytes: &[u8], options: &RtfOptions)
    -> Result<RtfImported, RtfError>;

pub struct RtfImported {
    pub document: strict_ooxml_wml::model::Document,
    pub media: Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>,
    pub report: RtfReport,
}
```

Вход — байты, не `&str`: файл может содержать однобайтовые/многобайтовые кодировки и бинарные области. Импортёр не читает файлы и не пишет результат сам. `import_reader` и потоковый публичный API — отдельное последующее решение; не обещать их в R02.

`RtfOptions`: `limits`, `unsupported_policy`, `invalid_text_policy`. По умолчанию неподдержанное содержимое обрабатывается с отчётом (`ReportAndContinue`), некорректное текстовое кодирование отвергается (`Error`). Опциональные `Reject` для неподдержанных конструкций и `ReplaceAndReport` для декодирования задаются явно. Синтаксические ошибки и превышение лимитов всегда фатальны; best-effort не означает ремонт скобок/обрезанного binary.

`RtfError`: `InvalidHeader`, `Malformed { offset, reason }`, `InvalidEncoding { offset, reason }`, `LimitExceeded { offset, limit, observed }`, `UnsupportedRejected { span, feature }`, `ModelInvariant { reason }`. `offset`/`span` — нулевые байтовые позиции исходного RTF, полуинтервал `[start,end)`. Не подменять их XML SourceLocation.

`DocumentSource` содержит синтетические выходные part ids, например `/word/document.xml`; они обозначают будущий DOCX, а не исходный RTF. Там, где текущий DOM требует XML location, использовать единый явно документированный synthetic marker; provenance RTF хранить отдельно в отчёте. Не объявлять parser SupportModel актуальной оценкой всего преобразования.

Для изображений создавать согласованные `MediaIndex`, drawing references и media bytes. Writer получает их через существующий `MediaBag`; успешная запись недостаточна без повторного открытия и проверки ссылок/content types. Не создавать фиктивные package relationships ради прохождения теста.

## 5. Разбор и ограничения ресурсов

Внутренние слои: byte lexer → ограниченный стек групп и destinations → декодирование текста/ресурсов → построение WML → проверка модели и отчёт. Не применять регулярные выражения как основной парсер. Сканер не должен принимать управляющие слова внутри `\\binN` за синтаксис RTF.

Обязательные свойства разбора:

- Группы сохраняют и восстанавливают character/paragraph/section state и параметры Unicode fallback; `\\plain` и `\\pard` сбрасывают соответствующие свойства.
- Control word, signed parameter, optional delimiter, control symbol, escapes `\\{`, `\\}`, `\\\\`, `\\'hh` различаются; CR/LF синтаксиса не становятся видимым текстом.
- Учитывать `\\ansicpg`, font charset/codepage, смену текущего шрифта, `\\uN` как UTF-16 code unit, `\\ucN` и surrogate pairs. Не пропускать Unicode fallback простым срезом N байт без правил RTF. `\\upr`/`\\ud` не дают двойной текст.
- Неизвестный ignorable destination `\\*` пропускается целиком; обычное неизвестное control word не превращается в текст и не удаляет окружающий текст. Сохранять диагностику неоднозначного влияния.
- `\\binN` потребляет ровно N сырых байт; отрицательное N, переполнение, обрезанные данные, лишние закрытия групп и незакрытые группы возвращают ошибку с позицией.
- Разбор выполняется итеративно по группам; nesting не превращается в неограниченную Rust recursion. Числа, размеры и счётчики проверяются до выделения памяти.

Предлагаемые default limits; R01/R03 проверяют их на корпусе и фиксируют изменения до приёмки:

| Лимит | Default |
|---|---:|
| Вход | 64 MiB |
| Глубина групп | 256 |
| Токены | 10 000 000 |
| Декодированный текст | 16 000 000 Unicode scalars |
| Узлы модели суммарно | 1 000 000 |
| Строки / ячейки таблиц суммарно | 100 000 / 1 000 000 |
| Декодированные bytes одного / всех media | 32 / 64 MiB |
| Определения fonts/styles/list levels суммарно | 100 000 |
| Детализированные issues | 10 000 |

Лимиты независимы и настраиваются вызывающим кодом. Для issues сверх детализации сохранять агрегаты по feature/severity и общий счётчик; truncation отчёта указывать явно, outcome не улучшать. Bounding input не заменяет bounding DOM/media. В harness процесс на файл имеет timeout и ограниченный ресурсный бюджет; это не обещание wall-clock cancellation внутри синхронного API.

## 6. Матрица поддержки

| Этап | Поддерживаемые конструкции | Ограничения |
|---|---|---|
| R03 | текст/Unicode/кодировки, paragraphs/runs, tabs/line/page breaks; bold/italic/underline/strike, size/font/color, superscript/subscript; alignment, indents, spacing, page size/margins, базовый RTL | Сложное bidi/shaping проверяется отдельно от сохранности логического текста |
| R04 | font/color tables, paragraph/character styles, list/listoverride tables, ordered/bullet multi-level lists | Циклы и отсутствующие references диагностируются; numbering не подменяется текстовыми маркерами |
| R05 | обычные таблицы, размеры, borders/shading, horizontal/vertical merges | Nested tables — отдельная карточка после census; отсутствие поддержки даёт named loss |
| R06 | inline PNG/JPEG, section boundaries, headers/footers, footnotes/endnotes, обычные hyperlinks | Размер/положение сохранять в доступных моделях; unsupported media не исчезает из отчёта |
| После R06 | floating shapes/textboxes, WMF/EMF, OLE, формулы, comments, revisions, сложные fields, nested tables | Только по отдельным карточкам с модельным контрактом и эталоном |

Не вычислять поля и не выполнять OLE/внешние ссылки. Для полей сохранять доступный result text; утрату инструкции/семантики отмечать. Для неподдержанного контейнера с видимым текстом сохранять текст в логическом порядке, когда структура позволяет это доказать, и записывать потерю структуры. Если нельзя — явно учитывать утраченное содержимое; не публиковать результат как Clean.

## 7. Отчёт и инварианты

`RtfReport` содержит version=1, перечень issues и census конструкций; issue: stable id, severity (`Info`, `Unsupported`, `Lossy`, `Error`), feature, source span, count, action (`Preserved`, `Flattened`, `Skipped`, `Replaced`, `Rejected`), reason. Некритическое unknown control word не объявлять автоматически lossless: классификация должна опираться на таблицу известных нейтральных слов.

Исходы используют существующий `PipelineOutcome`: Clean — без существенных потерь; Degraded — результат существует с Unsupported/Lossy/replacements; Failed — fatal error, недействительная модель или нарушенный учёт. Фатальный error несёт доступную диагностику до точки отказа; полного census после отказа не обещать.

Для успешного разбора каждая обнаруженная конструкция получает disposition. Для известных unsupported destinations учитывать группу, span, размер и видимый текст, если он извлекается; не выдавать один счётчик на всю невидимую область за исчерпывающий census вложенных features. Проверять `detected = preserved + transformed + skipped + rejected` для каждого уровня census, но дополнительно проверять текст, свойства и ресурсы: равенство счётчиков не доказывает отсутствие потерь.

При интеграции адаптировать RTF report в существующую стадию `Convert`, с id `rtf.*`, без нового параллельного pipeline. RTF byte spans передавать через location без выдуманных part/page. Потери импорта и writer не дедуплицировать. Support report WML и fidelity report RTF — разные измерения.

CLI после R08: 0 Clean, 1 Degraded с созданным результатом, 2 Failed. Запрошенный report, который не удалось записать, означает Failed. Library return `Ok` означает наличие модели, а не Clean.

## 8. Проверки и корпусный гейт

Хранить отдельные verdicts: integrity, parse, text, structure/properties, write/reopen, Strict XSD, media, visual, resources. Итог не сводить к числу «открылось». Для каждого verdict допустимы Pass/Fail/Blocked/NotApplicable; отсутствие oracle — Blocked, не Pass.

Минимальные fixtures проверяют независимые ожидаемые строки, свойства и число объектов. Обязательные случаи: кириллица/CP1251, несколько codepages/fonts, Unicode fallback и surrogate pairs, combining marks, RTL logical order, escaped braces, scopes/reset, unknown destinations, binary с braces/backslashes, пустой документ, повреждённые группы/hex/числа, budgets, style/list references, merges, media и section boundaries. Property/fuzz tests проверяют bounded failure, отсутствие panic/hang и утечки состояния групп.

Корпусная процедура:

1. Проверить manifest и hashes всех 100 файлов. Создать census control words/destinations, encoding/fonts, styles/lists/tables/images/sections и invalid syntax candidates. Lexer census не считать полноценной проверкой семантики.
2. Заморозить expectations на каждый sha256 до GREEN: expected outcome, известные unsupported features, проверяемые text/structure facts и версия oracle. Невалидный вход может иметь ожидаемый Failed только с независимо воспроизводимой причиной. Не добавлять exceptions автоматически из результата импортёра.
3. Построить независимый text/structure oracle через закреплённую версию Word или LibreOffice и его DOCX/XML-выход; записать application/version/options/fonts. Для спорных случаев проверять спецификацию и минимальную fixture. Ни текст собственного импортёра, ни его round trip не могут быть единственным эталоном.
4. Сравнить логический текст по областям body/notes/headers/footers, сохраняя tabs, breaks, NBSP, paragraph boundaries. Любая нормализация сравнения явно перечисляется заранее; не удалять пробелы/пунктуацию ради совпадения.
5. Импортировать, сохранить единственным writer, повторно открыть Strict reader, проверить текст/свойства/section references и every media hash/reference/content type. Валидировать выход закреплёнными Strict schemas через существующий gate adapter; document-part-only check не заменяет проверку всех созданных частей.
6. Отдельно сравнить визуальные эталоны всех 100 применимых файлов: raster output/reference при одинаковых fonts/DPI. Сначала зафиксировать baseline и метод/пороги, затем применять; не обещать SSIM до census. Renderer-дефект остаётся отдельным Blocked/Fail визуального гейта и не маскирует importer defect.
7. Запускать corpus harness с isolation, timeout, временем, peak RSS и размерами выхода; отсутствие measurement не объявлять соблюдением памяти. Итоговые p50/p95/max и бюджеты закрепить в R07 на известной машине.

Итоговый JSON содержит ровно 100 строк по manifest identity, даже при crash/timeout. Отсутствие корпуса, oracle, schemas или шрифта не становится успешным skip. Дополнительно сводить Clean/Degraded/Failed, expected/unexpected, число unsupported constructs и непроверенные гейты. Финальная приёмка требует 0 unexpected outcomes, 0 необъяснённых расхождений поддержанного текста/свойств, 0 silent losses; Degraded нельзя переименовывать в полную совместимость.

Fixtures и schema/text/media проверки запускаются для каждой карточки; полный корпус — на границах R03–R07 и после изменений parser/state/mapping. Визуальные regressions повторять для затронутых классов и полный набор перед итогом. Corpus oracle artifacts не обновлять автоматически.

## 9. Очередь реализации и критерии приёмки

| ID | Работа / зависимости | Артефакт и «готово, когда» |
|---|---|---|
| R01 | corpus census и oracle contract; начинает сейчас после документации | `docs/rtf/CORPUS.md`, harness inventory и expectations; 100/100 hashes, все inputs учтены; feature distribution и пробелы oracle явно перечислены |
| R02 | API scaffold, errors/limits/report, empty WML adapter; после R01 | новый крейт; API компилируется, основные поведенческие RED assertions падают на scaffold; ошибка компиляции не считается RED |
| R03 | lexer/state/decoding и базовый WML; после R02 | GREEN text/property fixtures + corpus baseline; unknown/fatal/resource paths покрыты; все 100 имеют verdict |
| R04 | styles/fonts/lists; после R03 | exact property/inheritance/numbering fixtures, census-supported cases и reopened output совпадают с эталоном |
| R05 | таблицы и merges; после R03, styles после R04 | независимые expected grids/merge regions; текст не дублируется/не исчезает; unsupported nesting учтено |
| R06 | media/sections/regions/notes/links; после R04/R05 | reopening подтверждает hashes и ссылки, границы секций и регионов; unsupported formats имеют отчёт |
| R07 | hardening, full corpus, Strict/visual/resource gates; после R03–R06 | `docs/rtf/ACCEPTANCE.md`; 100 строк, 0 unexpected, 0 silent loss, воспроизводимые measurements; каждый Blocked не даёт закрыть соответствующий gate |
| R08 | facade feature `rtf`, CLI import, viewer adapter и CI; после библиотечного R07 и готовности общих pipeline/UI контрактов | feature matrix и CLI 0/1/2, source/media/report lifetime; 100 inputs через публичный путь; green других очередей сохраняется |

R05/R06 не делегируются параллельно без явного разделения shared builder/report файлов. Импортёр как отдельная очередь может идти параллельно editing и renderer audit. Закрытие R03 — базовый импорт, R07 — документированный поддержанный профиль, R08 — пользовательская интеграция; это разные утверждения.

Для каждой карточки: минимальная fixture и компилируемый RED → реализация → тот же GREEN → обратный контроль в изолированной копии. Не менять рабочее дерево других работ для mutation check. Для новой функциональности scaffold даёт исходную проверяемую неработоспособность; не требовать доказать дефект ранее отсутствовавшего API.

Артефакты выполнения: `target/rtf-import/<Rnn>/{red,green,mutation,corpus}/`, `receipt.json`. Receipt содержит source tree hash/commit + dirty diff hash, corpus manifest hash и per-input sha256, команды и exit codes, versions/oracles/schema/font hashes, limits/options, outcomes, timings/RSS и известные ограничения. Эти пути и команды harness фиксируются окончательно в R01; пока harness не создан, они не считаются выполненными проверками.

## 10. Следующий шаг и переоценка сроков

Следующий шаг — R01: автоматизировать integrity/census, выявить реальные сложные классы всех 100 файлов, определить доступный независимый oracle и выпустить поддержку/expectations matrix. Только затем уточнять объём R03–R06 и сроки. Предыдущая оценка 5–10 дней для ограниченного MVP и ещё 2–4 недели для расширения остаётся ориентиром, а не обязательством по этому корпусу.

Исходники RTF-импортёра пока не создавались. Проверка hashes корпуса выполнена; parsing, fidelity, XSD, renderer и performance gates по RTF не запускались.
