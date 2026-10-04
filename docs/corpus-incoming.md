# Корпус: документы, ожидающие разбора (`strict-ooxml-core/tests/docx-incoming/`)

**Поступление:** 2026-10-03, 64 документа. **Решение владельца:** вынести их из `docx/` в
`docx-incoming/` (в `.gitignore`), пока их дефекты не закрыты; гейт §0.4 работает на принятом
корпусе (`docx/`).

## Закрыто 2026-10-04 (AUD-17 / AUD-37 / AUD-38 / AUD-68 / AUD-69)

18 документов из таблицы ниже разобраны задачами:

| Задача | Документы | Куда |
|---|---|---|
| **AUD-68** | 12 (`1. First-Steps…` … `ProjectionSolver Design`, `doc-with-toc`) | → `tests/docx/` |
| **AUD-69** | 3 (`Contoso_Guest_WiFi…`, `SampleEmploymentAgreement`, `SampleOfferLetter`) | → `tests/docx/` |
| **AUD-37** | 1 (`Spanner visibility graph`) | → `tests/docx/` |
| **AUD-38** | 1 (`Programming-Basics-CSharp-…-Nakov-v2019`) | → `tests/docx/` |
| **AUD-17** | 1 (`rec.docx`) | удалён; случай в `strict-ooxml/tests/hostile.rs` (`opc`) |

Чистые 46 были возвращены ранее (2026-10-03). Корпусный гейт §0.4 теперь на полном наборе
в `docx/` (без битого `rec.docx`).

## История разбора (2026-10-03)

Прогон на коммите `83fa60c` в отдельном worktree со всеми 100 документами:

- `cargo test --workspace --all-features` — красный **только** `strict-ooxml-write --test
  normalize_roundtrip`;
- XSD-гейт — PASS; census — PASS.

| Документы | Тесты `normalize_roundtrip` | Причина | Задача |
|---|---|---|---|
| 12 файлов с блочным `w:sdt` | `nothing_that_reaches_the_page_disappears` / fixed-point | писатель разворачивал `w:sdt` | **AUD-68** ✅ |
| 3 Contoso/Sample* | `every_written_part_is_well_formed_xml` | `xmlns` на соседних `customXml` | **AUD-69** ✅ |
| `Spanner visibility graph` | 6 из 9 | `wps:` без объявления | **AUD-37** ✅ |
| `Programming-Basics-…` | fixed-point | `a:srcRect` внутри `a:blip` | **AUD-38** ✅ |
| `rec.docx` | не открывается | битый ZIP | **AUD-17** ✅ |

## Зависимости (поступление 2026-10-04)

Синтетические фикстуры, провоцирующие известные дыры апстрим-зависимостей
(`quick-xml`, `hayro`). **Не** часть гейта §0.4 и **не** кандидаты в `docx/`:
это локальный карантин для AUD-95…AUD-99. Пересоздать:

```text
cargo +1.92.0 run -p strict-ooxml-testkit --example write_dep_incoming
```

| Файл | Что провоцирует | Задача |
|---|---|---|
| `dep-hayro-jbig2-absurd.pdf` | JBIG2 / ImageXObject с `Width×Height` за бюджетом (hayro#1259) | **AUD-95** ✅ |
| `dep-hayro-inline-absurd.pdf` | Inline image `/W 4294967295` | **AUD-95** ✅ |
| `dep-hayro-deep-dict.pdf` | Глубокая литеральная вложенность `<<` в trailer (stack abort) | **AUD-96** ✅ |
| `dep-hayro-tiling-self.pdf` | Самоссылающийся tiling-паттерн | **AUD-96** ✅ |
| `dep-hayro-kids-cycle.pdf` | Цикл `/Kids` в дереве страниц | **AUD-96** ✅ |
| `dep-hayro-cid-huge-w.pdf` | CID `/W [0 4294967295 …]` | **AUD-97** ✅ |
| `dep-quickxml-many-attrs.docx` | O(N²)/лимит атрибутов (RUSTSEC-2026-0194) | **AUD-98** ✅ |
| `dep-quickxml-xmlns-bomb.docx` | Много `xmlns:` на одном теге (RUSTSEC-2026-0195) | **AUD-98** ✅ |
| `dep-quickxml-deep-ns.docx` | Глубокая вложенность + `xmlns` на уровень (#977/#980) | **AUD-98** ✅ |
| `dep-quickxml-doctype.docx` | `DOCTYPE` + внешняя entity (XXE) | **AUD-98** ✅ |
| `dep-quickxml-custom-entity.docx` | Непредопределённая entity | **AUD-98** ✅ |
| `dep-quickxml-dup-attr.docx` | Дубликат имени атрибута | **AUD-98** ✅ |

Приёмка каждой AUD: `hostile` зелёный на том же случае (G-6: бинарник не
коммитится; файлы здесь — локальный оракул для ручного прогона и апстрим-PR),
патч в `vendor/` или подтверждение, что апстрим уже закрыл дыру, запись в
`vendor/README.md`.

## Корпус CC0 (`testdata/CC0/`, 2026-10-04)

100 Transitional `.docx` (CC0, ~37 MiB, gitignored). Не в `docx-incoming/`:
это отдельный локальный оракул. Прогон — `corpus_scan`; сводка —
`.scratch/cc0-findings.md`. Четыре ранее красных оракула (AUD-100…103)
**4/4 OK** 2026-10-04; полный каталог — повторный `corpus_scan`.

| Файл | Симптом | Задача |
|---|---|---|
| `046_20260814_20260814_1519_docx.docx` | `fontTable.xml`: duplicate `w:characterSet` после T4 | **AUD-100** ✅ |
| `030_20240916_20240916_1838_docx.docx` | writer fixed-point: −~50 image rels | **AUD-101** ✅ |
| `065_4chan-clubpenguin_GX-SWC-GSAT_Guide_v01.docx` | writer fixed-point: теряется `image1.wmf` | **AUD-102** ✅ |
| `020_20-de-thi-cuoi-hoc-ki-1-lop-5_20_e_thi_cuoi_hoc_ki_1_lop_5.docx` | writer fixed-point: `document.xml` +~1200 B | **AUD-103** ✅ |

```text
cargo +1.92.0 run -p strict-ooxml --features write,svg --example corpus_scan --release -- testdata/CC0
```
