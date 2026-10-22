# file-manager — двухпанельный файловый менеджер

Встраиваемый компонент egui 0.35 для MuFoEd, будущая вкладка «Файлы». За
образец взят Total Commander. Вся работа с диском идёт фоновыми потоками
(листинги, копирование, архивы, поиск, подсчёт места), поэтому интерфейс не
замирает ни на больших файлах, ни на папках со многими тысячами файлов.
Копирование потоковое, блоками по 1 МиБ, с прогрессом и отменой.

## Возможности

- **Панели**: кнопки дисков (буквы на Windows; `/`, `~` и тома в `/media`,
  `/mnt` в Linux), строка пути (щелчок — ввести путь), колонки Имя / Тип /
  Размер / Дата / Атрибуты с сортировкой (щелчок по заголовку, Ctrl+F3…F6),
  естественный порядок имён, папки первыми, скрытые файлы по Ctrl+H, история
  Alt+← / Alt+→, быстрый фильтр по маске. Список виртуализирован: в кадре
  рисуются только видимые строки.
- **Выделение**: Insert, Пробел (у папки ещё и подсчёт занимаемого места),
  Shift+стрелки, Ctrl/Shift+щелчок, Ctrl+A; `+` / `-` — выделить / снять по
  маске, `*` — инвертировать (как на цифровом блоке в TC). Строка состояния:
  выделено N из M, объём.
- **Маски** в стиле TC: `*.jpg *.png | thumb*` (после `|` — исключения),
  `?`, `;` или пробел как разделитель; слово без `*`/`.` ищется в имени.
- **Операции**: F5 копировать, F6 переместить (в пределах тома — мгновенное
  переименование), Shift+F6 переименовать, F7 новая папка (`a/b/c` —
  вложенные), F8/Delete — в корзину, Shift+F8/Shift+Delete — навсегда (с
  подтверждением). Конфликты: пропускать, заменять, заменять более старые,
  оставлять оба с номером. Копия пишется во временный `.part` и
  переименовывается по готовности, дата изменения сохраняется.
- **Архивы** (всё на чистом Rust): ZIP, 7z, RAR 1.5–7 (solid, многотомные,
  шифрованные — через `vendor/unrar-rs`), TAR, TAR.GZ, TAR.BZ2, TAR.XZ,
  TAR.ZST, одиночные GZ/BZ2/XZ/ZST. Enter или Ctrl+PgDn — войти в архив как в
  папку; F5 из архива — извлечь, Alt+F9 — распаковать, Alt+F5 — упаковать в
  ZIP / 7z / TAR / TAR.GZ / TAR.XZ / GZ. Пути внутри архивов проверяются:
  ничего не пишется вне папки назначения. RAR, BZ2 и ZST — только чтение.
- **Просмотр**: F3 — встроенный просмотрщик: текст (BOM, UTF-8,
  Windows-1251), hex, изображения (все форматы галереи, включая RAW, PSD,
  JPEG XL, SVG), 3D-модели (панель галереи с анимацией), сводка по папке.
  Ctrl+Q — быстрый просмотр того же в соседней панели.
- **Редактирование**: F4 и Enter отправляют документы (docx, rtf, html, tex,
  pdf, djvu) хосту событием `Event::OpenInEditor` — их открывает MuFoEd;
  прочие файлы F4 отдаёт `Event::EditText`, Enter — `Event::OpenWithSystem`.
  Члены архивов перед этим извлекаются во временную папку.
- **Поиск** (Alt+F7): маска имени, текст внутри файлов (с учётом регистра
  или без, совпадения на границе блоков находятся), размер от/до, изменён за
  N дней, глубина, папки, скрытые. Результаты появляются по ходу; «Перейти к
  файлу» или «В панель» (как feed to listbox в TC).
- **Подсчёт места**: Пробел на папке, Alt+Shift+Enter — все папки панели;
  размеры видны в колонке «Размер».
- **Терминал**: строка команд под панелями с подсказкой текущей папки;
  набранный в панели текст попадает в неё. Enter выполняет, Shift+Enter —
  новая строка (многострочные команды выполняются по очереди), ↑/↓ —
  история, Ctrl+Enter вставляет имя файла под курсором, `cd` меняет папку
  панели. Журнал вывода раскрывается кнопкой или Ctrl+\` (высота меняется),
  stdout и stderr разными цветами, кнопка «Остановить». Оболочка: cmd или
  PowerShell на Windows, sh в остальных системах; вывод консоли в кодировке
  OEM (866) декодируется.
- Tab — другая панель, Ctrl+U — поменять панели, Ctrl+R — обновить,
  Backspace / Ctrl+PgUp — вверх.

## Встраивание

```rust
let mut manager = file_manager::FileManager::new(
    file_manager::Locale::new("ru"),
    file_manager::Settings::default(),
);
for event in manager.show(ui) {
    match event {
        file_manager::Event::OpenInEditor(path) => { /* открыть во вкладке MuFoEd */ }
        file_manager::Event::OpenWithSystem(path) => { /* программа по умолчанию */ }
        file_manager::Event::EditText(path) => { /* текстовый редактор */ }
    }
}
```

`settings()` возвращает сериализуемые настройки (папки панелей, скрытые
файлы, оболочка, терминал, последний поиск, вид архива). Ядро (`archive`,
`ops`, `search`, `entry`, `mask`, `terminal`, `preview`) не зависит от egui.

## Запуск демонстрации

```powershell
cargo +1.92.0 run -p file-manager --features demo --bin file-manager-demo -- D:\
```

Переменные: `FILE_MANAGER_LANGUAGE=ru`, `FILE_MANAGER_SETTINGS=<файл.json>`,
`FILE_MANAGER_SHOT=<файл.png>`.


Files and folders have vector icons for directories, images, archives, documents,
source code and media. Markdown (`.md`, `.markdown`, `.mdown`, `.mkd`) opens in
formatted mode in F3 and Ctrl+Q, with GFM tables, lists, code, links and local
images. The full viewer also offers source text and Hex modes.

Audio and video play inside both viewers, with pause, seek, stop and mute.
The default `oxideav` feature uses pinned OxideAV codec/container crates for
metadata and decoding, and CPAL for the system audio device on Windows, Linux
and macOS. It launches no FFmpeg, FFprobe, FFplay or other media subprocesses.
Decode, seek and device setup run on a cancellable worker; replacing a preview
joins that worker and closes its audio stream. Video is fitted to 1280 x 720;
frame/sample queues and decoder allocations have explicit limits.

This is an experimental backend, not a claim of FFmpeg format parity. Regression
fixtures cover H.264 + AAC-LC in MP4, MPEG-4 Part 2, VP8/VP9 in WebM, WAV, MP3 and
FLAC. Current audio output supports mono/stereo at 8-192 kHz. Unsupported codecs,
layouts and damaged files show a diagnostic; there is no automatic external-tool
fallback. Hardware acceleration, HDR tone mapping, surround downmix and complete
container/codec compatibility remain future work. CPAL uses the platform audio
APIs (Linux builds need ALSA development libraries).

A legacy standalone build is available with `--no-default-features`; it retains
the previous Windows MediaPlayer / FFmpeg implementation. MuFoEd uses OxideAV by
default. `cargo run -p file-manager --example oxideav_probe -- FILE...` reports
actual decoded frame/sample counts and saves a first-frame preview.

Media pauses when changing MuFoEd tabs and stops when closing its preview or
selecting another entry, including the parent folder or an empty listing.

MuFoEd supplies a read-only document provider for DOCX/DOCM/DOTX, RTF, HTML/HTM,
LaTeX (TEX/LTX), PDF and DjVu/DJV. Both F3 and Ctrl+Q show page images with
previous/next page, page number, fit-to-width and zoom controls. PgUp/PgDn work
in the full viewer. The existing Markdown, text/hex, raster/vector, 3D and media
viewers remain available. PDF and DjVu use native page rendering, without OCR
or editable conversion. Other documents use the editor's import/layout pipeline.

Hosts register `documents::Provider` through `set_document_provider`. Opening,
layout and rasterization happen on cancellable workers (at most two concurrent
loads); stale page requests are discarded and extracted members remain alive
until their worker finishes. Only the current page bitmap is retained by the UI.
The standalone component offers the host-independent viewers unless a document
provider is supplied. Supported documents retain the importers' format limits.

Each panel has its own folder tabs. `+` / Ctrl+T duplicates the current location;
Ctrl+Tab and Ctrl+Shift+Tab cycle the active panel's tabs. Close with `×`, middle
click or Ctrl+W. The last tab stays open. The tab context menu offers duplication,
pinning, moving left/right and moving to the other panel. Pinned tabs stay first
and must be unpinned before closing. Their location is locked: navigating to a
different folder/archive or search results opens a new unpinned tab, leaving the
pinned tab and its history intact. This applies to path/drive buttons, Back/Forward,
parent-folder navigation, terminal `cd` and locations opened by the host.
Drag tabs to reorder within the pinned or
unpinned group, or onto a tab of the other panel to transfer them (a panel keeps
at least one tab). Tabs retain independent navigation history and selection in
the current session. Switching tabs stops the previous quick preview.

The host settings persist tab order, pin state, selected tab and active panel,
folder/archive location, sort column/direction and filter for every tab. Old
settings containing only `left` / `right` migrate automatically. Missing folders
are retained with the normal folder error, allowing removable drives to return.
Only selected tabs read their listings at startup; other tabs load when selected.
MuFoEd saves this state through its existing `files.json` preferences mechanism.
