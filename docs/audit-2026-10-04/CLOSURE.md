# Закрытие аудита 2026-10-04: повторная приемка

Проверяемое дерево — рабочая копия поверх коммита `015e0c72e1870bebfef1642bf15788e665f12754` (`feat(edit): add search, atomic replace-all and compound moves`). Это не аудируемый исходный коммит `7a8e6d37d4d96543db00a9c4f2be51f879397c1f`: измерения в [AUDIT.md](AUDIT.md) и [REPORT.md](REPORT.md) остаются измерениями того коммита. Коммит этой приемки не создавался.

Свидетельства лежат в `target/audit-closure/` (каталог сборки, не исходник).

## Версия

| Что | Значение |
| --- | --- |
| HEAD | `015e0c72e1870bebfef1642bf15788e665f12754` |
| Снимок до прогона | `target/audit-closure/HEAD.sha`, `status.txt`, `diffstat.txt` |
| Где исполнялись проверки | рабочая копия, если ниже не сказано «чистый HEAD» |

Чистый HEAD проверялся только для `strict-ooxml-edit/tests/operations.rs`, во временном worktree `StrictLib-head-closure` (затем удален). Остальные команды этой приемки читали рабочую копию: незакоммиченные исправления аудита плюс перевод переводов строк Rust-файлов аудита на LF.

`strict-ooxml-edit/src/operations.rs` и `tests/operations.rs` в рабочей копии совпадают с HEAD. Обратный контроль идентификатора в worktree после прогона возвращен (`git checkout`); в основной репозиторий он не переносился.

## Операции: два падения

Оба теста описывают контракт [EDITING_OPERATIONS.md](../EDITING_OPERATIONS.md): действительный перенос выдает абзацу новый ParaId, пока старый идентификатор еще занят; перенос границы секции, чужой story и выход за диапазон отвергаются без изменения тела и без потери redo. Ожидания не ослаблялись и тесты не удалялись.

**Идентификатор вложенного переноса** — дефект реализации, уже исправленный в HEAD. `assign_id(..., fresh=true)` заменяет существующий ParaId. Insert вызывает `fresh_ids` до Delete, поэтому старый идентификатор остается занятым (`strict-ooxml-edit/src/structured.rs`).

Обратный контроль на чистом HEAD: ранний выход `assign_id`, если `para_id` уже есть, независимо от `fresh`. Результат до восстановления файла:

- команда: `cargo +1.92.0 test -p strict-ooxml-edit --locked --no-default-features --test operations -- --test-threads=1 nested_block_move_retains_contents_and_reports_fresh_ids`
- код 101, `target/audit-closure/operations-before-fresh-id.log`
- `left: Some(ParaId("00000001")) right: Some(ParaId("00000001"))`

После `git checkout -- strict-ooxml-edit/src/structured.rs` тот же фильтр вместе с `illegal_moves_are_atomic_and_preserve_redo`: код 0, `operations-after-restore.log`.

Полный набор на чистом HEAD до контроля: 15 passed, код 0, `operations-head-worktree.log`. На рабочей копии: 15 passed, код 0, `operations-working-copy.log`.

**Незаконный перенос** — ожидание верное, на этом HEAD тест проходит. `Editor::new` открывает документ с границей секции в абзаце и `sectPr` тела. `move_block` абзаца с границей возвращает `OperationError::Edit(InvalidModel)`, потому что Delete отвергает `contains_boundary`. Тело остается исходным, перенос в сноску дает `UnsupportedMove`, индекс 9 дает `InvalidParagraph`, redo восстанавливает текст `a` / `c`. Исторический panic на `unwrap` был отказом открытия или транзакции до этих проверок; текущий код доходит до них и выполняет их.

## Приемка рабочей копии

Все команды с `cargo +1.92.0`. Коды: `target/audit-closure/exit-codes.txt`.

| Проверка | Код | Лог |
| --- | --- | --- |
| `fmt --all -- --check` | 0 | `fmt.log` |
| `clippy --workspace --all-targets --all-features -- -D warnings` | 0 | `clippy.log` |
| `check --workspace --all-targets` (фичи по умолчанию) | 0 | `check-default.log` |
| `test --workspace --all-features --locked` | 0 | `workspace-test.log` |
| `test --workspace --all-features --release --locked --test hostile` | 0 | `hostile-release.log` (hostile: 43 passed) |
| матрица фич `strict-ooxml` none/svg/pdf/write/convert/default/all-features | 0 | `features-*.log` |
| пример `render_pdf` | 0 | `example-pdf.log` |
| `check -p strict-ooxml-cli --locked` | 0 | `check-cli.log` |
| edit: none, save, visual; фасад `--features edit --test editing` | 0 | `edit-*.log`, `facade-edit.log` |

До `fmt --check` часть новых файлов аудита была с переводами строк CRLF. `cargo fmt --all` записал LF. Повторный `--check` дал код 0. Пороги и ожидания тестов при этом не менялись.

### Вложенность текстовых рамок

Бюджет и стек не повышались.

- `strict-ooxml-testkit/src/harness.rs`: `STACK = 1 << 20` (1 МиБ)
- `strict-ooxml-core/src/limits.rs`: `max_text_box_nesting = 5`, `max_block_nesting = 12`, `max_xml_depth = 256`

`cargo +1.92.0 test -p strict-ooxml --all-features --locked --test hostile -- --test-threads=1 nesting::` — код 0, `hostile-nesting-debug.log`. Тест `nesting::a_text_box_past_its_own_budget_costs_its_content_not_the_document` открывает глубины 6 и 20, сообщает `max_text_box_nesting (5)`, оставляет 3 блока тела и проводит SVG и запись внутри `assert_survives`. Глубина 40 дает `LimitKind::XmlDepth`. Тот же набор входит в release-прогон hostile (43 passed).

## XSD и OPC на этой рабочей копии

Писатель: `cargo +1.92.0 build --release -p strict-ooxml-cli`, код 0, `xsd-build.log`. 27 пакетов из `strict-ooxml-core/tests/strict/*.docx` записаны в `target/audit-closure/xsd-written`. Коды записи: `xsd-write-codes.txt`. `annotation-ref-sdk-mixed-rels.docx` завершился кодом 2 (G-10, смешанный Strict/Transitional). Это ожидаемый отказ, не пропущенный выход. Остальные коды 0 или 1 (записано с потерями в отчете); файл для гейта есть у всех, кроме G-10.

| Команда | Код | Лог |
| --- | --- | --- |
| `python xtool/xsd-gate/test_f01_gates.py` | 0 | `xsd-f01.log` (`f01 gates: pass`) |
| `python xtool/xsd-gate/xsd_gate.py --written target/audit-closure/xsd-written --quiet-messages` | 0 | `xsd-gate.log` |
| `python xtool/xsd-gate/opc_gate.py --written target/audit-closure/xsd-written` | 0 | `opc-gate.log` |

Строка измерения: `documents=27 validated=26 missing=0 refused=1 unmatched=0 ours=0 source=11`.

- Новых нарушений писателя нет: `ours=0`. Пункты XS-01…XS-15, XS-20…XS-23, XS-26 в этом прогоне `NOT_EXERCISED`.
- Перенесенные нарушения источника: XS-16, 11 сообщений (`c:gapWidth` / `c:overlap` / `c:lblOffset` в частях диаграмм). Выход из-за них не является схемно чистым. Это не новый дефект писателя.
- G-10 остается в списке отказов (`refused=1`) и не считается неожиданным отсутствием файла.

OPC: `packages: 26`, `violations: 0`, `PASS`.

## Linux, тот же HEAD, та же рабочая копия

Среда: WSL Ubuntu, `rustc 1.92.0 (ded5c06cf 2025-12-08)`, `cargo-llvm-cov 0.6.15`. Снимок: `linux-version.txt` (HEAD `015e0c72…`, на старте покрытия 85 измененных путей). `CARGO_TARGET_DIR=$HOME/strictlib-cov-target`, `CARGO_PROFILE_DEV_CODEGEN_UNITS=1`. Каталог Windows `target/` не использовался.

`opc/zip.rs` в отчетах нет (поиск по `cov-*.log` и `cov-pkg-*.log`). В отчете core есть `opc/zip/mod.rs` и `opc/zip/write.rs`. Фантомный `opc/zip.rs` с Windows llvm-cov 0.9.0 в этот прогон не входит и его 76,23% не используются.

| Крейт | Команда как в CI, код | Строки |
| --- | --- | --- |
| strict-ooxml-core | 0 | 90,71% (`cov-strict-ooxml-core.log`) |
| strict-ooxml-fidelity | 0 | 91,01% (`cov-strict-ooxml-fidelity.log`) |
| strict-ooxml-wml | 1 | TOTAL 66,68% включает зависимости |
| strict-ooxml-report | 1 | TOTAL 52,07% включает зависимости |
| strict-ooxml-render-svg | 1 | TOTAL 63,52% включает зависимости |

Версия 0.6.15 складывает в TOTAL зависимости. Порог CI относится к строкам крейта. Повтор с `--fail-under-lines 80` и `--ignore-filename-regex` по путям зависимостей:

| Крейт | Код | Строки своего кода | Лог |
| --- | --- | --- | --- |
| wml | 0 | 83,06% (регионы 78,60%; порог CI — строки) | `cov-pkg-strict-ooxml-wml.log` |
| report | 0 | 96,57% | `cov-pkg-strict-ooxml-report.log` |
| render-svg | 0 | 84,80% | `cov-pkg-strict-ooxml-render-svg.log` |

Покрытие по строкам пяти крейтов на этой рабочей копии выше 80. Код 1 первых трех команд без фильтра — свойство отчета 0.6.15, не недостаток строк крейта. Регионы wml ниже 80; гейт CI их не проверяет.

Fuzz, `cargo +nightly fuzz run --target x86_64-unknown-linux-gnu`, `CARGO_TARGET_DIR=$HOME/strictlib-fuzz-target`, лимит `-max_total_time=60`. Коды: `linux-fuzz-exit-codes.txt`. Все восемь целей завершились кодом 0; в логе каждой есть `Done` примерно за 61–70 с (`fuzz-fuzz_zip.log` … `fuzz-fuzz_convert.log`). Это дымовой прогон, не часовой `fuzz-nightly`. Nightly на время прогона дописал в `fuzz/Cargo.lock` пакеты hayro; файл возвращен к HEAD, чтобы измерение не меняло lock.

## Статусы, без одной формулировки на все карточки

**PASS, синтетические свидетели и эта приемка.** F00–F15 и F17–F21, плюс синтетический сдвиг общей рамки F16. Квитанции карточек — прежние `target/audit-fixes/`. Повтор на этой рабочей копии: таблица выше, XSD `ours=0`, OPC `violations: 0`, вложенность 6/20 и отказ 40 через `XmlDepth`.

**PASS, только синтетика F16.** Соседние абзацы с одинаковыми свойствами рамки образуют одну группу, и сдвиг рамки двигает потомков. Это не полнота схем страниц 54, 56 и 104.

**BLOCKED, F16, сложные схемы.** Эталон страниц снят в WPS Office 12.1.0.28485 (`visual-thesis/wps-reference/`). Сравнение SVG тех же номеров страниц с базовой линией текста в PDF есть в `target/audit-closure/wps-compare/lines.json`. Порог 0,25 px не выполнен.

- Страница 54. Имена праймеров (`L15996`, `H16142` и соседние) имеют dx 0,09–0,20 px и устойчивый dy около −3,9 px. `HVR-I`: dx +0,86 px, dy −1,93 px. Горизонталь схемы близка к эталону, вертикаль выходит за допуск.
- Страница 56. Подписи клад (`-M`, `■H` и соседние) имеют dx около +0,09 px и dy около −35,8 px. Номера полиморфизмов (`11719`, `6371` и соседние) сдвинуты примерно на −359 px по x и −425 px по y. Подпись рисунка и номер страницы укладываются в 0,25 px. Дерево не собрано.
- Страница 104. Два JPEG: ширина и высота совпали (182,4×153,6 и 182,4×144 px), x в пределах 0,2 px, y ниже эталона примерно на 1 px. Подписи `A,C` и `B,D` сдвинуты вправо на 3,1 и 5,1 px. Часть подписей поверх карт приходит нечитаемой строкой.

- Страница 54, подписи системы праймеров. Рамка x=1787, y=1340, w=8357, h=5250 twips. Нужен эталон начала каждой подписи. На измерении аудита было 233 text и 7 начал вне страницы.
- Страница 56, филогенетическая схема. Рамка x=8635 twips, ширина 480 twips. Нужен эталон положений подписей и концов ребер. На измерении аудита многие подписи начинались с x=6,667; 53 text и 22 line не доказывают дерево.
- Страница 104, карты. JPEG `image4.jpeg` и `image5.jpeg` сохранены байт-в-байт (880×398 и 880×396) — это отдельный факт о растрах, не о подписях. Нужен эталон начала подписи относительно каждого изображения. Байты JPEG не менять, чтобы «починить» подпись.

**BLOCKED, закрытый корпус.** Публичный CI и эта приемка его не исполняют. Пропуск не записывается как успех.

**Неподдерживается, A02.** `cubic`, `clip`, `pattern`, `tight-contour`, `through-contour`. Они называются в отчете потерь и не рисуются (`UNSUPPORTED_VECTOR_PRIMITIVES` в `strict-ooxml-testkit/tests/f21_public.rs`).

**Не является PASS.** Ночной fuzz на час (`fuzz-nightly` в CI, 3600 с). Закрытый corpus census. Полнота F16. Утверждение «F00–F21 пройдены» целиком.

## Что исправлено в этой проверке

- Переводы строк новых файлов аудита приведены к LF, иначе `fmt --check` падал.
- Вводные абзацы [AUDIT.md](AUDIT.md) и [REPORT.md](REPORT.md) больше не называют набор F00–F21 закрытым, пока схемы F16 заблокированы.
- Причина падения ParaId подтверждена обратным контролем и оставлена исправленной в HEAD. Тест незаконного переноса оставлен как есть: он проходит.

## Условия окончательной приемки

1. Рабочая копия, для которой заявлен PASS, совпадает с деревом, на котором сняты логи `target/audit-closure/`. Новый коммит требует повторения тех команд, чей код выхода на него ссылаются.
2. Синтетические карточки остаются зелеными на `cargo +1.92.0` без ослабления порогов, без роста `STACK` и без смены `max_text_box_nesting`.
3. XSD на публичном корпусе: `ours=0`. Ненулевое `source` допустимо только как перенос XS-16. G-10 остается `refused`, не `missing`.
4. F16 страницы 54, 56 и 104 остаются BLOCKED: сравнение с эталоном WPS уже есть, допуск 0,25 px не выполнен.
5. Закрытый корпус и неподдерживаемые примитивы A02 не называются пройденными.
6. Дымовой Linux fuzz на этом дереве выполнен: восемь целей, код 0, около 60 с каждая. Часовой прогон CI (`-max_total_time=3600`) не запускался и PASS не получает.
