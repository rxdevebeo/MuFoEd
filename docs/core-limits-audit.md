# REWORK-CORE-1 — лимиты декомпрессии: отчёт о доработке

**Шифр:** ZAKAZ-CORE-1 (REWORK-CORE-1)  
**Крейт:** `strict-ooxml-core`  
**Основание:** `REWORK-CORE-LIMITS.md`; приёмка Этапа 4 на новых файлах
`strict-ooxml-core/tests/samples/`  
**Дата:** 2026-09-28  
**Статус:** C-1 и C-2 закрыты; аудит прочих лимитов выполнен; гейты зелёные.

---

## 1. C-1 — ложное срабатывание `max_compression_ratio` (закрыто)

**Причина.** Дефолт `max_compression_ratio = 200` отвергал легитимный
стресс-документ `sample-docx-200kb.docx`: `word/document.xml`
193 805 Б сжатый → 44 153 871 Б распакованный (отношение **227.8**), при том что
абсолютные лимиты (128 МиБ / 512 МиБ) проходят.

**Что сделано.**

1. Дефолт поднят до **1000** (`limits.rs`), близко к теоретическому максимуму
   отношения deflate (≈1032:1). Документ с отношением 227 больше не отвергается.
2. Проверка отношения сделана **вторичной**: в `opc/zip.rs` абсолютные
   `max_single_uncompressed`/`max_total_uncompressed` проверяются **до** отношения,
   поэтому основной барьер — абсолютные размеры, а отношение ловит только бомбы,
   остающиеся ниже них.
3. Защита от бомб сохранена: синтетическая запись 1000 Б → 100 000 000 Б
   (отношение 100 000, ниже абсолютных лимитов) отклоняется по
   `CompressionRatio`; запись сверх `max_single_uncompressed` отклоняется по
   `SingleUncompressed`.
4. Лимиты по-прежнему переопределяемы через `OpenOptions::limits`; документация
   `limits.rs` и ТЗ §12.1 обновлены.

**Критерии приёмки C-1.**

| № | Критерий | Статус |
|---|---|---|
| 1 | `sample-docx-200kb.docx` открывается при дефолтных лимитах | ✅ `tests/samples_corpus.rs::legitimate_stress_document_opens_and_strict_ratio_still_rejects` (файл Transitional, поэтому открывается `Permissive`; лимит не срабатывает) |
| 2 | Бомба с отношением ≥ 1000 и размером ниже абсолютных лимитов → `LimitExceeded` | ✅ `opc/zip.rs::compression_bomb_is_rejected_and_reports_the_ratio` |
| 3 | При `max_compression_ratio: 200` документ отклоняется | ✅ тот же тест + `strict_ratio_override_rejects_a_legitimate_document` |
| 4 | Обновлены `limits.rs`, ТЗ §12.1; без `xfail`-обходов | ✅ |

> Замечание: критерий №1 в заказе записан как `OpenOptions::default()` (StrictOnly),
> но `sample-docx-200kb.docx` — **Transitional**, поэтому `StrictOnly` законно
> возвращает `TransitionalNotSupported`. Дефект был именно в лимите; `inspect`
> (Permissive) теперь открывает файл и определяет conformance:
> `conformance: Transitional`.

---

## 2. C-2 — неверное поле `actual` (закрыто)

`LimitExceeded { kind: CompressionRatio, .. }` теперь сообщает **отношение**
(`uncompressed / compressed`), а не распакованный размер в байтах. Для
`compressed_size == 0` (вырожденный случай) отношение считается неопределённым и
репортится как `u64::MAX`.

Пример: для стресс-документа при строгом пороге 200 —
`limit = 200, actual = 227` (а не `44153871`).

---

## 3. Аудит прочих дефолтных лимитов (§3 заказа)

Прогон: `cargo test -p strict-ooxml-core --test limits_audit -- --nocapture`.
Открыто **58** `.docx` (22 публичных `tests/samples/` + 36 локальных
`tests/docx/`), permissive, дефолтные лимиты. Все файлы уложились; ни один не
отклонён по лимиту.

| Лимит | Дефолт | Максимум на корпусе | Запас |
|---|---:|---:|---:|
| `max_compressed_input` | 536 870 912 (512 МиБ) | 2 051 606 (≈2.0 МиБ) | ≈256× |
| `max_single_uncompressed` | 134 217 728 (128 МиБ) | 44 153 871 (≈42.1 МиБ) | ≈3.0× |
| `max_total_uncompressed` | 536 870 912 (512 МиБ) | 44 174 641 (≈42.1 МиБ) | ≈12× |
| `max_parts` / `max_zip_entries` | 4096 | 86 частей | ≈47× |
| `max_xml_depth` | 256 | 20 | ≈12× |
| `max_text_len` | 67 108 864 (64 МиБ) | 1 396 Б | >10⁴× |
| `max_compression_ratio` | 1000 | 227 (стресс-документ) | ≈4.4× |

Вывод: единственный дефолт, дававший ложные срабатывания, —
`max_compression_ratio` (исправлен). Остальные лимиты имеют запас ≥3× на
корпусе; `max_single_uncompressed` — наименьший запас (3.0×), что приемлемо.

Примечания:
- `max_parts`/`max_zip_entries` измерены по числу частей; число записей ZIP
  ≥ числа частей (директории не считаются частями), верхняя граница совпадает.
- `max_xml_depth`/`max_text_len` измерены байт-сканером по `document.xml`;
  полноценно эти лимиты применяются в `XmlReader` и покрыты корпусными тестами
  `wml` (ограничения не срабатывают на корпусе).

---

## 4. Изменённые артефакты

- `strict-ooxml-core/src/limits.rs` — дефолт `max_compression_ratio = 1000` +
  документирующий комментарий.
- `strict-ooxml-core/src/opc/zip.rs` — абсолютные лимиты первичны; отношение
  вторично; `actual` = отношение; новые unit-тесты (бомба, строгий порог,
  легитимное 227:1).
- `strict-ooxml-core/tests/samples_corpus.rs` — регрессия на стресс-документ.
- `strict-ooxml-core/tests/limits_audit.rs` — аудит лимитов по корпусам + таблица.
- `TZ-STRICT-OOXML-RUST.md` §12.1 — значение 1000 и пояснение.

---

## 5. Порядок сдачи (фактические результаты)

| Проверка | Команда | Результат |
|---|---|---|
| Формат | `cargo fmt --all -- --check` | ✅ 0 |
| Линт | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ 0 |
| Тесты | `cargo test --workspace --all-features` | ✅ 0 падений |
| Покрытие core | `cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80` | ✅ **88.38%** строк |
| Зависимости | `cargo deny check` | ✅ ok |
| Репро | `cargo run -p strict-ooxml-cli -- inspect …/sample-docx-200kb.docx` | ✅ открывается, `conformance: Transitional` |

---

## 6. Definition of Done

- [x] **C-1**: 200 КБ→44 МБ документ открывается при дефолтных лимитах; бомба
  отклоняется.
- [x] **C-2**: `actual` в ошибке отношения — само отношение.
- [x] Аудит прочих лимитов с таблицей запасов на корпусе (§3).
- [x] Обновлены `limits.rs`, ТЗ §12.1, тесты.
- [x] Гейты (fmt/clippy/test/coverage) зелёные; повторная приёмка — см. отчёт.
