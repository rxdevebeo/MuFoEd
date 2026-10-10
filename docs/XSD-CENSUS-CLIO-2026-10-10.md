# XSD, census и Clio — 2026-10-10

Ветка `task/xsd-census-clio-2026-10-10` от master `a96a074`.

## 1. XSD-гейт (регрессия master)

376 нарушений `satMod`/`gs`/`fillToRect`/`shade`/`tint`/`alpha` появились после `edb4714`: `a:fmtScheme`
темы стал сохраняться как прочитан, а SDK-документы пишут проценты DrawingML тысячными по-Transitional
(`pos="35000"`) в Strict-частях, куда нормализатор не заходит. Исправление: сохраняемые фрагменты DrawingML
переводят голые тысячные в Strict-проценты (`strict_percent`, `parse/drawing.rs`). Итог: `unmatched=0`,
11 нарушений источника (как 2026-10-08).

## 2. Census: 72 → 0

Сохраняется (у Strict есть место): `w:br@clear` (`RunContent::ClearingBreak`), явное «выкл.» у
`framePr@anchorLock`, `tblInd@type`, художественные рамки (`ST_Border`, 166 значений), диагонали
`tcBorders`, `w:ffData`, флаги и дети settings (`autoHyphenation`/`mirrorMargins` off, `activeWritingStyle`,
`smartTagType`, `stylePaneSortMethod`, начала сетки), `a:fmtScheme` рядом с текстурной заливкой, позиции
градиентов, `wpg:cNvPr`, внешние картинки `r:link`, картинки-маркеры (`w:numPicBullet` с `w:drawing`,
`w:lvlPicBulletId`, своя `numbering.xml.rels`; VML-маркер переводится T7, TZ-64 теперь `declared_transform`,
TZ-63 закрыт; рендер рисует картинку вместо текстового маркера), ревизия знака абзаца
(`w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` первыми в `w:pPr/w:rPr`, с обязательным `w:author`; TZ-72 закрыт).

Называется (места нет): VML-умолчания фигур в settings (TZ-60/61), текстура
темы (TZ-65), служебная перекодировка `stylePaneSortMethod` (TZ-62), явное off у `customMarkFollows` (TZ-71),
метки `w:ins`/`w:del` внутри непереносимого `w:rPrChange` (TZ-75), `prefixMappings` (TZ-74), переписанные при переносе диаграммы (TZ-80..82).

Находка: нормализатор печатает id потери как `<uri>#local`, census видит голое `local` — `requires_cited`
с `w:` для потерь нормализатора не работает; TZ-60/61 привязаны к VML-именам из текста причины.

Не делалось: история изменений форматирования (`w:rPrChange`/`w:pPrChange` — пункт 3.6 плана редактора),
`customMarkFollows=1` (свой знак сноски).

## 3. Clio

`strict-ooxml/tests/clio_wps_geometry.rs` — 26 обязательных точек P1 со страниц 54/56/104, эталон из
wps-gate ledger, допуск 0,25 px. Выполняется там, где есть локальный корпус (гейт на хосте его копирует).

Запас не поднят. Диагностика: WPS округляет ширину каждого глифа до сетки 0,6 pt и ставит начало каждого
слова на сетку 1/600 дюйма, остаток выравнивания уходит в пробелы. Калибровка `--wps-times` — один
коэффициент поверх этого; на двух тесных строках знаки ошибки противоположны (`p104.modern.5` −0,248,
`p54.HVR_I.2` +0,241), так что коэффициентом не поднять. Нужен режим, воспроизводящий раскладку WPS —
отдельная задача.

## Гейты

`f904115`: fmt, clippy, test 1785/0, deny, doc — зелёные; census PASS (0); XSD PASS (наших 0, источника 11).
