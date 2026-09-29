# Заказ на доработку. Этап 5 (5A) — повторная приёмка (колонтитулы, упаковка фикстуры)

**Шифр:** ZAKAZ-STAGE-5-1 (STAGE-5-REWORK-1)  
**Крейты:** `strict-ooxml-render-svg` (колонтитулы), `xtool` (генератор
фикстуры), `strict-ooxml-wml`/`strict-ooxml-core` (тесты/корпус),
`strict-ooxml` (сквозной тест)  
**Основание:** приёмка эшелона 5A (`STAGE-5-ACCEPTANCE.md`): обнаружены
блокирующие дефект рендера колонтитулов и дефект упаковки фикстуры,
исказивший вывод об эталоне WPS  
**Обнаружил:** приёмка (независимая проверка: регенерация WPS-эталона,
инспекция SVG, внешний SSIM)  
**Дата:** 2026-09-29  
**Приоритет:** высокий (блокирует приёмку 5A; ломает продуктовый инвариант
«default/first/even выбираются верно» и независимый оракул)  
**Оценка:** 16–28 ч

> Только требования и критерии приёмки. Код не приводится.

---

## 0. Что подтверждено приёмкой (сохранить без регрессий)

- Гейты `fmt/clippy/test/doc/deny` — зелёные; 348 тестов, 0 падений; покрытие
  `core` 88.53 %, `wml` 86.56 %, `report` 99.43 %, `render-svg` 89.70 %;
  `xtool`-гейты 97.3 % и 5A 90.2 %.
- Детерминизм: два прогона CLI `render` дают побайтово идентичные SVG.
- Работает в `strict-stage5.docx`: 2-проходные поля (`Page 1 of 2`),
  многоуровневая нумерация (`1.`, `1.1`, `1.2`, `2.`), `gridSpan`/`vMerge`,
  концевые сноски в конце документа, резолв темы в каскаде.

Ниже — только то, что требует доработки.

---

## 1. Проблема B5-1 (блокирующая). `titlePg` без first-колонтитула падает в Default

**Факт.** `strict-ooxml-render-svg/src/layout/headerfooter.rs`,
`select_reference`: при `page_number == 1 && title_page` выбирается first-ссылка
только если она есть; иначе управление уходит на общий путь и возвращается
**Default** (для первой страницы — и/или Even).

**Ожидаемое (по спецификации и по WPS).** Если `w:titlePg` задан, первая
страница использует **first**-колонтитул; при отсутствии `first`-ссылки
колонтитул первой страницы **пуст** — fallback на Default/Even недопустим.

**Контролируемый эксперимент (воспроизведено при приёмке).**
`strict-stage5.docx`: `w:titlePg` есть, заданы только `header default`,
`header even`, `footer default`. Наш вывод:

```text
page-1.svg: содержит "Default header" и "Default footer"   <-- НЕВЕРНО
page-2.svg: содержит "Even header"                          (верно)
```

WPS-эталон (см. B5-2, после исправления упаковки) на странице 1 колонтитул
**не рисует** (первая текстовая полоса y ≈ 105), у нас — полоса колонтитула
y ≈ 56 и лишний футер.

**Почему тесты не поймали.** `strict-ooxml-render-svg/tests/headers.rs`,
`first_page_header_wins_when_title_page` **всегда подаёт** `first`-часть; случай
«`titlePg` без first» не покрыт (самосогласованная фикстура).

**Требование B5-1.**
1. При `page_number == 1 && titlePg` выбор **ограничить** `First`: вернуть
   `find(First)` (возможно `None`) — **без** перехода к Default/Even.
2. Применить к заголовкам и футерам (общая функция выбора).
3. Проверить комбинации: `titlePg`+`evenAndOddHeaders` (стр. 1 first/пусто;
   стр. 2 even/пусто; далее default), `titlePg` без first, first без titlePg.
4. Сохранить корректные случаи: default на всех страницах; first при наличии;
   even при `evenAndOddHeaders`.

**Критерии приёмки B5-1.**
1. Регресс-тест: при `titlePg` без first на первой странице **нет** ни
   Default-, ни Even-колонтитула (hdr и ftr), на последующих — Default.
2. Существующие тесты `headers.rs` (default/first/even, геометрия, футер) —
   зелёные.
3. На `strict-stage5.docx`: `page-1.svg` не содержит текста default-заголовка/
   футера; `page-2.svg` — содержит even-заголовок (как сейчас).
4. Ни одна страница не теряет контент; координаты конечны; SVG валиден.

---

## 2. Проблема B5-2 (блокирующая). Генератор фикстуры пишет non-ISO content types

**Факт.** `xtool/src/main.rs` (`gen-docx`): `[Content_Types].xml` объявляет
**legacy/transitional** типы `application/vnd.ms-word.*`:

```text
document/settings/numbering/theme/header/footer/footnotes/endnotes -> vnd.ms-word.*
```

Валидированные ранее Strict-фикстуры проекта (`strict-text.docx`,
`strict-profile.docx`, для которых WPS даёт корректный эталон) используют
стандартные ISO/IEC 29500-2 типы `application/vnd.openxmlformats-officedocument.*`.

**Последствие (воспроизведено).** На `strict-stage5.docx` `kwpsconvert
word2photo` (WPS `12.1.0.28485`) выдаёт **12 страниц** с бессмысленным
содержимым (≈180 полос текста при ~15 строках в документе). **Замена только
content-type строк** (XML-части не меняются) даёт WPS **2 страницы**, а SSIM
против нашего рендера — **0.9612 / 0.9901** (≥ 0.95).

**Вывод.** Утверждение приёмки A-3 («WPS не соблюдает минимальную Strict-модель,
эталон не применим») — **неверный диагноз**. Причина — дефект упаковки фикстуры;
WPS-эталон для 5A **применим** и должен быть включён в гейт.

**Требование B5-2.**
1. Исправить `xtool/src/main.rs`: все части — стандартные ISO content types
   (`...openxmlformats-officedocument.wordprocessingml.{document,settings,numbering,
   header,footer,footnotes,endnotes}.main/settings/...` и
   `...openxmlformats-officedocument.theme+xml`), в соответствии с уже
   закоммиченными Strict-фикстурами.
2. Перегенерировать `strict-ooxml-core/tests/strict/strict-stage5.docx`
   детерминированно (тем же `xtool gen-docx --stage5`).
3. Сгенерировать WPS-эталон `refs/strict-stage5/page_N.png` (пиннинг
   `12.1.0.28485`), закоммитить с командой/версией/SHA-256 (как на Этапе 4).
4. Включить `strict-stage5` в SSIM-гейт (`tests/ssim.rs`): **SSIM ≥ 0.95 +
   структурный инвариант (обе оси) + инвариант страниц** (после B5-1).
5. Проверить, что generic `gen-docx` (не только `--stage5`) тоже исправлен.

**Критерии приёмки B5-2.**
1. `xtool gen-docx --stage5` выдаёт стандартные content types (регресс-тест или
   внешняя проверка `zip`+`roxmltree`).
2. На перегенерированной фикстуре WPS `word2photo` даёт **2 страницы** и
   воспроизводимый (байтово) результат; эталон закоммичен с SHA-256.
3. `cargo test -p strict-ooxml-render-svg --test ssim` — зелёный, включая
   `strict-stage5`; `strict-profile` 2/2 и эталоны Этапа 4 — без регрессий.
4. `strict-ooxml-wml/tests/stage5_fixture_oracle.rs` и `strict-ooxml` corpus —
   зелёные (при необходимости — с проверкой content types).

---

## 3. Проблема B5-3 (значимая). Карта покрытия 5A завышена; A-2 открыт

**Факт.** `coverage/stage5-scenarios.toml` помечает `headers.first`/
`headers.default` как `supported`, но поведение при `titlePg` без first —
неверно (B5-1). Критерий §11.1 требует **независимого** подтверждения матрицы
(приёмка A-2); независимая проверка выявила дефект, т.е. матрица не отражает
реальность.

**Требование B5-3.**
1. После B5-1 привести карту в соответствие фактам (при необходимости —
   разбить `headers.first`/`titlePg`-случай, понизить до `partial` до фикса).
2. Зафиксировать **независимое** подтверждение матрицы (ревью третьей
   стороной / приёмочный прогон) — A-2 закрывается только этим.
3. Исправить `STAGE-5-ACCEPTANCE.md` A-3 (неверная причина) и, при готовности
   SSIM 5A, отметить критерий §11.2 как выполненный WPS-эталоном.

**Критерий приёмки B5-3.** Матрица не содержит завышенных `supported`;
независимое подтверждение оформлено; A-3 переписан по факту.

---

## 4. Что НЕ входит

- Эшелоны 5B/5C (DrawingML-якоря/фигуры/группы, границы страниц, MathML).
- Этап 6 (нормализация Transitional), VML.
- Расширение функциональности колонтитулов (разрыв на несколько страниц,
  посекционные колонтитулы) — остаются ограничениями A-4.
- Изменение публичного API, кроме необходимого для гейта.

---

## 5. Порядок сдачи и повторная приёмка

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test  --workspace --all-features
cargo doc   --workspace --no-deps
cargo deny  check

# перегенерация фикстуры после фикса content types:
cargo run -p xtool -- gen-docx --stage5 --out strict-ooxml-core/tests/strict/strict-stage5.docx

# WPS-эталон (пиннинг 12.1.0.28485):
kwpsconvert.exe word2photo --input strict-stage5.docx --output refs/

# гейт визуальной точности (включая strict-stage5):
cargo test -p strict-ooxml-render-svg --all-features --test ssim -- --nocapture
cargo test -p strict-ooxml-render-svg --all-features --test headers
cargo test -p strict-ooxml --all-features --test stage5_corpus
cargo run -p xtool -- coverage --file coverage/stage5-scenarios.toml --min 85
```

Ожидается: колонтитулы первой страницы при `titlePg` без first — пустые;
WPS-эталон 5A даёт 2 страницы; SSIM ≥ 0.95 + структурный инвариант зелёные;
все гейты зелёные.

---

## 6. Definition of Done

- [ ] **B5-1**: `titlePg` без first не рисует Default/Even на первой странице
      (hdr+ftr); регресс-тест; существующие тесты зелёные.
- [ ] **B5-2**: `xtool gen-docx` — стандартные ISO content types (и generic);
      фикстура перегенерирована; WPS-эталон 5A закоммичен (SHA-256, версия,
      команда); `strict-stage5` в SSIM-гейте (≥ 0.95 + структурный + страницы).
- [ ] **B5-3**: карта 5A не завышена; независимое подтверждение оформлено;
      `STAGE-5-ACCEPTANCE.md` A-3 исправлен.
- [ ] Гейты fmt/clippy/test/doc/deny, покрытие ≥ 80 % — зелёные; нет регрессий
      Этапа 4 (`strict-profile` 2/2, `strict-text`/`strict-text-grid` SSIM).
- [ ] Повторная приёмка (ACCEPT-STAGE-5) пройдена.

---

## 7. Приложение. Репро (команды и ожидаемые числа)

**B5-1 (titlePg без first):**
```text
render strict-stage5.docx --out out/
page-1.svg: содержит "Default header"/"Default footer"   <-- ДЕФЕКТ (ожидается пусто)
page-2.svg: содержит "Even header"                        (верно)
WPS page 1: первая текстовая полоса y≈105 (без колонтитула); у нас — y≈56
# покрытие: tests/headers.rs::first_page_header_wins_when_title_page всегда даёт first
```

**B5-2 (content types → WPS):**
```text
# исходная фикстура (vnd.ms-word.*): WPS word2photo -> 12 pages (мусор, ~180 строк)
# замена только content-type строк на ISO:
#   main:   application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml
#   header: ...wordprocessingml.header+xml  (footer/footnotes/endnotes/settings/numbering аналогично)
#   theme:  application/vnd.openxmlformats-officedocument.theme+xml
# -> WPS word2photo -> 2 pages
# SSIM(наш, WPS) = 0.9612 (page 1) / 0.9901 (page 2)
```

**Затронутые места:**
- `strict-ooxml-render-svg/src/layout/headerfooter.rs` — `select_reference`;
- `strict-ooxml-render-svg/tests/headers.rs` — добавить регресс-тест;
- `xtool/src/main.rs` — content types (`--stage5` и generic);
- `coverage/stage5-scenarios.toml`, `STAGE-5-ACCEPTANCE.md`, `docs/stage-5-report.md`,
  `strict-ooxml-core/tests/strict/refs/README.md`.

---

**Конец заказа.**
