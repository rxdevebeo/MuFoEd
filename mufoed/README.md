# MultiFormatEditor (MuFoEd)

A desktop demonstrator of the strict-ooxml toolkit: one window, one tab per
document (as in WPS Office), a Word-style ribbon. Windows, Linux and macOS
(egui/eframe, native file dialogs through `rfd`).

```text
cargo +1.92.0 run -p mufoed --release -- [FILE...]
```

What it shows:

- **Open**: DOCX (Strict; Transitional through the normalizer), RTF, HTML and LaTeX (importers).
- **Convert**: PDF and DjVu into an editable Strict document, semantic or
  visual mode, page range, images; scanned pages recognized by a local Ollama
  vision model (feature `ocr`, on by default; endpoint and model in Options).
- **Display**: pages rendered to SVG by the library and rasterized with the
  bundled faces; navigation pane (headings, pages), support report pane.
- **Navigation**: a hand cursor marks headings, page entries, bookmarks,
  document hyperlinks, cross-reference fields and note numbers. Click follows
  the target; note numbers return to their reference in the body. Internal
  targets scroll to the top; external addresses open through the system.
  Shift+click and dragging still select text. Link hit regions are derived
  from disposable renderer attribution without changing the saved document.
- **Edit**: typing, deleting, Enter, selection by mouse and keyboard, bold /
  italic / underline / strike / super- and subscript / size / font / colour /
  highlight, alignment, paragraph styles, page and section breaks, tables,
  inserting another document, page orientation / size / margins, find and
  replace, accept / reject tracked changes, undo and redo - all through the
  transactional editor (`strict_ooxml::edit`).
- **Equations and charts**: click to select and drag the handles to resize;
  drag the selected object's body to move it in the text, double-click an
  equation to edit it. Moving is one undoable operation; equations stay as
  editable OMML, including when copied and pasted.
- **Text from file**: also accepts UTF-8/UTF-16 TXT. If a document contains
  package-bound content the structured importer cannot transfer, its text
  is inserted and the status line reports that formatting and linked objects
  were not transferred. The entire insertion is one Undo/Redo step.
- **Comments**: Done, Escape, closing the pane or clicking the document commits
  the draft. Saving also commits the current draft. Click highlighted text
  to open, scroll to and highlight its comment card.
- **Paragraph**: the Home paragraph group's launcher and the context menu open
  indents/spacing and line/page controls with a preview. Only changed fields
  are applied to selected paragraphs, together in one undoable operation.
- **Context menu**: uses the clicked text, table or object; preserves an existing
  selection when clicked inside it and offers the actions that apply there.
- **AI → Clean up document**: review whitespace, extra empty paragraphs, manual
  indents, soft hyphens and invisible separators before applying selected fixes
  in one Undo step. Leading spaces can become a real tab or first-line indent.
  AI suggests accidental paragraph joins without rewriting text; joins require
  individual selection. Turn off AI joins for offline technical cleanup.
  Formatting survives; protected content and recognized code are skipped.
  Editing the document after analysis invalidates the plan and requires a rescan.
- **Tables**: Tab / Shift+Tab walk the cells (Tab in the last cell adds a
  row); a click left of a row selects it, Ctrl+click a column, a drag across
  cells a block of them; selected cells are cleared, copied (tabs between
  cells) and formatted together; rows are inserted above / below and deleted
  from the Layout tab. Outside tables Tab inserts a tab.
- **Save**: Strict DOCX; export to PDF (embedded fonts), SVG pages, plain text.

OLE cached previews use the same selection, resize, move and wrapping commands
as pictures. In a picture's context menu, **Recognize equation (OCR → MathML)**
uses the configured Ollama vision model. A review dialog shows the formula and
lets you copy its MathML. Only **Replace with equation** changes the document,
as one Undo step; Cancel keeps the picture. Editable DOCX equations are stored
as standard OMML. OLE payload preservation follows the toolkit's import report;
editing a preview does not execute the embedded object.

## Localization

Interface strings are JSON (`locales/en.json`, `locales/ru.json`, compiled in).
Drop `xx.json` into `locales/` beside the executable or into
`<config>/MuFoEd/locales/` to add a language or override strings; nested
objects flatten to dotted keys, missing keys fall back to English.

## Architecture

- `worker.rs` - one thread per document: owns `StrictDocument` and the editing
  session, the visual map, caret and selection; answers with SVG pages, caret
  rectangles and state.
- `formats.rs` - every input becomes an opened Strict package.
- `canvas.rs` - pages on the desk, rasterized lazily on the rayon pool.
- `app.rs`, `ribbon.rs`, `tab.rs`, `icons.rs`, `theme.rs` - the interface.

## Rendering backend and keyboard-layout switchers

On Windows the window draws with Direct3D 12 only (`WGPU_BACKEND` overrides
it). wgpu's default set includes OpenGL, whose initialization leaves hidden
windows on threads that never pump messages; a layout change (Punto
Switcher's automatic one, or any `WM_INPUTLANGCHANGEREQUEST`) makes Windows
notify every GUI thread of the process synchronously, and the window froze
waiting for them.

## Diagnostics

A watchdog notes in `<config>/MuFoEd/trace.log` when the interface does not
paint for three seconds or a document request runs that long, with the
interface thread's stack (names and lines when `mufoed.pdb` lies beside the
executable: build with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`).
`MUFOED_TRACE=1` also logs every key, text and clipboard event and every
document request with its timing.
# Форматированный системный буфер

В Windows команда вставки читает RTF и HTML с изображениями. Приоритет — RTF,
затем HTML, затем обычный текст. HTML разбирает отдельный `strict-ooxml-html`;
редактор отвечает за системный буфер, загрузку ресурсов и одну операцию Undo.
Поддерживаются встроенные картинки и ссылки на доступные файлы/HTTP(S)-ресурсы.
Недоступные картинки заменяются альтернативным текстом с уведомлением о потерях.
Для HTML применяется документированный поднабор форматирования; полная вёрстка
браузера не воспроизводится. Загрузка изображений выполняется в рабочем потоке,
с ограничениями объёма и общего времени HTTP-запросов.

HTML (`.html`, `.htm`), RTF и LaTeX (`.tex`, `.ltx`) доступны также через «Открыть», «Импорт документа»
и «Вставить текст из файла». При вставке сохраняются поддерживаемое форматирование
и изображения, заменяется выделение; вся операция отменяется одним Undo.
`\input` и `\include` читаются из папки исходного `.tex`.
Относительные пути изображений HTML разрешаются от папки исходного файла.
HTML читается в UTF-8, с BOM UTF-16 или в кодировке, указанной в `meta charset`.
Исходный файл не перезаписывается: результат сохраняется в DOCX.

## Фигуры и схемы в документе

«Вставка → Схема» открывает редактор `diagram-editor` с примером, который можно
изменить или заменить своей схемой. «Применить» вставляет результат на место
каретки или выделенного текста. Для изменения схемы выделите её и нажмите ту же
кнопку, выберите «Редактировать схему» в контекстном меню либо щёлкните дважды.
«Отмена» закрывает черновик без изменений документа. Каждое применение отменяется
одним Undo; внутри окна редактора действует собственная история правок.

В документе схема выделяется, перемещается и масштабируется как единый рисунок.
DOCX содержит стандартные редактируемые фигуры и линии, а не растровый снимок.
Поддерживаемые схемы можно редактировать после сохранения и повторного открытия.
Неподдерживаемая структура существующей группы не заменяется упрощённой копией:
редактор сообщает причину отказа.

Ограничение MVP: текущая модель StrictLib не хранит привязки концов соединителей
к фигурам. В открытом сеансе исходная сцена сохраняется в памяти; после открытия
DOCX связи восстанавливаются по однозначному совпадению геометрии. Для точного
сохранения сцены используйте «Сохранить JSON» в окне редактора. Неоднозначные
соединения при импорте отклоняются с пояснением.


Редактор схем следует языку интерфейса MuFoEd (русский/английский, для других
языков — английский fallback). Существующие подписи фигур при смене языка не
переводятся. В окне схем доступны выделение рамкой, выравнивание и распределение,
повороты и обход препятствий соединителями. Группы выравниваются и распределяются
как единые объекты. Настройки и ограничения поиска маршрута описаны в
`diagram-editor/README.md`; неудача обхода отображается предупреждением.
