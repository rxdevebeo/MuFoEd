# Передача сессии — RTF-очередь (strict-ooxml), продолжение

**Дата:** 2026-10-12 (сессия `b`)
**Ветка:** `codex/rtf-import-integrated`
**Worktree:** `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
**Предыдущая передача:** [SESSION-HANDOFF-2026-10-12.md](SESSION-HANDOFF-2026-10-12.md)
**План:** [docs/rtf/REMAINING_MAJOR_WORK.md](docs/rtf/REMAINING_MAJOR_WORK.md),
[REMAINING_COMPATIBILITY_PLAN.md](docs/rtf/REMAINING_COMPATIBILITY_PLAN.md),
карточка [compatibility/RC09](docs/rtf/compatibility/RC09/STATUS.json).
**Правило:** все команды — с этим worktree как cwd; `target/` локальный;
коммиты — с явного разрешения.

> **Состояние на передаче:** рабочее дерево **чистое** — вся работа сессии
> закоммичена (см. §2). Незакоммиченного среза нет.

---

## 1. Контекст

Импорт RTF → Strict DOCX + рендер SVG/PDF. Продолжение §8 предыдущей передачи
(«следующий шаг»): остаток RC09 (grouped shapes, OLE, comments/revisions,
хвост флагов) и пересмотр scope RC07. Общие крейты (`render-svg`, `render-pdf`,
`write`, `wml`, `view`, `convert`) — upstream (`fix/wps-qa-2026-10-12`); наша
ветка их не владеет.

- **MSRV:** `cargo +1.92.0` (обычный `cargo` ниже MSRV).
- **Корпус:** `D:\projects\StrictLib\testdata\CC0_RTF` (100 файлов).
- **WPS-эталоны** (`reference.docx` + `reference.pdf` + `facts.json`):
  `target/rtf-import/RC05-wps-full/oracle/files/<NNN>/`.
- **poppler:** `target/rtf-import/poppler/poppler-26.09.0/Library/bin/{pdfinfo,pdftoppm}.exe`.
- **CLI:** `target/debug/strict-ooxml.exe` (`cargo +1.92.0 build -p strict-ooxml-cli`).
- **Python:** 3.11; `python -m pytest xtool/rtf/`.

## 2. Что сделано и закоммичено

| Коммит | Суть |
|---|---|
| `151e3ed` | **RC09 `\shppict`/`\nonshppict`**: `{\*\shppict{\pict …}}` вне shape теперь импортируется (картинка попадает в пакет); `{\nonshppict{\pict …}}` (metafile-fallback) → named Info. Свидетели 003 (+1 PNG) и 005 (+4 JPEG; раньше `media_parts` пуст) |
| `c9e7a10` | **RC09 OLE `{\object …}`**: кэшированный `\result`-превью импортируется как картинка; `\objemb`/`\objw`/`\objh` и `{\*\objclass}`/`{\*\oleclsid}`/`{\*\objname}`/`{\*\objtime}` → Info; `{\*\objdata}` payload остаётся явным Loss. Свидетель 008 (+9 WMF-превью, losses 115→27) |
| `b52bef5` | **RC09 `\shpgrp` → `wpg:wgp`**: group transform + дочерние `{\shp …}` → `wps:wsp` с `relLeft/relTop` (или `geoRight/geoBottom`) в локальных координатах группы. Свидетель 092 (+3 группы, 8 `wps:wsp`) |
| `535af60` | **RC09 character stroke**: `\strokewidth0` (единственное значение в корпусе) и его `\strokec` → Info (у Strict `w:outline` нет ширины/цвета, WPS не пишет stroke); положительный `\strokewidth` → Loss |
| (docs) | карточка RC09, `REMAINING_MAJOR_WORK.md` (RC09 + RC07 scope), эта передача |

**Цифры корпуса сейчас (receipt `target/rtf-import/rc09-final3/`).**
import losses **1 489 → 1 371 (−118)**; resource 100/100; `check` 100/100
`ok: strict`; pagination **66/100** (меняются страницы только 005 270→272 и
008 56→37; оба и раньше не совпадали с ref). DOCX SHA-256 меняется ровно у
003/005/008/092 (новые media/группы); stroke-срез severity-only → SHA идентичен.

**Остаток топ-классов (1 371):** `language:unknown-lcid:1024` 217 (+ `:255` 47,
`:0` 10), `style:undefined-parent:rtf-s0` 56, `undefined-font:6` 32,
`\clmar*` 104, `\gaph` 26, `\stshf*` 80, `control:fet` 22, `stylesheet:unbound-text`
20, `\snext` 18, `\adeff` 17, `\horzdoc` 16, `\sectlinegrid` 14, … (все —
сознательно не переклассифицированный хвост; см. §4).

**Тесты.** `cargo +1.92.0 test -p strict-ooxml-rtf` — 86+37 passed;
`clippy -p strict-ooxml-rtf --all-targets` — чисто; `fmt --all -- --check` —
чисто. Новые тесты:
`shppict_pair_imports_the_primary_picture_and_names_the_fallback`,
`embedded_ole_object_imports_its_result_picture_and_names_the_payload`,
`grouped_shapes_map_to_an_anchored_wpg_group`,
`zero_width_character_stroke_is_info_and_a_real_stroke_is_a_loss`.

## 3. Незакоммиченная работа

**Нет.** Дерево чистое (`git status --porcelain` пуст).

## 4. Состояние плана

| Этап | Статус |
|---|---|
| RC05 / RC06 / RC08 columns+field | Done |
| RC04 | resolver-probe исправлен; остаток 019/039 genuine; pixel blocked на faces |
| RC03 | Blocked на bidi-oracle; render — upstream |
| RC07 | **scope пересмотрен:** decoder — upstream `render-svg::metafile` (`emfsdk`), подключён к `paint/image.rs`; остаётся интеграционная проверка RTF WMF/EMF (в т.ч. OLE-превью 008) + RC10. Раньше числился «не начат» |
| RC09 | InProgress: 1 489 → **1 371**. Остаток: comments/revisions/textboxes (в корпусе нет — нужны fixtures), language-sentinels, implicit `\s0`, хвост флагов |
| RC10 / RC11 / RC12 | идёт с декодерами / Blocked (WPS-harness) / Planned |

**Хвост, сознательно оставленный Loss (нужен WPS-proof):** `\newtblstyruls`,
`\notbrkcnstfrctbl`, `\indrlsweleven`, `\sectlinegrid`, `\horzdoc`, `\stshf*`,
`\clmar*`, `\gaph`, `\fet`.

## 5. Как проверять (рецепты)

```text
# Гейты (MSRV!)
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

# Корпус целиком: import-rtf в target/rtf-import/<slice>/ для all 100,
# сумма j.import.loss_count + SHA-256 docx.
# baseline пагинации — target/rtf-import/RC08-columns-layout/corpus-pages.json
```

**Свидетели:** 003/005 (`\shppict`), 008 (OLE `\result`, 9 WMF),
092 (`\shpgrp`, 3 группы). Receipts сессии: `target/rtf-import/rc09-final2/`,
`target/rtf-import/rc09-final3/`, `target/rtf-import/rc09-shppict/`,
`rc09-ole/`, `rc09-shpgrp/`.

## 6. Уроки этой сессии

1. **`\shppict` вне shape был молчаливой потерей картинок.** Гейт
   `self.shape.is_some()` пропускал `\shppict` в общий skip-list. Проверять
   `media_parts`/`word/media` в отчёте, не только `loss_count`: 005 отдавал 0
   media при «нормальном» ledger.
2. **`\object` содержит картинку-превью (`\result`).** Для content-классов ООО
   сначала смотреть, есть ли визуальный fallback; payload (`\objdata`) можно
   честно оставить Loss, но превью возвращать.
3. **Групповые shapes требуют отдельного collector'а.** `\shpgrp` держит
   `ShapeBuilder` группы + `Vec<Graphic>` детей; `\shp`-дети используют общий
   `finish_shape`, но при открытой группе кладут `Graphic` в неё, а не якорят.
   Геометрия детей — `relLeft/relTop` (+`geoRight/geoBottom`) относительно
   начала группы.
4. **`\strokec` без положительного `\strokewidth` невидим.** Трекать ширину в
   `State`, а не классифицировать цвет безусловно.
5. **Upstream покрыл RC07.** Перед стартом «нового decoder» — проверить, не
   вмерджил ли upstream (`render-svg/src/metafile.rs`, `emfsdk`).
6. **Severity-only срезы проверять по SHA DOCX по всем 100** (stroke-срез дал 0
   изменений хэшей; SHA изменился только у 4 затронутых content-документов).

## 7. Риски / внешние активы

- WPS `reference.docx/pdf/facts` в `RC05-wps-full/oracle/files/` могут не
  сохраниться — перегенерировать `xtool/rtf/wps_oracle.py --pdf`.
- Реальные faces для 007/010/011/018 не лицензированы (RC03).
- poppler 26.09.0 провижионен локально.
- **После merge любые правки общих model/render/write — только согласованно и
  с regression по consume-крейтам.**
- `\clmar*` = 104 losses в одном документе (073): значения `10`, всегда рядом с
  `\clpad*20`; WPS для 073 не пишет `w:tcMar` вообще — семантику (`w:tcMar`?)
  не подтвердили, оставлено Loss.

## 8. Рекомендуемый следующий шаг

1. **Комментарии/ревизии/textboxes** — в корпусе не наблюдаются; собрать
   portable fixtures (`{\annotation …}`, `\revised`/`\revauth`/`\revdttm`,
   textbox `w:txbxContent` вне shape) и смапить в DrawingML/`w:comment*`.
2. **Хвост флагов по WPS-proof:** проверить `\fet`→`w:footnotePr`/`w:endnotePr`
   pos, `\adeff`→`w:docDefaults`, `\stshf*`→`w:docDefaults`/theme, `\clmar*`→
   `w:tcMar`; при отсутствии Strict-слота/WPS-следа — оставить Loss.
3. **RC07-интеграция:** подтвердить, что RTF WMF/EMF (OLE 008, `\shppict`
   метафайлы) растеризуются upstream-путём в SVG и PDF, бюджет bounds.
4. **RC04/RC11** — приёмка/геометрия (нужны растеризатор и WPS-harness).

---

**Конец передачи.**
