# Расширение ядра редактирования StrictLib

Дата: 5 октября 2026 года. Статус: архитектурные решения и план реализации; код этой очереди еще не написан. Основание: исходный список оставшихся возможностей в обсуждении, [Stage-10](../STAGE-10-TASK.md), [E01–E06](EDITING_PLAN.md), [Operations](EDITING_OPERATIONS.md). Базовые коммиты: `efccd98`, `015e0c7`. Визуальный аудит остается самостоятельной приемкой.

## 1. Цель и полнота покрытия

Целевая цепочка: открыть поддерживаемый документ → проверить/явно починить → создать определения и ресурсы → отредактировать текст, структуру и секции → принять/отклонить правки → обновить поддерживаемые поля/оглавление → объединить документы → получить DOM diff и EditReport → сохранить Strict → открыть и проверить результат. Все изменения согласованы с undo/redo, источниками байтов, ссылками и отчетом поддержки.

Полное покрытие списка означает выполнение каждой строки следующей матрицы. Это не обещание поддержки всех конструкций Word. Неизвестные механизмы сохраняются, если существующий pipeline умеет их переносить; ненадежное преобразование явно отвергается. Такой отказ не засчитывается как реализация обязательного положительного сценария.

| Исходный пункт | Состояние на базе | Очередь |
|---|---|---|
| Поиск по тексту/стилю, replace-all с отчетом | Реализовано Operations | Регрессии K00/K12; перенос между документами K10 |
| Создание/удаление стилей и списков | Определения есть в модели, команд управления нет | K02 |
| Создание/удаление сносок и колонтитулов | Stories читаются; определения/ссылки не управляются Edit | K03/K04 |
| Добавление новых медиа-байтов | Геометрия/ссылки редактируются; Source остается исходным | K05 |
| Поля/ориентация/границы секций, подключение колонтитулов | Свойства представлены; boundaries защищены командами | K04 |
| Split/join с hyperlinks, полями и рисунками | Plain split/join отвергают сложное содержимое | K06 |
| Обновление полей и оглавления | Cached results защищены от текстовой замены | K08 |
| Accept/reject tracked changes | Content revisions представлены; команд решения нет | K07 |
| Repair модели и конфликты ID | Editor требует валидный вход | K01/K11 |
| Объединение документов / перенос содержимого | Связной операции импорта нет | K10 |
| Полный DOM diff и структурированный EditReport | ChangeSet/pipeline summary не являются diff/EditReport | K00/K12 |
| Клавиатура, IME, clipboard, панели, drag-and-drop | UI не реализован этой работой | K13 контракты ядра; K14 consumer |
| Идентичность после reopen, графемы через узлы, точный object hit | Ограничения E02/E05 | K06/K13/K15 |
| Полный XSD и ручная проверка в Word | Не подтверждены editing очередью | K15 |
| Cross-container moves, merge/split cells из Operations | Не входят в текущий адаптер | K06/K09 |

Regex, normalization и full Unicode case folding не требуются исходным списком: literal search сохраняет документированный контракт. Новые изображения и поддерживаемые typed shapes входят в план; редактор chart/SmartArt/OLE, произвольные Word field codes и HTML/RTF clipboard импорт не обещаются.

## 2. Проверенная база

Сверены Edit, Editor::transact, validator/save, Document, StyleTable, NumberingTable, NoteTable, MediaIndex, SectionProperties, Revision и writer Source. Graph project `D-projects-StrictLib`, Tier 2, generation `2026-10-04T21:57:47Z`. Coverage не сообщает зарегистрированных пропусков, freshness — metadata_changed; существенные выводы проверены исходниками. Graph не показывает call edges для Editor::transact: clone/apply/validate/snapshot/commit проверены непосредственно; отсутствие ребер не означает отсутствие вызовов. Это проверка точек расширения, не новый аудит всего проекта.

Точки опоры: [structured.rs](../strict-ooxml-edit/src/structured.rs), [save.rs](../strict-ooxml-edit/src/save.rs), [operations.rs](../strict-ooxml-edit/src/operations.rs), [document.rs](../strict-ooxml-wml/src/model/document.rs), [styles.rs](../strict-ooxml-wml/src/model/styles.rs), [numbering.rs](../strict-ooxml-wml/src/model/numbering.rs), [notes.rs](../strict-ooxml-wml/src/model/notes.rs), [props.rs](../strict-ooxml-wml/src/model/props.rs), [drawing.rs](../strict-ooxml-wml/src/model/drawing.rs), [Source](../strict-ooxml-write/src/package.rs), [ADR-0018](adr/0018-revisions.md).

Stage-10 содержит исторические находки. Они не объявляются актуальными только по старому тексту: перед изменением механизма фиксируются текущий baseline и воспроизводящий тест. Закрытые E01–E06 и Operations не переписываются без обусловленного новым контрактом изменения.

## 3. Архитектура и общие контракты

### Один Document и атомарное состояние ресурсов

Сохраняется один wml::Document для parser/writer/renderer. Второй DOM/serializer не создается. Внутренний EditState объединяет Document, IdentityIndex, ResourceOverlay, derived-cache stamps и journal. Document публично читается; изменяется командами.

Сохраняется совместимый Editor::new(&mut Document, limits). Добавляется сессия с заимствованным EditContext; facade владеет context между сессиями. Legacy save(source) остается. Media/import команды без context возвращают ContextRequired; старые операции работают как прежде. Context хранит добавленные/замененные/явно удаленные resources и side metadata; исходный Package неизменяем.

Commit атомарно публикует модель, overlay, identities и journal. Undo/redo восстанавливают весь aggregate. Payload хранится как Arc<[u8]>; история учитывает уникальные удерживаемые buffers, а не только Debug модели. No-op сохраняет revision/redo. Промежуточный batch может быть несогласованным, final candidate — нет; локальные preconditions проверяются немедленно. Engine не выполняет сеть, публикацию файлов и открытие Word.

Дополнительные defaults: 200 000 новых/индексируемых nodes на операцию; 100 000 команд; 10 000 новых resource parts; 32 MiB одного нового payload и 128 MiB суммарно; 64 уровня dependency traversal; 64 000 000 посещений/сравнений; 8 field/layout итераций. Применяется более строгий из этих лимитов и действующего ResourceLimits. История сохраняет текущие defaults 100 транзакций/64 MiB с новым учетом buffers. Размер поиска сохраняет defaults Operations. Временный candidate ограничен отдельно; нельзя получить неограниченную память при выключенной истории.

### Revision, plans, identity и ошибки

Mutating API принимает expected revision. Долгие операции создают immutable EditPlan с base revision, context token, policies, dependencies, ресурсами и findings. Apply проверяет tokens и candidate, не публикует частичный результат. Plan не меняет историю.

IdentityIndex добавляет session NodeId для blocks/paragraphs/inline nodes, отдельно от ParaId/SourceLocation/XML пути. Move сохраняет NodeId, copy создает новый, split оставляет левый ID и создает правый, join сохраняет левый и возвращает mapping правого. Удаленный ID внутри session не переиспользуется; undo восстанавливает его. Address/ParaId API совместим; новые команды возвращают явный remapping.

Новые стабильные diagnostic codes: InUse, ConflictingDefinition, InvalidSection, BoundaryCrossing, UnsupportedField, RevisionConflict, UnresolvedResource, OpaqueDependency, NonConvergentLayout, StalePlan, ContextRequired, LimitExceeded. Они содержат адрес/NodeId. Существующие APIs сохраняют EditError; подробности предоставляет новый расширяемый тип/diagnostics. Новые публичные enums — non_exhaustive.

Default удаления используемого definition = RejectInUse. Replacement refs требует явной совместимой цели. Неиспользованные definitions и opaque parts автоматически не удаляются; GC не является побочным эффектом правки. Force, молчаливое усечение и подстановка похожего ID запрещены.

### Validation, derived state, отчет и features

Общий validate_document размещается в WML: writer не зависит от edit. Ordered findings/summary проверяют представленную модель, refs/resource inventory с заданными limits; context checks остаются в edit. Final Error = отказ. Warning не означает автоматически допустимый lossless save.

Style chains, numbering redirects, sections и current support пересчитываются в candidate; первичные данные не заменяются cache. Resolve идемпотентен, counters не растут от повторного запуска. Provenance входа хранится отдельно и не исчезает при refresh. Для секций одна canonical transformation согласует paragraph sectPr и ordered Document.sections; они не меняются независимо. Final section существует всегда, boundaries разрешены только в body.

EditReport — отдельный JSON документ, schema_version 1.0, edit-report.schema.json. Support-report schema не меняется. Поля: session/transaction IDs, before/after revision и validation, applied operations, skipped/rejected items, ID/resource remapping, findings, invalidation, losses, provenance. JSON — opt-in report feature.

Минимальное ядро не зависит от writer/renderer. Overlay содержит neutral bytes/relationship metadata, реализация Source подключается save feature. fields feature использует общий layout для paginated fields; text/metadata fields доступны headless. visual обслуживает interaction. Новые runtime dependencies минимальному ядру не добавляются; для отдельного feature новая зависимость требует ADR/lock/license gate.

Operations::move_block сохраняет прежний copy/delete контракт с новыми ParaId. Новый MoveNodes явно сохраняет NodeId; замена поведения существующего API не допускается.

## 4. Очереди реализации

Каждая очередь включает production changes, behavioral tests, compiled inverse, документацию и receipt. Новые имена модулей/тестов ниже — запланированные файлы. Исполнитель пишет код по принятым решениям; архитектурное отклонение документируется до реализации, не скрывается сменой приемки.

### K00 — context, identity, transaction evidence

**Задача.** Ввести aggregate и подробный результат, сохранив legacy API. Execution engine обслуживает новые/старые команды, facade sessions не теряют resources. Foundation API transact_with_resources принимает ResourceDelta и существующие Edit commands; delta включает metadata/bytes/rels, а использование изображения задается существующим Drawing/InsertInline. Удобные AddMedia/InsertImage появляются в K05.

**Владение:** edit structured/context/identity/report, facade edit adapter; tests context.rs, identity.rs; ADR о state/reporting.

**Тесты:** batch меняет текст, drawing reference, media metadata и staged payload; ошибка последней команды сохраняет точное исходное состояние, journal и обе истории. Success/undo/redo проверяет buffers/IDs. Закрытие/reopen session facade не теряет payload. Проверить monotonic allocator, исчерпание revision/ID, no-op с redo, context byte limit. Definition atomicity добавляется в K02 на его публичных командах. Stub публикации ломает положительный тест; mutant rollback — отрицательный.

**Приемка:** прежние 52 editing/2 facade сценария сохраняются; новые tests доказывают aggregate atomicity. Ресурсная команда не возвращает success без context. Journal отражает выполненные команды, а не входной plan.

### K01 — общий validator и мутационные primitives модели

**Задача.** Перенести общие инварианты в WML; корректные remove/replace/get_mut для StyleTable, NumberingTable, NoteTable, MediaIndex. Maps/cache согласованы с порядком хранения. SupportModel/ThemeColors обновляются специализированным recompute/replace, не произвольной правкой counters.

**Владение:** WML validation/model/resolve, thin edit adapter, writer validation entry point; tests validation.rs, model_mutation.rs.

**Тесты:** удалить первый/средний/последний definition, сравнить все lookups/iterators с независимым набором. Resolve дважды идентичен. Отдельные invalid fixtures: missing/wrong-kind/cyclic style; numbering level 9; dangling note/section; duplicate identity; mismatched fields/ranges; cell topology; absent payload. Edit/writer/render preflight дают одинаковые codes. Невалидная модель не вызывает panic при записи.

**Приемка:** writer default path отказывает Error-модели; edit использует тот же validator и явные ResourceLimits. Валидное невыразимое содержимое сохраняет existing loss policy; invalid structure не превращается в допустимый loss record.

### K02 — стили и списки

**Задача.** Create/Replace/DeleteStyle, SetDocDefaults, Create/Replace/DeleteAbstractNum/Num, Apply/RemoveList, RestartList. Create collision = conflict, replace требует существующий ID. Delete: RejectInUse или ReplaceReferences(target). Walker учитывает все stories, styles basedOn/next/link, table style, numbering style links/levels. NumId=0 не выделяется как новый instance.

**Владение:** edit definitions.rs, WML tables/resolve, writer styles/numbering; tests definitions.rs.

**Тесты:** создать/apply/change/delete paragraph/character/table style; различить explicit/inherited props. Delete in-use отказывает, replacement remaps каждый вид refs. Cycle/wrong-kind rollback. Multi-level restart/override дает независимо заданную последовательность labels после edit/reopen.

**Приемка:** undo включает defaults/chains/redirects; сохранение не меняет style semantics и labels. Unused definitions остаются и отражаются в report.

### K03 — footnotes/endnotes

**Задача.** Create/Replace/DeleteNote, Insert/RemoveNoteReference. Оба представления ref — Inline и RunContent — учитывает один walker. Normal ID положительный; reserved separators исключены из allocator. Delete in-use = отказ; RemoveAllReferences явно убирает все refs в batch. Contents меняются existing Story API.

**Владение:** edit notes.rs, WML NoteTable, writer notes; tests notes.rs.

**Тесты:** обе ref формы в разных stories; delete без policy InUse; explicit удаление и undo восстанавливает definition и все refs. Normal/separator IDs не пересекаются. Reopen notes/rels; limit/stale/последняя ошибка не оставляет пустой definition.

**Приемка:** ref counts/maps совпадают с независимым обходом; reachable notes записываются/переоткрываются. Удаление paragraph не запускает скрытый note GC.

### K04 — секции и колонтитулы

**Задача.** SetSectionProperties, Insert/RemoveSectionBreak, Create/Replace/DeleteHeaderFooter, Attach/DetachHeaderFooter. Boundary адресуется NodeId body paragraph, final section — SectionHandle. Margins/size/orientation/columns/page numbering меняются typed properties. Remove boundary выбирает KeepLeftProperties (default) или KeepRightProperties; move требует CarrySectionBoundary.

Attach указывает Header/Footer и Default/First/Even slot, согласованно создает part/relationship. Detach удаляет локальную ссылку и оставляет WML наследование предыдущей секции. AttachEmpty создает отдельную пустую часть. LinkToPrevious/DifferentFirst/EvenOdd flags управляются явными командами; detach нельзя называть «скрыть колонтитул».

**Владение:** edit sections.rs, WML section resolve, writer section/header-footer, facade overlay; tests sections.rs.

**Тесты:** 2–3 sections, независимые portrait/landscape размеры/margins, все slots, shared/inherited/empty parts. Delete referenced part InUse; detach одной ссылки не удаляет shared part. Split/remove/undo согласует оба section representations. Layout golden: page/body/header/footer bounds без пересечения, после связанных audit gates.

**Приемка:** model + rels + reopen совпадают по sections/slots/dimensions. Визуальная приемка не закрывается при непрошедших layout gates.

### K05 — resource overlay и новые изображения

**Задача.** AddMedia(bytes,type,kind), ReplaceMedia, DeleteMedia(policy), InsertImage(handle,geometry). Overlay реализует весь Source: read_part, relationship(s), content_type, parts, bounded reachability; priority overlay → original. Tombstone запрещает fallback к старым bytes. Имена частей deterministic, учитывают original Package и case-insensitive collisions.

AddMedia default не deduplicates; explicit deduplicate сравнивает bytes+type+kind. ReplaceShared требуется для общего handle. Replace одной картинки создает новый handle и remaps ровно этот drawing. Delete in-use = отказ, каскад перечисляет потребителей явно. Unknown opaque refs = OpaqueDependency. External URI не загружается, automatic GC отсутствует.

**Владение:** edit resources.rs/context snapshots, writer Source adapter, facade render/save; tests resources.rs.

**Тесты:** новый PNG/JPEG сохраняется без original media part; reopen bytes идентичны. Original и overlay image preview работают вместе. Replace one/shared, delete/undo, byte budget, MIME mismatch/corrupt decode, case collision, tombstone/foreign rels. Fault injection последнего resource read дает rollback. Бюджет истории считает unique Arc buffers.

**Приемка:** новый resource не запрашивается из original Package. Unsupported decode не засчитывается как корректный preview; bytes сохраняются подтвержденно либо есть явный refusal/loss.

### K06 — rich fragments, split/join и move

**Задача.** RichPosition(NodeId,content,scalar,affinity), Fragment дерева с boundary metadata, typed splice, SplitRich/JoinRich, CopyNodes/MoveNodes. Hyperlink/SDT/directional wrapper можно разделить с сохранением props; drawing неделим. Simple/complex field переносится целиком; split внутри instructions/result дает BoundaryCrossing. Plain text не заменяет дерево. Revision-bearing fragments остаются protected до K07. Общий bounded field inventory/instruction AST вводится здесь для безопасного bookmark remapping K10; вычисление cache появляется в K08. Неизвестный instruction с неразрешимой зависимостью запрещает remap, не переписывается regex.

Bookmarks/comments, пересекающие split, сохраняют один range через paragraphs одного story. Copy/move переносит обе границы либо отказывается от половины range. Opaque node переносится целиком с resource closure. Join через section boundary требует policy K04. Cross-container move корректирует пути по NodeId, запрещает ancestor→descendant и сохраняет обязательный final paragraph source/destination cell.

Cross-story move remaps story-local IDs/rels, сохраняет NodeId. Copy выделяет новые IDs/bookmark names и переписывает field refs к копируемым bookmarks. External-to-fragment bookmark dependency default RequireTarget; explicit PreserveDependency возвращает finding и требует существующую однозначную цель. Совпадение имени не означает разрешенную подстановку.

**Владение:** edit fragment.rs/rich.rs/move.rs, identity/reference walker; tests rich.rs/moves.rs.

**Тесты:** split/join hyperlink с TAB, drawing и field рядом; exact tree undo. Whole field transfer проходит, internal split падает. Cross-paragraph bookmark, partial comment range, nested SDT/table/textbox. Cross-cell/story, descendant destination, section policy, пустая source cell и placeholder. Независимо проверить порядок, NodeId и ресурсный closure, не Debug contains.

**Приемка:** positive cases работают без flatten/loss; package rels/boundaries корректны после reopen. Новый Move сохраняет identity и не меняет старый Operations move.

### K07 — accept/reject revisions и история свойств

**Задача.** Accept/RejectRevisions(selector), TrackEdit(fragment,author,date), paired moves. Часы не читаются engine: author/date передает caller. Insert accept оставляет content и снимает marker, reject удаляет. Delete accept удаляет, reject снимает marker. MoveFrom/MoveTo решаются парой; missing/ambiguous pairing = RevisionConflict. Paragraph-mark decision выполняет отдельный splice, не удаляет весь paragraph по одному marker.

ADR-0018 не моделирует property-change history и полноценную pairing moves. Для полного accept/reject нужен ADR extension: typed previous-property snapshots, revision group/range identity, parser/writer roundtrip. История не выводится из текущих props задним числом. Lost input history остается finding, не восстанавливается выдуманными данными. Нужны также pPr/rPr/table/row/cell/section property history для тех представленных свойств, которые эта очередь объявляет reviewable; capability registry фиксирует каждый тип.

**Владение:** WML revision/props/parse, edit revisions.rs, writer wrappers, renderer Final/Original; tests revisions.rs + parser/writer regressions.

**Тесты:** independently expected Final/Original до/после каждой kind, paired move через paragraphs, paragraph-mark join, previous/current property snapshots каждого обязательного типа, crossing field/section, missing pair и malformed nested history rollback. Subset сохраняет другие markers; accept all идемпотентен; save/reopen сохраняет review metadata до решения.

**Приемка:** одних Run::revision недостаточно. Проверено фактическое дерево, previous props и paragraph marks. Unsupported выбранная history отклоняет strict batch целиком; отказ не закрывает обязательную supported capability.

### K08 — поля и оглавление через общий layout

**Задача.** Evaluator использует field inventory/AST K06; mandatory PAGE, NUMPAGES, SECTIONPAGES, REF, PAGEREF, SEQ, DATE, TIME, TOC. Simple/complex syntax и nested fields. DATE/TIME принимают timestamp/locale; внешние links/macros не исполняются. Unknown codes/switches защищены и дают UnsupportedField, instructions не стираются. Locked fields по умолчанию skipped с отчетом; override lock только explicit option. Strict unsupported selection отказывается атомарно.

Обязательный профиль TOC: heading/outline levels 1–9, `\\o`, `\\u`, `\\h`, `\\n`, `\\z`, `\\t`, `\\b`; tab leaders и right-aligned page column задаются paragraph tab properties. `\\t` содержит явную карту style→level, `\\b` ограничивает bookmark range, `\\z` сохраняется и влияет только на отдельный web-view режим, не на paginated output. REF: bookmark text и hyperlink switch `\\h`; PAGEREF: bookmark page label/`\\h`; SEQ: named counters, increment, current `\\c`, reset `\\r`; PAGE/NUMPAGES/SECTIONPAGES: decimal/upperRoman/lowerRoman/upperLetter/lowerLetter из section numbering и явный format switch. DATE/TIME: `\\@` с профилем yyyy-MM-dd, dd.MM.yyyy, MMMM d, yyyy, HH:mm, HH:mm:ss; locale en-US/ru-RU, default en-US/UTC, явный timestamp/offset от caller. Field AST содержит unsupported tokens, evaluator не игнорирует их. Для `\\n` TOC документируется уровень/диапазон подавления page labels.

Новый TOC cache сохраняет existing paragraph/run styles; new entries используют явно заданные TOC styles, создавать их можно через K02 при explicit allow-create. Heading inclusion не берется из похожего названия style: используются resolved outline и заданные style IDs.

Calculation: semantic fields → layout → paginated fields/TOC → повторный layout. Commit только fixed point page map и relevant cache, максимум 8 iterations; oscillation = NonConvergentLayout, весь batch откатывается. Один renderer/font provider; page label не угадывается по длине текста. Fingerprint включает options/fonts/revision/resource token. Fields без page dependency работают headless; paginated evaluator без fields/layout feature дает ContextRequired.

**Владение:** WML fields при необходимости, edit fields.rs, shared renderer field/layout service, writer cache; tests fields.rs.

**Тесты:** semantic REF/SEQ/DATE oracle; explicit page breaks и section starts с независимыми PAGE/NUMPAGES/SECTIONPAGES/PAGEREF labels. TOC levels/hyperlinks/bookmarks/leaders/page labels, каждый mandatory switch. Nested/locked/unknown fields, длина source меняет pages, forced oscillation rollback; options/fonts change делает plan stale; exact undo instructions/cache.

**Приемка:** semantics + package/reopen + geometry/layout обязательны. TOC/pages не принимаются до связанных audit pagination/container gates. Capability registry явно показывает unsupported switches; accepted TOC profile не выдается за все Word fields.

### K09 — merge/split cells

**Задача.** MergeCells(rect), SplitCell(columns,rows) адресуют logical grid. Merge помещает contents в top-left row-major, final block paragraph. Split сохраняет content только top-left, остальные cells пустые paragraphs. Width sum сохраняется; остаток twips последней колонке. Равномерные widths по умолчанию; explicit widths должны давать исходную сумму. Partial existing merge отвергается; совпавший целый merge допускается. Создание дополнительных rows явно отражено в plan.

**Владение:** edit tables.rs, shared grid invariants; tests table_operations.rs.

**Тесты:** 2x2, horizontal/vertical topology, existing merge, unequal widths, nested table/drawing; независимая occupancy matrix и content inventory. Undo/split exactness, overflow/partial merge/impossible span rollback. Crossing page сравнить со связанным audit table/merge gate.

**Приемка:** content и общий размер не теряются, restart/continue/final p валидны. Ручная правка properties не засчитывается как готовая операция.

### K10 — fragment import и объединение документов

**Задача.** plan_import(donor,donor_source,selection,target,policies), append_document; donor immutable. Definition dedup только semantic equality включая dependencies/defaults. Same ID/different meaning deterministic rename. Num/AbstractNum/notes/bookmark names/IDs/comments/drawing IDs/part names/rels имеют отдельные maps. Все copied NodeId новые.

Closure включает media, selected section headers/footers, notes, field targets, opaque package dependencies. Internal relationship targets при переносе частей переписываются; external URI переносится без загрузки. Одинаковый path в двух Sources не означает одинаковые bytes. Неизвестная target rewrite/unresolved closure = OpaqueDependency, не битая копия.

Comments требуют typed definitions/parts и range binding: marker IDs недостаточно. Минимальное model/parser/writer extension сохраняет body/author/date и relationships. Unknown extensions — opaque closure или explicit unsupported, не исчезают из clean report.

Append default PreserveSections: donor начинается новой секцией с его slots/layout. UseDestinationSection — explicit alternative. Fragment import не заменяет target settings/theme/defaults: зависимости материализуются в imported definitions/properties. Неподтвержденная эквивалентность strict mode отклоняет; AllowDegraded сообщает конкретную потерю. AdoptDonorGlobalSettings допустим только для нового пустого target.

**Владение:** edit import.rs/dependency.rs, WML comments, overlay, writer foreign parts; tests import.rs.

**Тесты:** одинаковые style/list/note/bookmark IDs/media paths с разными meaning/bytes; независимая expected map, target content и unchanged donor. REF/hyperlink donor bookmark, note image/shared footer, chart→embedded closure. Bounded graph и missing dependency rollback; repeated plan deterministic; stale target/context не применяется.

**Приемка:** reopen без dangling refs; каждый payload равен donor bytes либо явно remapped XML с equivalent rels. Простого увеличения block count недостаточно. Private style/numbering caches не переносятся как primary meaning.

### K11 — repair невалидной модели

**Задача.** RepairSession::open сначала проверяет structural/resource limits, затем допускает semantic errors. Editor::new остается strict. plan_repair возвращает proposed changes/findings; apply коммитит только валидный финал, после которого открывается обычный Editor. Непочиненная Error возвращает unchanged input + diagnostics; частичная починка не объявляется валидной.

SafeMechanical: rebuild maps/caches, reassign duplicate paragraph/text IDs с exact anchor map, rebuild sections из valid boundaries, obligatory final paragraph cell. Missing style/note/media, broken field/range, ambiguous revision pair не исправляются угадыванием. DropDanglingReference и ReplaceMissingStyle(target) — explicit actions с loss records вне safe default. Повторный repair no-op. Для successful repair undo допускает восстановление invalid source только в RepairSession, не в обычном Editor.

**Владение:** edit repair.rs, shared inventory, facade bootstrap; tests repair.rs.

**Тесты:** handmade duplicate IDs/cache mismatch/cell без final p, independent expected tree, before Error/after valid. Safe missing media/style/ambiguous field unchanged. Explicit lossy profile, ошибка последнего action, bounded depth bomb, повторный repair/undo/redo.

**Приемка:** success имеет 0 remaining Error, dropped refs никогда не дают false Clean. Report содержит changed/unresolved и исходные anchors. Repair не запускается через уже отвергший вход Editor.

### K12 — DOM diff, EditReport, headless CLI

**Задача.** Exact diff включает represented tree/run boundaries/identities/provenance/definitions/overlay; Semantic diff игнорирует только перечисленные derived caches, physical rel IDs и virtual provenance. Meaningful props/order не сортируются ради равенства. Typed Added/Removed/Moved/Modified с old/new/path, resource maps и ambiguity. Within-session alignment NodeId; cross-session deterministic structural matching возвращает ambiguous groups, не угадывает identity по тексту. Limit = error, не truncated success.

EditReport публикуется после commit; rejected/no-op plans отдельны. Writer/parser losses остаются provenance. Схема §3 не меняет support-report. Report не включает raw payload и содержимое внешних credentials.

CLI принимает versioned JSON plan, input/output/report; validate, repair preview/apply, diff, merge. Exit codes: 0 success, 2 input/plan error, 3 validation/unsupported/lossless refusal, 4 IO/pipeline error. Output создается temp и atomic rename только после checks; отчет и output имеют общий transaction token, отсутствие одного из двух artifacts после publication failure дает failure с recovery metadata. stdout не объявляет файловую атомарность.

**Владение:** edit diff.rs/report.rs, edit-report.schema.json, CLI adapter; tests diff.rs/edit_report.rs/CLI.

**Тесты:** style/default/header/note/field/drawing/media-only изменения видны. Equal text/different props не равны. Run split Exact отличается, Semantic может быть равен. Undo diff пустой, known move отдельный, ambiguous import reported. JSON schema/stable order и все exit codes. Отказ не заменяет final output. Mutant учета удаления должен ломать evidence test.

**Приемка:** сравнение не основано на Debug строках. Каждое intended removal имеет evidence; unsupported placement не маскируется diff. CLI reopened output совпадает с intended result.

### K13 — графемы, ввод/clipboard и object hit: ядро

**Задача.** Общая paragraph projection отображается назад в nodes; grapheme cluster может пересекать adjacent runs/wrappers, structural units разрывают cluster. Grapheme/UTF-16 → RichPosition не разрезает combining mark/ZWJ/surrogate. Multi-paragraph editing использует Fragment splice с section/field policies.

Composition — preview overlay с base revision/range; update не пишет историю, commit один batch, cancel no-op. Внешняя mutation перед commit = StalePlan. Plain clipboard: CRLF/CR нормализуются в paragraphs, TAB в RunContent::Tab. Rich internal clipboard использует Fragment/closure K10. HTML/RTF — отдельные import adapters, не silent plain fallback. Панель сообщает Value/Mixed/Inherit и применяет typed patch.

Hit modes: Bounds совместимый default; Geometry для поддерживаемых vector paths; Alpha для поддерживаемых raster images. Учитываются transforms/clipping/paint order/group children. Unsupported exact mode = CannotHitExactly; fallback только explicit consumer option. Drag preview не меняет модель; commit один geometry delta относительно исходного snapshot.

**Владение:** edit input.rs, visual projection/object hit, shared renderer geometry; tests input.rs/visual_objects.rs.

**Тесты:** combining/emoji ZWJ между runs: caret не внутри, delete удаляет cluster. UTF-16 interior rejection. IME update/cancel без history, commit один undo, concurrent edit refusal. Multi-p/TAB paste сохраняет props/final p. Rotated/group shape и transparent PNG: bounds hit есть, exact hit вне geometry/alpha нет. Drag/cancel/undo original extent/offset.

**Приемка:** positions layout/edit используют одну projection; per-node graphemes недостаточны. Core tests без OS windows/clipboard; attribution mismatch по-прежнему дает отказ.

### K14 — приложение редактора, отдельный consumer

**Задача.** Включить keyboard selection/input, IME, OS clipboard, style/list/section/review/TOC panels и drag preview/commit. UI хранит selection/composition/view state; модель/history/context принадлежат ядру. Revision/layout invalidation rebuild maps; старые coordinates не применяются. Protected content/refusal показывается конкретным diagnostic.

**Владение:** отдельная очередь strict-ooxml-view/выбранного host, без native clipboard/IME в минимальном edit. Файловые границы согласуются с параллельным viewer аудитом.

**Тесты:** headless consumer event stream: keyboard replace, multi-p selection, IME cancel/commit, clipboard paste, mixed format panel, attach header, review, TOC, drag, save/reopen. Затем Windows manual IME/clipboard smoke с steps/expected/artifacts; ошибка не мутирует модель, cancel не создает undo.

**Приемка:** UI строка закрывается после application smoke, не после создания API. Недоступная среда = BLOCKED этой части; library acceptance можно завершить отдельно как library-only.

### K15 — portable identity, XSD/Word, итоговые gates

**Задача.** Strict output не получает w14 identity extensions ради handles. Save возвращает optional IdentityManifest sidecar: document namespace, NodeId, emitted part/path anchor, shape/version, artifact digest. Writer emission map строит anchors по реально записанным nodes; dropped/coalesced mappings явны. Reopen применяет manifest только к точно соответствующему artifact/совместимой schema.

SHA-256 digest provider передает host через context; ядро вызывает его на фактических DOCX bytes и сверяет manifest. Feature не добавляет скрытую crypto dependency минимальному ядру. Без provider/manifest открывается новая identity session без persistence promise. Корректность provider проверяется host тестами по известным SHA-256 vectors; CI artifacts считают digest независимым инструментом. В обычном Word DOCX без sidecar identity не обещается.

Полный XSD gate проверяет весь собранный пакет: измененные и неизмененные XML parts, relationships/content types, WML/DrawingML/OMML/chart/diagram и остальные заявленные schema families, включая перенесенный dependency closure. Schema/content-type inventory определяет валидатор каждой части; неизвестная обязательная schema означает UNVERIFIED/BLOCKED полного gate, не молчаливый skip. Schema/version/checksum закреплены. Package Strict scanner и extension/pass-through inventory отдельны. Clean fixtures имеют 0 XSD errors и 0 непроверенных обязательных частей. Intentionally preserved unsupported baseline документируется degraded; новый edit не добавляет violations и не получает lossless PASS. Нельзя расширять expectations для скрытия ошибки.

Word matrix фиксирует установленную версию/build: open без repair prompt, ожидаемый text/sections/notes/media/lists/review/TOC, Word save/reopen parser + Semantic diff. WPS/LibreOffice не заменяют Word. До Word auto-recalculation снимается исходный результат StrictLib; Word recompute проверяется отдельно.

**Владение:** facade save/open identity, writer emission map, xtool gates/CI, manual fixtures/receipts; tests identity_roundtrip.rs/end_to_end.rs.

**Тесты:** exact DOCX+sidecar восстанавливает handles; same text/other digest отказывает; unknown schema не угадывает IDs. Insert/split/move/save/reopen и следующий edit проверяет mapping. XSD mutant с invalid attr, removed-node mutant и bad relationship ломают gates. End-to-end covers каждую очередь/строку matrix.

**Приемка:** MODEL/ROUNDTRIP/XSD/STRICT/VISUAL/WORD verdicts раздельны и привязаны к SHA/artifacts. Полный PASS невозможен при skipped/blocked mandatory gate. Consumer подтверждает сохранение sidecar отдельно от DOCX; потеря sidecar не считается потерей текста/Strict конформности.

## 5. Порядок выполнения и зависимости

Порядок: **K00 → K01 → K02 → K03 → K04 → K05 → K06 → K07 → K09 → K10 → K11 → K08 → K12 → K13 → K14 → K15**. K08 ждет связанной pagination/TOC приемки аудита; модельные очереди на нее не ждут. K12 использует журнал K00, не создает второй. Field AST K06 доступен K10 раньше evaluator K08. XSD fixtures заводятся с K01, K15 собирает итоговую приемку, а не начинает ее с нуля.

| Очередь | Зависимости | Возможная независимая подготовка |
|---|---|---|
| K00 | Baseline E01–E06/Operations | Fixtures/specs K01+ |
| K01 | K00 | Definition/note/section/resource fixtures |
| K02 | K01 | K03–K05 tests |
| K03 | K02 | K04/K05 при разделенном владении |
| K04 | K00–K03 | Resource/rich fixtures |
| K05 | K00/K01/K04 | Rich text tests |
| K06 | K00–K05 | Review/table test design |
| K07 | K06, revision ADR extension | K09 при независимых файлах |
| K09 | K01/K06 | Import closure planning |
| K10 | K02–K07/K09 | Repair/diff fixtures |
| K11 | K00/K01 и команды K02–K10 | Semantic field fixtures |
| K08 | K02–K07/K10, audit layout gates | Headless diff/report |
| K12 | K00–K11 | Consumer event fixtures |
| K13 | K06/K10/K12, renderer attribution | UI без изменения ядра |
| K14 | K03–K13 | Word matrix preparation |
| K15 | Все обязательные queues/gates | Итоговый closure |

Параллельность не дает совместного владения structured.rs, общей WML моделью, writer package builder и placement. Work order называет точные files и интеграционного владельца; чужие изменения не откатываются. Shared изменение интегрируется до зависимой очереди. Этот документ не запускает исполнителей.

## 6. Тесты должны падать без кода

Сначала вводятся публичные сигнатуры и компилируемый scaffold, затем независимые expected fixtures. RED — только assertion/behavior failure после успешной компиляции и запуска. Undefined symbol, отсутствующий feature, build failure или invalid unrelated fixture не засчитываются. Отрицательный тест имеет положительный противопример: возвращать Err всегда недостаточно.

После GREEN выполняется inverse в отдельной копии: новая реализация заменяется компилируемой заглушкой/targeted mutant. Старые принятые механизмы не отключаются, иначе падение не доказывает новую очередь. Manifest связывает acceptance test → mechanism → mutant → expected assertion. Новый тест требует обновить inverse receipt.

Обязательные группы для каждой очереди:

1. Положительная мутация с independently constructed expected tree/reference/resource inventory.
2. Ошибка последней команды после действительной правки: Document/context/revision/обе истории/journal неизменны.
3. Один undo/redo всего batch, no-op с существующим redo, stale revision/plan.
4. Реальные limits: nesting/nodes/bytes/commands/relationships/iterations/comparisons; limit−1/limit/limit+1. Candidate ограничен даже без undo history.
5. Save→parse semantic equivalence и exact payload bytes; provenance проверяется отдельно, не удаляется для равенства.
6. No-default/feature-only/all-features compatibility, затронутые WML/writer/renderer suites.
7. Compiled inverse и fault injection validation/resource/publication.

Property tests используют независимые Vec splice/grid occupancy/ref maps/revision projections, не копию production функции. Fixtures генерируются без ignored корпуса. Word ~$ lock-файлы не входят в corpus manifest; поврежденный пакет с обычным именем нельзя пропустить как lock. Manifest перечисляет файлы/digest, случайный локальный каталог не определяет приемку.

52 editing/2 facade — исторический baseline, не hardcoded размер будущего набора. Новые сценарии добавляются; assertions/expectations не ослабляются ради GREEN.

## 7. Артефакты и поручение исполнителю

Каждая очередь оставляет:

- ADR/API defaults и capability matrix со supported/unsupported cases.
- Production changes в согласованных files и legacy migration notes.
- Behavioral tests/fixtures/corpus manifest/inverse recipe в repository.
- target/editing-kernel/Kxx/: red.log, green.log, inverse.log, clippy.log, feature logs, receipt с SHA-256 sources/fixtures/logs и actual exit codes.
- docs/editing-kernel/Kxx_ACCEPTANCE.md с выполненными сценариями, метриками, потерями и незакрытыми gates. Target receipt не является единственным долговечным отчетом.

Проверки: cargo +1.92.0 test -p strict-ooxml-edit --no-default-features --locked; соответствующие feature-only/all-features runs; facade integration; затронутые WML/writer/renderer suites; rustfmt; clippy -D warnings. Shared validator/model/Source/placement требует широкого затронутого прогона; markdown-only правка — проверки ссылок/полноты документа. CI/license/XSD оцениваются на точном commit SHA; внешний CI без run/URL/SHA не отмечается PASS.

Work order имеет обязательные поля: Kxx/dependencies, owned paths, frozen API/defaults, allowed model changes, oracle/fixtures, positive/atomic-error acceptance, mutant mapping, commands/artifacts. Исполнитель не выбирает другой deletion/remapping/fallback/serializer самостоятельно; отклонение сначала принимает архитектурный владелец.

Связь с Stage-10: shared validation/model mutation/resolve — E1–E8/E15/E18; repair — E9–E14; diff — E21–E23; операции/merge/sections — E28–E32; writer honesty/CLI/report — E34–E44/E43a; Strict identities/conformance — E45/SC-E14; остаточная XSD приемка — соответствующий отдельный заказ. Эта связь не объявляет все исторические Stage-10 пункты уже выполненными. Точные инварианты/коды существующего проекта сохраняются либо меняются явно в ADR с регрессиями.

## 8. Условие завершения

Library scope завершен, когда K00–K13 и библиотечные K15 contracts имеют production code, behavioral GREEN, compiled inverse и roundtrip, а matrix подтверждена artifacts. Полное покрытие списка завершено после K14 application smoke и обязательных K15 XSD/Strict/Word gates. Недоступный Word/незакрытый audit layout = BLOCKED конкретной части, не заменяется обещанием.

План не объявляет новые команды реализованными. Написание документа не дает разрешения на новый коммит, deployment или публикацию от имени пользователя.
