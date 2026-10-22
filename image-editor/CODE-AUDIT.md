# Повторный аудит Image Studio — 2026-10-22

Рабочее дерево: `codex/image-studio`, base commit `43b36138284e74f4a554ca2cb45f5318facfecd3`.
Область: только `image-editor/`; StrictLib, MuFoEd и общий `model-runtime` не изменены. Предыдущие незакоммиченные UI02/Escape-исправления сохранены. Коммит не создавался.

## Метод и границы

Инвентаризация всех 59 Rust-файлов `src/` и 14 Python-скриптов, проверка их статическими средствами; углублённое чтение прикладных контрактов и переходов состояний в UI/документе, слоёв и масок, clipboard/Undo, импорта/PSD/recovery, настроек/пресетов, inference/pipeline и загрузки ресурсов. Существующие тесты численных контрактов UNet/sampler/tokenizer/VAE исполняются в production-конфигурации. Это не построчное доказательство корректности всех заимствованных математических реализаций и не проверка каждой модели на реальном GPU.

Граф MCP относится к `D:/projects/StrictLib-mufoed`, generation `2026-10-19T19:03:34Z`; image-editor в C:/worktree отсутствует в индексе (`search_graph: 0`, coverage freshness=missing). Поэтому выводы опираются на прямой исходный код и выполненные проверки. Индекс соседнего рабочего дерева не выдаётся за покрытие этого редактора. SHA256/размеры исходников: `artifacts/audit-source-inventory.json`.

## Найдено и исправлено

| ID | Приоритет | Ошибка и результат исправления | Основная проверка |
| --- | --- | --- | --- |
| A01 | P1 | Быстрые фильтры и общая цветокоррекция меняли весь слой/маску, игнорируя выделение. Теперь учитываются мягкие края, alpha и скрытый RGB сохраняются. Цветовой preview строится тем же путём и совпадает с Apply. | `editing_audit.rs`, `color_preview_matches_commit_through_soft_selection_for_raster_and_mask` |
| A02 | P1 | SAM-cutout/BiRefNet теряли blend/mask и помещали результат на вершину вместо позиции исходника. Новый слой теперь стоит сразу над скрытым оригиналом и сохраняет opacity, blend и bitmap-mask. Ошибки лимита/блокировки не меняют стек. | `cutout_preserves_stack_position_blending_and_layer_mask`, `birefnet_keeps_source_mask_blend_and_stack_position`, `failed_cutout_at_layer_limit_is_atomic_even_without_ui_rollback` |
| A03 | P1 | SAM писал AI-mask, но оставлял редактор на layer-mask. Теперь успешное принятие выбирает именно маску ИИ; отклонённый устаревший результат не переключает инструмент/режим. | `sam_acceptance_switches_from_layer_mask_to_the_new_ai_mask`, `stale_sam_result_does_not_change_mode_or_mask_destination` |
| A04 | P1 | Команда во время удерживаемого мазка могла смешать Undo-снимки, после команды продолжалось рисование. Общая граница жеста завершает мазок отдельно, сбрасывает лассо/геометрический drag и ждёт отпускания указателя. Тот же путь у Copy/Paste и открытия окон. | `command_during_stroke_stays_separate_in_history_and_cannot_resume_the_drag` |
| A05 | P1 | Окна коррекции/холста/настроек/пресетов не владели клавиатурой: Delete и другие команды могли менять холст за окном. Добавлен общий запрет клавиатурных действий холста и рисования; Esc сначала отменяет окно. Inline-transform сохраняет собственные команды; Esc/Enter не перехватываются фоновой трансформацией под окном настроек. | `floating_dialogs_block_canvas_delete_deselect_and_drawing_and_own_escape`, `settings_owns_escape_before_a_background_inline_transform`, прежние tone-modal и inline-transform tests |
| A06 | P2 | После Hand постановка SAM-точек не работала; смена режима генерации через API/пресет могла оставлять canvas в режиме точек. Вход в SAM завершает старый жест и выходит из Hand/Zoom, source-mode освобождает ввод. Кнопка кисти SAM явно выбирает кисть маски ИИ. | `sam_points_exit_hand_and_source_modes_release_canvas_input`, существующие canvas/navigation tests |
| A07 | P2 | Ручной seed до запуска не попадал в preferences; признак random seed не сохранялся. Теперь сохраняются введённый u64 и режим random/fixed, старые настройки читаются с прежним random=true. | `fixed_seed_and_random_mode_roundtrip_with_legacy_default`, preferences roundtrip |
| A08 | P2 | При apply preset нормализовался старый request, затем поверх него присваивался новый — возвращался несовместимый Hires для img2img/inpaint. Теперь нормализуется загруженный request, также при apply_settings. | `source_mode_preset_cannot_restore_incompatible_hires`, hires/preset roundtrip |
| A09 | P2 | Автоматический VAE HF-комплекта оставался после смены checkpoint. Учтён источник VAE: автоматический следует за выбранной моделью, явный VAE/пресет остаётся закреплённым. Все пути выбора, включая identifier/preparation, общие; ошибка подготовки не меняет VAE заранее. | `switching_checkpoint_drops_only_the_previous_automatic_bundle_vae`, settings/preset suites |
| A10 | P2 | «Показывать маску ИИ» на ленте меняло флаг, но могло продолжать показывать layer-mask. Теперь используется тот же явный переход к AI-mask, что и в inpaint-панели. | существующие layer-mask destination/context tests и полный UI suite |

A01/A02/A03/A04 воспроизведены падающими тестами до исправления: `artifacts/audit-baseline-corrections.log`, `audit-baseline-sam.log`, `audit-baseline-gesture.log`. В исходном падении A04 выводится полный Document; для чтения достаточно assertion/названия теста.

## Проверенные контракты

- Рисование/выделение/лассо/палочка/цвет/ретушь: координаты холста после zoom/pan, Raster/Mask, блокировки, мягкое выделение, отпускание кнопки, Esc, один Undo на действие.
- Слои: переключение/видимость, reorder, masks, blend, merge и ограничения безопасного сведения. Готовое выделение остаётся областью холста; незавершённые жесты сбрасываются.
- Clipboard: current/new layer, alpha, mask destination, плавающая вставка и её отмена, защита locked-layer. Системный clipboard в unit tests заменён внутренним; native smoke не доказывает обмен с внешними приложениями.
- Геометрия/цвет: preview/commit, небольшие углы, canvas sizing/crop, layers/masks, no-op/error/lock, Undo.
- Файлы: PSD RGB8 roundtrip и внешние fixtures, честный отказ для неподдерживаемых PSD-конструкций, атомарный project/settings/recovery write, stale-result checks, отсутствие изменения исходного файла при recovery.
- Модели: Request validation, очередь/leases через существующий общий runtime, отмена, управляемый worker и его завершение, staged pipeline; tokenizer/sampler/attention/VAE contract tests. Общий менеджер не заменён локальным.
- Загрузки: pinned HF revision, ограниченный stream/range, хеши, публикация после проверки, отмена и отсутствие credentials в проекте. Live HTTP и реальные веса в этом проходе не скачивались.

## Неоднозначности, требующие отдельного изменения контракта

Ниже сохранены исходные ограничения на момент аудита. **U01–U05 устранены последующим изменением**; актуальные контракты, тесты и GPU-приёмка: [STANDARD-TOOLS-VALIDATION.md](STANDARD-TOOLS-VALIDATION.md). Исторические результаты этого раздела не относятся к новой сборке.

1. **U01: маска при вставке.** `Document::paste_pixels` для mask-fragment заменяет всю целевую маску, очищая её вне фрагмента. Это нынешний явный API-контракт; по аналогии с растром пользователь может ожидать вставку только в область. Нужны отдельные команды «заменить маску» / «вставить в область» и правила мягкого совмещения.
2. **U02: непрозрачность кисти.** `BrushStyle` задаёт per-segment strength: перекрывающиеся сегменты накапливают покрытие. Это ближе к Flow; верхний предел opacity на весь мазок не реализован. Нельзя обещать независимость результата от частоты событий/кадров.
3. **U03: Ctrl+T после вставки в текущий слой.** Плавающий фрагмент закрепляется, затем трансформируется весь активный слой. Для вставки новым слоем это ожидаемо; для текущего слоя нужна отдельная трансформация фрагмента до объединения.
4. **U04: сохранённая точка истории.** Dirty-флаг консервативен: Undo к точному сохранённому состоянию не обязан снять предупреждение о несохранённых изменениях. Нужна savepoint-семантика вместо одного boolean; автосохранение/закрытие должны использовать тот же контракт.

5. **U05 / P1: источник SDXL VAE в локальной конверсии.** `scripts/prepare_asset.py` при первой конверсии сохраняет `pipe.vae` в общий cache namespace `models--madebyollin--sdxl-vae-fp16-fix`, только если `refs/main` ещё нет. `generation/managed_sd.rs::Files::resolve` на GPU выбирает этот общий namespace, когда Request.vae отсутствует. Значит, default VAE может фактически происходить из первого сконвертированного checkpoint, а не из модели, названной в namespace; порядок конверсии влияет на дальнейшее поведение. Это подтверждённый исходным кодом риск, оставленный открытым: необходимо явно разделить VAE checkpoint и pinned fp16-fix, предусмотреть проверку старого кэша и провести аппаратную A/B-проверку. Автоматическая миграция существующих пользовательских весов и изменение inference-default этим UI-исправлением не выполнялись. A09 закрывает перенос выбранного HF-bundle VAE между checkpoint, а не U05.

Поддержка PSD по-прежнему ограничена RGB8 raster + plain bitmap masks + Normal/Multiply/Screen/Overlay; groups/ICC/smart objects/off-canvas extents требуют отдельной реализации и приёмки. Это не выдаётся за полный Photoshop interchange.

## Валидация

- CPU demo all targets: **207 passed, 1 ignored, 0 failed**. Production demo,plakat-cudnn,ollama all targets: **225 passed, 2 ignored, 0 failed**.
- Строгий Clippy `-D warnings` обоих вариантов: PASS. fmt и git diff --check: PASS.
- Python: синтаксис всех 14 скриптов проверен через ast.parse; 5 unittest апскейла/timing проходят. Upscale-тест использует синтетический nearest4x model, не веса UltraSharp/AuraSR.
- Release demo,plakat-cudnn: PASS. Четыре native запуска финального EXE с PSD: color/canvas/settings/presets, exit0; PNG просмотрены. Изолированные settings/recovery находятся в artifacts/audit-*. EXE SHA256 `924609271ff891fcb522d86f869f227c91e8327315bfd5ee755dfa8a5448d34f`.
- Добавлены 15 новых regression tests (2 integration, 13 unit); исходные тесты сохранены.
- Результаты: artifacts/audit-cpu-clippy.log, audit-cpu-tests.log, audit-production-clippy.log, audit-production-tests.log, audit-python-tests.log, audit-release.log, audit-*-launch.log, audit-native-results.json. Полный source/EXE receipt: artifacts/audit-release-receipt.json.
- Ignored live HF range/CUDA VAE probe не выдаются за выполненные. Актуальный запуск: `image-editor/start.bat -Release`.

Новый аппаратный прогон SDXL/SD1.5 + LoRA + SAM/BiRefNet/AuraSR не выполнялся. Production feature tests и успешный native запуск не означают проверки качества/VRAM/скорости всех реальных моделей. Исторические R03–R05/GPU-результаты не используются как свидетельство этого изменённого дерева.
