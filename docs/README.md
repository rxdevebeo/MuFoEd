# Документация

Здесь лежат действующие документы. Как собрать и проверить проект, описано в
[README](../README.md) в корне.

## Задание и решения

- [TZ-STRICT-OOXML-RUST.md](TZ-STRICT-OOXML-RUST.md) — техническое задание (расширенная
  редакция 2.1).
- [adr/](adr/README.md) — принятые архитектурные решения (ADR-0001…0019).

## Гейты и качество

- [GATE-STRATEGY.md](GATE-STRATEGY.md) — что проверяют гейты и как выдаётся исключение;
  сами исключения — в [waivers.toml](waivers.toml) (его читает тест).
- [CORE-QUEUE.md](CORE-QUEUE.md) — очередь ядра и критерии приёмки;
  [STAGE-8-OPEN.md](STAGE-8-OPEN.md) — реестр открытых пунктов, на который она опирается.
- [WORDCRAFT_ADOPTION_2026-10-07.md](WORDCRAFT_ADOPTION_2026-10-07.md) — план never-crash.
- [fuzz-protocol.md](fuzz-protocol.md) — условия и шаблон записи fuzz-прогонов.
- [WPS-QA-DISCREPANCIES.md](WPS-QA-DISCREPANCIES.md) — живой реестр расхождений рендера
  с эталоном WPS.
- [PARSE-RENDER-GAPS-2026-10-11.md](PARSE-RENDER-GAPS-2026-10-11.md) — что теряется между
  парсером и рендерером; [VML-UNSUPPORTED-SHAPES.md](VML-UNSUPPORTED-SHAPES.md) —
  неподдерживаемые фигуры VML.
- [GRAPHICS-VIEWER-2026-10-10.md](GRAPHICS-VIEWER-2026-10-10.md) — графика и визуальный
  просмотрщик.

## Корпус

- [CC0_CORPUS_MIGRATION_PLAN.md](CC0_CORPUS_MIGRATION_PLAN.md) — тестовый корпус CC0 и его
  lock-файл (`testdata-lock/cc0.toml`).
- [corpus-task-strict-docx.md](corpus-task-strict-docx.md) — корпус подлинных
  Strict-документов; [corpus-incoming.md](corpus-incoming.md) — документы, ожидающие разбора.

## Импорт

- PDF: [PDF-IMPORT-COMPARISON-2026-10-19.md](PDF-IMPORT-COMPARISON-2026-10-19.md) (сравнение с
  pdf2docx по стадиям конвейера), [PDF-VECTOR-GRAPHICS-2026-10-19.md](PDF-VECTOR-GRAPHICS-2026-10-19.md)
  (векторная графика), [PDF-CORPUS-RUN-2026-10-19.md](PDF-CORPUS-RUN-2026-10-19.md) (прогон по
  корпусу).
- DjVu: [DJVU_IMPORT.md](DJVU_IMPORT.md); оракулы и манифесты проверки — в
  [djvu-validation/](djvu-validation/README.md) (их читает CI).
- RTF: [rtf/ACCEPTANCE.md](rtf/ACCEPTANCE.md) и остальное в [rtf/](rtf/) (данные читают
  скрипты `xtool/rtf`).

## Редактирование и MuFoEd

- [EDITING_PLAN.md](EDITING_PLAN.md) — архитектура и приёмка слоя редактирования;
  [EDITING_REPORT.md](EDITING_REPORT.md) — что реализовано;
  [EDITING_OPERATIONS.md](EDITING_OPERATIONS.md) — поиск, замена, перемещение.
- [EDITING_ROADMAP_2026-10-08.md](EDITING_ROADMAP_2026-10-08.md) и
  [EDITING_KERNEL_EXTENSION_PLAN.md](EDITING_KERNEL_EXTENSION_PLAN.md) — планы развития.
- [mufoed-ai-chat-plan.md](mufoed-ai-chat-plan.md) — чат с ИИ в MuFoEd.

## Прочее

- [ATTRIBUTION-vendor-hayro.md](ATTRIBUTION-vendor-hayro.md) — происхождение и лицензия
  вендоренного hayro.
- [gate-data/](gate-data/) — эталоны WPS и ledger для `xtool/wps-gate`.
- [history/](history/) — завершённая история: задания, приёмки и доработки этапов
  (`STAGE-*`), отчёты этапов (`stage-*-report.md`), закрытые аудиты и журналы. На эти файлы
  по имени ссылаются комментарии в коде. Разовые отчёты, журналы сессий и материалы аудитов
  без ссылок удалены 2026-10-20; они остались в истории git.
