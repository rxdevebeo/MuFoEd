# Заказ на доработку. Этап 5B — повторная приёмка (пространства DrawingML-расширений, WPS-оракул)

**Шифр:** ZAKAZ-STAGE-5B-1 (STAGE-5B-REWORK-1)  
**Крейты:** `xtool` (генератор фикстуры), `strict-ooxml-core` (корпус/эталоны),
`strict-ooxml-render-svg` (SSIM-гейт), `strict-ooxml-wml` (оракул),
`strict-ooxml` (сквозной тест)  
**Основание:** приёмка эшелона 5B (`docs/stage-5b-report.md`): обнаружен дефект
упаковки фикстуры (несуществующие ISO-пространства для `wps`/`wpg`), из-за
которого WPS-эталон был ошибочно признан «неприменимым»; SSIM-гейт для 5B
отсутствует  
**Обнаружил:** приёмка (независимая проверка: замена URI пространств и
регенерация WPS-эталона, внешний SSIM, инспекция цветов)  
**Дата:** 2026-09-29  
**Приоритет:** высокий (блокирует критерий §11.3; лишает 5B независимого
оракула)  
**Оценка:** 12–24 ч

> Только требования и критерии приёмки. Код не приводится.

---

## 0. Что подтверждено приёмкой (сохранить без регрессий)

- Гейты `fmt/clippy/test(372)/deny` — зелёные; 5B-гейт сценариев 92.4 %.
- `strict-stage5b.docx`: **корректные ISO content types**, генератор
  детерминирован и совпадает с закоммиченной фикстурой.
- **`strict-profile.docx` теперь рендерит текст поля** (`Your text here`) —
  критерий §11.4 выполнен.
- Наш рендерер корректно обрабатывает **реальные Microsoft-расширения**
  (`wps`/`wpg`): при их использовании совпадает с WPS (см. §1).
- Границы страниц (`w:pgBorders`) рендерятся; эталоны/SSIM Этапа 4 и 5A — без
  регрессий (`strict-text` 0.9768, `strict-text-grid` 0.9709, `strict-stage5`
  0.9795, `strict-profile` 2/2).

Ниже — только то, что требует доработки.

---

## 1. Проблема 5B-1 (блокирующая). Несуществующие ISO-пространства `wps`/`wpg`

**Факт.** Генератор `xtool gen-docx --stage5b` объявляет и использует
пространства

```text
wps = http://purl.oclc.org/ooxml/drawingml/wordprocessingShape
wpg = http://purl.oclc.org/ooxml/drawingml/wordprocessingGroup
```

(`xtool/src/main.rs:819-820` и вся разметка фигуры/группы/поля в фикстуре).
**Этих пространств нет в ISO/IEC 29500-1:** `WordprocessingShape` и
`WordprocessingGroup` — расширения Microsoft
(`http://schemas.microsoft.com/office/word/2010/wordprocessingShape` и
`.../wordprocessingGroup`). Реальные Strict-документы Word используют именно
MS-расширения (что и делает `strict-profile.docx`).

**Последствие (воспроизведено).** На закоммиченной `strict-stage5b.docx` WPS
`12.1.0.28485` рисует **только границы страницы** — фигуры/группа/поле/картинка
игнорируются (пикселей цвета заливок ≈ 0). Отсюда вывод отчёта §5 «WPS не
отрисовывает `wordprocessingShape` в ISO-Strict, эталон неприменим» — **неверный
диагноз** (аналог B5-2 в 5A).

**Контролируемый эксперимент.** Замена **только** URI `wps`/`wpg` на реальные
MS-расширения (XML не меняется) даёт:

| Вариант | WPS: заливки (px) | Наш рендер |
|---|---|---|
| committed (`purl.oclc.org/.../wordprocessingShape`) | blue 0, green 0, orange 22 | shapes есть |
| заменены на `.../office/word/2010/wordprocessingShape\|Group` | blue 13164, green 13164, orange ~13176 (ink 4.26 %) | blue 13164, green 13164, orange ~13148; **SSIM 0.9641** |

Т.е. при корректных расширениях WPS-эталон **применим**, и наш рендер ему
соответствует.

**Требование 5B-1.**
1. В `xtool gen-docx --stage5b` использовать **реальные** пространства
   расширений для `wps`/`wpg`
   (`http://schemas.microsoft.com/office/word/2010/wordprocessingShape` /
   `.../wordprocessingGroup`) во всех элементах (`wps:wsp`, `wps:spPr`,
   `wpg:wgp`, `w:txbxContent` и т.д.), сохранив строгие ISO-пространства для
   ядра WML (`w`) и DrawingML (`wp`/`a`/`pic`) — как в `strict-profile`.
2. Перегенерировать `strict-ooxml-core/tests/strict/strict-stage5b.docx`
   детерминированно.
3. Убедиться, что WPS `word2photo` на новой фикстуре **рендерит фигуры** (не
   только границы), и сгенерировать эталон `refs/strict-stage5b/page_N.png`
   (пиннинг `12.1.0.28485`, SHA-256, команда — как в 5A).
4. Обновить независимый оракул/тесты так, чтобы они проверяли **реальный**
   путь (MS-расширения), а поддержка «ISO-`wps`» (если сохраняется) помечалась
   как совместимость, а не как основной путь.

**Критерии приёмки 5B-1.**
1. `xtool gen-docx --stage5b` выдаёт фикстуру с MS-расширениями для `wps`/`wpg`
   (регресс-тест/внешняя проверка `zip`+`roxmltree`).
2. На перегенерированной фикстуре WPS рендерит фигуры/группу/поле/картинку
   (проверка по цвету заливок); эталон закоммичен с SHA-256.
3. `strict-ooxml-wml/tests/stage5b_fixture_oracle.rs` и
   `strict-ooxml/tests/stage5b_corpus.rs` — зелёные; `strict-profile` — без
   регрессий.

---

## 2. Проблема 5B-2 (блокирующая). У 5B нет WPS-эталона и SSIM-гейта

**Факт.** В репозитории нет `refs/strict-stage5b/`, в `tests/ssim.rs`
`strict-stage5b` не участвует. Критерий §11.3 (SSIM ≥ 0.95 + структурный
инвариант для фигур/групп) **не выполнен**; отчёт §5 прямо фиксирует
«структурный инвариант … без SSIM».

**Требование 5B-2.** После 5B-1 подключить `strict-stage5b` к SSIM-гейту:
- **SSIM ≥ 0.95** + инвариант страниц;
- rasterizer-независимые проверки — **обязательны всегда** (ink/blank,
  сдвиг профиля, дрейф центроида);
- пороги корреляций строк/столбцов — ослабленные, как `STAGE5_LIMITS` (см.
  §4: на графике они ниже текстовых), но не отключённые;
- негативный контроль: **пустой рендер отвергается**.

**Критерии приёмки 5B-2.**
1. `cargo test -p strict-ooxml-render-svg --test ssim` зелёный и включает
   `strict-stage5b`; для него enforced ink/blank + сдвиг/центроид + SSIM ≥ 0.95.
2. Контроль-тест «пустая страница для `strict-stage5b` отвергается».
3. Эталоны/пороги Этапа 4 и 5A — без регрессий.

---

## 3. Проблема 5B-3 (процессная). Карта покрытия и унаследованные открытые пункты

**Факт.** `coverage/stage5-scenarios.toml` (5B-секция, 92.4 %) составлена
исполнителем; независимое подтверждение (требование приёмки) не оформлено.
Открытые пункты A-1/A-2 эшелона 5A не закрыты.

**Требование 5B-3.**
1. Независимо подтвердить карту 5B (ревью/приёмочный прогон) либо явно
   зафиксировать как неподтверждённую с соответствующим статусом.
2. Зафиксировать решение по A-1 (реальный Strict под попиксельный SSIM) и
   A-2 (независимое подтверждение карты 5A).

**Критерий приёмки 5B-3.** Статусы карт 5A/5B и A-1/A-2 корректно отражены в
`STAGE-5B-ACCEPTANCE.md`/`docs/stage-5b-report.md`.

---

## 4. Что НЕ входит

- Эшелон 5C (MathML/OMML), Этап 6 (нормализация Transitional), VML.
- Растеризация содержимого charts/SmartArt.
- Расширение функциональности 5B (полный `custGeom`, `wrapTight` по контуру,
  наконечники линий, поворот групп) — по-прежнему ограничения, не блокер.
- Изменение публичного API, кроме необходимого для гейта.

---

## 5. Порядок сдачи и повторная приёмка

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test  --workspace --all-features
cargo doc   --workspace --no-deps
cargo deny  check

# перегенерация фикстуры (MS-расширения):
cargo run -p xtool -- gen-docx --stage5b --out strict-ooxml-core/tests/strict/strict-stage5b.docx

# WPS-эталон (пиннинг 12.1.0.28485):
kwpsconvert.exe word2photo --input strict-stage5b.docx --output refs/

# гейт визуальной точности (включая strict-stage5b):
cargo test -p strict-ooxml-render-svg --all-features --test ssim -- --nocapture
cargo test -p strict-ooxml-render-svg --all-features --test strict_profile
cargo test -p strict-ooxml --all-features --test stage5b_corpus
cargo run -p xtool -- coverage --file coverage/stage5-scenarios.toml --min 85
```

Ожидается: WPS рендерит фигуры/группу/поле/картинку; SSIM ≥ 0.95 +
обязательные структурные проверки зелёные; пустой рендер отвергается; все гейты
зелёные.

---

## 6. Definition of Done

- [ ] **5B-1**: `xtool gen-docx --stage5b` использует реальные MS-расширения для
      `wps`/`wpg`; фикстура перегенерирована; WPS-эталон `refs/strict-stage5b/`
      закоммичен (SHA-256, версия, команда).
- [ ] **5B-2**: `strict-stage5b` в SSIM-гейте (SSIM ≥ 0.95 + ink/blank + сдвиг/
      центроид; корреляции — ослабленные); контроль пустой страницы.
- [ ] **5B-3**: карта 5B подтверждена (или явно помечена); A-1/A-2 зафиксированы.
- [ ] `strict-profile` рендерит текст поля; регрессий Этапа 4/5A нет.
- [ ] Гейты fmt/clippy/test/doc/deny, покрытие ≥ 80 % — зелёные.
- [ ] Повторная приёмка (`ACCEPT-STAGE-5B`) пройдена.

---

## 7. Приложение. Репро (команды и ожидаемые числа)

**5B-1 (пространства `wps`/`wpg`):**
```text
# committed фикстура: wps = http://purl.oclc.org/ooxml/drawingml/wordprocessingShape
#   -> WPS рисует только границы страницы; заливки blue/green/orange ≈ 0 px
# замена только URI на MS-расширения:
#   wps -> http://schemas.microsoft.com/office/word/2010/wordprocessingShape
#   wpg -> http://schemas.microsoft.com/office/word/2010/wordprocessingGroup
#   -> WPS рисует фигуры: blue 13164, green 13164, orange ~13176 (ink 4.26%)
#   -> наш рендер: blue 13164, green 13164, orange ~13148; SSIM = 0.9641
```

**5B-2 (структурные метрики на корректной фикстуре):**
```text
SSIM = 0.9641 (>= 0.95)
corr_y = 0.724, corr_x = 0.601  -> ниже текстовых порогов 0.9/0.85
   (нужны ослабленные пороги, но ink/blank + сдвиг/центроид обязательны)
ink ref = 0.0273, cand = 0.0280
```

**`strict-profile` (критерий §11.4):**
```text
render strict-profile.docx -> page-2.svg содержит "Your text here" (текст поля)
```

**Затронутые места:**
- `xtool/src/main.rs` (URI `WPS_NS`/`WPG_NS`, разметка фикстуры);
- `strict-ooxml-core/tests/strict/strict-stage5b.docx`, `refs/strict-stage5b/`,
  `refs/README.md`;
- `strict-ooxml-render-svg/tests/ssim.rs` (подключение `strict-stage5b`),
  golden/корпус;
- `coverage/stage5-scenarios.toml`, `docs/stage-5b-report.md`.

---

**Конец заказа.**
