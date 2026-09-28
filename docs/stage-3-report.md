# Этап 3 — отчёт о выполнении (Feature Report)

**Задача:** `STAGE-3-TASK.md` (ZAKAZ-STAGE-3 / TASK-STAGE-3)  
**Связь с ТЗ:** §5.1–§5.4, §6, §11, §15 (Этап 3), §17; ADR-0005  
**Окружение:** Windows x86_64 (MSVC), Rust 1.91.1, cargo 1.91.1,
cargo-llvm-cov 0.9.0, cargo-deny  
**Крейты:** новый `strict-ooxml-report`; интеграция в `strict-ooxml` (meta) и
`strict-ooxml-cli`  
**Статус:** реализовано; инварианты §5 и независимый оракул §8.2 зелёные.

---

## 1. Что сделано (WBS STAGE-3)

| ID | Работа | Артефакт | Статус |
|---|---|---|---|
| S3.1 | ADR-0005 (JSON-стек, локации, severity/overall, Transitional) | `docs/adr/0005-report.md` | готово |
| S3.2 | Каркас крейта, линты, интеграция в workspace | `strict-ooxml-report/Cargo.toml`, `lib.rs` | готово |
| S3.3 | Модель отчёта (`model.rs`, `severity.rs`) | `report/src` | готово |
| S3.4 | Сборка из `SupportModel`+conformance (`build.rs`) | `report/src` | готово |
| S3.5 | JSON-сериализация + детерминизм (`json.rs`) | `report/src` | готово |
| S3.6 | JSON Schema + встроенная валидация-контракт | `schema/`, `schema.rs` | готово |
| S3.7 | Человекочитаемый рендер (`text.rs`) | `report/src` | готово |
| S3.8 | Публичный API meta-крейта | `strict-ooxml/src/lib.rs` | готово |
| S3.9 | CLI: расширенный `check` + `report` | `strict-ooxml-cli/src/main.rs` | готово |
| S3.10 | Тесты: unit, schema, determinism, golden, corpus | `report/tests`, `cli/tests` | готово |
| S3.11 | Независимый оракул + самопроверка | `report/tests/{oracle,schema}.rs` | готово |
| S3.12 | CI: schema-валидация, CLI-тесты, покрытие, deny | `.github/workflows/ci.yml` | готово |
| S3.13 | Документация API + пример | rustdoc, `strict-ooxml/examples/support_report.rs` | готово |

---

## 2. Решения (ADR-0005)

1. **JSON-стек.** `serde` + `serde_json` для сериализации; `jsonschema`
   (**dev-only**, `default-features = false`) как независимый валидатор в
   тестах. Ручной writer отклонён.
2. **Схема/версия.** `schema/support-report.schema.json` — источник истины,
   `schema_version = "2.0"` (синхронизировано с `model::SCHEMA_VERSION`).
3. **Локации.** `FeatureUse` расширен до `locations: Vec<SourceLocation>` с
   лимитом `MAX_LOCATIONS_PER_FEATURE = 8`, дедупликацией и стабильным порядком
   «первого появления»; `count` остаётся полным счётчиком.
4. **severity/overall.** Таблица `supported/ignored → info`,
   `partial/unsupported → warning`, `error → error` закодирована и в коде, и в
   схеме (`if/then`). `overall_status` — «worst of»; `error` зарезервирован.
   **Критическая проблема** (для `check`) — `unsupported` или `error`.
5. **Transitional.** Отказ (нужна нормализация Этапа 6): `check` → код `1`,
   `report` → код `2`, отчёт не формируется.

---

## 3. Результаты команд (фактические)

| Проверка | Команда | Результат |
|---|---|---|
| Формат | `cargo fmt --all -- --check` | ✅ exit 0 |
| Линт | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ exit 0 |
| Сборка | `cargo check --workspace --all-targets --all-features` | ✅ exit 0 |
| Тесты | `cargo test --workspace --all-features` | ✅ **219 тестов**, 0 падений |
| Docs | `cargo doc --workspace --no-deps` | ✅ exit 0 |
| Зависимости | `cargo deny check` | ✅ advisories/bans/licenses/sources ok |
| Покрытие core | `cargo llvm-cov -p strict-ooxml-core --fail-under-lines 80` | ✅ 87.93% строк |
| Покрытие wml | `cargo llvm-cov -p strict-ooxml-wml --fail-under-lines 80` | ✅ 90.64% строк |
| Покрытие report | `cargo llvm-cov -p strict-ooxml-report --fail-under-lines 80` | ✅ **99.82% строк** |

Тесты `strict-ooxml-report`: 37 (21 unit + corpus 1 + determinism 3 + golden 2 +
oracle 6 + schema 4). CLI: 12.

---

## 4. Независимый оракул и самопроверка (`STAGE-3-TASK.md` §8.2)

Два независимых источника, оба внедрены:

1. **JSON Schema** (`report/tests/schema.rs`) — каждый сгенерированный и
   golden-отчёт валидируется внешним валидатором `jsonschema`. Отрицательные
   контроли: `partial` без локации, несоответствие `status`/`severity`,
   лишнее поле — **отвергаются**.
2. **Инварианты против `SupportModel`** (`report/tests/oracle.rs`, функция
   `oracle_check`): совпадение множеств `feature_id`, счётчиков, сообщений,
   локаций; `≥ 1` локация для `partial`/`unsupported`/`error`; согласованность
   `summary` и `overall_status`; сортировка. Отрицательные контроли (сломанная
   локация, счётчик, лишний механизм, `summary`) — оракул **падает**.

Дополнительно: тест `unsupported_mechanism_is_reported_with_severity_and_location`
проверяет, что заведомо неподдержанный механизм (`wp:anchor`) присутствует с
severity `warning` и локацией.

---

## 5. Критерии приёмки (§11 заказа)

| № | Критерий | Статус |
|---|---|---|
| 1 | Отчёт для любого успешно разобранного Strict-документа (`StrictDocument::support_report()`) | ✅ meta + corpus |
| 2 | Валидность по JSON-схеме (независимая валидация) | ✅ `tests/schema.rs` |
| 3 | `partial`/`unsupported`/`error` — с ≥ 1 локацией | ✅ инвариант + fallback (§ADR) |
| 4 | Согласованность `summary`/`overall_status`/`severity` | ✅ `tests/oracle.rs` |
| 5 | Детерминизм (побайтовое равенство) | ✅ `tests/determinism.rs` |
| 6 | CLI `check` 0/1/2; `report` JSON/текст | ✅ `cli/tests/cli.rs` (12) |
| 7 | Человекочитаемый отчёт со сводкой и проблемами | ✅ `text.rs`, golden |
| 8 | Независимый оракул + самопроверка | ✅ §4 |
| 9 | fmt/clippy/test/doc/deny; покрытие report ≥ 80%; CI 3 ОС | ✅ локально; CI-джобы обновлены |
| 10 | Публичный API документирован; примеры компилируются | ✅ `#![deny(missing_docs)]`, `examples/support_report.rs` |

---

## 6. Как проверить

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test  --workspace --all-features
cargo doc   --workspace --no-deps
cargo deny check
cargo test -p strict-ooxml-report --all-features --test schema
cargo test -p strict-ooxml-report --all-features --test oracle
cargo test -p strict-ooxml-report --all-features --test determinism
cargo llvm-cov -p strict-ooxml-report --all-features --fail-under-lines 80

# CLI
cargo run -p strict-ooxml-cli -- check  <file.docx>   # 0 / 1 / 2
cargo run -p strict-ooxml-cli -- report <file.docx> --text
cargo run -p strict-ooxml --example support_report -- <file.docx>
```

Обновить golden (только при намеренном изменении формата):

```text
UPDATE_GOLDEN=1 cargo test -p strict-ooxml-report --test golden
```

---

## 7. Замечания и границы

- **Семантика отчёта (приёмочная находка F1, вариант (a)):** отчёт — это
  *problem/attention view*, а не карта покрытия. `features` перечисляет
  механизмы, которые записал парсер (как правило, требующие внимания:
  `partial`/`unsupported`/`ignored`); полностью поддержанная ядровая разметка
  **не** перечисляется. `summary.supported` считает только явно записанные
  supported-механизмы и в продакшене обычно `0`. `overall_status == supported`
  означает «не зафиксировано ограничений», а не «всё проверено и поддержано».
  Зафиксировано в ADR-0005; текстовый отчёт печатает поясняющую строку `note:`,
  схема несёт те же описания. Реальная карта покрытия (вариант (b), запись
  `Supported` по инвентарю) отложена на более поздний этап.
- **F2 (закрыто).** `check` больше не печатает `ok:` при наличии блокеров: при
  отсутствии критических проблем — `ok: strict`, иначе —
  `strict: N blocker(s) require attention`; ниже `overall`, `summary` и строки
  `blocker: <id> [<status>] @ <location>`. Код возврата прежний (0/1/2).
- **F3 (закрыто).** В список блокеров включаются и `unsupported`, и `error`
  (зарезервированный статус), поэтому будущий `error` будет показан, а не только
  посчитан.
- **F4 (подтверждено).** ТЗ §15 (Этап 3) дополнено оговоркой: отчёт формируется
  для любого успешно разобранного **Strict**-документа; Transitional без
  нормализации Этапа 6 отчёта не даёт (осознанный отказ). Реализация совпадает:
  `check` → 1, `report` → 2.
- **F5 (информационное, закрыто).** Golden — это фиксация **формата**
  (порядок полей, `note:`, все статусы модели), а не продакшен-поведения;
  назначение задокументировано в `report/tests/golden.rs`. Реальный Strict-пример
  (`w:altChunk`) проверяется в `strict-ooxml/tests/open.rs`.
- **F6 (информационное).** Единственный `expect` — в `json.rs`; сериализация
  модели не может упасть (нет fallible-сериализаторов и map-ключей), что
  зафиксировано в ADR-0005. Оставлен осознанно.
- Блок `normalization` присутствует и пуст (`applied: []`, `losses: []`,
  `invariants_ok: true`) — заполняется на Этапе 6.
- `status: error` зарезервирован и в Stage 3 не производится; схема и модель
  его принимают.
- Корпус `strict-ooxml-core/tests/samples/` полностью Transitional: тест
  `report/tests/corpus.rs` фиксирует отказ (`TransitionalNotSupported`) и
  отсутствие отчёта (§7.2). Strict-путь покрыт golden/oracle/meta-тестами.
- `FeatureUse.location` заменён на `locations` (breaking pre-1.0, разрешено
  §2.2); Stage-2-тесты обновлены, регрессий нет.
