# Задача на реализацию. Этап 2 — Модель и парсер WordprocessingML Strict

**Шифр:** TASK-STAGE-2  
**Связь с ТЗ:** `TZ-STRICT-OOXML-RUST.md`, §3.3 (матрица), §7 (DOM), §15 «Этап 2», §6 (API), §17 (тестирование)  
**Основание:** Этап 1 принят (`STAGE-1-TASK.md`, `STAGE-1-REWORK.md`). Результаты Этапа 1: `strict-ooxml-core` v0.1.0, покрытие 88% строк.  
**Крейт:** `strict-ooxml-wml` (новый), точка входа в мета-крейте `strict-ooxml`  
**Оценка:** 4–6 месяцев (≈ 640–960 человеко-часов)  
**Приоритет:** Критический (блокирует Этапы 3–7)  
**Статус:** К реализации  

---

## 1. Цель этапа

Построить **внутреннюю объектную модель (DOM)** WordprocessingML Strict и
**событийный парсер**, превращающий части Strict-пакета в типизированное дерево
документа. На выходе этапа библиотека умеет: открыть Strict-документ, разобрать
`document.xml`, `styles.xml`, `numbering.xml`, `settings.xml`, секции (`sectPr`),
абзацы, runs, текст, таблицы, списки и inline-графику DrawingML, собрать
разрешённую (style-resolved) модель и зафиксировать использование механизмов
для будущего Feature Report.

Этап **не** рендерит (Этап 4) и **не** нормализует Transitional (Этап 6).

---

## 2. Scope этапа

### 2.1. Входит
- Крейт `strict-ooxml-wml`: доменная модель + парсер.
- Парсинг главной части `document.xml` (body, блоки, inline, секции).
- Парсинг `styles.xml`, `numbering.xml`, `settings.xml`.
- Абзацы, runs, текст (с `xml:space`), базовые свойства (`pPr`/`rPr`).
- Таблицы (сетка, строки, ячейки, объединения).
- Списки/нумерация (привязка абзацев к `numPr`, разбор `numbering.xml`).
- Секции (`sectPr`, колонки, размеры/поля страницы, ссылки на колонтитулы — без их разбора).
- Inline-графика DrawingML: `w:drawing` → `wp:inline` → `a:graphic`/`pic:pic` →
  `a:blip` → разрешение relationship на медиа-часть (без декодирования пикселей).
- Разрешение ссылок: стили (`pStyle`/`rStyle`/`basedOn`), нумерация, relationships.
- Модель поддержки (`SupportModel`) с локациями — вход для Этапа 3.
- Публичное API «прочитать документ», интеграция в мета-крейт.
- Тесты, корпус, fuzz-таргет WML.

### 2.2. Не входит
- Нормализация Transitional → Strict (Этап 6). На входе — только Strict (через `ConformancePolicy`).
- Рендеринг (SVG/PNG) — Этап 4.
- Сноски/концевые сноски, колонтитулы, поля, сложные таблицы, темы, фигуры/группы/якоря — Этап 5.
- Полный JSON Feature Report и CLI `check` — Этап 3 (Этап 2 даёт данные).
- MathML, VML, OLE.
- Редактирование/сериализация обратно в `.docx`.

---

## 3. Зависимости от Этапа 1 (используемое API)

| Источник | Использование |
|---|---|
| `opc::Package`, `PartSource`, `PartId` | открытие пакета, доступ к частям |
| `opc::rels::{RelType, Relationship, TargetMode}` | разрешение `r:embed`, `r:id`, стилей/нумерации |
| `xml::{XmlReader, XmlEvent, QName, Attr}` + `XmlReader::from_vec` | потоковый разбор |
| `limits::ResourceLimits` | лимиты (в т.ч. новый `max_text_len`, `max_xml_depth`) |
| `error::{StrictError, Result, SourceLocation}` | единая модель ошибок + локации |
| `ns::{Conformance, NamespaceRegistry}` | проверка Strict-пространств имён |

**Требования-наследие Этапа 1:** не паниковать (`#![deny(unsafe_code)]`,
`missing_docs`); локации O(1) на событие (кэш из `XmlReader`); учитывать
`ResourceLimits`; в публичных путях — только `Result`.

---

## 4. Технические решения

### 4.1. Событийный парсер без промежуточного generic-DOM
Из `XmlReader` строится типизированная модель напрямую (state machine + dispatch),
без промежуточного «сырого» дерева. Причины: память/скорость, точные типы,
контроль неизвестных элементов.

### 4.2. Табличное диспетчеризование
Соответствие «(namespace, local) → handler» задаётся данными
(`ElementSpec`), а не цепочками `if`. Неизвестный элемент → узел `Opaque`
(сохраняется для отчётности), не ошибка.

### 4.3. Интернирование имён (P9 из `docs/perf-baseline.md`)
Внутри парсера — `Interner` для `(ns, local)`-символов и строк-значений
(стилевые id, значения enum). Узлы DOM хранят `Sym`/`Arc<str>`, а не `String`
на каждое имя. Снижает аллокации и память DOM.

### 4.4. Двухфазная сборка
1. **Parse phase** — части разбираются в «сырые» typed-структуры (без разрешения ссылок).
2. **Resolve phase** — разрешение наследования стилей, нумерации, relationship-ссылок;
   проверка целостности; построение `Document` (immutable).

Части `styles`/`numbering`/`settings` независимы: при feature `parallel` (rayon)
разбираются параллельно.

### 4.5. Неизменяемая модель
`Document` после `resolve` иммутабелен → `Send + Sync`, безопасен для Этапа 4.
Строки — `Arc<str>`/интернированные; пулы на документ.

### 4.6. Модель ошибок
Все ошибки — `StrictError` (расширяется вариантом `Model(String)` или
переиспользует существующие). Некорректный элемент → понятная ошибка с
`SourceLocation`, но частично-валидные документы (`Permissive`) дают модель с
записями в `SupportModel`, а не отказ.

---

## 5. Архитектура крейта `strict-ooxml-wml`

```
strict-ooxml-wml/
├── src/
│   ├── lib.rs                 # публичный API, #![deny(missing_docs, unsafe_code)]
│   ├── model/                 # доменная модель (только данные)
│   │   ├── mod.rs
│   │   ├── document.rs        # Document, Body, Section
│   │   ├── block.rs           # Block, Paragraph, Table, Sdt
│   │   ├── inline.rs          # Inline, Run, RunContent, Field, Break, Text
│   │   ├── props.rs           # PPr, RPr, TblPr, TcPr, PgMar, ...
│   │   ├── styles.rs          # StyleTable, Style, StyleType
│   │   ├── numbering.rs       # NumberingTable, AbstractNum, Num, Level
│   │   ├── settings.rs        # Settings
│   │   ├── drawing.rs         # Drawing, Inline, Anchor(stub), Pic, BlipRef
│   │   └── support.rs         # SupportModel, FeatureUse, SupportStatus
│   ├── parse/
│   │   ├── mod.rs             # Parser (state machine), ParseContext
│   │   ├── dispatch.rs        # ElementSpec-таблицы
│   │   ├── document.rs        # body/blocks/paragraphs/runs
│   │   ├── props.rs           # pPr/rPr/tblPr/... разбор
│   │   ├── table.rs           # tbl/tr/tc
│   │   ├── styles.rs          # styles.xml
│   │   ├── numbering.rs       # numbering.xml
│   │   ├── settings.rs        # settings.xml
│   │   ├── drawing.rs         # w:drawing/wp:inline/pic
│   │   └── interner.rs        # Sym/Interner
│   ├── resolve/
│   │   ├── mod.rs             # resolve phase
│   │   ├── styles.rs          # basedOn/next/link, каскад
│   │   ├── numbering.rs       # numId→abstractNumId, level overrides
│   │   └── rels.rs            # r:embed/r:id → PartId
│   └── error.rs               # (при необходимости) маппинг в StrictError
└── tests/ + benches/ + fuzz
```

Зависимости: `strict-ooxml-core` (обязательно), `rayon` (optional, feature
`parallel`). Никаких новых тяжёлых зависимостей без ADR.

---

## 6. Доменная модель (черновик типов)

```rust
pub struct Document {
    pub body: Body,
    pub styles: StyleTable,
    pub numbering: NumberingTable,
    pub settings: Settings,
    pub sections: Vec<Section>,      // из sectPr (в т.ч. финальный в body)
    pub media: MediaIndex,           // PartId -> MediaItem (метаданные)
    pub support: SupportModel,
    // источник (PartId'ы) для отладки/локаций
}

pub struct Body { pub blocks: Vec<Block> }

pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    SdtBlock(SdtContainer),
    AltChunk(AltChunkInfo),          // не рендерится; фиксируется
}

pub struct Paragraph {
    pub props: ParagraphProperties,
    pub inlines: Vec<Inline>,
    pub rsids: Rsids,                // w:rsid*
    pub para_id: Option<ParaId>,
    pub text_id: Option<TextId>,
    pub location: SourceLocation,
}

pub struct ParagraphProperties {
    pub style: Option<StyleId>,
    pub alignment: Option<Justification>,     // ST_Jc (Strict: start/end/…)
    pub numbering: Option<NumPr>,             // numId + ilvl
    pub spacing: Option<Spacing>,
    pub indentation: Option<Indentation>,     // start/end (direction-neutral)
    pub borders: Borders,
    pub shading: Option<Shading>,
    pub tabs: Vec<TabStop>,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    pub outline_level: Option<u8>,
    pub bidi: bool,
    pub run_props: Option<RunProperties>,     // свойства маркера абзаца (rPr)
    pub section: Option<SectionProperties>,   // sectPr внутри pPr
    // ...
}

pub enum Inline {
    Run(Run),
    Hyperlink(Hyperlink),
    Field(Field),
    Drawing(Drawing),
    Break(BreakKind),
    Tab,
    SdtInline(SdtContainer),
    BookmarkStart(BookmarkId),
    BookmarkEnd(BookmarkId),
    CommentRef(CommentId),           // фиксируется; комментарии — Этап 5
    FootnoteRef(u32),                // фиксируется; рендер — Этап 5
    EndnoteRef(u32),
    Opaque(OpaqueInline),            // неизвестное, сохраняется для отчёта
}

pub struct Run {
    pub props: RunProperties,
    pub content: Vec<RunContent>,
    pub location: SourceLocation,
}

pub enum RunContent {
    Text(TextNode),
    Tab,
    Break(BreakKind),
    Drawing(Drawing),
    InstrText(String),               // поле: инструкция
    FieldChar(FieldCharType),
    Symbol { font: String, char: char },
    LastRenderedPageBreak,
    NoBreakHyphen,
    SoftHyphen,
    Opaque(OpaqueInline),
}

pub struct TextNode { pub text: String, pub space: Space } // xml:space

pub struct RunProperties {
    pub style: Option<StyleId>,
    pub fonts: Option<Fonts>,        // ascii/hAnsi/eastAsia/cs
    pub bold: TriState, pub italic: TriState,
    pub underline: Option<Underline>, pub strike: TriState, pub dstrike: TriState,
    pub color: Option<Color>, pub highlight: Option<Highlight>,
    pub size: Option<HalfPoints>, pub size_cs: Option<HalfPoints>,
    pub vert_align: Option<VertAlign>,
    pub spacing: Option<Twips>, pub position: Option<HalfPoints>,
    pub caps: bool, pub small_caps: bool, pub rtl: bool,
    // ...
}

pub struct Table {
    pub props: TableProperties,
    pub grid: Vec<GridCol>,
    pub rows: Vec<TableRow>,
    pub location: SourceLocation,
}
pub struct TableRow { pub props: RowProperties, pub cells: Vec<TableCell> }
pub struct TableCell { pub props: CellProperties, pub blocks: Vec<Block> }

pub struct SectionProperties {
    pub page_size: Option<PageSize>,
    pub page_margins: Option<PageMargins>,
    pub columns: Option<Columns>,
    pub sect_type: SectionType,
    pub title_page: bool,
    pub headers: Vec<HeaderFooterRef>,   // по типу (default/first/even)
    pub footers: Vec<HeaderFooterRef>,
    pub doc_grid: Option<DocGrid>,
    // ...
}
```

**Инварианты модели**
- Каждый узел несёт `SourceLocation` (part + line/column/byte offset).
- Неизвестные элементы/атрибуты не теряются молча: узел `Opaque` + запись
  `FeatureUse` в `SupportModel`.
- Никаких `unwrap`/`expect`/`panic!` на путях разбора входных данных.

---

## 7. Правила парсинга и покрытие элементов

### 7.1. Обязательные элементы (must-have)
Корень `w:document`/`w:body`, `w:p`, `w:r`, `w:t`, `w:br`, `w:tab`, `w:tbl`,
`w:tr`, `w:tc`, `w:tblGrid`, `w:sectPr`, `w:pPr`, `w:rPr`, `w:pStyle`, `w:rStyle`,
`w:numPr`/`w:numId`/`w:ilvl`, `w:drawing`, `w:hyperlink`, `w:bookmarkStart/End`,
`w:fldSimple`, `w:instrText`, `w:fldChar`. Разбор всех обязательных —
**критерий приёмки**.

### 7.2. Опциональные элементы (target ≥ 90%)
Полный инвентарь WML Strict генерируется утилитой `xtool xsd-inventory` из
официальных XSD (надстройка над `xtool` из Этапа 0) в
`coverage/wml-elements.toml` со статусами: `supported | partial | unsupported |
ignored`. CI считает процент покрытия опциональных элементов и валит сборку при
< 90%. Каждый парсер-модуль ссылается на список элементов, которые он покрывает.

### 7.3. Разбор свойств (`pPr`/`rPr`/`tblPr`/`tcPr`/`trPr`)
- Единый стиль: `PropParser` с таблицей «элемент → поле».
- Порядок дочерних элементов по схеме; внеочередные — фиксируются в отчёте, но
  применяются (устойчивость к «нестрогому» порядку в реальных файлах).
- Tri-state для наследуемых boolean (`on/off/absent`).

### 7.4. Значения и единицы
- `ST_Jc`, `ST_Underline`, `ST_VerticalAlign` и др. — строгие enums с
  `from_strict(value) -> Option<Self>`; неизвестное значение → `InvalidEnumValue`
  - fallback (значение схемы по умолчанию) + запись в отчёт.
- Единицы: twips (1/20 pt), half-points, eighths-of-a-point для границ —
  отдельные newtypes, чтобы исключить смешение.

### 7.5. Локации
Использовать `XmlReader::last_event_location()` (O(1) после Этапа 1) для каждого
значимого узла.

---

## 8. DrawingML inline-изображения (в объёме Этапа 2)

```
w:drawing
 └─ wp:inline (или wp:anchor → Этап 5; на Этапе 2 anchor фиксируется)
     ├─ wp:extent/@cx,@cy
     ├─ wp:docPr/@id,@name,@descr
     └─ a:graphic
         └─ a:graphicData[@uri=.../picture]
             └─ pic:pic
                 ├─ pic:nvPicPr (name/descr)
                 ├─ pic:blipFill
                 │   └─ a:blip/@r:embed  → resolve → PartId (media)
                 └─ pic:spPr (extent, prstGeom/rect)
```

- Разрешение `r:embed`/`r:link` через relationship исходной части.
- `MediaIndex`: `PartId` + content type (`.png`, `.jpeg`, `.gif`, `.emf`, `.wmf`…);
  **байты изображения не декодируются** на Этапе 2 (`load_media` опция позже).
- `wp:anchor` (плавающая графика) не поддерживается на Этапе 2 → `SupportModel`
  как `partial/unsupported` с локацией.

---

## 9. Модель поддержки (вход для Feature Report)

```rust
pub enum SupportStatus { Supported, Partial, Unsupported, Ignored }

pub struct FeatureUse {
    pub feature_id: String,        // "w:tbl", "w:drawing", "wp:anchor"
    pub status: SupportStatus,
    pub message: Option<String>,
    pub location: Option<SourceLocation>,
}

pub struct SupportModel { /* агрегировано по feature_id */ }
```

- Парсер регистрирует `FeatureUse` при встрече механизма (в т.ч. неизвестного).
- Etap 3 превратит `SupportModel` в JSON Feature Report и CLI `check`.
- Для критерия Этапа 2 «Feature Report формируется для каждого документа»
  достаточно: `Document::support()` доступен, а CLI/метод `support_debug()`
  выводит человекочитаемую сводку (полный JSON — Этап 3).

---

## 10. Производительность и ресурсы

- Цель Этапа 2: разбор `document.xml` 100-страничного документа — доминирующая
  часть бюджета ТЗ §14 (≤ 2 с на весь разбор). Замеряется `criterion`-бенчем
  `wml_parse` (документы 10/100/500 страниц).
- Переиспользовать `XmlReader::from_vec` (без копии), интернирование (4.3),
  ленивое разрешение (без построения лишних структур).
- Память: DOM ≤ ~10× распакованного `document.xml` (цель); строки интернированы.
- Лимиты из `ResourceLimits` соблюдаются; при превышении — `LimitExceeded`.

---

## 11. Декомпозиция работ (WBS)

| ID | Работа | Артефакт | Оценка, ч |
|---|---|---|---|
| S2.1 | ADR: структура модели/парсера, интернирование, dispatch | `docs/adr/0004-wml-model.md` | 16 |
| S2.2 | Каркас крейта, линты, интеграция в workspace/мета | `Cargo.toml`, `lib.rs` | 12 |
| S2.3 | `model/*`: типы, инварианты, `SourceLocation` | `model/` | 56 |
| S2.4 | `parse/interner`, `ParseContext`, dispatch-таблицы | `parse/` | 40 |
| S2.5 | Разбор тела: `document/body/blocks/paragraphs/runs/text` | `parse/document.rs` | 72 |
| S2.6 | Разбор свойств `pPr/rPr` и базовых enums | `parse/props.rs`, `model/props.rs` | 72 |
| S2.7 | Таблицы `tbl/tr/tc/tblGrid` | `parse/table.rs`, `model/block.rs` | 56 |
| S2.8 | `styles.xml` | `parse/styles.rs`, `model/styles.rs` | 40 |
| S2.9 | `numbering.xml` | `parse/numbering.rs`, `model/numbering.rs` | 40 |
| S2.10 | `settings.xml` | `parse/settings.rs` | 24 |
| S2.11 | `sectPr`/секции/колонки/размеры | `parse/document.rs`, `model/document.rs` | 40 |
| S2.12 | DrawingML inline + `MediaIndex` +rels | `parse/drawing.rs`, `resolve/rels.rs` | 56 |
| S2.13 | Resolve phase (стили/нумерация/ссылки) | `resolve/` | 48 |
| S2.14 | `SupportModel` + регистрация `FeatureUse` | `model/support.rs` | 24 |
| S2.15 | Публичное API + интеграция в `strict-ooxml` meta | `lib.rs`, meta | 24 |
| S2.16 | `xtool xsd-inventory` + `coverage/wml-elements.toml` | `xtool/`, CI | 32 |
| S2.17 | Тесты: unit, golden DOM, property, corpus, fuzz WML | `tests/`, `fuzz/` | 96 |
| S2.18 | Бенчмарки `wml_parse` + базовые локации | `benches/` | 24 |
| S2.19 | Документация API + примеры | rustdoc, `examples/` | 24 |
| S2.20 | CI-гейт покрытия элементов ≥ 90% + ревью | CI | 16 |
| **Итого** | | | **≈ 812** |

> P0-путь: S2.3 → S2.4 → S2.5 → S2.6 → (S2.7/S2.8/S2.9) → S2.13 → S2.15.
> Оценка — с буфером; при 2 разработчиках календарно ~4–5 месяцев.

---

## 12. Тест-план и корпус

### 12.1. Виды тестов
- **Unit** — по семействам элементов (абзац, run, таблица, стиль, нумерация, drawing).
- **Golden DOM** — разобранный DOM сериализуется в каноничный текстовый вид
  (`Debug`/dump) и сравнивается со снапшотом `tests/golden/*.txt`.
- **Property-based** (`proptest`) — «любой набор корректных событий → нет паник,
  DOM детерминирован»; round-trip `dump(parse(x))` стабилен.
- **Интеграционные** — сквозной разбор из `Package` (Strict).
- **Корпус** — см. 12.2.
- **Fuzz** — `fuzz_wml`: `Package` → `strict_ooxml_wml::parse` без паник.

### 12.2. Корпус

| Категория | Источник | Назначение |
|---|---|---|
| Публичный синтетический | `tests/samples/` (versioned) | прогон парсинга (сейчас Transitional — для Этапа 2 допускается после нормализации; до Этапа 6 — только проверка отсутствия паник) |
| Strict-фикстуры | `tests/fixtures/` (добавляются) | сквозной разбор Strict, golden |
| Локальный | `tests/docx/` (gitignored) | проверка отсутствия паник, skip при отсутствии |
| Генератор | `xtool gen-docx` | синтетические Strict-документы по параметрам |

> Требование: тесты не зависят от gitignored-корпуса; `tests/samples/`
> прогоняется всегда (как в `tests/samples_corpus.rs`).

### 12.3. Матрица покрытия (по группам)

| Группа | Обязательные | Цель опц. |
|---|---|---|
| Body/секции | 100% | 90% |
| Абзацы/свойства | 100% | 90% |
| Runs/текст | 100% | 90% |
| Таблицы | базовая 100% | 90% |
| Стили/наследование | базовая 100% | 90% |
| Нумерация/уровни | базовая 100% | 85% |
| DrawingML inline | базовая 100% | — |
| Settings | ключевые 100% | 80% |

---

## 13. Критерии приёмки и DoD

### 13.1. Критерии приёмки (из ТЗ §15 Этап 2 + уточнения)
1. Разбираются **все обязательные** элементы WML Strict (7.1).
2. Покрытие **опциональных** элементов — **не менее 90%** (CI-гейт по `coverage/wml-elements.toml`).
3. На тестовом корпусе (Strict + `tests/samples/`) **нет паник** и
   необработанных ошибок; повреждённые/неполные — `Result`.
4. `SupportModel` формируется для каждого документа; `Document::support()` доступен.
5. Таблицы, списки и inline-изображения представлены в DOM корректно (golden-тесты).
6. Наследование стилей и нумерация разрешаются (resolve-тесты).
7. Локации узлов корректны (тест на позиции, как в Этапе 1).
8. Покрытие тестами `strict-ooxml-wml` ≥ 80% строк.
9. `cargo fmt/clippy -D warnings/test/doc/deny` — зелёные; CI на 3 ОС.
10. `cargo doc` без ошибок; публичный API документирован; примеры компилируются.
11. Fuzz `fuzz_wml` подключён; smoke в CI зелёный (24 ч — по протоколу Этапа 1).

### 13.2. Definition of Done
- [ ] Крейт `strict-ooxml-wml` реализован по структуре §5.
- [ ] Публичное API (§6 + интеграция в meta) зафиксировано ADR-0004.
- [ ] `coverage/wml-elements.toml` сгенерирован; гейт ≥ 90% опциональных.
- [ ] Golden/property/corpus/fuzz тесты добавлены и зелёные.
- [ ] Бенчмарк `wml_parse` показывает приемлемый бюджет для 100 стр.
- [ ] Покрытие ≥ 80%; CI зелёный на Linux/macOS/Windows.
- [ ] Отклонения от задачи задокументированы; ревью проведено.

---

## 14. Риски этапа

| Риск | Вероятность | Влияние | Митигация |
|---|---|---|---|
| Объём WML Strict недооценён | Высокая | Высокое | Инвентарь XSD + гейт покрытия, приоритет обязательных |
| «Грязная» реальная разметка (порядок элементов) | Высокая | Среднее | Устойчивый разбор + записи в отчёт |
| Производительность DOM | Средняя | Высокое | Интернирование, ленивое разрешение, бенчмарки |
| DrawingML-сложность | Средняя | Среднее | Ограничить inline; anchor — Этап 5 |
| Паники на экзотике | Средняя | Высокое | `Result`, fuzz, property-тесты |
| Смешение Strict/Transitional примеров | Средняя | Низкое | Парсер строго по namespace; нормализация — Этап 6 |
| Дрейф от ТЗ §7 | Низкая | Среднее | ADR-0004, ревью модели |

---

## 15. Открытые вопросы к заказчику

1. Утвердить вариант API `Document::open` в мета-крейте (сигнатуры в §6) — ADR-0004.
2. Подтвердить, что Stage 2 парсер принимает **только Strict**, а `tests/samples/`
   (Transitional) до Этапа 6 используется лишь на «no-panic»-проверках.
3. Подтвердить объём `SupportModel` на Этапе 2 (данные) против полного JSON (Этап 3).
4. Утвердить цель по памяти (≤ 10× `document.xml`) и необходимость feature `parallel`.
5. Решить судьбу `w:altChunk`: узел `AltChunk` + запись (как в ТЗ §7.2) без встраивания.

---

**Конец задачи.**
