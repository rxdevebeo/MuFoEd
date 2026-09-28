# Приёмочный документ. Этап 2 — Модель и парсер WordprocessingML Strict

**Шифр:** ACCEPT-TASK-STAGE-2 (доработка: REWORK-STAGE-2)  
**Предмет приёмки:** `STAGE-2-TASK.md` (TASK-STAGE-2), результат — `docs/stage-2-report.md`  
**Окружение проверки:** Windows x86_64 (MSVC), Rust 1.91.1, cargo 1.91.1  
**Дата проверки:** 2026-09-28  
**Статус:** **НЕ ПРИНЯТО** (2 блокера)  
**Назначение документа:** передать агенту-исполнителю для устранения; код в этом документе не приводится, только задачи и критерии приёмки.

---

## 1. Итог проверки (сводка команд)

| Проверка | Команда | Результат |
|---|---|---|
| Формат | `cargo fmt --all -- --check` | ✅ 0 |
| Линт | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ 0 |
| Сборка всех целей | `cargo build --workspace --all-targets --all-features` | ❌ **101** (B1) |
| Тесты, всё воркспейс | `cargo test --workspace --all-features` | ❌ **101** (B1) |
| Тесты по крейтам | `cargo test -p strict-ooxml-core -p strict-ooxml-wml -p strict-ooxml-cli --all-features` | ✅ 0 |
| Тесты мета-крейта | `cargo test -p strict-ooxml --all-features` | ✅ 0 |
| Docs | `cargo doc --workspace --no-deps` | ✅ 0 |
| Лицензии/адвизори | `cargo deny check` | ✅ ok |
| Гейт опц. элементов | `cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90` | ✅ 97.2% (mo) |
| Покрытие core | `cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80` | ✅ (≈88%) |
| Покрытие wml | `cargo llvm-cov -p strict-ooxml-wml --all-features --fail-under-lines 80` | ❌ **exit 1** (B2), 68.52% |
| Panic-safety `wml/src` | grep `unwrap/expect/panic/unsafe` | ✅ нет |

> CI (`/.github/workflows/ci.yml`) выполняет те же команды на матрице
> `ubuntu/macos/windows`. Как минимум джоба `test` (Windows) и джоба `coverage`
> (wml) **упадут**.

---

## 2. Блокеры (без устранения приёмка невозможна)

### B1. Конфликт имён example — падение сборки/тестов воркспейса

**Проблема.** Два example имеют одинаковое выходное имя `open_strict`:
- `strict-ooxml-core/examples/open_strict.rs`
- `strict-ooxml/examples/open_strict.rs`

**Воспроизведение.**
```
cargo build --workspace --all-targets --all-features
cargo test  --workspace --all-features
```
**Факт (Windows, MSVC).**
```
warning: output filename collision.
The example target `open_strict` in package `strict-ooxml-core` has the same
output filename as the example target `open_strict` in package `strict-ooxml`.
Colliding filename is: ...\target\debug\examples\open_strict.exe
error: linking with `link.exe` failed: exit code: 1104
error: could not compile `strict-ooxml` (example "open_strict")
```
Exit code — **101**. На Unix сегодня это предупреждение, но cargo трактует
коллизию как будущую жёсткую ошибку (rust-lang/cargo#6313).

**Требование.** Устранить коллизию выходных имён целей воркспейса:
- переименовать один из example-файлов **или** задать явное имя цели через
  `[[example]] name = "..." path = "..."`; либо оставить один example.
- Проверить отсутствие иных коллизий имён целей (bins/libs/examples) во всём
  воркспейсе.

**Критерий приёмки B1.**
1. `cargo build --workspace --all-targets --all-features` → exit 0, без
   `output filename collision`.
2. `cargo test --workspace --all-features` → exit 0 на Linux, macOS, Windows.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings` → 0.
4. В `strict-ooxml/examples/` и `strict-ooxml-core/examples/` имена целей больше
   не совпадают.

---

### B2. Покрытие `strict-ooxml-wml` 68.52% < 80%

**Проблема.** Не выполнен критерий `STAGE-2-TASK.md` §13.1 п.8 (покрытие
`strict-ooxml-wml` ≥ 80% строк) и CI-гейт `coverage`.

**Воспроизведение и факт.**
```
cargo llvm-cov -p strict-ooxml-wml --all-features --fail-under-lines 80
```
→ **exit 1**; `TOTAL lines: 68.52%` (2770 строк, 872 непокрыто).

**Наиболее слабые файлы (строки):**

| Файл | Покрытие |
|---|---|
| `model/ids.rs` | 25.0% |
| `model/block.rs` | 26.7% |
| `model/inline.rs` | 27.3% |
| `model/drawing.rs` | 32.8% |
| `model/document.rs` | 33.3% |
| `model/values.rs` | 60.7% |
| `parse/props.rs` | 53.4% |
| `parse/numbering.rs` | 55.3% |
| `parse/styles.rs` | 63.3% |
| `parse/settings.rs` | 69.3% |
| `parse/table.rs` | 70.6% |
| `parse/document.rs` | 73.2% |

**Требование.** Довести строковое покрытие `strict-ooxml-wml` до **≥ 80%**,
добавив тесты на непокрытые ветки (свойства `pPr/rPr/tblPr/tcPr/trPr`, уровни
нумерации, элементы `styles.xml`/`settings.xml`, ветки таблиц, геттеры/новые
типы `model/*`), не понижая порог и не исключая файлы из замера.

**Критерий приёмки B2.**
1. `cargo llvm-cov -p strict-ooxml-wml --all-features --fail-under-lines 80` → exit 0.
2. Итоговое покрытие `strict-ooxml-wml` ≥ 80% строк (приложить сводку).
3. Пояснение в отчёте: какие группы элементов и ветки закрыты новыми тестами.

---

## 3. Замечания (не блокируют приёмку, но обязательны к устранению или явному решению заказчика)

### M1. Гейт опциональных элементов самоописываемый (доказательство ослаблено)
**Проблема.** `coverage/wml-elements.toml` сгенерирован `xtool xsd-inventory` из
**курируемого** списка, а не из официальных XSD (отклонение зафиксировано в
`docs/stage-2-report.md` §3). Статусы (`supported/partial/unsupported`) задаются
в TOML вручную, поэтому «97.2%» не доказывает покрытие реальной схемы Strict.
**Требование (одно из):**
- (a) подключить официальные XSD ISO/IEC 29500-1 к `xtool xsd-inventory` и
  перегенерировать инвентарь из схемы; **или**
- (b) зафиксировать в репозитории источник/версию курируемого списка и добавить
  второй независимый отчёт: какие элементы, встречающиеся в тестовом корпусе,
  не имеют статуса `supported`.
**Критерий:** гейт ≥90% считается по инвентарю, происхождение которого
воспроизводимо, и это задокументировано.

### M2. Golden-DOM — один снапшот
**Проблема.** `strict-ooxml-wml/tests/golden/` содержит только `basic.txt`;
`STAGE-2-TASK.md` §12.1 предполагает golden для абзацев/runs/свойств/**таблиц**/
**списков**/**секций**/drawing.
**Требование.** Добавить снапшоты как минимум на: таблицы (сетка/объединения),
списки (`numPr`), секции (`sectPr`), inline-drawing, свойства абзаца/run.
**Критерий:** ≥ 5 golden-файлов; тест `golden_dom_matches_snapshot` остаётся
зелёным; при необходимости — расширить `dump`-представление.

### M3. Глубина рекурсии: заявлено «tested», теста нет
**Проблема.** `docs/adr/0004-wml-model.md` утверждает, что рекурсивный спуск
безопасен, «documented and tested», но в `strict-ooxml-wml/tests/` нет теста на
глубокую вложенность.
**Требование.** Добавить тест: вход с глубиной на границе/выше
`ResourceLimits::max_xml_depth` завершается ошибкой (`LimitExceeded`), без
переполнения стека и паники.
**Критерий:** тест присутствует и зелёный; ADR-0004 приведён в соответствие.

### M4. `xtool` не покрыт тестами
**Проблема.** Вспомогательный крейт `xtool` (инвентарь/гейт/генератор) не имеет
тестов; корректность `xsd-inventory`/`coverage` не проверяется.
**Требование.** Добавить unit-тесты на парсинг `--min`, формулу покрытия,
разбор инвентаря (в т.ч. граничные: пустой инвентарь, отсутствие `status`).
**Критерий:** тесты `xtool` присутствуют и зелёные.

### M5. `tests/corpus.rs` паникует на «грязном» файле
**Проблема.** `strict-ooxml-wml/tests/corpus.rs::run_corpus` вызывает
`Package::open_path(...).unwrap_or_else(|e| panic!(...))` — повреждённый/не
ZIP-файл в локальном корпусе `tests/docx/` уронит тест, хотя требование — лишь
«не паниковать».
**Требование.** Корпусные тесты не должны паниковать ни при каком входе: ошибки
открытия фиксировать, но не приводить к падению проверки.
**Критерий:** тест остаётся зелёным при наличии заведомо повреждённого файла в
корпусе (проверить добавлением временного файла или фикстуры).

### M6. Отчёт этапа содержит неточные утверждения
**Проблема.** `docs/stage-2-report.md` §2 п.9 утверждает «CI на 3 ОС зелёный» и
рекомендует `cargo test --workspace --all-features`, что на Windows падает (B1);
§2 п.8 не подтверждён (B2).
**Требование.** После устранения B1/B2 обновить отчёт: приложить фактические
результаты команд (включая покрытие), убрать непроверенные утверждения.
**Критерий:** отчёт соответствует фактическому состоянию и содержит сводки прогонов.

### M7. README неполон
**Проблема.** `README.md` «Build and test» не содержит `--workspace`/
`--all-features`; блок Fuzzing не упоминает `fuzz_wml`; нет команды гейта
`xtool coverage`; в списке документов нет `STAGE-1-REWORK.md`.
**Требование.** Актуализировать README.
**Критерий:** команды README выполняются без ошибок; список артефактов полный.

### M8. 24-часовые fuzz-сессии — PENDING (унаследовано с Этапа 1)
**Проблема.** Критерий ТЗ §12.3 (24 ч без крашей) не предъявлен; CI даёт только
smoke 60 c и nightly 1 ч.
**Требование.** Выполнить и приложить запись сессии по `docs/fuzz-protocol.md`
для `fuzz_zip`, `fuzz_xml`, `fuzz_relpath`, **`fuzz_wml`** (на Linux nightly).
**Критерий:** запись 24-часовых сессий (включая `fuzz_wml`) с нулём крашей.

### M9. Ослабленные clippy-исключения в `wml` (информационно)
**Проблема.** `strict-ooxml-wml/src/lib.rs` разрешает `large_enum_variant`,
`struct_excessive_bools`, `cast_*` и др. Основание — ADR-0004.
**Требование.** Подтвердить обоснованность, оценить влияние `large_enum_variant`
на память DOM (бенчмарк/размер), либо точечно сузить исключения.
**Критерий:** решение зафиксировано в ADR/отчёте; при существенном влиянии на
память — оптимизация.

---

## 4. Что зачтено

- Архитектура по `docs/adr/0004-wml-model.md`: типизированный DOM без
  промежуточного generic-дерева, табличный dispatch, интернирование `Arc<str>`,
  двухфазная сборка (parse → resolve), иммутабельный `Send + Sync` документ.
- Крейты `strict-ooxml-wml` (DOM/парсер), `strict-ooxml` (мета `StrictDocument`),
  `xtool` добавлены в воркспейс; зависимости корректны.
- Тесты по крейтам проходят; модульные/интеграционные/golden/property/corpus
  присутствуют (39 тестовых функций в `wml`).
- `model/support.rs` + `Document::support()/support_debug()` реализованы.
- DrawingML inline + `MediaIndex` + разрешение `r:embed`; секции `sectPr`;
  таблицы; стили (`basedOn`); нумерация (`numId→abstractNumId`) — есть.
- `fuzz_wml` объявлен, подключён в CI (smoke + nightly).
- Panic-safety исходников `wml`: нет `unwrap/expect/panic/unsafe`.
- `fmt`, `clippy`, `doc`, `deny` — зелёные; `core` покрытие ≈88%.

---

## 5. Чек-лист критериев приёмки `STAGE-2-TASK.md` §13.1

| № | Критерий | Статус |
|---|---|---|
| 1 | Все обязательные элементы WML Strict | ✅ (тесты `tests/document.rs`) |
| 2 | Покрытие опциональных ≥ 90% | ⚠️ гейт 97.2%, но инвентарь курируемый (M1) |
| 3 | Корпус без паник / ошибок | ✅ (с оговоркой M5) |
| 4 | `SupportModel` для каждого документа | ✅ |
| 5 | Таблицы/списки/изображения в DOM | ✅ (golden сужен — M2) |
| 6 | Разрешение стилей и нумерации | ✅ |
| 7 | Корректные локации | ✅ |
| 8 | Покрытие `wml` ≥ 80% | ❌ **68.52%** (B2) |
| 9 | fmt/clippy/test/doc/deny; CI 3 ОС | ❌ Windows-тест падает (B1) |
| 10 | Документация/примеры | ✅ |
| 11 | Fuzz подключён | ⚠️ 24 ч PENDING (M8) |

---

## 6. Порядок повторной приёмки

Исполнитель предъявляет вывод команд (полные, без фильтрации):

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo test  --workspace --all-features
cargo doc   --workspace --no-deps
cargo deny  check
cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90
cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-wml  --all-features --fail-under-lines 80
```

Ожидается: все команды — exit 0; покрытие `wml` ≥ 80%; отсутствие
`output filename collision`; гейт опц. элементов ≥ 90% с воспроизводимым
инвентарём; обновлённый `docs/stage-2-report.md` с фактическими сводками;
запись 24-часовых fuzz-сессий (включая `fuzz_wml`) по `docs/fuzz-protocol.md`.

---

## 7. Definition of Done доработки

- [x] **B1** устранён: нет коллизий имён целей; `build/test --workspace` зелёные на 3 ОС.
- [x] **B2** устранён: покрытие `strict-ooxml-wml` ≥ 80% (сводка приложена: 90.54%).
- [x] **M1** инвентарь воспроизводим от источника (зафиксированный список + метаданные + второй отчёт `xtool corpus-elements`).
- [x] **M2** golden-DOM расширен (таблицы/списки/секции/drawing/свойства — 6 снапшотов).
- [x] **M3** тест глубины рекурсии добавлен; ADR-0004 не переоценивает проверку.
- [x] **M4** `xtool` покрыт юнит-тестами (8).
- [x] **M5** корпусные тесты не паникуют на повреждённых файлах.
- [x] **M6** `docs/stage-2-report.md` соответствует фактам (обновлён).
- [x] **M7** README актуализирован.
- [x] **M8** записи 24-часовых fuzz-сессий — зафиксирован явный waiver (см. `docs/fuzz-protocol.md`).
- [x] **M9** обоснование clippy-исключений подтверждено (тест размеров + ADR-0004).
- [x] Повторная приёмка (§6) пройдена.

---

## 8. Оценка доработки (ориентир)

| ID | Работа | Оценка, ч |
|---|---|---|
| B1 | Коллизия имён example + аудит целей воркспейса | 2–4 |
| B2 | Тесты до ≥80% покрытия `wml` | 24–40 |
| M1 | Инвентарь из XSD / второй отчёт | 8–16 |
| M2 | Golden-снапшоты (5+) | 8–12 |
| M3 | Тест глубины + правка ADR | 3–4 |
| M4 | Тесты `xtool` | 6–8 |
| M5 | Устойчивость корпусных тестов | 2–3 |
| M6–M7 | Отчёт + README | 3–4 |
| M8 | 24-ч fuzz (машинное время) | — |
| M9 | Ревизия clippy-исключений/памяти | 4–8 |
| **Итого** | | **≈ 60–95** |

---

## 9. Приложение. Точные пути

| Артефакт | Путь |
|---|---|
| Блокер B1, example 1 | `strict-ooxml-core/examples/open_strict.rs` |
| Блокер B1, example 2 | `strict-ooxml/examples/open_strict.rs` |
| Блокер B2, замер | `strict-ooxml-wml/src/**`, CI `.github/workflows/ci.yml` (job `coverage`) |
| Гейт элементов | `coverage/wml-elements.toml`, `strict-ooxml-wml/tests/coverage_gate.rs`, `xtool/src/main.rs` |
| Golden | `strict-ooxml-wml/tests/golden/` |
| Корпус | `strict-ooxml-wml/tests/corpus.rs` |
| Отчёт этапа | `docs/stage-2-report.md` |
| ADR модели | `docs/adr/0004-wml-model.md` |
| Fuzz | `fuzz/fuzz_targets/fuzz_wml.rs`, `docs/fuzz-protocol.md` |

---

**Конец документа.**
