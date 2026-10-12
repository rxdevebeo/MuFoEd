# План закрытия census и WPS-геометрии по влиянию на визуал

Дата: 6 октября 2026.  
Основание: [STATUS.json](audit-remediation-2026-10-06/STATUS.json), [AUDIT_REMAINING_WORK_2026-10-06.md](AUDIT_REMAINING_WORK_2026-10-06.md), inventory `audit-remediation-2026-10-06/census-inventory.json.gz`, диагностика `wps-absolute-baselines.json`.  
Машинный разбор свидетелей: [plan-witnesses.json](audit-remediation-2026-10-06/plan-witnesses.json) (пересобирается `_plan_witnesses.py`).

**Итог измерения D05:** 221/221 документов, `unmatched_schema=0`, `ours=0`, **10 486** строк inventory без disposition (537 labels, 205 документов).  
**Итог измерения D06:** WPS OPEN, допуск **0,25 px**, ledger неполный; Word = NOT_RUN.

## Статус на 2026-10-08 (сверка с кодом master)

Все коммиты пакетов (`35f346a`, `0e11a21`, `ac51b68`, `f96f99c`, `67eafbf`, `68813f5`,
`7ea3fd9`, `462882a`) в master; у каждого пакета `audit-remediation-2026-10-06/PNN-*/STATUS.json`.

| Пакет | Статус | Что осталось |
|---|---|---|
| P0, P2–P7, P9–P11, P13 | сделано (ACCEPT/PASS) | P2: визуальный допуск среза 0,75 px; P3/P11: `descr` у `wp:docPr`/`pic:cNvPr` перешёл в хвост P15; P5: линии VML-групп выбрасываются (`T7.vml-group`), waiver нет; P5/P6: тесты на RM0090 пропускаются без локального корпуса — нужны синтетические двойники |
| P1 | сделано, перепроверено 2026-10-08 | перепроверка (hostq `wps`) нашла регрессию от `9d9b372` на с. 56 (подписи в таблице с `framePr`, до 0,67 px), исправлено `93b054a`: PASS 26/26 ≤ 0,25 px с `--wps-times`; запас `p104.modern.5` по-прежнему 0,002 px; без калибровки — до 3,4 px (по замыслу); Rust-теста на Clio нет (корпус локальный) |
| P8 | сделано | цветовых строк в census нет: `a14:hiddenFill` уходит по ADR-0014 с цитатой `a:ext`, `a:sysClr`/`prstClr`/модификаторы пишутся исходным элементом (P15); тесты T-P8-1..3 в `p7_p8.rs` |
| P12 | закрыт (кроме VML) | footnote/endnote −1 закрыт; `headerReference@id`/`footerReference@id` 139 → 10 частей: semantic digest (Strict-написания, MCE, текст надписей, `w:sym@char`), писатель сохраняет `w:cs`, `autoSpaceDE/DN`, `adjustRightInd`; остаток — VML-линии/фигуры (T7), уходит в VML-группы |
| P14 | сделано, перезамер | после `5446135` (SDT-свойства сохраняются) `w:dataBinding`/`w:text` должны уйти; TZ-32 в `census.toml` устарел |
| P15 | частично | census на хосте `b3bed99`: unclassified 633 → 108, unmatched_schema 0, ours 0; остаток — VML (`w:txbxContent`, `o:rules`, hdr/ftr), внешние картинки `a:blip@link`, `c:chart@id`, тема (`a:reflection`, `a:hueOff`), `w:br@clear`, `customMarkFollows`, поля форм, одиночные |

Отдельно: ~~`unmatched_schema = 1198`~~ → **0** (census на `3332800`, 2026-10-08): все 1198 были
процентами DrawingML/диаграмм, которых не было в T4 нормализатора (`a:spcPct` 1161, `a:miter@lim` 30,
`a:buSzPct` 7 — тысячные; `c:lblOffset`/`c:gapWidth`/`c:overlap` — целые проценты, новое правило
`T4.chart-percent`); census сравнивает целые проценты диаграмм как одно значение. Unclassified — 763. Вехи: M1 PASS,
M2 PASS (срез), M3 ACCEPT, M4 OPEN, M5 не начат (Word NOT_RUN). Waiver на остатки P8/P12/P15
нет; `CENSUS-LOCAL` устарел — ночной census уже в CI (`ci.yml`), `census-baseline.json` не закоммичен.

Порядок дальше: ~~перепроверка P1~~ → ~~синтетические двойники P5/P6~~ → база census и `CENSUS-LOCAL` →
~~`unmatched_schema`~~ → ~~P12~~ → P15 по метке (72) → ~~P8~~ → ~~VML-группы~~ → M5 (Word).

Этот план не заменяет критерии приёмки D05/D06. Он задаёт **порядок работ по визуальному ущербу** и фиксирует, **на каких документах** каждый пункт измерять.

---

## 1. Принцип приоритета

Приоритет = влияние на то, что видит пользователь (положение, размер, цвет, шрифт, картинки), а не число строк census.

| Уровень | Значение |
|---|---|
| **V0** | Уже измеренный визуальный FAIL (WPS ≤0,25 px нарушен) или неизбежный сдвиг геометрии рисунка |
| **V1** | Меняет layout текста/таблиц/картинок на многих страницах |
| **V2** | Меняет цвет/заливку/тему заметно, но без сдвига геометрии |
| **V3** | Может менять глифы/метрики шрифта или identity ресурсов |
| **V4** | Почти невидимо; блокирует только census gate |

Правила исполнения:

1. Не закрывать класс registry disposition’ом, пока нет положительного и отрицательного контроля.
2. Для DrawingML/WPS сначала доказать **эквивалентность координат** (или исправить), потом писать disposition.
3. Не поднимать допуск 0,25 px; ambiguous match ≠ PASS.
4. Не возвращать `*` / `a:*`.
5. После каждого V0–V1 пакета — census inventory на затронутых документах + целевые SVG/PDF замеры.

---

## 2. Сводка очереди

| # | Пакет | Визуал | Строк inventory | Док-ов | Карточки | Цель пакета |
|---|---|---|---:|---:|---|---|
| P0 | WPS ledger + matcher | **V0** | — (рендер) | 1 (Clio) | D06 | Сделать геометрию измеримой |
| P1 | WPS FAIL: baseline Y / SNP X / ритм | **V0** | — | Clio 54/56/104 | D06 | ≤0,25 px на обязательных точках |
| P2 | DrawingML `a:off`/`a:ext`/`chOff` | **V0–V1** | 1 560 | 14 | D05↔D06 | Координаты рисунков сохраняются |
| P3 | `wp:` extent/position/wrap | **V1** | 461 | 52 | D05↔D06 | Якоря и размеры inline/anchor |
| P4 | WPS `bodyPr` / text box props | **V1** | 366 | 26 | D05↔D06 | Text-box поведение не теряется |
| P5 | Метрики run/абзаца | **V1** | 1 324 | 101 | D05 | sz/spacing/ind/tabs |
| P6 | Табличные ширины/границы | **V1** | 503 | 55 | D05 | tcW/tblW/gridCol |
| P7 | themeColor / themeTint / shade | **V2** | 1 027 | 57 | D05 | Цвет после resolve темы |
| P8 | сделано | цветовых строк в census нет: `a14:hiddenFill` уходит по ADR-0014 с цитатой `a:ext`, `a:sysClr`/`prstClr`/модификаторы пишутся исходным элементом (P15); тесты T-P8-1..3 в `p7_p8.rs` |
| P9 | Шрифты / hints | **V3** | 510 | 125 | D05/D09 | rFonts/hint/charset/panose |
| P10 | Бинарные media | **V3** | 84 | 34 | D05 | SHA картинок/шрифтов |
| P11 | graphicData URI / pic identity | **V3–V4** | 333 | 99 | D05 | URI и cNvPr |
| P12 | Header/footer rel + footnote −1 | **V4** | 422 | 112 | D05 | Semantic rel / sep ids |
| P13 | Ignorable hdr/ftr/notes | **V4** | 387 | 100 | D05 | Узкое расширение TZ-46 |
| P14 | SDT/docPart (unnamed) | **V4** | 405 | 54 | D05 | Cite named_loss или сохранить |
| P15 | частично | census на хосте `b3bed99`: unclassified 633 → 108, unmatched_schema 0, ours 0; остаток — VML (`w:txbxContent`, `o:rules`, hdr/ftr), внешние картинки `a:blip@link`, `c:chart@id`, тема (`a:reflection`, `a:hueOff`), `w:br@clear`, `customMarkFollows`, поля форм, одиночные |

\* Часть строк styles/settings пересекается с theme/metrics; после P5–P8 объём уменьшится. Не закрывать N_styles blanket’ом.

**Важно:** у Clio в census почти нет DrawingML placement (0 строк A_placement). WPS FAIL на 54/56/104 — в основном **layout/render**, не writer inventory. P2–P4 всё равно обязательны: они валят census и портят визуал других 14–52 документов.

---

## 3. Протокол замеров (общий)

### 3.1 Census per-package

Для каждого пакета после фикса:

```powershell
# полный корпус (приёмка)
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build `
  --quiet-messages --keep-written target/census-written `
  --inventory-out target/census-inventory.json

# срез по свидетелям пакета (ежедневный)
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build `
  --quiet-messages --keep-written target/census-written-slice `
  --inventory-out target/census-inventory-slice.json
```

Фиксировать в receipt пакета:

- `unclassified_element_changes` всего и **только labels пакета**;
- `missing`, `unmatched_schema`, `ours`;
- SHA исходных и записанных DOCX свидетелей;
- список новых/изменённых `TZ-*` items.

### 3.2 Визуал SVG/PDF

| Режим | Когда | Метрика |
|---|---|---|
| WPS absolute glyph origin | P0–P1 | dx, dy ≤ 0,25 px; ambiguous отдельно |
| Structural / bbox для рисунков | P2–P4 | origin/extent рисунка в SVG и PDF |
| Text metric / line width | P5 | half-points / line spacing |
| Color sample | P7–P8 | resolved sRGB в paint vs theme |
| Image hash / pixel ROI | P10 | SHA media + ROI SSIM при необходимости |

Отрицательные контроли обязательны: искусственный сдвиг EMU / смена themeColor / смена media SHA должны давать FAIL.

### 3.3 Минимальный fixture на пакет

Каждый пакет добавляет:

1. **Unit/integration** в соответствующем crate (parse → model → write → compare).
2. **Negative control** (чужое значение не считается эквивалентным).
3. **Corpus witness list** из таблицы ниже (не «весь 221», а перечисленные DOCX + полный прогон на закрытии пакета).

---

## 4. Пакеты работ

### P0 — WPS ledger и matcher (V0, блокер измерения)

**Проблема.** Диагностика есть, полного ledger нет. Ключи `HVR-I`, `L15996`, `H16142`, `L16117` (стр. 54), `H` (56), `modern` (104) = `AMBIGUOUS_OR_MISSING`. Пока они неизмеримы, PASS по 0,25 px формально невозможен.

**Свидетель.**  
`strict-ooxml-core/tests/docx/Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx`  
SHA-256: `4c5b9b178bdc8c3abae865f00ab5aaa9e102f81634e53691afc3cd3073162462`  
Страницы: **54, 56, 104**. Reference: сохранённые WPS PDF/SVG из remediation; Word отдельно (`word-export-request.json`).

**Тесты / артефакты.**

| ID | Что сделать | Где |
|---|---|---|
| T-P0-1 | Ledger JSON: page → component_id → {text, role, ref_xy, actual_xy, font, transform} | `xtool/` или `docs/audit-remediation-…/wps-ledger.json` |
| T-P0-2 | Matcher: уникальность по (page, text, neighborhood); split runs склеивать; substring-first запрещён | тест comparator |
| T-P0-3 | Статусы: MEASURED / AMBIGUOUS / MISSING; AMBIGUOUS не PASS | selftest |
| T-P0-4 | Negative: сдвиг одного origin на 1 px → FAIL | selftest |

**Замер корпуса.** Только Clio 54/56/104 + hashes PDF/SVG из `wps-absolute-baselines.json`. Полный 221 не нужен.

**Критерий выхода.** Все обязательные компоненты имеют статус MEASURED или явно вынесены из обязательного набора с обоснованием. Нет «PASS по оценке baseline».

---

### P1 — Исправить измеренные WPS FAIL (V0)

**Проблема (из `wps-absolute-baselines.json`).**

| Стр. | Класс | Ключи | Отклонение |
|---|---|---|---|
| 54 | Вертикальный ритм / центрирование | `L16055`, `H16233` | dy ≈ **−3,88…−3,93** px; dx в допуске |
| 56 | Подозрительный missmatch | `-M` | dy ≈ **−35,7** px — сначала подтвердить matching |
| 56 | Ритм клад | `8994`, `6371`, `11719`, `14766`, `7028` | dy ≈ **−0,52…−0,67** px |
| 104 | SNP / подписи по X | `A,C`, `B,D` | dx ≈ **+3,06 / +5,14** px |

**Владение.** `strict-ooxml-render-svg` layout/paint; при необходимости WML drawing parse; PDF glyph path в `strict-ooxml-render-pdf` / `strict-ooxml-pdf`.

**Тесты.**

| ID | Содержание |
|---|---|
| T-P1-1 | Per-point asserts ≤0,25 px для уникальных ключей 54/56/104 |
| T-P1-2 | Подписи внутри границ страницы |
| T-P1-3 | Glyph Metric/Shaped и ToUnicode regression (уже есть для `office`) не ломаются |
| T-P1-4 | Negative: принудительный offset в layout → FAIL gate |

**Замер.** Повторить absolute-baseline диагностику на тех же PDF/SVG SHA входах; обновить ledger. Census на Clio — контроль, что write не ухудшил inventory (сейчас 21 строка, в основном metrics/styles).

**Критерий выхода.** Все обязательные MEASURED точки ≤0,25 px; ambiguous = 0 среди обязательных; D06 READY_FOR_ACCEPTANCE (Word всё ещё отдельно).

---

### P2 — DrawingML placement `a:off` / `a:ext` / `chOff` (V0–V1)

**Inventory.** 1 560 строк / 14 документов. Топ labels: `a:off@y` 640, `a:off@x` 517, `a:ext@cx` 214, `a:ext@cy` 179.

**Главный свидетель.** `070_Innovations_and_New_Technologies.docx` — **1 473** строки (≈94% класса).  
Вторичные: `RM0090 16-23 Справочное руководство по STM32F4xx.docx` (30), `009_stratigrafi_regional_daerah_kulon_progo_diy.docx` (10), серия SoftUni `1. First-Steps…`–`8.…` (по 4).

**Гипотеза причины.** Writer пишет `a:off` из модели, `a:ext` часто из соседнего extent (у picture — `picture.extent`), group `chOff`/`chExt` теряются/пересчитываются. Нужна эквивалентность **конечных** координат после group transform, не совпадение сырого EMU.

**Тесты.**

| ID | Содержание |
|---|---|
| T-P2-1 | Roundtrip: source `a:xfrm` offset/extent → write → compare EMU или доказанный group-equivalent |
| T-P2-2 | Nested group: `chOff`/`chExt` сохраняются |
| T-P2-3 | Negative: изменение x на 1 EMU → inventory FAIL / test FAIL |
| T-P2-4 | SVG/PDF bbox рисунка на странице для `070_…` (минимум 3 фигуры) |

**Замер корпуса.**

1. Slice: `070_…`, `RM0090…`, `009_…`, `1. First-Steps…`.
2. Считать только labels `a:off@*`, `a:ext@*`, `a:chOff*`, `a:chExt*`.
3. Полный 221 — на закрытии пакета.

**Критерий выхода.** Строк класса A_placement = 0 **или** каждая оставшаяся строка имеет узкий declared_transform с доказанной эквивалентностью координат + behavioral witness.

---

### P3 — `wp:` position / wrap / extent (V1)

**Inventory.** 461 / 52 док. Labels: `wp:extent@cx/cy`, `wp:inline@dist*`, `wp:positionV`, `wp:effectExtent@*`, `wp:docPr@*`.

**Свидетели.** `070_…` (60), `009_…` (37), `077_2016_Greater_Launceston_…` (36), SoftUni `4. Complex-Conditions.docx` (19), `DOCX_46_Pages_Large_…` (20).

**Тесты.** T-P3-1 inline extent roundtrip; T-P3-2 floating `positionH/V` + wrap; T-P3-3 effectExtent zero vs non-zero; T-P3-4 visual wrap distance на одном якоре `009_…` или SoftUni.

**Замер.** Slice из 5 свидетелей; labels `wp:*`. Визуал: положение картинки относительно текста (не только inventory).

---

### P4 — WPS `bodyPr` / text-box (V1)

**Inventory.** 366 / 26 док. Labels без префикса (`bodyPr@wrap`, `@vert`, `@anchorCtr`, `@rot`, `cNvSpPr@txBox`) — namespace `wordprocessingShape` не в `NS_PREFIX`.

**Свидетели.** `PEP - DVOJEZIČNI -IZJAVA…` (42), `018_SINGING_AND_ENCHANTING_…` (25), `RM0090…` (21), SoftUni series (16 каждый).

**Тесты.** T-P4-1 parse/write `wps:bodyPr` ключевых attrs; T-P4-2 text box vert/wrap влияет на SVG; T-P4-3 negative: drop `wrap` при отличии от default → FAIL/report.

**Замер.** Slice 4 документа; визуально — один text box с non-default `vert`/`anchorCtr`.

---

### P5 — Метрики абзаца/run (V1)

**Inventory.** 1 324 / 101 док. Labels: `w:w@val` 185, `w:sz@val` 169, `w:szCs` 133, `w:spacing@line` 132, `w:spacing@after` 97, `w:tab@pos` 80, `w:ind@left` 75.

**Свидетели.** `RM0090…` (296), `018_SINGING_…` (250), `064_40_Islamic_Books.docx` (111), SoftUni series (~20).

**Тесты.** T-P5-1 sz/szCs half-point roundtrip; T-P5-2 spacing line/after после T2 twip↔pt; T-P5-3 tab pos; T-P5-4 text-metric probe на абзаце из `RM0090` (ширина/межстрочие).

**Замер.** Slice 3 тяжёлых + 1 SoftUni; полный corpus labels `w:sz*`, `w:spacing*`, `w:ind*`, `w:tab@*`, `w:w@val`.

---

### P6 — Таблицы (V1)

**Inventory.** 503 / 55. Labels: `w:tcW@w` 173, `w:tblW@w` 61, `w:gridCol@w` 59, borders width.

**Свидетели.** `RM0090…` (90), `070_…` (90), `033_Nghiep_cam_but_chi.docx` (32).

**Тесты.** T-P6-1 gridCol/tcW roundtrip; T-P6-2 table bbox SVG на странице с таблицей; T-P6-3 negative: сжатие колонки на 1 twip → FAIL.

**Замер.** Slice 3 документа; визуал — ширина колонок/ячеек.

---

### P7 — themeColor / themeTint / themeShade (V2)

**Inventory.** 1 027 / 57. Часто по **46** строк на sample-files/Contoso-like документы.

**Свидетели (по 46):** `Contoso_Guest_WiFi_Connection_Guide.docx`, `Digital_Marketing_Service_Agreement.docx`, `sample-files.com-table-document.docx`, `sample-simple.docx`, SoftUni `1. First-Steps…` (theme на borders/rPr).

**Тесты.** T-P7-1 resolve theme → sRGB равен исходному resolved; T-P7-2 lexical themeColor drop при том же hex → declared_transform только с color witness; T-P7-3 negative: accent1→accent2 FAIL.

**Замер.** Slice 4 sample + 1 SoftUni; paint color sample (не только XML).

---

### P8 — Явные цвета (V2)

**Inventory.** 351 / 68. `w:color@val` 232, `a:schemeClr@val` 65.

**Свидетели.** `018_SINGING_…` (21), `081_Lab_De_Uniones.docx` (12), Contoso/sample series (10).

**Тесты.** T-P8-1 hex case-insensitive already OK — проверить real value change; T-P8-2 `auto`/`000000` сохранение политики; T-P8-3 schemeClr resolve.

**Замер.** Как P7, labels без theme*.

**STATUS 2026-10-09.** Закрыт вместе с P15: census `b3bed99` не содержит ни `w:color`, ни `a:srgbClr`,
`a:sysClr`, `a:schemeClr@val`, `w:shd`. Попутно: T4.measure терял знак значения меньше пункта
(`tblInd w="-5"` → `0.25pt`, отступ таблицы уезжал вправо); линии фигур держат `cap`/`cmpd`/`algn` и
соединение (`a:round`/`a:miter`), группы — `a:grpSpLocks` и `bwMode`, соединители остаются
`wps:cNvCnPr` с `a:cxnSpLocks`; census читает голый `w:vMerge` как `continue`.

---

### P9 — Шрифты / hints (V3)

**Inventory.** 510 / 125. `themeFontLang`, `rFonts@hint/ascii/hAnsi/cs`, panose/pitch/charset связаны со Stage-5C.

**Свидетели.** `003_HTyn_…` (26), `main_document.docx` / `sub_document.docx` (14), `014_BG_Slokas_…` (12), Strict `05-strict-math-simple` (corpus contract).

**Тесты.** T-P9-1 сохранение hint-элементов fontTable **или** named_loss с feature id; T-P9-2 stage5c_corpus GREEN без удаления assertions; T-P9-3 negative: silent drop charset → FAIL.

**Замер.** Slice + `stage5c_corpus` + census labels font*.

---

### P10 — Бинарные ресурсы (V3)

**Inventory.** 84 / 34. В основном `word/media/image*.png`.

**Свидетели.** `018_SINGING_…` (35), `070_…` (7), SoftUni (по 2).

**Тесты.** T-P10-1 media SHA identity; T-P10-2 rename part без смены bytes OK; T-P10-3 recompress без доказательства = FAIL.

**Замер.** Slice; ROI pixel compare для одного изменённого PNG при необходимости.

---

### P11 — graphicData URI / pic identity (V3–V4)

**Inventory.** 333 / 99. `a:graphicData@uri` 110, `pic:spPr@bwMode` 74, cNvPr name/descr.

**Свидетели.** `031_toki_soweli_…` (21), `009_…` (13), SoftUni.

**Тесты.** T-P11-1 канон известной URI-пары; T-P11-2 смена URI на другой vocabulary FAIL; T-P11-3 bwMode default drop только если доказано no-op.

---

### P12 — Header/footer relationship digest + footnote/endnote id −1 (V4)

**Inventory.** 422 / 112. Почти все `named=1`.

**Свидетели.** `003_HTyn_…` (24), `Programming-Basics-CSharp-Book-…` (13), CC0 с hdr/ftr.

**Тесты.** T-P12-1 semantic header part equality (не SHA регенерированного XML); T-P12-2 separator id −1 disposition; T-P12-3 negative: чужой footnote id не маскируется.

**Визуал.** Низкий, если содержимое hdr/ftr сохранено — проверить один header text roundtrip.

**STATUS 2026-10-09.** census на `a4a136e`: unmatched_schema 0, unclassified 646 → 633. Digest
(`_semantic_part_digest`) сравнивает смысл части: Strict-написания (`start/end`, твипы, on/off, `tblLook`,
pct), выбранную ветку `mc:AlternateContent`, рисунок — по тексту надписей, `w:sym@char` с ремапом F0xx.
Писатель терял `w:cs` (024, 038, 066, 097) и выключенные `autoSpaceDE/DN`/`adjustRightInd` (070) —
исправлено, тест `strict-ooxml-write/tests/p12_headers.rs`. Осталось 10 частей (001, 065, 090, 100):
VML-линии и фигуры без надписи, которые T7 теряет, — это пункт «VML-группы». Дампы частей пишутся в
`census-reports/semantic-parts/` при `--write-reports`.

---

### P13 — Ignorable на hdr/ftr/notes (V4)

**Inventory.** 387 / 100, все named. TZ-46 закрывает document/styles/fonts/settings, **не** hdr/ftr/footnotes/endnotes/glossary.

**Свидетели.** Те же, что P12.

**Тесты.** T-P13-1 узкое расширение disposition; T-P13-2 negative: произвольный Ignorable prefix не принимается.

---

### P14 — SDT / docPart unnamed (V4)

**Inventory.** 405 / 54. `docPartGallery/Unique/Obj`, `sdtEndPr`. TZ-32 требует `named=1`, writer часто не цитирует.

**Свидетели.** SoftUni `1…8` (по 22).

**Тесты.** T-P14-1 cite loss в report **или** preserve; T-P14-2 negative: потеря пользовательского SDT alias без cite FAIL.

---

### P15 — settings / styles / numbering / хвост (V4)

**Inventory.** settings 658, styles+numbering ~2 079 (с пересечениями theme/color), Z_other ~1 722 (`a:ext` в extLst, hyperlink@history, `w:b`, custom Properties, `w:txbxContent`, …).

**Порядок внутри P15:** (1) residual theme/color в styles после P7–P8; (2) `w:style@customStyle`, `w:lvl@tentative`; (3) settings compat; (4) Z_other по убыванию labels.

**Замер.** Полный 221; цель `unclassified_element_changes=0`.

**STATUS 2026-10-09.** 633 → 108 (`b3bed99`), unmatched_schema 0, ours 0. Сделано:
писатель держит alt text с переводами строк (атрибут переписывается со ссылками
`&#10;`), пустой `w:compat`, все `CT_OnOff` из settings, `w:subsetted="0"`, колонки
закладок, `tblHeader w:val="0"`, `suppressOverlap`, `specVanish`, `w:cs`,
`autoSpaceDE/DN`, `adjustRightInd`, `w:aliases`, имя `abstractNum`, весь `w:tblPrEx`,
`docGrid@charSpace` (беззнаковая запись Word = знаковый шаг), `pBdr` `between`/`bar`;
в рисунках — `simplePos`, `anchor@hidden`, атрибуты `cNvPicPr`/`picLocks`/`blipFill`,
`a:spLocks`, дети `a:blip` (эффекты), остаток `pic:spPr` (геометрия, линия, заливка),
эффекты фигур, исходный элемент цвета (`lumMod`/`lumOff`, `alpha`, `sysClr`,
`prstClr`), `a:rect` пользовательской геометрии, `a:lin@scaled`,
`gradFill@rotWithShape`; в теме — сам `a:fmtScheme` вместо заглушки, `a:font` по письменностям и
`a:sysClr`. Сохранённая разметка пишется в той же форме, что и писатель (fixed point). Расширения Office (`a14` в `a:extLst`) по ADR-0014 не
пишутся и называются в отчёте (`a:ext`). Census: `val="1"` → голый `CT_OnOff`,
повторный `proofState`, `documentProtection` (TZ-52/53), раскрытый `smartTag`
(TZ-54), пары пространств `lc`/`cdr`/`dgm`, точное совпадение имён для TZ-13;
при `--write-reports` — `unclassified.txt` и записанные пакеты в `written/`.

**VML-группы, 2026-10-09** (ветка `task/vml-groups-2026-10-09`): 92 → 72, колонтитулов в остатке нет. Конвертированные VML-фигуры
получают заливку и обводку по атрибутам VML (`fillcolor`/`filled`, `strokecolor`/`strokeweight`/`stroked`,
`v:fill`, `v:stroke`, прозрачность, пунктир) с умолчаниями VML — до этого 721 `v:rect` и все надписи
выходили невидимыми рамками. `v:line` (55) и прямые соединители (14) — `prstGeom line` в своём габарите с
отражением; `v:roundrect`, `v:oval`. Члены `v:group` — не только надписи, но и линии и простые фигуры, и все
ставятся со смещением самой группы. WordArt-водяной знак (`v:textpath`) — повёрнутая надпись со строкой,
шрифтом, размером и цветом заливки; `rotation` сохраняется. `mso-position-*-relative:text` — колонка и абзац,
не страница. Census: `w:txbxContent` не зависит от обёртки (`v:textbox`/`wps:txbx`), члены группы в одном
прогоне — один рисунок, рисунок колонтитула — его текст. Pixels на `ae506cd` зелёные.
Остаток VML: `o:rules`/`o:shapedefaults` в settings, VML-маркеры списков (`w:numPicBullet`).

Остаток 108 (до VML-групп): VML (`w:txbxContent` 14, `o:rules`, `shapedefaults`, hdr/ftr 3+3); внешние картинки `a:blip@link` 5; `c:chart@id` 6; тема
`w:br@clear` 4; `customMarkFollows` 2; поля форм
(`w:checkBox`, `w:textInput`, …) 7; прочие одиночные.

---

## 5. Матрица свидетелей (короткий список для ежедневных прогонов)

Минимальный набор документов, покрывающий V0–V2 без полного 221:

| Документ | Корпус | Пакеты |
|---|---|---|
| Clio Der Sarkissian…2011.docx | docx | P0, P1 |
| 070_Innovations_and_New_Technologies.docx | cc0 | P2, P3, P6, P10 |
| RM0090 16-23 … STM32F4xx.docx | samples/docx | P2, P4, P5, P6 |
| 009_stratigrafi_regional_daerah_kulon_progo_diy.docx | cc0 | P2, P3, P4 |
| 018_SINGING_AND_ENCHANTING_…docx | cc0 | P4, P5, P8, P10 |
| 1. First-Steps-in-Programming.docx | samples | P2–P5, P7, P11, P14 |
| Contoso_Guest_WiFi_Connection_Guide.docx | samples | P7, P8, P15 |
| sample-files.com-table-document.docx | samples | P6, P7 |
| 003_HTyn_…docx | cc0 | P9, P12, P13 |
| 05-strict-math-simple (Strict) | strict | P9 / Stage-5C |

Полный список top-docs по классам — в `plan-witnesses.json`.

---

## 6. Порядок исполнения и milestones

```
P0 matcher/ledger
 → P1 WPS FAIL (параллельно можно готовить T-P2 fixtures)
 → P2 off/ext  (с визуалом на 070_)
 → P3 wp: + P4 bodyPr
 → P5 metrics + P6 tables
 → P7/P8 colors
 → P9 fonts (Stage-5C)
 → P10 media
 → P11–P15 census cleanup
 → полный census 221 + WPS ledger + D09 acceptance
```

| Milestone | Условие |
|---|---|
| **M1** | P0+P1: WPS обязательные точки ≤0,25 px |
| **M2** | P2–P4: placement inventory = 0 или доказанные transforms; 070_/SoftUni visual bbox OK |
| **M3** | P5–P8: text/table/color без silent visual change на slice |
| **M4** | P9–P15: `unclassified_element_changes=0`, census PASS |
| **M5** | D09: полный acceptance run; Word остаётся NOT_RUN до эталона |

Коммиты/push — только по отдельному разрешению владельца. CI exact-SHA — после M4/M5.

---

## 7. Форма receipt на пакет

Для каждого закрытого пакета каталог `docs/audit-remediation-2026-10-06/PNN-<slug>/`:

```
STATUS.json          # package, visual_level, before/after counts, exit codes
witnesses.json       # documents + source/output sha256
inventory-before.json.gz
inventory-after.json.gz
tests.md             # команды и результаты T-PNN-*
visual/              # SVG/PDF snippets или absolute-baseline diff
registry-diff.patch  # если менялся census.toml
```

Поля `STATUS.json` минимум: `package`, `visual_level`, `rows_before`, `rows_after`, `labels`, `witness_docs`, `tests_pass`, `corpus_slice_exit`, `full_census_exit` (`NOT_RUN` допустим до M4).

---

## 8. Что не делать

- Не объявлять D06 PASS по SSIM без absolute ledger.
- Не подгонять expected координаты и не поднимать 0,25 px.
- Не закрывать P2 disposition’ом «тег a:off есть».
- Не считать lexical themeColor drop эквивалентностью без resolve.
- Не смешивать WPS-приёмку с Word.
- Не тратить первую очередь на P12–P15, пока открыты V0/V1.

---

## 9. Связь с карточками

| Карточка | Пакеты |
|---|---|
| D06 | P0, P1 (+ визуальные части P2–P4) |
| D05 | P2–P15 |
| D09 / Stage-5C | P9, затем полный прогон после M4 |
| D07 Word | вне этого плана; `word-export-request.json` |

После выполнения M1–M4 обновить `AUDIT_REMEDIATION_…` / `STATUS.json`; исторические R-карточки не переписывать.
