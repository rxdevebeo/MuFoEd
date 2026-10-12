# Заказ на разработку. Этап 4 — Базовый рендеринг в SVG

**Шифр:** ZAKAZ-STAGE-4 (TASK-STAGE-4)  
**Связь с ТЗ:** §3.3 (матрица покрытия), §5.1–§5.4 (крейты/features), §6 (API `RenderOptions`), §7 (DOM), §14 (НФТ), §15 «Этап 4», §17 (тестирование)  
**Основание:** Этапы 1–3 приняты. Вход — разрешённый DOM Этапа 2 (`strict-ooxml-wml::model::Document`) и медиа-индекс.  
**Новый крейт:** `strict-ooxml-render-svg` + интеграция в `strict-ooxml` (meta, feature `svg`) и CLI  
**Оценка:** 4–6 месяцев (≈ 640–960 человеко-часов)  
**Статус:** К реализации  

---

## 1. Цель этапа

Построить базовый рендеринг Strict-документа в **SVG**: постраничная вёрстка
(разбиение на страницы) с поддержкой текста, абзацев, базовых стилей, таблиц и
растровых изображений; детерминированный вывод и интеграция в публичный API и
CLI. Рендеринг — вторая половина продуктовой ценности (визуализация).

Этап **не** нормализует Transitional (Этап 6) и не поддерживает расширенную
графику/сноски/поля (Этап 5). Вход — корректный Strict-`Document`.

---

## 2. Scope

### 2.1. Входит
- Крейт `strict-ooxml-render-svg`: вёрстка + SVG-backend.
- **Разбиение на страницы** по `sectPr` (размер/поля/ориентация/колонки — базово).
- **Текст**: строки/абзацы, интервалы, отступы, выравнивание (`start/end/center/both`),
  табуляции, `line`/`lineRule`, размеры, полужирный/курсив/подчёркивание/зачёркивание,
  цвет, `vertAlign` (базово), `xml:space`.
- **Базовые стили**: применение каскада `docDefaults → style (basedOn) → pPr/rPr`
  к конкретным абзацам/runs (вычисленные свойства).
- **Таблицы**: сетка (`tblGrid`), строки/ячейки, границы, заливка, `gridSpan`/
  `vMerge` (базово), ширина/выравнивание.
- **Изображения (raster)**: inline `DrawingML` → `a:blip` → медиа-часть;
  вставка как `<image>` (data URI или файл — по `RenderOptions`).
- **SVG-backend**: страница как отдельный документ с `viewBox`, детерминированный
  вывод (стабильная сортировка/форматирование чисел).
- Публичный API `RenderOptions`/`Page`/`render_*`; интеграция в meta (`svg`).
- CLI-команда `render` (SVG по страницам/файлам/в каталог).
- Тесты (структурные/детерминизм/SVG-валидность), визуальное сравнение (SSIM),
  бенчмарки, независимый оракул рендеринга.

### 2.2. Не входит
- Нормализация Transitional (Этап 6).
- Сноски/концевые сноски, колонтитулы (содержимое), поля (fields), сложные
  таблицы, темы, DrawingML-фигуры/группы/якоря (`wp:anchor`), диаграммы,
  MathML, VML/OLE (Этап 5 / вне scope).
- PNG/PDF-вывод (ТЗ §3.3, опционально после MVP) — только SVG.
- Редактирование.

---

## 3. Входы (интерфейсы Этапов 1–2)

| Источник | Что даёт |
|---|---|
| `strict_ooxml_wml::model::Document` | `body`, `sections` (`SectionProperties`), `styles` (`StyleTable` с `based_on_chain`), `numbering`, `settings` (`default_tab_stop`), `media` |
| `model::block::{Block, Paragraph, Table, TableRow, TableCell}` | блоки и таблицы |
| `model::inline::{Inline, Run, RunContent}` | runs/текст/break/tab/drawing/hyperlink |
| `model::props::{ParagraphProperties, RunProperties, SectionProperties, PageSize, PageMargins, Columns}` | свойства |
| `model::values::*` | единицы (`Twips`, `HalfPoints`, `EighthPoints`, `Emu`), enums (`Justification`, `Underline`, …) |
| `model::drawing::{Drawing, DrawingKind, BlipRef}` + `MediaIndex` | изображения |
| `strict_ooxml_core::part::PartId`, `Package::read_part` | байты медиа |
| `strict_ooxml_core::error::{Result, StrictError}` | ошибки |

**Пробел входа (решение нужно):** DOM хранит стили и ссылки, но **эффективные
(вычисленные) свойства** абзаца/run собираются каскадом. Где считать каскад —
в `wml` (новый `resolve`-проход `computed`) или в render-крейте — ADR-0006 (§9).

---

## 4. Архитектура крейта

```
strict-ooxml-render-svg/
├── src/
│   ├── lib.rs              # публичный API: render_svg, RenderOptions, Page
│   ├── units.rs            # twips/half-point/emu/px/pt ↔ f64, округление
│   ├── style.rs            # каскад стилей → Computed{P,R}unProps
│   ├── font/
│   │   ├── mod.rs          # trait FontProvider { metrics(font) -> &FontMetrics }
│   │   ├── metrics.rs      # advance widths/ascender/descender, sTypoMetrics
│   │   └── builtin.rs      # детерминированные метрики по умолчанию (бандл таблиц)
│   ├── layout/
│   │   ├── mod.rs          # line breaking, inline flow
│   │   ├── paragraph.rs    # spacing/indent/alignment/tabs/lineRule
│   │   ├── table.rs        # grid, cell layout, borders/shading, span/merge
│   │   └── paginate.rs     # секции, page size/margins, разрывы, fragmentation
│   ├── paint/
│   │   ├── mod.rs          # SVG writer (детерминированный, экранирование)
│   │   ├── text.rs         # <text>/<tspan>, шрифт/размер/цвет/decorations
│   │   ├── shapes.rs       # линии границ, заливки
│   │   └── image.rs        # <image> (data URI/файл), масштаб из extent/EMU
│   └── error.rs            # RenderError → StrictError::Render
├── tests/ benches/ + fuzz (опц.)
```

Зависимости: `strict-ooxml-core`, `strict-ooxml-wml`. Для тестов визуального
сравнения — растризатор SVG (например, `resvg`/`usvg`) и `image` (**dev-only**);
ADR-0006 и `cargo-deny`. Бандл метрик шрифта — только **данные** (публичная
лицензия), без бинарных шрифтов, если не согласовано иначе.

Meta-крейт: feature `svg` (ТЗ §5.4: default должен стать `["report", "svg"]`).

---

## 5. Ключевые подсистемы

### 5.1. Единицы и точность
- `twips → pt/px` (1 pt = 20 twips), `half-points → pt`, `eighth-point → pt`,
  `EMU → px` (914400 EMU = 1 inch). Базовая единица вывода — **px при 96 DPI**,
  фиксированный `scale`, все числа форматируются детерминированно (ограниченное
  число знаков, без `-0`/`NaN`).

### 5.2. Шрифтовые метрики (ADR-0006 — критично)
Вёрстке нужны ширины глифов. Требования:
- абстракция `FontProvider`/`FontMetrics` (advance width, ascender, descender,
  line gap, units-per-em); рендер **не** зависит от системных шрифтов;
- **детерминированный провайдер по умолчанию** (таблицы метрик для набора
  распространённых семейств + fallback), чтобы результат был воспроизводим и в CI;
- опциональный провайдер системных шрифтов — за feature, вне критериев приёмки.
Открытый вопрос §9.1.

### 5.3. Каскад стилей
- `docDefaults → rPrDefault/pPrDefault → стиль (basedOn-цепочка) → прямые pPr/rPr`
  → `ComputedParagraphProperties`/`ComputedRunProperties`.
- Наследование boolean (`b`/`i`/…), переопределение полей, `style`-ссылки.
- Решение о размещении (§3) — ADR.

### 5.4. Вёрстка абзаца
- Line breaking по метрикам; `line`/`lineRule` (`auto`/`exact`/`atLeast`),
  `before/after`, `ind` (`start/end/firstLine/hanging`), `jc`
  (`start/end/center/both`), `tabs` (схемные `w:pos`/`w:val`), `snapToGrid` (базово).
- Вертикальная позиция маркера абзаца (`rPr`).

### 5.5. Таблицы
- `tblGrid` → колонки; `tblW`/`tcW` (dxa/pct/auto), `gridSpan` (объединение по
  горизонтали), `vMerge` (continue/restart), `tcMar`, `tblBorders`/`tcBorders`,
  `shd`, `vAlign`, `tblLayout` (fixed/auto — базовая эвристика).
- Содержимое ячейки — блочная вёрстка (абзацы).
- Разрыв строки таблицы между страницами — базово (по `cantSplit`/высоте).

### 5.6. Разбиение на страницы
- Страницы из секций (`sectPr`): `pgSz` (w/h/orient), `pgMar`, `cols` (число/space;
  многоколоночный поток — базово или как отложенное, §9).
- Разрывы: `w:br w:type="page"`, `pageBreakBefore`, `sectPr` (новая секция —
  новая страница), переполнение страницы.
- Минимальное управление `keepNext`/`keepLines`/widow-orphan — базово.

### 5.7. Изображения
- `DrawingKind::Inline` → `a:blip/@r:embed` → медиа-часть; размеры из `wp:extent`
  (EMU→px) или из `a:ext`.
- Форматы: растровые (`png`, `jpeg`, `gif`). `emf`/`wmf` — **не** рендерятся
  (запись в `SupportModel`, как Unsupported); `wp:anchor` — вне scope Этапа 4.
- Встраивание: data URI (по умолчанию, самодостаточный SVG) либо вынос файлов
  (`RenderOptions`) — детерминированный выбор.

### 5.8. SVG-backend
- Одна страница = один `<svg>` (`width`/`height`/`viewBox`).
- Текст — `<text>`/`<tspan>` с `font-family`, `font-size`, `fill`, decorations;
  перенос строк — явные `tspan`/`x/y` (не полагаться на движок).
- Числа — фиксированный формат; атрибуты — стабильный порядок; экранирование.
- Полный SVG валиден по XML и (опционально) проверяется независимым парсером.

---

## 6. Публичный API и CLI

```rust
pub struct RenderOptions {
    pub scale: f32,                 // px на pt/дюйм, по умолчанию 96 DPI
    pub media: MediaMode,           // EmbedDataUri | ExternalFiles | None
    pub font_provider: FontProviderKind, // Builtin | System(feature)
    pub pages: PageSelection,       // All | Range(..)
    pub background: bool,
}

pub struct Page { pub index: usize, pub width_px: f32, pub height_px: f32, pub svg: String }

pub fn render(document: &Document, options: &RenderOptions) -> Result<Vec<Page>>;
```

Meta (`strict-ooxml`, feature `svg`):
- `StrictDocument::render_svg(&RenderOptions) -> Result<Vec<Page>>`
- `render_page_svg(index)`, `render_all_svg()` (строки).

CLI:
- `render <file.docx> [--out <dir|file>] [--pages 1-3] [--scale N]`
  (по умолчанию — в stdout одна страница или объединённо, §9).
- Коды возврата: `0` успех, `1` документ разобран, но с неподдержанными
  визуальными механизмами (по отчёту), `2` ошибка/повреждённый вход.

---

## 7. Производительность, детерминизм, безопасность

- НФТ ТЗ §14: 100-страничный документ — рендеринг **≤ 5 с** на референсном
  оборудовании (бенчмарк `criterion`).
- **Детерминизм**: одинаковый вход → побайтово одинаковый SVG (метрики
  детерминированы; нет системных шрифтов/локали; стабильные сортировки; числа с
  фиксированной точностью).
- **Безопасность**: никаких паник/`unwrap` в библиотечных путях; лимиты
  (страницы/размер SVG/медиа) из `ResourceLimits`; отсутствие `NaN`/`inf` в
  координатах; защита от патологических документов (бесконечный line-break и т.п.).
- Размер вывода: обрезка/лимит числа страниц и медиа; предупреждения — в отчёт.

---

## 8. Тестирование и независимый оракул

### 8.1. Виды
- Unit: единицы, формат чисел, метрики, line-break, каскад стилей.
- Структурные: страницы/`viewBox`, наличие элементов, отсутствие `NaN`, лимиты.
- **Детерминизм**: два прогона → идентичные байты (как в Этапе 3).
- **SVG-валидность**: независимый XML/SVG-парсер (`roxmltree`/`resvg`) успешно
  разбирает каждый вывод (dev-only).
- Golden SVG: эталонные документы (синтетические Strict).
- Визуальное сравнение: растеризация (`resvg`) + **SSIM** против утверждённых
  эталонов; критерий ≥ 95% (§11).
- Корпус: `strict-ooxml-core/tests/samples/` — Transitional, рендер отклонит
  (как в Этапе 3); используется для no-panic/переходных проверок, плюс
  синтетические Strict-документы.
- Бенчмарки: 10/100/500 страниц; медиа-тяжёлый документ.

### 8.2. Независимый оракул (обязательно; уроки Этапов 2–3)
1. **SVG-валидность независимым парсером** — не «наш writer» и не golden.
2. **Структурные инварианты** страницы: ширина/высота из `sectPr`, все координаты
   конечны и в пределах (+ допуск), число страниц ≥ 1, текст не выходит за
   `viewBox` (допуск).
3. **SSIM-эталоны** с внешним источником (утверждённый рендер), а не самосогласованный.
4. Самопроверка оракула: намеренно испорченный SVG/вёрстка — красный.

---

## 9. Открытые вопросы (решения до старта)

1. **Шрифтовые метрики** (ADR-0006): какие семейства/таблицы бандлить; fallback;
   где хранить данные (лицензия). Критерий детерминизма CI.
2. **Где считать каскад стилей** — новый `computed`-проход в `wml` или в
   render-крейте. Влияет на Stage-2 API.
3. **Эталоны SSIM**: кто и чем производит «утверждённые» эталоны (Word/
   LibreOffice export / авторизованный рендер), и как версионируются.
4. **Media по умолчанию**: data URI (self-contained, крупнее) или внешние файлы.
5. **Многоколоночный поток** (`cols`): поддержать базово или записать как
   `partial` и отложить.
6. **EMF/WMF**: подтвердить «не рендерим, фиксируем Unsupported» на Этапе 4.
7. **CLI `render`**: по умолчанию stdout одной страницы или запись набора файлов.
8. **Default features meta**: перевести на `["report", "svg"]` (ТЗ §5.4/решение Г.7).

---

## 10. Декомпозиция работ (WBS)

| ID | Работа | Артефакт | Оценка, ч |
|---|---|---|---|
| S4.1 | ADR-0006 (метрики, каскад, эталоны SSIM, media) | `docs/adr/0006-render.md` | 16 |
| S4.2 | Каркас крейта, feature `svg` в meta, CLI-скелет | `Cargo.toml`, `lib.rs` | 16 |
| S4.3 | Единицы/формат чисел/`RenderOptions`/`Page` | `units.rs`, `lib.rs` | 32 |
| S4.4 | Каскад стилей → computed props | `style.rs` | 64 |
| S4.5 | Шрифтовые метрики (builtin + trait + fallback) | `font/` | 72 |
| S4.6 | Line-break и вёрстка абзаца (spacing/ind/jc/tabs/lineRule) | `layout/paragraph.rs` | 96 |
| S4.7 | Вёрстка таблиц (grid/span/merge/borders/shd) | `layout/table.rs` | 88 |
| S4.8 | Разбиение на страницы (секции/поля/разрывы/переполнение) | `layout/paginate.rs` | 96 |
| S4.9 | SVG-writer (детерминизм, экранирование, shapes) | `paint/` | 64 |
| S4.10 | Изображения (inline raster, EMU→px, data URI/файлы) | `paint/image.rs` | 48 |
| S4.11 | Публичный API meta + CLI `render` | `strict-ooxml`, `cli` | 40 |
| S4.12 | Тесты: unit/структурные/детерминизм/SVG-валидность/golden | `tests/` | 88 |
| S4.13 | Harness SSIM + утверждённые эталоны | `tests/ssim`, `docs/` | 64 |
| S4.14 | Независимый оракул + самопроверка | `tests/oracle.rs` | 32 |
| S4.15 | Бенчмарки и профиль 100/500 страниц | `benches/` | 24 |
| S4.16 | Документация API + примеры | rustdoc, `examples/` | 24 |
| S4.17 | CI: SVG-валидность, SSIM (порог), покрытие, perf-gate | `.github/workflows/ci.yml` | 24 |
| **Итого** | | | **≈ 948** |

> P0-путь: S4.1 → S4.3 → S4.4/S4.5 → S4.6 → S4.8 → S4.9 → S4.11 → S4.13.

---

## 11. Критерии приёмки (ТЗ §15 Этап 4 + уточнения)

1. **100%** тестов базового корпуса рендерятся без ошибок.
2. **SSIM ≥ 95%** на утверждённых тестах (методика §9.3; отчёт с числами).
3. **≤ 5 с** на 100-страничный документ (бенчмарк, референс ТЗ).
4. Детерминизм: два прогона → идентичный SVG (тест).
5. Каждый вывод валиден по независимому XML/SVG-парсеру.
6. Нет паник/`NaN`/бесконечных координат на корпусе и негативных тестах; лимиты
   соблюдаются.
7. Текст/абзацы/базовые стили/таблицы/изображения представлены (golden +
   структурные тесты).
8. Независимый оракул внедрён и самопроверен.
9. `cargo fmt/clippy -D warnings/test/doc/deny` — зелёные; покрытие
   `strict-ooxml-render-svg` ≥ 80% строк; CI на Linux/macOS/Windows.
10. Публичный API документирован; примеры компилируются.

### Definition of Done
- [ ] Крейт `strict-ooxml-render-svg` реализован по §4.
- [ ] Meta feature `svg` (default `["report","svg"]`) и CLI `render` интегрированы.
- [ ] SSIM-harness и утверждённые эталоны; порог ≥ 95% в CI.
- [ ] Детерминизм/SVG-валидность/структурные инварианты — тестами.
- [ ] Оракул §8.2 + самопроверка.
- [ ] Перф-бенчмарк ≤ 5 с / 100 стр.; покрытие ≥ 80%; CI зелёный на 3 ОС.
- [ ] ADR-0006 и `docs/stage-4-report.md`; ревью.

---

## 12. Риски

| Риск | Вероятность | Влияние | Митигация |
|---|---|---|---|
| Точность вёрстки (без реальных шрифтовых метрик) | Высокая | Высокое | Бандл метрик, допуски, SSIM с внешним эталоном |
| Недостижимость SSIM 95% без Word/LibreOffice-эталонов | Высокая | Высокое | ADR-0006: зафиксировать источник эталонов и допуски; иначе пересмотр критерия с заказом |
| Производительность на больших таблицах/медиа | Средняя | Высокое | Бенчмарки, лимиты, ленивая растеризация |
| Детерминизм (шрифты/локаль/float) | Средняя | Среднее | Builtin-метрики, формат чисел, тест байтового равенства |
| Разрастание объёма (сложная вёрстка раньше срока) | Средняя | Среднее | Строгий scope MVP; сложное — Этап 5 |
| Паники на экзотике | Средняя | Высокое | `Result`, лимиты, property/fuzz-тесты |

---

## 13. Ожидаемые артефакты

- Крейт `strict-ooxml-render-svg` (units/style/font/layout/paint).
- Интеграция в `strict-ooxml` (feature `svg`) и `strict-ooxml-cli` (`render`).
- Тесты (unit/структурные/детерминизм/SVG-валидность/golden/SSIM/oracle) + примеры.
- `docs/adr/0006-render.md`, `docs/stage-4-report.md`, утверждённые эталоны SSIM.
- Обновлённый CI (SVG-валидность, SSIM-порог, покрытие, perf-gate).

---

**Конец заказа.**
