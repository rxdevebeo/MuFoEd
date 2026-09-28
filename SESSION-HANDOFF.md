# Передача сессии — StrictLib (strict-ooxml)

**Дата:** 2026-09-28  
**Назначение:** передать новому агенту/сессии текущее состояние проекта: что
сделано, что в работе, решения, конвенции и как продолжать.  
**Репозиторий:** `D:\projects\StrictLib` (git). Ветка: по умолчанию.

> ⚠️ **Рабочее дерево сейчас НЕ чистое:** доработка O1/O3 Этапа 4 реализована, но
> **не закоммичена** и **не проверена приёмкой в этой сессии** (см. §4).

---

## 1. Что это за проект

Rust-библиотека для чтения, анализа (Feature Report) и рендеринга в SVG
документов **WordprocessingML Strict** (ISO/IEC 29500-1), с осознанным отказом от
Transitional до отдельной нормализации. Полное ТЗ —
`TZ-STRICT-OOXML-RUST.md`; исходное — `base_target.md`.

## 2. Структура репозитория

| Крейт | Назначение | Этап |
|---|---|---|
| `strict-ooxml-core` | OPC/ZIP, XML, namespaces, лимиты, ошибки | 1 |
| `strict-ooxml-wml` | DOM + парсер WML Strict | 2 |
| `strict-ooxml-report` | Feature Report (модель/JSON/текст/схема) | 3 |
| `strict-ooxml-render-svg` | Вёрстка + SVG | 4 |
| `strict-ooxml` | Мета-крейт публичного API (`StrictDocument`) | 2–4 |
| `strict-ooxml-cli` | CLI: `inspect`/`check`/`report`/`render` | 1–4 |
| `xtool` | Dev-утилита: XSD-inventory, coverage-gate | 2–3 |

Документы этапов в корне: `STAGE-{1,2,3,4}-TASK.md`,
`STAGE-{1,2}-REWORK*.md`, `STAGE-3-ACCEPTANCE.md`, `STAGE-4-ACCEPTANCE.md`,
`REWORK-CORE-LIMITS.md`, `REWORK-WML-1.md`, `STAGE-4-RENDER-FIDELITY.md`.
ADR: `docs/adr/0001..0006`. Отчёты: `docs/stage-{2,3,4}-report.md`,
`docs/core-limits-audit.md`, `docs/perf-baseline.md`, `docs/fuzz-protocol.md`.

## 3. Статус этапов

| Этап | Состояние |
|---|---|
| 0. Проектирование | ТЗ + ADR есть |
| 1. Ядро OPC/XML/NS | ✅ принят; rework закрыт (`REWORK-CORE-1`) |
| 2. Модель/парсер WML | ✅ принят; rework #1/#2 закрыты (D-1/D-2/P-1/T-1, C-3) |
| 3. Отчётность (Feature Report) | ✅ принят; F1–F6 закрыты |
| 4. SVG-рендеринг | ⚠️ принят **условно**; доработка O1/O3 **в работе, не закоммичена** |
| 5. Расширенная поддержка | ⛔ не начат |
| 6. Нормализация Transitional | ⛔ не начат |
| 7. Стабилизация/релиз | ⛔ не начат |

## 4. Незакоммиченная работа (O1/O3 Этапа 4) — главное на сейчас

По `STAGE-4-RENDER-FIDELITY.md` (S4F) реализовано (рабочее дерево):

- Шрифты: добавлены `Arimo/Tinos/Cousine` рядом с `Carlito/Caladea`
  (`strict-ooxml-render-svg/assets/fonts/`), обновлён `ATTRIBUTION.md`.
- Метрики: `font/family.rs` (маппинг Calibri→Carlito, Arial→Arimo, Times→Tinos,
  Courier→Cousine, Cambria→Caladea), `font/builtin.rs` читает реальные
  advance-ширины через **`skrifa`** (в workspace-deps).
- SSIM-harness: `tests/ssim.rs` (+~350 строк), dev-deps `resvg` + `png`.
- Эталоны: `strict-ooxml-core/tests/strict/refs/{strict-profile,strict-text}/page_N.png`
  (WPS Office `12.1.0.28485`, `kwpsconvert word2photo`) + `refs/README.md`.
- Новый синтетический Strict-фикстур `tests/strict/strict-text.docx`.
- Обновлены `ADR-0006`, `docs/stage-4-report.md`, `STAGE-4-ACCEPTANCE.md`,
  golden SVG, `ci.yml`; правка `strict-ooxml-wml/src/parse/mod.rs`.

**Заявлено автором доработки (НЕ проверено в этой сессии):**
worst SSIM = **0.9768 ≥ 0.95** на `strict-text`; число страниц `strict-profile`
**2/2**; waiver O1 снят.

**Что сделать новому агенту:**
1. Прогнать гейты (см. §7) и приёмку по `STAGE-4-RENDER-FIDELITY.md` §4.
2. Независимо проверить SSIM: растеризовать наш SVG (`resvg`), сверить с
   `refs/*/page_N.png`, убедиться в пороге и инварианте страниц.
3. Проверить, что `strict-profile` 2/2 (пагинация 2↔1 исправлена) и что
   golden/CI зелёные; проверить `skrifa`/`resvg`/`png` в `cargo-deny`.
4. Если всё ок — закоммитить; иначе — заказ на доработку.

## 5. Незакрытые пункты (после O1/O3)

- **O2 (частично):** реальный Strict-корпус. Сейчас: 1 реальный Strict
  (`tests/strict/strict-profile.docx`, MIT из `kklimuk/docx-cli`) + синтетические
  Strict. Желательно **≥3** реальных Strict. AGPL-файл (`Esword618/unioffice`)
  — только локально, **не коммитить**.
- **Fuzz 24h** — waiver (нет Linux-nightly хоста); CI даёт smoke/nightly.
- **Этапы 5, 6, 7** — не начаты (см. ТЗ §15).
- Прочие Strict-источники: WPS/LibreOffice/Word Strict не сохраняют (кроме Word
  «Strict Open XML Document»); поиск по GitHub дал 2 файла.

## 6. Ключевые решения (ADR)

| ADR | Решение |
|---|---|
| 0001 | Собственный ZIP-ридер поверх `miniz_oxide` |
| 0002 | `quick-xml` + свой слой namespaces/лимитов |
| 0003 | Owned XML-события, `XmlReader` владеет буфером |
| 0004 | Типизированный DOM, табличный dispatch, интернирование, 2 фазы |
| 0005 | Feature Report: `serde`/`serde_json`, схема — источник истины; отчёт — **problem/attention view** (F1a); `jsonschema` только dev |
| 0006 | Рендеринг: детерминированные метрики (бандл OFL-шрифтов), каскад в render-крейте, `MediaSource`; SSIM — внешние эталоны |

Сквозные политики: **без `unwrap`/паник** в библиотечных путях; детерминизм
вывода; **лимиты ресурсов** обязательны; **неподдержанное не терять молча**
(в `SupportModel`).

## 7. Как собирать/проверять

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo test  --workspace --all-features
cargo doc   --workspace --no-deps
cargo deny  check

# покрытие (порог 80%)
cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-wml  --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-report --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-render-svg --all-features --fail-under-lines 80

# гейт опциональных элементов
cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90

# CLI
cargo run -p strict-ooxml-cli -- inspect <f.docx>
cargo run -p strict-ooxml-cli -- check   <f.docx>        # 0 / 1 / 2
cargo run -p strict-ooxml-cli -- report  <f.docx> --text
cargo run -p strict-ooxml-cli -- render  <f.docx> --out out/ --pages 1-2
```

Golden обновлять только намеренно: `UPDATE_GOLDEN=1 cargo test -p <crate> --test golden`.

## 8. Окружение и инструменты

- ОС: Windows x86_64 (MSVC); Rust 1.91.x; MSRV проекта 1.75.
- Есть: `cargo`, `cargo-llvm-cov`, `cargo-deny`, `gh` (авторизован), Python
  (`jsonschema`), **WPS Office `12.1.0.28485`** (`kwpsconvert.exe`).
- Нет: LibreOffice, Microsoft Word.
- WPS headless: `kwpsconvert.exe word2pdf|word2photo --input <f> --output <o>`
  (прямой `wpscli.exe` падает на проверке подписи).
- Fuzz: `cargo +nightly fuzz` — на Windows не линкуется (см. `docs/fuzz-protocol.md`).

## 9. Конвенции и уроки (важно!)

1. **Приёмка ≠ зелёные тесты.** Проверять код против схемы/реальных файлов
   независимым оракулом. Уроки: D-1/D-2 (`w:tab`, дробные измерения) и C-3
   (пробел в прологе) были скрыты **самосогласованными фикстурами**.
2. Каждый этап/доработка оформляется **заказом** (требования + критерии
   приёмки) и **приёмочным документом**; код в заказах не приводится.
3. **Independent oracle** обязателен: внешний ZIP (`zip`), внешний XML
   (`roxmltree`), внешняя JSON-схема (`jsonschema`), WPS-эталоны для рендера.
4. **Строгость namespace:** Strict (`purl.oclc.org/ooxml`) vs Transitional
   (`schemas.openxmlformats.org`). Корневые парсеры WML принимают только Strict
   namespace при conformance=Strict; для Unknown — по локальному имени.
5. **Лицензии:** сторонние фикстуры/шрифты — только совместимые (MIT/Apache-2.0/
   OFL); AGPL — не коммитить. Шрифты — метрик-совместимые открытые аналоги.

## 10. Риски/подводные камни

- Strict-файлов в природе мало — корпус тонкий (TZ-риск «Мало Strict-файлов»).
- WPS/шрифты пиннятся по версии; эталоны генерируются один раз (в CI WPS нет).
- SSIM зависит от шрифтов/D PI: растеризатор должен получать бандл-шрифты.
- Пагинация (2↔1) — проверять на каждом изменении layout.
- `max_compression_ratio` не занижать (легитимные docx дают ~228:1).

## 11. Индекс: где что искать

| Вопрос | Документ |
|---|---|
| Полное ТЗ | `TZ-STRICT-OOXML-RUST.md` |
| Текущие заказы | `REWORK-CORE-LIMITS.md`, `REWORK-WML-1.md`, `STAGE-4-RENDER-FIDELITY.md` |
| Приёмки | `STAGE-3-ACCEPTANCE.md`, `STAGE-4-ACCEPTANCE.md` |
| Отчёты этапов | `docs/stage-{2,3,4}-report.md`, `docs/core-limits-audit.md` |
| Решения | `docs/adr/0001..0006` |
| Корпуса | `strict-ooxml-core/tests/samples/` (Transitional), `strict-ooxml-core/tests/strict/` (реальные Strict + refs), `tests/docx/` (локальный, gitignored) |
| Fuzz | `docs/fuzz-protocol.md`, `fuzz/fuzz_targets/` |

## 12. Рекомендуемый следующий шаг

1. **Закрыть O1/O3:** проверить незакоммиченную доработку (§4), принять и
   закоммитить. Обновить `STAGE-4-ACCEPTANCE.md`/отчёт, снять waiver.
2. Затем — по ТЗ: **Этап 5** (сноски/колонтитулы/поля/сложные таблицы/темы/
   DrawingML-фигуры), **Этап 6** (нормализация Transitional), **Этап 7** (релиз).
3. По возможности — добор реальных Strict-фикстур (≥3).

---

**Конец документа.**
