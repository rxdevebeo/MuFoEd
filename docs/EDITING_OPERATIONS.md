# Операции поверх editing ядра

Задача — добавить поиск, массовую замену текста и перемещение блоков/строк в пределах возможностей существующего ядра. Слой Operations использует только публичные чтения Editor и существующий transact/Edit. Протокол, validator, snapshots, writer и renderer этой очередью не расширяются. API доступен из strict_ooxml_edit и фасада strict_ooxml::edit.

Область: поиск по literal text (case sensitive или Unicode lowercase, whole word), paragraph/character style, SourceLocation и наличию complex field; scopes All/Story/Paragraph. Поиск проходит body, headers/footers, notes, tables, SDT и text boxes. Не переходит границы абзаца. Whole word означает границы alphanumeric/underscore; Unicode normalization, regex и full case folding не добавляются.

Replace-all компилирует найденные text slices в TextNode commands и вызывает transact один раз. Совпадения могут пересекать runs и поддерживаемые inline wrappers; структурные символы, fields/revisions не уничтожаются. Strict policy отменяет всю операцию при защищенном совпадении; EditableOnly возвращает явные skipped hits. No-op не меняет revision/redo. Один undo отменяет всю замену. Результат поиска привязан к revision. Число совпадений, scalars, команд и вычислительная работа ограничены.

Поиск работает по тексту модели, включая скрытый текст и cached field results, а не по визуально отрисованным глифам. InstrText не включается. Tab и Break проецируются как соответствующие символы, рисунки и другие структурные узлы — как U+FFFC. Такое совпадение можно найти, но нельзя заменить как обычный текст. Совпадения не перекрываются; offsets считаются в Unicode scalars. Метаданные стиля — прямые ссылки, без вычисления наследования. SourceLocation сравнивается точно. В замене новый текст помещается в первый узел совпадения и наследует его свойства; последующие matched slices очищаются, wrappers остаются. Replacement с CR/LF/tab или недопустимым XML символом отвергается.

Revision действительна внутри текущей Editor session. После изменения адреса и результаты поиска нужно получить заново. По умолчанию лимиты планирования: 10 000 совпадений, 16 000 000 просмотренных scalars, 100 000 команд, 64 000 000 сравнений/посещений. При превышении лимита частичный результат не публикуется и транзакция не начинается. Ограничения истории, размера абзаца и глубины остаются за существующим Editor.

Перемещение блока — Insert + Delete в одной транзакции в пределах одного block container. destination — граница вставки до изменения, включая конец. Перемещение строки — InsertRow + DeleteRow в пределах одной таблицы. No-op позиции сохраняют IDs/history. При действительном перемещении Insert переидентифицирует paragraphs; отчет возвращает old/new IDs, destination и ChangeSet. Некорректные sectPr/cell-final-paragraph/merge/boundary операции отвергает существующее ядро без мутации. Между контейнерами/stories перемещение пока не предоставляется, чтобы не угадывать remapping адресов.

Вставка выполняется первой, пока исходные paragraph IDs заняты; это предотвращает повторное выделение прежнего ID. Delete использует скорректированный после вставки адрес. MoveReport содержит также конечный индекс строки для move_row. Все paragraphs перемещаемого поддерева перечислены в identities в порядке обхода. SourceLocation вставленного поддерева становится virtual, text IDs очищаются по существующему контракту Insert. Это композиция команд копирования/удаления, а не обещание сохранения идентичности перемещенного объекта.

Приемка: компилируемый scaffold RED; GREEN на фактическом тексте/метаданных, независимых offsets и exact undo; compiled обратная проверка. Обязательны across-run и wrapper search/replace, Unicode lowercase expansion, whole-word, все stories, protected field rollback/skip, limits/stale/no-op/redo, block and row order/IDs/undo, section and merge отказ. Сохранение/переоткрытие замененного документа проверяется существующим writer. Логи: target/editing-operations/.

| Контракт | Тест и критерий приемки |
|---|---|
| Поиск текста | Через runs, Unicode lowercase expansion и whole word: точные ranges/slices; исходная модель неизменна |
| Поиск метаданных | ParagraphStyle, CharacterStyle, Location и HasComplexField: точный список адресов; неизвестное значение дает пустой результат |
| Scope | Body, headers/footers, notes, таблицы, SDT, shape text boxes; Story/Paragraph ограничивают результат и замену |
| Массовая замена | Все совпадения меняются одной revision; wrapper properties и структурные символы сохраняются; один undo восстанавливает исходную модель |
| Защищенные совпадения | Strict отклоняет batch целиком; EditableOnly меняет доступные совпадения и перечисляет skipped |
| Лимиты и история | Stale revision и каждый лимит дают ошибку без мутации; no-op и отказ сохраняют redo |
| Перемещение | Точный порядок блоков/строк, конечный адрес/индекс, новые paragraph IDs и точный undo; проверены SDT, cell и text box |
| Невалидная структура | Перемещение section boundary или restart/continue merged row отвергается существующим validator; документ остается исходным |
| Сохранение | Writer/parser roundtrip и facade save/reopen сохраняют замененный текст и порядок абзацев |
| Чувствительность тестов | Компилируемая заглушка Operations при неизменном ядре проваливает все 15 поведенческих тестов |

Фактическая локальная приемка: editing all features — 52/52 теста; без default features — 42/42; save-only — 46/46; visual-only — 48/48; facade editing — 2/2. Clippy editing all targets/all features и facade lib/editing test: -D warnings. Исходный RED: 4/4 compiled tests падают; итоговая обратная проверка: 15/15 падают на поведении, не на компиляции. Внешний CI и ручное открытие в Word этой очередью не проверены.

Воспроизводимая обратная проверка: `python xtool/check_editing_operations.py` (Python 3.11+, cargo +1.92.0). Она создает отдельную копию в target/editing-operations/disabled-adapter, подменяет только Operations, сверяет побайтовую неизменность остальных editing исходников и требует падения каждого теста operations. Исходники рабочего дерева не подменяются. Логи green.log, feature-{none,save,visual,all}.log, facade.log, clippy.log, facade-clippy.log и disabled-adapter.log лежат в target/editing-operations.

За границей этой очереди: UI, regex, Unicode normalization/full case folding, вычисление полей/оглавления, cross-container moves, merge/split cells, новая графика и механизмы модели. Они требуют отдельного контракта или расширения ядра. Визуальные дефекты корпуса закрываются параллельным аудитом и здесь не объявляются исправленными.

Реализация адаптера зафиксирована коммитом `015e0c7`. Полное покрытие исходного списка оставшихся возможностей, архитектурные решения и порядок следующих работ зафиксированы в [EDITING_KERNEL_EXTENSION_PLAN.md](EDITING_KERNEL_EXTENSION_PLAN.md).
