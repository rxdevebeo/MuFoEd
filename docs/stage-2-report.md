# Этап 2 — отчёт о выполнении и доработке

**Задача:** `STAGE-2-TASK.md` (TASK-STAGE-2)  
**Доработка:** `STAGE-2-REWORK.md` (REWORK-STAGE-2, ACCEPT-TASK-STAGE-2)  
**Окружение:** Windows x86_64 (MSVC), Rust 1.91.1, cargo 1.91.1, cargo-llvm-cov 0.9.0  
**Крейты:** `strict-ooxml-wml`, `strict-ooxml`, `xtool`  
**Статус:** блокеры B1/B2 устранены; замечания M1–M9 закрыты (M8 — waiver).

---

## 1. Что сделано (WBS STAGE-2)

| ID | Артефакт | Статус |
|---|---|---|
| S2.1 | `docs/adr/0004-wml-model.md` | готово |
| S2.2 | `strict-ooxml-wml/{Cargo.toml,lib.rs}`, воркспейс | готово |
| S2.3 | `model/*` (Document, Block, Inline, props, styles, numbering, settings, drawing, support) | готово |
| S2.4 | `parse/{interner,mod,dispatch}` + `PartParser` | готово |
| S2.5 | `parse/document.rs` | готово |
| S2.6 | `parse/props.rs` | готово |
| S2.7 | `parse/table.rs` | готово |
| S2.8 | `parse/styles.rs`, `model/styles.rs` | готово |
| S2.9 | `parse/numbering.rs`, `model/numbering.rs` | готово |
| S2.10 | `parse/settings.rs` | готово |
| S2.11 | `sectPr`/секции/колонки | готово |
| S2.12 | `parse/drawing.rs`, `MediaIndex` | готово |
| S2.13 | `resolve/{styles,numbering,rels}` | готово |
| S2.14 | `model/support.rs` | готово |
| S2.15 | мета-крейт `strict-ooxml` (`StrictDocument`) | готово |
| S2.16 | `xtool xsd-inventory`, `coverage/wml-elements.toml` | готово |
| S2.17 | unit/golden (6 снапшотов)/property/corpus/fuzz | готово |
| S2.18 | `benches/wml_parse.rs` | готово |
| S2.19 | rustdoc + 3 примера | готово |
| S2.20 | CI-гейт опц. элементов ≥ 90% | готово |

## 2. Результаты команд (фактические)

| Проверка | Команда | Результат |
|---|---|---|
| Формат | `cargo fmt --all -- --check` | ✅ exit 0 |
| Линт | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ exit 0 |
| Сборка всех целей | `cargo build --workspace --all-targets --all-features` | ✅ exit 0, без `output filename collision` |
| Тесты | `cargo test --workspace --all-features` | ✅ 162 теста, 0 падений |
| Docs | `cargo doc --workspace --no-deps` | ✅ exit 0 |
| Гейт опц. элементов | `cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90` | ✅ 97.3% |
| Покрытие core | `cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80` | ✅ 87.97% строк |
| Покрытие wml | `cargo llvm-cov -p strict-ooxml-wml --all-features --fail-under-lines 80` | ✅ **90.54% строк** |
| Второй отчёт (M1) | `cargo run -p xtool -- corpus-elements` | ✅ см. §4 |

Тестовые функции: `wml` 73, `core` 73, `strict-ooxml` 3, `xtool` 8, `cli` 5.

## 3. Блокеры

### B1 — коллизия имён example (устранён)
`strict-ooxml/examples/open_strict.rs` переименован в
`strict-ooxml/examples/read_document.rs`. Аудит целей воркспейса: других
совпадений нормализованных имён целей нет. `cargo build --workspace
--all-targets --all-features` больше не выдаёт `output filename collision`.

### B2 — покрытие `strict-ooxml-wml` (устранён)
Было 68.52% → стало **90.54% строк** (2790 строк, 264 не покрыто). Гейт
`--fail-under-lines 80` — exit 0.

Новые тесты закрывают группы:
- `tests/model.rs` — лексические значения всех Strict-enum (`from_strict`/
  `as_str`), единицы/цвета, `StyleTable`/`NumberingTable`/`MediaIndex`/
  `SupportModel` (включая `merge`, `default_for`, `resolved_abstract`),
  аксессоры `Block`/`Inline`/`Document`, `MediaKind` (content-type/extension).
- `tests/props.rs` — полные `pPr`/`rPr`/`tblPr`/`trPr`/`tcPr`/`sectPr`
  (границы, заливка, табуляции, `spacing`/`ind`, `tblLayout`, `tblCellMar`,
  `tblLook`, `lnNumType`, `cols`), ветки невалидных enum/чисел.
- `tests/styles_settings.rs` — `styles.xml` целиком (`docDefaults`,
  `latentStyles`, `style` со всеми детьми, отбраковка style без id/type),
  все элементы `settings.xml`, номерные уровни/`lvlOverride`.
- `tests/misc.rs` — ветки inline (комментарии/сноски/endnotes, `fldChar`,
  `sym`, `cr`, CDATA), `background`, MCE, `sdt` c `placeholder`, change-в
  ячейках, `r:link` без `r:embed`, ограничение глубины, усечённый XML.

При правке тестов найдены и исправлены три реальных дефекта парсера:
`w:fldChar/@w:fldCharType`, `w:tblLayout/@w:type`, `w:tblInd/@w:w` читались из
`w:val`.

## 4. Замечания M1–M9

- **M1.** В `coverage/wml-elements.toml` добавлены поля происхождения
  (`source`, `revision`, `generator`, `specification`). Добавлен независимый
  второй отчёт `xtool corpus-elements`: он открывает корпус и классифицирует
  реально встречающиеся элементы. Текущий результат: 11 файлов, 154 различных
  элемента, 98 покрыто, 7 `ignored`, 49 без статуса `supported` — это в
  основном Math (`m:*`), VML (`v:*`) и детали DrawingML вне области inline-картинок
  Этапа 2. Отчёт воспроизводим и не влияет на гейт (инвентарь — курируемый
  список Stage-2; источник зафиксирован).
- **M2.** Golden-DOM расширен до **6 снапшотов**: `basic`, `props`, `table`,
  `list`, `section`, `drawing`; `dump` расширен (секции, медиа, drawing blip).
- **M3.** Добавлен `tests/misc.rs::depth_limit_is_enforced_without_panic`
  (вложенность выше `max_xml_depth` → `LimitExceeded`, без паники). ADR-0004
  приведён в соответствие.
- **M4.** Добавлены unit-тесты `xtool` (8): формула покрытия, разбор инвентаря и
  граничные случаи (пустой инвентарь, отсутствующий/неизвестный статус),
  `status_for`, скан XSD, рендер инвентаря.
- **M5.** `tests/corpus.rs` больше не паникует: ошибки открытия считаются, а не
  приводят к падению; добавлен `damaged_files_do_not_panic` (не-ZIP и пустой
  файл во временном каталоге).
- **M6.** Настоящий отчёт обновлён по фактическим прогонам.
- **M7.** `README.md` дополнен: полный набор команд сборки/теста/покрытия/гейта,
  `fuzz_wml`, ссылки на `STAGE-1-REWORK.md`/`STAGE-2-REWORK.md`.
- **M8.** 24-часовые fuzz-сессии недоступны в окружении приёмки (Windows,
  `cargo-fuzz` не линкуется; Linux/macOS nightly-хост отсутствует). В
  `docs/fuzz-protocol.md` зафиксирован явный **waiver** с областью и владельцем;
  CI продолжает выполнять smoke (60 c) и nightly (1 ч) для четырёх таргетов,
  включая `fuzz_wml`.
- **M9.** Обоснование исключений clippy: доминирующие `Block`/`Inline` —
  сознательно плоские (ADR-0004); размер проверяется тестом
  `tests/misc.rs::model_variant_sizes_are_bounded` (`Block` < 2048 Б,
  `Inline` < 1024 Б). `cast_*` сужены до разбора числовых атрибутов, где значения
  валидируются. Решение зафиксировано в ADR-0004 и здесь.

## 5. Критерии приёмки ТЗ §13.1

| № | Критерий | Статус |
|---|---|---|
| 1 | Обязательные элементы WML Strict | ✅ |
| 2 | Покрытие опциональных ≥ 90% | ✅ 97.3% (инвентарь с зафиксированным источником) |
| 3 | Корпус без паник | ✅ (в т.ч. повреждённые файлы) |
| 4 | `SupportModel` для каждого документа | ✅ |
| 5 | Таблицы/списки/изображения в DOM | ✅ (6 golden) |
| 6 | Стили и нумерация | ✅ |
| 7 | Локации | ✅ |
| 8 | Покрытие `wml` ≥ 80% | ✅ **90.54%** |
| 9 | fmt/clippy/test/doc; 3 ОС | ✅ локально; CI на 3 ОС |
| 10 | Документация/примеры | ✅ |
| 11 | Fuzz подключён; 24 ч | ✅ подключён; 24 ч — waiver (M8) |

## 6. Как проверить

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo test  --workspace --all-features
cargo doc   --workspace --no-deps
cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90
cargo run -p xtool -- corpus-elements
cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-wml  --all-features --fail-under-lines 80
```
