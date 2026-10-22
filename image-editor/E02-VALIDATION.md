# E02 — быстрый выбор и ретушь

База: `ad98e204` (E01 и ранее подготовленные изменения закоммичены по запросу пользователя). Ветка `codex/image-studio`, рабочее дерево `C:/Users/gamer/.codex/worktrees/image-studio/StrictLib-mufoed`. Проверки выполнены до коммита E02. Изменения только в image-editor; StrictLib и общий model-runtime не изменялись.

## Реализовано
- Палочка W: four-connected flood, без перехода через края строк/диагонали. Shift+W: выбор по цвету по всему холсту. Допуск0..255, сравнение premultiplied RGBA (скрытый RGB прозрачных пикселей игнорируется). Образец активного слоя либо composite; в режиме маски — grayscale mask. Replace/Add/Subtract/Intersect и модификаторы Shift/Alt. Отдельное ручное выделение, без изменения AI mask и слоёв.
- Штамп S и восстанавливающая кисть J в одной группе палитры. Alt+ЛКМ выбирает источник, не рисуя и не создавая Undo. Контур радиуса и движущееся перекрестие источника на холсте, настройки радиуса/жёсткости/opacity в контекстной ленте. Каждый новый штрих начинает выборку с выбранного источника (unaligned). Snapshot активного слоя/маски захватывается один раз на жест; перекрытие не вызывает обратную связь от нанесённых пикселей.
- RetouchStroke и RetouchMode — публичный API крейта; источник, радиус, стиль и размеры валидируются до записи. Билинейная выборка с premultiplied alpha; композитинг source-over, прозрачный источник не стирает цель, выход за источник пропускается. Учитываются ручное выделение и опциональная маска ИИ; soft alpha, opacity и hardness.
- Оба инструмента работают через общий режим Растр/Маска. Растровый layer lock защищает растр, но не препятствует отдельной правке маски. Один Undo на жест, Escape отменяет незавершённый штрих; смена режима/слоя и история сбрасывают источник.

## Проверки
- CPU: `cargo +1.92.0 test -p image-editor --all-targets --features demo` — **143 passed, 1 ignored, 0 failed**. Strict Clippy всех targets с `demo` и `-D warnings` — PASS.
- Новые тесты: семь backend-сценариев в tests/retouch.rs и три egui pointer-сценария в src/ui/retouch_tools.rs. Проверены connected/global, tolerance, все combine modes, прозрачные цвета, соседство без row wrap, opacity/hardness, дробная alpha-выборка, immutable source, heal с сохранением текстуры и адаптацией тона, границы/invalid/locks, отдельная маска. UI: 200% zoom и смещение canvas origin, Alt без Undo, release endpoint, one Undo/Redo, Escape, отсутствие источника и переключение цели. Системный clipboard не используется.
- Производственные features `demo,plakat-cudnn,ollama`: **161 passed, 2 ignored, 0 failed**; strict Clippy всех targets `-D warnings` — PASS. Два ignored аппаратных/диагностических теста не выдаются за выполненные.
- Release `demo,plakat-cudnn`: PASS. Два изолированных native GUI запуска exit0; снимки [ретушь1440×900](artifacts/e02-retouch-1440.png) и [палочка/маска1100×700](artifacts/e02-wand-mask-1100.png) просмотрены. Контекстные группы помещаются в ленте; canvas и layers остаются видимыми, палитра в компактном окне прокручивается, цвета закреплены снизу.
- EXE SHA256: `5c9f355da18fd92a540231f84fff2e81f47d42c428ca7070357798b62c6b3a97`. Снимок Rust source и параметры — artifacts/e02-release-receipt.json. Запуск: `image-editor/start.bat -Release`.
- Логи: artifacts/e02-cpu-tests.log, e02-cpu-clippy.log, e02-full-tests.log, e02-full-clippy.log, e02-release.log, e02-retouch-launch.log и e02-wand-launch.log.

## Границы
Heal — цветоадаптивный перенос текстуры: разница локальных средних цветов вычисляется по двум круговым окрестностям один раз в начале штриха, с весом alpha. Пространственного Poisson solve, автоматического поиска источника и sampling merged layers для ретуши нет. Прозрачность применяется на сегмент как у обычной кисти. На холсте показывается геометрия источника; полупрозрачного пиксельного ghost-overlay переноса нет. PSD профиль не расширяется, инструментальное состояние/источник в PSD не записываются. Повторный AI inference не нужен для этих растровых изменений и не проводился.

Граф проекта D:/projects/StrictLib-mufoed относится к другому checkout, нужные image-editor пути имеют freshness=missing. Использованы прямое чтение/поиск исходников, компиляция и исполненные тесты. Логи и снимки — в локальном игнорируемом artifacts/.
