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
| RC05 lists | InProgress | override levels `\lfolevel` | parser (model/render готовы) |
| RC06 tables | InProgress | borders/shading; cell shading/diagonal; nested tables; `\tblind` | shared model + parser + renderer |
| RC07 metafiles | Planned | bounded WMF/EMF decoder in typed primitives | shared render |
| RC08 regions/fields | InProgress | column layout; multi-paragraph field block; custom marks; `\ftncn` | shared layout/model |
| RC09 other losses | Planned | comments/revisions/textboxes/OLE/metadata | shared model |
| RC10 hardening | непрерывно | fuzz/budgets/hostile на каждом новом декодере | все crates |
| RC11 acceptance | Blocked | полный корпусный/профильный прогон, 100 verdicts | harness/assets |
| RC12 public/CI | Planned | матрица фич, CI без WPS, публичный frontend | CI |

Текущие dev-цифры (последний прогон `RC0506-corpus`): import text/resource
100/100, `check` 100/100 `ok: strict`, pagination 65/100, pixels 6/100 док. /
22/458 стр. (переносятся по идентичности PDF). Import losses 127 228.

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

**Осталось — override levels.** `{\*\lfolevel ...}`:
- `\listoverrideformat{\listlevel ...}` — полная замена уровня;
- `\listoverridestart\levelstartatN` — start override.

Сейчас → `list-override-control:*` Loss. Witness 021 (override меняет глиф
маркера с `*` на bullet).

**Контракт/подход.** Модель и renderer уже держат `w:lvlOverride`
(`LevelOverride { start_override, level }`). Работа локальна в parser:
- `OverrideDraft` получает `levels: Vec<LevelOverride>` и признак «строим
  уровень override»;
- `\lfolevel` начинает уровень; `\listlevel` открывает `LevelDraft`;
- управляющие уровня (`\levelnfc`/`\leveltext`/`\levelstartat`/…) маршрутизировать
  тем же кодом, что `Destination::Lists` (вынести в общий helper);
- `finish_level` при активном override строит `LevelOverride` вместо
  `list_draft.levels`.

**DoD.** marker values/sequence/geometry и numbering XML проходят independent
facts, Strict XSD и reopen; нет двойного cached list text или потерянного
restart; fixtures покрывают legal forward refs, missing abstracts, overrides.

**Риск.** низкий-средний (parser-local; общий код уровня не менять семантику).

---

## RC06 — таблицы

**Состояние** ([карточка](compatibility/RC06/STATUS.json)). Grid/merge/widths/
header rows/`w:tblLook`/`w:bidiVisual` маппятся; nested tables — явный Reject.

**Осталось.**
1. **Бордеры и shading** — крупнейший блок (~35k Loss):
   - paragraph: `\brdrt`/`\brdrb`/`\brdrl`/`\brdrr`/`\brdrbar`/`\brdrbtw`;
   - cell: `\clbrdr{t,b,l,r}`; table: `\brdrtbl`; row: `\trbrdr*`; page:
     `\pgbrdr*`;
   - стиль-токены: `\brdrs`/`\brdrth`/`\brdrdb`/`\brdrdot`/`\brdrdash`/
     `\brdrnil`/`\brdrnone`/`\brdrw`/`\brdrcf`/`\brsp`.
   Грамматика RTF: `<edge> <style> \brdrw? \brsp? \brdrcf?`.
2. Cell shading `\chshdng`/`\clshdrawnil`; diagonal borders `\cldgll`/`\cldglu`.
3. Nested tables (`\itap>1`) — сейчас Reject; поддержать bounded WML builder,
   не flatten.
4. Table indent `\tblind`/`\tblindtype`.

**Контракт/подход.**
- State-machine в parser: «текущий edge» + текущий стиль/ширина/цвет/space;
  edge-токен фиксирует край для последующих style-токенов. Четыре стандартных
  края → `Borders`; cell/table → `CellProperties.borders`/`TableProperties.
  borders`.
- `\brdrbar`/`\brdrbtw` требуют `w:pBdr` `between`/`bar`: расширить общий
  `Borders` полями `between`/`bar` (+ reader/writer). Проверить, что Strict
  XSD принимает, и что DOCX-roundtrip не регрессирует.
- `\brdrtbl` = «использовать границу таблицы» (reference), не отдельный край.
- Nested tables: bounded `w:tbl` внутри `w:tc` (Strict допускает), с проверкой
  бюджетов; до реализации — явный Reject (уже так).
- `\tblindN` → `w:tblInd` (перекрывает текущее `\trleft`-выведение — сначала
  свериться с WPS reference; `\tblindtype` без Strict-формы → Info).

**Свидетели.** 001/004/006 (RC07 пересекается по таблицам?), existing
vertical_merge probe, fixtures mixed merged cells, документы с `\brdrtbl`
(008/015) и с реальными cell-бордерами.

**DoD.** независимые grid/merge regions, cell text и widths совпадают; renderer
не дублирует/не теряет cells; page breaks воспроизводимы; resource bounds
сохраняются; не закрывать таблицу только совпадением concatenated text.

**Риск.** высокий (shared model `Borders` + renderer borders). Делать после
RC04, отдельными под-срезами: (a) cell borders, (b) paragraph borders с
моделью, (c) shading, (d) nested.

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
1. **Column layout** в shared renderer: `w:cols` round-trip'ится, но рисуется
   одна колонка. Witness 074/092.
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

1. **RC05 override levels** (parser-local, model/render готовы) — быстрый выигрыш.
2. **RC06 borders** под-срезами: (a) cell borders, (b) `Borders` + paragraph
   borders (`between`/`bar`), (c) shading, (d) nested tables. С RC10 на каждом.
3. **RC08 columns** (2 свидетеля) → затем **RC08 field block** (witness 014).
4. **RC04 resolved-cascade** (resolver) и corpus geometry — параллельно с RC02/RC03.
5. **RC03 line-level bidi** — после bidi-oracle с per-char levels.
6. **RC07 metafiles** (отдельная очередь) + RC10.
7. **RC09** по семантическому impact (RC01 triage).
8. **RC11** после RC02–RC10; **RC12** после RC11.

Правила параллелизма из плана сохраняются: shared `parser.rs`, model и layout не
имеют двух независимых авторов одновременно.

## Сквозные prerequisites (общие изменения)

- `Borders` (+`between`/`bar`) — RC06.
- Field block-result контракт (model + reader/writer/layout/edit/report) — RC08.
- Column-менеджер в paginator — RC08.
- `customMarkFollows` для `FootnoteRef`/`EndnoteRef` — RC08.
- Bounded WMF/EMF decoder (typed primitives) — RC07.
- Каждое изменение общих model/layout/writer требует согласованной единой
  реализации и regression-тестов всех потребителей; скрытые XML-фрагменты
  контракт не подменяют.

## Блокеры и внешние активы

- WPS reference PDF больше не сохраняются под sibling-oracle → pixel-замеры
  подтверждаются эквивалентностью PDF-идентичности; свежий raster требует
  восстановления reference-набора (RC11).
- Реальные system faces 007/010/011/018 не лицензированы (RC03).
- poppler 26.09.0 provisioned (`target/rtf-import/poppler/`); `pdfinfo`/`pdftoppm`
  доступны локально.
