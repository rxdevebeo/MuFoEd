# Потери между парсером DOCX и рендерером

Дата: 2026-10-11, обновлено 2026-10-12. Актуальное состояние: ветка
`fix/wps-qa-2026-10-12` поверх `master` (`4399071`): коммиты `ed013a7` …
`9131ea3`. Исходный аудит ниже описывает состояние до исправлений; текущий
статус — в разделах «Результат продолжения», «Продолжение 2026-10-12» и
«Что осталось».

## Повод

Парсер читает `w:cols` в [`Columns`](../strict-ooxml-wml/src/model/props.rs),
а [`geometry_for`](../strict-ooxml-render-svg/src/layout/mod.rs) до последней
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

## Результат продолжения (2026-10-11)

Реализация существенно расширена во всех семи группах. Полное закрытие всех
визуальных различий **не подтверждено**: ниже явно перечислены остающиеся
ограничения. «Реализовано» означает наличие пути parser/model/layout/paint,
а не проверенное совпадение с Word.

### Реализовано

| Группа | Текущее поведение | Основные исходники |
|---|---|---|
| Таблицы | Высота строки и вертикальное выравнивание, вертикальные объединения, границы/диагонали/заливки, `tblPrEx`, каскад table/row/cell и условных полос/краёв/углов; явное `false` для `bidiVisual`, `cantSplit`, `noWrap`, `tcFitText`, `hideMark`; `gridBefore/After`, `wBefore/After`, `tcW` dxa/pct и spans; AutoFit по содержимому, включая слова через runs, CJK, формулы, графику и вложенные таблицы. | [table.rs](../strict-ooxml-render-svg/src/layout/table.rs), [table_style.rs](../strict-ooxml-render-svg/src/table_style.rs) |
| Таблицы: страницы и объекты | Строки без `cantSplit` делятся между областями содержимого, заголовки повторяются. `tblpPr` задаёт плавающее положение и обтекание; таблицы с `tblOverlap=never` избегают пересечений. Длинные плавающие таблицы продолжаются на новых страницах. Привязки `layoutInCell=0` сохраняются до размещения строки; `layoutInCell=1` получает геометрию ячейки с уточнением автоматической высоты. | [paginate.rs](../strict-ooxml-render-svg/src/layout/paginate.rs), [table.rs](../strict-ooxml-render-svg/src/layout/table.rs) |
| Абзацы | `pBdr`/`between`, общие рамки, `contextualSpacing`, `widowControl`, autospacing, `beforeLines/afterLines`, символьные отступы, `wordWrap`, `br/@clear`, mirrored bidi indents и `start/end`. Unicode bidi с вложенными `dir`/`bdo`; inline-графика и формулы переставляются как объекты. Направление текста наследуется от стиля/контейнера/секции. | [paragraph.rs](../strict-ooxml-render-svg/src/layout/paragraph.rs), [style.rs](../strict-ooxml-render-svg/src/style.rs) |
| Runs | Тип/цвет подчёркивания, dstrike, smallCaps, position, shading/patterns, `sym` и шрифтовые каналы; независимые Latin/complex-script bold/italic/size, язык и порог кернинга; рамки соседних фрагментов объединяются. Shadow/emboss/imprint/outline и знаки emphasis по shaped clusters. `xml:space` влияет на вывод; writer сохраняет политику whitespace. | [decoration.rs](../strict-ooxml-render-svg/src/layout/decoration.rs), [style.rs](../strict-ooxml-render-svg/src/style.rs), [body.rs](../strict-ooxml-write/src/body.rs) |
| Страница | Gutter/rtlGutter/gutterAtTop, зеркальные поля, разные ширины и промежутки колонок, повторная раскладка и балансировка, разделители, vAlign и номера строк, charSpace/snapToGrid/adjustRightInd. Вертикальное тело секции выводится через общую аффинную геометрию SVG/PDF. PAGE учитывает chapStyle/chapSep и Heading N, включая цепочку basedOn. | [mod.rs](../strict-ooxml-render-svg/src/layout/mod.rs), [sectionpage.rs](../strict-ooxml-render-svg/src/layout/sectionpage.rs), [fields.rs](../strict-ooxml-render-svg/src/fields.rs) |
| Переносы и настройки | Автоматическая расстановка переносов по встроенным шаблонам `hypher`, включая слова через форматированные runs, language/bidi language, zone и запрет capitals. Применяются все семь флагов, представленных в `CompatFlags`: spaceForUL, ulTrailSpace, doNotExpandShiftReturn, adjustLineHeightInTable, balanceSingleByteDoubleByteWidth, doNotLeaveBackslashAlone, applyBreakingRules. `clrSchemeMapping` участвует в разрешении цвета. | [paragraph.rs](../strict-ooxml-render-svg/src/layout/paragraph.rs), [settings.rs](../strict-ooxml-wml/src/model/settings.rs), [style.rs](../strict-ooxml-render-svg/src/style.rs) |
| Нумерация и сноски | Нумерация через стили, lvlJc/isLgl/ilvl, свойства и numStart/format по секциям, eachSect/eachPage, beneathText/sectEnd/endDoc. Пользовательские separator/continuationSeparator/continuationNotice и separator_ids, ссылки в таблицах, продолжения длинных сносок. Клонирование таблиц при разрешении стилей сохраняет адресную идентичность маркеров. | [numbering.rs](../strict-ooxml-render-svg/src/numbering.rs), [notes.rs](../strict-ooxml-render-svg/src/notes.rs), [paginate.rs](../strict-ooxml-render-svg/src/layout/paginate.rs) |
| DrawingML | Crop, style refs, стрелки/join, индивидуальные fill/stroke контуров, аффинные повороты/отражения групп, hidden/docPr.hidden, simplePos и процентные позиции. LockedCanvas разбирается в дерево объектов. Custom paths поддерживают направляющие, move/line/cubic/quadratic/arc/close; guides также задают текстовую область. EffectExtent участвует в обтекании. | [graphics.rs](../strict-ooxml-render-svg/src/paint/graphics.rs), [drawing.rs](../strict-ooxml-wml/src/parse/drawing.rs), [drawing.rs writer](../strict-ooxml-write/src/drawing.rs), [floating.rs](../strict-ooxml-render-svg/src/layout/floating.rs) |
| Надписи и эффекты | bodyPr wrap/rot/upright/vert/vert270, numCol/spcCol/rtlCol, anchorCtr и clip; вертикальные варианты получают поворот глифов. Outer/inner shadow и glow имеют общий векторный вывод с прозрачностью, inner shadow обрезается контуром поверх заливки. | [graphics.rs](../strict-ooxml-render-svg/src/paint/graphics.rs), [effects.rs](../strict-ooxml-render-svg/src/paint/effects.rs), [PDF document.rs](../strict-ooxml-render-pdf/src/document.rs) |
| Шрифты | embedRegular/Bold/Italic/BoldItalic: загрузка частей, GUID-деобфускация, проверка OpenType, идентификация по SHA-256. Измерение, shaping, SVG font-face и PDF subsetting используют одну программу. Ограничены размер/число шрифтов и кэши; отсутствие частей/глифов даёт подстановку и диагностику. | [embedded.rs](../strict-ooxml-render-svg/src/font/embedded.rs), [face.rs](../strict-ooxml-render-svg/src/font/face.rs), [PDF font.rs](../strict-ooxml-render-pdf/src/font.rs) |
| OMML | Document defaults naryLim/intLim, dispDef/defJc, margins/wrap/spacing/mathFont/smallFrac; brkBin after/before/repeat и brkBinSub; matrix count/cGpRule/cSp/plcHide; eqArr maxDist/objDist и вложенные точки выравнивания; delimiter shape, groupChr vertJc и растяжение маркера; phantom transp/show, borderBox hide/strike и run alignment. | [math/layout.rs](../strict-ooxml-render-svg/src/math/layout.rs), [math.rs parser](../strict-ooxml-wml/src/parse/math.rs), [math.rs writer](../strict-ooxml-write/src/math.rs) |

### Исправлено при завершении текущего этапа

- Поворот абзаца/ячейки на 180° сохраняет ширину строки и высоту блока.
- Повторные заголовки плавающей таблицы сохраняют сноски, рамки и anchors;
  сноски резервируются до вычисления масштаба неделимого фрагмента.
- Привязки строк сохраняются и при выводе таблиц внутри рамок.
- Автоматическая высота ячейки сначала измеряется без плавающих исключений;
  затем обтекание уточняется по полученной высоте.
- Продолжение сноски сохраняет исходную систему координат: смена ширины
  колонки больше не переинтерпретирует уже потреблённую высоту.
- Writer больше не добавляет `xml:space="preserve"` к узлу с Default.
- Пустые колонки до/после ячеек учитываются при выборе условного стиля.
- PDF использует метрики встроенного шрифта и charScale в запасном пути вывода;
  поворот CJK-глифов использует те же метрики.
- Неподдержанные эффекты blur/reflection/softEdge/fillOverlay/duotone/prstShdw
  дают предупреждение вместо молчаливого пропуска.

### Уточнения исходного аудита

1. `a:blip/@cstate` описывает сохранённое состояние компрессии, а не команду
   повторно сжимать изображение при выводе. Его неприменение при обычном
   цветном рендеринге само по себе не является потерей картинки.
2. `bwMode` применяется в режиме чёрно-белого вывода. Текущий API выводит
   цветной документ; отдельный BW/print mode не реализован. Microsoft также
   описывает ограничение этого свойства режимом BW:
   [MS-OE376 bwMode](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/c4ab2d04-29dc-4ad6-8934-a24a8ccc09bf).
3. `m:lit/nor/scr/sty/aln` внутри `m:argPr` не являются допустимыми детьми
   Strict CT_OMathArgPr. Допустимый argSz сохраняется; run properties должны
   находиться в m:rPr. Расширенные поля модели учитываются при раскладке,
   но writer сообщает об их непредставимости вместо создания невалидного XML.
4. Строка `w:compat/*` исходного аудита относится к семи полям существующей
   модели. Она не означает поддержку всех исторических флагов Word.

Семантика вертикальных направлений и groupChr сверена с
[TextDirection](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.textdirection?view=openxml-3.0.1),
[VerticalJustification](https://learn.microsoft.com/en-nz/dotnet/api/documentformat.openxml.math.verticaljustification?view=openxml-2.10.1)
и [GroupCharProperties](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.math.groupcharproperties?view=openxml-2.16.0).

## Продолжение 2026-10-12

### Сборка и гейты

На `master` после `55f1584` не проходили `cargo fmt --check`, `clippy -D
warnings` (около 150 замечаний, в т. ч. индексирование в путях never-crash) и
12 тестов в 9 бинарниках (SSIM- и PDF-гейты, `geometry`, `math`, `notes`,
`props`, `f06`, `f16`, `pipeline_never_panics`). Всё исправлено; причины:

- `pipeline_never_panics` (таймаут 60 с на `cc0-docx/068`): SHA-256 мегабайтного
  встроенного шрифта на каждое измеряемое слово. Хэш считается один раз,
  шейпинг встроенных шрифтов кэшируется: 64 с → 2 с.
- `ssim`/`pdf_pixels` (`strict-stage5c`, SSIM 0,948 < 0,95 и сдвиг кромки
  до 58 px): строка со встроенной формулой резервировала рыхлые границы группы
  вместо чернил глифов; display-сетки (`m:m`, `m:eqArr`) не центрировались по
  оси, и `{` вокруг системы была вдвое выше; пределы n-арного оператора во
  встроенной формуле ставились над/под; `m:limUpp` уводил базу с линии;
  `groupChr` уводил базу с линии. SSIM 0,956, ратчеты обновлены (5 → −4,
  24 → 25, eqArr 7 → 2).
- Структурная корреляция `strict-stage5` (SVG): разделитель сносок 0,5 pt на
  дробной координате; теперь 0,75 pt (1 px).
- Остальные — устаревшие ожидания тестов (координаты внутри групп формул,
  `w:bdr` теперь моделируется, `iCs` только для complex script, длинная
  сноска переносится на следующую страницу, а не масштабируется).

### Закрыто из «Что осталось»

- **R4, частично:** `bodyPr/@vertOverflow="ellipsis"` — последняя видимая
  строка заканчивается «…», остальное отбрасывается и обрезается
  (`drawing_properties.rs::vertical_overflow_ellipsis_*`).
- **R7, частично:** приёмочные тесты новых путей —
  `tests/acceptance_r7.rs` (встроенный обфусцированный шрифт: деобфускация,
  метрики, ресурс SVG; custom geometry с `a:gd` и `a:arcTo`; плавающая
  таблица `w:tblpPr`), `tests/wps_qa_fixes.rs` (знак абзаца, inline-картинки,
  `effectExtent`, перенос после дефиса, сетка строк, табуляция за полем,
  `noWrap`, рамка в умолчаниях, номер формулы, 16 надписей подряд), юнит-тесты
  TIFF и метрик встроенного шрифта. Визуальная приёмка — прогоны `xtool/wps-qa`
  по 551 документу (`docs/WPS-QA-DISCREPANCIES.md`, «Итог 2026-10-12»).
- **Встроенные шрифты:** высота строки теперь по Windows-метрикам (как GDI у
  Word), а не по `hhea` (WQ-040).

### Найдено попутно и исправлено

- Глубина вложенности блоков в контексте раскладки не восстанавливалась после
  надписи: с 13-й надписи содержимое молча отбрасывалось (WQ-009).
- `w:framePr` в `w:pPrDefault` делал рамкой каждый абзац документа (WQ-006).
- `w:suppressAutoHyphens` не был в модели (WQ-039).

## Что осталось

Это текущий остаток; исторические формулировки «не читается» ниже не являются
списком сегодняшних дефектов.

| ID | Приоритет | Статус 2026-10-12 | Оставшийся пробел | Условие закрытия |
|---|---|---|---|---|
| R1 | P2 | открыт | Полное согласование `allowOverlap` и обтекания между абзацами: окончательный сдвиг при рисовании может отличаться от области, использованной при раскладке текста. Не едины коллизии drawings, frames и floating tables. Сюда же WQ-016/WQ-019: зона обтекания плавающей таблицы/объекта регистрируется после раскладки предшествующих абзацев. | Единый реестр расположений до раскладки; повторная раскладка затронутых строк по окончательным координатам. |
| R2 | P2 | открыт | Вертикальная вёрстка: физические PAGE/simplePos-привязки, escaped anchors повёрнутых ячеек, явно переопределённое направление CJK и номера вертикальных строк. Смена направления в continuous/nextColumn пока начинает новую страницу с предупреждением. | Разделить физические и логические координаты, сохранить привязки/порядок глифов при смене направления; определить поведение смешанной страницы. |
| R3 | P2 | частично | AutoFit эвристический. Для `noWrap` правило сверено со спецификацией и с WPS (WQ-020), явная `tblW` больше не сжимается в колонку; конфликты grid/tcW/spans/min-content и повторное измерение ячеек с рамками не проверены. | Воспроизводимые случаи конфликтующих ширин/вложенных таблиц и независимое измерение без изменения состояния рамок. |
| R4 | P2 | частично | `vertOverflow="ellipsis"` сделан; mongolianVert и wordArtVertRtl пока без отдельного порядка вертикальных колонок. | Отдельное направление колонок для этих режимов. |
| R5 | P3 | открыт | Эффекты blur/reflection/softEdge/fillOverlay/duotone/prstShdw не рисуются (предупреждение). Размытие shadow/glow приближено; inner shadow берёт первый контур. Новое: `a:lum`/`a:alphaModFix` картинки передаются прозрачностью (WQ-042). | Общий эффектный контур/маска и поддержка остальных узлов; согласовать прозрачность групп SVG/PDF. |
| R6 | P2 | открыт | Продолжения сносок при смене ширины сохраняют исходную раскладку и сжимаются по горизонтали. Неделимый фрагмент, не влезающий на страницу, теперь переносится на следующую (а не масштабируется на этой). | Семантический курсор по содержимому сноски вместо consumed-y и повторная раскладка только оставшейся части. |
| R7 | P2 | частично | Тесты и визуальная приёмка добавлены (см. выше). Не покрыты отдельными фикстурами: bidi+графика, сложные eqArr с несколькими точками выравнивания, коллизии нескольких объектов, таблицы со сносками. | Фикстуры на оставшиеся пути; повторный прогон `xtool/wps-qa` после каждого изменения раскладки. |

### Проверка и Git

- 2026-10-12: `cargo +1.92.0 fmt --check`, `clippy --workspace --all-targets
  --all-features -D warnings`, `check --workspace --all-targets`, `doc`,
  полный `cargo test --workspace --all-features` с `STRICT_OOXML_CORPUS=require`,
  python-гейты CI (`no_ecma_bytes`, `text_metric`, `audit-fixes` F00/F21) —
  зелёные.
- Визуальная приёмка: `xtool/wps-qa` по 551 документу против эталона WPS,
  средний балл 67,0 (`bin-6e0487d90c`) → см. итог в
  `docs/WPS-QA-DISCREPANCIES.md`.
- Коммиты: `ed013a7`, `2c4803e`, `3cb3439`, `2de33bf`, `ce3b9eb`, `5f83025`,
  `9131ea3` в ветке `fix/wps-qa-2026-10-12`.

## Исторический аудит до исправлений

Следующие таблицы сохранены для трассировки исходных находок. Колонка
«Что происходило» описывает прошлое состояние, а не текущую поддержку.

## Приоритеты

- **P1** — встречается в обычных документах и сразу видно на странице.
- **P2** — заметно в отдельных классах документов (RTL, CJK, вёрстка журналов,
  формулы, фигуры).
- **P3** — редкие или мелкие визуальные эффекты.

## 1. Таблицы

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P1 | `w:tcPr/w:vAlign` | [`CellProperties.vertical_align`](../strict-ooxml-wml/src/model/props.rs) | Читается только для каскада в [`table_style.rs:266`](../strict-ooxml-render-svg/src/table_style.rs) и нигде не применяется. Содержимое ячейки всегда прижато к верху; `center`/`bottom` теряются. |
| P1 | `w:trHeight/@w:hRule` | [`RowHeight.rule`](../strict-ooxml-wml/src/model/values.rs) | [`table.rs:108`](../strict-ooxml-render-svg/src/layout/table.rs) всегда берёт `max(контент, высота)`, то есть `atLeast`. `exact` не обрезает строку, `auto` не игнорирует значение. |
| P1 | `w:tcBorders`/`w:tblBorders` `@w:val` | [`BorderStyle`](../strict-ooxml-wml/src/model/values.rs) | Различаются только «нет», «пунктир/точки» и «сплошная» ([`table.rs:1463`](../strict-ooxml-render-svg/src/layout/table.rs)). `double`, `triple`, `thinThick*`, `wave`, `threeDEmboss` и т.п. рисуются одной линией. Так же в [`pageborders.rs:166`](../strict-ooxml-render-svg/src/layout/pageborders.rs). |
| P1 | `w:tblPr/w:shd` | [`TableProperties.shading`](../strict-ooxml-wml/src/model/props.rs) | Не читается. Заливка на уровне таблицы теряется, работает только заливка ячеек. |
| P1 | `w:shd/@w:val`, `@w:color` | [`Shading.pattern`](../strict-ooxml-wml/src/model/values.rs), `color`, `theme_color` | Используется только `fill`. `solid` (цвет берётся из `w:color`), `pct10…pct90`, штриховки теряются. Касается абзацев, ячеек и run. |
| P1 | `w:trPr/w:cantSplit` | [`RowProperties.cant_split`](../strict-ooxml-wml/src/model/props.rs) | Не читается, но потеря обратная: пагинатор никогда не делит строку, так что все строки ведут себя как `cantSplit`, а длинная строка без него переносится целиком (уточнено при реализации, см. «Статус»). |
| P2 | `w:tblpPr` | [`TableProperties.positioning`](../strict-ooxml-wml/src/model/props.rs) и вся [`TablePositioning`](../strict-ooxml-wml/src/model/props.rs) | Плавающие таблицы ставятся в поток; обтекание не строится. |
| P2 | `w:tblLayout` | [`TableProperties.layout`](../strict-ooxml-wml/src/model/props.rs), [`TableLayout`](../strict-ooxml-wml/src/model/values.rs) | `fixed` и `autofit` не различаются. |
| P2 | `w:tcW` | `CellProperties.width` | Не читается: ширины берутся из `w:tblGrid` и `gridSpan`. Документы с устаревшей сеткой расходятся с Word. |
| P2 | `w:gridAfter`, `w:wBefore`, `w:wAfter` | `RowProperties.grid_after/width_before/width_after` | Не читаются. `w:gridBefore` читается. |
| P2 | `w:tblPrEx/w:tblBorders` | `RowProperties.exception_borders` | Не читается. Из `tblPrEx` используется только `tblCellSpacing`. |
| P2 | `w:bidiVisual` | `TableProperties.bidi_visual` | RTL-таблицы не зеркалируются. |
| P2 | `w:tblStylePr/w:tblPr`, `w:trPr` в стиле | `TableStyleCondition.table/row`, `Style.row` | Условные свойства таблицы и строки из стиля не применяются, только `tcPr`/`pPr`/`rPr`. |
| P3 | `w:tblInd/@w:type` | `TableProperties.indent_kind` | Отступ всегда трактуется как twips. |
| P3 | `w:tl2br`, `w:tr2bl` | `CellProperties.diagonal_down/up` | Диагонали ячеек не рисуются. |
| P3 | `w:noWrap`, `w:tcFitText`, `w:hideMark` | `CellProperties.no_wrap/fit_text/hide_mark` | Не читаются. |
| P3 | `w:tcPr/w:textDirection` | [`TextDirection`](../strict-ooxml-wml/src/model/values.rs) | Обрабатываются только `btLr` и `tbRl` ([`table.rs:447`](../strict-ooxml-render-svg/src/layout/table.rs)). Вертикальные варианты `*V` (CJK) идут как горизонтальные. |

## 2. Абзац

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P1 | `w:pBdr` (линии) | [`ParagraphProperties.borders`](../strict-ooxml-wml/src/model/props.rs) | В [`style.rs:360`](../strict-ooxml-render-svg/src/style.rs) из `top`/`bottom` берётся только отступ. **Линии рамки не рисуются вообще**; `left`, `right` не используются. |
| P1 | `w:pBdr/w:between`, `w:bar` | [`Borders.between`](../strict-ooxml-wml/src/model/values.rs), `bar` | Не читаются. |
| P1 | `w:contextualSpacing` | [`ParagraphProperties.contextual_spacing`](../strict-ooxml-wml/src/model/props.rs) | Не читается: между абзацами одного стиля (списки) остаются интервалы `before/after`. |
| P1 | `w:widowControl` | `ParagraphProperties.widow_control` | Не читается: висячие строки не контролируются, разрывы страниц отличаются от Word. |
| P2 | `w:spacing/@w:beforeAutospacing`, `@w:afterAutospacing` | [`Spacing`](../strict-ooxml-wml/src/model/values.rs) | Не читаются (типично для документов из HTML: Word ставит 14 pt). |
| P2 | `w:spacing/@w:beforeLines`, `@w:afterLines` | [`Spacing.before_lines`](../strict-ooxml-wml/src/model/values.rs), `after_lines` | `before_lines` проверяется только на наличие в [`paragraph.rs:208`](../strict-ooxml-render-svg/src/layout/paragraph.rs) (математический абзац). Значения в строках сетки не применяются. |
| P2 | `w:ind/@w:startChars`… `@w:hangingChars` | `Indentation.*_chars` | Не читаются (CJK). |
| P2 | `w:bidi` | `ParagraphProperties.bidi` | RTL-абзацы не зеркалируют выравнивание и отступы. |
| P2 | `w:textDirection` абзаца | `ParagraphProperties.text_direction` | Не читается. |
| P3 | `w:snapToGrid`, `w:wordWrap`, `w:autoSpaceDE/DN`, `w:adjustRightInd`, `w:suppressOverlap` | соответствующие `ParagraphProperties.*` | Не читаются. |
| P3 | `w:jc` = `*Kashida`, `thaiDistribute` | [`Justification`](../strict-ooxml-wml/src/model/values.rs) | Не различаются, падают в общую ветку. |
| P3 | `w:br/@w:clear` | `BreakClear` | `left/right/all` не различаются: перенос под плавающий объект не делается. |

## 3. Run

Все свойства run проходят через [`ComputedRun`](../strict-ooxml-render-svg/src/style.rs).
Всё, для чего там нет поля, теряется в [`apply_run_props_mode`](../strict-ooxml-render-svg/src/style.rs).

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P1 | `w:u/@w:val` | [`Underline`](../strict-ooxml-wml/src/model/values.rs) | Схлопывается в `bool` ([`style.rs:476`](../strict-ooxml-render-svg/src/style.rs)). `double`, `thick`, `dotted`, `dash*`, `wave*`, `words` рисуются одной сплошной линией. |
| P1 | `w:u/@w:color`, `@w:themeColor` | `RunProperties.underline_color/underline_theme` | Не читаются: подчёркивание всегда цвета текста. |
| P1 | `w:dstrike` | `RunProperties.double_strike` | Сливается с `w:strike` ([`style.rs:479`](../strict-ooxml-render-svg/src/style.rs)), рисуется одной линией. |
| P1 | `w:smallCaps` | `RunProperties.small_caps` | Не читается: текст выводится строчными. |
| P1 | `w:position` | [`RunProperties.position`](../strict-ooxml-wml/src/model/props.rs) | Не читается: подъём/опускание базовой линии теряется. |
| P1 | `w:rPr/w:shd` | [`RunProperties.shading`](../strict-ooxml-wml/src/model/props.rs) | Не читается: фон run теряется (работает только `w:highlight`). |
| P1 | `w:sym/@w:font` | [`Symbol.font`](../strict-ooxml-wml/src/model/inline.rs) | Не читается: символ выводится шрифтом run ([`paragraph.rs:541`](../strict-ooxml-render-svg/src/layout/paragraph.rs)). Wingdings/Symbol-глифы из `w:sym` превращаются в случайные буквы. |
| P1 | `w:rFonts/@w:eastAsia`, `@w:eastAsiaTheme` | [`Fonts.east_asia`](../strict-ooxml-wml/src/model/values.rs) | Читаются только при слиянии стилей. CJK-текст набирается латинским шрифтом или запасным. |
| P2 | `w:rFonts/@w:hint`, `@w:cstheme` | `Fonts.hint`, `Fonts.cs_theme` | Не применяются. |
| P2 | `w:bdr` | `RunProperties.borders` | Рамка вокруг run не рисуется. |
| P2 | `w:lang` | `RunProperties.language` | Не читается (шейпинг, локализованные формы). |
| P2 | `w:cs` | `RunProperties.complex_script` | Флаг не учитывается; выбор `bCs/iCs/szCs` идёт только по `w:rtl` и `w:rFonts/@w:cs`. |
| P2 | `w:dir`, `w:bdo` | `Directional.kind` | Не различаются: явное направление/переопределение bidi не применяется. |
| P3 | `w:shadow`, `w:emboss`, `w:imprint`, `w:outline` | `RunProperties.*` | Не читаются. |
| P3 | `w:em` | `RunProperties.emphasis` | Знаки акцента (CJK) не рисуются. |
| P3 | `w:kern` | `RunProperties.kerning` | Не читается (кернинг шейпера включён всегда). |
| P3 | `w:specVanish`, `w:snapToGrid` | `RunProperties.*` | Не читаются. |
| P3 | `w:t/@xml:space` | [`TextNode.space`](../strict-ooxml-wml/src/model/inline.rs) | Не читается рендерером. Нужно проверить, обрезает ли парсер пробелы без `preserve`; иначе ведущие и хвостовые пробелы видны там, где Word их убирает. |

## 4. Секция и страница

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P1 | `w:pgMar/@w:gutter` | [`PageMargins.gutter`](../strict-ooxml-wml/src/model/props.rs) | Не читается (только конструируется как `None`). Поле переплёта не добавляется. |
| P1 | `w:mirrorMargins` | `Settings.mirror_margins` | Не читается: на чётных страницах поля не меняются местами. Вместе с ним не читаются `w:gutterAtTop` и `w:rtlGutter`. |
| P2 | `w:cols/@w:equalWidth="0"`, `w:col` | `Columns.equal_width`, [`ColumnSpec`](../strict-ooxml-wml/src/model/props.rs) | В [`geometry_for`](../strict-ooxml-render-svg/src/layout/mod.rs) читается только количество `w:col`; их ширины и промежутки не используются, все колонки равные. |
| P2 | `w:cols/@w:sep` | [`Columns.separator`](../strict-ooxml-wml/src/model/props.rs) | Разделитель между колонками не рисуется. |
| P2 | `w:vAlign` секции | [`SectionProperties.vertical_align`](../strict-ooxml-wml/src/model/props.rs) | Не читается: страница не центрируется и не выравнивается по низу. |
| P2 | `w:lnNumType` | `SectionProperties.line_numbering`, `LineNumbering` | Нумерация строк не выводится; `w:suppressLineNumbers` тоже не читается. |
| P3 | `w:docGrid/@w:charSpace` | `DocGrid.character_space` | Сетка символов не применяется (используется только `linePitch`). |
| P3 | `w:textDirection`, `w:bidi` секции | `SectionProperties.*` | Не читаются. |
| P3 | `w:pgNumType/@w:chapStyle`, `@w:chapSep` | `PageNumberType.*` | Номера страниц с номером главы не строятся. |
| P3 | `w:autoHyphenation`, `w:hyphenationZone`, `w:doNotHyphenateCaps` | `Settings.*` | Автоматические переносы не делаются. |
| P3 | `w:compat/*` | `Settings.compat_flags` | Ни один флаг совместимости не читается (`spaceForUL`, `adjustLineHeightInTable`, `balanceSingleByteDoubleByteWidth` и т.д.). |
| P3 | `w:clrSchemeMapping` | `Settings.color_scheme_mapping` | Предполагается стандартное сопоставление `bg1→lt1`, `tx1→dk1`. |

## 5. Сноски и нумерация

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P1 | `w:lvlJc` | [`Level.justification`](../strict-ooxml-wml/src/model/numbering.rs) | Не читается: маркеры списков всегда выровнены влево (`center`/`right` теряются, в частности римские номера по правому краю). |
| P2 | `w:isLgl` | `Level.is_legal` | Не читается: «1.1.1» в юридическом стиле не перестраивается из римских/буквенных уровней. |
| P2 | `w:numRestart` | [`NoteProperties.num_restart`](../strict-ooxml-wml/src/model/notes.rs) | Не читается: сноски не перезапускают нумерацию по странице или секции. |
| P2 | `w:footnotePr/w:pos`, `w:endnotePr/w:pos` | `NoteProperties.position` | Используется только при слиянии: `beneathText`, `sectEnd` не применяются. |
| P2 | `w:footnotePr` секций | `SectionProperties.footnote_properties` | [`notes.rs:131`](../strict-ooxml-render-svg/src/notes.rs) берёт свойства только **последней** секции. Формат и старт нумерации других секций теряются. |
| P3 | разделители сносок (`w:footnote w:type="separator"`) | `Notes::separator()`, `NoteProperties.separator_ids` | Методы не вызываются: рисуется своя линия, пользовательский разделитель из документа не используется. |

## 6. DrawingML и VML

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P1 | `a:srcRect` | [`Picture.src_rect`](../strict-ooxml-wml/src/model/drawing.rs) | Не читается: обрезка картинки теряется, изображение выводится целиком и сжимается в рамку. |
| P1 | `wps:style` (`a:lnRef`, `a:fillRef`, `a:fontRef`) | [`Shape.style`](../strict-ooxml-wml/src/model/drawing.rs), [`ShapeStyle`](../strict-ooxml-wml/src/model/drawing.rs) | Не читается. Фигуры, вставленные в Word без явного `spPr`, остаются без заливки и контура, а их текст — без цвета из `fontRef`. |
| P2 | `a:headEnd`, `a:tailEnd` | [`ShapeStroke.head_end/tail_end`](../strict-ooxml-wml/src/model/drawing.rs) | Стрелки на линиях не рисуются; `ShapeStroke.join` тоже не читается. |
| P2 | `a:path/@fill`, `@stroke` | [`GeometryPath.fill/stroke`](../strict-ooxml-wml/src/model/drawing.rs) | Не читаются: контуры `fill="none"` в `custGeom` заливаются. |
| P2 | `a:grpSpPr/a:xfrm/@rot`, `@flipH`, `@flipV` | [`GroupTransform.rot/flip_h/flip_v`](../strict-ooxml-wml/src/model/drawing.rs) | Поворот и отражение группы не применяются (для отдельных фигур применяются). |
| P2 | `wp:anchor/@hidden` | `AnchorDrawing.hidden` | Скрытые объекты рисуются. |
| P2 | `wp:anchor/@simplePos`, `wp:simplePos` | `AnchorDrawing.simple_pos/simple_pos_point` | Не читаются: такое позиционирование заменяется обычным. |
| P2 | `wp:anchor/@layoutInCell` | `AnchorDrawing.layout_in_cell` | Не читается: привязка к ячейке не отличается от привязки к странице. |
| P2 | `wps:bodyPr/@wrap="none"`, `@vert`, `@upright`, `@rot` | [`TextBoxBody`](../strict-ooxml-wml/src/model/drawing.rs) | Текст в надписи всегда переносится и идёт горизонтально. Также не читаются `numCol`/`spcCol`, `horzOverflow`/`vertOverflow`, `anchorCtr`. |
| P2 | `wp14:pctPosHOffset`/`pctPosVOffset` | `Position.percent_offset` | Не читается. |
| P2 | `wpc:lockedCanvas` | `LockedCanvas.markup/images` | Не читается: содержимое не рисуется. |
| P3 | `a:effectLst` | `Shape.effects` | Тени и свечение не рисуются. |
| P3 | `a:custGeom/a:rect` | `CustomGeometry.text_rect` | Текстовая область своей геометрии не учитывается. |
| P3 | `wp:effectExtent`, `@allowOverlap` | `AnchorDrawing.*` | Не читаются. |
| P3 | `a:blip/@cstate`, `@bwMode` | `BlipRef.cstate`, `Picture.bw_mode`, `Shape.bw_mode` | Не читаются (чёрно-белый режим). |
| P3 | встроенные шрифты (`w:embedRegular`…) | `FontEntry.embeds` | `FontTable::embedded_parts()` не вызывается: встроенные шрифты документа не используются, работает подстановка. |
| P3 | `w:pBdr`/`w:pgBorders` `@w:themeColor`, `@w:shadow`, `@w:frame` | [`Border.theme_color`](../strict-ooxml-wml/src/model/values.rs), `shadow`, `frame`; `PageBorder.shadow` | Тематический цвет границы и тень не применяются. |

## 7. Математика (OMML)

| Pri | OOXML | Модель | Что происходило |
|---|---|---|---|
| P2 | `m:mathPr/m:naryLim`, `m:intLim` | `MathProperties.nary_limit/integral_limit` | Не читаются: размещение пределов по умолчанию для документа игнорируется (работает только `m:limLoc` у оператора). |
| P2 | `m:mathPr/m:dispDef`, `m:defJc`, `m:lMargin`, `m:rMargin`, `m:wrapIndent`, `m:wrapRight`, `m:smallFrac`, `m:brkBin`, `m:brkBinSub`, `m:interSp`, `m:intraSp`, `m:mathFont` | `MathProperties.*` | Не читаются. Из `m:mathPr` используются только `m:preSp` и `m:postSp`. |
| P2 | `m:mcs/m:mc/m:mcPr/m:count` | [`MatrixColumn.count`](../strict-ooxml-wml/src/model/math.rs) | Не читается: выравнивание колонок матрицы применяется к одной колонке вместо `count`. |
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
  `Theme::font` ([`style.rs:691`](../strict-ooxml-render-svg/src/style.rs)).
- `AbstractNum.levels`, `Level.ilvl` — через `AbstractNum::level`
  ([`numbering.rs:185`](../strict-ooxml-render-svg/src/numbering.rs)).
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

## Исходный порядок исправления

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
