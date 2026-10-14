# RTF: проверка готовности

2026-10-05. Ветка `codex/rtf-import`, изолированный worktree
`C:/Users/gamer/.codex/worktrees/rtf-import/StrictLib`. Проверочный снимок
расширенной реализации подготовлен поверх `fad5dc9` перед её коммитом.
Основной checkout и чужие документы editing/audit не изменялись.

**R07 и полная приёмка R08 остаются открытыми.** Совпадение текста и валидный
DOCX не доказывают структурную/визуальную совместимость. Наблюдаемые failures
не превращались в утверждённые expectations.

## Публичный путь

Owned WML/media с bounded import report доступны через crate и facade feature
`rtf`. CLI сохраняет DOCX и JSON report; viewer обнаруживает RTF и показывает
диагностику даже при наличии страниц. Общий core pipeline различает Clean,
Degraded и Failed. Корпусный профиль включается явно:

```powershell
target/debug/strict-ooxml.exe import-rtf input.rtf --out output.docx --wps-text --allow-unsigned-unicode --drop-nul
```

Стандартный режим не принимает эти три политики автоматически. CLI codes:
0 Clean, 1 Degraded, 2 error. Ошибки input/conversion/write сохраняют Failed
report, если доступен сам путь отчёта. Aggregate loss totals сохраняются после
ограничения подробного списка. Все 100 inputs данного корпуса — Degraded.

## Добавлено после текстового baseline

- Styles/defaults/metadata с bounded inheritance и проверками ссылок/циклов;
  lists/overrides/levels, отрицательные list IDs, templates и indents.
- Проверка table grids/merge областей без дублирования текста.
- Cached fields, headers/footers default/even/first, footnotes/endnotes и refs.
  Неявные/custom note markers получают Loss, а не обещание lossless.
- Page defaults и `sectd` reset подтверждены отдельными WPS probes. Свойства
  страницы внутри региона диагностируются и не сбрасывают геометрию body.
- WMF/EMF сохраняются побайтно с extent, budgets и container signature checks.
  Это не проверка всех drawing records и не реализация их отрисовки.
- Writer сохраняет DocDefaults и правильный schema order list levels.
  XSD gate теперь отклоняет также незарегистрированные violations.
- Обязательный PDF CIDSystemInfo проверяется независимым lopdf test.
- Исправлены bfchar counts, UTF-16 surrogate mappings, неполный самодельный
  Identity-H encoding и отсутствующий FontBBox. Unicode text extraction теперь
  проверяется lopdf, pypdf и pdfminer; отдельный PDF test проверяет non-BMP `𝑥`.
- Regular PostScript aliases ArialMT/TimesNewRomanPSMT сопоставляются с уже
  поддержанными Arial/Times New Roman. Метаданные оригинальных fonts подтверждают
  name IDs 1/6; их payload не копируется. Четыре bold/italic сочетания дают
  byte-identical PDF с canonical families и правильно извлекаемый текст:
  [font-alias.verification.json](font-alias.verification.json).

## Измерения

Полные 100 строк, input/output/oracle hashes, options, source snapshot и limits:
[acceptance.development.json](acceptance.development.json). Исходные RTF/PDF/media
не публикуются в документации. Raw measurements остаются под
`target/rtf-import/{public-v7,media-roundtrip-v5,visual-public-v7,pixels-public-v7}`.
После all-feature сборки повторён import тем же бинарником, что render:
все 100 DOCX hashes public-v7 совпали с public-v5. Поэтому schema gates
относятся к точным текущим пакетам. Render/pixel measurements повторены после
font/CMap fixes; предыдущие результаты сохранены как исторические snapshots.

| Проверка | Результат |
| --- | --- |
| Изолированный публичный CLI import / integrity | 100/100 |
| Точный body text с WPS facts v2, без нормализации | 100/100 |
| Writer + reader text/media roundtrip | 100/100; 38 media parts |
| Writer losses | 0; import losses учитываются отдельно |
| Strict XSD / OPC XSD | 100/100; 0 violations, 0 uncovered roots |
| Совпадение числа PDF страниц с WPS | 61/100; 39 failures |
| PDF render reports | 48 Clean, 52 Degraded |
| Pixel gate документов | 2 Pass, 98 Fail |
| Все страницы с совпавшей пагинацией | 244: 7 Pass, 237 Fail |

Import supervisor: 30 секунд и 512 MiB на файл. Windows final process counters
подтверждают peak RSS; максимум прогона 2.3655 секунды и 248.4375 MiB.
Это measurement одной машины/сборки, не общий performance SLA. Render supervisor:
90 секунд и 1 GiB. Pixel gate: 96 DPI, без resizing, штатный Rust comparator,
SSIM 0.96 и штатные structural bounds. Новый RTF corpus ещё не получил reviewed
class/profile. Разное число страниц не даёт предполагать соответствие страниц.

WPS exports получены из RTF независимо от импортёра. Facts v2 исправляют ошибку
прежнего oracle с tabs в pPr; raw references не переписывались. Восемь probes:
[structure.fixture-inputs.json](structure.fixture-inputs.json) и
[structure.wps-comparison.json](structure.wps-comparison.json). Проверяются
выбранные факты, а не весь DOCX. Grid/vertical merge и текст совпали, но стили
и paragraph properties различаются. WPS иначе трактует отрицательный list ID,
custom note mark и cached field с несколькими абзацами; это не основание
автоматически удалять данные источника.

Locked all-feature tests и clippy первоначальных семи изменённых crates проходят;
после новых font/CMap fixes дополнительно проходят полные SVG/PDF suites и clippy
обоих renderer. Независимые PDF contracts: 13 GREEN. Отдельно:
минимальная facade feature `rtf`, lifetime media и четыре pipeline tests.
Логи `final-tests-v5.log`, `final-clippy.log`, `minimal-rtf-v5.log`,
`pipeline-v5.log` под `target/rtf-import`. Python harness: 25 tests GREEN.
Negative XSD control с 72 незарегистрированными violations теперь возвращает
failure. Старый public-v1 с неверным list order не является acceptance evidence.

После aliases шесть документов больше не теряют текст из-за этих face names.
Документ 076 изменился с 14 на 15 страниц при эталоне 14: прежнее совпадение
получалось с неверными метриками. Поэтому общее количество pagination PASS
снизилось с 62 до 61. Это открытая layout задача, а не разрешённое исключение.

## Обязательные следующие приоритеты

1. Согласованное разрешение шрифтов для metrics/SVG/PDF embedding, glyph coverage,
   hashes и embedding rights. Сейчас `pdf.font.missing` означает отсутствие текста
   этих faces в PDF. Документ 002 с Futura/Garamond имеет ноль embedded faces.
   Feature `system-fonts` пока создаёт Builtin provider и проблему не решает.
2. WMF/EMF rendering с record/resource limits и независимыми fixtures.
3. Разбор 39 pagination failures и pixel failures через минимальные probes:
   styles, line metrics, tables, sections, regions, lists, bidi и drawings.
   Пороги не ослаблять под текущие outputs.
4. Конкретные mapping/rejection правила для custom marks, comments, revisions,
   textboxes, shapes/OLE, non-inline fields и остальных unsupported controls.
5. Reviewed support-profile expectations, release feature matrix, clean-tree
   receipt и current-SHA CI. Изменённый workflow не доказывает внешний CI
   или интеграцию с основной веткой.

Исторический план: [RTF_IMPORT_PLAN.md](../RTF_IMPORT_PLAN.md). NUL и девять
текстовых расхождений: [COMPATIBILITY_RESOLUTION.md](COMPATIBILITY_RESOLUTION.md).
