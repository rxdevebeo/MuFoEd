# Передача сессии — RTF-очередь (strict-ooxml)

**Дата:** 2026-10-11 (продолжено 2026-10-12)
**Ветка:** `codex/rtf-import-integrated`
**Worktree:** `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
**План:** [docs/rtf/REMAINING_MAJOR_WORK.md](docs/rtf/REMAINING_MAJOR_WORK.md),
[REMAINING_COMPATIBILITY_PLAN.md](docs/rtf/REMAINING_COMPATIBILITY_PLAN.md),
карточки `docs/rtf/compatibility/RCxx/{STATUS.json,tests.md}`.
**Правило:** коммиты — только с явного разрешения пользователя. Все команды —
с этим worktree как cwd; `target/` локальный.

> **Состояние на передаче:** рабочее дерево **чистое** — вся работа этой сессии
> закоммичена (см. §2). Незакоммиченного среза нет.

---

## 1. Контекст

Импорт RTF → Strict DOCX + рендер SVG/PDF с независимой приёмкой на корпусе
100 RTF. Владею `strict-ooxml-rtf/**`, `docs/rtf/**`, `xtool/rtf/**`; общие
model/render/write менять согласованно и с regression-тестами.

- **MSRV:** `cargo +1.92.0` (обычный `cargo` на машине — 1.91.1, ниже MSRV).
- **Корпус:** `D:\projects\StrictLib\testdata\CC0_RTF` (100 файлов, из git
  исключён; копия также в worktree `rtf-import`).
- **WPS-эталоны** (Transitional `reference.docx` + `reference.pdf` + `facts.json`)
  лежат здесь: `target/rtf-import/RC05-wps-full/oracle/files/<NNN>/`.
- **poppler:** `target/rtf-import/poppler/poppler-26.09.0/Library/bin/{pdfinfo,pdftoppm}.exe`.
- **CLI:** `target/debug/strict-ooxml.exe` (собирать `cargo +1.92.0 build -p strict-ooxml-cli`).
- **Python:** 3.11; xtool-тесты — `python -m pytest xtool/rtf/`.

## 2. Что сделано и закоммичено

| Коммит | Суть |
|---|---|
| `f9a14f2` | RC05 `\lfolevel`-override уровни → `w:lvlOverride`; RC06 табличные бордеры/shading/cell-props, общий `Borders`, renderer штрихи `w:pBdr` |
| `016e290` | доки RC05/RC06 |
| `4bf8f81` | **RC08 column layout**: `layout::column_layout`, колоночный менеджер в paginator, `w:br type=column` |
| `2fc2e02` | доки RC08 columns |
| `b69ab9a` | **RC08 field block**: block-result `{\field}` → complex marker-форма; `\flddirty` → `w:dirty` |
| `9a090a0` | доки RC08 field block |
| `6b5ebe7` | `w:customMarkFollows` теперь `Partial` |
| `3c5ff0e` | доки RC08 custom marks / `\ftncn` |
| `f277519` | **RC09 triage №1**: rsid + word6/paragraph-compat defaults → Info (83010 → 11394) |
| `16f3203` | доки RC09 triage №1 |
| `13c904c` | **RC09 slot mappings**: `\kerning`→`w:kern`, `\ulc`→`w:u/@color`, `\cb`→run `w:shd`; `\trpadd*`→`w:tblCellMar`, `\trrh`→`w:trHeight`, `\trqr/ql/qc`→`w:trPr/w:jc`, `\trautofit`→`w:tblLayout`; `\pardeftab`→`w:defaultTabStop`; WPS-defaults → Info (11394 → 6995) |
| `d0625c3` | **RC09 metadata**: `{\*\panose}`→`w:panose1`; метаданные-дестинации → Info (6995 → 5869) |
| `d61f718` | **RC09 settings**: rsid/view/cocoa/dg/math/settings-семейства → Info (severity-only, 5869 → 4696) |
| `dea8eb1` | **RC09 defaults**: `\pardirnatural`, `\deflang*` → Info (severity-only, 4696 → 4531) |
| `84b3091` | доки RC09 + передача |
| `b0c7499` | **RC04 probe**: resolver накладывает `w:rFonts` по атрибутам и нормализует absent-vs-default; +`test_effective_style_resolver.py` (019: 133→20, 039: 18 — слот `w:cs`) |
| `eac22b2` | доки: откат эксперимента с implicit `\s0` (регрессия pagination 66→63) |
| `aa7f690` | **RC09 `\highlight`** → `w:highlight` по цветовой таблице (меняется только 086) |
| `450319c` | **RC09** `wps:header-whitespace:dropped` → Info (severity-only) |

### Продолжение (2026-10-12)

| Коммит | Суть |
|---|---|
| `2e453ce` | **RC09 picture crop**: `\piccrop{t,b,l,r}` → `a:srcRect` (twips→тысячные процента extent; нулевой crop = нет элемента); `{\*\picprop}` → Info |
| `69a6e73` | **RC09 table styles**: `{\*\tsN}`/`\tsN`/`\tsdN` → Info (WPS всегда пишет built-in Normal Table) |
| `7f28d2b` | **RC09 metadata/numbering**: `{\*\pn}`, `\defchp`/`\defpap`, `\template`, `\datastore`, `\blipuid`, `\updnprop`, `\mmathPr`, `\themedata`/`\colorschememapping` → Info |
| `6f270e0` | **RC09** `wps:leading-group-line-ending` → Info (WPS пишет тот же `&#10;`, body-text совпадает с оракулом) |

**Цифры корпуса сейчас:** import losses **1 580** (было 11 394 на входе сессии,
4 441 после RC09-срезов 2026-10-11, 83 671 после RC06, 127 228 изначально);
resource 100/100; text 99/100 (014 —
WPS field-recompute quirk); `check` 100/100 `ok: strict`; pagination **66/100**;
pixels 6/100 док. / 22/458 стр. Срезы 2026-10-12 severity-only (SHA-256-идентичный
DOCX), кроме picture crop — меняется только 075, без смены номера страниц.

**Единственный красный тест (pre-existing, не наш):**
`strict-ooxml-render-svg::font::family::tests::maps_regular_postscript_names_to_the_same_bundled_faces`.

## 3. Незакоммиченная работа

**Нет.** Дерево чистое.

## 4. Состояние плана (порядок исполнения)

| Этап | Статус |
|---|---|
| RC05 / RC06 | **Done** |
| RC08 columns + field block | **Done**; custom marks/`\ftncn` — explicit non-support; остаётся **same-page continuous geometry** (074) |
| RC04 | resolver-probe **исправлен** (rFonts overlay + default-normalization); остаток диффов 019/039 — genuine (`\sl276` WPS-quirk, `\f2` vs docDefaults, слот `w:cs`). Pixel — **blocked** на faces (RC02/RC03) |
| RC03 | **Blocked** на bidi-oracle с per-char levels |
| RC07 | не начат: bounded WMF/EMF decoder (крупный, high-risk, +RC10) |
| RC09 | **InProgress**: ledger 83 671 → **1 580**. Закрыто 2026-10-12: picture crop (`a:srcRect`), `\picprop`/`\ts`/`\pn`/`\defchp`/`\defpap`/`\template`/`\datastore`/`\blipuid`/`\updnprop`/`\mmathPr`/`themedata`/`colorschememapping` → Info, `wps:leading-group-line-ending` → Info. Остаток — content-классы (shapes `\shp`/OLE `\object`/comments/revisions/textboxes; WPS пишет их как transitional VML), unknown LCID (закреплены тестами), implicit `\s0` (сознательно не материализуется), хвост низкоуверенных флагов |
| RC10 / RC11 / RC12 | не начаты / Blocked / Planned |

## 5. Как проверять (рецепты)

```text
# Сборка и базовые гейты (MSRV!)
cargo +1.92.0 fmt --all -- --check
cargo +1.92.0 clippy -p strict-ooxml-rtf --all-targets
cargo +1.92.0 test -p strict-ooxml-rtf
python -m pytest xtool/rtf/

# Один документ
target/debug/strict-ooxml.exe import-rtf <in.rtf> --out <out.docx> \
    --wps-text --allow-unsigned-unicode --drop-nul
target/debug/strict-ooxml.exe check <out.docx>
target/debug/strict-ooxml.exe to-pdf <out.docx> --out <out.pdf>

# Число страниц
target/rtf-import/poppler/poppler-26.09.0/Library/bin/pdfinfo.exe <out.pdf>

# Корпус: baseline пагинации — target/rtf-import/RC08-columns-layout/corpus-pages.json

# RC04 resolver-probe (resolved effective properties vs WPS reference)
python xtool/rtf/effective_style_resolver.py \
  --our-docx <our.docx> \
  --wps-docx target/rtf-import/RC05-wps-full/oracle/files/019/reference.docx \
  --receipt <r.json>
```

**Свидетели WPS-оракула:** `{028,033,073,099}/reference.docx` (`\ulc`→`w:u/@color`,
`\kerning`→`w:kern`), `{008}` (`\trpadd*`→`w:tblCellMar`, `\trrh`→`w:trHeight`,
`\trautofit`→`w:tblLayout`, `\trqr`→`w:trPr/w:jc`), `{002}/word/fontTable.xml`
(`\panose`→`w:panose1`), `{028,032}/word/settings.xml` (`\pardeftab`→`w:defaultTabStop`),
`{086}/word/document.xml` (`\highlight`→`w:highlight`).

## 6. Уроки этой сессии

1. **WPS-reference — решающий аргумент для «default vs loss».** Перед
   классификацией контрол-слова проверять, пишет ли WPS-эталон соответствующий
   элемент.
2. **Severity-only правки проверять по SHA DOCX по всем 100.** Многие срезы
   (`d61f718`, `dea8eb1`, `450319c`) дали 0 изменений хэшей.
3. **`\falt` не декодировать через text()-путь** (CJK-имена в кодировке гарнитуры,
   `InvalidTextPolicy` по умолчанию `Error`) — оставлен Info; `\panose` безопасен.
4. **`\highlightN` индексирует colortbl**, а не фиксированную палитру (086:
   colortbl 2=yellow, 3=white = имена WPS).
5. **Implicit `\s0` НЕ материализовать.** WPS не делает первый stylesheet-элемент
   `w:default`; попытка неё регрессировала 6 док. (pagination 66→63, 004 117→156).
   Откат — `eac22b2`.
6. **Resolver: `w:rFonts` накладывается по атрибутам**, явный Strict-дефолт равен
   отсутствию; заодно guard на `docDefaults` без `pPrDefault`/`rPrDefault`.
7. **Shared model уже всё нужное имеет** (`RowHeight`/`CellMargins`/`SrcRect`/
   `FontHints.panose1`/`Settings.default_tab_stop`) — большинство маппингов parser-local.
8. **Loss-класс может быть консервативной ошибкой.** `wps:leading-group-line-ending`
   держался как Loss, но декодированный body-text совпадал с WPS-оракулом для всех
   затронутых (005/058/059/075), а WPS пишет тот же `&#10;`. Перед сохранением
   «deliberate loss» проверять его декодированным текстом, не raw XML (2026-10-12).

## 7. Риски / внешние активы

- WPS `reference.docx/pdf/facts` для 100 док. в `RC05-wps-full/oracle/files/` могут
  не сохраниться навсегда — перегенерировать `xtool/rtf/wps_oracle.py --pdf`.
- Реальные faces для 007/010/011/018 не лицензированы (RC03).
- poppler 26.09.0 провижионен.
- Общие model/render/write трогать только с regression по consume-крейтам.

## 8. Рекомендуемый следующий шаг

1. **Продолжить RC09** (оставшийся ledger, 1 580):
   (a) content-классы: shapes (`\shp`), OLE (`\object`/`\objdata`, opaque
       payload/relations без activation), comments (`\annotation`), revisions,
       textboxes. WPS пишет их как transitional `w:pict`/`v:shape` VML, Strict
       его запрещает — нужен DrawingML-маппинг (`Shape`/`TextBox` в модели уже есть);
   (b) хвост низкоуверенных флагов (`\stshf*`, `\clmar*`, `\gaph`, `\fet`,
       `\horzdoc`, `\sectlinegrid`, shape-stroke) — только по WPS-доказательству.
2. Либо **RC07** metafiles (следующий по порядку, крупный, +RC10).
3. Либо RC04 corpus geometry (нужен растеризатор; pixel заблокирован на faces).

---

**Конец передачи.**
