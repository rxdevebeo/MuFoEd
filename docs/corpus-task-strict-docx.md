# Задание: набрать корпус подлинных Strict-документов

**Дата:** 2026-10-01 · **Заказ:** `STAGE-10-TASK.md` фаза 10G, §16 п. 9
**Выход:** `strict-ooxml-core/tests/strict/` рос с 11 документов до 27 (16 новых
сторонних из 3 репозиториев), и каждая строка таблицы происхождения **проверяема**

---

## 0. Почему это сложно, и в чём именно

1. **Strict-файлов почти нет.** Word по умолчанию пишет Transitional
   (`schemas.openxmlformats.org/...`). Strict (`purl.oclc.org/...`) — скрытый
   режим, **не выставленный в UI**. Проверено: во всех найденных Strict-файлах
   `docProps/app.xml` содержит `<Application>Microsoft Office Word</Application>`,
   и сама эта часть лежит в **Strict**-неймспейсе extended-properties. То есть
   **все** Strict-файлы на свете сделаны Word.
2. **Независимого генератора не существует.** Проверено по каждому кандидату:

   | Инструмент | Strict на выходе | Чем проверено |
   |---|---|---|
   | **WPS 12.1.0.28485** (наш оракул) | **нет** | COM-проба на 5 значениях `FileFormat`; `wdFormatStrictOpenXMLDocument` (24) **тихо выдаёт legacy `.doc`** (OLE-сигнатура `D0 CF 11 E0`). Вдобавок **понижает Strict → Transitional при открытии** |
   | **LibreOffice** | нет | один Writer-фильтр `"Office Open XML Text"`; `namespaces-strict.txt` подключён **только** импортёром; в `docxexport.cxx` (97 587 байт) ноль вхождений `strict` |
   | **pandoc** | нет | `stdAttributes` жёстко задаёт Transitional; единственное упоминание `purl.oclc.org` — предикат для чтения |
   | **Apache POI** | нет | ноль `conform`/`strict` в `XWPFSettings.java`; одно упоминание `purl.oclc.org` в константах типов отношений |
   | **docx4j** | нет | единственный инструмент, который **читает** Strict (`mc-preprocessor.xslt`), сериализатора Strict нет |
   | **Open-Xml-PowerTools** | нет | `DocumentBuilder.cs:1404`: `throw … "is saved in strict mode, not supported"` |
   | **Open XML SDK** (dotnet) | нет | таблица `TryGetStrict*` **отсутствует** (есть только `TryGetTransitional*`); `StrictRelationshipFound` — read-only bool |

   Итог: генерировать нечем, **собирать можно только готовое**.
3. **«Третьесторонний производитель» — понятие shaky.** Раз все файлы сделаны
   Word, вопрос звучит не «чей это документ», а «**в чьём MIT/Apache-репозитории
   он лежит**». Лицензия берётся у репозитория, а не из метаданных файла.
4. **Копирование корпусов — ловушка.** GitHub code search по `Strict01.docx` и
   `O14ISOStrict` находит ~20 репозиториев, которые перезалили одни и те же
   файлы через посредника без лицензии. Идти надо в **первоисточник**, а не в
   копию.

---

## 1. Уже найденные источники (всё проверено скачиванием, не пересказом)

| Источник | Лицензия | Strict `.docx` | Проверено мной |
|---|---|---:|---|
| **`dotnet/Open-XML-SDK`** | **MIT** (`.NET Foundation and Contributors`) | **62** | дерево репозитория обойдено, каждый `.docx` скачан и распакован, `xmlns:w` проверен |
| **`Plutext/docx4j`** | **Apache-2.0** (`legals/LICENSE`) | **4** + 1 `.xlsx` | файлы скачаны, хэши сходятся |
| **`LibreOffice/core`** | MPL-2.0 (код) | **6** | скачаны, все `Application: Microsoft Office Word` |
| `apache/poi` | Apache-2.0 | **0** | все 130 `.docx` скачаны и проверены |
| `python-openxml/python-docx` | MIT | 0 | 4 файла проверены |
| `apache/tika` | Apache-2.0 | 0 | 61 файл проверен |
| `jgm/pandoc` | GPL-2.0 | 0 | 125 проверено |
| `OpenXmlDev/Open-Xml-PowerTools` | MIT | 0 | обойдено целиком |
| `OfficeDev/office-content` | **CC-BY-NC-ND** | 0 | ❌ **исключить навсегда** |

### 1.1 Точные адреса

**`dotnet/Open-XML-SDK`**, MIT:

```
test/DocumentFormat.OpenXml.Tests.Assets/assets/TestFiles/Strict01.docx          831 961
test/DocumentFormat.OpenXml.Tests.Assets/assets/TestFiles/AnnotationRef.docx      15 649
test/DocumentFormat.OpenXml.Tests.Assets/assets/TestDataStorage/O14ISOStrict/Word/*.docx   (55 файлов)
test/DocumentFormat.OpenXml.Tests.Assets/assets/TestDataStorage/O14ISOStrict/Graphics/*.docx (3 файла)
```

⚠️ **`Strict01.docx` — самый ценный файл в наборе**: 1832 слова, SmartArt +
диаграммы + комментарии + customXml + EMF + встроенный бинарник 752 КБ, Strict
во всех четырёх WML-частях **включая `numbering.xml`**.

⚠️ **Провенанс `O14ISOStrict/` требует оговорки.** Папка приехала единым
коммитом `c411da602` от 2018-03-02 (Taylor Southwick, *«Move test assets to own
aseembly (#415)»*), без атрибуции, без `licence_files/`, без README; внутренние
метаданные: `dc:creator = Terminal Server Users`,
`cp:lastModifiedBy = Office Automation Limited Client`, `AppVersion 14.0000` —
то есть **внутренняя тестовая оснастка Microsoft**, попавшая в MIT-репозиторий.
Содержимое — 2–33 слова автозаполнителя, стороннего контента нет. Риск низкий,
остаточный — ненулевой, и он **объявляется**, а не закрывается.

**`Plutext/docx4j`**, Apache-2.0, ветка `VERSION_17_2_1`:

```
docx4j-core-tests/src/test/resources/strict/strict-sample-docx.docx   143 069
docx4j-core-tests/src/test/resources/strict/strict-smartart.docx       22 416
docx4j-core-tests/src/test/resources/strict/strict-math.docx            13 754
docx4j-core-tests/src/test/resources/strict/strict-chart.docx           36 691
```

✅ **Самая чистая Provenance во всём наборе**: автор Jason Harrop (Plutext Pty
Ltd), `dc:creator = cp:lastModifiedBy = Jason Harrop`, 2–33 слова, никакого
стороннего содержимого.

**`LibreOffice/core`**, MPL-2.0, ветка `master`:

```
sw/qa/extras/ooxmlexport/data/strict.docx                          25 636  Miklos Vajna, 2014
sw/qa/extras/ooxmlexport/data/strict-smartart.docx                 18 661  Miklos Vajna, 2014
sw/qa/extras/ooxmlexport/data/tdf116410.docx                       49 192  Regina Henschel, 2018
sw/qa/extras/ooxmlexport/data/tdf150822.docx                       14 767  Regina Henschel, 2022
sw/qa/extras/ooxmlexport/data/tdf79272_strictDxa.docx               12 279  Luke Benes, 2014
sw/qa/extras/ooxmlexport/data/tdf82065_Ind_start_strict.docx        12 533  user, 2017
```

✅ **Настоящие баг-репорты**, то есть реальные документы. Как враждебный
материал лучше синтетики `O14ISOStrict`. ⚠️ MPL-2.0 **нет** в allow-list
`deny.toml` — нужно решение владельца (см. §4).

---

## 2. Что брать — список утверждён 2026-10-01

Цель: **≥ 16 документов** в `tests/strict/`, из них **≥ 5 сторонних** и **≥ 3
из разных репозиториев**.

| # | Файл (новое имя в корпусе) | Источник | Размер | Что проверяет |
|---:|---|---|---:|---|
| 1 | `strict01-sdk.docx` | `dotnet/Open-XML-SDK` @ `main`, `test/…/assets/TestFiles/Strict01.docx` | 832 КБ | **самый богатый документ в наборе**: SmartArt, диаграммы, комментарии, customXml, EMF, встроенный бинарник, 1832 слова. Strict во всех четырёх WML-частях **включая `numbering.xml`** |
| 2 | `strict-sample-docx.docx` | `Plutext/docx4j` @ `VERSION_17_2_1` | 143 КБ | чистая provenance, Apache-2.0; большой смешанный документ |
| 3 | `strict-math.docx` | там же | 13.7 КБ | **OMML** — а у нас там как раз 17 нарушений (`XS-12…XS-15`) |
| 4 | `strict-smartart.docx` | там же | 22 КБ | SmartArt → pass-through |
| 5 | `annotation-ref-sdk.docx` | `dotnet/Open-XML-SDK`, `test/…/TestFiles/AnnotationRef.docx` | 15.6 КБ | аннотации, ссылки на аннотации |
| 6 | `sdk-ftr-bookmark.docx` | `O14ISOStrict/Word/ftr - Footer - bookmarkStart - Bookmark Start.docx` | 19.6 КБ | **колонтитулы + закладки**: `HeaderFooterRef.part` (I4), `w:tr/@w14:paraId` (Q-E6) |
| 7 | `sdk-instrtext-eq-array.docx` | `O14ISOStrict/Word/instrText-Field Definitions-EQ-Array-AlignLeft.docx` | 13.9 КБ | поля, `EQ`-массив, `w:instrText` |
| 8 | `sdk-customxml-special-chars.docx` | `O14ISOStrict/Word/customXmlDelRangeStart-Custom XML Markup Deletion Start-val-author-special-chars.docx` | 14.6 КБ | **customXml + спецсимволы в `w:author`** — проверка на escape и на `Opaque*` |
| 9 | `sdk-text-direction-tbrl.docx` | `O14ISOStrict/Word/textDirection-Paragraph Text Flow Direction-val-tbRl.docx` | 14.5 КБ | вертикальный текст, `w:textDirection` |
| 10 | `sdk-tbllayout.docx` | `O14ISOStrict/Word/tblLayout-Table Layout.docx` | 14.1 КБ | **таблицы** → `XS-08`/`XS-09`, `I8` |
| 11 | `sdk-sectpr-rsid.docx` | `O14ISOStrict/Word/sectPr-Previous Section Properties-rsidSelect-004B4C75.docx` | 14.2 Кб | **`sectPr` и `rsid`** → `I5` (соответствие `sections`), `ST_LongHexNumber` |
| 12 | `sdk-numfmt-decimalfullstop.docx` | `O14ISOStrict/Word/numFmt-Numbering Format-val-decimalEnclosedFullstop.docx` | 15.0 КБ | **нумерация** → `I7`, `Ilvl`, `w:numFmt` |
| 13 | `sdk-shd-tbl-pct5.docx` | `O14ISOStrict/Word/shd-Table Shading-val-pct5.docx.docx` | 14.5 КБ | `w:shd` с процентами → тип значения (`ST_Percentage`, ср. `XS-11`) |
| 14 | `lo-tdf116410.docx` | `LibreOffice/core` @ `master`, `sw/qa/extras/ooxmlexport/data/tdf116410.docx` | 49 КБ | **настоящий баг-репорт**, MPL-2.0 |
| 15 | `lo-strict.docx` | там же, `sw/qa/extras/ooxmlexport/data/strict.docx` | 25.6 КБ | настоящий документ, 2014 |
| 16 | `lo-tdf79272-strictdxa.docx` | там же, `sw/qa/extras/ooxmlexport/data/tdf79272_strictDxa.docx` | 12.3 КБ | строгие `dxa`-величины |

**Итого 16, из них 15 сторонних, из 3 репозиториев** — при минимуме 16/5/3 это
с запасом.

**Правило отбора — уже применено** к списку выше: каждый файл двигает хотя бы
одну из наших проверок.

| Что файл проверяет | Наша дыра |
|---|---|
| `w:tr/@w14:paraId`, закладки, колонтитулы | `TableRow` не несёт `para_id` (`Q-E6`), `I4` |
| `w:tbl`, `w:tblLayout` | `XS-08`/`XS-09`, `I8` |
| `m:` элементы OMML | 17 нарушений, `XS-12…XS-15` |
| `c:` части диаграммы, `dgm:` SmartArt | `XS-16` — pass-through |
| `a:` DrawingML, `wp14:` | `XS-19` — расширения |
| Тема с непустым `a:fmtScheme` | `XS-02`/`XS-03` — у нас тема **пустая**, чужой файл покажет, что это неправда |
| `w:sectPr` с `rsid` | `I5`, `ST_LongHexNumber` |
| `w:numFmt`, `w:shd` со значениями-процентами | `I7`, `ST_Percentage` |
| `w:author` со спецсимволами | экранирование, `Opaque*` |

> **Побочная ценность выше проверок.** `document.xml` наших фикстур построены
> **нами**, поэтому наш порядок элементов в них может совпадать с нашим же
> неправильным порядком — гейт тогда подтверждает сам себя. Разметка от Word —
> единственный способ это разорвать, и `Strict01.docx` здесь незаменим.

---

## 3. Как это сделать

### Шаг 1. Проверка подлинности Strict (обязательна для каждого файла)

```python
import zipfile, sys
from lxml import etree
W = "http://purl.oclc.org/ooxml/wordprocessingml/main"
z = zipfile.ZipFile(path)
parts = ["word/document.xml", "word/styles.xml", "word/numbering.xml", "word/settings.xml"]
for p in parts:
    if p not in z.namelist():
        continue
    ns = etree.QName(etree.fromstring(z.read(p)).getroottree().getroot()).namespace
    assert ns == W, f"{path}::{p} -> {ns}, NOT Strict"
```

**Правило:** файл берётся, только если `document.xml` **и** `styles.xml` в
Strict-namespace. Одного достаточно для попадания в корпус, но в таблицу
происхождения пишется, какие части проверены.

### Шаг 2. Проверка лицензии (обязательна)

1. `https://api.github.com/repos/{owner}/{repo}` → `license.spdx_id`.
2. Если `null`/`NOASSERTION` — **читать лицензию вручную**:
   `raw.githubusercontent.com/{owner}/{repo}/{branch}/LICENSE`
   (docx4j держит её в `legals/LICENSE`, у POI — в `doap_POI.rdf`; **`null`
   в spdx_id не значит «нет лицензии»**).
3. Подтвердить текст: MIT или Apache-2.0, с пригодной для редистрибуции нормой.
4. Отклонять без обсуждения: **CC-BY-NC-ND**, CC-BY-NC, «free for
   non-commercial use», ShareAlike, лицензии без нормы редистрибуции.

### Шаг 3. Проверка содержимого на отсутствие чужого материала

Прочитать `docProps/core.xml` → `dc:creator`, `cp:lastModifiedBy`,
`dcterms:created`; распаковать и посмотреть `word/media/*`. Если внутри есть
**чужой** текст, скан, фото или сторонняя графика — файл не берётся без
отдельного разрешения. Для всех кандидатов выше медиаданных нет, кроме
`Strict01.docx` (EMF и 752 КБ binary — **сгенерировано самим Word**, не
стороннее).

### Шаг 4. Прогнать гейт до принятия решения

Файл **не принимается в корпус**, пока не показано, что он проходит
`write` без новых нарушений схемы и что его выход не хуже входа. Файл, который
даёт новые `XS-nn`, берётся **только вместе с записью в реестр**.

### Шаг 5. Запись в таблицу происхождения

Строка в `strict-ooxml-core/tests/strict/README.md` обязана содержать:

| Поле | Пример |
|---|---|
| Файл | `strict01-sdk.docx` |
| Точный путь | `test/…/assets/TestFiles/Strict01.docx` |
| Репозиторий + **ветка или SHA** | `dotnet/Open-XML-SDK` @ `main` |
| **SHA-256 файла** | полный, не префикс |
| Лицензия + где лежит её текст | `MIT (LICENSE в корне)` |
| Провенанс файла | `dc:creator = Eric White, 2015-11-18, AppVersion 15.0000` |
| Что файл проверяет | «закладки + `numbering.xml` в Strict» |
| Проверено | дата, кем |

**Без SHA-256 строка неполна.** Именно его отсутствие позволило годами
переносить один и тот же файл между репозиториями под разными именами —
и привело к ошибке атрибуции, исправленной 2026-10-01 (§4.3).

---

## 4. Решения владельца — приняты 2026-10-01

### 4.1 MPL-2.0 — ДА, добавлена в allow-list

`deny.toml` теперь содержит `MPL-2.0` **с обоснованием в самом файле**, и
обоснование говорит правду про то, что этот гейт **не полицей**:

> `cargo deny` не может видеть файлы `.docx` — они не Cargo-зависимости, сканер
> лицензий до них не доходит. Запись существует, чтобы политика в файле
> совпадала с политикой, которой корпус реально следует. Если MPL когда-нибудь
> появится в графе зависимостей, он должен провалить ревью — и `cargo deny`
> теперь хотя бы увидит его в allow-list и потребует осознанного удаления, а не
> случайности.

Обоснование MPL-2.0 §1.2: копиleft **файловый**, ограничен покрытым исходным
кодом; определение «Covered Software» **исключает выходные данные** и файлы без
MPL-заголовка. `.docx`-фикстура не является ни тем, ни другим.

### 4.2 `O14ISOStrict/` — ДА, берём

Восемь файлов из папки входят в утверждённый список (§2, п. 6–13). Оговорка
обязана быть записана **дословно** в `PROVENANCE.md`:

> Файлы `O14ISOStrict/` пришли в `dotnet/Open-XML-SDK` единым коммитом
> `c411da602` от 2018-03-02, без атрибуции, без `licence_files/`, без README.
> Внутренние метаданные: `dc:creator = Terminal Server Users`,
> `cp:lastModifiedBy = Office Automation Limited Client`,
> `dcterms:created = 2010-09-24`, `AppVersion 14.0000` — то есть **внутренняя
> тестовая оснастка Microsoft**, попавшая в MIT-репозиторий .NET Foundation.
> Стороннего содержимого нет: 2–33 слова автозаполнителя плюс сгенерированная
> самим Word разметка. Риск низкий, остаточный ненулевой, и он объявлен, а не
> закрыт.

### 4.3 Атрибуция уже исправлена

`strict-profile.docx` был приписан `kklimuk/docx-cli` (MIT). Файл **побайтово
идентичен** `strict-chart.docx` из `plutext/docx4j` (Apache-2.0) — SHA-256
совпадает целиком. Атрибуция исправлена 2026-10-01. Обе лицензии совместимы с
проектом, поэтому по usability ничего не меняется, но **имя и лицензия были
неверны**. Подробности — `docs/stage-10-xsd-audit.md` §7.

---

## 5. Порядок исполнения

| Шаг | Что | Выход |
|---:|---|---|
| **1** | Скачать 15 файлов §2 в `tests/strict/` под новыми именами | файлы на месте |
| **2** | Проверка Strict по скрипту §3.1 для каждого | ни одного отклонённого |
| **3** | Записать `PROVENANCE.md` + строки в `README.md` корпуса с **полным SHA-256** | таблица происхождения полна |
| **4** | Прогнать `write` на новых файлах, получить вход/выход | числа, а не «посмотрели» |
| **5** | **Каждый новый `XS-nn` → строка в `docs/stage-10-xsd-audit.md`** | реестр растёт, а не прячется |
| **6** | Прогнать гейт; пересчитать `XSD-CORPUS` в `docs/waivers.toml` | waiver либо закрыт, либо переформулирован |
| **7** | `cargo deny check licenses` | MPL-2.0 в allow-list, зелено |

**Оценка:** шаг 4 — самый интересный. Документы от Word построены **не нами**,
поэтому порядок элементов в них — эталонный, а не наш. Если наш writer даст на
них больше нарушений, чем на собственных фикстурах, — это и есть измерение,
ради которого всё затевалось.

---

## 6. Выход задания

| # | Критерий |
|---|---|
| **C1** | ≥ **16** документов в `tests/strict/`, ≥ **5** сторонних, ≥ **3** репозиториев |
| **C2** | У каждой строки таблицы происхождения: точный путь, репозиторий + ветка, **полный SHA-256**, лицензия + где её текст, провенанс, что проверяет |
| **C3** | Каждый файл проверен на Strict-namespace **по скрипту**, не на глаз |
| **C4** | Ни одного файла из списка исключений (`office-content`, ~20 копий `Strict01.docx`) |
| **C5** | `PROVENANCE.md` рядом с корпусом, с оговорками про `O14ISOStrict` |
| **C6** | Ни один принятый файл **не добавил** новых `XS-nn` без записи в реестр |
| **C7** | `docs/waivers.toml`: `XSD-CORPUS` пересмотрен — либо закрыт, либо сформулирован заново под фактический размер корпуса |

**Смысл C1 в одной строке:** сейчас «мы пишем Strict» проверяется гейтом на
документах, которые **написали мы сами**. Сторонний документ — единственное,
что делает этот гейт независимым.
