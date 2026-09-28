# Задача на реализацию. Этап 1 — Ядро OPC / XML / Namespaces

**Шифр:** TASK-STAGE-1  
**Связь с ТЗ:** `TZ-STRICT-OOXML-RUST.md`, раздел 15 «Этап 1», разделы 8, 9, 12, 13  
**Крейт:** `strict-ooxml-core` (+ каркас `strict-ooxml-cli`)  
**Оценка:** 2–3 месяца (≈ 320–480 человеко-часов)  
**Приоритет:** Критический (блокирует Этапы 2–7)  
**Статус:** К реализации  

---

## 1. Цель этапа

Реализовать надёжный, ресурсно-безопасный «сырой» слой библиотеки: чтение OPC-пакета
(`.docx`), разбор XML с разрешением пространств имён, реестр Strict/Transitional
пространств имён, определение conformance и единую модель ошибок. На выходе этапа
библиотека умеет открыть Strict-документ, перечислить его части и relationships,
определить тип документа и не падать на повреждённых/враждебных файлах.

Этап **не** строит DOM WordprocessingML и **не** рендерит — это задача Этапов 2–4.

---

## 2. Scope этапа

### 2.1. Входит
- ZIP-ридер OPC с полным контролем ресурсов (без сторонней «всё-в-память» логики).
- `[Content_Types].xml`: `Default`/`Override`, индекс content-type по части.
- `_rels/*.rels`: relationships (package + part-level), рекурсивное разрешение.
- Разрешение/канонизация target-путей (относительные, `..`, ведущий `/`, external).
- Потоковый namespace-aware XML-парсер (pull/SAX) с лимитами.
- Реестр пространств имён Strict ↔ Transitional + определение conformance.
- Модель ошибок `StrictError` и `ResourceLimits`.
- Скелет CLI: команды `inspect` и `check` (детекция + карта частей).
- Fuzz-таргеты `zip`, `xml`, `namespaces`.
- Публичный API крейта `strict-ooxml-core`.

### 2.2. Не входит
- WordprocessingML DOM и его элементы (Этап 2).
- Нормализация T1–T8 (Этап 6) — только закладывается точка расширения.
- SVG-рендеринг (Этап 4).
- Полный Feature Report (Этап 3) — только `conformance` + карта частей.
- Загрузка медиа в память (только доступ по запросу).

---

## 3. Зависимости (предлагаемые)

| Зависимость | Версия | Назначение | Обоснование |
|---|---|---|---|
| `miniz_oxide` | 0.8+ | DEFLATE-декодер | Pure Rust, без C, безопасно, fuzz-friendly |
| `memchr` | 2 | Быстрый поиск в буферах | Парсинг центральной директории |
| `encoding_rs` | 0.8 | UTF-16/legacy кодировки | XML-декодирование |
| `thiserror` | 1/2 | Эргономичные ошибки | Стандарт экосистемы |
| `quick-xml` | 0.36+ | Низкоуровневый XML-токенизатор | Либо собственная реализация (см. 4.1) |
| `proptest` (dev) | 1 | Property-based тесты | 17.1 ТЗ |
| `cargo-fuzz` (dev) | — | Fuzz | 12.3 ТЗ |

Все зависимости проходят `cargo-deny` (лицензии, advisory, дубликаты).

---

## 4. Технические решения (обязательны к согласованию до старта)

### 4.1. ZIP: собственная обвязка (ADR-0001, принято)
Реализуется собственный ридер поверх `miniz_oxide`: ручной разбор EOCD и
центральной директории, чтение локальных заголовков. Причины:
- точный контроль лимитов (распакованный размер, ratio, число записей)
  до начала распаковки;
- защита от path traversal и дубликатов частей;
- отсутствие «удобной» семантики сторонних ZIP-крейтов, затрудняющей аудит.
Ридер изолируется в модуле `opc::zip` и покрывается fuzz. ADR-0001 обязателен к созданию (S1.1).

### 4.2. XML: `quick-xml` + обёртка-лимитер (ADR-0002, принято)
`quick-xml` в режиме низкоуровневых событий + **собственный слой
namespace-резолвинга и лимитов** поверх него. DTD и внешние сущности отключаются
(ручной запрет DOCTYPE до токенизации). Риск зависимости
купируется: (а) запретом DOCTYPE на уровне нашего лимитера; (б) fuzz.
ADR-0002 обязателен к созданию (S1.1).

### 4.3. Хранение частей
Части не читаются целиком при открытии. `Package` хранит метаданные (offset, размер,
метод сжатия) и предоставляет `PartReader`, распаковывающий потоково. При запросе
части с превышением `max_single_uncompressed` — `LimitExceeded`.

### 4.4. Conformance-политика по умолчанию
`ConformancePolicy::StrictOnly` на Этапе 1. Точка расширения для `Normalize`
(Этап 6) — трейт/функция `RawNormalizer`, применяемая в `Package::open` между
разбором OPC и разрешением пространств имён.

---

## 5. Структура модулей `strict-ooxml-core`

```
strict-ooxml-core/
├── src/
│   ├── lib.rs                 # реэкспорты, #![deny(missing_docs)]
│   ├── error.rs               # StrictError, LimitKind, Result
│   ├── limits.rs              # ResourceLimits
│   ├── part.rs                # PartId, Part, PartReader, ContentTypeIndex
│   ├── opc/
│   │   ├── mod.rs             # Package, PackageBuilder
│   │   ├── zip.rs             # ZipReader (central dir, local headers)
│   │   ├── content_types.rs   # [Content_Types].xml
│   │   ├── rels.rs            # .rels parser + RelationshipGraph (T2-ready)
│   │   └── path.rs            # канонизация target-путей
│   ├── xml/
│   │   ├── mod.rs             # XmlReader, XmlEvent
│   │   ├── qname.rs           # QName, NsUri
│   │   ├── ns_stack.rs        # scope префиксов
│   │   └── safety.rs          # запрет DTD/сущностей, depth/attr лимиты
│   ├── ns/
│   │   ├── mod.rs             # NamespaceRegistry, Conformance
│   │   ├── registry.rs        # таблица Strict/Transitional
│   │   └── detect.rs          # detect_conformance()
│   └── normalize/
│       └── mod.rs             # RawNormalizer (заготовка для T1–T8)
└── tests/
    ├── opc_*.rs
    ├── xml_*.rs
    ├── ns_*.rs
    └── fixtures/
```

---

## 6. Публичные интерфейсы (черновик)

### 6.1. Ошибки и лимиты

```rust
pub type Result<T> = core::result::Result<T, StrictError>;

#[non_exhaustive]
pub enum StrictError { /* см. ТЗ раздел 13 */ }

pub struct ResourceLimits { /* см. ТЗ раздел 12.1 */ }
impl Default for ResourceLimits { /* безопасные значения */ }
```

### 6.2. Части

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PartId(Arc<str>); // "/word/document.xml"

pub struct Part {
    pub id: PartId,
    pub content_type: Option<Arc<str>>,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression: Compression, // Stored | Deflate
}

pub trait PartSource {
    fn open_part(&self, id: &PartId) -> Result<Box<dyn std::io::Read + '_>>;
}
```

### 6.3. Пакет

```rust
pub struct Package { /* ... */ }

impl Package {
    pub fn open_reader<R: Read + Seek>(reader: R, opts: &OpenOptions) -> Result<Package>;
    pub fn open_path(path: impl AsRef<Path>, opts: &OpenOptions) -> Result<Package>;

    pub fn parts(&self) -> impl Iterator<Item = &Part>;
    pub fn part(&self, id: &PartId) -> Option<&Part>;
    pub fn content_type(&self, id: &PartId) -> Option<&str>;

    pub fn relationships(&self, from: &PartId) -> &[Relationship];
    pub fn resolve_relationship(&self, from: &PartId, rel_id: &str) -> Result<&Relationship>;

    pub fn conformance(&self) -> Conformance;
    pub fn main_document_part(&self) -> Result<&PartId>;
    pub fn source(&self) -> &dyn PartSource;
}
```

### 6.4. Relationships

```rust
pub struct Relationship {
    pub id: String,
    pub rel_type: RelType,
    pub raw_type: String,
    pub target: String,
    pub target_mode: TargetMode,
    pub resolved: Option<PartId>,
}
pub enum TargetMode { Internal, External }
pub enum RelType { OfficeDocument, Styles, Numbering, Settings, Theme,
                   FontTable, Image, Hyperlink, Header, Footer,
                   Footnotes, Endnotes, Other(String) }
```

### 6.5. XML

```rust
pub struct XmlReader<'a> { /* wraps bytes + quick-xml + ns stack */ }

pub enum XmlEvent<'a> {
    StartElement { name: QName<'a>, attrs: Vec<Attr<'a>> },
    EndElement { name: QName<'a> },
    Text(&'a str),
    CData(&'a str),
    Eof,
}

pub struct Attr<'a> { pub name: QName<'a>, pub value: Cow<'a, str> }

pub struct QName<'a> {
    pub ns: Option<NsUri<'a>>, // разрешённый URI
    pub prefix: Option<&'a str>,
    pub local: &'a str,
}

impl<'a> XmlReader<'a> {
    pub fn new(bytes: &'a [u8], limits: &ResourceLimits) -> Result<Self>;
    pub fn next_event(&mut self) -> Result<Option<XmlEvent<'a>>>;
}
```

### 6.6. Пространства имён

```rust
pub enum Conformance { Strict, Transitional, Mixed, Unknown }

pub struct NamespaceRegistry { /* ... */ }
impl NamespaceRegistry {
    pub fn global() -> &'static NamespaceRegistry;
    pub fn lookup(&self, uri: &str) -> Option<&NamespaceEntry>;
}

pub struct NamespaceEntry {
    pub key: &'static str,          // "wordprocessingml.main"
    pub strict: Option<&'static str>,
    pub transitional: Option<&'static str>,
    pub in_scope: ScopeSupport,
    pub verified: bool,
}

pub fn detect_conformance(
    root_ns: &[&str],
    rel_types: &[&str],
    content_types: &ContentTypeIndex,
) -> Result<Conformance>;
```

### 6.7. Опции

```rust
pub struct OpenOptions {
    pub conformance: ConformancePolicy,
    pub limits: ResourceLimits,
    // заготовки под Этап 6:
    pub normalization: Option<Box<dyn RawNormalizer + Send + Sync>>,
}
pub enum ConformancePolicy { StrictOnly, Normalize, Permissive }
```

---

## 7. Декомпозиция работ (WBS)

| ID | Работа | Артефакт | Оценка, ч |
|---|---|---|---|
| S1.1 | ADR-0001/0002, каркас крейта, lint-политика | `docs/adr/*`, `Cargo.toml`, `lib.rs` | 16 |
| S1.2 | `ResourceLimits`, `StrictError`, `LimitKind` | `limits.rs`, `error.rs` | 16 |
| S1.3 | ZIP-ридер: EOCD, ZIP64, центральная директория | `opc/zip.rs` | 48 |
| S1.4 | Локальные заголовки, потоки `PartReader`, лимиты распаковки | `opc/zip.rs`, `part.rs` | 40 |
| S1.5 | Защита: ratio, total size, entries, path traversal, дубликаты | `opc/zip.rs`, `opc/path.rs` | 24 |
| S1.6 | `[Content_Types].xml` + `ContentTypeIndex` | `opc/content_types.rs` | 24 |
| S1.7 | `.rels` парсер + `RelationshipGraph` | `opc/rels.rs` | 32 |
| S1.8 | `Package` API, lazy part access, `main_document_part` | `opc/mod.rs` | 24 |
| S1.9 | XML-токенизатор + `XmlEvent` + безопасность (DTD/сущности/depth) | `xml/*.rs` | 56 |
| S1.10 | Namespace-стек, `QName`, `UnboundPrefix` | `xml/ns_stack.rs`, `xml/qname.rs` | 32 |
| S1.11 | Реестр namespace Strict↔Transitional (таблица из ТЗ 9.3) | `ns/registry.rs` | 24 |
| S1.12 | `detect_conformance` + обработка Mixed | `ns/detect.rs` | 24 |
| S1.13 | Заготовка `RawNormalizer` (интерфейс под Этап 6) | `normalize/mod.rs` | 8 |
| S1.14 | CLI `inspect` и `check` | `strict-ooxml-cli/src/main.rs` | 32 |
| S1.15 | Модульные + интеграционные тесты, фикстуры | `tests/*` | 48 |
| S1.16 | Fuzz-таргеты zip/xml/namespaces | `fuzz/fuzz_targets/*` | 24 |
| S1.17 | Документация публичного API, примеры | rustdoc, `examples/` | 24 |
| S1.18 | CI: сборка/тесты/clippy/deny/покрытие | `.github/workflows/*` | 16 |
| **Итого** | | | **≈ 512** |

> Оценка включает буфер на ревью; при параллельной работе 2 разработчиков —
> календарно 2–3 месяца.

---

## 8. Порядок выполнения (критический путь)

1. S1.1 → S1.2 (фундамент, ошибки/лимиты).
2. S1.3 → S1.4 → S1.5 (ZIP + безопасность) — критический путь.
3. S1.6 → S1.7 → S1.8 (OPC-модель).
4. S1.9 → S1.10 (XML + namespaces) — параллельно с (3).
5. S1.11 → S1.12 (реестр + детекция) — зависит от (3) и (4).
6. S1.13 (заготовка), S1.14 (CLI) — после (5).
7. S1.15–S1.18 (тесты, fuzz, доки, CI) — непрерывно, финализация в конце.

---

## 9. Тест-план

### 9.1. Модульные тесты
- ZIP: EOCD с/без ZIP64; store/deflate; пустой архив; битый CRC; усечённый файл.
- Path: `a/../b`, `/abs`, `./x`, `..`, дубликаты, обратные слеши (Win vs ZIP).
- Content types: только Default; Default+Override; отсутствие `[Content_Types].xml`.
- Rels: package-level `_rels/.rels`; part-level; external; циклические ссылки.
- XML: UTF-8/UTF-16 BOM; namespace scope; вложенные префиксы; CDATA;
  `xml:space`; unbound prefix; DOCTYPE (должен быть отвергнут).
- Limits: depth, attrs-per-elem, text len — каждое превышение → нужный `LimitKind`.
- Namespaces: таблица реестра, `detect_conformance` на Strict/Transitional/Mixed.

### 9.2. Интеграционные тесты
- Открыть реальный Strict `.docx` → `conformance == Strict`, `main_document_part` найден.
- Открыть Transitional `.docx` в `StrictOnly` → `TransitionalNotSupported` с локацией.
- Открыть Transitional в `Normalize` (заглушка) → понятная `Unsupported`-ошибка
  (до Этапа 6) либо no-op при включённом NoopNormalizer.
- Повреждённые/усечённые/пустые файлы → `Err`, без паник.
- ZIP-bomb фикстура → `LimitExceeded`.

### 9.3. Property-based тесты (`proptest`)
- Канонизация путей: `canon(p) == canon(canon(p))` (идемпотентность).
- ZIP round-trip: сгенерированный архив → части с ожидаемыми байтами.
- Namespace-стек: произвольные scope-последовательности → корректный резолвинг.

### 9.4. Fuzz (`cargo-fuzz`)
- `fuzz_zip`: сырые байты → `Package::open_reader` без паник.
- `fuzz_xml`: сырые байты → `XmlReader` до EOF без паник.
- `fuzz_relpath`: строки → канонизация без паник.
- Условие: 24 ч без крашей на каждом таргете.

### 9.5. Корпус (фикстуры)

| Категория | Файлы |
|---|---|
| Strict валидный | `strict_min.docx`, `strict_with_rels.docx` |
| Transitional | `transitional_min.docx` |
| Повреждённые | `truncated.zip`, `bad_crc.docx`, `no_content_types.docx` |
| Враждебные | `zip_bomb.docx`, `deep_xml.zip`, `path_traversal.zip` |
| XML-границы | `utf16.docx`, `entities.zip`, `cdata.docx` |

---

## 10. Критерии приёмки этапа

1. Оба режима открытия (`path`, `reader`) работают на Strict-корпусе.
2. Transitional-файл **определяется**; в `StrictOnly` отклоняется с
   `TransitionalNotSupported` и локацией первого несоответствия.
3. `Mixed` определяется как `MixedConformance`.
4. Ни одной паники на повреждённом/враждебном корпусе.
5. Fuzz: 24 ч без крашей на `fuzz_zip`, `fuzz_xml`, `fuzz_relpath`.
6. Покрытие `strict-ooxml-core` ≥ 80% строк (и ≥ 70% ветвлений).
7. Все лимиты из 12.1 соблюдаются и переопределяемы.
8. `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check` — зелёные.
9. CLI `inspect` печатает: conformance, список частей с content-type, граф
   relationships; `check` возвращает код 0/1/2 (0 = Strict ok, 1 = Transitional
   в StrictOnly, 2 = повреждённый/ошибка).
10. `#![deny(missing_docs)]` проходит; пример в `examples/open_strict.rs` собирается.

---

## 11. Definition of Done

- [ ] Код в `strict-ooxml-core` соответствует структуре раздела 5.
- [ ] Публичный API реализован согласно разделу 6 (или отклонения зафиксированы в ADR).
- [ ] Модульные/интеграционные/property тесты добавлены и зелёные.
- [ ] Fuzz-таргеты добавлены; протокол 24-часовой сессии приложен.
- [ ] Покрытие подтверждено отчётом (`cargo-llvm-cov`).
- [ ] CI зелёный на Linux/macOS/Windows.
- [ ] ADR-0001 (ZIP-бэкенд), ADR-0002 (XML-бэкенд) утверждены.
- [ ] README крейта + rustdoc; пример компилируется.
- [ ] Точки расширения под Этап 6 (`RawNormalizer`) присутствуют.
- [ ] Ревью проведено; замечания закрыты или заведены задачи.

---

## 12. Риски этапа и митигации

| Риск | Митигация |
|---|---|
| Ошибки в разборе ZIP64/центральной директории | Приоритетные тесты + fuzz + сверка с реальными файлами Word/LibreOffice |
| `quick-xml` не даёт нужного контроля над DTD | Обёртка-лимитер, запрет DOCTYPE до токенизации; при неудаче — собственный токенизатор (ADR) |
| Ложная классификация Strict/Transitional | Мультисигнальная детекция (ns + rel types + content types), тесты на Mixed |
| Производительность на больших пакетах | Lazy-доступ, бенчмарки на 100-страничном файле |
| Утечка ресурсов при ошибке распаковки | RAII-обёртки, проверка лимитов до аллокации |

---

## 13. Зафиксированные решения (ранее открытые вопросы)

Все вопросы сняты заказчиком; решения обязательны к исполнению на Этапе 1.

| № | Вопрос | Решение |
|---|---|---|
| 1 | ZIP-бэкенд | Собственный ридер поверх `miniz_oxide` — ADR-0001 |
| 2 | XML-бэкенд | `quick-xml` + собственная обёртка-лимитер — ADR-0002 |
| 3 | MSRV | Rust 1.75+ |
| 4 | Features по умолчанию (мета-крейт) | `default = svg + report`; `round-trip-normalize` отдельно |
| 5 | `check` при Transitional | Код 0 = Strict OK, 1 = Transitional, 2 = ошибка |
| 6 | ZIP64 | Желательно, не блокер приёмки |
| 7 | `w:altChunk` | Неподдерживаемый блок + Feature Report, содержимое не встраивать |
| 8 | `TargetMode="External"` | Не загружать (без сети), фиксировать |
| 9 | Direction-neutral renames | Отложены до XSD-диффа Этапа 0, `Verified=false` |

Соответствующие ADR (`docs/adr/0001-zip-backend.md`, `docs/adr/0002-xml-backend.md`)
создаются в рамках S1.1.

---

**Конец задачи.**
