# Передача сессии — RTF-очередь (strict-ooxml)

**Дата:** 2026-10-12
**Ветка:** `codex/rtf-import-integrated`
**Worktree:** `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
**Предыдущая передача:** [SESSION-HANDOFF-2026-10-11.md](SESSION-HANDOFF-2026-10-11.md)
**План:** [docs/rtf/REMAINING_MAJOR_WORK.md](docs/rtf/REMAINING_MAJOR_WORK.md),
[docs/rtf/REMAINING_COMPATIBILITY_PLAN.md](docs/rtf/REMAINING_COMPATIBILITY_PLAN.md),
карточки `docs/rtf/compatibility/RCxx/{STATUS.json,tests.md}`.
**Правило:** все команды — с этим worktree как cwd; `target/` локальный;
коммиты — с явного разрешения.

> **Состояние на передаче:** рабочее дерево **чистое** — вся работа этой сессии
> закоммичена (см. §2). Незакоммиченного среза нет.

---

## 1. Контекст

Импорт RTF → Strict DOCX + рендер SVG/PDF с независимой приёмкой на корпусе
100 RTF. Владею `strict-ooxml-rtf/**`, `docs/rtf/**`, `xtool/rtf/**`, а также
RTF-модулем фасада (`strict-ooxml/src/rtf.rs`, `strict-ooxml-cli/src/rtf.rs`).

**Важно (после merge 2026-10-12).** Общие крейты (`render-svg`, `render-pdf`,
`write`, `wml`, `view`, `convert`) теперь берутся у `fix/wps-qa-2026-10-12`
(upstream wins). Наша ветка **не владеет** их внутренностями: правки в них —
только согласованно и с regression. Подробности §2/§6.

- **MSRV:** `cargo +1.92.0` (обычный `cargo` ниже MSRV).
- **Корпус:** `D:\projects\StrictLib\testdata\CC0_RTF` (100 файлов, из git
  исключён).
- **WPS-эталоны** (`reference.docx` + `reference.pdf` + `facts.json`):
  `target/rtf-import/RC05-wps-full/oracle/files/<NNN>/`.
- **poppler:** `target/rtf-import/poppler/poppler-26.09.0/Library/bin/{pdfinfo,pdftoppm}.exe`.
- **CLI:** `target/debug/strict-ooxml.exe` (`cargo +1.92.0 build -p strict-ooxml-cli`).
- **Python:** 3.11; `python -m pytest xtool/rtf/`.

## 2. Что сделано и закоммичено

### Начало сессии — RC09-срезы (до merge)

| Коммит | Суть |
|---|---|
| `2e453ce` | **RC09 picture crop**: `\piccrop{t,b,l,r}` → `a:srcRect` (twips→тысячные процента extent; нулевой crop = нет элемента); `{\*\picprop}` → Info |
| `69a6e73` | **RC09 table styles**: `{\*\tsN}`/`\tsN`/`\tsdN` → Info (WPS всегда пишет built-in Normal Table) |
| `7f28d2b` | **RC09 metadata/numbering**: `{\*\pn}`, `\defchp`/`\defpap`, `\template`, `\datastore`, `\blipuid`, `\updnprop`, `\mmathPr`, `\themedata`/`\colorschememapping` → Info |
| `6f270e0` | **RC09** `wps:leading-group-line-ending` → Info (был консервативный Loss; WPS пишет тот же `&#10;`, body-text совпадает с оракулом) |
| `e2990dc` | доки: срез + передача `SESSION-HANDOFF-2026-10-11.md` |

### Синхронизация с основной линией

| Коммит | Суть |
|---|---|
| `5274d07` | **merge `fix/wps-qa-2026-10-12`**. Upstream wins для shared. RTF-работа сохранена и адаптирована под модель upstream: `RunContent::NoteSeparator(bool)` (вместо `Separator`/`ContinuationSeparator`), диагонали ячеек → `CellProperties.diagonal_down/up` (вместо `Borders.tl2br/tr2bl`); новые поля `Style.aliases`, `AbstractNum.name`, `DocPr.hidden`, `Picture.markup`, `FieldChar.form_data`, `RowProperties.header: TriState` + `cant_split_state`. Колоночный менеджер и font-asset-подсистема (`font/{policy,registry,resource}`, `--fonts`) отброшены (у upstream свои). `import-rtf` сохранён в CLI. |

### RC09 — floating shapes → DrawingML

| Коммит | Суть |
|---|---|
| `0e67950` | **RC09 floating `\shp` shapes → `wp:anchor`**: `\shpinst`-геометрия (`\shpleft/top/right/bottom`, `\shpbx/by`, `\shpwr`, `\shpz`, `\shpfblwtxt`) → `wp:positionH/V`/`wp:wrap*`/`relativeHeight`/`behindDoc`; `{\sp{\sn}{\sv}}` → preset-геометрия/fill/outline/name; `\shptxt` → `wps:txbx` (с полями); вложенный `{\pict}` (`pib`/`fillBlip`/`\shppict`) → `pic:pic` с сохранением media; хинты без слота и blip-id → Info; `\shpgrp` — Loss |
| `df27348` | доки: shape-маппинг, RC09-карточка/`tests.md`/план/передача |

**Цифры корпуса сейчас.** import losses **1 489** (было 127 228 изначально,
83 671 после RC06, 11 394 на входе прошлой сессии, 4 441 после RC09-срезов
2026-10-11, 1 580 после продолжения до shape-маппинга); resource 100/100;
`check` 100/100 `ok: strict`; pagination **66/100**; pixels 6/100 док. /
22/458 стр. Срезы severity-only дают SHA-256-идентичный DOCX; picture crop
меняет только 075, shape-маппинг — только 003/017/075/078/092 (без смены
числа страниц).

**Тесты.** `cargo +1.92.0 test -p strict-ooxml-rtf` — 82+37 passed;
`cargo +1.92.0 clippy -p strict-ooxml-rtf --all-targets` — чисто;
`cargo +1.92.0 fmt --all -- --check` — чисто; `pytest xtool/rtf/` — 35 passed;
facade/cli rtf-тесты — 5/2 passed. Red-тест прошлой сессии
(`render-svg::font::family::tests::maps_regular_postscript_names_to_the_same_bundled_faces`)
после merge **зелёный** (взят upstream `family.rs`).

## 3. Незакоммиченная работа

**Нет.** Дерево чистое (`git status --porcelain` пуст).

## 4. Состояние плана (порядок исполнения)

| Этап | Статус |
|---|---|
| RC05 / RC06 | **Done** |
| RC08 columns + field block | **Done**; custom marks/`\ftncn` — explicit non-support; остаётся **same-page continuous geometry** (074) — теперь в render-svg upstream |
| RC04 | resolver-probe исправлен; остаток диффов 019/039 — genuine; pixel **blocked** на faces (RC02/RC03) |
| RC03 | **Blocked** на bidi-oracle с per-char levels; render — upstream |
| RC07 | не начат. Замечание: upstream вмерджил `render-svg/src/metafile.rs` — пересмотреть scope перед стартом |
| RC09 | **InProgress**: ledger 83 671 → **1 489**. Остаток — `\shpgrp`-группы, OLE `\object`/`\objdata`, comments `\annotation`, revisions, textboxes вне shape; unknown LCID (закреплены тестами); implicit `\s0` (сознательно не материализуется); хвост низкоуверенных флагов |
| RC10 / RC11 / RC12 | идёт с декодерами / Blocked (WPS-harness) / Planned |

**Топ остатка RC09 (1 489):** `language:unknown-lcid:1024` 221,
`control:strokewidth` 68, `style:undefined-parent:rtf-s0` 65,
`language:unknown-lcid:255` 47, `control:strokec` 45, `undefined-font:6` 32,
`\clmar*` 104, `\gaph` 26, `\fet` 22, `\stshf*` 80, `\sectlinegrid` 20,
`stylesheet:unbound-text` 20, `\snext` 18, `\adeff` 17, `\horzdoc` 16, …

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

# Корпус целиком (losses/SHA): писать report.json в target/rtf-import/<slice>/
# baseline пагинации — target/rtf-import/RC08-columns-layout/corpus-pages.json

# RC04 resolver-probe
python xtool/rtf/effective_style_resolver.py \
  --our-docx <our.docx> \
  --wps-docx target/rtf-import/RC05-wps-full/oracle/files/019/reference.docx \
  --receipt <r.json>
```

**Свидетели WPS-оракула:** `{028,033,073,099}` (`\ulc`→`w:u/@color`,
`\kerning`→`w:kern`), `{008}` (`\trpadd*`→`w:tblCellMar`, `\trrh`→`w:trHeight`,
`\trautofit`→`w:tblLayout`, `\trqr`→`w:trPr/w:jc`), `{002}` (`\panose`→`w:panose1`),
`{028,032}` (`\pardeftab`→`w:defaultTabStop`), `{086}` (`\highlight`→`w:highlight`),
`{003,017,075,078,092}` (shapes: WPS пишет transitional `w:pict`/`v:shape`).
Receipts сессии: `target/rtf-import/rc09-{slice9..12,shapes}/`.

## 6. Уроки этой сессии

1. **Синхронизация была глубоким merge'ом.** `fix/wps-qa` и наша ветка
   независимо переписали render/font/layout (колонки, shaper, embedded fonts,
   note separators, диагонали). Приняли **upstream wins для shared**; нашу
   RTF-работу адаптировали под модель upstream. Следствие: наши font-asset
   registry и старый колоночный менеджер отброшены — при необходимости их надо
   перепортить на API upstream (в исходном плане такого пункта не было).
2. **Loss-класс может быть консервативной ошибкой.** `wps:leading-group-line-ending`
   держался как Loss, но декодированный body-text совпадал с WPS-оракулом для
   всех затронутых (005/058/059/075). Перед сохранением «deliberate loss»
   проверять **декодированным текстом**, не raw XML.
3. **Флаг `first` + destination гейт.** Регион-обработчик (`\field`/`\footnote`/
   `\header`) открывался только при `destination == Body`; для shape-текста
   (`\shptxt`) пришлось включить `Destination::ShapeText` в гейт.
4. **Парсинг нового destination обнажает вложенные контролы.** Как только
   `\shp` перестал skip'аться, наружу полезли shape-хинты (`\posx`, `\pvpara`,
   `\wraparound`, `\dfrmtxt*`, blip-id) — их пришлось классифицировать в Info,
   иначе ledger вырос бы. Проверять `feature_counts` до/после, не только total.
5. **`\shpinst`/`\shppict`/`\nonshppict` — `\*`-ignorable.** Внутри shape их
   нельзя skip'ать (иначе теряется содержимое); перехватываем до ignorable-ветки.
6. **Severity-only правки проверять по SHA DOCX по всем 100.** Многие срезы
   (`2e453ce` кроме 075, `6f270e0`) дали 0 изменений хэшей.
7. **Implicit `\s0` не материализовать** (регрессия pagination 66→63) — остаётся
   deliberate Loss.
8. **Resolver: `w:rFonts` накладывается по атрибутам**, явный Strict-дефолт
   равен отсутствию.

## 7. Риски / внешние активы

- WPS `reference.docx/pdf/facts` в `RC05-wps-full/oracle/files/` могут не
  сохраниться — перегенерировать `xtool/rtf/wps_oracle.py --pdf`.
- Реальные faces для 007/010/011/018 не лицензированы (RC03).
- poppler 26.09.0 провижионен локально.
- **После merge любые правки общих model/render/write — только согласованно и
  с regression по consume-крейтам** (мы больше не владелец этих подсистем).

## 8. Рекомендуемый следующий шаг

1. **Продолжить RC09** (оставшийся ledger, 1 489):
   (a) `\shpgrp` групповые shapes и OLE (`\object`/`\objdata`; opaque
       payload/relations без activation);
   (b) comments (`\annotation`), revisions, textboxes вне shape;
   (c) хвост низкоуверенных флагов (`\stshf*`, `\clmar*`, `\gaph`, `\fet`,
       `\horzdoc`, `\sectlinegrid`, character-stroke) — только по
       WPS-доказательству.
2. Либо **RC07** metafiles (сначала пересмотреть scope: upstream мог покрыть).
3. Либо **RC04/RC11** — приёмка/геометрия (нужны растеризатор и WPS-harness).

---

**Конец передачи.**
