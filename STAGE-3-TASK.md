# Заказ на разработку. Этап 3 — Система отчётности (Feature Report)

**Шифр:** ZAKAZ-STAGE-3 (TASK-STAGE-3)  
**Связь с ТЗ:** `TZ-STRICT-OOXML-RUST.md` §11 (Feature Report v2), §5.1–§5.3 (крейты/слои), §6 (публичный API), §15 «Этап 3», §17 (тестирование)  
**Основание:** Этапы 1 и 2 приняты; `strict-ooxml-wml` поставляет `SupportModel` (вход отчёта).  
**Новый крейт:** `strict-ooxml-report` + интеграция в `strict-ooxml` (meta) и `strict-ooxml-cli`  
**Оценка:** 1–2 месяца (≈ 200–320 человеко-часов)  
**Статус:** К реализации  

---

## 1. Цель этапа

Превратить внутреннюю `SupportModel` (Этап 2) в **Feature Report** —
формализованный, детерминированный и валидный по JSON-схеме отчёт о том, какие
механизмы Strict поддержаны полностью / частично / не поддержаны, с локациями;
а также дать CLI-команду `check` с понятными кодами возврата и
человекочитаемый отчёт.

Этап **не** рендерит (Этап 4) и **не** выполняет нормализацию Transitional
(Этап 6) — блок `normalization` присутствует, но пуст.

---

## 2. Scope

### 2.1. Входит
- Крейт `strict-ooxml-report`: модель отчёта, сборка из `SupportModel` + conformance,
  сериализация в JSON, человекочитаемый рендер.
- JSON-схема отчёта (v2), версионируется в репозитории; валидация в тестах.
- Правила агрегации: `status`, `severity`, `overall_status`, `summary`.
- Трассируемость: каждый `partial/unsupported/error` имеет ≥ 1 локацию.
- Интеграция в meta-крейт: `StrictDocument::support_report()`, `report_json()`,
  `report_text()`.
- CLI: расширение `check` (полный отчёт + коды 0/1/2) и команда `report`.
- Детерминизм вывода (побайтовая стабильность JSON).
- Тесты (схема/golden/determinism/CLI/корпус) и независимый оракул отчёта.

### 2.2. Не входит
- Рендеринг SVG/PNG (Этап 4).
- Нормализация Transitional и Loss Report (Этап 6) — только зарезервированный
  пустой блок.
- Полный Feature Report для Transitional-документов (без нормализации парсинг
  запрещён; см. §7 — решение по поведению).
- Изменение модели/парсера WML, кроме случаев, необходимых для отчёта
  (например, несколько локаций на механизм — см. §9, решение).

---

## 3. Входы (интерфейсы Этапа 2/1)

| Источник | Что даёт |
|---|---|
| `strict_ooxml_wml::model::support::{SupportModel, FeatureUse, SupportStatus}` | агрегированные механизмы: `feature_id`, `status`, `message`, `location`, `count` |
| `strict_ooxml_wml::model::Document` | `support()`, `support_debug()`, источник частей |
| `strict_ooxml_core::opc::{Package, ConformancePolicy}` | conformance, части, relationships |
| `strict_ooxml_core::ns::Conformance` | `Strict/Transitional/Mixed/Unknown` |
| `strict_ooxml_core::error::{StrictError, SourceLocation}` | локации и ошибки |
| `strict-ooxml` (meta) | `StrictDocument` — точка входа |

> Ограничение текущей модели: `FeatureUse` хранит **одну** локацию и **одно**
> сообщение (первое вхождение) + `count`. Требование «указаны все
> неподдержанные механизмы с локацией» выполнимо; требование «все локации» —
> нет. Решение — §9, вопрос 1.

---

## 4. Архитектура

```
strict-ooxml-report/
├── src/
│   ├── lib.rs              # публичный API, #![deny(missing_docs, unsafe_code)]
│   ├── model.rs            # SupportReport, Feature, Severity, OverallStatus, Summary
│   ├── build.rs            # сборка отчёта из (SupportModel, Conformance, мета)
│   ├── severity.rs         # правила severity/overall_status
│   ├── json.rs             # сериализация (детерминированная)
│   ├── text.rs             # человекочитаемый рендер
│   └── schema.rs           # встроенная JSON-схема + (dev) валидация
├── schema/
│   └── support-report.schema.json   # JSON Schema (v2), source of truth
└── tests/ (schema, golden, determinism, cli-...)
```

Зависимости: `strict-ooxml-core`, `strict-ooxml-wml`. Для JSON — см. ADR:
рекомендуется `serde` + `serde_json` (сериализация) и `jsonschema` (**dev-only**,
для валидации в тестах). Альтернатива — ручной writer JSON (не рекомендуется:
риск невалидного JSON). Решение фиксируется ADR-0005.

Meta-крейт `strict-ooxml`: включает `strict-ooxml-report` под feature `report`
(уже зарезервирована).

---

## 5. Модель отчёта (соответствие ТЗ §11)

```rust
pub struct SupportReport {
    pub schema_version: String,        // "2.0"
    pub file: String,                  // имя/путь входного файла
    pub standard: String,              // "ISO/IEC 29500-1:2008 Strict"
    pub tool: Tool,                    // name, version
    pub conformance: ConformanceBlock, // declared, detected, normalized
    pub overall_status: OverallStatus, // supported | partial | unsupported
    pub summary: Summary,              // supported/partial/unsupported/ignored/error
    pub features: Vec<Feature>,        // отсортированы по feature_id
    pub normalization: NormalizationBlock, // пусто до Этапа 6
}

pub struct Feature {
    pub feature_id: String,            // "w:tbl", "wp:anchor"
    pub status: FeatureStatus,         // supported|partial|unsupported|ignored|error
    pub severity: Severity,            // info|warning|error
    pub message: Option<String>,
    pub locations: Vec<Location>,      // ≥1 для partial/unsupported/error
    pub count: u32,
}

pub enum Severity { Info, Warning, Error }
pub enum OverallStatus { Supported, Partial, Unsupported }
```

JSON-представление — ровно как в `TZ §11.1` (поля `schema_version`, `file`,
`standard`, `tool`, `conformance`, `overall_status`, `summary`, `features`,
`normalization`).

**Статусы/severity (правила, обязательны к документированию):**
- `status`: `supported`, `partial`, `unsupported`, `ignored`, `error`
  (в `SupportModel` нет `error` — он появляется из проблем сборки отчёта;
  см. §9, вопрос 2).
- `severity` по умолчанию: `supported/ignored → info`, `partial → warning`,
  `unsupported → warning`, `error → error`. Таблица может быть уточнена
  (например, обязательные элементы → `error`); фиксируется в схеме.
- `overall_status`: агрегация «worst of» (`unsupported`/`error` → `unsupported`;
  иначе `partial` при наличии `partial`; иначе `supported`) — см. §9, вопрос 3.

**Инварианты отчёта (проверяются тестами, не «на глаз»):**
1. `summary.*` равны фактическим счётчикам `features` по статусам.
2. Каждый `partial/unsupported/error` имеет `locations.len() >= 1`.
3. `features` отсортированы по `feature_id` (детерминизм).
4. Сериализация побайтово одинакова при повторных прогонах (и независима от
   порядка `HashMap`/`BTreeMap`).
5. Отчёт валиден по `support-report.schema.json`.

---

## 6. JSON-схема и детерминизм

- `strict-ooxml-report/schema/support-report.schema.json` — **источник истины**,
  версионируется; поле `schema_version` синхронизировано с ней.
- Валидация: dev-dependency `jsonschema`; тест «каждый golden/сгенерированный
  отчёт валиден по схеме». Это **независимый оракул** (см. §8): схема — внешний
  контракт, а не совпадающая с кодом структура.
- Детерминизм: стабильная сортировка (`feature_id`), отсутствие
  неупорядоченных коллекций в выводе; тест «два прогона → идентичные байты».
- Кодировка UTF-8; перевод строки `\n`; завершающий перевод строки — зафиксировать.

---

## 7. Человекочитаемый отчёт и CLI

### 7.1. Текстовый рендер
Сводка (overall, счётчики, conformance) + список механизмов с severity,
статусом, числом вхождений и локацией; проблемы (`partial/unsupported/error`) —
в конце/выделены. Порядок детерминирован. Сообщения — на английском (ТЗ §14);
локализация — вне scope.

### 7.2. CLI
- `check <file>` — открыть (StrictOnly) → разобрать → отчёт → краткая сводка.
  **Коды возврата (уточнение ТЗ G.8):**
  - `0` — нет критических проблем (нет `error`/`unsupported`-блокеров);
  - `1` — есть критические проблемы (по правилам §5) **или** документ
    Transitional в режиме StrictOnly;
  - `2` — повреждённый вход / внутренняя ошибка.
- `report <file> [--json] [--out <path>]` — полный отчёт (JSON по умолчанию или
  человекочитаемый при `--text`); код `0` при успешной генерации.
- `inspect` — сохранить текущее поведение (карта частей/relationships).

**Открытый вопрос (§9, вопрос 4):** поведение `check`/`report` на Transitional
под `Permissive` (отчёт с `conformance.detected = transitional` и пустыми
`features` без нормализации — или отказ).

---

## 8. Тестирование и независимый оракул

### 8.1. Виды
- Unit: правила severity/overall/summary; сборка из `SupportModel`.
- **Schema-валидация** (dev `jsonschema`) — каждый отчёт валиден.
- Determinism: побайтовое равенство двух прогонов и при перемешанном порядке.
- Golden: эталонные JSON и текстовые отчёты на синтетических Strict-документах.
- CLI: `check` коды 0/1/2; `report --json/--text`; `report --out`.
- Корпус: прогон по `strict-ooxml-core/tests/samples/` (Transitional → ожидаемое
  поведение по §7.2).

### 8.2. Независимый оракул (обязательно; уроки Этапа 2)
Отчёт не должен проверяться только собственными golden-снапшотами. Минимум два
независимых источника:
1. **JSON Schema** (внешний контракт) — валидация отчёта независимым
   валидатором (`jsonschema`).
2. **Инварианты против `SupportModel`** — тест: множество `feature_id` отчёта
   равно множеству из `SupportModel`; счётчики совпадают; каждый
   `partial/unsupported` имеет локацию. Оракул должен «падать» на намеренно
   испорченном отчёте (самопроверка).

Дополнительно: тест, что `report --json` на документе с заведомо
неподдержанным механизмом (например, `wp:anchor`) содержит его с severity и
локацией.

---

## 9. Открытые вопросы (решения до старта)

1. **Множественные локации.** Расширить `SupportModel`/`FeatureUse` до
   нескольких локаций (с ограничением N на механизм) или ограничиться первой?
   ТЗ §11 показывает массив `locations`. Рекомендация: хранить до N (например,
   8) локаций на механизм + `count`; N и политика — ADR.
2. **`status: error`.** Откуда берутся `error`: только сбои сборки отчёта или
   также «обязательный механизм не поддержан»? Нужна таблица «механизм →
   severity».
3. **Агрегация `overall_status`.** Подтвердить правило (§5) или иное.
4. **Transitional/Permissive.** Формировать ли отчёт с `detected=transitional`
   и пустыми `features`, или отказывать.
5. **JSON-стек.** `serde`+`serde_json` (+ dev `jsonschema`) — согласовать
   добавление зависимостей; ADR-0005.
6. **Версия схемы.** Начать с `"2.0"` (как в ТЗ) или `"1.0"` до релиза.

---

## 10. Декомпозиция работ (WBS)

| ID | Работа | Артефакт | Оценка, ч |
|---|---|---|---|
| S3.1 | ADR-0005 (JSON-стек, локации, severity/overall) | `docs/adr/0005-report.md` | 12 |
| S3.2 | Каркас крейта, линты, интеграция в workspace/meta | `Cargo.toml`, `lib.rs` | 12 |
| S3.3 | Модель отчёта (`model.rs`, `severity.rs`) | `report/src` | 40 |
| S3.4 | Сборка из `SupportModel`+conformance (`build.rs`) | `report/src` | 40 |
| S3.5 | JSON-сериализация + детерминизм (`json.rs`) | `report/src` | 32 |
| S3.6 | JSON Schema + встроенная валидация-контракт | `schema/`, `schema.rs` | 24 |
| S3.7 | Человекочитаемый рендер (`text.rs`) | `report/src` | 24 |
| S3.8 | Публичный API meta-крейта (`support_report`/`report_json`/`report_text`) | `strict-ooxml/src` | 16 |
| S3.9 | CLI: расширенный `check` + `report` (коды, `--json/--text/--out`) | `cli/src` | 40 |
| S3.10 | Тесты: unit, schema, determinism, golden, corpus | `report/tests`, `cli/tests` | 72 |
| S3.11 | Независимый оракул отчёта + самопроверка | `report/tests` | 24 |
| S3.12 | CI: schema-валидация, CLI-тесты, gate | `.github/workflows/ci.yml` | 12 |
| S3.13 | Документация API + примеры | rustdoc, `examples/` | 16 |
| **Итого** | | | **≈ 364** |

> P0-путь: S3.1 → S3.3 → S3.4 → S3.5 → S3.9 → S3.11.

---

## 11. Критерии приёмки (ТЗ §15 Этап 3 + уточнения)

1. Для **любого** успешно разобранного Strict-документа формируется отчёт
   (`StrictDocument::support_report()`).
2. Отчёт **валиден по JSON-схеме** (независимая валидация в тестах).
3. **Все** `partial`/`unsupported`/`error` механизмы указаны с ≥ 1 локацией.
4. `summary`, `overall_status`, `severity` согласованы с `features`
   (инварианты §5 проверяются тестами).
5. Вывод **детерминирован** (побайтово одинаков при повторных прогонах).
6. CLI: `check` возвращает `0` при отсутствии критических проблем и `1` при их
   наличии (и/или Transitional), `2` — повреждённый вход; `report` эмитит
   JSON/текст.
7. Человекочитаемый отчёт содержит сводку и проблемы с локациями.
8. Независимый оракул (§8.2) внедрён и самопроверен (падает на испорченном
   отчёте).
9. `cargo fmt/clippy -D warnings/test/doc/deny` — зелёные; покрытие
   `strict-ooxml-report` ≥ 80% строк; CI на Linux/macOS/Windows.
10. Публичный API документирован; примеры компилируются.

### Definition of Done
- [ ] Крейт `strict-ooxml-report` реализован по §4.
- [ ] `schema/support-report.schema.json` — источник истины; валидация в CI.
- [ ] Meta-крейт и CLI интегрированы; коды возврата соответствуют §7.2.
- [ ] Инварианты §5 и оракул §8.2 покрыты тестами и зелёные.
- [ ] Детерминизм подтверждён тестом.
- [ ] Покрытие ≥ 80%; CI зелёный на 3 ОС.
- [ ] ADR-0005 и `docs/stage-3-report.md` обновлены; ревью проведено.

---

## 12. Риски

| Риск | Вероятность | Влияние | Митигация |
|---|---|---|---|
| Расхождение «схема ↔ код» | Средняя | Высокое | Схема как источник истины + независимый валидатор |
| Детерминизм ломается (порядок коллекций) | Средняя | Среднее | Сортировка по `feature_id`, тест на байтовое равенство |
| Слишком много/мало `error` (ложные «критические») | Средняя | Среднее | Таблица severity + согласование с заказчиком (§9) |
| Рост объёма отчёта на больших документах | Низкая | Среднее | Ограничение локаций N, только агрегаты |
| Новые зависимости (`serde`/`jsonschema`) | Средняя | Низкое | ADR, `cargo-deny`, `jsonschema` только dev |
| Неверная агрегация `overall_status` | Низкая | Среднее | Зафиксировать правило и тесты |
| Повторение класса «самосогласованных» ошибок | Средняя | Высокое | Независимый оракул (§8.2) |

---

## 13. Ожидаемые артефакты

- Крейт `strict-ooxml-report` (модель, сборка, JSON, текст, schema).
- `schema/support-report.schema.json` (v2).
- Интеграция в `strict-ooxml` (meta) и `strict-ooxml-cli`.
- Тесты (unit/schema/determinism/golden/corpus/oracle) и примеры.
- `docs/adr/0005-report.md`, `docs/stage-3-report.md`.
- Обновлённый CI.

---

**Конец заказа.**
