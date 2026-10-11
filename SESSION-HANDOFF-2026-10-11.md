# Передача сессии — RTF-очередь (strict-ooxml)

**Дата:** 2026-10-11
**Ветка:** `codex/rtf-import-integrated`
**Worktree:** `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
**План:** [docs/rtf/REMAINING_MAJOR_WORK.md](docs/rtf/REMAINING_MAJOR_WORK.md),
[REMAINING_COMPATIBILITY_PLAN.md](docs/rtf/REMAINING_COMPATIBILITY_PLAN.md),
карточки `docs/rtf/compatibility/RCxx/{STATUS.json,tests.md}`.
**Правило:** коммиты — только с явного разрешения пользователя. Все команды —
с этим worktree как cwd; `target/` локальный.

> **Состояние на передаче:** в дереве лежит **незакоммиченный** срез RC09
> (см. §3). Всё остальное из сессии закоммичено (§2).

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
| `4bf8f81` | **RC08 column layout**: `layout::column_layout`, колоночный менеджер в paginator, `w:br type=column` (`Flow::ColumnBreak`); single-column fast-path |
| `2fc2e02` | доки RC08 columns |
| `b69ab9a` | **RC08 field block**: block-result `{\field}` (и `\flddirty`) → complex marker-форма `fldChar begin/instrText/separate/…/end`; `\fldedit`/`datafield` → Info |
| `9a090a0` | доки RC08 field block |
| `6b5ebe7` | `w:customMarkFollows` теперь `Partial`, а не молчаливый drop |
| `3c5ff0e` | доки RC08 custom marks / `\ftncn` |

**Цифры корпуса сейчас:** import losses **11 394**; resource 100/100; text
99/100 (014 — WPS field-recompute quirk); `check` 100/100 `ok: strict`;
pagination **66/100** (092 закрыт колонками); pixels 6/100 док. / 22/458 стр.
(переносятся по идентичности PDF; свежий raster требует эталонов).

**Единственный красный тест (pre-existing, не наш):**
`strict-ooxml-render-svg::font::family::tests::maps_regular_postscript_names_to_the_same_bundled_faces`
— ждёт `map_family("Arial-BoldMT") == "Arial-BoldMT"`, реализация возвращает
`"Carlito"`. `map_family` реально вызывается с styled PostScript-именами
(`builtin.rs`), так что там возможен настоящий баг, но интерпретация теста
неоднозначна. Не трогали.

## 3. Незакоммиченная работа (главное на сейчас)

Срез **RC09 triage №1** — ревизионные save-ID и legacy layout-контролы,
чья bare-форма равна Strict-дефолту, классифицированы Info:

```
M strict-ooxml-rtf/src/parser.rs      # rsid-compat / word6-compat / paragraph-compat
M strict-ooxml-rtf/tests/structure.rs  # 2 теста
M docs/rtf/REMAINING_MAJOR_WORK.md
M docs/rtf/REMAINING_COMPATIBILITY_PLAN.md
?? docs/rtf/compatibility/RC09/        # STATUS.json + tests.md (новая карточка)
```

- `\insrsid`/`\charrsid`/`\pararsid` → `rsid-compat` Info (невизуальные
  merge-метаданные).
- `\faauto`/`\adjustright`/`\aspalpha`/`\aspnum` (bare) → `word6-compat` Info
  (это Strict-дефолты `w:textAlignment=auto`/`w:adjustRightInd` on/
  `w:autoSpaceDE`/`DN` on).
- `\hyphpar`/`\wrapdefault`/`\sectdefaultcl`/`\linex`/`\lnbrkrule`/
  `\partightenfactor` → `paragraph-compat` Info.
- Non-default формы (`\hyphpar0`, `\adjustright0`) **остаются Loss** (нет слота
  в paragraph model).

**Результат:** import losses **83010 → 11 394** (−71 616). Изменение только
severity → DOCX байт-идентичен (SHA-256 совпал на 014/074/092), pagination/
pixels без изменений. Тесты `strict-ooxml-rtf` 70 Pass; clippy/fmt чисто.
Receipts — `target/rtf-import/rc09-triage/`.

**Действие:** прогнать `fmt --check` + `clippy -p strict-ooxml-rtf --all-targets`
+ `test -p strict-ooxml-rtf` и **закоммитить** (код + доки), с разрешения.

## 4. Состояние плана (порядок исполнения)

| Этап | Статус |
|---|---|
| RC05 / RC06 | **Done** (закоммичено) |
| RC08 columns + field block | **Done**; custom marks/`\ftncn` — explicit non-support; остаётся **same-page continuous geometry** (074: 4 стр. против ref 3, 2-column секция — continuous break, откладывается на след. страницу) |
| RC04 | resolver-probe: остаток — корректность самого инструмента (`resolve` заменяет свойство целиком, не по атрибутам) + нормализация absent-vs-default; есть WPS-quirk (019 теряет `\sl276`). Pixel — **blocked** на faces (RC02/RC03) |
| RC03 | **Blocked** на bidi-oracle с per-char levels |
| RC07 | не начат: bounded WMF/EMF decoder (крупный, high-risk, +RC10) |
| RC09 | **InProgress** этим срезом: остаток — table/char-контролы с WML-слотом (`\trpadd*`→`w:tblCellMar`, `\ulc`→`w:u/@w:color`, `\cltxlrtb`→`w:textDirection`, `\pardeftab`, `\nooverflow`/`\nocwrap`, `\trrh`/`\trautofit`/`\trftsWidth`) и content-классы (comments/revisions/textboxes/OLE/metadata) |
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
# (см. target/rtf-import/rc08-field-verify2/corpus/pages.json для примеров)

# RC04 resolver-probe (на текущем выходе против WPS reference)
python xtool/rtf/effective_style_resolver.py \
  --our-docx <our.docx> \
  --wps-docx target/rtf-import/RC05-wps-full/oracle/files/019/reference.docx \
  --receipt <r.json>
```

## 6. Уроки этой сессии

1. **Severity-only правки проверять по SHA DOCX.** Классификация Loss→Info не
   должна менять вывод: сравнивать `Get-FileHash` до/после, а не только
   `loss_count`.
2. **Baseline пагинации — `RC06-final3`** (свежий WPS-оракул), а не старые
   receipt'ы: `RC08-columns-visual` устарел на RC05/RC06 (017/053 там другие).
3. **`column_layout` — single-column fast-path:** пустой список колонок и
   нулевой offset держат старую раскладку byte-identical; проверять, что
   меняется ровно один документ (092).
4. **Marker-форма сложного поля уже поддержана** model/reader/writer/layout;
   для block-result `\field` не нужен `Field { blocks }`.
5. **`\flddirty`/`datafield`/`\fldedit`** в 014 были чистым Loss; `\flddirty` →
   `w:dirty` на begin-маркере, остальные — Info.
6. **Probe ≠ gate.** `effective_style_resolver` — probe; его «0 mismatches»
   достигается ручным рассуждением, а не самим инструментом (см. §4 RC04).

## 7. Риски / внешние активы

- WPS `reference.docx/pdf/facts` для 100 док. лежат в
  `RC05-wps-full/oracle/files/` и могут не сохраниться навсегда — при нужде
  перегенерировать (`xtool/rtf/wps_oracle.py --pdf`).
- Реальные faces для 007/010/011/018 не лицензированы (RC03) — блокирует
  corpus witnesses bidi.
- poppler 26.09.0 провижионен.
- Общие model/render/write трогать только с regression по consume-крейтам.

## 8. Рекомендуемый следующий шаг

1. **Закоммитить незакоммиченный срез RC09** (код + доки).
2. Затем — на выбор: (a) **продолжить RC09** — table/char-контролы с реальными
   WML-слотами (наибольший оставшийся ledger, ~11k), (b) **RC07** metafiles
   (следующий по порядку, крупный), (c) RC04 resolver-probe fix.

---

**Конец передачи.**
