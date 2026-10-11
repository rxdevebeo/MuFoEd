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
| RC08 regions/fields | InProgress | column layout + multi-paragraph field block **done (2026-10-11)**; remaining: custom marks, `\ftncn`, continuous-section same-page geometry | shared layout/model |
| RC09 other losses | InProgress | revision/compat metadata + table/char controls with slots **done (2026-10-11)** (losses 83 671 → 4 531); remaining: `wps:leading-group-line-ending`, `\ts` table styles, picture crop, comments/revisions/textboxes/OLE/metadata | shared model |
| RC10 hardening | непрерывно | fuzz/budgets/hostile на каждом новом декодере | все crates |
| RC11 acceptance | Blocked | полный корпусный/профильный прогон, 100 verdicts | harness/assets |
| RC12 public/CI | Planned | матрица фич, CI без WPS, публичный frontend | CI |

Текущие dev-цифры (полный прогон `RC06-final3`, свежий WPS oracle 2026-10-11):
import resource 100/100, text 99/100 (014 — WPS field-recompute quirk), `check`
100/100 `ok: strict`, pagination 66/100, pixels 6/100 док. / 22/458 стр.
Import losses 4 531 (после RC09-срезов 2026-10-11; было 11 394, 83 671 после
RC06, 127 228 изначально).

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
2. **Multi-paragraph field block** — **сделано (2026-10-11)**. `{\field ...}` с
   block-результатом (или `\flddirty`) теперь пишется complex marker-формой
   `w:fldChar begin` + `w:instrText` + `w:fldChar separate` + кэшированные
   result-параграфы + `w:fldChar end` (single-paragraph/актуальный результат
   остаётся `w:fldSimple`). `\flddirty` → `w:dirty` на begin-маркере;
   `\fldedit` и legacy `{\*\datafield}` → Info. `field:multi-paragraph-
   instruction-dropped` Loss убран. Renderer уже трекал `fldChar`/`instrText`
   и пропускает инструкцию, поэтому картинка не меняется. Witness 014: losses
   830 → 796; весь корпус 83671 → 83010, все 100 page counts без изменений.
   Тесты `structure.rs::{multi_paragraph_field_becomes_the_complex_marker_form,
   dirty_field_marker_and_compat_controls_are_not_losses}`.
3. **Custom marks** (`w:customMarkFollows`) — 0 вхождений в корпусе; reader
   теперь помечает ссылку `Partial` явно (не молчаливый drop), но глиф-ран
   custom-марки не различается — **explicit non-support**.
4. `\ftncn`/`\aftncn` (continuation notice, не `\ftnsepc` continuation separator,
   который уже смаплен) — нет Strict-элемента, остаётся явным
   `destination:ftncn` Loss — **explicit non-support**.

**Контракт/подход.**
- Columns — **сделано (2026-10-11)**: колонко-менеджер в paginator; single-column
  fast-path изолирован.
- Field block — **сделано (2026-10-11)**: выбран marker-форма (`fldChar` begin/
  separate/end + `instrText`) как уже поддержанная model/reader/writer/layout;
  `Field { blocks }` не понадобился. Осталось только same-page continuous
  geometry для columns.
- Custom marks: `FootnoteRef`/`EndnoteRef` + `customMarkFollows` + glyph-run.

**DoD.** exact region text/refs, instructions/targets/geometry, Strict/OPC и
reopen; отсутствие flatten с ложным Clean; columns воспроизводимы на
074/092 в page/pixel — по page 092 закрыт, 074 упирается в same-page continuous
geometry.

**Риск.** высокий (paginator и field model) — реализовано отдельными срезами
(columns, затем field block).

---

## RC09 — остальные классы потерь

**Состояние.** Ledger-driven triage (2026-10-11). Ревизионные save-ID и Word 6/7 +
Asian layout-контролы, чья bare-форма равна Strict-дефолту, теперь именованный Info
(`rsid-compat`/`word6-compat`/`paragraph-compat`). Затем:
- **slot mappings:** `\kerning`→`w:kern`, `\ulc`→`w:u/@w:color`, `\cb`→run `w:shd`;
  `\trpadd{t,b,l,r}`/`\trpaddf*`→`w:tblCellMar`, `\trrh`→`w:trHeight` (отрицательный
  = `exact`), `\trqr`/`\trql`/`\trqc`→`w:trPr/w:jc`, `\trautofit`→`w:tblLayout`;
  `\pardeftab`/`\deftab`→`w:defaultTabStop`; `{\*\panose}`→`w:panose1`;
- **named Info по WPS-оракулу** (reference пишет пусто): `\hyphpar0`/`\noline`,
  `\nooverflow`/`\nocwrap`, `\cltxlrtb`, `\hres`/`\chhres`, `\qk`/`\qnatural`,
  `\animtext`, `\trftsWidth*`/`\trwWidth*`, метаданные-дестинации (`{\*\falt}`,
  `{\*\hyphen}`, `{\*\pnseclvl}`, `{\*\rsidtbl}`, `{\*\revtbl}`, `{\*\latentstyles}`,
  `{\*\expandedcolortbl}`, `{\*\xmlnstbl}`, `{\*\pgptbl}`, `{\*\pgdsctbl}`,
  `{\*\wgrffmtfilter}`, `{\*\userprops}`, `{\*\fname}`, `{\*\generator}`) и
  settings/cocoa/view/drawing-grid/math/rsid-семейства, `\widowctrl`, `\additive`,
  `\pardirnatural`, `\deflang*`.

Итог: import losses **83 671 → 11 394 → 4 531**; pagination 66/100 (0 изменений
номеров страниц против `RC08-columns-layout`), `check` 100/100 `ok: strict`.
Metadata/settings-срезы severity-only → DOCX SHA-256 идентичен. Тесты —
`structure.rs::{legacy_layout_and_revision_controls_are_info_not_losses,
non_default_legacy_layout_controls_stay_losses,
document_settings_and_compat_controls_are_info,
kerning_underline_colour_and_character_background_map_to_run_props,
table_padding_row_height_alignment_and_layout_map,
default_tab_stop_and_compat_controls_map_and_classify,
font_panose_maps_and_metadata_destinations_are_info}`. Карточка
[compatibility/RC09](compatibility/RC09/STATUS.json).

**Осталось.** (a) `wps:leading-group-line-ending` (2 541) — WPS пишет `&#10;`, наш
WPS-профиль сохраняет байт как LF, тексты расходятся, поэтому остаётся явным Loss;
(b) `\ts` table styles (`w:tblStyle`); (c) picture crop `\piccrop*`/`{\*\picprop}`
(нужен `a:srcRect`); (d) content-классы, которым нужен shared model — comments,
revisions, textboxes/shapes, OLE, document metadata; (e) language-сентинелы
`\lang0`/`\lang1024` и прочие unknown LCID закреплены как Loss тестами; (f) хвост
низкоуверенных application/layout-флагов (`\newtblstyruls`, `\sectlinegrid`,
`\horzdoc`, `\stshf*`, `\clmar*`, `\gaph`, `\fet`, shape-stroke) сознательно не
переклассифицирован.

**DoD.** у каждого наблюдаемого класса — implemented semantics + independent
roundtrip test либо явное неподдержанное правило с открытым compatibility status.
Blanket ignore/rename-to-info запрещён.

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
3. **RC08 columns** — **сделано (2026-10-11)**; **RC08 field block** (witness 014)
   — **сделано (2026-10-11)** (marker-форма); custom marks и `\ftncn` —
   **explicit non-support** (в корпусе не наблюдаются). Далее RC04.
4. **RC04 resolved-cascade** (resolver) и corpus geometry — параллельно с RC02/RC03.
5. **RC03 line-level bidi** — после bidi-oracle с per-char levels.
6. **RC07 metafiles** (отдельная очередь) + RC10.
7. **RC09 по семантическому impact** — сделано (2026-10-11): revision/compat
   metadata, table/char-контролы с реальными слотами и метаданные-дестинации
   (losses 83 671 → 4 531); далее — `\ts` table styles, picture crop и
   content-классы (comments/revisions/textboxes/OLE/metadata).
8. **RC11** после RC02–RC10; **RC12** после RC11.

Правила параллелизма из плана сохраняются: shared `parser.rs`, model и layout не
имеют двух независимых авторов одновременно.

## Сквозные prerequisites (общие изменения)

- `Borders` (+`between`/`bar`/`tl2br`/`tr2bl`) — RC06 — **сделано (2026-10-11)**,
  включая writer/reader/render-svg merge; regression-тесты wml/write/convert/
  render-svg прогнаны.
- Field block-result контракт — RC08 — **сделано (2026-10-11)** marker-формой
  (`fldChar`/`instrText`), уже поддержанной model/reader/writer/layout.
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
