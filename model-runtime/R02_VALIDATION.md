# R02 — общий lifecycle MuFoEd/Ollama

Базовый Image Studio и R01 сохранены коммитом `10990e4c` в `codex/image-studio`.
Изменения R02 подготовлены отдельно, без нового коммита. Библиотечные исходники
StrictLib и исходные package/version/source в Cargo.lock не менялись.

## Реализовано

- `model-runtime` feature `ollama`: общий координатор локального endpoint,
  отдельные очереди remote endpoint, реестр операций endpoint/model/id и проверка digest,
  защита от принятия уже резидентной сторонней модели, включая её alias.
- Text AI, инструментальный чат, PDF/DjVu text recovery/figure classification и
  распознавание объектов/формул используют прикладной wrapper MuFoEd.
  `OllamaVision`/`TextRecovery` StrictLib не изменялись; его промпты, parsing и
  typed errors сохранены. Текст/чат передают отмену ожидающей задачи.
- Выгрузка выбранной модели: `/api/generate` с `keep_alive: 0`, подтверждение
  `done_reason: unload`, затем отсутствие имени/digest в `/api/ps`.
  Пустого `/api/ps` без unload acknowledgement недостаточно.
- Неподтверждённая выгрузка или panic оставляет lease у recovery worker,
  повторяющего ограниченную проверку. Чужие модели массово не выгружаются.
- В MuFoEd и Image Studio отображаются execution/releasing/unknown состояния.

## Проверки

| Проверка | Результат | Свидетельство |
|---|---|---|
| Общий runtime с Ollama | PASS, 17 тестов | FIFO, бюджеты, отмена очереди, конкурентные worker; подтверждённый unload, alias/ownership, broken HTTP, callback error/panic, неизвестная выгрузка, remote domains |
| Image Editor | PASS, 29 тестов | `cargo +1.92.0 test -p mufoed-model-runtime -p image-editor --features mufoed-model-runtime/ollama` |
| MuFoEd regression | PASS, 143; 6 ignored | `cargo +1.92.0 test -p mufoed --bin mufoed`; настоящие TCP-фикстуры проверяют текст/tool/image payload и полный metadata/inference/unload lifecycle |
| Без OCR | PASS | `cargo +1.92.0 check -p mufoed --no-default-features --all-targets`; существующие dead_code warnings сохранены |
| Реальный text → SDXL → text | PASS, два прогона | `native_smoke`, один Runtime, Ollama `qwen3.5:4b` → Plakat/CUDA `local/image-studio-sdxl` → та же Ollama; первый 29.0 → 10.9 → 4.3 с; финальные исходники 4.4 → 11.5 → 4.3 с. Text ответы READY, unload подтверждён; `image-editor/artifacts/r02-mixed-models.png`. Веса загружались заново, условия файлового кэша различались |
| Реальный ManagedVision OCR | PASS для lifecycle/структуры | `raster::live::a_drawn_formula_comes_back_as_an_equation -- --ignored --nocapture`, 15.10 с; ответ преобразован в OMML с `m:f`; выгрузка подтверждена |
| Clippy общего runtime | PASS, strict | `cargo +1.92.0 clippy -p mufoed-model-runtime --features ollama --all-targets -- -D warnings` |
| Clippy CUDA/UI/общего runtime | PASS, strict | `cargo +1.92.0 clippy -p mufoed-model-runtime -p image-editor --features demo,plakat-cuda,ollama --all-targets --config 'profile.dev.package.plakat.opt-level=2' -- -D warnings` |
| Полный strict Clippy MuFoEd | FAIL вне новых строк | В исходном состоянии есть lint-долг, в том числе `needless_return`, `map_unwrap_or`, `needless_pass_by_value`, `default_trait_access`; JSON-проверка первичных spans против изменённых строк не нашла замечаний на новых строках R02. Лог `image-editor/artifacts/r02-mufoed-clippy.jsonl` |

Два старых тестовых `PlacedPage` в `mufoed/src/tab.rs` дополнены `off_page: Vec::new()`
по существующему контракту библиотеки: без этого baseline test target не собирался.
Library API ради тестов не менялся. Исправления постороннего lint-долга не входят в R02.

## Ограничения

Это проверка координации и подтверждения освобождения, а не бюджетирования VRAM
или качества LLM/OCR. В live OCR модель прочитала исходный латинский `x` как `χ`:
тест проверяет структуру дроби и pipeline, не точность всех символов.
Импорт целого PDF/DjVu с реальной OCR-моделью отдельно не прогонялся; соответствующие
callbacks обёрнуты тем же ManagedVision, его production-методы компилируются и
проверены через formula/region HTTP fixture и живую formula recovery.

Кэш моделей между запросами, component offload, реальные пики памяти и OOM retry
остаются R03–R05. Отмена активного HTTP пока ждёт завершения/cleanup. Текущие OCR
API не имеют отдельного флага отмены UI-запроса; очередь ограничена 900 с.
Серверное владение невозможно гарантировать против другого клиента, начинающего
загрузку после preflight. Другие OS-процессы требуют выделенного endpoint/broker.
Тестирование отказов серверов выполнялось через mocks, сам установленный Ollama
не завершался и не перезапускался принудительно.

Следующий пользовательский этап — [UI01: палитра ручных инструментов и слои](PLAN.md#ui01-ручные-инструменты-и-менеджер-слоёв).
