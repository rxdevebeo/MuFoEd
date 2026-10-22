# E03.2 — маски слоёв и режимы наложения

База: `13c694ab`, ветка `codex/image-studio`, отдельный worktree image-studio. Коммит E03.1 создан по запросу пользователя; E03.2 подготовлен отдельно. Изменения только в image-editor. StrictLib, mufoed и общий model-runtime не изменяются.

## Поведение

- Отдельная grayscale-маска каждого слоя, enabled/default0/255. Растровые пиксели не удаляются. Создание белой/чёрной маски, из ручного выделения/маски ИИ, отключение, инверсия, применение в alpha, удаление. Создание/копирование/resize учитывают маски в общем лимите128MiB/16слоёв; все ошибки отклоняются до публикации результата.
- Единый переключатель Растр/Маска. Внутри режима Маска выбирается назначение «Маска ИИ» / «Маска слоя». Миниатюры в доке, синяя рамка назначения и красная диагональ отключённой маски. Белый показывает, чёрный скрывает. Общие кисти/фигуры/заливка/ретушь, пипетка/палочка, коррекция/уровни/кривые и буфер используют выбранную плоскость. Блокировка защищает растр и его маску; общая маска ИИ остаётся независимой.
- Copy/Cut/Delete и вставка в маску не меняют растр и маску ИИ. После вставки Рука перемещает покрытие без создания слоя; один Undo возвращает состояние до вставки и размещения. Выделение остаётся самостоятельной плоскостью. Смена назначения/стека, Undo/Redo, очистка холста сбрасывают устаревшие индексы и жесты. Явные команды показа маски ИИ переключают отображение на неё.
- Растр и его маска связаны при перемещении, зеркале, повороте и масштабе. Общие canvas-операции преобразуют все маски. В режиме маски преобразуется только выбранное покрытие. Вне исходных границ сохраняется default; белая маска не создаёт повторного затемнения полупрозрачного края при повороте.
- Normal/Multiply/Screen/Overlay используют один straight-sRGB8 композитор для холста, до/после, PSD merged и PNG. Реализована alpha-aware формула source-over с blend function, Overlay выбирает ветвь по backdrop. [W3C compositing](https://www.w3.org/TR/compositing-1/#blending).
- Для слоёв с масками/режимами transform preview выполняется на CPU-копии и кэшируется по параметрам; исходник до Apply неизменен. Обычный Normal без масок сохраняет прежний GPU-предпросмотр. Скорость интерактивной трансформации больших документов отдельно не измерялась; GPU-ускорение этого нового пути не заявляется.
- Объединение нижней пары запекает режимы/маски; для пары над другим фоном нужны два Normal, иначе команда отключена, чтобы не изменить зависимое от фона изображение. Применение маски округляет alpha до8bit, погрешность последующего сведения может составить1/255.

## PSD-профиль

- Стандартные blend keys `norm`, `mul `, `scrn`, `over`; растровая маска — channel -2 и обычная20-byte mask record. Enabled/default и grayscale передаются стандартными полями. [Adobe PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/).
- RGB8/PSD v1, растровые слои внутри canvas, RAW/RLE/ZIP. Прямоугольные маски внутри canvas нормализуются в full-canvas coverage, default0/255 заполняет внешнюю часть. Учитываются relative bounds. PSD merged имеет тот же композитор, что PNG.
- Не поддерживаются vector/real/filter masks, obsolete invert flag, feather/density, off-canvas bounds, группы, clipping, adjustment/text/effect/smartobject layers, остальные blend modes, ICC/CMYK/16/32bit/PSB. Raw preflight отвергает неизвестные ключи, неподдерживаемые flags/каналы и дубликаты; они не превращаются в Normal и не теряются молча.
- Приватный MuFoEd.Project по-прежнему хранит маску ИИ, ручное выделение, recipes/active/locks. Layer-mask и blend входят в checksum, поэтому внешнее изменение не принимает устаревший служебный блок. Маски видимости не зависят от приватного блока.
- Legacy JSON: версия2 только при наличии layer mask/не-Normal. Старые обычные проекты остаются версией1. Более старый редактор явно отвергнет версию2, а не проигнорирует новые свойства.

## Воспроизведение

```powershell
cargo +1.92.0 clippy -p image-editor --all-targets --features demo -- -D warnings
cargo +1.92.0 test -p image-editor --all-targets --features demo
. ./image-editor/scripts/enter-cuda.ps1
. ./image-editor/scripts/enter-cudnn.ps1
cargo +1.92.0 clippy -p image-editor --all-targets --features demo,plakat-cudnn,ollama --config image-editor/cargo-performance.toml -- -D warnings
cargo +1.92.0 test -p image-editor --all-targets --features demo,plakat-cudnn,ollama --config image-editor/cargo-performance.toml
cargo +1.92.0 run -p image-editor --example layer_psd_smoke -- image-editor/artifacts/e032-psd image-editor/artifacts/sd15-release-1536.png
$env:PYTHONPATH=(Join-Path $PWD 'image-editor/.runtime/psd-qa')
python image-editor/scripts/validate_layer_psd.py --check image-editor/tests/fixtures/psd --outputs image-editor/artifacts/e032-psd
```

Для независимой проверки нужен psd-tools1.25/Pillow/numpy; это optional QA, без зависимости runtime. Шесть checked-in PSD/PNG fixtures и tests/psd_layer_masks.rs работают без Python. `--fixtures image-editor/tests/fixtures/psd` воспроизводит их сторонним writer/compositor. Фото для native-примера необязательно; без аргумента PNG пример выполняет только шесть маленьких PSD-кейсов.

Граф основного D:/checkout не покрывает image-studio: generation2026-10-19T19:03:34Z, проверенные пути freshness=missing. Применены прямое чтение исходников, тесты и независимый PSD oracle. Артефакты/логи игнорируются Git. Финальные результаты и SHA находятся в разделе ниже и artifacts/e032-release-receipt.json.


## Результаты приёмки

- CPU `demo`, all targets: **171 passed, 1 ignored, 0 failed**. Strict Clippy `-D warnings`: PASS.
- Production `demo,plakat-cudnn,ollama`, all targets: **189 passed, 2 ignored, 0 failed**. Strict Clippy `-D warnings`: PASS. Два ignored — специальный CUDA VAE probe и live HTTPS Hub range; не выдаются за выполненные.
- Девять backend-сценариев layer_compositing: формулы и прозрачность, independent planes/lock/Undo, атомарный отказ, связанные transforms/default, версии JSON и PSD, безопасный merge, memory admission. Два psd_layer_masks: шесть независимых fixtures с точными raw/mask и max1/255 composite, отказ неизвестных blend/mask flags. UI: shared stroke/commands, Copy/Cut/paste/Hand/one Undo, exact transform preview=Apply=PSD, сброс устаревших индексов при очистке/retain и явный AI overlay control.
- Independent PSD oracle: все шесть re-export/roundtrip кейсов PASS. Стандартные поля/порядок/имена/opacity/disabled/default и raw RGBA/mask точны; merged PSD совпадает с нашим PNG точно. Повторная независимая композиция экспортированного PSD совпадает с исходным fixture, с нашим результатом расходится максимум на1/255 из-за округления. Это проверка psd-tools, ручная проверка Photoshop не проводилась.
- Release `demo,plakat-cudnn`: PASS. Три изолированных native запуска финального EXE, exit0: [маска1440×900](artifacts/e032-mask-1440.png), [маска1100×700](artifacts/e032-mask-1100.png), [трансформация1440×900](artifacts/e032-transform-1440.png). Снимки просмотрены: назначения/миниатюры/режимы доступны, док не переполнен, диалог центрирован. Настройки изолированы от пользовательских.
- EXE SHA256: `5a85f6ba385bdb8bbefca2cb16a77f5e5a0b5b02daf1e316bde26261b07278f8`. Source snapshot, базовый SHA и параметры: artifacts/e032-release-receipt.json. Запуск: `image-editor/start.bat -Release`.
- Логи: artifacts/e032-cpu-tests.log, e032-cpu-clippy.log, e032-production-tests.log, e032-production-clippy.log, e032-release.log, e032-oracle-export.log, e032-independent-psd.log и e032-*-launch.log. Генерация изображений/работа с весами в этом этапе не менялась и заново не запускалась.

E03 завершён. E04 (атомарное автовосстановление отдельной копии) остаётся следующим этапом; группы и текст не входят в эту приёмку.
