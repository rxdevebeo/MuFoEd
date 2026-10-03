# Корпус: документы, ожидающие разбора (`strict-ooxml-core/tests/docx-incoming/`)

**Поступление:** 2026-10-03, 64 документа. **Решение владельца:** вынести их из `docx/` в
`docx-incoming/` (в `.gitignore`), пока их дефекты не закрыты; гейт §0.4 работает на принятом
корпусе (`docx/`, 36 документов). Задачи — в `REWORK-AUDIT-2026-10.md`, раздел «Находки на
новом корпусе».

## Как разбирали

Прогон на коммите `83fa60c` в отдельном worktree со всеми 100 документами:

- `cargo test --workspace --all-features` — красный **только** `strict-ooxml-write --test
  normalize_roundtrip`; все остальные корпусные тесты (wml, render-svg, report, core, write
  `schema_order`/`passthrough`/`strict_conformance`, pdf) зелёные;
- XSD-гейт — PASS; census — PASS (122 документа, 20 названных потерь, ни одной неназванной).

`normalize_roundtrip` останавливается на первом плохом документе, поэтому затем он прогнан на
каждом новом документе по отдельности (тот же бинарь, в папке корпуса — один документ).

## Итог: 46 чистых, 18 с дефектами

| Документы | Тесты `normalize_roundtrip` | Причина | Задача |
|---|---|---|---|
| `1. First-Steps-in-Programming`, `2. Simple-Calculation`, `3. Simple-Conditions`, `4. Complex-Conditions`, `5. Loops`, `6. Nested-Loops`, `7. More-Complex-Loops`, `8. Become-a-Software-Engineer`, `doc-with-toc`, `Design for Rectilinear Edge Routing`, `OverlapRemoval Design`, `ProjectionSolver Design` (12) | `nothing_that_reaches_the_page_disappears`; у части также `the_written_form_is_a_fixed_point` | писатель разворачивает блочный `w:sdt`: дети становятся блоками `w:body`. Прирост блоков у каждого документа **в точности** равен «дети − 1» по всем его блочным `w:sdt` (+1 … +138) | **AUD-68** |
| `Contoso_Guest_WiFi_Connection_Guide`, `SampleEmploymentAgreement`, `SampleOfferLetter` (3) | `every_written_part_is_well_formed_xml` | в записанном `customXml/itemN.xml` нет `xmlns:xsd` на втором и следующих `xsd:schema` (в исходнике объявлен на каждом из соседних) | **AUD-69** |
| `Spanner visibility graph` (1) | 6 из 9 | нормализатор вставляет `wps:` в `document.xml`, не объявляя префикс; записанный документ наш же парсер не читает | **AUD-37** |
| `Programming-Basics-CSharp-Book-and-Video-Lessons-Nakov-v2019` (1) | `the_written_form_is_a_fixed_point` | запись не стабилизируется (16 512 273 → 16 507 911 байт), `w:sdt` в документе нет — причина не установлена | **AUD-38** |
| `rec.docx` (1) | все 9 (не открывается) | не документ: архив 433 байта, одна запись с неверным CRC, смещения центрального каталога не сходятся. Отказ открыть — правильное поведение | **AUD-17** |

**Чистые (46):** все остальные файлы папки. На них зелёны все 9 тестов `normalize_roundtrip`
по отдельности и весь остальной набор в общем прогоне.

## Порядок возврата в `docx/`

1. ✅ **Чистые 46 возвращены в `docx/` 2026-10-03** после коммитов Д-1/Д-2/Д-3. В `docx/`
   теперь 82 документа; полный гейт §0.4 на них зелёный (`normalize_roundtrip` включительно,
   census 98/104 без потерь, XSD-гейт PASS). В `docx-incoming/` осталось 18 — ровно таблица выше.
2. **Документы с дефектами — каждый задачей, которая его закрывает:** приёмка задачи включает
   перенос её документов из таблицы в `docx/` и зелёный `normalize_roundtrip` на них. Это и есть
   доказательство закрытия.
3. `rec.docx` в `docx/` не возвращается: он уходит в `hostile` (AUD-17) и удаляется.

Когда `docx-incoming/` опустеет, этот документ закрывается отметкой «закрыт, дата».
