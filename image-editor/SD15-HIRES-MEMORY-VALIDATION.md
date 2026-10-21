# SD1.5 Hires: исправление OverBudget при первом проходе512²

Сообщение: VRAM requested27380416512 / available11548701492B. Проверен локальный импорт D:/Models/uberRealisticPornMerge_v23Final.safetensors,2132648310B: family sd15, repo local/image-studio-sd15-66016f92d675f0d7, revision66016f92d675f0d79dd34f7e31518f6618802186. Определение архитектуры было верным.

Сохранённые настройки содержали первый проход512² и включённый Hires Fix1536². Ошибка относилась ко второму Img2Img-проходу. Старый расчёт:3GiB +1536²×1024 +36864²×2×8 =27380416512B, точное совпадение с сообщением. Две головы не ограничивали квадратичную матрицу по пространственным tokens SD1.5.

Исправление: при >16384 query tokens обрабатываются блоки1024 строк; каждая строка видит все keys и полный softmax. Head slice2 и последовательный CFG включаются для SD1.x >512², для SDXL прежний порог >1024² сохранён. Неполные группы heads/rows сохраняются. Это не приближение, не crop и не уменьшение размера. Порог и dtype VAE сохранены.

Native admission использует ту же политику и оценивает1536² в6241124352B (5.8125GiB),512² с пакетным CFG —5.25GiB,1024² —8GiB. Оценка512 увеличена после наблюдения фактического UNet-прироста около4.78GiB, вместо прежних3.75GiB. Защитный резерв и общий runtime не менялись. Ошибка admission теперь содержит фактические размеры прохода и разрешённое семейство.

## Проверки

- Strict Clippy all-targets demo,plakat-cudnn,ollama -Dwarnings: PASS.
- Rust all-targets:117 passed,0 failed,2 explicit probes ignored. Новые численные проверки query chunks сравнивают полный dense softmax на self/cross, CFG batch1/2, неполных heads и rows; отдельно проверяется автоматическая граница16385 tokens. Регрессия admission воспроизводит прежнее число27380416512B и проверяет512/768/1024/1536. Существующая независимая проверка с исходным Plakat сохранена.
- Release demo,plakat-cudnn: сборка PASS, GUI launch exit0 и снимок sd15-release-launch.png. Настройки пользователя не переписывались: для GUI-проверки задан отдельный IMAGE_STUDIO_SETTINGS.
- На том же checkpoint с нейтральным prompt о синей керамической вазе, seed42, DPM++2M Karras, CFG7.5/7.5, denoise0.3:

| Цикл | Шаги1/расписание2 | Фактических UNet steps | Весь цикл,с | min free VRAM,GiB | max наблюдаемый прирост UNet,GiB |
|---|---:|---:|---:|---:|---:|
|512→768|2/4|3|15.476|9.839|4.752|
|512→1024|2/4|3|18.607|6.808|7.750|
|512→1536|50/30|59|112.548|9.433|5.125|

Все три проверки — полный PipelineJob с UltraSharp и Img2Img на релизном коде, включая assert размера, неизменности исходного Document/Request и пустого residents после release_idle_async. Все memory samples корректны; опрос20ms не является точным allocator high-water mark. Время — одиночные cold-runtime проверки, не performance A/B. Использован существующий safetensors UltraSharp и CUDA; в пользовательских настройках выбран PT и CPU для UltraSharp, поэтому приведённое время не обещает такую же длительность при CPU-апскейле.

Отдельный короткий dev-прогон512→1536,4/4 завершился29.580с. Финальный1536² PNG просмотрен: присутствует нормальное изображение без пустого/чёрного результата. Численные перестановки матричных операций могут менять отдельные пиксели; идентичность всей генерации прежнему backend не заявляется.

Артефакты: artifacts/sd15-release-{768,1024,1536}{.jsonl,-receipt.json,-console.log,.png}, sd15-hires-results.json, sd15-hires-{clippy,tests}.log, sd15-hires-release-build.log. Скрипт generation_bench поддерживает BENCH_MODEL/BENCH_FAMILY/BENCH_REFINE_CFG для воспроизведения; основной SDXL benchmark по умолчанию не менялся. Готовый EXE — target/release/image-editor-demo.exe, запуск image-editor/start.bat -Release. StrictLib/model-runtime не изменялись; нового коммита нет.
