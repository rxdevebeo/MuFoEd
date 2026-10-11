# Доработка основных незавершённых этапов RTF

Дата: 10 октября 2026. Worktree:
`C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`, ветка
`codex/rtf-import-integrated`. План и общие контракты —
[REMAINING_COMPATIBILITY_PLAN.md](REMAINING_COMPATIBILITY_PLAN.md); карточки —
`compatibility/RCxx/{STATUS.json,tests.md}`.

Этот документ фиксирует **крупные незавершённые работы** (shared-build и
acceptance), а не мелкие mapping-срезы. Для каждого этапа: текущее состояние,
что осталось, предлагаемый контракт/подход, свидетели, критерий закрытия (DoD),
зависимости и риск. Порядок исполнения — в конце.

## Сводка

| Этап | Состояние | Основная незавершённая работа | Тип |
| --- | --- | --- | --- |
| RC03 shaping/bidi | InProgress | line-level visual reordering mixed-direction runs (bidi-oracle) | renderer |
| RC04 styles/metrics | InProgress | resolved-cascade equality; corpus tab/leader/keep/pixel geometry | renderer + resolver |
| RC05 lists | InProgress | override levels `\lfolevel` — **done (2026-10-11)**; remaining is renderer counter continuity | parser done; renderer |
| RC06 tables | Done | all table controls mapped (grid/merge/borders/shading/margins/widths/diagonals); nested tables = explicit Reject | parser + shared model |
| RC07 metafiles | Planned | bounded WMF/EMF decoder in typed primitives | shared render |
| RC08 regions/fields | InProgress | column layout **done (2026-10-11)**; remaining: multi-paragraph field block, custom marks, `\ftncn`, continuous-section same-page geometry | shared layout/model |
| RC09 other losses | Planned | comments/revisions/textboxes/OLE/metadata | shared model |
| RC10 hardening | непрерывно | fuzz/budgets/hostile на каждом новом декодере | все crates |
| RC11 acceptance | Blocked | полный корпусный/профильный прогон, 100 verdicts | harness/assets |
| RC12 public/CI | Planned | матрица фич, CI без WPS, публичный frontend | CI |

Текущие dev-цифры (полный прогон `RC06-final3`, свежий WPS oracle 2026-10-11):
import resource 100/100, text 99/100 (014 — WPS field-recompute quirk), `check`
100/100 `ok: strict`, pagination 65/100, pixels 6/100 док. / 22/458 стр.
Import losses 83 671 (после RC06; было 127 197 до RC06, 127 228 изначально).

---

## RC03 — shaping и двунаправленный текст

**Состояние.** Второй shaper не создаётся: RTF использует общий harfrust/cluster
pipeline. Импорт делит direction-mixed runs по UBA на direction-homogeneous
`w:rtl`-runs; `unicode_cluster_map`/`unicode_x_positions_px` и PDF ActualText
закреплены witness'ами (Arabic/Tamil/Bengali/Devanagari/CJK).

**Осталось.**
1. **Line-level visual reordering** mixed-direction runs в RTL-base параграфах —
   `Blocked` на bidi-aware независимом oracle. `python-bidi 0.6` отдаёт только
   переупорядоченную строку без per-character levels, поэтому geometry-oracle
   «шейпить каждый level-run» не строится.
2. Корпусные свидетели 007/010/011/018 требуют реальных system faces
   (не лицензированы).

**Контракт/подход.** Взять независимый UBA-oracle с per-character levels
(например, ICU/`fribidi` через отдельный offline-инструмент) → построить
level-runs → шейпить каждый run существующим harfrust → собрать визуальную
строку и сверить origins/advances с reference. Реализация — в renderer (не RTF
mapping): после импорт-сплита.

**Свидетели.** synthetic mixed LTR/RTL с цифрами/punctuation и ligatures;
corpus 007/010/011/018 (после получения faces).

**DoD.** независимый cluster/geometry oracle и Unicode extraction проходят для
каждого заявленного script; нет перестановки текста, разрывов clusters и
ложного Clean; mixed-run reordering совпадает с UBA level-runs.

**Зависимости.** RC02 (faces). **Риск.** средний (renderer-only, изолирован от
RTF mapping).

---

## RC04 — styles и геометрия текста

**Состояние.** RTF-mapping run/paragraph-свойств закрыт (scale/spacing/tabs/
leaders/keep/widow/kern/position/effect и т.д.), три доказанных geometry-фикса
(`\deff`, paragraph-mark, script-qualified fonts). Остаточный pixel-gap назван
как substitute-face fidelity (RC02/RC03), не mapping.

**Осталось.**
1. **Resolved-cascade effective-style equality** — нужен styles.xml resolver;
   сейчас есть только direct-diff (receipts `RC04-resolved-{019,039}.json`).
2. **Corpus tab/leader/keep/widow/pixel geometry** на 019/039, 052/076 — нужен
   растеризатор (poppler есть; WPS reference PDF — в sibling).

**Контракт/подход.** Resolver: `docDefaults → pStyle chain → rStyle → direct` с
проверкой effective-значений и явного false/plain/pard. Для geometry —
targeted-страницы сравнивать по line origins/paragraph placement, не только по
page count.

**Свидетели.** 019/039 (одинаковое число страниц при pixel FAIL), 052/076;
style/spacing/tabs probes.

**DoD.** независимые effective-style facts и line/paragraph geometry совпадают
на выбранных probes; units/defaults не угадываются; targeted pixels проходят;
ранее закрытый текст/roundtrip остаются 100/100.

**Зависимости.** RC01/RC02. **Риск.** средний (resolver — shared, но read-only).

---

## RC05 — списки и numbering

**Состояние** ([карточка](compatibility/RC05/STATUS.json)). Легаси/совместимые
конструкции → Info; list table/override и отрицательный `\listid` уже маппятся.

**Сделано — override levels (2026-10-11).** `{\lfolevel ...}` теперь строит
`w:lvlOverride` на `w:num`:
- `\listoverrideformat{\listlevel ...}` — вложенные level-контролы идут через
  общий helper (`Parser::level_control`), результат → `LevelOverride.level`;
- `\listoverridestart\levelstartatN` → `LevelOverride.start_override` (без `w:lvl`);
- индекс уровня — порядковый номер `\lfolevel` (0..8);
  `\listoverridecount` принимается при любом значении.

Свидетель 021 (единственный `\lfolevel`) теперь несёт override: маркер —
Symbol bullet вместо абстрактной `*`. Import losses 152 → 131
(`-18 list-override-control:*`, `-3 list:unbound-text`); `\levelold`/`\fbias`
переклассифицированы как list-compat Info (015 `-1`, 087 `-9`). Тесты
`list_override_level_replaces_the_level` /
`list_override_start_sets_the_start_override` покрывают обе формы, нулевой Loss,
Strict write и reopen; 021 проходит `strict-ooxml check` → `ok: strict`.

**Полный WPS-прогон (2026-10-11).** Свежий независимый oracle WPS Writer
12.1.0.28485 (100/100 Captured + facts), receipts
`target/rtf-import/RC05-wps-full/`. Итоги: import losses 127228 → 127197 (−31) на
всём корпусе; resources 100/100; text 99/100 (единственный Fail — 014,
`\flddirty` PAGEREF: импортер сохраняет stale cached page number, WPS его
пересчитывает — quirk оракула, не RC05); `check` 100/100 `ok: strict`;
pagination 65/100; pixels 6/100 док. / 22/458 стр. — идентично RC08-final, то
есть RC05 не даёт визуальной регрессии. 021 остаётся pixel Fail (ssim 0.9067)
из-за pre-existing ~20 px line-pitch gap vs WPS; изменился только 46-px регион
маркера (`*` → Symbol bullet).

**Осталось.** Counter continuity across paragraphs/tables/sections — renderer,
не переаудирован; `list:unbound-text` остаётся Loss (malformed source); 014
PAGEREF-quirk требует явного accepted-статуса в reviewed profile RC11.

---

## RC06 — таблицы — **DONE (2026-10-11)**

**Состояние** ([карточка](compatibility/RC06/STATUS.json)). Все корпусные
табличные конструкции смаплены; nested tables — явный bounded Reject.

**Сделано (2026-10-11).**
1. **Borders** — state-machine `<edge> <style> \brdrw? \brsp? \brdrcf?`:
   - paragraph `\brdrt/b/l/r` → `w:pBdr`, `\brdrbar`/`\brdrbtw` → `bar`/`between`;
   - cell `\clbrdr{t,b,l,r}` → `w:tcBorders`; диагонали `\cldgll`/`\cldglu` → `tl2br`/`tr2bl`;
   - row `\trbrdr{t,b,l,r,h,v}` → `w:tblPrEx/w:tblBorders`;
   - стиль/`\brdrw`(twips→`w:sz` 1/8 pt)/`\brdrcf`/`\brsp`(→`w:space` pt);
   - `\clbrdX\brdrtbl` = inherit (override снимается, не Loss).
2. **Shading** — cell `\clcbpat/clcfpat/clshdng/clshdrawnil`, row `\trcbpat/...`
   (на все ячейки строки), run `\chcbpat/chcfpat/chshdng`, paragraph
   `\cbpat/cfpat/shading`; значение → `ST_Shd` (0=clear, 10000=solid, иначе pctNN).
3. **Cell props** — `\clhidemark`→`w:hideMark`, `\clpad*`→`w:tcMar`,
   `\trgaph`→`w:tblCellMar`, `\clftsWidth`/`\clwWidth`→`w:tcW`;
   `\tblind`/`\tblindtype`/`\alntblind` → Info.
4. Общий `Borders` расширен `between`/`bar`/`tl2br`/`tr2bl`; writer пишет каждое
   только в своём контейнере, reader парсит, render-svg merge учитывает.

Import losses 127 197 → **83 671** (−43 526). Text 99/100 (только 014), resources
100/100, `check` 100/100, pagination 65/100, pixels 6/100 док. / 22/458 стр. —
без регрессии. Плюс page borders `\pgbrdr{t,b,l,r}` → `w:pgBorders` и character
borders `\chbrdr` → `w:rPr/w:bdr` (reader теперь сохраняет `w:bdr`). Receipts —
`target/rtf-import/RC06-final3/`.

**Явно вне scope RC06 (со статусом).**
- Nested tables (`\itap>1`) — нет corpus-свидетеля; bounded named Reject, не flatten.
- Renderer не рисует штрихи `w:pBdr` (только pads), run-бордеры и диагонали;
  рисует `w:tcBorders`/`w:tblPrEx`/`w:pgBorders`. Renderer-gap, не import-gap.
- `\brdrart` (art page border, 015) — нет Strict-формы; explicit Loss.

**DoD.** независимые grid/merge regions, cell text и widths совпадают; renderer
не дублирует/не теряет cells; page breaks воспроизводимы; resource bounds
сохраняются; не закрывать таблицу только совпадением concatenated text —
выполнено (text/resource/XSD 100/100, pagination/pixels без изменения).

---

## RC07 — WMF/EMF и свойства изображений

**Состояние.** Не начат. Первый набор: 001, 004, 006 + portable fixtures.
Сохранённый payload/extent — control, но не доказательство rendering.

**Контракт/подход.** Единый bounded record-decoder в typed vector primitives,
используемый SVG и PDF; растеризация — отдельная явно помеченная policy с
фиксированным DPI. Покрыть placeable/standard WMF, EMF transforms, object
handles, SaveDC/RestoreDC, clipping, strokes/fills/text, scaling/crop, bitmap
records. Неизвестный state-changing record → Degraded/Reject (не продолжать с
недостоверным DC state). Стартовые ceilings: 100 000 records, 4096 live
handles, 64 saved DC, decoded bitmap ≤ `max_single_uncompressed`.

**DoD.** оригинальные media bytes сохраняются; reference geometry и pixels
совпадают в обоих backend; malformed lengths/overflow/handles/records/decode
bombs отклоняются до allocation и не обходят бюджеты.

**Зависимости.** RC00/RC01. **Риск.** высокий (новый decoder; обязателен RC10
сразу).

---

## RC08 — секции, регионы, примечания и сложные поля

**Состояние** ([карточка](compatibility/RC08/STATUS.json)). Секции/заголовки/
поля/закладки/регионы/колонки/note numbering + separators + contextualSpacing
закрыты.

**Осталось.**
1. **Column layout** — **сделано (2026-10-11)**. `layout::column_layout` строит
   per-column `(left, width)` из `w:cols` (equal-width с `@space` либо explicit
   `w:col/@w`+`@space`; необъявленная ширина в explicit-наборе = равная доля
   остатка). Paginator переносит блок, не влезший в колонку, в **следующую
   колонку на той же странице** и открывает новую страницу только когда колонки
   полны; page/section-break сбрасывает на первую колонку, а continuous-секция
   несёт свои колонки вместе с отложенной геометрией. Элементы в следующей
   колонке сдвигаются на column-offset (параграф, начавшийся в одной колонке и
   продолжившийся в следующей, сохраняет свой wrap). Single-column — fast-path:
   список колонок пуст, все offsets нулевые, старая раскладка byte-identical.
   `w:br w:type="column"` → `Flow::ColumnBreak`; endnotes идут на всю страницу.
   Тесты `f14_columns.rs` (column break, overflow, fast-path). Corpus: pagination
   **65/100 → 66/100** — меняется только 092 (21 → 20 стр., совпало с WPS ref),
   остальные page counts равны `RC06-final3`. Receipts —
   `target/rtf-import/RC08-columns-layout/`.
   Witness 074: обе колонки рисуются (left 76 px / right 425 px), но 4 стр. против
   ref 3 — его 2-column секция это continuous-break, а continuous-break по-прежнему
   откладывает геометрию (и колонки) на следующую страницу, поэтому колонки
   начинаются со стр. 2. Остаётся same-page continuous geometry. У unequal-колонок
   нет per-column re-wrap: параграф из широкой колонки в узкую сохраняет wrap
   стартовой колонки (074/092 это не задействуют).
2. **Multi-paragraph field block results**: inline `Field` не держит block
   result; marker-форма сложного поля требует document-level field state между
   параграфами. Witness 014 (поле TOC).
3. **Custom marks** (`w:customMarkFollows`): 0 вхождений в корпусе, нет
   поддержки в model.
4. `\ftncn`/`\aftncn` (continuation notice) остаются Loss.

**Контракт/подход.**
- Columns: колонко-менеджер в paginator (лево/ширина на колонку, заполнение
  сверху вниз, `w:br type="column"`, новая страница когда все колонки полны);
  equal-width и explicit `w:col`. Изолировать single-column fast-path.
- Field block: либо `Field { blocks }`, либо marker-форма (`fldChar` begin/
  separate/end + `instrText`) с прогонкой state через параграфы в layout;
  reader/writer/layout/edit/report должны поддержать контракт до включения
  mapping.
- Custom marks: `FootnoteRef`/`EndnoteRef` + `customMarkFollows` + glyph-run.

**DoD.** exact region text/refs, instructions/targets/geometry, Strict/OPC и
reopen; отсутствие flatten с ложным Clean; columns воспроизводимы на
074/092 в page/pixel.

**Риск.** высокий (paginator и field model). Начинать с columns (2 реальных
свидетеля), потом field block.

---

## RC09 — остальные классы потерь

**Состояние.** Не начат по существу; ledger задаёт triage. Уже частично снято
RC08 (fields/bookmarks/notes) и RC05/RC06 реклассификациями.

**Осталось.** Разложить по RC01: comments, revisions, textboxes/shapes, OLE,
metadata и прочие controls на mapping-tasks с witnesses. Использовать
существующие WML revision/DrawingML типы; проверять final/original view. OLE —
opaque payload/relationships без activation; preview измерять отдельно.
Метаданные (авторство/даты/docprops) не считать визуальными glyph-losses, но
показывать в import report. Шапковые metadata-дестинации, всё ещё Loss:
`{\*\rsidtbl}`, `{\*\expandedcolortbl}`, `{\*\xmlnstbl}`, `{\*\pgptbl}`,
`{\*\pgdsctbl}`, `{\*\revtbl}`.

**DoD.** у каждого наблюдаемого класса — implemented semantics + independent
roundtrip test либо явное неподдержанное правило с открытым compatibility
status. Blanket ignore/rename-to-info запрещён.

**Риск.** средний; делается порциями по классам.

---

## RC10 — hardening и границы ресурсов

**Состояние.** Сопровождает каждый новый decoder/builder; существующие
supervisor-лимиты есть.

**Осталось/подход.** debug/release hostile inputs, бюджеты fonts/media/records/
nodes/clusters, time/memory, cycles/forward refs, issue cap и точные totals.
Controls на abort/write failure и неизменность источника. Fuzz targets с
фиксированным seed/corpus/time budget; crash artifacts не выдаются за
successful documents. Для каждого нового RC07/RC08-декодера — сразу.

**DoD.** ни одного panic/hang/silent fallback вне policy; import 30 s / 512 MiB
и render 90 s / 1 GiB измерены на всех 100 с достоверным peak RSS; новые
публичные limits документированы и проверены на границах.

**Риск.** средний; обязателен вместе с RC07/RC08.

---

## RC11 — полная корпусная и профильная приёмка

**Состояние.** Blocked на внешнем harness: полный pagination/pixel gate требует
poppler + pinned comparator + WPS reference PDF (в sibling worktree).

**Осталось.** Независимо review support matrix/expectations для всех 100:
semantics, oracle versions/hashes, fonts, expected outcome, resources и каждый
допустимый Loss. Не заполнять pending expectations из текущих failures.
Структурные comparisons — эффективные свойства и все регионы, не только
выбранные style names.

**DoD (полная corpus compatibility).** 100/100 content/media/structure,
100/100 page counts, все сопоставленные страницы проходят pixel/geometry
policy, 0 silent loss, 0 unexpected/Blocked, 0 необъяснённых degradation.
Известный WPS quirk с проверенной RTF-семантикой — отдельный reference и явный
статус. Threshold SSIM 0.96 и structural bounds не ослаблять; reviewed
profile/threshold version закрепить.

**Блокер.** WPS reference PDF и tooling. **Риск.** высокий (результат зависит
от RC02–RC10).

---

## RC12 — публичная интеграция и CI

**Состояние.** Не начат.

**Осталось.** Проверить no-default/base, rtf-only, default, all-features и
релевантные платформы; CLI status/reports, viewer diagnostics, source/media/font
lifetime. Portable synthetic fixtures — в CI на Windows/Linux/macOS без WPS.
Полный локальный корпус и WPS references — воспроизводимым отдельным acceptance
job; их отсутствие = NotRun/Blocked.

**DoD.** current-SHA CI подтверждён; existing shared queues не регрессировали;
100 inputs проходят публичный путь; frontend показывает resolved/substitute/
unsupported statuses и подробный report. Merge/release — отдельные завершающие
операции.

**Риск.** низкий-средний; инфраструктурный.

---

## Порядок исполнения

1. **RC05 override levels** (parser-local, model/render готовы) — сделано
   2026-10-11.
2. **RC06** (cell/paragraph/row borders, shading, cell props, диагонали) —
   **сделано (2026-10-11)**, этап закрыт; nested tables — явный Reject. С RC10.
3. **RC08 columns** — **сделано (2026-10-11)**; далее **RC08 field block**
   (witness 014) и custom marks.
4. **RC04 resolved-cascade** (resolver) и corpus geometry — параллельно с RC02/RC03.
5. **RC03 line-level bidi** — после bidi-oracle с per-char levels.
6. **RC07 metafiles** (отдельная очередь) + RC10.
7. **RC09** по семантическому impact (RC01 triage).
8. **RC11** после RC02–RC10; **RC12** после RC11.

Правила параллелизма из плана сохраняются: shared `parser.rs`, model и layout не
имеют двух независимых авторов одновременно.

## Сквозные prerequisites (общие изменения)

- `Borders` (+`between`/`bar`/`tl2br`/`tr2bl`) — RC06 — **сделано (2026-10-11)**,
  включая writer/reader/render-svg merge; regression-тесты wml/write/convert/
  render-svg прогнаны.
- Field block-result контракт (model + reader/writer/layout/edit/report) — RC08.
- Column-менеджер в paginator — RC08 — **сделано (2026-10-11)**.
- `customMarkFollows` для `FootnoteRef`/`EndnoteRef` — RC08.
- Bounded WMF/EMF decoder (typed primitives) — RC07.
- Каждое изменение общих model/layout/writer требует согласованной единой
  реализации и regression-тестов всех потребителей; скрытые XML-фрагменты
  контракт не подменяют.

## Блокеры и внешние активы

- WPS reference PDFs/факты не сохраняются долго; 2026-10-11 они перегенерированы
  свежим прогоном WPS Writer 12.1.0.28485 (`target/rtf-import/RC05-wps-full/`),
  который воспроизвёл pagination 65/100 и pixels 6/100. Свежий raster для RC11
  можно получить тем же `wps_oracle.py --pdf`.
- Реальные system faces 007/010/011/018 не лицензированы (RC03).
- poppler 26.09.0 provisioned (`target/rtf-import/poppler/`); `pdfinfo`/`pdftoppm`
  доступны локально.
