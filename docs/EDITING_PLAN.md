# Редактирование StrictLib: реализация и приемка

Дата: 4 октября 2026 года. Область — библиотечный editing слой над существующим WML Document. Связанный план: [A01–A21 / F00–F21](audit-2026-10-04/FIX_PLAN.md). Зеленые editing-тесты не закрывают визуальный аудит.

## Архитектура

strict-ooxml-edit изменяет тот же strict-ooxml-wml::model::Document, который читают parser, writer и renderer. Второй модели, serializer и pagination нет. Editor эксклюзивно заимствует &mut Document; document() дает только чтение. Команды исполняются на временной копии, валидируются и фиксируются атомарно. Undo/redo восстанавливают снимки без повторной нормализации.

Минимальное ядро (--no-default-features) зависит от WML/core. Feature save подключает существующий writer; visual — SVG renderer и unicode-segmentation. Обе включены по умолчанию. Facade предоставляет opt-in feature edit, StrictDocument.edit() и re-export strict_ooxml::edit. Исходный Package остается у facade и используется для медиа, отношений и provenance.

Прежний EditSession сохраняет узкий контракт E01 и точечные снимки абзацев. Полный Editor хранит снимки Document для атомарных изменений разных stories и контейнеров. Медиа-байты остаются во внешнем Source. По умолчанию сохраняются 100 транзакций и не более 64 MiB оценочного объема снимков (with_history_byte_limit). Оценка — длина потокового UTF-8 Debug представления обоих состояний, а не точный RAM budget. Слишком большая одиночная транзакция отвергается; самая старая undo-история удаляется первой. При отключенной истории снимки не удерживаются.

В renderer добавлен публичный helper выбора mapped font family для visual adapter; алгоритмы layout/pagination этой очередью не меняются. Writer изменен точечно для учета успешно переименованных media parts (E04). Остальные параллельные правки аудита сохранены.

## Общий протокол

transact(expected_revision, commands) выполняет команды последовательно; адреса/offsets следующей команды относятся к результату предыдущей. Ошибка сохраняет документ, revision и обе истории. Содержательная правка, undo и redo увеличивают revision; stale revision отвергается. No-op сохраняет redo/revision. Revision и VisualMap действуют в пределах одной открытой сессии: новая сессия требует новой карты взаимодействия.

Диапазоны — Unicode scalar offsets [start,end). UTF-16 адаптер отвергает середину surrogate pair. XML-invalid controls и CR/LF/tab в строках запрещены; структурные символы задаются typed content. Лимит затрагиваемого абзаца — 1 000 000 scalars, включая inline wrappers.

ChangeSet инвалидирует layout/support. E01 возвращает точные body indices; структурный Editor консервативно инвалидирует весь body, в том числе при изменении другого story. Физические номера страниц не выдумываются. После правки SupportModel устаревает. Facade отражает это через support_is_stale() и запись edit.support-stale в отчете; прямой document_mut() также ставит stale-флаг.

## E01 — текст и direct formatting

Command::ReplaceText/Format: наследование вставки от левого символа (при начале — правого), сохранение prefix/suffix properties, direct bold/italic/underline/character style. Run разделяется только на границе диапазона; exact undo сохраняет границы, xml:space и metadata. Fields/drawings/hyperlinks/bookmarks/tabs/breaks/opaque/revisions вызывают типизированный отказ узкого API; сложный абзац не превращается в plain text.

Приемка: 14 тестов; Unicode splice сравнивается с независимым Vec<char>::splice. Проверяются exact undo, rollback, no-op/redo, лимиты, стили, ids и секции. Первоначальный scaffold: 6/6 RED; обратный scaffold: 14/14 RED. Логи: target/editing-e01/.

## E02 — адреса и структурные команды

Stories: Body, HeaderFooter(PartId), Footnote(id), Endnote(id). Containers: table row/cell, block SDT, shape text box, включая graphic groups. Address относится к revision; Identify и find(story, ParaId) позволяют повторно найти абзац после вставки.

Команды: Text/Format, Split/Join, Insert/Delete block, ParagraphProperties, RunProperties, TextNode, InsertInline/DeleteInline, Identify. TextNode меняет отдельный текст внутри hyperlink/SDT/directional wrapper, сохраняя соседние tabs/breaks/drawings. Plain Text/Format/Split/Join отвергают структурный контент. Fields и tracked revisions защищены от изменения кэшированного текста.

Новые абзацы получают уникальные восьмизначные hexadecimal ParaId и виртуальную SourceLocation /__strict_edit__/paragraph_<id>. Это логическая идентичность нового узла, не позиция исходного XML; она предотвращает совпадения numbering keys renderer. Identify существующего абзаца сохраняет SourceLocation. Идентичность действует в живой модели; persistence ParaId после Strict serialization не обещается.

Split переносит sectPr вправо. Join через section boundary и Delete блока с sectPr запрещены. Удаление последнего body block оставляет пустой paragraph; ячейка сохраняет обязательный последний paragraph. Exact undo восстанавливает identities и порядок stories.

Приемка: nested cells/SDT/text boxes, identities после вставок, секции, совместный undo header/note, rich text и atomic rollback. Первые четыре теста были RED на компилируемом scaffold: target/editing-e01/structured-scaffold.rs, structured-red.log.

## E03 — инварианты и support

Validator вызывается при открытии сессии и перед commit/save. Проверяются nesting limits; существование и тип paragraph/run/table styles; basedOn cycles и cached chain; next/link; numbering definitions/levels; section/header/footer references; identity uniqueness; XML text; note references; bookmark/comment range и complex field boundaries; обязательные строки/ячейки/последний paragraph; положительные explicit grid widths/span, grid overflow и vertical merge topology; media references, drawing extents и вложенные text boxes; неотрицательные frame dimensions.

Это validator представленной модели, не полный ECMA-376 XSD validator. Opaque механизмы не исправляются автоматически. Создание глобальных style/numbering/note definitions и принятие tracked changes не входят в командный контракт.

refresh_support использует существующий writer/parser и меняет только SupportModel, сохраняя живую модель и историю. Сохранение с потерями не очищает stale-флаг. Undo/redo после refresh снова делают support устаревшим.

Приемка: invalid model/style/boundary/merge отказ без мутации; свежий support сравнивается с отдельно переоткрытым DOCX; history работает после refresh.

## E04 — сохранение и provenance

save принимает revision, исходный Source, WriteOptions и SavePolicy. Возвращает DOCX bytes, PipelineSummary и свежий parser support; запись файла остается решением вызывающего приложения. Lossless отвергает Degraded/Failed; AllowDegraded возвращает Degraded с полным отчетом и также отвергает Failed. Ранее зарегистрированные issues сохраняются через with_pipeline; facade автоматически добавляет normalization report исходного Package.

Результат переоткрывается с теми же resource limits. Исправлен интеграционный дефект W7: media part, успешно скопированный под новым именем, учитывается как сохраненный. Реально отсутствующий source part продолжает попадать в W7.dropped-part.

Приемка: четыре save-теста проверяют Unicode/xml:space, source PNG bytes и измененный extent после reopen, исходные/writer losses, policies/stale revision и undo после refresh. Отдельный writer regression различает renamed copy и orphan; существующие 17 passthrough-тестов проходят.

## E05 — каретка, выделение и hit testing

VisualMap строится для revision/RenderOptions/Source через общий place_pages и FontProvider. TextPosition хранит Address, inline path, content index и scalar offset. Каретка и glyph rectangles используют метрики renderer; графемы определяют допустимые границы внутри text node. selection_between проходит через runs/абзацы одного story, принимает обратное выделение и отвергает cross-story/неверные endpoints. Hit testing отвергает NaN/Infinity. Повторяемый header может иметь несколько физических прямоугольников.

Источник размещенного текста устанавливается уникальными маркерами в одноразовой копии модели. Живая модель неизменна; точное совпадение страниц и всей item geometry с обычным placement обязательно. Несовпадение дает Attribution, адрес не угадывается по одинаковому тексту. Пустой plain paragraph имеет виртуальную TextPosition с пустым inline path; zero-width probe существует только в копии. Проверяются нулевая ширина probe и неизменность остальных placements. Ввод выполняется через Text с диапазоном 0..0.

Приемка: шесть visual-тестов проверяют actual placement, emoji/surrogate pairs, combining grapheme, caps expansion, одинаковый текст, marker collision, пустой paragraph, table wrapping/144 DPI, выделение между абзацами и stale revision. Первые два теста были RED на компилируемом scaffold: target/editing-e01/visual-red.log.

## E06 — таблицы, рамки и рисунки

TableProperties, CellProperties, Grid, InsertRow/DeleteRow, Frame и Drawing replacement — typed команды над существующими свойствами, включая размеры/позицию, wrap и merge topology. Несколько связанных merge cells меняются одним валидируемым batch. Вставленные blocks/rows получают новые paragraph identities.

DrawingPosition/VisualMap возвращают прямоугольники картинок/форм и hit selection последнего painted объекта. Group имеет дочерние rectangles под общей root identity; text boxes редактируются по отдельным paragraph addresses. Изменения геометрии и текста имеют exact undo. Табличный текст использует общий caret protocol.

Приемка: table/frame/shape-textbox tests и image integration проверяют model values, atomic rejection, undo, координаты renderer, исходные пиксельные байты и reopen extent.

## Прогоны и доказательства

Реализованы библиотечные контракты E01–E06 и facade integration. Fixtures генерируются тестами; Word lock-файлы и игнорируемый корпус не сканируются.

| Проверка | Результат |
|---|---|
| Editor, all features | 37/37 поведенческих тестов |
| Facade edit/preview/save/refresh/reopen | 1/1; также без default features |
| Writer renamed/orphan и passthrough | 18/18 |
| Core / save-only / visual-only | Отдельные test-прогоны |
| Clippy editing, all targets/features | -D warnings |
| Обратный compiled no-op контроль | 37/37 compiled tests падают на assertions; compile failure не засчитывается |

Логи: target/editing-full/green.log, feature-core.log, feature-save.log, feature-visual.log, facade-feature.log, mutation.log. Изолированная обратная копия: target/editing-full/mutation/; рабочие исходники не подменяются. Новый тест требует повторного обратного прогона. Команда: cargo +1.92.0 test --manifest-path target/editing-full/mutation/Cargo.toml --offline --tests --no-fail-fast.

## Границы результата

Это библиотечный editing слой, не UI desktop-редактора. Точность верстки зависит от общего renderer и отдельно принимается по F06–F19. Неподтвержденный placement возвращает ошибку, не выдуманный прямоугольник. Drawing hit test использует axis-aligned placed boxes, не точный contour/alpha test. Графемы проверяются внутри text node; consumer не должен создавать grapheme fragments между узлами при вводе.

Полный XSD, открытие результата в Word и ручная визуальная приемка корпуса этим прогоном не подтверждены. Clean означает отсутствие зарегистрированных потерь, не обещание сохранения неизвестных механизмов или совпадения с Word. Аудит A01–A21 и весь исходный Stage-10 roadmap не объявляются закрытыми. Коммиты — только после отдельного разрешения пользователя.

Итоговый протокол с SHA-256 исходников и логов: target/editing-full/receipt.json. Отдельный compiled writer mutant также проваливает 1/1 regression. Feature matrix добавлена в CI; внешний CI этой сессией не запускался. Clippy проверен для всех editing targets/features, facade library/editing test и writer library/renamed_media test.
