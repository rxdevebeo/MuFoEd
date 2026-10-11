# Потери между парсером DOCX и рендерером

Дата: 2026-10-11. Состояние: рабочее дерево поверх `61d6756`, включая
незакоммиченные правки `strict-ooxml-render-svg` (колонки в `geometry_for`).

## Повод

Парсер читает `w:cols` в [`Columns`](../strict-ooxml-wml/src/model/props.rs:329),
а [`geometry_for`](../strict-ooxml-render-svg/src/layout/mod.rs:385) до последней
правки строил страницу без колонок. Нужно было проверить, нет ли других свойств,
которые модель хранит, а рендерер не использует.

## Метод

Поиск по имени поля для этой задачи ненадёжен: `width`, `space`, `x`, `count`
встречаются в десятках структур. Поэтому проверка сделана через компилятор.

1. Копия рабочего дерева во временный каталог (исходный репозиторий не менялся).
2. Скрипт ставит `#[deprecated(note = "AUDIT")]` на **каждое** публичное поле
   структуры и **каждый** вариант enum в `strict-ooxml-wml/src/model/*.rs`
   (1480 атрибутов; макрос `strict_enum!` пропускает атрибуты вариантов).
3. `cargo +1.92.0 check -p strict-ooxml-render-svg -p strict-ooxml-render-pdf
   --lib --all-features --message-format=json`.
4. Каждое предупреждение `deprecated` даёт точное, с учётом типов, место
   использования. Места разнесены по потребителям:
   `render-svg`/`render-pdf` (R), `wml/src/resolve` (V), парсер (P), методы самой
   модели (M). Отдельно помечено простое конструирование значения в рендерере
   (`gutter: None`) — это не чтение.
5. Элементы без R и V — кандидаты в потери. Элементы с M проверены вручную:
   вызывается ли соответствующий метод модели из рендерера.
6. Отдельно найдены поля, которые рендерер читает **только для слияния каскада**
   (`target.x = layer.x`, `x.is_some()`), но не применяет при раскладке.

Итог: из 1564 элементов модели (поля и варианты) 803 рендерер не читает ни
прямо, ни через `resolve`. Большинство из них служебные (`location`, rsid,
SDT-метаданные, настройки редактора, UI-флаги стилей) или варианты enum,
корректно покрытые веткой `_`. Ниже — только то, что влияет на картинку, плюс
частичные потери, которые компилятор сам не показывает.

### Ограничения метода

- Тесты (`#[cfg(test)]`) не учитывались: только `--lib`.
- Вариант enum, который не назван в рендерере, попадает в ветку `_` или
  `matches!`. Для двухвариантных enum это нормально (`LimitLocation`,
  `PageOrientation`), для многовариантных — означает потерю различий.
- Частичные потери (поле читается, но значение схлопывается) компилятор не
  видит. Они найдены ручным разбором мест чтения в `style.rs`, `table.rs`,
  `table_style.rs`, `notes.rs`; гарантии полноты по ним нет.
- Не проверялось, всё ли из XML парсер кладёт в модель (обратная задача).

## Статус исправлений (2026-10-11)

### Продолжение: весь оставшийся список

Объём продолжения включает все оставшиеся P2/P3 и отложенные задачи. Таблицы
ниже сохраняют исходные результаты аудита; актуальное состояние следует читать
по этому разделу. Полное закрытие документа пока не достигнуто.

- Колонки разной ширины реализованы в `c159215` (`fix/section-columns`):
  явные ширины и промежутки, повторная раскладка для ширины следующей колонки,
  балансировка непрерывных секций, сноски по колонкам.
- В текущем рабочем дереве сохраняется и рисуется `w:rPr/w:bdr`: четыре стороны
  рамки фрагмента текста через общий механизм SVG/PDF. Объединение рамок соседних
  run ещё требует реализации.
- `w:cs` включает канал complex script; при явном `cs`/`rtl` размер `szCs`
  имеет приоритет над `sz`. Полный независимый каскад каналов форматирования
  для смешанного текста остаётся открытым.
- `wp:simplePos` и процентные смещения применяются одинаково к положению
  плавающего объекта и области обтекания.
- `wp:docPr/@hidden` сохраняется в модели и при записи DOCX; скрытые inline
  и anchor объекты не попадают в текстовый поток или область обтекания.
- `w:bidi` зеркалирует отступы и `start`/`end`; отступ первой строки применяется
  с правой стороны. Направление глифов, маркеры списков и вертикальная раскладка
  остаются отдельными задачами.
- `gridAfter` учитывается при определении числа колонок; `wBefore`/`wAfter`
  участвуют в восстановлении отсутствующей сетки. Согласование с существующей
  устаревшей сеткой остаётся открытым.

Ранее пройденные проверки новых изменений: run properties (11), drawing
properties (6), paragraph properties (9), модуль позиции плавающего объекта (1).
В тесте пропущенных колонок обнаружена ошибка порядка ширин; она исправлена,
повторная проверка ещё не выполнена. После этого также изменён приоритет `szCs`.
Полный прогон SVG/PDF завершился ошибками doctest из-за несовпадения артефактов
зависимостей; успешным полным прогоном он не считается.
После добавления `docPr.hidden` прошла сборка всего workspace с
`--all-features --all-targets`; это проверка компиляции, а не поведения.

Порядок работ — из раздела «Порядок исправления» ниже. На каждую группу есть
интеграционный тест в `strict-ooxml-render-svg/tests/*_properties.rs`, плюс
модульные тесты в новых модулях.

**Новые общие механизмы**

- [`layout/decoration.rs`](../strict-ooxml-render-svg/src/layout/decoration.rs) —
  подчёркивание, зачёркивание, выделение и заливка run как геометрия, общая для SVG и
  PDF. SVG больше не использует `text-decoration`; **PDF раньше не рисовал ни
  подчёркивания, ни зачёркивания, ни выделения** — теперь рисует.
- [`layout/border.rs`](../strict-ooxml-render-svg/src/layout/border.rs) — сторона
  рамки как полоса: составные стили (`double`, `triple`, `thinThick*`), пунктиры, цвет
  темы. Используется рамками абзаца, ячеек и страницы.
- [`layout/sectionpage.rs`](../strict-ooxml-render-svg/src/layout/sectionpage.rs) —
  проход по готовым страницам: `vAlign` секции, разделитель колонок, номера строк,
  зеркальные поля.
- `ParagraphNeighbors` в [`layout/paragraph.rs`](../strict-ooxml-render-svg/src/layout/paragraph.rs)
  — соседи абзаца для `contextualSpacing`, общего блока рамки и autospacing в ячейке.

**Сделано** (по группам плана)

1. Run: тип и цвет подчёркивания, `dstrike`, `smallCaps`, `w:position`, заливка run
   с узором `w:shd` и белым `auto`-текстом на тёмном фоне, шрифт `w:sym` (с
   PUA-заменами Symbol/Wingdings), шрифты `eastAsia`/`eastAsiaTheme`/`cstheme`.
2. Абзац: линии `w:pBdr` (все стороны, `between`, общий блок для одинаковых рамок),
   `contextualSpacing`, `widowControl` (висячие и «вдовьи» строки), autospacing
   (14 pt, без отступа у края ячейки), `beforeLines`/`afterLines`.
3. Таблицы: `vAlign` ячейки (с учётом вертикального объединения), `hRule`
   (`exact`/`auto`/`atLeast`), стили линий границ, заливка таблицы и `tblPrEx`,
   границы `tblPrEx`, диагонали `tl2br`/`tr2bl`, `bidiVisual`, ширины из `tcW` при
   пустой сетке. Рамки страницы — тоже через `border.rs`.
4. DrawingML: `a:srcRect` (обрезка; поле `ImageItem::crop`, SVG и PDF), `wps:style`
   (`fillRef`/`lnRef`/`fontRef`), стрелки `headEnd`/`tailEnd`, `a:path/@fill="none"`
   и `@stroke="0"`, `wp:anchor/@hidden`, пресеты `straightConnector1`,
   `bentConnector2-5`, `curvedConnector2-5`, `arc`, `mathMinus`, `trapezoid`,
   `homePlate` и др.; линии и соединители не заливаются.
5. Страница: `gutter` (с `rtlGutter`), `mirrorMargins`, `vAlign` секции, разделитель
   колонок `w:sep`, нумерация строк `lnNumType` (`countBy`, `start`, `restart`,
   `distance`, `suppressLineNumbers`).
6. Нумерация и сноски: **нумерация через стиль абзаца** (см. ниже), `lvlJc`,
   `isLgl`, `w:ilvl` без `w:numId`, свойства сносок по секциям и
   `numRestart="eachSect"`.
7. Математика: `m:defJc` (по умолчанию `centerGroup`), `m:lMargin`/`m:rMargin`,
   `m:naryLim`/`m:intLim` для выносных формул, `m:mcPr/m:count`.

**Найдено по ходу работ** (не было в списке ниже)

- Нумерация, заданная в `w:pPr` **стиля** абзаца (заголовки, «Список»), не
  рисовалась: движок маркеров читал только прямой `w:numPr`, а каскад
  `ComputedParagraph.numbering` никто не использовал. Исправлено.
- Абзацы в ячейках таблиц теряли `line.graphics`: заливку абзаца, табуляторы `bar`,
  черты дробей и другие линии inline-формул, inline-фигуры. Исправлено в
  `place_cell_paragraph`.
- Соединители (`straightConnector1` — 768 вхождений в 13 документах корпуса) и
  `arc` рисовались серой заглушкой. Исправлено.
- Строки таблиц атомарны: пагинатор никогда не делит строку. Пункт `cantSplit`
  ниже описан неверно — фактически *все* строки ведут себя как `cantSplit`, и
  длинная строка без него переносится целиком, а не делится, как в Word.
- `wp:docPr/@hidden` парсер не читает (пробел парсера; `wp:anchor/@hidden` теперь
  учитывается).

**Отложено** (требует отдельной переработки)

- Деление строк таблицы между страницами (обратная сторона `cantSplit`).
- `tblLayout autofit` — подбор ширин колонок по содержимому; сейчас ширины всегда
  из сетки (поведение `fixed`).
- Плавающие таблицы `tblpPr`.
- Колонки разной ширины (`equalWidth="0"`): абзацы раскладываются до того, как
  известна колонка.
- Перезапуск сносок `eachPage`, положение сносок `beneathText`/`sectEnd`,
  пользовательские разделители сносок.
- Повороты и отражения групп, `bodyPr/@vert`, `@wrap="none"`, `numCol` надписей
  (в корпусе: `vert270` в 2 документах, `wrap="none"` в 8, повороты групп — 0).
- `gutterAtTop`; зеркалирование объектов, привязанных к странице.
- Рамка run (`w:bdr`), эффекты (`emboss`, `shadow`, `outline`, `w:em`), CJK-сетка
  символов, автоматические переносы, флаги `w:compat`.

## Приоритеты

- **P1** — встречается в обычных документах и сразу видно на странице.
- **P2** — заметно в отдельных классах документов (RTL, CJK, вёрстка журналов,
  формулы, фигуры).
- **P3** — редкие или мелкие визуальные эффекты.

## 1. Таблицы

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P1 | `w:tcPr/w:vAlign` | [`CellProperties.vertical_align`](../strict-ooxml-wml/src/model/props.rs:263) | Читается только для каскада в [`table_style.rs:266`](../strict-ooxml-render-svg/src/table_style.rs:266) и нигде не применяется. Содержимое ячейки всегда прижато к верху; `center`/`bottom` теряются. |
| P1 | `w:trHeight/@w:hRule` | [`RowHeight.rule`](../strict-ooxml-wml/src/model/values.rs:1263) | [`table.rs:108`](../strict-ooxml-render-svg/src/layout/table.rs:108) всегда берёт `max(контент, высота)`, то есть `atLeast`. `exact` не обрезает строку, `auto` не игнорирует значение. |
| P1 | `w:tcBorders`/`w:tblBorders` `@w:val` | [`BorderStyle`](../strict-ooxml-wml/src/model/values.rs:167) | Различаются только «нет», «пунктир/точки» и «сплошная» ([`table.rs:1463`](../strict-ooxml-render-svg/src/layout/table.rs:1463)). `double`, `triple`, `thinThick*`, `wave`, `threeDEmboss` и т.п. рисуются одной линией. Так же в [`pageborders.rs:166`](../strict-ooxml-render-svg/src/layout/pageborders.rs:166). |
| P1 | `w:tblPr/w:shd` | [`TableProperties.shading`](../strict-ooxml-wml/src/model/props.rs:194) | Не читается. Заливка на уровне таблицы теряется, работает только заливка ячеек. |
| P1 | `w:shd/@w:val`, `@w:color` | [`Shading.pattern`](../strict-ooxml-wml/src/model/values.rs:980), `color`, `theme_color` | Используется только `fill`. `solid` (цвет берётся из `w:color`), `pct10…pct90`, штриховки теряются. Касается абзацев, ячеек и run. |
| P1 | `w:trPr/w:cantSplit` | [`RowProperties.cant_split`](../strict-ooxml-wml/src/model/props.rs:226) | Не читается, но потеря обратная: пагинатор никогда не делит строку, так что все строки ведут себя как `cantSplit`, а длинная строка без него переносится целиком (уточнено при реализации, см. «Статус»). |
| P2 | `w:tblpPr` | [`TableProperties.positioning`](../strict-ooxml-wml/src/model/props.rs:206) и вся [`TablePositioning`](../strict-ooxml-wml/src/model/props.rs:381) | Плавающие таблицы ставятся в поток; обтекание не строится. |
| P2 | `w:tblLayout` | [`TableProperties.layout`](../strict-ooxml-wml/src/model/props.rs:188), [`TableLayout`](../strict-ooxml-wml/src/model/values.rs:691) | `fixed` и `autofit` не различаются. |
| P2 | `w:tcW` | `CellProperties.width` | Не читается: ширины берутся из `w:tblGrid` и `gridSpan`. Документы с устаревшей сеткой расходятся с Word. |
| P2 | `w:gridAfter`, `w:wBefore`, `w:wAfter` | `RowProperties.grid_after/width_before/width_after` | Не читаются. `w:gridBefore` читается. |
| P2 | `w:tblPrEx/w:tblBorders` | `RowProperties.exception_borders` | Не читается. Из `tblPrEx` используется только `tblCellSpacing`. |
| P2 | `w:bidiVisual` | `TableProperties.bidi_visual` | RTL-таблицы не зеркалируются. |
| P2 | `w:tblStylePr/w:tblPr`, `w:trPr` в стиле | `TableStyleCondition.table/row`, `Style.row` | Условные свойства таблицы и строки из стиля не применяются, только `tcPr`/`pPr`/`rPr`. |
| P3 | `w:tblInd/@w:type` | `TableProperties.indent_kind` | Отступ всегда трактуется как twips. |
| P3 | `w:tl2br`, `w:tr2bl` | `CellProperties.diagonal_down/up` | Диагонали ячеек не рисуются. |
| P3 | `w:noWrap`, `w:tcFitText`, `w:hideMark` | `CellProperties.no_wrap/fit_text/hide_mark` | Не читаются. |
| P3 | `w:tcPr/w:textDirection` | [`TextDirection`](../strict-ooxml-wml/src/model/values.rs:735) | Обрабатываются только `btLr` и `tbRl` ([`table.rs:447`](../strict-ooxml-render-svg/src/layout/table.rs:447)). Вертикальные варианты `*V` (CJK) идут как горизонтальные. |

## 2. Абзац

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P1 | `w:pBdr` (линии) | [`ParagraphProperties.borders`](../strict-ooxml-wml/src/model/props.rs:52) | В [`style.rs:360`](../strict-ooxml-render-svg/src/style.rs:360) из `top`/`bottom` берётся только отступ. **Линии рамки не рисуются вообще**; `left`, `right` не используются. |
| P1 | `w:pBdr/w:between`, `w:bar` | [`Borders.between`](../strict-ooxml-wml/src/model/values.rs:1026), `bar` | Не читаются. |
| P1 | `w:contextualSpacing` | [`ParagraphProperties.contextual_spacing`](../strict-ooxml-wml/src/model/props.rs:78) | Не читается: между абзацами одного стиля (списки) остаются интервалы `before/after`. |
| P1 | `w:widowControl` | `ParagraphProperties.widow_control` | Не читается: висячие строки не контролируются, разрывы страниц отличаются от Word. |
| P2 | `w:spacing/@w:beforeAutospacing`, `@w:afterAutospacing` | [`Spacing`](../strict-ooxml-wml/src/model/values.rs:1081) | Не читаются (типично для документов из HTML: Word ставит 14 pt). |
| P2 | `w:spacing/@w:beforeLines`, `@w:afterLines` | [`Spacing.before_lines`](../strict-ooxml-wml/src/model/values.rs:1091), `after_lines` | `before_lines` проверяется только на наличие в [`paragraph.rs:208`](../strict-ooxml-render-svg/src/layout/paragraph.rs:208) (математический абзац). Значения в строках сетки не применяются. |
| P2 | `w:ind/@w:startChars`… `@w:hangingChars` | `Indentation.*_chars` | Не читаются (CJK). |
| P2 | `w:bidi` | `ParagraphProperties.bidi` | RTL-абзацы не зеркалируют выравнивание и отступы. |
| P2 | `w:textDirection` абзаца | `ParagraphProperties.text_direction` | Не читается. |
| P3 | `w:snapToGrid`, `w:wordWrap`, `w:autoSpaceDE/DN`, `w:adjustRightInd`, `w:suppressOverlap` | соответствующие `ParagraphProperties.*` | Не читаются. |
| P3 | `w:jc` = `*Kashida`, `thaiDistribute` | [`Justification`](../strict-ooxml-wml/src/model/values.rs:47) | Не различаются, падают в общую ветку. |
| P3 | `w:br/@w:clear` | `BreakClear` | `left/right/all` не различаются: перенос под плавающий объект не делается. |

## 3. Run

Все свойства run проходят через [`ComputedRun`](../strict-ooxml-render-svg/src/style.rs:20).
Всё, для чего там нет поля, теряется в [`apply_run_props_mode`](../strict-ooxml-render-svg/src/style.rs:464).

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P1 | `w:u/@w:val` | [`Underline`](../strict-ooxml-wml/src/model/values.rs:73) | Схлопывается в `bool` ([`style.rs:476`](../strict-ooxml-render-svg/src/style.rs:476)). `double`, `thick`, `dotted`, `dash*`, `wave*`, `words` рисуются одной сплошной линией. |
| P1 | `w:u/@w:color`, `@w:themeColor` | `RunProperties.underline_color/underline_theme` | Не читаются: подчёркивание всегда цвета текста. |
| P1 | `w:dstrike` | `RunProperties.double_strike` | Сливается с `w:strike` ([`style.rs:479`](../strict-ooxml-render-svg/src/style.rs:479)), рисуется одной линией. |
| P1 | `w:smallCaps` | `RunProperties.small_caps` | Не читается: текст выводится строчными. |
| P1 | `w:position` | [`RunProperties.position`](../strict-ooxml-wml/src/model/props.rs:137) | Не читается: подъём/опускание базовой линии теряется. |
| P1 | `w:rPr/w:shd` | [`RunProperties.shading`](../strict-ooxml-wml/src/model/props.rs:173) | Не читается: фон run теряется (работает только `w:highlight`). |
| P1 | `w:sym/@w:font` | [`Symbol.font`](../strict-ooxml-wml/src/model/inline.rs:28) | Не читается: символ выводится шрифтом run ([`paragraph.rs:541`](../strict-ooxml-render-svg/src/layout/paragraph.rs:541)). Wingdings/Symbol-глифы из `w:sym` превращаются в случайные буквы. |
| P1 | `w:rFonts/@w:eastAsia`, `@w:eastAsiaTheme` | [`Fonts.east_asia`](../strict-ooxml-wml/src/model/values.rs:1039) | Читаются только при слиянии стилей. CJK-текст набирается латинским шрифтом или запасным. |
| P2 | `w:rFonts/@w:hint`, `@w:cstheme` | `Fonts.hint`, `Fonts.cs_theme` | Не применяются. |
| P2 | `w:bdr` | `RunProperties.borders` | Рамка вокруг run не рисуется. |
| P2 | `w:lang` | `RunProperties.language` | Не читается (шейпинг, локализованные формы). |
| P2 | `w:cs` | `RunProperties.complex_script` | Флаг не учитывается; выбор `bCs/iCs/szCs` идёт только по `w:rtl` и `w:rFonts/@w:cs`. |
| P2 | `w:dir`, `w:bdo` | `Directional.kind` | Не различаются: явное направление/переопределение bidi не применяется. |
| P3 | `w:shadow`, `w:emboss`, `w:imprint`, `w:outline` | `RunProperties.*` | Не читаются. |
| P3 | `w:em` | `RunProperties.emphasis` | Знаки акцента (CJK) не рисуются. |
| P3 | `w:kern` | `RunProperties.kerning` | Не читается (кернинг шейпера включён всегда). |
| P3 | `w:specVanish`, `w:snapToGrid` | `RunProperties.*` | Не читаются. |
| P3 | `w:t/@xml:space` | [`TextNode.space`](../strict-ooxml-wml/src/model/inline.rs:21) | Не читается рендерером. Нужно проверить, обрезает ли парсер пробелы без `preserve`; иначе ведущие и хвостовые пробелы видны там, где Word их убирает. |

## 4. Секция и страница

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P1 | `w:pgMar/@w:gutter` | [`PageMargins.gutter`](../strict-ooxml-wml/src/model/props.rs:315) | Не читается (только конструируется как `None`). Поле переплёта не добавляется. |
| P1 | `w:mirrorMargins` | `Settings.mirror_margins` | Не читается: на чётных страницах поля не меняются местами. Вместе с ним не читаются `w:gutterAtTop` и `w:rtlGutter`. |
| P2 | `w:cols/@w:equalWidth="0"`, `w:col` | `Columns.equal_width`, [`ColumnSpec`](../strict-ooxml-wml/src/model/props.rs:318) | В [`geometry_for`](../strict-ooxml-render-svg/src/layout/mod.rs:449) читается только количество `w:col`; их ширины и промежутки не используются, все колонки равные. |
| P2 | `w:cols/@w:sep` | [`Columns.separator`](../strict-ooxml-wml/src/model/props.rs:337) | Разделитель между колонками не рисуется. |
| P2 | `w:vAlign` секции | [`SectionProperties.vertical_align`](../strict-ooxml-wml/src/model/props.rs:485) | Не читается: страница не центрируется и не выравнивается по низу. |
| P2 | `w:lnNumType` | `SectionProperties.line_numbering`, `LineNumbering` | Нумерация строк не выводится; `w:suppressLineNumbers` тоже не читается. |
| P3 | `w:docGrid/@w:charSpace` | `DocGrid.character_space` | Сетка символов не применяется (используется только `linePitch`). |
| P3 | `w:textDirection`, `w:bidi` секции | `SectionProperties.*` | Не читаются. |
| P3 | `w:pgNumType/@w:chapStyle`, `@w:chapSep` | `PageNumberType.*` | Номера страниц с номером главы не строятся. |
| P3 | `w:autoHyphenation`, `w:hyphenationZone`, `w:doNotHyphenateCaps` | `Settings.*` | Автоматические переносы не делаются. |
| P3 | `w:compat/*` | `Settings.compat_flags` | Ни один флаг совместимости не читается (`spaceForUL`, `adjustLineHeightInTable`, `balanceSingleByteDoubleByteWidth` и т.д.). |
| P3 | `w:clrSchemeMapping` | `Settings.color_scheme_mapping` | Предполагается стандартное сопоставление `bg1→lt1`, `tx1→dk1`. |

## 5. Сноски и нумерация

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P1 | `w:lvlJc` | [`Level.justification`](../strict-ooxml-wml/src/model/numbering.rs:28) | Не читается: маркеры списков всегда выровнены влево (`center`/`right` теряются, в частности римские номера по правому краю). |
| P2 | `w:isLgl` | `Level.is_legal` | Не читается: «1.1.1» в юридическом стиле не перестраивается из римских/буквенных уровней. |
| P2 | `w:numRestart` | [`NoteProperties.num_restart`](../strict-ooxml-wml/src/model/notes.rs:149) | Не читается: сноски не перезапускают нумерацию по странице или секции. |
| P2 | `w:footnotePr/w:pos`, `w:endnotePr/w:pos` | `NoteProperties.position` | Используется только при слиянии: `beneathText`, `sectEnd` не применяются. |
| P2 | `w:footnotePr` секций | `SectionProperties.footnote_properties` | [`notes.rs:131`](../strict-ooxml-render-svg/src/notes.rs:131) берёт свойства только **последней** секции. Формат и старт нумерации других секций теряются. |
| P3 | разделители сносок (`w:footnote w:type="separator"`) | `Notes::separator()`, `NoteProperties.separator_ids` | Методы не вызываются: рисуется своя линия, пользовательский разделитель из документа не используется. |

## 6. DrawingML и VML

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P1 | `a:srcRect` | [`Picture.src_rect`](../strict-ooxml-wml/src/model/drawing.rs:107) | Не читается: обрезка картинки теряется, изображение выводится целиком и сжимается в рамку. |
| P1 | `wps:style` (`a:lnRef`, `a:fillRef`, `a:fontRef`) | [`Shape.style`](../strict-ooxml-wml/src/model/drawing.rs:419), [`ShapeStyle`](../strict-ooxml-wml/src/model/drawing.rs:296) | Не читается. Фигуры, вставленные в Word без явного `spPr`, остаются без заливки и контура, а их текст — без цвета из `fontRef`. |
| P2 | `a:headEnd`, `a:tailEnd` | [`ShapeStroke.head_end/tail_end`](../strict-ooxml-wml/src/model/drawing.rs:213) | Стрелки на линиях не рисуются; `ShapeStroke.join` тоже не читается. |
| P2 | `a:path/@fill`, `@stroke` | [`GeometryPath.fill/stroke`](../strict-ooxml-wml/src/model/drawing.rs:266) | Не читаются: контуры `fill="none"` в `custGeom` заливаются. |
| P2 | `a:grpSpPr/a:xfrm/@rot`, `@flipH`, `@flipV` | [`GroupTransform.rot/flip_h/flip_v`](../strict-ooxml-wml/src/model/drawing.rs:436) | Поворот и отражение группы не применяются (для отдельных фигур применяются). |
| P2 | `wp:anchor/@hidden` | `AnchorDrawing.hidden` | Скрытые объекты рисуются. |
| P2 | `wp:anchor/@simplePos`, `wp:simplePos` | `AnchorDrawing.simple_pos/simple_pos_point` | Не читаются: такое позиционирование заменяется обычным. |
| P2 | `wp:anchor/@layoutInCell` | `AnchorDrawing.layout_in_cell` | Не читается: привязка к ячейке не отличается от привязки к странице. |
| P2 | `wps:bodyPr/@wrap="none"`, `@vert`, `@upright`, `@rot` | [`TextBoxBody`](../strict-ooxml-wml/src/model/drawing.rs:332) | Текст в надписи всегда переносится и идёт горизонтально. Также не читаются `numCol`/`spcCol`, `horzOverflow`/`vertOverflow`, `anchorCtr`. |
| P2 | `wp14:pctPosHOffset`/`pctPosVOffset` | `Position.percent_offset` | Не читается. |
| P2 | `wpc:lockedCanvas` | `LockedCanvas.markup/images` | Не читается: содержимое не рисуется. |
| P3 | `a:effectLst` | `Shape.effects` | Тени и свечение не рисуются. |
| P3 | `a:custGeom/a:rect` | `CustomGeometry.text_rect` | Текстовая область своей геометрии не учитывается. |
| P3 | `wp:effectExtent`, `@allowOverlap` | `AnchorDrawing.*` | Не читаются. |
| P3 | `a:blip/@cstate`, `@bwMode` | `BlipRef.cstate`, `Picture.bw_mode`, `Shape.bw_mode` | Не читаются (чёрно-белый режим). |
| P3 | встроенные шрифты (`w:embedRegular`…) | `FontEntry.embeds` | `FontTable::embedded_parts()` не вызывается: встроенные шрифты документа не используются, работает подстановка. |
| P3 | `w:pBdr`/`w:pgBorders` `@w:themeColor`, `@w:shadow`, `@w:frame` | [`Border.theme_color`](../strict-ooxml-wml/src/model/values.rs:1001), `shadow`, `frame`; `PageBorder.shadow` | Тематический цвет границы и тень не применяются. |

## 7. Математика (OMML)

| Pri | OOXML | Модель | Что происходит |
|---|---|---|---|
| P2 | `m:mathPr/m:naryLim`, `m:intLim` | `MathProperties.nary_limit/integral_limit` | Не читаются: размещение пределов по умолчанию для документа игнорируется (работает только `m:limLoc` у оператора). |
| P2 | `m:mathPr/m:dispDef`, `m:defJc`, `m:lMargin`, `m:rMargin`, `m:wrapIndent`, `m:wrapRight`, `m:smallFrac`, `m:brkBin`, `m:brkBinSub`, `m:interSp`, `m:intraSp`, `m:mathFont` | `MathProperties.*` | Не читаются. Из `m:mathPr` используются только `m:preSp` и `m:postSp`. |
| P2 | `m:mcs/m:mc/m:mcPr/m:count` | [`MatrixColumn.count`](../strict-ooxml-wml/src/model/math.rs:558) | Не читается: выравнивание колонок матрицы применяется к одной колонке вместо `count`. |
| P2 | `m:mPr/m:cGpRule`/`m:cgSp`, `m:plcHide` | `Matrix.column_group_spacing/hide_placeholders` | Не читаются. |
| P2 | `m:eqArrPr/m:maxDist`, `m:objDist` | `EquationArray.max_distance/object_distance` | Не читаются. |
| P3 | `m:dPr/m:shp` | `Delimiter.shape` | `centered`/`match` не различаются. |
| P3 | `m:groupChrPr/m:vertJc` | `GroupCharacter.vertical_justification`, `MathVerticalJc` | Не читается. |
| P3 | `m:phantPr/m:transp`, `m:show` | `Phantom.transparent/show_all` | Не читаются. |
| P3 | `m:borderBoxPr`, `m:boxPr/m:aln` | `BorderBox.alignment/lines`, `Boxed.alignment` | Не читаются. |
| P3 | `m:argPr` (`m:lit`, `m:nor`, `m:scr`, `m:sty`), `m:rPr/m:aln` | `ArgumentProperties.*`, `MathRunProperties.alignment` | Не читаются (у `m:r` `lit/nor/scr/sty` читаются). |

## 8. Проверено: не потери

Компилятор показал эти элементы как не читаемые рендерером напрямую, но они
доходят до рендеринга через методы модели или служат только для записи и отчётов:

- `Theme.colors`, `Theme.fonts`, `ThemeFonts.major/minor` — через `Theme::color`,
  `Theme::font` ([`style.rs:691`](../strict-ooxml-render-svg/src/style.rs:691)).
- `AbstractNum.levels`, `Level.ilvl` — через `AbstractNum::level`
  ([`numbering.rs:185`](../strict-ooxml-render-svg/src/numbering.rs:185)).
- `Document.headers_footers`, `HeaderFooter.part` — через `Document::header_footer`.
- `RevisionKind::*` — через методы `Revision` в `revision_visible`.
- `PageOrientation::Portrait`, `LimitLocation::SubSup`, `DocGridType::Default`,
  `WidthKind::Auto/Nil` — второй вариант назван явно, этот корректно идёт в `else`.
- Без визуального эффекта: `location`, rsid, `para_id`/`text_id`, SDT-метаданные
  (`tag`, `alias`, `data_binding`, `lock`…), proofing, `zoom`, `view`,
  `document_protection`, переменные документа, UI-флаги стилей (`q_format`,
  `ui_priority`, `semi_hidden`…), сигнатуры шрифтов (`FontSig`, `panose`),
  `Hyperlink.target/tooltip` (для PDF можно сделать ссылки, но картинку это не
  меняет), `FeatureUse`, `SupportStatus`, `MediaKind`.

## Порядок исправления

Группы сформированы по месту исправления, внутри — по приоритету.

1. **`ComputedRun`** ([`style.rs`](../strict-ooxml-render-svg/src/style.rs)): тип
   и цвет подчёркивания, `dstrike`, `smallCaps`, `position`, заливка run,
   `w:sym/@w:font`, `eastAsia`-шрифт. Всё правится в одном месте и сразу видно.
2. **Абзац:** рисование линий `w:pBdr` (включая `between`), `contextualSpacing`,
   `widowControl`, autospacing.
3. **Таблицы:** `vAlign`, `hRule`, стили границ (`double` и др.), заливка
   таблицы и `w:shd/@w:val`, `cantSplit`. Затем `tblpPr`, `tblLayout`, `tcW`.
4. **DrawingML:** `srcRect`, `wps:style`, стрелки, `path/@fill`, поворот групп,
   `hidden`, свойства `bodyPr`.
5. **Страница:** `gutter`, `mirrorMargins`, неравные колонки и разделитель,
   `vAlign` секции, `lnNumType`.
6. **Сноски и нумерация:** `lvlJc`, `isLgl`, `numRestart`, свойства сносок по
   секциям.
7. **Математика:** `naryLim/intLim`, `defJc/dispDef`, `mcs/count`, остальное по мере
   появления в корпусе.

Перед исправлением каждого P1 стоит посчитать частоту элемента в корпусе
(`testdata`), чтобы порядок опирался на данные, а не только на оценку.

## Как повторить проверку

1. Скопировать рабочее дерево без `target` во временный каталог.
2. В копии перед каждым `pub поле:` в `pub struct` и каждым вариантом
   `pub enum` в `strict-ooxml-wml/src/model/*.rs` вставить
   `#[deprecated(note = "AUDIT")]`.
3. `cargo +1.92.0 check -p strict-ooxml-render-svg -p strict-ooxml-render-pdf --lib --all-features --message-format=json > check.json`.
4. Из `check.json` взять сообщения с `code = deprecated`, сгруппировать по
   элементу (путь после `model::` без имён модулей, так как модель
   реэкспортирует типы) и по файлу использования. Элементы без использований в
   `render-svg/src`, `render-pdf/src`, `wml/src/resolve` — кандидаты в потери.
