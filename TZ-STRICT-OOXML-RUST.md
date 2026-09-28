# Техническое задание (расширенная редакция)

## на разработку библиотеки чтения, нормализации и отображения документов WordprocessingML Strict (ISO/IEC 29500-1:2008) на Rust

**Шифр:** TZ-STRICT-OOXML-RUST  
**Версия:** 2.0 (расширение редакции 1.0, см. `base_target.md`)  
**Статус:** Черновик к утверждению  
**Дата:** 2026-09-28  

> Настоящая редакция полностью сохраняет положения версии 1.0 и дополняет их:
> детальной архитектурой, спецификацией публичного API и модели данных,
> требованиями по безопасности и ресурсным лимитам, моделью ошибок,
> расширенной системой отчётности и **новой подсистемой нормализации
> Transitional → Strict (раздел 10)**.

---

## Содержание

1. Общие сведения
2. Цели и задачи
3. Область работ (Scope)
4. Соответствие версий и терминология
5. Архитектура
6. Публичный API (спецификация)
7. Внутренняя модель данных (DOM)
8. Подсистема OPC / упаковки
9. Подсистема XML и пространств имён
10. Подсистема нормализации Transitional → Strict
11. Feature Report (схема v2)
12. Безопасность и ресурсные лимиты
13. Модель ошибок
14. Нефункциональные требования
15. Этапы разработки
16. Общие критерии приёмки проекта
17. Тестирование
18. CI/CD и релизный процесс
19. Риски
20. Поставка
21. Приложения

---

## 1. Общие сведения

### 1.1. Наименование работ
Разработка библиотеки на языке Rust для чтения, разбора, **нормализации**, отображения
и анализа поддержки документов формата **WordprocessingML Strict**, определённого в
**ISO/IEC 29500-1:2008 (Strict, Part 1)**, с возможностью чтения документов
**Transitional (ISO/IEC 29500-4:2008, Part 4)** в режиме нормализации к модели Strict.

### 1.2. Заказчик
Не указан.

### 1.3. Исполнитель
Не указан.

### 1.4. Основание для разработки
Необходимость создания открытой, безопасной и производительной библиотеки для работы
с «чистым» подмножеством OOXML без нативной поддержки устаревших механизмов Transitional,
но с контролируемой нормализацией Transitional-документов к Strict-модели при чтении.

### 1.5. Термины и сокращения

| Термин | Определение |
|---|---|
| OOXML | Office Open XML, ISO/IEC 29500 |
| Strict | ISO/IEC 29500-1:2008, Part 1 |
| Transitional | ISO/IEC 29500-4:2008, Part 4 |
| WML | WordprocessingML |
| OPC | Open Packaging Conventions (ISO/IEC 29500-2) |
| DrawingML | Язык разметки графики в OOXML |
| VML | Vector Markup Language (Transitional-only, не поддерживается в Strict) |
| MCE | Markup Compatibility and Extensibility |
| DOM | Внутренняя объектная модель документа |
| Feature Report | Отчёт о поддержке механизмов конкретного файла |
| Нормализация | Преобразование Transitional-разметки к модели Strict |
| Loss Report | Часть Feature Report, описывающая потери при нормализации |
| Part | Часть пакета OPC (например, `/word/document.xml`) |
| Relationship | Связь OPC между частями пакета |

### 1.6. Область стандарта
В рамках данного ТЗ под «Strict» понимается:
- **WordprocessingML Strict** — основная область;
- **OPC** — упаковка `.docx`;
- **DrawingML** — в объёме, необходимом для встроенной графики;
- **Shared MathML** — опционально, на поздних этапах.

SpreadsheetML и PresentationML в данное ТЗ не входят. VML и OLE нативно не поддерживаются.

---

## 2. Цели и задачи

### 2.1. Цель
Создать Rust-библиотеку, которая:
1. Читает документы `.docx`, соответствующие Strict.
2. Строит внутреннюю объектную модель документа (DOM Strict).
3. Отображает документ в векторном формате (SVG) и/или растровом виде.
4. Для каждого файла формирует отчёт: какие механизмы Strict поддержаны полностью,
   частично или не поддержаны.
5. Не падает и не вызывает неопределённого поведения на некорректных, повреждённых
   или Transitional-файлах.
6. **Способна прочитать Transitional-документ и нормализовать его к модели Strict
   с обязательным отчётом о потерях (см. раздел 10).**

### 2.2. Задачи
- Реализовать парсер OPC и XML.
- Реализовать парсер WordprocessingML Strict.
- Реализовать внутреннюю DOM.
- Реализовать систему отчётности о поддержке.
- Реализовать подсистему нормализации Transitional → Strict.
- Реализовать базовый рендеринг в SVG.
- Обеспечить тестовый корпус и инструменты проверки.
- Обеспечить ресурсную безопасность (лимиты, fuzz).
- Подготовить документацию и опубликовать крейты.

### 2.3. Ключевые принципы проектирования
1. **Fail-safe, а не fail-fast для входных данных.** Любой вход — потенциально враждебный;
   ошибка возвращается, а не приводит к панике.
2. **Разделение разбора и нормализации.** Strict-модель строится только после
   нормализации слоя входных данных (namespace/relation/attribute).
3. **Table-driven.** Маппинги Transitional → Strict задаются данными (таблицы),
   а не разбросанной логикой в коде.
4. **Наблюдаемость.** Каждое применённое преобразование и каждая потеря фиксируются.
5. **Опциональность тяжёлых зависимостей** через cargo features.

---

## 3. Область работ (Scope)

### 3.1. Входит в scope
- Чтение `.docx` Strict.
- Чтение `.docx` Transitional в режиме нормализации (раздел 10).
- Валидация пространств имён Strict: `purl.oclc.org/ooxml/...`.
- Разбор основных частей пакета: `document.xml`, `styles.xml`, `numbering.xml`,
  `settings.xml`, `theme`, `fontTable`, `relationships`, `media`, `[Content_Types].xml`.
- Построение DOM: секции, абзацы, runs, таблицы, стили, списки, поля,
  сноски/концевые сноски, колонтитулы, изображения DrawingML.
- Отображение в SVG.
- Формирование Feature Report в JSON и человекочитаемом виде.
- Обработка ошибок без паник.
- CLI-утилита для проверки, инспекции, нормализации и рендеринга.

### 3.2. Не входит в scope
- Редактирование документов.
- Сохранение в Strict (сериализация результата нормализации на диск — вне scope MVP;
  нормализация выполняется в памяти).
- Полная поддержка всех механизмов Transitional.
- Нативная поддержка VML.
- SpreadsheetML и PresentationML.
- Побитовая совместимость с Microsoft Word.
- Поддержка макросов, ActiveX, OLE.
- Совместная работа в реальном времени.

### 3.3. Матрица покрытия по этапам

| Механизм | MVP (Этапы 0–4) | Расширение (Этап 5) | Нормализация (Этап 6) |
|---|---|---|---|
| OPC / ZIP | Полное | — | — |
| XML / Namespaces | Полное | — | — |
| Абзацы, runs, текст | Полное | — | — |
| Таблицы | Базовые | Сложные | — |
| Списки/нумерация | Базовые | Полные | — |
| Стили | Базовые | Полные + темы | — |
| DrawingML inline | Базовое | Фигуры/группы/якоря | VML → отчёт/растр |
| Сноски/концевые | — | Полное | — |
| Колонтитулы | — | Полное | — |
| Поля (fields) | Частично | Полное | — |
| MathML | — | Опционально | — |
| Transitional → Strict | — | — | Полное (нормализация) |

---

## 4. Соответствие версий и терминология

### 4.1. Определение версии пакета
Библиотека различает три состояния входного документа:

| Состояние | Критерий определения | Поведение по умолчанию |
|---|---|---|
| `Strict` | Все ключевые пространства имён принадлежат `purl.oclc.org/ooxml/...` | Чтение напрямую |
| `Transitional` | Ключевые пространства имён принадлежат `schemas.openxmlformats.org/...` | Отклонение (Этапы 1–5) либо нормализация (Этап 6) |
| `Mixed` | Обнаружены оба семейства в пределах одного документа | Ошибка `MixedConformance` |

Определение выполняется по:
1. пространству имён корневого элемента главной части документа;
2. пространствам имён частей `styles.xml`, `numbering.xml`, `settings.xml`;
3. типам relationships главной части;
4. содержимому `[Content_Types].xml`.

**Приоритет:** явное определение по namespace является основным; несовпадающие сигналы
приводят к `MixedConformance`, а не к молчаливому выбору.

### 4.2. Политика чтения
- Режим по умолчанию (Этапы 1–5): `StrictOnly`. Transitional-файл → ошибка
  `TransitionalNotSupported` с локацией первого несоответствия.
- Режим Этапа 6: `Normalize`. Transitional-файл читается и нормализуется к Strict-модели,
  результат помечается `normalized: true`, все потери попадают в Loss Report.
- Режим `Permissive` (опционально): нормализация + продолжение при частично неподдержанных
  механизмах с фиксацией в отчёте.

---

## 5. Архитектура

### 5.1. Крейты

| Крейт | Назначение |
|---|---|
| `strict-ooxml-core` | OPC, ZIP, XML, пространства имён, нормализация сырого слоя, лимиты, ошибки |
| `strict-ooxml-wml` | Модель и парсер WordprocessingML Strict |
| `strict-ooxml-report` | Система отчётности о поддержке и потерь |
| `strict-ooxml-render-svg` | Рендеринг в SVG |
| `strict-ooxml` | Мета-крейт с публичным API |
| `strict-ooxml-cli` | CLI-утилита |

### 5.2. Граф зависимостей

```
strict-ooxml-cli
      │
      ▼
strict-ooxml ────────┬──────────► strict-ooxml-report
      │              │
      ▼              ▼
strict-ooxml-wml ─► strict-ooxml-render-svg
      │
      ▼
strict-ooxml-core
```

Правила:
- `core` не зависит ни от одного крейта проекта.
- `wml` зависит только от `core`.
- `render-svg` зависит от `wml` (читает DOM) — не наоборот.
- `report` — независимый от `render`, используется `wml`/`cli`.
- циклические зависимости запрещены и проверяются в CI (`cargo-deny`/`cargo tree`).

### 5.3. Основные слои
1. **Слой упаковки (core::opc)** — ZIP, `[Content_Types].xml`, relationships, part lookup.
2. **Слой XML (core::xml)** — потоковый namespace-aware парсер, лимиты глубины/размера.
3. **Слой нормализации сырого уровня (core::normalize)** — пространства имён,
   типы relationships, детекция conformance. Применяется до модели.
4. **Слой модели (wml::dom)** — DOM Strict.
5. **Слой анализа (report)** — Feature Report + Loss Report.
6. **Слой рендеринга (render-svg)** — SVG backend.
7. **Публичный API (strict-ooxml)** — безопасные обёртки, builder, опции.

### 5.4. Cargo features

| Feature | Крейт | Назначение |
|---|---|---|
| `default` | meta | `svg`, `report` |
| `svg` | meta | включает `strict-ooxml-render-svg` |
| `report` | meta | включает `strict-ooxml-report` |
| `round-trip-normalize` | meta | включает нормализацию Transitional (Этап 6) |
| `parallel` | core, wml | параллельный парсинг частей (rayon) |
| `wasm` | meta | подготовка к WASM (без std-файловых API) |

---

## 6. Публичный API (спецификация)

### 6.1. Базовое использование

```rust
use strict_ooxml::{StrictDocument, OpenOptions, RenderOptions, ConformancePolicy};

let doc = StrictDocument::open_with(
    "document.docx",
    OpenOptions::default()
        .conformance(ConformancePolicy::StrictOnly)
        .limits(ResourceLimits::default()),
)?;

let report = doc.support_report();
for issue in report.issues() {
    eprintln!("{}: {} at {:?}", issue.severity, issue.message, issue.location);
}

let svg = doc.render_svg(&RenderOptions::default())?;
std::fs::write("page.svg", svg)?;
```

### 6.2. Чтение из памяти

```rust
use std::io::Cursor;
use strict_ooxml::{StrictDocument, OpenOptions};

let bytes: Vec<u8> = std::fs::read("document.docx")?;
let doc = StrictDocument::open_reader(Cursor::new(bytes), OpenOptions::default())?;
```

### 6.3. Опции открытия

```rust
pub struct OpenOptions {
    pub conformance: ConformancePolicy,
    pub limits: ResourceLimits,
    pub resolve_theme: bool,
    pub load_media: bool,
    pub normalization: NormalizationOptions,
}

pub enum ConformancePolicy {
    StrictOnly,
    Normalize,
    Permissive,
}

pub struct NormalizationOptions {
    pub enabled: bool,
    pub direction_policy: DirectionPolicy, // MapToStartEnd | Keep
    pub vml_fallback: VmlFallback,         // Report | RasterizeIfPossible | Drop
    pub mce: McePolicy,                    // ProcessChoice | PreferFallback | Report
    pub strictness: NormalizationStrictness, // Strict | Lenient
}
```

### 6.4. Основные публичные типы

| Тип | Назначение |
|---|---|
| `StrictDocument` | Открытый документ (владеет пакетом и DOM) |
| `OpenOptions`, `RenderOptions` | Опции открытия/рендеринга |
| `ResourceLimits` | Лимиты ресурсов (см. 12) |
| `SupportReport` | Результат анализа поддержки |
| `Issue`, `Severity`, `SupportStatus` | Элементы отчёта |
| `NormalizationReport` | Применённые преобразования и потери |
| `StrictError` | Общая ошибка библиотеки |

### 6.5. Инварианты публичного API
- Ни один публичный метод не возвращает `Result` с `panic`; паники исключены.
- Все публичные типы реализуют `Send + Sync` (при отсутствии явных исключений, документируемых).
- API следует SemVer после 1.0; до 1.0 допускаются ломающие изменения с минорным бампом.
- Все публичные элементы документированы (`#![deny(missing_docs)]`).

---

## 7. Внутренняя модель данных (DOM)

### 7.1. Иерархия (упрощённо)

```
StrictDocument
├── Package (OPC)
│   ├── parts: PartId -> Part
│   └── relationships: RelId -> Relationship
├── DocumentModel
│   ├── body: Vec<Block>
│   ├── sections: Vec<SectionProperties>
│   ├── styles: StyleTable
│   ├── numbering: NumberingTable
│   ├── settings: Settings
│   ├── theme: Theme
│   ├── font_table: FontTable
│   ├── footnotes / endnotes
│   └── headers / footers
└── MediaStore
    └── items: PartId -> MediaItem
```

### 7.2. Блочная и inline-модель

```rust
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    SdtBlock(SdtContainer),   // структурированные элементы
    AltChunk(AltChunkInfo),   // не рендерится, фиксируется в отчёте
}

pub enum Inline {
    Run(Run),
    Hyperlink(Hyperlink),
    Drawing(Drawing),
    Field(Field),
    Break(BreakKind),
    Tab,
    SdtInline(SdtContainer),
    FootnoteRef(u32),
    EndnoteRef(u32),
}
```

### 7.3. Требования к DOM
- Все узлы хранят `SourceLocation` (part id + offset/строка) для отчётности.
- Строки хранятся в общих пулах (`Arc<str>`/арена) для экономии памяти.
- Модель **неизменяема** после построения (immutable), что упрощает `Sync`.
- Отсутствие `unsafe` в WML-модели (кроме, при необходимости, документированных узких мест).
- Неизвестные/неподдержанные элементы сохраняются как `Opaque`-узлы для отчётности,
  а не отбрасываются молча.

### 7.4. SourceLocation

```rust
pub struct SourceLocation {
    pub part: PartId,        // e.g. "/word/document.xml"
    pub line: u32,           // 1-based
    pub column: u32,         // 1-based
    pub byte_offset: u64,
    pub xpath: Option<CompactXPath>,
}
```

---

## 8. Подсистема OPC / упаковки

### 8.1. Требования
- Поддержка ZIP (методы `store` и `deflate`). Иные методы → `UnsupportedCompression`.
- Разбор `[Content_Types].xml`: элементы `Default` и `Override`.
- Разбор `_rels/*.rels`: relationships с `Id`, `Type`, `Target`, `TargetMode`.
- Разрешение относительных target-путей и нормализация (`..`, `.`, ведущий `/`).
- Обнаружение external-связей (`TargetMode="External"`) — не загружаются, фиксируются.
- Поддержка ZIP64 (опционально, но желательно) и корректная обработка
  больших центральных директорий.
- Доступ к части по `PartId` потоково (не держать всё в памяти без необходимости).

### 8.2. Защита (обязательно)
- Защита от ZIP-bomb: лимит на суммарный распакованный размер, коэффициент сжатия,
  число записей (см. 12).
- Отклонение путей с `..`, абсолютных путей, дубликатов имён (проверка на
  path traversal и подмену частей).
- Лимит на рекурсию relationships между частями.
- Центральная директория парсится с проверкой границ; повреждённый ZIP → ошибка, не паника.

### 8.3. Модель данных OPC

```rust
pub struct PartId(Arc<str>); // нормализованный абсолютный путь, e.g. "/word/document.xml"

pub struct Relationship {
    pub id: String,
    pub rel_type: RelType,      // нормализованный тип
    pub target: String,         // raw
    pub target_mode: TargetMode, // Internal | External
    pub resolved: Option<PartId>,// для Internal
}

pub enum RelType {
    OfficeDocument,
    Styles,
    Numbering,
    Settings,
    Theme,
    FontTable,
    Image,
    Hyperlink,
    Header,
    Footer,
    Footnotes,
    Endnotes,
    Other(String), // сырой URI
}
```

---

## 9. Подсистема XML и пространств имён

### 9.1. XML-парсер
- Потоковый (pull/SAX), без построения дерева по умолчанию.
- Обязательная поддержка: декларации, namespace-префиксы, CDATA, комментарии (игнор),
  processing instructions (игнор), корректная обработка `xml:space`.
- Кодировки: UTF-8 (основная), UTF-16 (BOM), ограниченный набор через `encoding_rs`.
- **Запрещено:** разрешение внешних сущностей (XXE), DTD-обработка с подстановкой
  сущностей, «billion laughs» (см. 12).

### 9.2. Резолвинг имён
Для каждого элемента/атрибута парсер возвращает:
```rust
pub struct QName<'a> {
    pub ns: Option<NsUri<'a>>, // разрешённый URI
    pub prefix: Option<&'a str>,
    pub local: &'a str,
}
```
- Разрешение префикса в URI выполняется с учётом scope.
- Неизвестный префикс → ошибка `UnboundPrefix` (не паника).

### 9.3. Реестр пространств имён
Реестр содержит соответствие Transitional ↔ Strict и флаги поддержки.

| Область | Transitional | Strict | В scope |
|---|---|---|---|
| WordprocessingML | `.../wordprocessingml/2006/main` | `.../wordprocessingml/main` | Да |
| Office relations | `.../officeDocument/2006/relationships` | `.../officeDocument/relationships` | Да |
| DrawingML main | `.../drawingml/2006/main` | `.../drawingml/main` | Да |
| DrawingML wp | `.../drawingml/2006/wordprocessingDrawing` | `.../drawingml/wordprocessingDrawing` | Да |
| DrawingML picture | `.../drawingml/2006/picture` | `.../drawingml/picture` | Да |
| DrawingML chart | `.../drawingml/2006/chart` | `.../drawingml/chart` | Частично |
| DrawingML diagram | `.../drawingml/2006/diagram` | `.../drawingml/diagram` | Частично |
| DrawingML spreadsheetDrawing | `.../drawingml/2006/spreadsheetDrawing` | `.../drawingml/spreadsheetDrawing` | Нет |
| DrawingML chartDrawing | `.../drawingml/2006/chartDrawing` | `.../drawingml/chartDrawing` | Нет |
| DrawingML lockedCanvas | `.../drawingml/2006/lockedCanvas` | `.../drawingml/lockedCanvas` | Нет |
| DrawingML compatibility | `.../drawingml/2006/compatibility` | `.../drawingml/compatibility` | Нет |
| Extended properties | `.../officeDocument/2006/extended-properties` | `.../officeDocument/extendedProperties` | Да (метаданные) |
| Custom properties | `.../officeDocument/2006/custom-properties` | `.../officeDocument/customProperties` | Опц. |
| Core properties | `.../package/2006/metadata/core-properties` | `.../package/metadata/coreProperties` | Да (метаданные) |
| Package relationships | `.../package/2006/relationships` | `.../package/relationships` | Да |
| Math | `.../officeDocument/2006/math` | `.../officeDocument/math` | Опц. |
| VML | `urn:schemas-microsoft-com:vml` | — (отсутствует) | Нет |
| MCE | `.../markup-compatibility/2006` | без изменений | Да |

> Примечание: полный формальный перечень берётся из нормативных схем ISO/IEC 29500
> и сверяется инструментом диффа XSD (см. 10.11). Таблица выше — рабочий базис;
> значения с неполной уверенностью помечаются в реестре флагом `Verified`.

### 9.4. Определение conformance

```rust
pub enum Conformance { Strict, Transitional, Mixed, Unknown }

pub fn detect_conformance(
    root_namespaces: &[NsUri],
    rel_types: &[RelType],
    content_types: &ContentTypeIndex,
) -> Result<Conformance, StrictError>;
```

---

## 10. Подсистема нормализации Transitional → Strict

Раздел описывает **методы трансформации Transitional-элементов, атрибутов и значений
в модель Strict**. Нормализация является детерминированной, идемпотентной и полностью
протоколируемой.

### 10.0. Общая стратегия

Нормализация состоит из упорядоченного конвейера стадий. Каждая стадия:
- имеет стабильный идентификатор (`T1`…`T8`);
- принимает на вход сырой или частично нормализованный слой;
- возвращает счётчик применений и список локаций;
- не имеет права молча терять данные: каждое удаление фиксируется в Loss Report.

```
[OPC parts]
    │
    ▼ T0  Определение conformance (Strict / Transitional / Mixed)
    ▼ T1  Нормализация пространств имён (element/attribute/namespace decl)
    ▼ T2  Нормализация типов relationships и content-types
    ▼ T3  Переименование элементов и атрибутов (direction-neutral + прочие)
    ▼ T4  Маппинг значений перечислений (ST_Jc, alignment и др.)
    ▼ T5  Удаление/игнорирование Transitional-only элементов (w:compat и др.)
    ▼ T6  Разрешение MCE (mc:AlternateContent / Choice / Fallback)
    ▼ T7  Обработка legacy-графики (VML, w:pict, w:object) → растр/отчёт
    ▼ T8  Валидация результата против Strict-инвариантов + Loss Report
    │
    ▼ [Strict-normalized layer] → WML DOM parser
```

**Принцип порядка:** нормализация имён (T1–T3) выполняется до семантической
нормализации значений (T4) и до удаления (T5), чтобы таблицы маппинга работали
по каноническим QName Strict.

### 10.1. Классификация различий Transitional ↔ Strict

| Класс | Суть | Метод | Обратимость |
|---|---|---|---|
| C1 Пространства имён | URI namespace-ов различаются | T1 | Полная |
| C2 Типы связей | URI relationship types различаются | T2 | Полная |
| C3 Имена элементов/атрибутов | direction-neutral переименования | T3 | Полная |
| C4 Значения перечислений | `left/right` → `start/end` и пр. | T4 | Полная (с учётом направления) |
| C5 Удалённые механизмы | элементы/атрибуты отсутствуют в Strict | T5 | Частичная/потери |
| C6 Extensibility | MCE-блоки с Transitional-содержимым | T6 | Возможны потери |
| C7 Legacy-графика | VML, `w:pict`, `w:object`, OLE | T7 | Потери (растр/отчёт) |
| C8 Валидность | результат может не проходить Strict-инварианты | T8 | Диагностика |

### 10.2. Метод T1 — Нормализация пространств имён

**Вход:** поток XML-событий сырой части.  
**Действие:** замена namespace URI Transitional → Strict по реестру (9.3);
удаление namespace-деклараций, относящихся к не-Strict расширениям (если они
не нужны после MCE), либо их сохранение как `ignorable`.

**Правила:**
1. Замена применяется к namespace-декларациям (`xmlns`/`xmlns:prefix`) и к
   разрешённым URI элементов/атрибутов.
2. Префиксы сохраняются «как есть» (переименование префиксов не требуется);
   канонизация префиксов выполняется опционально и не влияет на семантику.
3. Неизвестные namespace-ы, не входящие в реестр:
   - если объявлены в `mc:Ignorable` → помечаются ignorable;
   - иначе → фиксируются в отчёте как `UnrecognizedNamespace` (не паника).
4. Смешение Strict и Transitional URI в одном документе после T0 → `MixedConformance`.

**Протокол:** `TransformRecord { id: "T1.namespace", count, from, to, locations }`.

```rust
pub fn normalize_namespace(uri: &str) -> NamespaceOutcome {
    match NS_REGISTRY.lookup(uri) {
        Some(entry) if entry.strict_uri.is_some() => NamespaceOutcome::Mapped(entry.strict_uri),
        Some(entry) => NamespaceOutcome::StrictNative,
        None => NamespaceOutcome::Unknown,
    }
}
```

### 10.3. Метод T2 — Нормализация типов relationships и content types

Relationship type URI также меняются между редакциями. Метод T2 приводит
типы связей к Strict-форме **до** построения индекса частей.

Примеры:

| Transitional rel type (суффикс) | Strict rel type (суффикс) |
|---|---|
| `.../officeDocument/2006/relationships/officeDocument` | `.../officeDocument/relationships/officeDocument` |
| `.../officeDocument/2006/relationships/styles` | `.../officeDocument/relationships/styles` |
| `.../officeDocument/2006/relationships/numbering` | `.../officeDocument/relationships/numbering` |
| `.../officeDocument/2006/relationships/image` | `.../officeDocument/relationships/image` |
| `.../officeDocument/2006/relationships/hyperlink` | `.../officeDocument/relationships/hyperlink` |

Также нормализуются content-type строки (например, main document part:
`application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml`
переходит в Strict-вариант). Полный перечень — в генерируемой таблице (10.11).

**Протокол:** `TransformRecord { id: "T2.reltype", count, locations }`.

### 10.4. Метод T3 — Переименование элементов и атрибутов

Direction-neutral и прочие переименования, задаваемые таблицей `RENAME_TABLE`.
Таблица состоит из записей `(class, transitional_qname, strict_qname, note)`.

Представительный перечень (рабочий базис; расширяется диффом схем):

| Область | Transitional | Strict | Примечание |
|---|---|---|---|
| WML | `w:tblpPr/@w:leftFromText` | `w:tblpPr/@w:startFromText` | direction-neutral |
| WML | `w:tblpPr/@w:rightFromText` | `w:tblpPr/@w:endFromText` | direction-neutral |
| WML | `w:ind/@w:left` | `w:ind/@w:start` | проверить по схеме |
| WML | `w:ind/@w:right` | `w:ind/@w:end` | проверить по схеме |
| WML | `w:pgMar/@w:left` | `w:pgMar/@w:start` | проверить по схеме |
| WML | `w:pgMar/@w:right` | `w:pgMar/@w:end` | проверить по схеме |
| WML | `w:tcMar/@w:left` | `w:tcMar/@w:start` | проверить по схеме |
| WML | `w:tcMar/@w:right` | `w:tcMar/@w:end` | проверить по схеме |
| DrawingML | `a:bodyPr/@compatLnSpc` | *(удалено)* | T5 |
| DrawingML | `a:bodyPr/@fromWordArt` | *(удалено)* | T5 |

> ⚠️ Записи, помеченные «проверить по схеме», обязаны быть подтверждены диффом
> официальных XSD (10.11) на Этапе 0/6. До подтверждения они хранятся с флагом
> `Verified = false` и не применяются к «белому» Strict-корпусу.

Правила T3:
1. Переименование применяется только к полностью разрешённым QName Strict-области.
2. Если переход неоднозначен (атрибут может означать и `start`, и `end` в зависимости
   от контекста/направления текста) — используется `direction_policy`:
   - `MapToStartEnd` (по умолчанию): `left`→`start`, `right`→`end`;
   - `Keep` (диагностический режим): значение сохраняется как расширение, фиксируется loss.
3. Коллизия после переименования (оба `left` и `start` присутствуют) → `AttributeCollision`,
   детерминированное разрешение (Strict-версия приоритетнее) + запись в отчёт.

### 10.5. Метод T4 — Маппинг значений перечислений

Раздельно от имён меняются значения. Таблица `VALUE_MAP`.

Примеры:

| Атрибут (Strict QName) | Transitional значение | Strict значение |
|---|---|---|
| `w:jc/@w:val` | `left` | `start` |
| `w:jc/@w:val` | `right` | `end` |
| `w:jc/@w:val` | `both` | `both` (без изменений) |
| `w:jc/@w:val` | `center` | `center` (без изменений) |
| `w:tblpPr`/`w:tblOverlap` | (проверять) | (проверять) |

Правила T4:
1. Маппинг значений применяется после T3, по каноническим Strict QName.
2. Значение вне таблицы и вне Strict-домена → `InvalidEnumValue` + fallback
   (значение по умолчанию схемы) + запись в отчёт.
3. Для direction-neutral значений учитывается контекст `rtl`/`bidi`:
   `left`/`right` семантически физические, `start`/`end` — логические;
   при включённом `bidi` физический `left` соответствует логическому `end`.
   Политика по умолчанию консервативна: физический маппинг + предупреждение.

### 10.6. Метод T5 — Удаление/игнорирование Transitional-only элементов

Элементы/атрибуты, отсутствующие в Strict, не могут быть перенесены. Метод T5:
1. Удаляет узел из нормализованного потока.
2. Записывает `LossRecord { feature_id, reason, locations }`.
3. Если удаление влияет на отображение (например, `w:compat`), добавляет
   `Severity::Warning`.

**Приоритетные цели T5:**
- `w:compat` и его потомки (`w:compatSetting`, `w:useFELayout`, `w:doNotExpandShiftReturn` и др.);
- Transitional-only элементы настроек (`w:mailMerge`, `w:saveThroughXslt` и подобные) —
  уточняются таблицей;
- `w:legacyDrawing`, `w:legacyDrawingHF`;
- `w:object` (OLE-контейнер);
- атрибуты DrawingML, удалённые в Strict (`fromWordArt`, `compatLnSpc` и др.).

Правила T5:
1. Удаление никогда не приводит к панике и не «рвёт» DOM: на месте узла
   фиксируется `Opaque`-маркер для отчётности, но в Strict-модель он не попадает.
2. Каскадное удаление (родитель удалён → потомки не разбираются) фиксируется одним
   `LossRecord` с агрегированной локацией.

### 10.7. Метод T6 — Разрешение MCE (Markup Compatibility)

MCE остаётся валидным в Strict, но `mc:Fallback`/`mc:Choice` могут содержать
Transitional-разметку. Алгоритм:
1. Собрать активные префиксы из `mc:Ignorable`.
2. Для `mc:AlternateContent` выбрать ветвь по `McePolicy`:
   - `ProcessChoice` (по умолчанию): выбрать первый `mc:Choice` с поддерживаемым
     `Requires` и Strict-содержимым;
   - `PreferFallback`: выбрать `mc:Fallback`;
   - `Report`: не выбирать, только зафиксировать.
3. Неподдерживаемые `Requires` → пропуск ветви.
4. Выбранная ветвь рекурсивно проходит T1–T5.
5. Необработанный `AlternateContent` → `LossRecord`.

### 10.8. Метод T7 — Legacy-графика (VML, w:pict, w:object)

- VML (`v:*`, `o:*`, `w10:*`) нативно не поддерживается.
- `VmlFallback`:
  - `Report` (по умолчанию): узел не рендерится, фиксируется `UnsupportedFeature(vml)`;
  - `RasterizeIfPossible`: при наличии растрового дубликата (например, `w:binData`/
    связанное изображение) подставить его как inline-изображение DrawingML;
  - `Drop`: удалить с записью loss.
- `w:object` (OLE): не поддерживается, `Содержимое OLE` фиксируется, вложенное
  preview-изображение может быть извлечено по `RasterizeIfPossible`.

### 10.9. Метод T8 — Валидация результата и Loss Report

После T1–T7 результат проверяется на набор Strict-инвариантов:
- все namespace URI ∈ Strict-реестр (или ignorable);
- отсутствуют запрещённые элементы (нет `w:compat`, `w:pict`, `v:*`);
- нет коллизий QName/атрибутов;
- значения перечислений принадлежат Strict-домену;
- relationships разрешаются в существующие части (кроме External).

Нарушение → `NormalizationInvariantViolation` с локацией; в режиме `Lenient`
фиксируется как `Warning`, в `Strict` — как `Error`.

Loss Report формируется из накопленных `LossRecord` и прикладывается к Feature Report.

### 10.10. Инварианты нормализации
1. **Детерминированность:** одинаковый вход → одинаковый выход и одинаковый отчёт.
2. **Идемпотентность:** нормализация уже-Strict-документа является no-op
   (все счётчики T1–T7 равны нулю).
3. **Отсутствие молчаливых потерь:** каждое изменение фиксируется.
4. **Отсутствие паник:** любой сбой → `Result`.
5. **Порядок стадий фиксирован** и не зависит от порядка частей в пакете.

### 10.11. Генерация таблиц маппинга (артефакт)

На Этапе 0/6 создаётся утилита `xtool xsd-diff`, которая:
1. Загружает официальные XSD Strict (ISO/IEC 29500-1) и Transitional (Part 4).
2. Строит перечень различий: добавленные/удалённые/переименованные элементы,
   атрибуты, типы и значения перечислений.
3. Формирует `mapping/*.toml` (`namespaces.toml`, `reltypes.toml`,
   `renames.toml`, `value_maps.toml`, `removals.toml`) со статусом `Verified`.
4. Таблицы версионируются вместе с библиотекой и являются источником истины.

```toml
# renames.toml (фрагмент)
[[rename]]
domain = "wml"
from   = "{...wordprocessingml/2006/main}ind/@left"
to     = "{...wordprocessingml/main}ind/@start"
verified = false
note   = "confirm with XSD diff"
```

### 10.12. Отчёт о потерях (структура)

```rust
pub struct LossRecord {
    pub transform_id: String,      // "T5.removal"
    pub feature_id: String,        // "w:compat"
    pub reason: String,
    pub severity: Severity,
    pub locations: Vec<SourceLocation>,
}

pub struct NormalizationReport {
    pub conformance_detected: Conformance,
    pub applied: Vec<TransformRecord>,
    pub losses: Vec<LossRecord>,
    pub invariants_ok: bool,
}
```

---

## 11. Feature Report (схема v2)

### 11.1. Общий вид

```json
{
  "schema_version": "2.0",
  "file": "document.docx",
  "standard": "ISO/IEC 29500-1:2008 Strict",
  "tool": { "name": "strict-ooxml", "version": "0.1.0" },
  "conformance": {
    "declared": "strict",
    "detected": "transitional",
    "normalized": true
  },
  "overall_status": "partial",
  "summary": { "supported": 12, "partial": 3, "unsupported": 1, "ignored": 0, "error": 1 },
  "features": [
    { "feature_id": "w:tbl", "status": "supported", "severity": "info", "locations": [] },
    { "feature_id": "w:drawing", "status": "partial", "severity": "warning",
      "message": "Поддерживаются только inline-изображения",
      "locations": ["/word/document.xml:42"] }
  ],
  "normalization": {
    "applied": [
      { "transform_id": "T1.namespace", "count": 128, "locations": ["/word/document.xml:1"] },
      { "transform_id": "T3.rename", "count": 4, "locations": ["/word/document.xml:77"] }
    ],
    "losses": [
      { "transform_id": "T5.removal", "feature_id": "w:compat",
        "reason": "Элемент отсутствует в Strict", "severity": "warning",
        "locations": ["/word/settings.xml:12"] }
    ],
    "invariants_ok": true
  }
}
```

### 11.2. Статусы и severity
- Статусы: `supported`, `partial`, `unsupported`, `ignored`, `error`.
- Severity: `info`, `warning`, `error`.
- `overall_status`: `supported` | `partial` | `unsupported` (агрегация по `error > partial > supported`).

### 11.3. Требования к отчёту
- Валидность по JSON-схеме (схема версионируется, хранится в репозитории).
- Детерминированный порядок элементов (сортировка по `feature_id`, затем по локации).
- Все `unsupported`/`error` имеют хотя бы одну локацию.

---

## 12. Безопасность и ресурсные лимиты

### 12.1. Лимиты (значения по умолчанию)

```rust
pub struct ResourceLimits {
    pub max_zip_entries: usize,          // 4096
    pub max_total_uncompressed: u64,     // 512 MiB
    pub max_single_uncompressed: u64,    // 128 MiB
    pub max_compression_ratio: u32,      // 200
    pub max_xml_depth: u32,              // 256
    pub max_xml_attributes_per_elem: u32,// 1024
    pub max_text_len: usize,             // 64 MiB на часть
    pub max_rel_depth: u32,              // 32
    pub max_parts: usize,                // 4096
}
```

Все лимиты переопределяемы через `OpenOptions`. При превышении — соответствующая
ошибка (`LimitExceeded { kind, limit, actual }`), без паники.

### 12.2. Угрозы и контрмеры

| Угроза | Контрмера |
|---|---|
| ZIP-bomb (deflate) | лимиты `max_total_uncompressed`, `max_compression_ratio` |
| ZIP-bomb (много записей) | `max_zip_entries`, `max_parts` |
| Path traversal (`../`) | канонизация и отказ от небезопасных путей |
| XXE / SSRF | внешние сущности и DTD не обрабатываются |
| Billion laughs | лимит на подстановку сущностей (полный запрет) |
| Глубокая вложенность XML | `max_xml_depth` |
| Огромный текст/атрибуты | `max_text_len`, `max_xml_attributes_per_elem` |
| Рекурсивные relationships | `max_rel_depth` |
| Целочисленные переполнения | checked arithmetic в парсерах размеров |
| Дубликаты частей | обнаружение и отказ (`DuplicatePart`) |
| «Мёртвые» ссылки rel | отчёт + (для External) без загрузки |

### 12.3. Fuzzing
- Таргеты: ZIP/OPC, XML-парсер, namespaces, WML-парсер, нормализация.
- Условие приёмки: 24 часа непрерывного фаззинга без паник/UB/утечек.

---

## 13. Модель ошибок

```rust
#[non_exhaustive]
pub enum StrictError {
    Io(std::io::Error),
    InvalidZip(String),
    UnsupportedCompression(u16),
    LimitExceeded { kind: LimitKind, limit: u64, actual: u64 },
    InvalidXml { location: SourceLocation, detail: String },
    UnboundPrefix { location: SourceLocation, prefix: String },
    MixedConformance { detail: String },
    TransitionalNotSupported { location: SourceLocation },
    NormalizationInvariantViolation { location: SourceLocation, detail: String },
    AttributeCollision { qname: String, location: SourceLocation },
    InvalidEnumValue { qname: String, value: String, location: SourceLocation },
    MissingPart(PartId),
    UnresolvedRelationship(RelId),
    Render(String),
}
```

Требования:
- Реализация `std::error::Error` + `thiserror` (или ручная, без зависимостей).
- Никаких `unwrap`/`expect`/`panic!` в библиотечных путях обработки входных данных.
- Индексация — только с проверкой (`get`), арифметика размеров — `checked_*`.

---

## 14. Нефункциональные требования

| Категория | Требование |
|---|---|
| Производительность (парсинг) | 100-страничный документ ≤ 2 с на референсном оборудовании |
| Производительность (рендеринг) | 100-страничный документ ≤ 5 с |
| Память | Пиковое потребление ≤ 10× размера распакованного документа (цель) |
| Безопасность | 0 паник/UB на fuzz-корпусе; лимиты по умолчанию включены |
| Тестируемость | покрытие ≥ 80% для `core` и `wml`, ≥ 70% ветвлений |
| Потокобезопасность | публичные типы `Send + Sync` |
| Совместимость | Rust stable, MSRV фиксируется (например, 1.75+) |
| Кроссплатформенность | Linux, macOS, Windows (x86_64, arm64) |
| Документация | `#![deny(missing_docs)]`, примеры для публичного API |
| Локализация | сообщения об ошибках на английском (в отчёте — опционально ru) |

---

## 15. Этапы разработки

### Этап 0. Анализ и проектирование
**Длительность:** 1–2 месяца.  
**Результаты:**
- Архитектурный документ (настоящее ТЗ в редакции 2.0).
- Спецификация публичного API.
- Схема Feature Report (JSON Schema) и Loss Report.
- Утилита и таблицы маппинга Transitional → Strict (`xtool xsd-diff`).
- Тестовый корпус Strict- и Transitional-документов.
- Настройка CI.

**Критерии приёмки:**
- Утверждена архитектура и публичный API.
- Создан репозиторий и CI (сборка, тесты, clippy, fmt, deny).
- Собрано ≥ 20 Strict-документов и ≥ 10 Transitional-документов для проверки отчёта.

---

### Этап 1. Ядро OPC/XML/Namespaces
**Длительность:** 2–3 месяца.  
**Результаты:**
- Чтение ZIP/OPC с лимитами.
- Разбор `[Content_Types].xml` и relationships.
- Потоковый namespace-aware XML-парсер.
- Реестр пространств имён Strict/Transitional.
- Определение Strict/Transitional/Mixed.
- Модель ошибок и лимиты.
- Скелет CLI (`inspect`, `check`).
- Fuzz-таргеты ZIP/XML.

**Критерии приёмки:**
- Библиотека открывает корректный `.docx` Strict.
- Transitional-файл определяется; в режиме `StrictOnly` отклоняется с понятной ошибкой.
- Нет паник на повреждённых ZIP/XML (fuzz 24 ч).
- Покрытие тестами ядра ≥ 80%.
- CLI `inspect` выводит conformance и карту частей.

**Детальная задача:** см. `STAGE-1-TASK.md`.

---

### Этап 2. Модель и парсер WordprocessingML Strict
**Длительность:** 4–6 месяцев.  
**Результаты:**
- DOM документа.
- Парсинг `document.xml`, `styles.xml`, `numbering.xml`, `settings.xml`, `sectPr`.
- Поддержка абзацев, runs, текста, базовых стилей, таблиц, списков.
- Парсинг DrawingML для inline-изображений.

**Критерии приёмки:**
- Разбираются все обязательные элементы WML Strict.
- Покрытие опциональных элементов — не менее 90%.
- На тестовом корпусе нет паник и необработанных ошибок.
- Feature Report формируется для каждого документа.

---

### Этап 3. Система отчётности
**Длительность:** 1–2 месяца.  
**Результаты:**
- Подсистема регистрации поддержки.
- JSON-схема отчёта.
- CLI-команда `check`.
- Человекочитаемый отчёт.

**Критерии приёмки:**
- Для любого файла формируется отчёт.
  *(Уточнение приёмки Этапа 3: отчёт формируется для любого успешно
  разобранного **Strict**-документа; Transitional без нормализации Этапа 6
  отчёта не даёт — осознанный отказ, см. ADR-0005 и `STAGE-3-ACCEPTANCE.md` F4.)*
- Отчёт валиден по JSON-схеме.
- Указаны все неподдержанные механизмы с локацией.
- CLI возвращает код 0, если нет ошибок, и 1 при критических проблемах.

---

### Этап 4. Базовый рендеринг в SVG
**Длительность:** 4–6 месяцев.  
**Результаты:**
- Рендеринг страниц в SVG.
- Поддержка текста, абзацев, базовых стилей, таблиц, изображений.
- Разбиение на страницы.

**Критерии приёмки:**
- 100% тестов из базового корпуса рендерятся без ошибок.
- Визуальное сравнение с эталонами: не менее 95% совпадения по SSIM для утверждённых тестов.
- Время рендеринга 100-страничного документа — не более 5 секунд на референсном оборудовании.

---

### Этап 5. Расширенная поддержка
**Длительность:** 6–12 месяцев.  
**Результаты:**
- Сноски и концевые сноски.
- Колонтитулы.
- Поля.
- Сложные таблицы.
- Стили и темы.
- Нумерация.
- DrawingML: фигуры, группы, якоря.
- Математические формулы (опционально).

**Критерии приёмки:**
- Покрытие расширенных сценариев — не менее 85%.
- Feature Report корректно указывает частичную поддержку.
- Нет регрессий по базовому корпусу.

---

### Этап 6. Нормализация Transitional → Strict
**Длительность:** 3–6 месяцев.  
**Результаты:**
- Реализация методов T1–T8 (раздел 10).
- Генератор таблиц маппинга из XSD.
- Поддержка «только чтение» Transitional-документов с нормализацией.
- Loss Report.
- CLI-команда `normalize --report`.

**Критерии приёмки:**
- Transitional-документ открывается в режиме нормализации.
- VML не поддерживается, но не вызывает паники и фиксируется в отчёте.
- Feature Report явно указывает на потери при конвертации.
- Инварианты 10.10 выполняются (детерминизм, идемпотентность, отсутствие молчаливых потерь).
- На Transitional-корпусе нет паник.

---

### Этап 7. Стабилизация и релиз
**Длительность:** 2–3 месяца.  
**Результаты:**
- Публикация на crates.io.
- Документация на docs.rs.
- Примеры.
- SemVer 1.0.
- Лицензия MIT/Apache-2.0.

**Критерии приёмки:**
- Все этапы пройдены.
- Нет открытых критических багов.
- Документация покрывает 100% публичного API.
- CI собирает Linux, macOS, Windows.

---

## 16. Общие критерии приёмки проекта

1. **Корректность:** библиотека разбирает и отображает Strict-документы в соответствии с заявленным покрытием.
2. **Безопасность:** отсутствие паник, UB, утечек памяти на тестовом корпусе и fuzz-тестах.
3. **Отчётность:** для каждого файла формируется Feature Report; при нормализации — Loss Report.
4. **Производительность:** разбор 100-страничного документа ≤ 2 с, рендеринг ≤ 5 с.
5. **Тестируемость:** покрытие тестами ≥ 80% для ядра и парсера.
6. **Документация:** публичный API документирован, есть примеры.
7. **Кроссплатформенность:** Linux, macOS, Windows.
8. **Лицензирование:** совместимое с open source (MIT/Apache-2.0).

---

## 17. Тестирование

### 17.1. Виды тестирования
- Модульные тесты.
- Интеграционные тесты.
- Property-based тесты (пространства имён, канонизация путей, relationships).
- Fuzz-тестирование (ZIP, XML, namespaces, WML, нормализация).
- Регрессионное тестирование (каждый исправленный баг → тест).
- Визуальное сравнение SVG (SSIM).
- Тестирование на повреждённых файлах.
- Тестирование на Transitional-файлах (отчёт и нормализация).

### 17.2. Тестовый корпус

| Категория | Кол-во (мин.) | Назначение |
|---|---|---|
| Strict базовые | 20 | Покрытие основных сценариев |
| Strict расширенные | 15 | Таблицы, стили, списки, графика |
| Transitional | 10 | Проверка детекции и Loss Report |
| Повреждённые | 15 | Отсутствие паник |
| Граничные (лимиты) | 10 | ZIP-bomb, глубина XML, ratio |
| Метаданные/OPC | 10 | Content types, rels, external |

### 17.3. Инструменты
- `cargo test`, `cargo nextest` (опц.).
- `proptest` — property-based.
- `cargo-fuzz` (libFuzzer) — fuzz.
- `cargo-llvm-cov` / `tarpaulin` — покрытие.
- `cargo-deny`, `cargo-audit` — безопасность зависимостей.
- Собственный `xtool` — XSD-дифф, валидация JSON-схемы, SSIM-сравнение.

---

## 18. CI/CD и релизный процесс

### 18.1. CI (каждый PR)
1. `cargo fmt --check`.
2. `cargo clippy --all-targets -- -D warnings`.
3. `cargo test --workspace`.
4. `cargo test --features round-trip-normalize`.
5. Проверка покрытия (порог).
6. `cargo deny check`.
7. Матрица ОС: Linux, macOS, Windows.

### 18.2. Ночные сборки
- Fuzz-сессии (тайм-бокс) на таргетах.
- Бенчмарки (`criterion`), сравнение с baseline.
- Регрессия SSIM на эталонном корпусе.

### 18.3. Релиз
- Теги `vX.Y.Z`, `CHANGELOG.md`.
- Публикация крейтов в порядке зависимостей.
- Артефакты: CLI-бинарники для 3 ОС.

---

## 19. Риски

| Риск | Вероятность | Влияние | Митигация |
|---|---|---|---|
| Сложность стандарта | Высокая | Высокое | Чёткий scope, table-driven, поэтапность |
| Мало Strict-файлов | Высокая | Среднее | Генерация тестовых файлов + корпус Transitional |
| Неполнота таблиц маппинга | Высокая | Высокое | Автогенерация из XSD, флаг `Verified`, тесты на корпусе |
| VML в Transitional | Высокая | Среднее | Не поддерживать нативно, только отчёт/растр |
| Точность рендеринга | Средняя | Высокое | Эталонные SVG, SSIM, допуски |
| Шрифты и метрики | Высокая | Среднее | Абстракция шрифтов, fallback |
| ZIP/XML-уязвимости | Средняя | Высокое | Лимиты, запрет DTD/XXE, fuzz |
| Ресурсы команды | Средняя | Высокое | MVP, опциональные этапы |
| Дрейф схем ISO | Низкая | Среднее | Версионирование таблиц и реестра |

---

## 20. Поставка

- Исходный код в Git-репозитории.
- Крейты на crates.io.
- Документация на docs.rs.
- CLI-утилита (`inspect`, `check`, `render`, `normalize`).
- Тестовый корпус (Strict + Transitional + повреждённые).
- Таблицы маппинга Transitional → Strict.
- Отчёты о тестировании и fuzz.
- Лицензия MIT/Apache-2.0.

---

## 21. Приложения

### Приложение А. Минимальный состав команды
- 2–4 Rust-разработчика.
- 1 QA/тестировщик.
- 1 эксперт по OOXML (частичная занятость).
- 1 технический писатель (частичная занятость).

### Приложение Б. Референсное оборудование
- CPU: 8 ядер, 3.0 GHz.
- RAM: 16 GB.
- SSD.
- Linux x86_64.

### Приложение В. Источники
- ISO/IEC 29500-1:2008 (Strict, Part 1).
- ISO/IEC 29500-2:2008 (OPC).
- ISO/IEC 29500-4:2008 (Transitional, Part 4).
- ECMA-376 Part 1 / Part 4.
- Документация Microsoft Open XML SDK.
- Схемы Strict: `purl.oclc.org/ooxml/...`.

### Приложение Г. Принятые решения (ранее открытые вопросы)

| № | Вопрос | Решение | Где отражено |
|---|---|---|---|
| Г.1 | Direction-neutral переименования (`w:ind/@left`→`start` и пр.) | **Отложены до XSD-диффа на Этапе 0**; хранятся с `Verified=false` и не применяются к Strict-корпусу до подтверждения официальными XSD | 10.4, 10.11 |
| Г.2 | ZIP-бэкенд | **Собственный ридер поверх `miniz_oxide`** (ручной разбор EOCD/центральной директории), изоляция в `opc::zip` + fuzz. ADR-0001 | 8, STAGE-1 §4.1 |
| Г.3 | XML-бэкенд | **`quick-xml` + собственная обёртка-лимитер** (namespace-резолвинг, запрет DTD/сущностей, depth/attr limits). ADR-0002 | 9, STAGE-1 §4.2 |
| Г.4 | Политика `w:altChunk` | **Неподдерживаемый блок + запись в Feature Report**; внешнее содержимое (HTML/RTF/MHT) не встраивается | 7.2, 7.4 |
| Г.5 | `TargetMode="External"` | **Не загружать** (без сети/SSRF); фиксировать как External в relationships и отчёте | 8.1, 12.2 |
| Г.6 | MSRV | **Rust 1.75+** | 14 |
| Г.7 | Набор features по умолчанию (мета-крейт) | **`default = svg + report`**; `round-trip-normalize` — отдельная feature | 5.4 |
| Г.8 | Коды возврата CLI `check` | **0 = Strict OK, 1 = обнаружен Transitional (в StrictOnly), 2 = ошибка/повреждение** | STAGE-1 §10.9 |
| Г.9 | ZIP64 на Этапе 1 | **Желательно, не блокер:** реализуется при наличии времени; отсутствие не блокирует приёмку | STAGE-1 §2.2, §10 |

---

**Конец документа.**
