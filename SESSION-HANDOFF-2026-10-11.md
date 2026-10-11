# Передача сессии — RTF-очередь (strict-ooxml)

**Дата:** 2026-10-11
**Ветка:** `codex/rtf-import-integrated`
**Worktree:** `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
**План:** [docs/rtf/REMAINING_MAJOR_WORK.md](docs/rtf/REMAINING_MAJOR_WORK.md),
[REMAINING_COMPATIBILITY_PLAN.md](docs/rtf/REMAINING_COMPATIBILITY_PLAN.md),
карточки `docs/rtf/compatibility/RCxx/{STATUS.json,tests.md}`.
**Правило:** коммиты — только с явного разрешения пользователя. Все команды —
с этим worktree как cwd; `target/` локальный.

> **Состояние на передаче:** рабочее дерево **чистое** — вся работа этой сессии
> закоммичена (см. §2). Незакоммиченного среза RC09 больше нет.

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

## 2. Что сделано и закоммичено

| Коммит | Суть |
|---|---|
| `f9a14f2` | RC05 `\lfolevel`-override уровни → `w:lvlOverride`; RC06 табличные бордеры/shading/cell-props, общий `Borders` (+`between`/`bar`/`tl2br`/`tr2bl`), renderer штрихи `w:pBdr` |
| `016e290` | доки RC05/RC06 |
| `4bf8f81` | **RC08 column layout**: `layout::column_layout`, колоночный менеджер в paginator, `w:br type=column`; single-column fast-path |
| `2fc2e02` | доки RC08 columns |
| `b69ab9a` | **RC08 field block**: block-result `{\field}` → complex marker-форма (`fldChar`/`instrText`); `\flddirty` → `w:dirty` |
| `9a090a0` | доки RC08 field block |
| `6b5ebe7` | `w:customMarkFollows` теперь `Partial`, а не молчаливый drop |
| `3c5ff0e` | доки RC08 custom marks / `\ftncn` |
| `f277519` | **RC09 triage №1**: `\insrsid`/`\charrsid`/`\pararsid`, `\faauto`/`\adjustright`/`\aspalpha`/`\aspnum`, `\hyphpar`/`\wrapdefault`/… → Info (83010 → 11394) |
| `16f3203` | доки RC09 triage №1 + карточка |
| `13c904c` | **RC09 slot mappings**: `\kerning`→`w:kern`, `\ulc`→`w:u/@color`, `\cb`→run `w:shd`; `\trpadd*`→`w:tblCellMar`, `\trrh`→`w:trHeight`, `\trqr/ql/qc`→`w:trPr/w:jc`, `\trautofit`→`w:tblLayout`; `\pardeftab`→`w:defaultTabStop`; `\hyphpar0`/`\nooverflow`/`\cltxlrtb`/… → Info (11394 → 6995) |
| `d0625c3` | **RC09 metadata**: `{\*\panose}`→`w:panose1`; `\falt`/`\hyphen`/`\pnseclvl`/`\rsidtbl`/… → Info (6995 → 5869) |
| `d61f718` | **RC09 settings**: style/table/section rsid, view/cocoa/drawing-grid/math/settings-семейства, `\widowctrl`/`\additive` → Info (severity-only, 5869 → 4696) |
| `dea8eb1` | **RC09 defaults**: `\pardirnatural`, `\deflang`/`\deflangfe`/`\adeflang` → Info (severity-only, 4696 → 4531) |

**Цифры корпуса сейчас:** import losses **4 531** (было 11 394 на входе сессии;
83 671 после RC06; 127 228 изначально); resource 100/100; text 99/100 (014 —
WPS field-recompute quirk); `check` 100/100 `ok: strict`; pagination **66/100**;
pixels 6/100 док. / 22/458 стр. Severity-only срезы дали SHA-256-идентичный DOCX;
slot-маппинг срезы не меняли номера страниц (0 изменений vs `RC08-columns-layout`).

**Единственный красный тест (pre-existing, не наш):**
`strict-ooxml-render-svg::font::family::tests::maps_regular_postscript_names_to_the_same_bundled_faces`
— ждёт `map_family("Arial-BoldMT") == "Arial-BoldMT"`, реализация возвращает
`"Carlito"`. Не трогали.

## 3. Незакоммиченная работа

**Нет.** Дерево чистое.

## 4. Состояние плана (порядок исполнения)

| Этап | Статус |
|---|---|
| RC05 / RC06 | **Done** |
| RC08 columns + field block | **Done**; custom marks/`\ftncn` — explicit non-support; остаётся **same-page continuous geometry** (074) |
| RC04 | resolver-probe: остаток — корректность самого инструмента + нормализация absent-vs-default; WPS-quirk (019). Pixel — **blocked** на faces (RC02/RC03) |
| RC03 | **Blocked** на bidi-oracle с per-char levels |
| RC07 | не начат: bounded WMF/EMF decoder (крупный, high-risk, +RC10) |
| RC09 | **InProgress, большой прогресс**: ledger 83 671 → **4 531**. Остаток — `wps:leading-group-line-ending` (2541; наш LF vs WPS `&#10;`), `\ts` table styles (`w:tblStyle`), picture crop `\piccrop*`/`{\*\picprop}` (`a:srcRect`), content-классы (comments/revisions/textboxes/shapes/OLE/metadata), unknown LCID (закреплены тестами), хвост низкоуверенных application/layout-флагов |
| RC10 / RC11 / RC12 | не начаты / Blocked / Planned |

## 5. Как проверять (рецепты)

```text
# Сборка и базовые гейты (MSRV!)
cargo +1.92.0 fmt --all -- --check
cargo +1.92.0 clippy -p strict-ooxml-rtf --all-targets
cargo +1.92.0 test -p strict-ooxml-rtf

# Один документ
target/debug/strict-ooxml.exe import-rtf <in.rtf> --out <out.docx> \
    --wps-text --allow-unsigned-unicode --drop-nul
target/debug/strict-ooxml.exe check <out.docx>
target/debug/strict-ooxml.exe to-pdf <out.docx> --out <out.pdf>
target/debug/strict-ooxml.exe render <out.docx> --out <svg-dir>

# Число страниц
target/rtf-import/poppler/poppler-26.09.0/Library/bin/pdfinfo.exe <out.pdf>

# Корпус: import + to-pdf + pdfinfo по всем 100; compare к baseline
# (baseline пагинации — target/rtf-import/RC08-columns-layout/corpus-pages.json)

# RC04 resolver-probe (на текущем выходе против WPS reference)
python xtool/rtf/effective_style_resolver.py \
  --our-docx <our.docx> \
  --wps-docx target/rtf-import/RC05-wps-full/oracle/files/019/reference.docx \
  --receipt <r.json>
```

**Свидетели WPS-оракула для новых классификаций:**
`target/rtf-import/RC05-wps-full/oracle/files/{028,033,073,099}/reference.docx`
(`\ulc`→`w:u/@color`, `\kerning`→`w:kern`), `{008}/reference.docx`
(`\trpadd*`→`w:tblCellMar`, `\trrh`→`w:trHeight`, `\trautofit`→`w:tblLayout`,
`\trqr`→`w:trPr/w:jc`), `{002}/word/fontTable.xml` (`\panose`→`w:panose1`),
`{028,032}/word/settings.xml` (`\pardeftab`→`w:defaultTabStop`).

## 6. Уроки этой сессии

1. **WPS-reference — решающий аргумент для «default vs loss».** Перед классификацией
   контрол-слова проверять, пишет ли WPS-эталон соответствующий элемент
   (`w:noWrap` для `\nooverflow`, `w:suppressAutoHyphens` для `\hyphpar0`,
   `w:jс kashida` для `\qk` — пусто ⇒ Info).
2. **Severity-only правки проверять по SHA DOCX по всем 100**, не только по
   `loss_count`: срезы `d61f718`/`dea8eb1` дали 0 изменений хэшей.
3. **`\falt` не декодировать через text()-путь.** Значения в корпусе — CJK-имена в
   кодировке гарнитуры (`\'81\'66`); `InvalidTextPolicy` по умолчанию `Error`,
   поэтому наивный маппинг в `w:altName` ломает импорт 002/004/017/051. `\panose`
   безопасно (чистый hex → ASCII).
4. **LCID 0/1024 закреплены тестами** как явный Loss
   (`unknown_language_lcid_remains_an_explicit_loss`,
   `unknown_alang_lcid_remains_an_explicit_loss`) — не переклассифицировать без
   переписывания этих тестов.
5. **`wps:leading-group-line-ending` — реальный Loss, не false positive.** На 005
   наш текст сохраняет байт как LF, WPS пишет `&#10;` (+замена) — тексты расходятся
   на длину. Оставить Loss.
6. **`Borders`/`RowHeight`/`CellMargins`/`FontHints.panose1`/`Settings.default_tab_stop`
   уже есть в shared model** — большинство маппингов этой сессии parser-local.

## 7. Риски / внешние активы

- WPS `reference.docx/pdf/facts` для 100 док. лежат в
  `RC05-wps-full/oracle/files/` и могут не сохраниться навсегда — при нужде
  перегенерировать (`xtool/rtf/wps_oracle.py --pdf`).
- Реальные faces для 007/010/011/018 не лицензированы (RC03) — блокирует
  corpus witnesses bidi.
- poppler 26.09.0 провижионен.
- Общие model/render/write трогать только с regression по consume-крейтам.

## 8. Рекомендуемый следующий шаг

1. **Продолжить RC09** (наибольший оставшийся ledger):
   (a) `\ts` table styles → `w:tblStyle` (нужна модель table-style; крупно);
   (b) picture crop `\piccrop{t,b,l,r}` → `a:srcRect` (DrawingML, shared model);
   (c) content-классы: comments (`\annotation`), revisions, textboxes/shapes
   (`\shp`), OLE (`\object`/`\objdata` — opaque payload/relations без activation),
   document metadata (`\info`/docprops — показывать в report, не glyph-loss).
2. Либо **RC07** metafiles (следующий по порядку, крупный, +RC10).
3. Либо RC04 resolver-probe fix.

---

**Конец передачи.**
