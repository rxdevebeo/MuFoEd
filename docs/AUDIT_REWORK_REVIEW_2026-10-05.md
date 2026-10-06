# Независимая приёмка доработок R01–R11

Дата: 5 октября 2026 года. Проверен `63be263497fa293d27734c0868c68c8540ff7f9f`, master, 11 новых коммитов после базы заказа `b413b15`. Проверка относится к содержимому исходников этого HEAD; tracked production diff отсутствует. Три ранее существовавших untracked группы CI/эталонов сохранены. Производственные исправления в ходе этой приёмки не выполнялись.

**Решение: частичная приёмка, общий заказ не закрыт.** Полный workspace сейчас красный. Приняты конкретные результаты сохранности FEFF, XSD на измеренных корпусах, две исправленные wrap-регрессии, активные регионы на выбранных свидетелях и live browser UI. Полные статусы PASS из карточек исполнителя не переносятся автоматически. Возврат — [заказ D01–D09](AUDIT_REWORK_RETURN_2026-10-05.md).

## Свежие проверки

| Проверка | Результат текущего дерева |
|---|---|
| `cargo +1.92.0 test --workspace --all-features --locked --no-fail-fast` | **1414 passed, 14 failed, 0 ignored; exit 101; 8 failed targets** |
| Обычный workspace без no-fail-fast | Exit 101 на hostile heavy footer |
| Heavy footer release, повторно отдельно | **Не завершился за прежние 10 секунд**, exit 101 |
| fmt | **FAIL**, содержательные format diffs; это не прежняя проблема CRLF |
| clippy workspace/all-targets/all-features `-D warnings` | **FAIL**, как минимум шесть диагностик core; сборка остановилась до полной проверки остальных крейтов |
| MSRV all-features workspace check, Rust 1.92.0 locked | PASS |
| F21 behavioral runner | PASS: 67 executed, 21 passed cards, 14 blocked matrix rows, 72/86 matrix entries labelled measured; `full_audit=false` |
| Census selftest | PASS; дополнительные контроли обнаружили обход inventory |
| Исходные wrap-пробы после обновления парсинга `x` | 3/3 PASS: host Square, следующий paragraph Square, TopAndBottom |
| F13 / F16 / R09 browser из workspace | 7/7, 6/6, 3/3 PASS соответственно |
| Strict XSD / OPC | 27 входов: 26 выходов, ожидаемый G-10 отказ; ours/unmatched/missing=0, source=11; OPC=0 |
| Local Transitional census | 121 validated, schema=0, gate PASS; полнота semantic dispositions этим не принята |
| CC0 write/reopen/rewrite | 100/100, fixed point 100/100, hash/size 100/100; write 33 Clean/67 Degraded |
| CC0 Strict XSD / OPC | 100 validated, ours/unmatched/missing=0, source=7; OPC=0 |
| CC0 первая страница | 98/100 завершились; два timeout по 90 секунд. Отдельно 035 завершился за 82,15 с, 017 снова timeout без SVG |

CC0 `017_Catalogue_Of_Books_At_J_C_Collage_Library_Mysore.docx` ранее рендерил первую страницу за 0,885 с; `035_Scarlat_Demetrescu_Din_Tainele_Vieii_Si_Ale_Univer.docx` — за 0,539 с. Это исторические измерения предыдущей базы, не строго контролируемый benchmark. Свежий отдельный timeout 017 и многократный hostile timeout подтверждают необходимость расследования производительности. Увеличение бюджета не является исправлением.

Полные страницы и multilingual fidelity CC0 не приняты. Probe завершился с exit 0 при двух TIMEOUT: этот exit обозначает завершение сбора результатов. Старые SVG в каталоге не использовались для объявления timeout успешным; повторные проверки выполнялись в новых выходных каталогах.

## Решение по каждой карточке

| Карточка | Решение независимой приёмки | Граница / возврат |
|---|---|---|
| R01 | Принято исправление FEFF | Свежие unit/regression tests и 100/100 fixed point; общий workspace остаётся красным |
| R02 | Частично принято, вернуть | Корпусный XSD улучшен; новая порча hue-angle и немая потеря неизвестного алгоритма защиты |
| R03 | Вернуть | Разделение schema/inventory работает; wildcard dispositions скрывают неразобранные изменения |
| R04 | Приняты две исходные регрессии; остальное PARTIAL | Все три дополнительные wrap-пробы GREEN; полная sides/distances/container matrix не завершена |
| R05 | Вернуть | Shaping добавлен, face hashes есть; PDF продолжает char mapping вместо glyph shaping; RTL intervals повреждены; caret/convert/visual/performance регрессии |
| R06 | Приняты семь региональных свидетелей; полное закрытие не принято | Active selection и явный DidNotConverge реализованы; actual oscillation witness отсутствует, heavy footer pipeline падает |
| R07 | PARTIAL | Шесть synthetics проходят, таблица собрана; WPS residuals остаются, Word-профиль NOT_RUN |
| R08 | PARTIAL, вернуть доказательства | Поведенческий F21 честнее, но 14 матриц открыты; hash porcelain не является hash содержимого |
| R09 | Принят live UI/Carlito свидетель; профиль PARTIAL | 3 browser tests PASS; путь установленного браузера не обеспечивает pin версии/байтов и среду без системных fonts |
| R10 | PARTIAL, вернуть | В артефактах line floors измерены; WML branches 63,79% <70%, regressions скрыты для coverage через ignore-run-fail; long fuzz и exact-SHA CI не доказаны |
| R11 | Не завершено | Исторический rollup не отражает свежий красный workspace; all-pages и остальные общие гейты не приняты |

## Подтверждённые дефекты и ограничения доказательств

### 1. R02: углы превращаются в проценты

`is_drawingml_percentage_attr` включает `hue` и `hueOff`. Независимая компилируемая [проба](audit-rework-review-2026-10-05/evidence/acceptance_angle_review.rs) получила `<a:hue val="60%"/><a:hueOff val="60%"/>` из `60000`. [Лог](audit-rework-review-2026-10-05/evidence/angle-review.log) — behavioral FAIL. Cached Strict XSD объявляет hue как CT_PositiveFixedAngle, hueOff как CT_Angle: это не процентные типы. [Извлечение схемы](audit-rework-review-2026-10-05/evidence/angle-schema.txt).

### 2. R02: неподдерживаемая защита объявляется Clean

Минимальный Transitional [вход](audit-rework-review-2026-10-05/evidence/unknown-protection.docx) содержит enforcement и `cryptAlgorithmSid=9999`. Writer сохраняет hash/salt/spinCount, но убирает алгоритм. Pipeline [возвращает Clean с пустыми issues](audit-rework-review-2026-10-05/evidence/unknown-protection.pipeline.json). Установлена необъявленная потеря параметра; фактическое поведение password verification в Word не измерялось. Контракт R02 требует точной потери/отказа для неподдерживаемой защиты.

### 3. R03: гейт принимает неизвестные изменения

TZ-24/35/36/37/39 используют `elements=["*"]` для целых частей, TZ-42 — `a:*`, `wp:*`, `c:*`, `m:*`. Нет semantic proof, что удалённое содержимое регенерировано эквивалентно. [Негативный контроль](audit-rework-review-2026-10-05/evidence/census-negative.json): неизвестные изменения settings и DrawingML классифицируются как declared_transform, а `a:left` под tblBorders ошибочно совпадает с WML-правилом TZ-23. `element_item_matches` сравнивает также local name квалифицированного pattern. `vanished_elements` всё ещё сравнивает присутствие local-name в части, поэтому сохранение одного одноимённого узла скрывает удаление другого. Schema/inventory separation принята; отсутствие скрытых потерь — нет.

### 4. R05: общий ресурс ещё не означает общий shaping

PDF `PageWriter::text` проходит по Unicode chars и font.chars, затем выдаёт `show(Str(encoded))` одним text matrix. Он не потребляет shaped glyph IDs, cluster advances и offsets из R05. SVG повторно shape-ит текст при paint и задаёт per-scalar x; glyph offsets не сохраняются в ShapedCluster. Сохранение bytes hash лица не доказывает согласованность геометрии.

Дополнительная [RTL-проба](audit-rework-review-2026-10-05/evidence/acceptance_shape_review.rs) воспроизводит интервалы `(2,0)`, `(4,2)`, `(6,4)` у текста `سلام`. [Лог FAIL](audit-rework-review-2026-10-05/evidence/shape-review.log). Сортировка byte_start производится после вычисления byte_end в визуальном порядке. Degraded статус не делает неверные Unicode intervals допустимыми. Latin `office` и combining `e+acute` имеют полный scalar count в этой пробе; их реальная glyph paint fidelity этим не доказана.

`LayoutContext::measure` вызывает resolve_face и SHA-256 полного font program на каждом измерении текста. Это кандидат причины сильной деградации времени; причинность требует profiler/контролируемого benchmark. Готового диагноза из одного исходного чтения не объявляем.

### 5. Общие regressions

[Полный список 14 failures](audit-rework-review-2026-10-05/evidence/workspace-summary.json). Часть — устаревшие oracles после многочисленного `x` в SVG: numbering/render/svg_oracle ожидают один float. Их надо обновить для корректного синтаксиса, сохранив проверку всех coordinates, glyphs и collisions.

Другие failures затрагивают поведение: caret selection width не равна renderer text width; convert tables даёт `Merged  header`, дополнительные paragraphs и нарушает table position; SVG/PDF strict-text и strict-text-grid не проходят прежние WPS bounds; hostile timeout. Сначала различить дефект oracle и production, затем исправить. Изменение expected/threshold/golden для получения GREEN не является приёмкой. Регенерация paragraphs/table goldens в R10 была сделана текущим renderer output, вопреки общему протоколу заказа; нужны независимые основания корректности изменений.

### 6. Receipts и coverage

`dirty_tree_hash()` — SHA-256 `git status --porcelain`. Содержимое файла может изменяться сколько угодно при том же статусе ` M file`: такой fingerprint не идентифицирует измеренный код. Даже честное название fingerprint в карточке не выполняет требование hash фактического дерева. Наш [verification.json](audit-rework-review-2026-10-05/evidence/verification.json) содержит хеши фактических tracked bytes, corpus manifest, CLI и артефактов.

R10 Linux JSON прочитаны независимо, но инструментированный Linux прогон этой приёмкой не повторялся. WML branch JSON: 657/1030 = 63,7864%; core branch 669/840 =79,6429%. `core-final.json` имеет 6858/7573 =90,5586%, тогда как текст карточки пишет 6860/7573 =90,5850%; оба выше line floor, но receipt должен назвать точный файл и run. Line coverage, измеренная с `--ignore-run-fail`, не становится behavioral PASS.

## Word сейчас недоступен

Предлагаемый порядок описан в [профиле приёмки без Word](WORD_INDEPENDENT_ACCEPTANCE_2026-10-05.md). Продолжаем STRUCTURAL и WPS-PINNED; WORD-COMPAT отдельно NOT_RUN. R07 уже имеет ошибки против существующего WPS, которые нужно исправлять сейчас. LibreOffice page 56 визуально содержит наложения и неверное размещение и не принят положительным эталоном. Word Online PNG не имеет достаточного provenance/масштаба для точной приёмки.

Source hashes `table.rs` и `paginate.rs` совпали с R07 post-fix receipt. Его WPS residuals относятся к той же реализации этих файлов; новый независимый рендер всей thesis и численная переаттестация всех её компонентов здесь не выполнялись. Проверены свежие six synthetics и сохранённые references. Наличие общих layout sources SVG/PDF не подменяет реальные backend measurements.

## Текущая база и следующий шаг

Последний проверенный успешный внешний CI всё ещё [37195191322](https://github.com/rxdevebeo/MuFoEd/actions/runs/37195191322), SHA `7a8e6d3`; scheduled run 37293517543 завершился cancelled. Exact HEAD CI отсутствует. Commit/push в ходе проверки не выполнялись. Waivers, unsupported vector/contour primitives и отдельные RTF/DjVu/editor expansion работы автоматически не закрыты.

Рабочие исправления сохраняются; следующая итерация идёт по D01–D09. Приоритет — восстановить regressions/performance, исправить новые normalizer/protection дефекты и убрать обход census. Геометрия WPS и доказательства продолжаются без ожидания Word. Для нового продуктового плана база остаётся **PARTIAL с текущими красными gates**, а не прежняя зелёная база.

Артефакты: `target/acceptance-rework-review-2026-10-05/`; компактные долговечные [свидетельства](audit-rework-review-2026-10-05/evidence/verification.json). Временные integration probes удалены из tests после выполнения, их код/логи сохранены в docs. Tier 2 graph/source review: relevant coverage проверена, metadata_changed дополнена прямым чтением, исключённые corpus/coverage/PDF/PNG inspected напрямую. Отрицательные выводы ограничены перечисленными путями и контрактами; исчерпывающий аудит всей кодовой базы этим не заявлен.
