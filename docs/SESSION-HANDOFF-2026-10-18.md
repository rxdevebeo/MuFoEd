# Передача сессии — 2026-10-18: кластер 3, RTL-списки, футер по чернилам, полный прогон

**Дата:** 2026-10-18 · **Ветка:** master (ahead of origin, ничего не пушилось)
**Тулчейн:** `cargo +1.92.0` (ADR-0011; на машине `stable` = 1.91.1)
**Состояние дерева:** чисто, всё закоммичено. Untracked и не наши: `.claude/`,
`ucimo-1-5.docx` (не коммитить).

**Это продолжение и завершение сессии того же дня.** Раньше закоммичены
`b3067e9`, `7efcf2d`, `e7ced78`, `e01210c`, `63bd6ee`, `835dc8f` (план §3.1
закрыт), затем — четыре коммита этой части: `b827879`, `0da6d71`, `98e4cc7`,
`89d0fea`. Полный прогон корпуса выполнен (метка `bin-8b133812c2`); следующий
шаг — §6.

---

## 0. Что читать первым

| Файл | Что это |
|---|---|
| `xtool/wps-qa/overlap.py` | метрика наложений и инструмент отчёта по классам |
| `xtool/wps-qa/README.md` §Overlap | как запускать, классы, ограничения |
| `wps-qa/overlap/bin-8b133812c2/report.md` | полный отчёт по корпусу (gitignored) |
| `docs/WPS-QA-DISCREPANCIES.md` | реестр расхождений с WPS (WQ-045 — кластер 3) |
| `strict-ooxml-render-svg/tests/off_page.rs` | опция `--off-page` и метки |
| `strict-ooxml-render-svg/tests/numbering.rs` | RTL-список: отступ уровня и сторона маркера |
| `strict-ooxml-render-svg/tests/headers.rs` | футер по нарисованным чернилам |
| `README.md` §CLI | как запускать `--off-page=keep|clamp`, `--off-page-marks` |

Команды:

```
python xtool/wps-qa/wps_qa.py render [--label L]   # наш рендер в PDF
python xtool/wps-qa/wps_qa.py overlap --label L --side both   # метрика + отчёт
python xtool/wps-qa/overlap.py find <pdf> [page …] # разбор одного PDF
python xtool/wps-qa/overlap.py report --label L [--corpus C …]
```

---

## 1. Что сделано

### 1.1 Размер страницы следует локали (`b3067e9`)
OOXML не задаёт размер секции без `w:pgSz` — его берёт приложение. Word/WPS берут
**локальный** дефолт (US Letter в US, A4 на метрической машине), мы всегда ставили
Letter. Добавлены `DefaultPaper{Letter,A4}` и `RenderOptions::default_paper`;
CLI читает локаль (`--default-page-size letter|a4|auto`, дефолт `auto`).

### 1.2 Метрика наложений + инструмент (`7efcf2d`, `e7ced78`)
`overlap.py`: слова — из текстового слоя, картинки и контуры — из объектов
pdfium (`get_bounds`), классы `text-text`, `text-over-lines`, `text-image`,
`text-image-bg`, `image-image`, `text-path`/`image-path`, `off-page-*`.
`wps_qa.py overlap` сводит по классу/корпусу/документу.

### 1.3 Колонки: абсурдный `w:space` (`e01210c`)
Зазор, не оставляющий колонку, считается отсутствующим → дефолт Word 720 twips.
Регрессия: `tests/columns.rs::an_absurd_column_space_keeps_the_next_column_on_the_page`.

### 1.4 Футер из плавающих объектов (`63bd6ee`)
`layout_region` разделил `height` (поток) и `ink_bottom` (самая низкая чернила);
футер ставится по `ink_bottom` (см. §1.9).

### 1.5 Кластер 3 разобран: текст за листом — фреймы и правые табы (`835dc8f`)
`off-page-text` — плавающие текстовые фреймы (WPS их отбрасывает, мы/LO рисуем;
WQ-045, статус `wps`) и правые табы оглавлений. Добавлены `--off-page=clamp` и
метки off-page-текста.

### 1.6 Опция `--off-page` и метки (`835dc8f`)
`RenderOptions::off_page_ink` = `Keep` | `Clamp`; `off_page_marks` + CLI
`--off-page-marks`. Метка (`Page::off_page`) считается всегда.

### 1.7 `--off-page=clamp` ужимает фрейм шире листа (`b827879`)
Решение по документу §6.1: фрейм, который позиционным сдвигом не ужать, **обрезать
по листу** (не отбрасывать). `clamp_frame` теперь возвращает `FramePlacement`
(origin x/y, ужатые до листа размеры, `off_page`, `clip`). Бокс шире/выше листа
ужимается до листа (переносящийся текст reflow-ится), а его чернила клипаются по
листу (`floating::clip_frame_items` → `Item::Group` с `GroupItem.clip`; работает
и в SVG, и в PDF). Текст вне листа **не теряется**: остаётся в модели и в выводе за
клипом (его можно перенести позже). Метка `floating` остаётся диагностикой.
Для `031` (`wrap="none"`, строка 2526 pt) — это клип: реflow невозможен, а paint-clip
не убирает слова из текстового слоя PDF, поэтому метрика там остаётся (это принято).
Дефолт (`keep`) — байт-в-байт прежний.
Регрессия: `tests/off_page.rs::clamping_clips_a_frame_wider_than_the_page`.

### 1.8 RTL-списки: отступ уровня и сторона маркера (`0da6d71`, `98e4cc7`)
`CC0_DOCX/001_wajiz` (арабица) — первая строка нумерованного списка уезжала вправо
за кромку. Причина: уровень нумерации даёт `w:ind w:start="5400" w:hanging="720"`,
абзац переопределяет только `w:hanging`; отступ уровня применялся не к той
физической стороне (`computed.indent_start_pt` после bidi-свопа — это левый отступ,
а `start` для RTL — правый). Теперь для bidi start идёт в `indent_end_pt`.
Эталон WPS подтверждает: правая граница первой строки = поле − (start − hanging).
Второй фикс — позиция **маркера**: он рисовался по LTR-`first_x` (слева); для bidi
anchored на правом старте (`right_edge - first_offset`), с зеркальной защитой от
наложения. Регрессия: `tests/numbering.rs::an_rtl_lists_first_line_stays_on_the_sheet`
(проверяет и вылет, и сторону маркера; без фиксов падает — rightmost 1092 из 816,
маркер слева). Эффект на wajiz (анализ сохранённых PDF-ов одинаковым инструментом):
over-lines 383 → 374, off-page-text 342 → 299.

### 1.9 Футер ставится по нарисованным чернилам (`89d0fea`)
Регрессия от §1.4: футер позиционировался по `ink_bottom`, а у авто-подгоняемого
VML-текстбокса (номер страницы) сохраняется объявленный прямоугольник **144 pt** —
он невидим (`filled="f" stroked="f"`), но раздувал `ink_bottom` на ~132 px и
поднимал текст футера в тело. Теперь `ink_bottom` считает только чернила, которые
реально рисуются (`headerfooter::paints_ink`: текст/картинка/линия — да; фигура — с
fill/paint/stroke; группа — по детям). `CC0_DOCX_MATH/095`: 118 наложений → **0**;
`CC0/019`, `CC0/020` остаются исправленными (у них чернила видимые).
Регрессия: `tests/headers.rs::a_bare_footer_box_does_not_lift_the_footer_text`.

### 1.10 Проверка §6.2: откуда «+» в текстовых классах
Наложения-«лидеры» оказались артефактами, а не дефектами вёрстки:
- `CC0_DOCX/065_ldny` (text-over-lines) — WordArt-«водяной знак» (автор, 54 pt,
  поворот 45°, `PowerPlusWaterMarkObject` в колонтитуле) поверх тела; WPS рисует его
  контурами (не в текстовом слое) — ожидаемое наложение;
- `CC0_DOCX_1/006`, `002`, `057` (text-text) — боксы арабской вязи (связанные буквы)
  соприкасаются (шум метрики; точных дублей боксов нет); `CC0/019` — две одинаковые
  placeholder-рамки в футере самого документа;
- реальный дефект был один — §1.9 (`095`).

---

## 2. Полный прогон корпуса (метка `bin-8b133812c2`)

Рендер: **885 ok, 24 fail** (23 CCX — HTML под именем `.docx`, + 1), 13
import-failed. Корпус вырос до **922** документов (в прошлой передаче ~606).
Значения — число срабатываний.

| класс | ours | ours без 033 | ref (WPS) |
|---|--:|--:|--:|
| **off-page-text** | **26662** | **2001** | 487 |
| text-over-lines | 12944 | 12909 | 6465 |
| text-text | 9727 | 9703 | 3435 |
| text-image | 1871 | 1871 | 1444 |
| text-image-bg | 342 | 342 | 158 |
| off-page-image | 323 | 323 | 160 |
| image-image | 99 | 99 | 943 |

Один документ — `CC0_DOCX_1/033_Cornelis_de_Bruijn_The_Complete_Works` — даёт
**24661** из 26662 `off-page-text` (см. §3.1). Проверка фиксов этой сессии:
`CC0_DOCX_MATH/095` → 0 (было 118); `CC0/019`, `CC0/020` — `off-page-text` 0
(у ref 6); `CC0_DOCX/001_wajiz` — over-lines 374, off-page 299.

---

## 3. План

### 3.1 `CC0_DOCX_1/033` — абзац не разбивается на страницы (главное)
1214 страниц, 317 с переливом; текст идёт **по одному фрагменту в строке** (ширина
абзаца схлопнута), уезжая на `y ≈ 47 000 px` при листе 1122 px (стр. 736: 448 слов
ниже кромки). Это 92% всего `off-page-text`. Гипотеза: доступная ширина ≈ 0
(объект/фрейм с обтеканием, отрицательный отступ или ячейка нулевой ширины) →
бесконечные переносы без пагинации. Инструмент — `overlap.py find <pdf> 736`,
`render --pages 736-736`.

### 3.2 «+» в текстовом классе `text-image` (1.3×)
`core-docx/RM0090` и `pdf-docx/RM0090`, `CC0_DOCX_1/007`, `CC0_DOCX_MATH/031` (31) —
картинка на тексте: проверить расстановку/обтекание.

### 3.3 Остальные классы (из отчёта `bin-8b133812c2`)
- `text-over-lines` 12944 vs 6465, `text-text` 9727 vs 3435 — во многом те же
  артефакты, что в §1.10 (вязь, водяные знаки, дубли-рамки); `PROBE_027` (+2525) —
  пробный корпус.
- `text-image-bg` 342 vs 158; `off-page-image` 323 vs 160.
- `image-image` ours 99 vs ref 943 — у WPS **больше**: артефакт WPS, не наша цель.

### 3.4 Находки вне overlap
- CCX (23 файла) — HTML под `.docx` (`<!DOCTYPE html>`), ридер падает `invalid ZIP`.
- `core-samples/sample-docx-200kb` — отказ на необъявленном префиксе `wp14`.
- Content-loss: глифы-квадраты, раздувание страниц — лечится шрифтовыми паками
  (`fonts install <pack>`); для `CC0/089` пак `cjk` убрал перелив (9 → 1 строка).

---

## 4. Состояние гейтов

```text
cargo +1.92.0 clippy -p strict-ooxml-render-svg -p strict-ooxml-render-pdf -p strict-ooxml-cli --all-targets   exit 0
cargo +1.92.0 test   -p strict-ooxml-render-svg -p strict-ooxml-render-pdf -p strict-ooxml-cli                 exit 0
cargo +1.92.0 test   -p strict-ooxml --all-features                                                            exit 0
```

`cargo fmt` применён к затронутым файлам (посторонний реформат откачен: `paper.rs`,
`headers.rs`, `render.rs`, `sections.rs` остаются с дореформенными diffs, не нашими).

Прогоны `wps_qa.py`/`overlap.py` — инструменты, не гейты (всегда exit 0 по рендеру).

---

## 5. Дерево

Чисто, всё закоммичено. Коммиты этой части сессии:

```
89d0fea render: a footer is placed by the ink it draws, not a bare box outline
98e4cc7 render: an RTL list marker sits on the start (right) side too
0da6d71 render: an RTL list's level indent lands on the start (right) side
b827879 render: --off-page=clamp confines a frame wider than the sheet to it
```

Ранние коммиты дня: `835dc8f`, `63bd6ee`, `e01210c`, `e7ced78`, `7efcf2d`,
`b3067e9`.

Хранилище (`wps-qa/`, gitignored): `ours/bin-8b133812c2/` (новый рендер, 922 док),
`overlap/bin-8b133812c2/report.md` (отчёт этой сессии); прежние
`ours/bin-df04a53867`, `overlap/bin-df04a53867`, `overlap/bin-e9e3023a10` — для
сравнения. Машина: установлен пак шрифтов `cjk` (`fonts install cjk`).

---

## 6. Первый шаг следующей сессии

1. **`CC0_DOCX_1/033` (§3.1)** — 24661 из 26662 `off-page-text`. Начать со стр. 736:
   почему ширина абзаца схлопнута (один фрагмент на строку → нет пагинации).
2. Затем **`text-image` (§3.2)** — картинка на тексте (`RM0090`, `007`, `031`).
3. При желании — перемерить корпус после фикса `033` (`wps_qa.py render` +
   `overlap --side both`) и сверить с `bin-8b133812c2`.

**Конец передачи.**
