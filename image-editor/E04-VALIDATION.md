# E04 — автовосстановление проекта

База: `dbe2b7c5` (E03.2), ветка `codex/image-studio`, отдельный worktree image-studio. Изменения только в image-editor; StrictLib, mufoed и общий model-runtime не меняются. Коммит E03.2 создан по запросу пользователя, E04 подготовлен отдельно.

## Контракт

- Demo автоматически сохраняет отдельный recovery checkpoint несохранённого документа с интервалом10секунд. Сериализация, fingerprint, сжатие и запись идут на одном фоновом потоке; очередь ограничена одним заданием. UI создаёт один снимок Document на попытку, без сериализации на каждом кадре. Идентичное состояние и origin не переписываются. При смене проекта предыдущая запись дожидается завершения, затем очищается до публикации нового проекта.
- Каждый запуск/окно имеет случайный128-bit session ID и отдельный OS-lock. Работающие окна и копии, уже рассматриваемые другим окном, пропускаются. После аварии OS освобождает lock. Закрытие одного окна не удаляет recovery другого.
- По умолчанию каталог `%APPDATA%/MuFoEd/ImageStudio/settings.recovery/` рядом с настройками. При `IMAGE_STUDIO_SETTINGS` путь также меняется; `IMAGE_STUDIO_RECOVERY_DIR` задаёт его явно. Snapshot — внутренний `.snapshot`, исходный PSD/PNG/JSON используется только как информационный origin и никогда не является целью фоновой записи.
- Полный документ: размеры, raw RGBA/hidden RGB, порядок/имена/visibility/opacity/locks, Normal/Multiply/Screen/Overlay, enabled/default/pixels масок слоёв, независимые маска ИИ и ручное выделение, active и generation recipes. Состояние инструментов, Undo-история, несохранённые GPU-кандидаты и веса моделей в recovery не включаются. Настройки/LoRA/пресеты продолжают сохраняться существующим механизмом preferences.
- Формат `MUFREC01`: bounded JSON metadata + бинарные плоскости, gzip/CRC. В отличие от JSON-массивов пиксели не раздуваются в несколько раз. Лимиты:16слоёв,16,777,216пикселей,128MiB плоскостей;4MiB metadata;133MiB сжатого файла. Заголовок/геометрия/бюджет проверяются до выделения плоскостей. CRC, усечение и лишние данные отвергаются. Metadata должна roundtrip точно: oversized/non-finite параметры не заменяют прежний валидный checkpoint.
- Запись через NamedTempFile в том же каталоге, `sync_all`, atomic persist поверх старой копии. Оборванный временный файл не предлагается для восстановления. Проверена авария процесса; устойчивость к физическому отключению питания отдельно не проверялась.
- При запуске асинхронно читается одна копия и показывается центрированный modal с origin, возрастом, миниатюрой и Восстановить / Удалить копию / Позже. До выбора редактор и его глобальные shortcuts не выполняются. Восстановление сохраняет пользовательские настройки, заменяет документ, очищает старую историю и помечает восстановленный проект несохранённым.
- Прежняя recovery-копия удаляется только после подтверждённой публикации нового checkpoint или явного Save/Discard. Если запись восстановленного проекта не удалась, прежняя копия остаётся. «Позже»/Escape/закрытие стартового окна оставляют копии на диске. При нескольких копиях удаление переходит к следующей, восстановление/откладывание сохраняет остальные до следующего запуска.
- Повреждённая копия показывает понятное состояние с доступными Удалить/Позже; исходный CLI-проект остаётся целым. Ошибки recovery не перезаписывают исходник. Ошибки записи повторяются с интервалом, а не каждый кадр. При невозможности очистки после явного закрытия приложение не зависает навсегда; старая копия может остаться для следующего запуска.
- Save/Discard и нормальное закрытие очищают только собственную копию. Закрытие дожидается завершения фоновой записи и очистки; после подтверждения закрытия новые правки не принимаются. При ошибке освобождения моделей редактор возвращается в работу с автовосстановлением.

## Встраивание

`image_editor::recovery::{Session, Candidate, Draft, discover, fingerprint}` — явный storage API без скрытых записей из виджета. MuFoEd выбирает каталог, расписание и интерфейс. Demo интегрирует его через recovery_host. `ImageEditor::project_epoch` меняется при New/Open/replace; `restore_recovered` сохраняет настройки и выставляет unsaved. Ни оригинальные PSD-контракты, ни общий менеджер моделей не изменены.

## Воспроизведение

```powershell
cargo +1.92.0 clippy -p image-editor --all-targets --features demo -- -D warnings
cargo +1.92.0 test -p image-editor --all-targets --features demo
. ./image-editor/scripts/enter-cuda.ps1
. ./image-editor/scripts/enter-cudnn.ps1
cargo +1.92.0 clippy -p image-editor --all-targets --features demo,plakat-cudnn,ollama --config image-editor/cargo-performance.toml -- -D warnings
cargo +1.92.0 test -p image-editor --all-targets --features demo,plakat-cudnn,ollama --config image-editor/cargo-performance.toml
cargo +1.92.0 build -p image-editor --features demo,plakat-cudnn --bin image-editor-demo --release --config image-editor/cargo-performance.toml
cargo +1.92.0 run -p image-editor --features demo --example recovery_probe -- seed image-editor/artifacts/e04-fixture
./image-editor/scripts/validate_recovery.ps1
```

Для фото можно передать PNG третьим аргументом seed. Script создаёт свежий изолированный каталог, запускает только свой child-процесс в режиме `IMAGE_STUDIO_RECOVERY_PROBE=draft`, дожидается checkpoint, проверяет второе окно, принудительно завершает только свой PID и независимо читает checkpoint через recovery_probe. Затем проверяет два размера стартового окна, повреждённую копию, SHA исходника и настроек. Нужны существующий settings template (параметр SettingsTemplate) и собранный recovery_probe; пользовательские процессы/файлы не используются. Env после теста восстанавливается.

Граф D:/projects/StrictLib-mufoed не покрывает этот checkout: generation2026-10-19T19:03:34Z, evidence paths freshness=missing. Использованы прямые чтения исходников, исполненные тесты и нативные процессы. Артефакты и логи игнорируются Git.


## Итог приёмки

- CPU all targets `demo`: **185 passed, 1 ignored, 0 failed**; strict Clippy `-D warnings`: PASS.
- Production all targets `demo,plakat-cudnn,ollama`: **203 passed, 2 ignored, 0 failed**; strict Clippy `-D warnings`: PASS. Ignored — отдельные live Hub range / CUDA VAE probe, не выданы за выполненные.
- Новые backend-сценарии: точность raw pixels/слоёв/масок/default/locks/selection/active/recipe, разные окна/эксклюзивный выбор, untouched origin, отказ всех усечений и неверного CRC, лишнего compressed/decompressed payload, forged dimensions/count/memory до allocation, metadata overflow/non-finite и сохранение прежнего checkpoint. Идентичный fingerprint не переписывает файл.
- Host-сценарии: восстановление как unsaved с retention настроек, прежняя копия до нового durable checkpoint, сбой новой записи, Позже во время чтения, corrupt+другие drafts, Save/Close/New при in-flight записи, resume после ошибки внешнего shutdown. Реальные egui pointer down/up нажимают Восстановить/Удалить/Позже; источник и dirty state проверяются после выбора.
- Нативный релиз: собственный тестовый процесс1536×1536 с двумя слоями, Screen, layer mask, независимыми inpaint/selection и recipe принудительно завершён после checkpoint. Отдельный recovery_probe восстановил документ и origin точно. Второе окно до завершения владельца пропустило его recovery. Четыре последующих запуска exit0; исходный PSD и файл настроек SHA256 не изменились.
- Просмотрены финальные [recovery1100×700](artifacts/e04-native-v2/recovery-1100.png), [recovery1440×900](artifacts/e04-native-v2/recovery-1440.png), [повреждённая копия](artifacts/e04-native-v2/corrupt-1100.png), [второе живое окно](artifacts/e04-native-v2/live-window-skipped-1100.png). Modal центрирован, миниатюра целиком видна, все кнопки доступны. Native подтверждает запуск/аварию/storage и показ выбора; нажатия кнопок и повторная durable публикация проверены UI/host-тестами.
- Release `demo,plakat-cudnn`: PASS. EXE SHA256 `3ac25b589008eff10e4f4f5f5814fc8a4cfe8e90f5170a68e94780e82a0fb12c`. Запуск: `image-editor/start.bat -Release`. Rust source hash, base SHA и результаты: artifacts/e04-release-receipt.json; нативная приёмка: artifacts/e04-native-v2/receipt.json.
- Логи: artifacts/e04-cpu-tests.log, e04-cpu-clippy.log, e04-production-tests.log, e04-production-clippy.log, e04-release.log, e04-native-validation.log, e04-native-v2/exact-recovery.log и native stdout/stderr. Предварительный e04-native-final относится к промежуточной сборке; финальный receipt использует только e04-native-v2.

Автовосстановление E04 принято. Текстовый инструмент остаётся отдельным следующим пунктом, требующим решения о PSD-семантике; он не входит в эту приёмку.
