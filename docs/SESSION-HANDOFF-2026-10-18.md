# Передача сессии — 2026-10-18: наложения (overlap), размер страницы и два фикса

**Дата:** 2026-10-18 · **Ветка:** master (ahead of origin, ничего не пушилось)
**Тулчейн:** `cargo +1.92.0` (ADR-0011; на машине `stable` = 1.91.1)
**Состояние дерева:** чисто, всё закоммичено. Untracked и не наши: `.claude/`,
`ucimo-1-5.docx` (не коммитить).

---

## 0. Что читать первым

| Файл | Что это |
|---|---|
| `xtool/wps-qa/overlap.py` | метрика наложений и инструмент отчёта по классам |
| `xtool/wps-qa/README.md` §Overlap | как запускать, классы, ограничения |
| `wps-qa/overlap/bin-df04a53867/report.md` | полный отчёт по корпусу (gitignored) |
| `docs/WPS-QA-DISCREPANCIES.md` | реестр расхождений с WPS |
| коммиты сессии | `b3067e9`, `7efcf2d`, `e7ced78`, `e01210c`, `63bd6ee` |

Команды:

```
python xtool/wps-qa/wps_qa.py run      # эталоны WPS + наш рендер + метрики + отчёт
python xtool/wps-qa/wps_qa.py overlap --label <метка> --side both [--corpus C …]
python xtool/wps-qa/overlap.py find <pdf> [page …]     # разбор одного PDF
```

---

## 1. Что сделано за сессию

### 1.1 Размер страницы следует локали (`b3067e9`)
OOXML не задаёт размер секции без `w:pgSz` — его берёт приложение. Word/WPS берут
**локальный** дефолт (US Letter в US, A4 на метрической машине), мы всегда ставили
Letter. Добавлены `DefaultPaper{Letter,A4}` и `RenderOptions::default_paper`;
`geometry_for`/`body_geometry_for` принимают бумагу явно (библиотека остаётся
машинно-независимой, ADR-0006: дефолт — Letter). CLI читает локаль
(`--default-page-size letter|a4|auto`, дефолт `auto`: `GetLocaleInfoW(LOCALE_IPAPERSIZE)`,
вне Windows — A4). На этой машине (`ru-RU`) `to-pdf` теперь даёт A4 (595.3×841.9).

### 1.2 Метрика наложений + инструмент (`7efcf2d`, `e7ced78`)
`overlap.py`: слова — из текстового слоя, картинки и векторные контуры — из объектов
pdfium (`get_bounds()`), пары через grid-обход, классы:
`text-text`, `text-over-lines` (интерлиньяж ≤ 0), `text-image`, `text-image-bg`
(подложка ≥ 40% страницы), `image-image`, `text-path`/`image-path` (`--with-paths`),
`off-page-text`, `off-page-image`. `wps_qa.py overlap` считает `ours` и/или `ref` (WPS)
и сводит по классу/корпусу/документу в `report.md`, `results-*.csv`, `summary.json`
и оверлеи `img/<side>/…png`. Класс, где у нас счёт заметно выше эталонного, — наш
дефект; где сравним — свойство документа.

### 1.3 Колонки: абсурдный `w:space` (`e01210c`)
`sample-files.com-multi-column` содержит `w:cols w:num="2" w:space="360000"` (250″).
Зазор не ограничивался: ширина колонки упиралась в 1 px, смещение второй колонки
= `index*(width+gap)` = 18000 pt. Теперь зазор, не оставляющий колонку (уже половины
её равной доли), считается отсутствующим → дефолт Word 720 twips. Регрессия:
`tests/columns.rs::an_absurd_column_space_keeps_the_next_column_on_the_page`.

### 1.4 Футер из плавающих объектов (`63bd6ee`)
`layout_region` возвращал высоту **потока**; футер только из `wp:anchor`-объектов имеет
поток 0 → ставился у нижней кромки, боксы рисовались **ниже страницы**. Разделены
`height` (поток — обтекание тела и кэш) и `ink_bottom` (самая низкая чернила с
плавающими; по ней ставится футер). Регрессия:
`tests/headers.rs::a_footer_of_floating_objects_keeps_its_text_on_the_page`.
Использовать `ink_bottom` и для обтекания тела нельзя — пагинация взрывается
(020: 62 → 1206), поэтому только позиционирование.

---

## 2. Полный прогон корпуса (метка `bin-df04a53867`)

Рендер: 582 ok, 24 fail (23 CCX + `core-samples/sample-docx-200kb`), overlap по всем
docx-корпусам. Значения — `text-*`/`off-page-*` (число срабатываний).

| класс | ours старое | ours новое | ref (WPS) |
|---|---:|---:|---:|
| **off-page-text** | **3625** | **581** | 268 |
| text-over-lines | 5820 | 5909 | 4022 |
| text-text | 4560 | 4642 | 2515 |
| text-image | 830 | 872 | 511 |
| off-page-image | 170 | 170 | 109 |
| text-image-bg | 156 | 156 | 64 |
| image-image | 42 | 42 | 919 |

`off-page-text: 3625 → 581 (−84%)`. По корпусам (off-page-text):

| корпус | старое | новое | что |
|---|---:|---:|---|
| core-samples | 1521 | **0** | колонки (§1.3) |
| CC0 | 1346 | **11** | футеры 019/020 (§1.4) |
| CC0_DOCX_1 | 51 | **1** | футер 036 |
| CC0_DOCX | 506 | 376 | 064 исправлен; остался `wajiz` |
| CC0_DOCX_MATH | 190 | 190 | `031` |
| core-docx | 11 | 3 | — |

---

## 3. План

### 3.1 Кластер 3 — `off-page-text` влево/вправо (главное, ~440 из 581)
Текст/объект выходит за оба поля:
- `CC0_DOCX/001_wajiz` (272) — RTL (арабица), x и влево (−88), и вправо (714 при A4 595);
- `CC0_DOCX_MATH/031` (166) — один объект уехал до x = −1307 (2× страницы влево) на
  одной странице;
- `…/055`, прочее — мелочь.
Гипотезы: (а) RTL-поля/зеркалирование и (б) размещённый объект (бокс/строка) с
неверным x. Начать с `031` (одна страница, одна сигнатура) и `wajiz` (RTL).
Инструмент проверки — `overlap.py find <pdf> <page>`; эталон — `refs/…`.

### 3.2 «+» в текстовых классах (проверить и починить)
Относительно **старого** нашего рендера: `text-over-lines` +89, `text-text` +82,
`text-image` +42 (≈1–2%). Относительно **WPS**: `text-over-lines` 1.47×, `text-text`
1.85×, `text-image` 1.7× — здесь мы устойчиво хуже. Правдоподобно: футерные текст-боксы
теперь ставятся **на** страницу и частично ложатся на тело (§1.4). Порядок разбора:
- отличить «бокс футера лёг на тело» (не дефект вёрстки, а следствие) от «строки
  схлопнуты» (`text-over-lines`: `CC0_DOCX/065_ldny` — 1018 у нас против 20 у WPS,
  интерлиньяж явно схлопнут);
- `text-text` массово в `CC0_DOCX_1/006` (617), `…/002` (384), `CC0_DOCX_1/057` (256) —
  в основном RTL/арабица/урду; проверить, не двойная ли отрисовка;
- `text-image` в `core-docx/RM0090` и `pdf-docx/RM0090` (56–71), `CC0_DOCX_1/007` (70),
  `CC0_DOCX_MATH/031` (31) — картинка на тексте.

### 3.3 Остальные классы (дальнейший план, из полного отчёта)
- **text-image** ours 872 vs ref 511 (1.7×) — наша расстановка картинок/обтекания.
- **text-image-bg** 156 vs 64 (2.4×) — текст поверх картинки-подложки.
- **off-page-image** 170 vs 109 — картинка за страницей (напр. `CC0/007` 47).
- **image-image** ours 42 vs ref 919 — у WPS **больше**: скорее артефакт WPS, не наша цель.
- **Дефект эталона WPS**: `ref_readability ≈ 15` при нашем ≈ 100 (`strict-smartart`,
  `strict-profile`, `CC0_DOCX_1/074`) и `sample-files.com-multi-column` (WPS извлёк
  1240 симв. из 4723) — эталон сам теряет текст; в такие сравнения не верить.

### 3.4 Находки вне overlap (из первого прогона, `bin-e9e3023a10`)
- Средний fidelity-score 72.9, grade A121/B247/C142/D69/F23; readability ours 92.7 vs
  WPS 88.1; 24 отказа рендера.
- **CCX (23 файла) — это HTML под именем `.docx`** (`<!DOCTYPE html>`), ридер падает
  `invalid ZIP`; WPS их открывает. Класс: нет снифинга HTML/legacy.
- `core-samples/sample-docx-200kb` — отказ на необъявленном префиксе `wp14` в
  `numbering.xml` (exit 2).
- Размер страницы (Letter↔A4) — 5 документов без `w:sectPr` (исправлено в §1.1).
- Content-loss: глифы-квадраты (`hidden_chars`), раздувание страниц — в осн. Tamil/
  Telugu/деванагари; лечится шрифтовыми паками (`to-pdf --fetch-fonts`) или
  `--system-fonts`.

---

## 4. Состояние гейтов

```text
cargo +1.92.0 clippy -p strict-ooxml-render-svg --all-targets   exit 0
cargo +1.92.0 test  -p strict-ooxml-render-svg                  exit 0   57 бинарников
cargo +1.92.0 test  -p strict-ooxml-cli                         exit 0
cargo +1.92.0 test  -p strict-ooxml-wml --test sections         exit 0
```

Прогоны `wps_qa.py` и `overlap.py` — инструменты, не гейты (всегда exit 0 по рендеру).

---

## 5. Дерево

Чисто, всё закоммичено. Коммиты сессии:

```
63bd6ee render: a header/footer of only floating objects is placed by its ink
e01210c render: a column gap too wide to leave a column is ignored, not obeyed
e7ced78 wps-qa: overlap report survives non-ASCII names on a redirected stdout
7efcf2d wps-qa: an overlap metric and a report that groups it by class
b3067e9 render: a missing page size follows the machine's locale, not always Letter
```

Хранилище (`wps-qa/`, gitignored): `ours/bin-df04a53867/` (новый рендер),
`overlap/bin-df04a53867/report.md` (отчёт), `overlap/bin-e9e3023a10/` (предыдущий,
для сравнения ours/ref).

---

## 6. Первый шаг следующей сессии

1. **Кластер 3 (§3.1)**: `overlap.py find wps-qa/ours/bin-df04a53867/CC0_DOCX_MATH/031….pdf 9`
   и по `wajiz`; найти, откуда x объекта = −1307 (размещённый объект/бокс) и почему
   RTL-текст уходит за оба поля. После фикса — перерендерить эти два документа и
   сверить с `refs/…` (off-page-text → 0, страницы не изменились).
2. Затем **«+» в текстовых классах (§3.2)**, начиная с `text-over-lines`
   (`CC0_DOCX/065_ldny`: 1018 против 20 у WPS — схлопнутый интерлиньяж).
3. Прогнать корпус новым бинарём и `wps_qa.py overlap --side both` — ожидание:
   `off-page-text → ~0`, классы §3.3 на месте.

**Конец передачи.**
