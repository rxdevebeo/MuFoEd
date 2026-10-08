# Переход тестов на корпуса CC0

Дата: 7 октября 2026 года. Статус: фазы 0–2 выполнены в ветке
`task/hardening-2026-10-07` (см. §12); фазы 3–7 — план.

## 1. Цель и решение

Заменить в тестах, гейтах и CI закрытый корпус `strict-ooxml-core/tests/docx/`
(99 Transitional `.docx`, в `.gitignore` с первого коммита, происхождение и
лицензии не установлены) на три локальных корпуса CC0 из `testdata/`, у каждого
из которых есть манифест с `download_url` и `sha256`.

Решение:

1. **Байты корпуса не коммитятся.** В репозиторий входит только lock-файл
   (идентификатор, путь, размер, SHA-256, URL, уровень). Документы скачиваются
   по lock-файлу с обязательной проверкой хеша и кешируются в CI.
2. **Одна точка доступа к корпусу** в `strict-ooxml-testkit` вместо 27 ручных
   `env!("CARGO_MANIFEST_DIR").join("../strict-ooxml-core/tests/…")`.
3. **Два уровня:** `ci-core` (27 документов, 10,9 MB) гоняется в каждом CI;
   `ci-full` (все 300 документов, ~255 MB) — ночной job и census.
4. **Тест, которому нужен документ, никогда не делает `.expect()` на его
   наличие.** Отсутствие корпуса — это либо явный skip с сообщением, либо явная
   ошибка в режиме `require`; режим выбирает окружение, а не тест.
5. **Закреплённые значения переснимаются из записей документа** (XML части,
   независимый zip-скан), а не из вывода нашего парсера и не с картинки.
6. **`tests/strict/` и `tests/samples/` остаются и используются как есть**
   (см. §8). Оба коммитятся и доступны в CI. `tests/strict/` — синтетический
   Strict-корпус (кроме одного-двух файлов): готовых Strict-документов в природе
   почти нет, и CC0 их не даёт. `tests/samples/` — документы с сайтов примеров,
   использование не ограничено. Переход касается только закрытого
   `tests/docx/`.

Закрывает: waiver `CENSUS-LOCAL` (`docs/waivers.toml:448`), красный CI на чистом
клоне (§3.2), часть D05 (census в CI).

## 2. Исходные факты

Проверено 2026-10-07 по файловой системе.

### 2.1. Корпуса

| Корпус | Документов | Объём | Языков (top) | Хеши |
|---|---:|---:|---|---|
| `testdata/CC0` | 100 | 38,8 MB | English 11, Arabic 8, Spanish 7, Chinese 7, Telugu 7; 12 без языка | 100/100 совпадают |
| `testdata/CC0_DOCX` | 100 | 114,3 MB | Swedish 8, Arabic 7, Turkish 7, English 7, Indonesian 6 | 100/100 |
| `testdata/CC0_DOCX_1` | 100 | 101,8 MB | Tamil 20, English 20, Swedish 11, Spanish 10, Arabic 9, Russian 9 | 100/100 |

- Пересечений между корпусами нет ни по `sha256`, ни по `identifier`: это 300
  разных документов.
- Все `download_url` ведут на `archive.org`. Все `license_url` — CC0 1.0
  (http/https-варианты).
- Схемы манифестов различаются: у `CC0` поле имени файла — `local_filename`,
  есть `license_basis`, `script_hint`; у `CC0_DOCX` — `filename`, `complexity`,
  `tables`, `textboxes`; у `CC0_DOCX_1` — `filename`, `page_count`, `has_math`.
  Lock-файл приводит их к одной схеме.
- Все 300 — Transitional, ни одного Strict. Битых пакетов нет.
- Источник документов (`docProps/app.xml`): Microsoft Word 222+24+8, без
  указания 20, WPS 5, Outlook 3, LibreOffice 2.
- Поле `language` ненадёжно (например, вьетнамский экзаменационный лист помечен
  как Cornish); для подбора по письменности использовать скан текста, не поле.

### 2.2. Покрытие конструкций (документов из 300)

| Конструкция | Документов | Замечание |
|---|---:|---|
| `w:cols` / колонтитулы (`headerReference`) | 251 / 100 | достаточно |
| `w:drawing` / `wp:anchor` / `mc:AlternateContent` | 182 / 66 / 31 | достаточно |
| VML `w:pict` / `v:textbox` | 82 / 29 | достаточно |
| Таблицы / `gridSpan` / `vMerge` / `tblHeader` | 79 / 26 / 21 / **4** | повтор шапки таблицы — мало |
| Поля `fldChar` | 78 | |
| RTL (`w:rtl` / `w:bidi`) | 70 / 47 | |
| `w:sdt` / `customXml` / glossary | 49 / 137 / 6 | |
| Сноски / **концевые сноски** | 25 / **2** | |
| Комментарии | 27 | |
| `m:oMath` | **8** | |
| OLE `o:OLEObject` | **4** | |
| WMF/EMF | 19 | |
| Встроенные шрифты `word/fonts/` | **7** | |
| Группы `wpg:wgp` / `pctPosVOffset` | **4 / 1** | |
| **Правки** `w:ins` / `w:del` | **2 / 1** | пробел |
| **Диаграммы** `word/charts/` | **2** | пробел |
| **SmartArt** `word/diagrams/` | **1** | пробел |
| Strict | **0** | не заменяет `tests/strict/` |

Пробелы (правки, диаграммы, SmartArt, концевые сноски, Strict) закрываются
синтетическими fixtures, а не корпусом (§7, фаза 7).

### 2.3. Правовые оговорки

- CC0 на archive.org — заявление загрузившего, а не результат правовой
  проверки. Часть документов (религиозные тексты, экзаменационные листы,
  гайды) может принадлежать не загрузившему.
- Поэтому: байты не коммитятся и не публикуются как артефакты CI; lock-файл
  хранит только метаданные и ссылку; документ, по которому поступила претензия
  или у которого исчезла CC0-пометка, исключается из lock-файла одной строкой.
- Содержимое документов не цитируется в тестах, отчётах и snapshot-файлах:
  закрепляются структурные значения (id, атрибуты, размеры, дайджесты), а не
  текст.
- Перед включением документа в `ci-core` выполнить ручную проверку страницы
  archive.org: пометка CC0 на месте, загружавший — правдоподобный автор.

## 3. Что сейчас зависит от закрытого корпуса

Полный инвентарь: 37 потребителей. Классы: **A** — обходит каталог целиком;
**B** — закрепляет документ по имени; **C** — закрепляет документ и точные
значения.

### 3.1. Сводка

| Класс | Потребителей | Примеры |
|---|---:|---|
| A | 14 | `core/tests/docx_corpus.rs`, `write/tests/normalize_roundtrip.rs`, `write/tests/opc_oracle.rs`, `render-svg/tests/corpus.rs`, `census_gate.py`, `fuzz/seed_corpus.sh` |
| B | 4 | `core/tests/parallel_report.rs`, `report/tests/normalization.rs`, `render-pdf/tests/p6_table_page.rs` |
| C | 12 Rust-тестов + 1 python-гейт | `p5_p6`, `f06_toggles`, `p7_p8`, `p9_fonts`, `p10_media`, `p11_graphics`, `softuni_position_v`, `wps_ledger.py` |
| Архивные скрипты аудита | ~10 | `docs/audit-2026-10-04/visual-*/reproduce.py` |

Общего слоя доступа к корпусу нет; переменной окружения тоже нет. Ближайшие
заготовки: `p10_media.rs:15 fn corpus()`, `p5_p6.rs:144 fn witness()`,
`strict-ooxml-view/src/main.rs:47 KNOWN_CORPORA`, `census_gate.py:151 CORPORA`.

### 3.2. Срочное: CI на чистом клоне

CI-шаг `Test` (`.github/workflows/ci.yml:41`, `cargo test --workspace
--all-features`, три ОС) запускает все интеграционные тесты. Каталоги
`tests/docx/` и `/testdata/` в `.gitignore`, `#[ignore]` нет. **11 тестов в 7
файлах падают через `.expect()` на отсутствующем файле:**

| Тест | Документ |
|---|---|
| `render-pdf/tests/p6_table_page.rs:17` | `RM0090 16-23…` |
| `render-svg/tests/p5_p6.rs:196, :261` | `RM0090 16-23…` |
| `render-svg/tests/f06_toggles.rs:295` | `Clio Der Sarkissian…2011` |
| `wml/tests/softuni_position_v.rs:17` | `4. Complex-Conditions` |
| `write/tests/p11_graphics.rs:27` (не закоммичен) | `1. First-Steps-in-Programming` |
| `write/tests/p9_fonts.rs:189, :207` | `docx-jinja2-demo`, `Contoso_Guest_WiFi…` |
| `write/tests/p7_p8.rs:226` | `Contoso_Guest_WiFi…` |
| `write/tests/p10_media.rs:63, :85, :97` | `1. First-Steps…`, `CC0_DOCX/014_…`, `CC0_DOCX/068_…` |

Тесты P7–P10 и `softuni_position_v` есть только в локальном `master` (он на 9
коммитов впереди `origin/master`), поэтому CI их ещё не видел. **Первый же push
сделает CI красным на всех трёх ОС.**

Отдельно: `wml/tests/corpus.rs:77` читает `strict-ooxml-wml/tests/docx`. Такого
каталога нет, поэтому тест молча ничего не проверяет.

## 4. Целевая архитектура

### 4.1. Lock-файл

`testdata-lock/cc0.toml` (коммитится; каталог `testdata/` остаётся в
`.gitignore`):

```toml
schema = 1

[[doc]]
id = "cc0-docx-1/076"                       # стабильный ключ для тестов
path = "CC0_DOCX_1/076_Humanidades_Mexico.docx"
sha256 = "…"
bytes = 122_000
url = "https://archive.org/download/…"
license = "CC0-1.0"
source_manifest = "CC0_DOCX_1/manifest.json"
tiers = ["ci-core", "ci-full"]
features = ["wpg_group", "pctPosV", "tbl", …]   # из скана, для отбора
```

- Генерируется командой `xtool corpus lock` из трёх `manifest.json` и
  скана возможностей; ручная правка — только поля `tiers`.
- Тесты ссылаются на `id`, а не на имя файла: переименование файла не ломает
  тесты, замена байтов ломает хеш.

### 4.2. Получение

`xtool corpus fetch --tier ci-core|ci-full [--root <dir>]`:

- скачивает с `url`, проверяет `sha256` и размер, пишет атомарно (tmp + rename);
- уже лежащий файл с верным хешем не трогает (локально корпуса уже есть —
  ничего не скачивается);
- повторы с backoff; итоговый отчёт: сколько скачано, пропущено, ошибок;
- не переходит по редиректам за пределы `archive.org` и `*.us.archive.org`.

Риск недоступности archive.org (§9) закрывается зеркалом: tar-архив уровня
`ci-core` (10,9 MB) как asset приватного релиза или кеш CI; `fetch` пробует
зеркало, затем `url`.

### 4.3. Доступ из тестов: `strict_ooxml_testkit::corpus`

```rust
pub enum Mode { Skip, Require }            // STRICT_OOXML_CORPUS=skip|require
pub fn root() -> PathBuf;                  // STRICT_OOXML_CORPUS_ROOT, иначе <workspace>/testdata
pub fn doc(id: &str) -> Option<CorpusDoc>; // путь + проверка sha256 (кешированная)
pub fn tier(t: Tier) -> Vec<CorpusDoc>;    // для тестов класса A
#[macro_export] macro_rules! corpus_doc { … } // Skip: eprintln + return; Require: panic с подсказкой `xtool corpus fetch`
```

- По умолчанию (локально, без переменных) — `Skip`: тест печатает
  `SKIP corpus doc cc0-docx-1/076 not fetched` и проходит.
- В CI `STRICT_OOXML_CORPUS=require`: отсутствие документа — ошибка, а не
  тихий зелёный.
- Несовпадение sha256 — всегда ошибка, в обоих режимах.
- Python-гейты (`census_gate.py`, `wps_ledger.py`) читают тот же lock-файл
  через маленький модуль `xtool/corpus_lock.py`.

### 4.4. CI

```yaml
- uses: actions/cache@v4
  with:
    path: testdata
    key: corpus-${{ hashFiles('testdata-lock/cc0.toml') }}-ci-core
- run: cargo run -p xtool --release -- corpus fetch --tier ci-core
- run: cargo test --workspace --all-features
  env: { STRICT_OOXML_CORPUS: require }
```

Ночной job: `--tier ci-full`, census-гейт, `normalize_roundtrip` и
`opc_oracle` по всем 300 документам.

## 5. Уровень `ci-core`

Отбор жадным покрытием множеств: каждая конструкция из §2.2 и каждая группа
письменностей (RTL, CJK, индийские, кириллица) — минимум в двух документах (или
во всех, если их меньше двух), с предпочтением малых файлов. Плюс два документа,
которые уже закреплены в `p10_media`. Итог: **27 документов, 10,9 MB.**

| Документ | KB | Зачем (основное) |
|---|---:|---|
| `CC0_DOCX_1/076_Humanidades_Mexico.docx` | 122 | единственный с `pctPosVOffset`; `wpg:wgp` |
| `CC0_DOCX/014_BG_Slokas_With_Transliteration_1_18.docx` | 537 | уже в `p10_media` |
| `CC0_DOCX/068_Madhurya_Kadambini_Roman_Sanskrit.docx` | 4156 | уже в `p10_media` (пустой `font6.odttf`) |
| `CC0_DOCX_1/066_qD_h_lqHTnyh.docx` | 28 | `tblHeader`, `bwMode`, AlternateContent, RTL |
| `CC0/073_20250706_20250706_1322_docx.docx` | 12 | `themeColor`, `themeFontLang eastAsia` |
| `CC0_DOCX/005_De_Thi_Van_Dgnl.docx` | 50 | AlternateContent |
| `CC0_DOCX/020_Nghien_cuu_cac_roi_loan_giao_tiep_cua_tre_tu_ki_ng.docx` | 109 | правки `ins`/`del`, диаграмма |
| `CC0_DOCX/028_nbhvv.docx` | 14 | `m:oMath` |
| `CC0/099_2011Aug23_EffectiveCommunicationSkil_ForUploadingToArchive.docx` | 36 | `bwMode`, PNG, WMF |
| `CC0/041_1_20260227_20260227_1453_1_.docx` | 19 | комментарии, кириллица |
| `CC0/025_afewquotationsfromvoltaire…Christ.docx` | 23 | концевые сноски |
| `CC0_DOCX/013_woord.docx` | 54 | OLE, WMF |
| `CC0/023_20251209_20251209_0552_25._11._24_.docx` | 37 | `themeFill`, CJK |
| `CC0/081_aisha-grimorio-1_nota_marco_epistemico.docx` | 10 | комментарии, character styles |
| `CC0/035_12-v-0.72_25-202120210509.docx` | 13 | `customXml`, CJK |
| `CC0_DOCX/090_Gritos_Chutes_E_Pontapes.docx` | 257 | концевые сноски |
| `CC0_DOCX_1/010_Electric_HypersonicAircraft.docx` | 279 | OLE, формулы |
| `CC0/075_20251012_20251012_0816_docx.docx` | 31 | кириллица |
| `CC0_DOCX_1/065_Teoria_de_la_comunicacion.docx` | 53 | комментарии |
| `CC0_DOCX/058_Apostila_Completa_C_E_Estruturas_De_Dados.docx` | 58 | `tblHeader` |
| `CC0/067_20211220_20211220_1030_------.docx` | 246 | единственный SmartArt |
| `CC0_DOCX/067_N_3_ACT_4_Ha_Tuan_Kiet.docx` | 85 | glossary |
| `CC0/020_20-de-thi-cuoi-hoc-ki-1-lop-5…lop_5.docx` | 134 | `wpg:wgp` (бывший AUD-103) |
| `CC0_DOCX_1/024_Cagdas_INAL_Kimdir_Ve_Stratejik_Arastirma_Vizyonu.docx` | 381 | встроенные шрифты |
| `CC0_DOCX/077_2016_Greater_Launceston_Metropolitan_Passenger_Tra.docx` | 3199 | вторая диаграмма |
| `CC0_DOCX/085_Fu_Ben_2Yue_2Ri_Xiu_Gai_Zhong_Yang_She_Luvme_HairG.docx` | 903 | правки, CJK |

Список предварительный: он пересобирается `xtool corpus lock --select ci-core`
после фазы 1 и после ручной проверки лицензий (§2.3). Скан возможностей
сделан регулярными выражениями по `document.xml`/`styles.xml`/`settings.xml`/
`numbering.xml` и именам частей; в фазе 1 его заменяет классификатор
`xtool corpus-elements`.

## 6. Замены для закреплённых документов (класс B/C)

Правило: кандидат принимается, только если нужная конструкция найдена в его
XML независимым сканом (zip + XML, без `strict-ooxml-*`). Закрепляемые значения
переснимаются из той же записи. Если кандидат не держит конструкцию, тест
переходит на синтетический fixture (`strict-ooxml-testkit`), а не на «похожий»
документ.

| Сейчас | Потребители | Что проверяется | Кандидат CC0 | Что переснять |
|---|---|---|---|---|
| `Manual.docx` | `cli.rs:407`, `parallel_report.rs:17`, `normalization_report.rs:17`, `report/normalization.rs:21` | `normalize` даёт `T2.reltype ×8`; parallel == sequential; детерминизм | любой Transitional с ≥5 различными Transitional-reltype; выбрать по скану `_rels` (из `ci-core`, малый) | число `T2.reltype`; ожидаемый вывод CLI |
| `Contoso_Guest_WiFi…` | `p7_p8.rs:226`, `p9_fonts.rs:207` | `themeColor` (1277) / `themeFill` (784) в стилях; `themeFontLang eastAsia` | `CC0/023_…` (themeFill + CJK), `CC0/073_…` (themeColor + eastAsia) | счётчики атрибутов по `styles.xml`; значение `eastAsia` |
| `1. First-Steps-in-Programming` | `p11_graphics.rs:27`, `p10_media.rs:63` | `cNvPr id`, `bwMode="auto"`, `blip cstate="print"`; дайджест `image1.png` | `CC0/099_…` (bwMode + PNG); `CC0/056_…` или `CC0_DOCX/100_…` (cstate) | конкретный `id`; имя и дайджест медиа-части |
| `docx-jinja2-demo` | `p9_fonts.rs:189` | шрифты маркеров нумерации Symbol/OpenSymbol | `CC0/057_04-valeur-temps-modes_TASK_1.docx` (9 KB) | список `rFonts` уровней по `numbering.xml` |
| `4. Complex-Conditions` | `softuni_position_v.rs:17` | группа сохраняет `pct` или `off` позиции | `CC0_DOCX_1/076_Humanidades_Mexico.docx` — **единственный** с `pctPosVOffset` | id группы, значение `pct`/`off` |
| `RM0090 16-23…` | `p6_table_page.rs:17`, `p5_p6.rs:196, :261` | таблица на ≥2 страницах; геометрия заголовка (32 px, ширина, зазор ±0,25); ширины колонок 1917/15, 2579/15 twips | `CC0_DOCX_1/066_…`, `CC0_DOCX/058_…` (есть `tblHeader`); проверить, что таблица действительно переходит страницу | номер страницы; ширины колонок из `w:tblGrid`; геометрия — только после эталона (фаза 5) |
| `Clio Der Sarkissian…2011` | `f06_toggles.rs:295`, `wps_ledger.py`, `wps_baselines.py` | character style даёт 4 pt; геометрия глифов на стр. 54/56/104 ±0,25 px | подобрать: длинный документ с мелкими character-style runs и VML/WPS-фигурами (290 документов с character styles); решение — в фазе 5 | стиль и размер из `styles.xml`; новые эталоны WPS |
| `CC0_DOCX/014_…`, `CC0_DOCX/068_…` | `p10_media.rs:85, :97` | уже CC0 | — | только перевести на `corpus_doc!` |

## 7. Фазы

Каждая фаза — отдельная ветка и отдельная приёмка. Порядок жёсткий для фаз 0–2;
фазы 4 и 5 могут идти параллельно.

### Фаза 0. Стоп-кран (до любого push)

- 11 тестов из §3.2: заменить `.expect()` на явный skip, если файла нет
  (временный локальный хелпер; в фазе 1 его заменит `corpus_doc!`).
- `wml/tests/corpus.rs:77`: исправить путь или удалить мёртвую ветку.
- **Приёмка:** на чистом клоне без `tests/docx/` и `testdata/`
  `cargo test --workspace --all-features` зелёный, а в выводе видны `SKIP`
  ровно по этим 11 тестам. На машине с корпусом все 11 по-прежнему выполняются
  и проходят.

### Фаза 1. Слой корпуса

- `xtool corpus lock` — нормализует три манифеста в `testdata-lock/cc0.toml`,
  добавляет `features` из скана; `xtool corpus fetch`; `xtool corpus verify`.
- `strict_ooxml_testkit::corpus` (§4.3) с unit-тестами: skip, require,
  несовпадение хеша, отсутствие lock-файла.
- `xtool/corpus_lock.py` для python-гейтов.
- Ручная проверка лицензий кандидатов в `ci-core`.
- **Приёмка:** `fetch --tier ci-core` на пустом каталоге скачивает 26 файлов с
  совпадающими хешами; повторный запуск ничего не скачивает; подменённый байт
  в файле даёт ошибку и в `verify`, и в тесте.

### Фаза 2. CI

- Шаги из §4.4 в job `test` (три ОС) с кешем.
- `STRICT_OOXML_CORPUS=require` в CI.
- Ночной job `corpus-full`.
- **Приёмка:** зелёный CI на трёх ОС; намеренно удалённый из кеша документ
  роняет job с понятным сообщением, а не даёт тихий зелёный.

### Фаза 3. Тесты класса A и census

- Перевести 14 потребителей класса A на `corpus::tier(...)`:
  `docx_corpus`, `limits_audit`, `normalize_roundtrip`, `opc_oracle`,
  `render-svg/corpus`, `report/corpus`, `wml/corpus`, `fuzz/seed_corpus.sh`,
  `strict-ooxml-view` (`KNOWN_CORPORA`), `xtool --corpus`, `corpus_scan`.
- `limits_audit`: на 300 документах максимум может превысить лимиты по
  умолчанию. Это находка, а не повод поднять лимит молча: оформить как задачу.
- `census_gate.py`: `CORPORA` из lock-файла; новая базовая линия по `ci-full`.
  Сейчас D05 = `PARTIAL_CENSUS_FAIL` (10 486 строк без решения), поэтому гейт
  в CI сначала работает как **храповик**: новые `unaccounted` и рост счётчиков
  `TZ-*` относительно записанной базы — ошибка; уменьшение — обновление базы.
  Требование «ноль» вернуть, когда D05 закроется.
- Обновить прозу `census.toml` («58 Transitional documents»).
- Закрыть waiver `CENSUS-LOCAL` ссылкой на этот документ.
- **Приёмка:** census в ночном CI; `CENSUS-LOCAL` закрыт; ни один тест класса A
  не читает `tests/docx/`.

### Фаза 4. Закреплённые тесты (класс B/C, кроме геометрии)

По одному тесту на коммит, в порядке §6: `Manual` → `Contoso` → `First-Steps` →
`jinja2-demo` → `Complex-Conditions` → `p10_media`.

Для каждого:

1. Независимым сканом подтвердить конструкцию в кандидате; записать
   свидетельство (часть, XPath-подобный путь, значение) в комментарий теста.
2. Переснять ожидаемые значения из записи, а не из вывода нашего кода.
3. Перевести тест на `corpus_doc!("cc0-…")`.
4. Проверить, что тест **падает** на намеренно сломанном коде (мутация того
   места, которое он охраняет), иначе он ничего не проверяет.

- **Приёмка:** grep по `tests/docx` в Rust-тестах находит только
  `p5_p6`, `p6_table_page`, `f06_toggles`; каждый переведённый тест проходит
  проверку мутацией из п. 4.

### Фаза 5. Геометрия и эталоны WPS

- Выбрать замену `RM0090` (таблица через страницу) и `Clio` (мелкие character
  styles, фигуры) из `ci-full` по критериям §6.
- Снять эталоны тем же инструментом и версией, что и сейчас: WPS Office
  12.1.0.28485, `ExportAsFixedFormat` / `kwpsconvert` (через hostq на
  Windows-хосте). Записать манифест эталонов с хешем исходного документа.
- Перенести `wps_ledger.py` на новый документ; переснять страницы и пороги.
- `p5_p6` и `f06_toggles`: новые значения — только из эталона и записей
  документа.
- Эталоны (PDF/JSON) коммитятся, если лицензия документа это позволяет (CC0 —
  да); сам документ — нет.
- **Приёмка:** WPS-гейт проходит на новом документе с теми же допусками
  (0,25 px, Y 1,25) или с явно записанным и обоснованным новым допуском; старый
  Clio-гейт помечен как исторический.

### Фаза 6. Вывод закрытого корпуса

- Ни один код (Rust, python, CI) не ссылается на `strict-ooxml-core/tests/docx`.
- Архивные `docs/audit-*/reproduce.py` не переписываются (это доказательства
  прошлых приёмок): в начало каждого добавить пометку «требует закрытого
  корпуса, только историческое воспроизведение».
- `.gitignore`: `tests/docx/` и `/testdata/` остаются игнорируемыми (у
  владельца корпус остаётся); `testdata-lock/` — отдельный каталог и
  коммитится без исключений.
- **Приёмка:** `grep -r "tests/docx"` вне `docs/audit-*` пуст; полный CI
  зелёный на чистом клоне.

### Фаза 7. Пробелы корпуса

Синтетические fixtures в `strict-ooxml-testkit` (генерируются кодом, коммитятся
как генератор, а не как байты), для того, чего в CC0 мало:

- правки `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo`, включая вложенные (это же
  закрывает регрессионный тест на переполнение стека вложенными inline-обёртками);
- диаграммы и SmartArt (проверка pass-through и loss report);
- концевые сноски, `tblHeader` на нескольких страницах;

Strict: корпус CC0 его не даёт; основой остаётся синтетический `tests/strict/`
(с `PROVENANCE.md`) и waiver `A-1`. Возможный источник «настоящих» Strict —
пересохранение документов CC0 в Word через «Сохранить как → Strict Open XML
Document» (если Word есть на хосте; через hostq). Это отдельная
задача.

## 8. Вне объёма

- `tests/strict/` — синтетический Strict-корпус с `PROVENANCE.md`, коммитится,
  используется как есть. Эталоны SSIM и PDF-пикселей сняты с него; SSIM-гейт
  (`render-svg/tests/ssim.rs`) и `pdf_pixels` от перехода **не зависят**.
- `tests/samples/` (22 файла с сайтов примеров, коммитятся) — используется как
  есть, вместе с CC0. Тесты, закреплённые на нём (`write/tests/fonts.rs`,
  `p9_fonts.rs:228`, `samples_corpus.rs`, `corpus_oracle.rs` и др.), не
  переносятся. Census-гейт меряет `samples` + CC0.
- `testdata/CC0_RTF`, `CC0_PDF`, `CC0_DJVU` — по своим планам
  (`RTF_IMPORT_PLAN.md`); формат lock-файла позволяет их добавить.

## 9. Риски

| Риск | Мера |
|---|---|
| archive.org недоступен или удалил документ | зеркало `ci-core` (кеш CI + архив-asset); выпадение документа → замена по `features` из lock-файла |
| Документ потерял CC0 или поступила претензия | удаление строки из lock-файла; тесты ссылаются на `id`, замена — по §6 |
| На 300 документах всплывут новые падения парсера/writer | ожидаемо и полезно; оформлять как задачи, не отключать тесты; в первом прогоне `ci-full` — режим отчёта, не гейта |
| Переснятые значения закрепят текущую ошибку | значения только из записей документа и независимого скана; мутационная проверка (фаза 4, п. 4) |
| Census на новом корпусе даст другие числа, D05 «откатится» | храповик от новой базы, а не сравнение со старой; старые числа остаются в истории аудита |
| Время CI | `ci-core` 10,9 MB с кешем; `ci-full` — только ночью |
| Ненадёжное поле `language` | отбор по скану письменности, не по полю |

## 10. Оценка

| Фаза | Объём |
|---|---|
| 0 | 0,5 дня |
| 1 | 1,5–2 дня |
| 2 | 0,5 дня |
| 3 | 1,5 дня + разбор находок `ci-full` |
| 4 | 1,5–2 дня (6 групп тестов) |
| 5 | 2–3 дня, из них снятие эталонов WPS — ручная работа на хосте |
| 6 | 0,5 дня |
| 7 | 2 дня |

Итого ~10–12 рабочих дней. Фазы 0–3 (≈4 дня) уже дают зелёный CI на чистом
клоне и census в CI.

## 11. Готовность

- Чистый клон: `cargo test --workspace --all-features` зелёный без корпуса
  (skip) и зелёный с `ci-core` в режиме `require`.
- CI на трёх ОС скачивает `ci-core` по lock-файлу и проверяет хеши.
- Ночной CI гоняет `ci-full` и census.
- Ни один код вне `docs/audit-*` не ссылается на `strict-ooxml-core/tests/docx`.
- Waiver `CENSUS-LOCAL` закрыт.
- Каждый закреплённый тест имеет записанное свидетельство из XML документа и
  проходит мутационную проверку.

## 12. Выполнено (ветка `task/hardening-2026-10-07`)

- **Фаза 0.** Все тесты, читавшие `tests/docx/` через `.expect()`, пропускаются
  без корпуса (`SKIP …`); `wml/tests/corpus.rs` читает существующий каталог.
- **Фаза 1.** `testdata-lock/cc0.toml` (300 документов, 26 в `ci-core`),
  генератор `xtool/corpus/make_lock.py`; `xtool corpus fetch|verify`
  (`xtool/src/corpus.rs`: SHA-256 и размер, атомарная запись, только
  `https://archive.org`, повторы); `strict_ooxml_testkit::corpus` (`tier`,
  `doc`, `corpus_doc!`, режимы `STRICT_OOXML_CORPUS=skip|require`,
  `STRICT_OOXML_CORPUS_ROOT`).
- **Фаза 2.** CI `test`: кэш + `corpus fetch --tier ci-core` +
  `STRICT_OOXML_CORPUS=require`; ночной job `corpus-full` (`ci-full`, корпусные
  тесты и `corpus_scan` по трём наборам).
- **Фаза 3 (частично).** `core/tests/docx_corpus.rs` и `wml/tests/corpus.rs`
  гоняют `ci-core`. Census и остальные потребители класса A — впереди.
