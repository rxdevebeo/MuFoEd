# E03.1 — уровни и кривые

База: `0a64079f` (закоммиченный E02), ветка codex/image-studio. Изменения только в image-editor; StrictLib, MuFoEd и общий model-runtime не менялись. Проверки выполнены до коммита E03.1. Это первый пункт E03; слойные маски и blend modes остаются следующим пунктом.

## Контракт
- Levels: общий RGB и R/G/B, input black<white, gamma0.1..10, output black<=white. Curves: до32 узлов, строгий порядок X, крайние x0/255, вертикальное перемещение краёв; локальная Hermite-интерполяция с ограниченными касательными и без overshoot между узлами. Master LUT выполняется до индивидуальных каналов; промежуточные значения округляются до RGB8.
- Одинаковые ToneAdjustment::apply и lookup_tables для preview/commit. Выделение смешивает исходный и скорректированный straight RGB, alpha сохраняется точно; полностью прозрачный скрытый RGB не меняется. Нет дополнительного ограничения маской ИИ: область задаётся независимым ручным выделением. Маска ИИ — самостоятельная цель в общем режиме «Маска» и корректируется через edit_mask.
- Histogram: Y/R/G/B, alpha×selection weighting в u64, без вклада невидимого RGB. Логарифмическая/линейная шкала; нейтральные настройки byte-exact. До/после уменьшаются после расчёта полного растра; для растра показывается общий composite с той же функцией смешивания, что при PNG export, для маски — grayscale coverage.
- Центрированный egui Modal блокирует рисование, глобальный Undo/Redo и файловые shortcuts; «Сбросить канал/всё», применение одним Undo, отмена без побочных изменений. Заблокированный растр защищён, маска доступна отдельно. Snapshot Document проверяется при Apply, чтобы host changes не были затёрты.
- Результат запекается в RGB8 пиксели; формат PSD/JSON не расширяется, adjustment layers не заявляются.

## Проверки
- CPU all targets `demo`: **155 passed, 1 ignored, 0 failed**; strict Clippy `-D warnings` — PASS.
- Восемь backend-сценариев tests/tone.rs: нейтральная точность, gamma/endpoints/master order, smooth curve knots/segment bounds/inversion, alpha/hidden RGB/selection/other layers, histogram weights, invalid/locked atomic отказ, маска и PSD roundtrip.
- Четыре UI-сценария: до Apply исходник не меняется; preview соответствует применённым пикселям и PSD; один Undo/Redo для обоих инструментов и целей; reset/no-op/Escape/stale source/lock/mode switch; блокировка глобального Undo с контрольным срабатыванием того же shortcut после закрытия; реальные pointer add/drag/delete кривой с сохранением порядка узлов.
- `demo,plakat-cudnn,ollama` all targets: **173 passed, 2 ignored, 0 failed**; strict Clippy всех targets с теми же features и `-D warnings` — PASS. Два ignored аппаратных/диагностических теста не выдаются за выполненные. Итоговый полный прогон включает финальную компоновку и исправленную симуляцию модификаторов клавиатуры.
- Отдельный сценарий проверяет полный composite со слоем alpha128/opacity128, неизменность исходника при preview, совпадение с PNG после реального save/decode и невидимость скрытого слоя. Новые настройки в PSD не сериализуются, сохраняются уже изменённые RGB8 пиксели.
- Release `demo,plakat-cudnn`: PASS. Три изолированных native запуска exit0. Снимки [уровни1440×900](artifacts/e03-levels-1440.png), [кривые1100×700](artifacts/e03-curves-1100.png), [уровни1100×700](artifacts/e03-levels-1100.png) просмотрены: окна центрированы, кнопки доступны, полный before/after помещается. При меньшей высоте доступна прокрутка содержимого с отдельным footer действий. Настройки запусков изолированы от пользовательских.
- EXE SHA256: `2aaf2ceb7acc622e0b413588aba1303b40660a336f477a2d31402ba66ec061e5`. Снимок Rust source и параметры: artifacts/e03-release-receipt.json. Запуск: `image-editor/start.bat -Release`.
- Логи: artifacts/e03-cpu-tests.log, e03-cpu-clippy.log, e03-full-tests.log, e03-full-clippy.log, e03-release.log, e03-levels-launch.log, e03-curves-launch.log и e03-levels-compact-launch.log. Inference не менялся и повторно не запускался.

Граф основного D:/projects/StrictLib-mufoed не содержит image-studio checkout, проверенные пути имеют freshness=missing. Для этой задачи применены прямые чтение/поиск исходников, компиляция и исполненные тесты. Логи/снимки/receipt находятся в игнорируемом artifacts/.
