# MultiFormatEditor (MuFoEd)

A desktop demonstrator of the strict-ooxml toolkit: one window, one tab per
document (as in WPS Office), a Word-style ribbon. Windows, Linux and macOS
(egui/eframe, native file dialogs through `rfd`).

```text
cargo +1.92.0 run -p mufoed --release -- [FILE...]
```

What it shows:

- **Open**: DOCX (Strict; Transitional through the normalizer), RTF (importer).
- **Convert**: PDF and DjVu into an editable Strict document, semantic or
  visual mode, page range, images; scanned pages recognized by a local Ollama
  vision model (feature `ocr`, on by default; endpoint and model in Options).
- **Display**: pages rendered to SVG by the library and rasterized with the
  bundled faces; navigation pane (headings, pages), support report pane.
- **Edit**: typing, deleting, Enter, selection by mouse and keyboard, bold /
  italic / underline / strike / super- and subscript / size / font / colour /
  highlight, alignment, paragraph styles, page and section breaks, tables,
  inserting another document, page orientation / size / margins, find and
  replace, accept / reject tracked changes, undo and redo - all through the
  transactional editor (`strict_ooxml::edit`).
- **Tables**: Tab / Shift+Tab walk the cells (Tab in the last cell adds a
  row); a click left of a row selects it, Ctrl+click a column, a drag across
  cells a block of them; selected cells are cleared, copied (tabs between
  cells) and formatted together; rows are inserted above / below and deleted
  from the Layout tab. Outside tables Tab inserts a tab.
- **Save**: Strict DOCX; export to PDF (embedded fonts), SVG pages, plain text.

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
