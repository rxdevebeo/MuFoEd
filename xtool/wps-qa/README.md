# WPS reference QA

Снимает эталон каждого документа всех docx-корпусов в WPS Writer (COM-сервер
`KWPS.Application`, WPS 12.1) и измеряет наш рендер относительно него —
**метриками, а не попиксельно**. Это QA-инструмент, а не гейт: он публикует
измерения и никогда не возвращает pass/fail за рендер.

## Требования

- Windows с установленным WPS Office (`KWPS.Application` в реестре);
- Python 3.11 с `pywin32`, `psutil`, `pypdfium2`, `numpy`, `scikit-image`, `Pillow`
  (`pip install -r xtool/wps-qa/requirements.txt`);
- собранный CLI: `cargo build --release -p strict-ooxml-cli`.

## Запуск

```
python xtool/wps-qa/wps_qa.py run                      # всё: capture -> render -> compare -> report
python xtool/wps-qa/wps_qa.py capture [--corpus CC0]   # только эталоны WPS
python xtool/wps-qa/wps_qa.py render  --cli <exe> --label <имя>
python xtool/wps-qa/wps_qa.py compare --label <имя>
python xtool/wps-qa/wps_qa.py report  --label <имя> [--baseline <прежнее имя>]
python xtool/wps-qa/wps_qa_selftest.py                 # самотест метрик, без WPS
```

Корпуса находятся сами (`wps_qa.py corpora` их перечисляет): явно
зарегистрированы только `core-strict`, `core-samples`, `core-docx` и
`pdf-docx` (`wpsqa_common.REGISTERED`), а корпусом считается **каждый**
каталог `testdata/<ИМЯ>`, где на верхнем уровне есть `.docx` или `.rtf`, — с
именем каталога. Новый корпус, положенный в `testdata/`, измеряется без правки
кода. Корпуса gitignored и лежат в основной копии; из worktree она находится
через `git rev-parse --git-common-dir` (или `--data-root`). PDF/DjVu-наборы —
входы импортёров, документов для вёрстки в них нет, и они не попадают в список.

RTF (`CC0_RTF`, `CC0_RTF_1` и любые новые): WPS снимает эталон с самого `.rtf`;
наш путь — `import-rtf --wps-text --allow-unsigned-unicode --drop-nul`
(корпусный профиль `docs/rtf/ACCEPTANCE.md`) в Strict DOCX, затем `to-pdf`.
DOCX и `.rtf-report.json` остаются рядом с нашим PDF; в отчёте — статус импорта
(Clean/Degraded/Failed) и число потерь импорта, по документам и по корпусам.

Через очередь хоста: вид задания `wps-qa` (`tools/hostq/README.md`) собирает CLI
на нужном sha и прогоняет render → compare → report с меткой `sha-<10 hex>`.

По умолчанию `--cli` — `<основная копия>/target/release/strict-ooxml.exe`,
`--label` — `bin-<sha256 бинарника[:10]>`, так что метка однозначно называет
сборку. Два прогона разных сборок сравниваются через `report --baseline`.

## Хранилище (`--store`, по умолчанию `<основная копия>/wps-qa/`, gitignored)

| путь | что |
|---|---|
| `refs/<corpus>/<doc>.pdf`, `.json` | эталон WPS и его паспорт: sha256 исходника, версия WPS, время, статус/ошибка |
| `ours/<label>/<corpus>/<doc>.pdf`, `.json` | наш `to-pdf --transitional`, код выхода, хвост stderr |
| `ours/<label>/build.json` | путь и sha256 CLI |
| `cmp/<label>/<corpus>/<doc>.json` | все метрики документа |
| `cmp/<label>/img/<corpus>/<doc>.png` | худшая страница: WPS \| наш \| наложение (красный — только WPS, синий — только наш) |
| `cmp/<label>/summary.json`, `results.csv`, `report.html` | сводка по корпусам и по документам |
| `refs/<corpus>/<key>.fonts.json` | доли шрифтов эталона (кэш по размеру и mtime PDF) |

Всё кэшируется: эталон переснимается только при смене sha256 исходника
(`--force`, `--retry-failed`), сравнение — при смене любого из двух PDF.

## Съёмка в WPS

Один документ — один дочерний процесс (`wps_worker.py`): WPS, повисший на
диалоге, стоит одного таймаута (`--timeout`, 240 с), а не всего прогона.
Воркер записывает pid-ы запущенных им `wps.exe`; по таймауту и после работы
убиваются только они — открытые пользователем окна WPS не трогаются. WPS
открывает приватную копию файла (`in.docx`) только на чтение, с фиктивным
паролем (защищённый файл падает, а не ждёт ввода), режим разметки страницы,
исправления скрыты; экспорт — `ExportAsFixedFormat(pdf)`.

## Метрики (`metrics.py`)

Оба PDF читаются одной библиотекой (pdfium) и растрируются одним растеризатором,
поэтому разница — это разница вёрстки, а не инструментов.

| семейство | что меряет | вес |
|---|---|---|
| pages | число страниц (0.8) и размер страниц (0.2) | 15% |
| text | F1 мультимножества слов (NFKC, casefold): потерянный/лишний текст | 25% |
| flow | доля сопоставленных слов на той же странице + F1 переносов строк между соседними словами | 25% |
| geometry | среднее из: абсолютное положение слов на той же странице ≤ 4 pt; шаг между соседними словами в строке (advance) и шаг строк (leading) ≤ max(1 pt, 5%) | 20% |
| raster | по парам страниц: SSIM при 36 dpi (видит блоки, а не глифы), корреляция профилей чернил по строкам и столбцам | 15% |

Слова сопоставляются patience-diff-ом (уникальные слова — якоря, LIS,
промежутки — ограниченный `SequenceMatcher`). Страницы WPS и наши
спариваются по словам, а не по номеру, так что сдвиг пагинации не ломает
растровое сравнение. Локальные меры (advance, leading, переносы) не
наследуют дрейф, накопленный выше, — одна ошибка не штрафуется многократно.

Итог — балл 0–100 и оценка A ≥ 90, B ≥ 75, C ≥ 50, D < 50; F — наш рендер не
отработал. Веса и допуски — константы вверху `metrics.py`; это измерительная
шкала, не порог.

## Известные ограничения

- RTL-текст (арабский, урду) WPS кладёт в PDF в визуальном порядке слов:
  F1 текста к этому нечувствителен, выравнивание и переносы на таких строках
  менее точны.
- WPS использует системные шрифты, мы — встроенные метрически совместимые;
  расхождения из-за подстановки шрифтов — часть измеряемого.
- Эталон — это WPS 12.1, а не Word: он сам может ошибаться.

## Эталоны в шрифте-подмене

WPS рисует отсутствующее на хосте семейство другим шрифтом: чаще всего
Segoe Print (`inter`, `EB Garamond`, даже `Cousine`), но и Yu Gothic UI
(Agency FB), Gabriola (Blackadder ITC), Verdana (Mallanna, Andika), Georgia
(Vani). Такой эталон меряет шрифты хоста, а не раскладку. Отчёт считает для
каждого эталона долю видимых символов, нарисованных начертанием, которого
документ не называет (таблица шрифтов, шрифты фрагментов, темы), кроме
законных замен — стандартных псевдонимов (Arial, Times New Roman, Courier
New), символьных и математических шрифтов и системных шрифтов других
письменностей (`metrics.LEGIT_UNNAMED`); Segoe Print считается всегда. Это
`ref_fallback` в `results.csv`; при доле ≥ 10% документ попадает в группу
«эталон в шрифте-подмене»: `summary.json`
`total.clean` / `total.ref_fallback`, по корпусам `mean_score_clean` и
`ref_fallback_docs`. Общий `mean_score` не меняется, чтобы метки оставались
сопоставимыми.

## Readability — the headline score

The report leads with **readability** (0–100): how well our PDF carries the
document to a reader, scored against the source document itself, not
against WPS (`metrics.readability`):

| Part | Weight | What it measures |
|---|---|---|
| completeness | 40% | share of the body's words found on our pages, discounted for characters drawn as missing-glyph boxes (`missing_glyphs` in the render metadata, from `pdf.font.no_glyph`) |
| reading order | 25% | share of the body's words that come in order (the same monotone alignment as the fidelity score) |
| legibility | 20% | share of words neither drawn over another word nor off the page, scaled by the share of words present |
| pictures | 15% | share of the body's pictures (DrawingML pictures, VML image data) drawn on the pages |

WPS's own PDF is scored the same way (`ref_readability`), which shows where
WPS itself loses the text. The source is the DOCX, or the Strict DOCX an RTF
was imported to. The **WPS fidelity** score (pages, text, flow, geometry,
raster against WPS's PDF) stays as the second number: how close the pages
are to WPS's, not whether the document reads.
