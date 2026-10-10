# Этап 9 — отчёт о выполнении (импорт DjVu → Strict)

**Задача:** `STAGE-9-TASK.md` (ZAKAZ-STAGE-9)
**Связь с ТЗ:** §3 (формат), §4–§8 (9A–9E), §9 (критерии), §10 (нет записи DjVu),
§11–§13; ADR-0012, ADR-0013
**Окружение:** Windows x86_64 (MSVC), Rust 1.92, cargo-llvm-cov, cargo-deny,
DjVuLibre `3.5.28` как внешний оракул (WSL), WPS `12.1.0.28485`
**Крейты:** `strict-ooxml-djvu` (чтение), `strict-ooxml-convert` (`djvu`,
`djvu-image`), `strict-ooxml`, `strict-ooxml-cli`, `xtool`
**Статус:** реализовано; гейты и тесты зелёные; **полная приёмка этапа не
закрыта** — открыты `SC-D8` (ручной прогон WPS), полный корпусный прогон
`SC-D3…D14` и собственные oracle-драйверы для `NAVM`/XMP.

---

## 1. Что реализовано (9A–9E)

| Фаза | Содержание | Статус |
|---|---|---|
| **9A** | Контейнер: `AT&T`/`SDJV`, `FORM`/чанки, паддинг по **смещению**, `DIRM` bundled/indirect, `INCL`, `INFO` (LE dpi, флаги поворота), `DjvuLimits`, `djvu-dump` | готово |
| **9B** | Текстовый слой `TXTa`/`TXTz`: UTF-8 диапазоны, 17-байтовые зоны с дельтами (раскладка по типу зоны), три состояния `Readable`/`Empty`/`Unusable` | готово |
| **9C** | Изображение: JB2/`Djbz`, IW44 (progressive), палитра `FGbz`, `BGjp`/`FGjp`, обычный `Smmr`, legacy `PM44`/`BM44`; PNG за фичей `djvu-image` | готово; striped `Smmr` — явный отказ |
| **9D** | Конвертер `djvu.rs`: дерево зон → WML (semantic/visual), `INFO` → секции, `maparea` → `w:hyperlink` + закладки (`L1`/`L2`), `NAVM` → плоские закладки (`L3`), metadata/XMP → `docProps/core.xml` (`L4`), счётчики в отчёте (`L6`) | готово |
| **9E** | `recover.rs` → `recover/policy.rs` + поставщики; `DjvuOptions::text_recovery`/`figure_classifier` за фичами; фигуры из маски `Sjbz` → `FigureClassifier` → `wp:docPr/@descr`; OCR пустых и смешанных страниц | готово |

Приёмка 9A–9D подтверждена детерминированными native-фикстурами, оракулами
DjVuLibre и корпусом (`testdata-lock/djvu.toml`, 100 документов); 9E — тестами
на скриптовых моделях (мок-`TextRecovery`/`FigureClassifier`).

## 2. Критерии приёмки (`SC-D1…SC-D14`)

| # | Критерий | Что измерено | Статус |
|---|---|---|---|
| SC-D1 | Детерминизм (два прогона в байт) | `tests` конвертера, повторный прогон | тест-backed |
| SC-D2 | Нет паник на корпусе/битых файлах | `fuzz_djvu` компилируется; локальный fuzz-smoke не запустился (linker MSVC), CI — Linux | частично |
| SC-D3 | Индекс чанков = `djvudump` | **100/100** файлов | измерено |
| SC-D4 | Текст = `djvutxt` | 3/3 прямых; корпусный оракул 99 равных + 1 объяснённый | измерено |
| SC-D5 | Дерево зон = `djvutxt --detail=word` | 3 файла, **63 682 слова**, 0 px | измерено |
| SC-D6 | DjVu → Strict без потерь текста | **100/100** DOCX, независимое извлечение из `word/document.xml` | измерено |
| SC-D7 | Картинка = `ddjvu -format=ppm` | битональные страницы точно (`max=0`); IW44 — regression floor (mean ≤ 2.0, PSNR ≥ 30 dB) | частично (IW44-допуск) |
| SC-D8 | Открывается внешним редактором | **не прогнано** | открыто |
| SC-D9 | Ссылки на месте | `maparea` гейт 1→1; `NAVM`/XMP реализованы, собственного оракула нет | частично |
| SC-D10 | Ничего не теряется молча | инвариант «потеряно ⇒ в отчёте» на тестах | тест-backed |
| SC-D11 | OCR не в ядре | вывод идентичен без `djvu-image`+`ocr-ollama` | тест-backed |
| SC-D12 | Лицензии и лимиты | `cargo +1.92.0 deny check` зелёный; GPL в дереве нет | измерено |
| SC-D13 | Indirect (`DIRM`+`INCL`) | 3/3 (72/121/2 страницы развёрнуты), сверено с `djvudump` | измерено |
| SC-D14 | Поворот страницы | синтетические 90/180/270: зоны — с unrotated `djvutxt`, растр — с повёрнутым `ddjvu`, точно | измерено (синтетика) |

Полные корпусные прогоны `SC-D3…D14` на CI не зафиксированы как окончательные;
они остаются требованием приёмки.

## 3. Что осталось

- **SC-D8** — ручное открытие написанного `.docx` в WPS и фиксация результата.
- **Оракулы** `djvused print-outline` (для `NAVM`) и XMP: реализации есть,
  сверки с эталоном нет.
- **SC-D7 IW44** — измеренный regression floor, not full SSIM; поворот растра
  и `native`-зоны покрыты синтетикой, не всем корпусом.
- **OCR смешанных регионов** — контракт и тесты есть; качество реальной
  vision-модели не измерялось (запускался только мок).
- **Striped `Smmr`** — осознанный отказ (framing полос не подтверждён вектором).

## 4. Отчётность и бюджет

- Ридер не ходит в сеть/ФС сам; indirect-документы требуют явного разрешителя;
  пути проверяются на выход из каталога (`CLI`).
- `DjvuLimits` (страницы, чанки, байты, BZZ-блок, зоны, глубина, текст,
  компоненты, пиксели, JB2-символы) проверяются **до** работы, которую
  ограничивают.
- Keep-lost honest: каждый `Inferred`/`Lost` — строка отчёта (`djvu.*`,
  `djvu.annotation.*`, `djvu.outline.*`, `djvu.figure`, `djvu.ocr.*`).

## 5. Лицензии и provenance

- Декодеры — MIT-подкрейты `matyushkin/djvu-rs` (BZZ, JB2, IW44) с
  бюджетирующими патчами; DjVuLibre (GPL-2.0-only) **не** линкуется и не
  читается — только исполняется как оракул.
- `docs/waivers.toml` `DJVU-PATENT` фиксирует позицию по патенту
  US 6 058 214 (истёк, только про энкодинг) без FTO-поиска; товарный знак
  используется референтно.
- Таблица provenance по модулям — `strict-ooxml-djvu/PROVENANCE.md` и ADR-0012.

## 6. Воспроизведение

```powershell
cargo +1.92.0 test -p strict-ooxml-djvu -p strict-ooxml-convert -p strict-ooxml-cli --all-features --lib --tests
cargo +1.92.0 check --workspace --all-targets
cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.92.0 fmt --all -- --check
cargo +1.92.0 deny check
```

Детали и свидетельства: [DJVU_IMPORT.md](DJVU_IMPORT.md),
[djvu-validation/README.md](djvu-validation/README.md),
[adr/0012](adr/0012-djvu-reading.md), [adr/0013](adr/0013-djvu-conversion.md).
