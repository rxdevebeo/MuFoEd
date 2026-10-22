# UI01 — 2026-10-20

Ручная палитра и слои: [UI01-VALIDATION.md](UI01-VALIDATION.md), 41 тест и снимки двух размеров native-окна. Сведения ниже относятся к предыдущим этапам.

# Проверки Image Studio — 2026-10-19

## Общий runtime — R02

Текст/чат/OCR MuFoEd подключены к тому же process-global runtime, что используется
графическими backend. Проверены владение моделью, подтверждение unload и recovery
при неизвестном результате HTTP, отдельные remote domains, 189 обычных тестов и
реальный переход Ollama → SDXL → Ollama; отдельно выполнена live formula recovery.
Подробные команды, результаты и ограничения: [R02_VALIDATION.md](../model-runtime/R02_VALIDATION.md).
Кэш резидентных моделей, offload и автоматический контроль VRAM остаются R03–R05.

## Общий runtime — R01

- PASS: `cargo +1.92.0 test -p mufoed-model-runtime -p image-editor`: 37 тестов
  (8 менеджер + 3 межадаптерных lifecycle + 26 прежних image-editor).
- PASS: Clippy `--all-targets -- -D warnings` обоих крейтов без ML features;
  image-editor также с `demo,plakat-cuda` и Plakat opt-level=2.
- PASS: реальный `model_tools_smoke` с одним host runtime: BiRefNet CPU 10.9 с,
  SAM CUDA 18.5 с. Перед каждым subprocess имитация текстового владельца удерживает
  admission; процесс ждёт. После завершения `active` и очередь пусты ещё до Drop job.
  Это проверка общей очереди, не реального Ollama.
- PASS: `native_smoke` с Plakat/CUDA через тот же контракт, 9.5 с inference
  (сборка отдельно). Ожидающая native-задача отменена под удерживаемым текстовым
  lease; её ticket и native guard освобождены. Следующая реальная генерация ждала
  разрешения, выполнилась и освободила lease. `artifacts/runtime-native-vase.png`.
- PASS: из исходного Cargo.lock не удалена/не заменена ни одна тройка
  package/version/source; добавлено 288 записей, включая новый локальный runtime.
  Исходники существующих крейтов StrictLib и MuFoEd не изменялись; коммитов нет.
- NOT_IMPLEMENTED: подключение текстовых/OCR путей MuFoEd, реальные замеры VRAM,
  resident-кэш, offload и OOM recovery. [План и приёмка R01–R05](../model-runtime/PLAN.md).

## Ранее проверенные графические возможности

| Проверка | Результат | Свидетельство |
|---|---|---|
| Unit + editing integration | PASS, 26 тестов | `cargo +1.92.0 test -p image-editor --features demo,plakat-cuda --all-targets --config 'profile.dev.package.plakat.opt-level=2'` |
| Строгий clippy | PASS | Те же features/all-targets, `-- -D warnings` |
| Civitai live catalog + cursor | PASS | `civitai_smoke`: 20 файлов версий на первой странице, 4 на следующей |
| Civitai сохранение файлов | PASS, изолированные fixtures | SHA256, временный файл, no-overwrite, отмена/повреждение, исключение pickle и файлов без SHA256; live download весов из Civitai не выполнялся |
| Локальный отдельный VAE | PASS | `prepare_asset.py`, `D:/Models/sdxl_vae.safetensors`; `.runtime/vae-result.json` |
| CUDA text-to-image: размер + LoRA + VAE | PASS, 21.4 с | `generation-lora-vae-wide.png`, 640×384; `add-detail-xl`, weight 0.65; отдельный VAE |
| CUDA inverted inpaint + LoRA + VAE | PASS, 25.0 с | `inpaint-lora-vae-inverted.png`; feather=8; каждый защищённый RGBA-пиксель проверен |
| CUDA генерация по изображению | PASS, 11.1 с | `native-img2img-vase.png`; штатный `plakat::api::Img2img`, strength=0.6 |
| BiRefNet ONNX General Tiny | PASS | Официальные веса загружены и граф проверен; `birefnet-vase-cutout.png`, CPU |
| SAM 2.1 Hiera Large | PASS | Официальные safetensors/config с фиксированными SHA256; `sam21-vase-mask.png`, CUDA, positive+negative points; отсутствующих весов image predictor нет |
| Rust subprocess bridge BiRefNet / SAM | PASS | `model_tools_smoke`: 9.5 с CPU / 28.3 с CUDA; `bridge-birefnet.png`, `bridge-sam21-mask.png`; исходный размер, RGB/alpha и непустая маска проверены |
| UltraSharp 4× CUDA | PASS | Локальные веса из D:/Models; `ultrasharp-vase-4x.png`, 640×384 → 2560×1536 |
| AuraSR v2 4× CUDA | PASS | Официальные safetensors, SHA256; `aurasr-v2-vase-4x.png`, 2560×1536, overlap batch=1 |
| Смесь 65% UltraSharp / 35% AuraSR v2 | PASS | `upscale-blend-65-35.png`; обе модели выполнены последовательно; UltraSharp — официальные скачанные safetensors |
| Alpha апскейлеров | PASS | Все три результата совпадают с Lanczos-resize исходного alpha; дополнительно `upscale_smoke` обработал настоящий прозрачный cutout BiRefNet через Rust bridge, 25.6 с, 2560×1536, `bridge-upscale-cutout-65-35.png` |
| Крейт без ML-features | PASS | `cargo +1.92.0 check -p image-editor --no-default-features --lib` |
| Семантика маски / прозрачного слоя / Undo | PASS | Тесты stale source, неизменности RGB, alpha 80→80/40/0, скрытия оригинала и Undo |
| Старые проекты / u64 seed / веса | PASS | serde-defaults, точное u64::MAX без float rounding, defaults 65/35, нормализация и запрет нулевой суммы, сохранение настроек при смене изображения |
| Три вкладки + ручная панель + диалоги | PASS для запуска и снимков | `ui-create.png`, `ui-img2img.png`, `ui-edit.png`, `ui-upscale.png`, `ui-model.png`, `ui-lora.png`, `ui-civitai.png`; exit=0, stderr=0 |
| Финальный запуск BAT / img2img | PASS | `bat-final-img2img.png`; корректные вкладки без переноса названия; кнопка загрузки источника и strength видимы |
| Границы изменений | PASS | Только `image-editor/`, member Cargo.toml и Cargo.lock. Ни одна исходная пара package/version/source из старого lockfile не удалена и не заменена; добавлено 287 пакетов для нового крейта |

Вспомогательные модели выполняются локальными Python worker, Plakat встроен в Rust. На этой машине Python 3.11, torch 2.8+cu128, transformers 4.56.2, ONNX Runtime 1.22; spandrel 0.4.2 / aura-sr 0.0.4 установлены только в `.runtime/python-packages`. BiRefNet General/Matting и GPU ONNX provider отдельно не проверялись; проверен General Tiny на CPU. Другие checkpoint/LoRA и SD1.5 не считаются автоматически принятыми. SAM здесь проверен для одиночного изображения, не видео.

Полный сценарий кликов мышью → inference → accept → сохранение в живом окне не автоматизирован целиком; отдельно проверены тот же runtime bridge, операции принятия/Undo и реальные снимки всех вкладок. Качество выделения волос/сложных рисунков, швы inpaint/upscale и длительная работа под ограничением VRAM требуют визуальной приёмки. Resident model cache остаётся в [TODO.md](TODO.md).

## Ранее проверенный первый срез

Worktree: `C:/Users/gamer/.codex/worktrees/image-studio/StrictLib-mufoed`, ветка `codex/image-studio`, база `9ff1e83ed1f962809b09a42c6144342417c7deac`. Коммиты не создавались.

| Проверка | Результат | Свидетельство |
|---|---|---|
| Поведение ядра / проекта / маски / аргументов | PASS, 13 тестов | `cargo +1.92.0 test -p image-editor --all-targets --features demo` |
| Строгий clippy ручного и библиотечного backend | PASS | `cargo +1.92.0 clippy -p image-editor --all-targets --features demo,plakat -- -D warnings` |
| Строгий clippy встроенного CUDA backend | PASS | `cargo +1.92.0 clippy -p image-editor --all-targets --features demo,plakat-cuda -- -D warnings` |
| CLI Plakat CUDA build | PASS | Plakat 7.9.0, `ce0d67a45bc9b5f742c3772aef85afe0d15890f6`, release |
| Локальная SDXL-конвертация | PASS | `.runtime/hub/models--local--image-studio-sdxl/snapshots/195eed7a7b1d493afdd8d66017a117ab5300b150/receipt.json`; исходники весов не изменялись, `local_files_only=True` |
| Настоящий CLI text-to-image | PASS, 17.7 с | `artifacts/generated-vase.png` |
| Настоящий CLI inpaint | PASS, 9.4 с | `artifacts/inpainted-vase.png`; проверен каждый защищённый RGBA-пиксель |
| Встроенный Plakat CUDA build | PASS | `demo,plakat-cuda`; `plakat::api`, без запуска `plakat.exe` |
| Настоящий встроенный text-to-image | PASS, 8.9 с | `artifacts/native-generated-vase.png`, example `native_smoke` |
| Настоящий встроенный inpaint | PASS, 12.5 с | `artifacts/native-inpainted-vase.png`; проверен каждый защищённый RGBA-пиксель |
| Запуск окна / screenshot | PASS для финального встроенного CUDA-демо | `artifacts/editor-mufoed-style.png`; stderr пуст; отдельные Generation/Editing, размеры элементов по образцу Invoke, палитра MuFoEd, реальный PNG на холсте |

Оборудование: RTX 5070 Ti 16 GB, CUDA 12.9, driver 616.92, MSVC 2022 14.44. Локальные веса: `D:/Models/dvine_v60.safetensors`, VAE `D:/Models/sdxl_vae.safetensors`. Все времена включают загрузку модели в отдельном запуске и не являются сравнительным бенчмарком: условия кэша различаются.

На CUDA обнаружена upstream-ошибка `no cuda implementation for metal-sdpa`. Применён штатный переключатель `PLAKAT_NO_SDPA=1`, исходники Plakat не менялись.

Inpaint проверен функционально: корректный размер и побитовая сохранность пикселей вне маски. Визуальное качество замены и стыка на границе маски не принято как production-ready; текущая маска имеет жёсткий край. Полный UI-сценарий рисования мышью → генерация → accept → сохранение не автоматизирован целиком. Отмена активного native inference и долговременная работа под ограничением VRAM требуют отдельной приёмки. [TODO по кэшу](TODO.md).

Изменения: только новый `image-editor/`, запись member в `Cargo.toml` и зависимости в `Cargo.lock`. Исходники существующих библиотечных крейтов StrictLib и MuFoEd не менялись.

## R03–R05

Актуальная приёмка shared memory admission, resident cache и recovery: [R345_VALIDATION.md](../model-runtime/R345_VALIDATION.md). Предыдущие разделы — исторические проверки ранних адаптеров.

## PSD

PSD стал форматом сохранения в демо; JSON остаётся legacy. [Профиль, проверки и ограничения](PSD-VALIDATION.md).

## Постобработка и диалоги

[POSTPROCESS-VALIDATION.md](POSTPROCESS-VALIDATION.md): цепочка, UltraSharp 1×, GPU и центрирование окон.
