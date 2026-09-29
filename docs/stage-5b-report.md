# Этап 5 — отчёт о выполнении (эшелон 5B: DrawingML и границы страниц)

**Задача:** `STAGE-5B-TASK.md` (ZAKAZ-STAGE-5B)  
**Связь с ТЗ:** §3.3, §5.1–§5.4, §6, §7, §14, §15 («Этап 5»), §17; ADR-0004/0006  
**Окружение:** Windows x86_64 (MSVC), Rust 1.91.x, cargo-llvm-cov, cargo-deny,
WPS Office `12.1.0.28485`  
**Крейты:** `strict-ooxml-wml` (модель/парсинг), `strict-ooxml-render-svg`
(вёрстка/SVG), `strict-ooxml-report`, `strict-ooxml-cli`, `strict-ooxml`,
`xtool`  
**Статус:** реализовано; гейты и тесты — зелёные. Ряд возможностей —
намеренно `Partial` (см. §5). Документ сдан заказчику на приёмку.

---

## 1. Что входит (S5B.1–S5B.13)

| ID | Работа | Статус |
|---|---|---|
| S5B.1 | Модель: `AnchorDrawing`, `Position`, `Wrap`, `Shape`, `GroupShape`, `TextBox`, `PageBorders`; `Picture` + `srcRect`/`xfrm` | готово |
| S5B.2 | Парсинг `wp:anchor` (`positionH/V`, `wrap*`, `dist*`, `behindDoc`/`relativeHeight`, `allowOverlap`/`layoutInCell`/`locked`, `effectExtent`) | готово |
| S5B.3 | Парсинг фигур (`wps:wsp`/`spPr`/`prstGeom`/`custGeom` подмножество), групп (`wpg:wgp`, `chOff/chExt`), `txbxContent` | готово |
| S5B.4 | Позиционирование якорей (`relativeFrom`, `align`/`posOffset`, EMU→px) | готово |
| S5B.5 | Обтекание: `wrapNone`, `wrapTopAndBottom` (реальный сдвиг потока); `wrapSquare/Tight/Through` — оверлеем (ограничение) | частично |
| S5B.6 | Рендер фигур: пресеты, заливки (solid/gradient/pattern/none, theme-цвета), линии (`prstDash`, толщина, цвет) | готово (head/tail — `Partial`) |
| S5B.7 | Группы (композиция `off/ext/chOff/chExt`, масштаб), текстовые поля (`w:txbxContent` как блоки, вертикальный анкор) | готово |
| S5B.8 | Картинки-якоря (`srcRect`/`rot`/`flip`), charts/SmartArt — позиция + placeholder | частично (кроп не растеризуется) |
| S5B.9 | Границы страниц (`w:pgBorders`: стороны, `offsetFrom`, `space`, `val`, `sz`, цвет/theme, `zOrder`) | готово |
| S5B.10 | Z-order/порядок отрисовки (`behindDoc` + `relativeHeight`), лимиты/детерминизм | готово |
| S5B.11 | Отчётность/карта покрытия 5B (`coverage/stage5-scenarios.toml`) | готово: 92.4% |
| S5B.12 | Корпус: `strict-stage5b.docx` (`xtool gen-docx --stage5b`) + независимый XML-оракул + in-memory рендер-тесты; `strict-profile` как сквозной кейс | готово |
| S5B.13 | Документация/ADR, отчёт эшелона | готово |

---

## 2. Решения по открытым вопросам (§9)

| № | Вопрос | Принятое решение |
|---|---|---|
| 1 | Подмножество `a:prstGeom` | `rect`, `roundRect`, `ellipse`/`oval`, `line`, `triangle`/`isosTriangle`, `rtTriangle`, `diamond`, `pentagon`, `hexagon`, `chevron`, `right/left/up/downArrow`, `star5`, `plus`/`mathPlus`, `flowChartProcess/Decision/Terminator/Document`. |
| 2 | `a:custGeom` | Подмножество `moveTo/lnTo/cubicBezTo/close` (масштаб path→ext); фиксируется `Partial`. |
| 3 | `wrapTight`/`wrapThrough` | По ограничивающей рамке; полный контур обтекания — вне 5B (`Partial`). |
| 4 | Charts/SmartArt | Позиция + placeholder extent; растр — вне 5B. |
| 5 | Порядок отрисовки | фон → `behindDoc`-якоря (по `relativeHeight`) → тело → над-текстовые якоря → колонтитулы. |
| 6 | EMF/WMF | Placeholder (как в 5A). |
| 7 | A-1/A-2 эшелона 5A | За заказчиком (не закрываются в 5B); `strict-profile` теперь рендерит текст поля. |
| 8 | `xtool gen-docx --stage5b` | Фикстура: якорная фигура, группа из 2 фигур, текстовое поле, якорная картинка, границы страниц. |

---

## 3. Архитектура

```
strict-ooxml-wml/
├── src/lib.rs                 # + WORD_PROCESSING_SHAPE/GROUP/C..., MS_* namespace consts
├── src/model/drawing.rs       # AnchorDrawing/Position/Wrap/Shape/GroupShape/TextBox/
│                              #   Graphic/ShapeFill/ShapeStroke/ShapeGeometry/PathCommand/
│                              #   SrcRect/Xfrm/Picture; MediaIndex
├── src/model/props.rs         # + PageBorders/PageBorder/BorderOffsetFrom/BorderZOrder
├── src/parse/drawing.rs       # полный разбор wp:anchor, wps:wsp, wpg:wgp, txbxContent
└── src/parse/props.rs         # + parse_page_borders

strict-ooxml-render-svg/
├── src/layout/floating.rs     # PendingAnchor, resolve(позиция/z-order), reserves_vertical_space
├── src/layout/pageborders.rs  # рендер w:pgBorders
├── src/layout/paragraph.rs    # Seg::Anchor → ParagraphFlow.anchors; inline-shape как Flow::Block
├── src/layout/paginate.rs     # запись якорей по страницам; порядок floating
├── src/paint/graphics.rs      # фигуры/группы/текстовые поля/картинки → Item
├── src/paint/shapes.rs        # emission <path>
└── src/lib.rs                 # RenderOptions.floating + builder
```

Ключевые решения:

1. **Модель аддитивна** (ADR-0004): `DrawingKind::Anchor` заменяет `AnchorStub`
   полноценной `AnchorDrawing`; `InlineDrawing.graphic` — `Box<Graphic>`
   (вариант остаётся мал; ограничение размера переиспользует allowance M9).
2. **Strict-first с совместимостью.** Разбор идёт в Strict-пространствах; для
   реальных «Strict» файлов, использующих Microsoft-пространства
   (`.../word/2010/wordprocessingShape`, `.../wordprocessingGroup`,
   `wne:txbxContent`, `wp:wsp` в wordprocessingDrawing), добавлен
   совместимый разбор по локальному имени с явной фиксацией `Partial`.
3. **Floating-слой.** Якоря собираются в `ParagraphFlow.anchors`, пагинатор
   привязывает их к странице хост-абзаца, `layout::floating::resolve` считает
   координаты (page/margin/column/paragraph/line + `align`/`posOffset`) и
   вставляет items: `behindDoc` — в начало страницы, остальные — в конец (до
   колонтитулов).
4. **Обтекание.** `wrapNone` — оверлей; `wrapTopAndBottom` резервирует высоту
   (реально влияет на перенос); `wrapSquare/Tight/Through` — оверлей по
   ограничивающей рамке (`Partial`).
5. **Геометрия.** EMU→px через `units::emu_to_px`; `wps:bodyPr` insets — в EMU
   (как в DrawingML), а не в twips.
6. **Детерминизм.** Все координаты конечны, порядок fixed; `--no-floating`
   отключает слой целиком.

---

## 4. Результаты команд

| Проверка | Результат |
|---|---|
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test --workspace --all-features` | ✅ 63 test-бинарника, 0 падений |
| `cargo deny check` | ✅ (зависимости не менялись) |
| `xtool coverage --file coverage/stage5-scenarios.toml --min 85` | ✅ **92.4%** |
| SSIM-гейт (5A/Этап 4) | ✅ без регрессий (`strict-text` 0.9768, `strict-text-grid` 0.9709, `strict-stage5` 0.9795) |
| `strict_profile_renders_anchored_text_box` | ✅ текст поля + позиция якоря |

Сквозные тесты 5B:

```text
cargo test -p strict-ooxml-render-svg --all-features --test floating
cargo test -p strict-ooxml-render-svg --all-features --test strict_profile
cargo test -p strict-ooxml-wml --all-features --test drawing
cargo test -p strict-ooxml-wml --all-features --test stage5b_fixture_oracle
cargo test -p strict-ooxml --all-features --test stage5b_corpus
cargo run -p strict-ooxml-cli -- render <f.docx> --out out/ [--no-floating]
cargo run -p xtool -- gen-docx --stage5b --out strict-ooxml-core/tests/strict/strict-stage5b.docx
```

---

## 5. Ограничения (честно)

- `wrapSquare`/`wrapTight`/`wrapThrough` не «расступают» текст: объект
  рисуется оверлеем по bounding box (позиция/размер верны, обтекание — нет).
  `wrapNone` и `wrapTopAndBottom` влияют на поток.
- Отрисовка `a:headEnd`/`a:tailEnd` (наконечники линий) и поворот/отражение
  **групп** не применяются (парсятся).
- `a:srcRect` картинок разбирается, но не вырезается при растеризации;
  `rot`/`flip` картинок применяются через SVG `transform`.
- Полный `a:custGeom`, градиенты (используется первый стоп), паттерны
  (foreground), 3D/анимации, OLE/ActiveX — вне 5B.
- Charts/SmartArt — только позиция + placeholder.
- **WPS-эталон 5B неприменим к чисто-Strict `wps`-разметке:** WPS
  `12.1.0.28485` не отрисовывает `wordprocessingShape` в ISO-Strict
  пространстве (проверено: в эталоне видны только границы страницы, без
  фигур/групп/картинок), поэтому попиксельный SSIM на `strict-stage5b.docx`
  был бы ложным. Реальный Strict `strict-profile.docx` (Microsoft-пространства)
  используется как сквозной кейс `tests/strict_profile.rs`. Полноценный
  WPS-оракул 5B требует фикстуры в Microsoft-пространствах либо обновления
  WPS — решение за заказчиком.
- Структурный инвариант (`tests/ssim.rs`) для `strict-stage5b` включён как
  page-count + валидность SVG сквозным тестом, без SSIM.

---

## 6. Карта покрытия (S5B.11)

`coverage/stage5-scenarios.toml` дополнен секцией 5B (14 записей):
`drawingml.anchors/positioning/zorder/shapes/fills/groups/textboxes` —
`supported`; `anchor_wrap/pictures_anchored/shape_outline/custgeom/
charts_smartart/namespace_compat` — `partial`; `page.borders` — `supported`.
Факт: **92.4%** (`supported 47, partial 14, unsupported 5, ignored 4`).

`SupportModel` фиксирует новые механизмы: `wp:anchor`, `wps:wsp`, `wpg:wgp`,
`w:txbxContent`, `w:pgBorders` — `Supported`; `a:custGeom`, Microsoft-пространства
и ненативные цветовые пространства — `Partial`. Неподдержанные fill/эффекты не
теряются молча.

---

**Конец отчёта.**
