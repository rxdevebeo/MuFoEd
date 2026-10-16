//! The window: document tabs in the title strip (as in WPS), the ribbon, the
//! desk, the side panes and the status bar.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, vec2, Color32, FontFamily, FontId, Key, RichText, Sense, Stroke};
use strict_ooxml::Mode;

use crate::canvas;
use crate::formats::{self, ImportOptions, Kind, OcrSettings, Source};
use crate::i18n::I18n;
use crate::icons::{self, Icon};
use crate::ribbon::{self, RibbonState, RibbonTab};
use crate::settings::{self, Settings};
use crate::tab::{AfterSave, DocTab, Status, TabEvent};
use crate::theme::{self, Palette};
use crate::worker::{
    Align, Busy, CaretFormat, EditState, ExportPurpose, FindQuery, FormatChange,
    HeaderFooterCommand, Insert, Motion, ObjectCommand, PictureEdit, Request, SaveFormat,
};
use crate::worker::{CommentCommand, PageChange, ReferenceCommand};

/// Everything a click can ask for.
pub(crate) enum Action {
    ToggleSpelling,
    DownloadDictionary(&'static str),
    New,
    OpenDialog,
    OpenPath(PathBuf),
    ImportDialog {
        ocr: bool,
    },
    Save,
    SaveAs,
    Export(SaveFormat),
    CloseTab(usize),
    Exit,
    Doc(Request),
    Paste,
    InsertDocument,
    InsertPicture,
    /// Crop the selected picture with its handles, or stop.
    ToggleCrop,
    /// Brightness and contrast with sliders and a preview.
    AdjustDialog,
    /// Recognise the selected picture with the configured model.
    RecognizePicture {
        formula: bool,
    },
    InsertDate,
    Open(Dialog),
    ToggleReport,
    ToggleNavigation,
    ToggleRuler,
    ToggleComments,
    /// A comment on the selection, by the user.
    NewComment,
    PrintDialog,
    /// Insert or update the table of contents.
    TableOfContents,
    /// Edit the header (or footer) of the page in view.
    EnterHeaderFooter {
        header: bool,
    },
    /// Page numbers in the header (`top`) or footer.
    PageNumbers {
        top: bool,
        align: Align,
        of_total: bool,
    },
    ToggleSystemFonts,
    ToggleDark,
    Language(String),
    Zoom(Zoom),
    FontColor([u8; 3]),
    Backstage(bool),
    Activate(Option<usize>),
}

/// A picture being edited in another program, by its file.
struct ExternalEdit {
    /// The document tab.
    tab: u64,
    /// The picture's drawing id.
    id: u32,
    path: PathBuf,
    /// When the file last changed.
    modified: Option<std::time::SystemTime>,
}

/// Opens a picture file in the system's editor for pictures (on Windows the
/// "Edit" verb: Paint, unless the user chose another), not its viewer.
fn open_in_editor(path: &std::path::Path) -> Result<(), String> {
    let mut command;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let quoted = path.display().to_string().replace('\'', "''");
        command = std::process::Command::new("powershell");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "try {{ Start-Process -FilePath '{quoted}' -Verb Edit -ErrorAction Stop }} \
                 catch {{ Start-Process -FilePath '{quoted}' }}"
            ))
            // No console window flashes up.
            .creation_flags(0x0800_0000);
    }
    #[cfg(target_os = "macos")]
    {
        command = std::process::Command::new("open");
        command.arg(path);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        command = std::process::Command::new("xdg-open");
        command.arg(path);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Zoom commands.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Zoom {
    In,
    Out,
    Set(f32),
    OnePage,
    PageWidth,
}

/// Modal and floating dialogs.
#[derive(Clone, Debug)]
pub(crate) enum Dialog {
    Find {
        replace: bool,
    },
    Table {
        rows: usize,
        columns: usize,
    },
    Symbols,
    WordCount,
    Settings,
    About,
    Import(Box<ImportDialog>),
    ConfirmClose {
        tab: u64,
    },
    ConfirmExit,
    Adjust(Box<AdjustDialog>),
    /// The first page number of the section: `None` continues.
    PageStart {
        start: u32,
        restart: bool,
    },
    Print(Box<PrintDialog>),
    /// Text columns: how many, the space between, a line between.
    Columns {
        count: u16,
        space_cm: f32,
        separator: bool,
    },
    /// A dictionary to fetch, for words no dictionary checks.
    Dictionary {
        code: &'static str,
    },
    /// The bookmarks: a name to add, the one chosen in the list.
    Bookmarks {
        name: String,
        selected: Option<usize>,
    },
}

/// The print dialog's fields.
#[derive(Clone, Debug)]
pub(crate) struct PrintDialog {
    printers: Vec<String>,
    printer: usize,
    copies: u32,
    collate: bool,
    range: PrintRange,
    /// Pages for [`PrintRange::Pages`], as typed: `1-3, 5`.
    pages: String,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintRange {
    All,
    Current,
    Pages,
}

/// Brightness and contrast of the selected picture, previewed on a small
/// copy of it.
#[derive(Clone, Default)]
pub(crate) struct AdjustDialog {
    /// The picture, small; `None` until its bytes arrive.
    base: Option<resvg::tiny_skia::Pixmap>,
    /// Percent, -100..=100.
    brightness: f32,
    contrast: f32,
    /// The preview and the values it shows.
    texture: Option<(egui::TextureHandle, f32, f32)>,
}

impl std::fmt::Debug for AdjustDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdjustDialog")
            .field("brightness", &self.brightness)
            .field("contrast", &self.contrast)
            .finish_non_exhaustive()
    }
}

/// The import dialog's fields.
#[derive(Clone, Debug)]
pub(crate) struct ImportDialog {
    path: PathBuf,
    kind: Kind,
    visual: bool,
    images: bool,
    all_pages: bool,
    first: usize,
    last: usize,
    ocr: bool,
    layout: bool,
}

/// The find bar's fields.
#[derive(Default)]
struct FindState {
    text: String,
    with: String,
    case_sensitive: bool,
    whole_word: bool,
    focus: bool,
}

/// A line in the status bar.
struct StatusLine {
    text: String,
    error: bool,
    at: f64,
}

pub(crate) struct App {
    i18n: I18n,
    settings: Settings,
    tabs: Vec<DocTab>,
    /// `None` is the start page.
    active: Option<usize>,
    ribbon_tab: RibbonTab,
    backstage: bool,
    dialog: Option<Dialog>,
    find: FindState,
    find_open: bool,
    find_replace: bool,
    report_open: bool,
    /// The comments pane is shown.
    comments_open: bool,
    /// The dictionaries, once loaded.
    speller: Option<Arc<crate::spell::Speller>>,
    speller_loading: Option<std::sync::mpsc::Receiver<crate::spell::Speller>>,
    /// A dictionary being fetched, and how it went.
    dictionary_download: Option<(&'static str, std::sync::mpsc::Receiver<Result<(), String>>)>,
    /// Languages offered this session, not offered again.
    dictionaries_offered: Vec<&'static str>,
    font_color: [u8; 3],
    status: Option<StatusLine>,
    next_id: u64,
    exit_confirmed: bool,
    /// Files named on the command line, opened on the first frame.
    startup: Vec<PathBuf>,
    /// `MUFOED_SHOT`: the window saved as a picture, then closed.
    shot: Option<crate::shot::Shot>,
    /// `MUFOED_DIALOG`: a dialog opened once the files are.
    startup_dialog: Option<String>,
    applied_dark: Option<bool>,
    /// Document faces for the navigation pane.
    doc_fonts: std::cell::RefCell<crate::docfonts::DocumentFonts>,
    /// What the last copy carried besides text.
    clip: Option<Box<crate::worker::ClipObjects>>,
    /// The contextual tab shown in the last frame.
    tool_shown: Option<RibbonTab>,
    /// The ribbon tab to go back to once the contextual one goes.
    tab_before_tool: RibbonTab,
    /// How the print job is going.
    print_progress: Option<std::sync::mpsc::Receiver<crate::print::Progress>>,
    /// A print job waiting for its PDF (CUPS).
    #[cfg(not(windows))]
    pending_print: Option<(
        crate::print::Job,
        PathBuf,
        Box<dyn Fn(crate::print::Progress) + Send>,
    )>,
    /// Pictures open in an outside editor.
    external: Vec<ExternalEdit>,
    /// When they were last looked at.
    external_checked: f64,
    window_title: String,
    /// This frame's input in its order, Tab keys included, when the Tab keys
    /// were taken from egui before it could move focus with them.
    frame_events: Option<Vec<egui::Event>>,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let settings = Settings::load();
        let mut i18n = I18n::load(&settings::locale_dirs());
        let language = settings
            .language
            .clone()
            .or_else(crate::i18n::system_language)
            .unwrap_or_else(|| "en".to_owned());
        if !i18n.set_language(&language) {
            i18n.set_language("en");
        }
        let interface_fonts = theme::install_fonts(&cc.egui_ctx);
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);
        std::thread::spawn(crate::raster::prepare);
        crate::stack::remember_interface_thread();
        crate::trace::watchdog(cc.egui_ctx.clone());
        let problems = i18n.problems.join("; ");
        let mut app = Self {
            i18n,
            settings,
            tabs: Vec::new(),
            active: None,
            ribbon_tab: RibbonTab::Home,
            backstage: false,
            dialog: None,
            find: FindState::default(),
            find_open: false,
            find_replace: false,
            report_open: false,
            comments_open: false,
            speller: None,
            speller_loading: Some(load_speller(&cc.egui_ctx)),
            dictionary_download: None,
            dictionaries_offered: Vec::new(),
            font_color: [192, 0, 0],
            status: None,
            next_id: 1,
            exit_confirmed: false,
            startup: files,
            shot: crate::shot::Shot::from_env(),
            startup_dialog: crate::shot::dialog(),
            applied_dark: None,
            doc_fonts: std::cell::RefCell::new(crate::docfonts::DocumentFonts::new(
                interface_fonts,
            )),
            clip: None,
            tool_shown: None,
            tab_before_tool: RibbonTab::Home,
            print_progress: None,
            #[cfg(not(windows))]
            pending_print: None,
            external: Vec::new(),
            external_checked: 0.0,
            window_title: String::new(),
            frame_events: None,
        };
        if !problems.is_empty() {
            app.status = Some(StatusLine {
                text: problems,
                error: true,
                at: 0.0,
            });
        }
        app
    }

    fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.i18n.t(key)
    }

    fn palette(&self) -> Palette {
        theme::palette(self.settings.dark)
    }

    fn active_tab(&self) -> Option<&DocTab> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    fn active_tab_mut(&mut self) -> Option<&mut DocTab> {
        self.active.and_then(|index| self.tabs.get_mut(index))
    }

    fn say(&mut self, text: impl Into<String>, error: bool, now: f64) {
        self.status = Some(StatusLine {
            text: text.into(),
            error,
            at: now,
        });
    }

    // ----- opening ------------------------------------------------------

    fn open_source(&mut self, ctx: &egui::Context, source: Source, title: String) {
        let id = self.next_id;
        self.next_id += 1;
        let tab = DocTab::open(
            id,
            source,
            title,
            self.settings.zoom,
            self.settings.system_fonts,
            ctx,
        );
        let mut tab = tab;
        tab.set_speller(self.speller_for_tabs());
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
        self.backstage = false;
    }

    fn new_document(&mut self, ctx: &egui::Context) {
        let count = self
            .tabs
            .iter()
            .filter(|tab| tab.kind == Some(Kind::New) || tab.source.is_none())
            .count();
        let title = self.i18n.tf("doc.untitled", &[("n", &(count + 1))]);
        self.open_source(ctx, Source::New, title);
    }

    fn open_path(&mut self, ctx: &egui::Context, path: PathBuf, now: f64) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.source.as_ref() == Some(&path) || tab.path.as_ref() == Some(&path))
        {
            self.active = Some(index);
            self.backstage = false;
            return;
        }
        match formats::detect(&path) {
            None => {
                let text = self
                    .i18n
                    .tf("error.unknown_format", &[("file", &path.display())]);
                self.say(text, true, now);
            }
            Some(kind) if kind.converted() => {
                self.dialog = Some(Dialog::Import(Box::new(ImportDialog::new(
                    path, kind, false,
                ))));
            }
            Some(_) => {
                self.settings.remember(&path);
                self.settings.save();
                let title = file_name(&path);
                self.open_source(
                    ctx,
                    Source::File {
                        path,
                        import: ImportOptions::default(),
                    },
                    title,
                );
            }
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context, now: f64) {
        let dialog = rfd::FileDialog::new()
            .set_title(self.t("dialog.open_title"))
            .add_filter(self.t("filter.all_supported"), formats::OPEN_EXTENSIONS)
            .add_filter("Word (DOCX)", &["docx", "docm", "dotx"])
            .add_filter("RTF", &["rtf"])
            .add_filter("PDF", &["pdf"])
            .add_filter("DjVu", &["djvu", "djv"]);
        if let Some(files) = dialog.pick_files() {
            for path in files {
                self.open_path(ctx, path, now);
            }
        }
    }

    fn import_dialog(&mut self, ocr: bool) {
        let picked = rfd::FileDialog::new()
            .set_title(self.t(if ocr {
                "dialog.recognize_title"
            } else {
                "dialog.import_title"
            }))
            .add_filter("PDF, DjVu", &["pdf", "djvu", "djv"])
            .pick_file();
        if let Some(path) = picked {
            let kind = formats::detect(&path).unwrap_or(Kind::Pdf);
            self.dialog = Some(Dialog::Import(Box::new(ImportDialog::new(path, kind, ocr))));
        }
    }

    fn start_import(&mut self, ctx: &egui::Context, import: &ImportDialog) {
        let ocr = (import.ocr && cfg!(feature = "ocr")).then(|| OcrSettings {
            endpoint: self.settings.ocr_endpoint.clone(),
            model: self.settings.ocr_model.clone(),
            language: Some(self.settings.ocr_language.trim().to_owned())
                .filter(|language| !language.is_empty()),
            layout: import.layout,
        });
        let options = ImportOptions {
            mode: if import.visual {
                Mode::Visual
            } else {
                Mode::Semantic
            },
            images: import.images,
            pages: (!import.all_pages)
                .then_some((import.first.max(1), import.last.max(import.first.max(1)))),
            ocr,
        };
        self.settings.remember(&import.path);
        self.settings.save();
        let title = file_name(&import.path);
        self.open_source(
            ctx,
            Source::File {
                path: import.path.clone(),
                import: options,
            },
            title,
        );
    }

    // ----- saving -------------------------------------------------------

    fn save(&mut self, save_as: bool, now: f64) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if !tab.ready() {
            return;
        }
        let path = if save_as { None } else { tab.path.clone() };
        let path = path.or_else(|| {
            rfd::FileDialog::new()
                .set_title(self.t("dialog.save_title"))
                .add_filter(self.t("filter.docx"), &["docx"])
                .set_file_name(tab.suggested_name("docx"))
                .save_file()
        });
        if let Some(path) = path {
            let path = with_extension(path, "docx");
            if let Some(tab) = self.active_tab() {
                tab.send(Request::Save {
                    path,
                    format: SaveFormat::Docx,
                });
            }
            self.say(self.t("status.saving").to_owned(), false, now);
        } else if let Some(tab) = self.active_tab_mut() {
            // A cancelled dialog cancels what was waiting for the save.
            tab.after_save = None;
        }
    }

    fn export(&mut self, format: SaveFormat, now: f64) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if !tab.ready() {
            return;
        }
        let (extension, filter) = match format {
            SaveFormat::Pdf => ("pdf", "PDF"),
            SaveFormat::Svg => ("svg", "SVG"),
            SaveFormat::Text => ("txt", self.t("filter.text")),
            SaveFormat::Docx => ("docx", "DOCX"),
        };
        let picked = rfd::FileDialog::new()
            .set_title(self.t("dialog.export_title"))
            .add_filter(filter, &[extension])
            .set_file_name(tab.suggested_name(extension))
            .save_file();
        if let Some(path) = picked {
            let path = with_extension(path, extension);
            tab.send(Request::Save { path, format });
            self.say(self.t("status.exporting").to_owned(), false, now);
        }
    }

    // ----- closing ------------------------------------------------------

    fn close_tab(&mut self, index: usize, force: bool) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if !force && tab.ready() && tab.modified() {
            self.active = Some(index);
            self.dialog = Some(Dialog::ConfirmClose { tab: tab.id });
            return;
        }
        self.tabs.remove(index);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            other => other,
        };
    }

    // ----- actions ------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn run(&mut self, ctx: &egui::Context, action: Action, now: f64) {
        match action {
            Action::New => self.new_document(ctx),
            Action::OpenDialog => self.open_dialog(ctx, now),
            Action::OpenPath(path) => self.open_path(ctx, path, now),
            Action::ImportDialog { ocr } => self.import_dialog(ocr),
            Action::Save => self.save(false, now),
            Action::SaveAs => self.save(true, now),
            Action::Export(format) => self.export(format, now),
            Action::CloseTab(index) => self.close_tab(index, false),
            Action::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Doc(request) => {
                if let Some(tab) = self.active_tab_mut() {
                    if tab.ready() {
                        tab.view.caret_moved = now;
                        tab.send(request);
                    }
                }
            }
            Action::Paste => match self.drawings_paste() {
                Some(request) => {
                    if let Some(tab) = self.active_tab() {
                        tab.send(request);
                    }
                }
                None => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            },
            Action::InsertDocument => {
                let picked = rfd::FileDialog::new()
                    .set_title(self.t("dialog.insert_title"))
                    .add_filter(self.t("filter.all_supported"), formats::OPEN_EXTENSIONS)
                    .pick_file();
                if let (Some(path), Some(tab)) = (picked, self.active_tab()) {
                    tab.send(Request::Insert(Insert::Document(path)));
                }
            }
            Action::InsertPicture => {
                let picked = rfd::FileDialog::new()
                    .set_title(self.t("dialog.picture_title"))
                    .add_filter(
                        self.t("filter.pictures"),
                        &["png", "jpg", "jpeg", "gif", "bmp"],
                    )
                    .pick_file();
                if let Some(path) = picked {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            let name = path
                                .file_stem()
                                .and_then(|stem| stem.to_str())
                                .unwrap_or("Picture")
                                .to_owned();
                            if let Some(tab) = self.active_tab() {
                                tab.send(Request::Object(ObjectCommand::InsertPicture {
                                    bytes,
                                    name,
                                }));
                            }
                        }
                        Err(error) => {
                            let text = self.i18n.tf("status.open_failed", &[("error", &error)]);
                            self.say(text, true, now);
                        }
                    }
                }
            }
            Action::AdjustDialog => {
                self.dialog = Some(Dialog::Adjust(Box::default()));
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::Object(ObjectCommand::Export {
                        purpose: ExportPurpose::Preview,
                    }));
                }
            }
            Action::ToggleCrop => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.view.crop = !tab.view.crop;
                }
            }
            Action::RecognizePicture { formula } => {
                let endpoint = self.settings.ocr_endpoint.clone();
                let model = self.settings.ocr_model.clone();
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::Object(ObjectCommand::Recognize {
                        formula,
                        endpoint,
                        model,
                    }));
                }
            }
            Action::InsertDate => {
                let date = today(self.i18n.language());
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::Type(date));
                }
            }
            Action::Open(dialog) => {
                if let Dialog::Find { replace } = dialog {
                    self.find_open = true;
                    self.find_replace = replace;
                    self.find.focus = true;
                } else {
                    self.dialog = Some(dialog);
                }
            }
            Action::ToggleReport => self.report_open = !self.report_open,
            Action::EnterHeaderFooter { header } => {
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::HeaderFooter(HeaderFooterCommand::Enter {
                        page: tab.view.current_page,
                        header,
                    }));
                }
            }
            Action::PageNumbers {
                top,
                align,
                of_total,
            } => {
                let words = of_total.then(|| {
                    (
                        self.t("page_number.page").to_owned(),
                        self.t("page_number.of").to_owned(),
                    )
                });
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::HeaderFooter(HeaderFooterCommand::Insert {
                        top,
                        align,
                        of_total: words,
                    }));
                }
            }
            Action::TableOfContents => {
                let title = self.t("toc.title").to_owned();
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::References(ReferenceCommand::TableOfContents {
                        title,
                    }));
                }
            }
            Action::PrintDialog => {
                let printers = crate::print::printers();
                if printers.is_empty() {
                    let text = self.t("print.no_printers").to_owned();
                    self.say(text, true, now);
                } else {
                    self.dialog = Some(Dialog::Print(Box::new(PrintDialog {
                        printers,
                        printer: 0,
                        copies: 1,
                        collate: true,
                        range: PrintRange::All,
                        pages: String::new(),
                        error: None,
                    })));
                }
            }
            Action::ToggleComments => self.comments_open = !self.comments_open,
            Action::ToggleSpelling => {
                self.settings.spelling = !self.settings.spelling;
                self.settings.save();
                let speller = self.speller_for_tabs();
                for tab in &mut self.tabs {
                    tab.set_speller(speller.clone());
                }
            }
            Action::DownloadDictionary(code) => self.download_dictionary(ctx, code, now),
            Action::NewComment => {
                let author = self.settings.author.clone();
                let initials: String = author
                    .split_whitespace()
                    .filter_map(|word| word.chars().next())
                    .collect();
                if let Some(tab) = self.active_tab() {
                    tab.send(Request::Comments(CommentCommand::Add { author, initials }));
                }
            }
            Action::ToggleRuler => {
                self.settings.ruler = !self.settings.ruler;
                self.settings.save();
            }
            Action::ToggleNavigation => {
                self.settings.navigation = !self.settings.navigation;
                self.settings.save();
            }
            Action::ToggleSystemFonts => {
                self.settings.system_fonts = !self.settings.system_fonts;
                self.settings.save();
                for tab in &self.tabs {
                    tab.send(Request::SystemFonts(self.settings.system_fonts));
                }
            }
            Action::ToggleDark => {
                self.settings.dark = !self.settings.dark;
                self.settings.save();
            }
            Action::Language(code) => {
                if self.i18n.set_language(&code) {
                    self.settings.language = Some(code);
                    self.settings.save();
                }
            }
            Action::Zoom(zoom) => self.zoom(zoom, now),
            Action::FontColor(rgb) => self.font_color = rgb,
            Action::Backstage(open) => self.backstage = open,
            Action::Activate(index) => {
                self.active = index;
                self.backstage = false;
            }
        }
    }

    fn zoom(&mut self, zoom: Zoom, now: f64) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let (rects, _) = canvas::page_rects(tab);
        let unit = rects
            .get(tab.view.current_page)
            .or_else(|| rects.first())
            .map(|rect| (rect.width() / tab.zoom, rect.height() / tab.zoom));
        let next = match zoom {
            Zoom::In => next_zoom(tab.zoom, true),
            Zoom::Out => next_zoom(tab.zoom, false),
            Zoom::Set(value) => value,
            Zoom::PageWidth => unit.map_or(tab.zoom, |(w, _)| (tab.view.viewport.x - 48.0) / w),
            Zoom::OnePage => unit.map_or(tab.zoom, |(w, h)| {
                ((tab.view.viewport.y - 40.0) / h).min((tab.view.viewport.x - 48.0) / w)
            }),
        };
        tab.zoom = next.clamp(0.25, 5.0);
        tab.view.zoom_changed = now;
        tab.view.scroll_to_page = Some(tab.view.current_page);
        self.settings.zoom = tab.zoom;
    }

    // ----- frame --------------------------------------------------------

    fn poll_tabs(&mut self, ctx: &egui::Context, now: f64) {
        let mut close = Vec::new();
        let mut messages = Vec::new();
        let mut pictures = Vec::new();
        let mut missing_scripts = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            for event in tab.poll() {
                match event {
                    TabEvent::Note(note) => {
                        let mut text = self.i18n.t(note.key).to_owned();
                        if !note.detail.is_empty() {
                            text = if text.contains("{detail}") {
                                text.replace("{detail}", &note.detail)
                            } else {
                                format!("{text}: {}", note.detail)
                            };
                        }
                        messages.push((text, note.error));
                    }
                    TabEvent::Clipboard(text) => {
                        // A copy without drawings replaces the last one's.
                        self.clip = None;
                        ctx.copy_text(text);
                    }
                    TabEvent::ClipboardObjects(clip) => {
                        if let Some((width, height, rgba)) = &clip.image {
                            ctx.copy_image(egui::ColorImage::from_rgba_premultiplied(
                                [*width, *height],
                                rgba,
                            ));
                        }
                        self.clip = Some(clip);
                    }
                    TabEvent::Picture {
                        id,
                        bytes,
                        extension,
                        purpose,
                    } => pictures.push((tab.id, id, bytes, extension, purpose)),
                    #[cfg(not(windows))]
                    TabEvent::Saved { path, .. }
                        if self
                            .pending_print
                            .as_ref()
                            .is_some_and(|(_, pdf, _)| *pdf == path) =>
                    {
                        if let Some((job, pdf, report)) = self.pending_print.take() {
                            crate::print::start_pdf(job, pdf, report);
                        }
                    }
                    TabEvent::Saved { path, format } => {
                        let key = if format == SaveFormat::Docx {
                            "status.saved"
                        } else {
                            "status.exported"
                        };
                        messages.push((self.i18n.tf(key, &[("file", &path.display())]), false));
                        if format == SaveFormat::Docx {
                            self.settings.remember(&path);
                            match tab.after_save.take() {
                                Some(AfterSave::Close) => close.push(index),
                                None => {}
                            }
                        }
                    }
                    TabEvent::CommentAdded => self.comments_open = true,
                    TabEvent::SpellingMissing(scripts) => missing_scripts.extend(scripts),
                    TabEvent::Loaded => {
                        messages.push((
                            self.i18n.tf("status.opened", &[("file", &tab.title)]),
                            false,
                        ));
                    }
                    TabEvent::Failed(error) => {
                        messages.push((
                            self.i18n.tf("status.open_failed", &[("error", &error)]),
                            true,
                        ));
                    }
                }
            }
        }
        for (text, error) in messages {
            self.say(text, error, now);
        }
        self.offer_dictionaries(&missing_scripts);
        for (tab, id, bytes, extension, purpose) in pictures {
            match purpose {
                ExportPurpose::External => self.edit_externally(tab, id, &bytes, extension, now),
                ExportPurpose::Save => self.save_picture(&bytes, extension, now),
                ExportPurpose::Preview => {
                    if let Some(Dialog::Adjust(dialog)) = self.dialog.as_mut() {
                        dialog.base = crate::raster::picture_preview(&bytes, 320);
                        dialog.texture = None;
                    }
                }
            }
        }
        self.watch_external(now);
        self.poll_print(now);
        if !self.external.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if !close.is_empty() {
            self.settings.save();
        }
        for index in close.into_iter().rev() {
            self.close_tab(index, true);
        }
    }

    /// Sends a print job off and reports how it goes in the status bar.
    fn start_print(&mut self, job: crate::print::Job, ctx: &egui::Context) {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.print_progress = Some(receiver);
        let repaint = ctx.clone();
        let report = move |progress| {
            let _ = sender.send(progress);
            repaint.request_repaint();
        };
        #[cfg(windows)]
        {
            let pages = self
                .active_tab()
                .map(|tab| tab.pages.clone())
                .unwrap_or_default();
            crate::print::start(job, pages, report);
        }
        #[cfg(not(windows))]
        {
            // The document goes to CUPS as a PDF, written first.
            let path =
                std::env::temp_dir().join(format!("mufoed-print-{}.pdf", std::process::id()));
            if let Some(tab) = self.active_tab() {
                tab.send(Request::Save {
                    path: path.clone(),
                    format: SaveFormat::Pdf,
                });
            }
            self.pending_print = Some((job, path, Box::new(report)));
        }
    }

    /// The print job's progress, in the status bar.
    fn poll_print(&mut self, now: f64) {
        let Some(receiver) = &self.print_progress else {
            return;
        };
        let mut said = None;
        while let Ok(progress) = receiver.try_recv() {
            said = Some(progress);
        }
        match said {
            Some(crate::print::Progress::Sheets { done, total }) => {
                let text = self
                    .i18n
                    .tf("print.progress", &[("done", &done), ("total", &total)]);
                self.say(text, false, now);
            }
            Some(crate::print::Progress::Done(Ok(()))) => {
                self.print_progress = None;
                let text = self.t("print.sent").to_owned();
                self.say(text, false, now);
            }
            Some(crate::print::Progress::Done(Err(error))) => {
                self.print_progress = None;
                let text = self.i18n.tf("print.failed", &[("error", &error)]);
                self.say(text, true, now);
            }
            None => {}
        }
    }

    /// Saves a picture's bytes where the user says.
    fn save_picture(&mut self, bytes: &[u8], extension: &str, now: f64) {
        let picked = rfd::FileDialog::new()
            .set_title(self.t("dialog.save_picture_title"))
            .set_file_name(format!("picture.{extension}"))
            .add_filter(self.t("filter.pictures"), &[extension])
            .save_file();
        if let Some(path) = picked {
            match std::fs::write(&path, bytes) {
                Ok(()) => {
                    let text = self
                        .i18n
                        .tf("status.exported", &[("file", &path.display())]);
                    self.say(text, false, now);
                }
                Err(error) => {
                    let text = self.i18n.tf("status.open_failed", &[("error", &error)]);
                    self.say(text, true, now);
                }
            }
        }
    }

    /// Writes a picture to a file of its own, opens it in the system's
    /// picture editor and watches it: each save there comes back into the
    /// document.
    fn edit_externally(&mut self, tab: u64, id: u32, bytes: &[u8], extension: &str, now: f64) {
        let dir = std::env::temp_dir().join("mufoed-pictures");
        let path = dir.join(format!("picture-{tab}-{id}.{extension}"));
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, bytes));
        if let Err(error) = written {
            let text = self.i18n.tf("status.open_failed", &[("error", &error)]);
            self.say(text, true, now);
            return;
        }
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        match open_in_editor(&path) {
            Ok(()) => {
                self.external.retain(|watch| watch.path != path);
                self.external.push(ExternalEdit {
                    tab,
                    id,
                    path,
                    modified,
                });
                let text = self.t("status.external_edit").to_owned();
                self.say(text, false, now);
            }
            Err(error) => {
                let text = self.i18n.tf("status.open_failed", &[("error", &error)]);
                self.say(text, true, now);
            }
        }
    }

    /// Brings pictures saved in an outside editor back, once a second.
    fn watch_external(&mut self, now: f64) {
        if self.external.is_empty() || now - self.external_checked < 1.0 {
            return;
        }
        self.external_checked = now;
        let tabs: Vec<u64> = self.tabs.iter().map(|tab| tab.id).collect();
        self.external.retain(|watch| tabs.contains(&watch.tab));
        let mut changed = Vec::new();
        for watch in &mut self.external {
            let modified = std::fs::metadata(&watch.path)
                .and_then(|m| m.modified())
                .ok();
            if modified.is_some() && modified != watch.modified {
                watch.modified = modified;
                if let Ok(bytes) = std::fs::read(&watch.path) {
                    changed.push((watch.tab, watch.id, bytes));
                }
            }
        }
        for (tab, id, bytes) in changed {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == tab) {
                tab.send(Request::Object(ObjectCommand::Replace { id, bytes }));
            }
        }
    }

    /// The copied drawings alone, when the clipboard holds no text of ours
    /// (a lone picture): a paste then brings them back.
    fn drawings_paste(&self) -> Option<Request> {
        self.clip
            .as_ref()
            .filter(|clip| clip.text.is_empty() && !clip.objects.is_empty())
            .map(|clip| Request::PasteObjects(clip.clone()))
    }

    /// Keys for the open document and the window-wide shortcuts.
    #[allow(clippy::too_many_lines)]
    fn keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let typing_elsewhere = ctx.egui_wants_keyboard_input();
        let modal = self.dialog.is_some() || self.backstage;
        let document = !typing_elsewhere && !modal && self.active_tab().is_some_and(DocTab::ready);
        let events = self
            .frame_events
            .take()
            .unwrap_or_else(|| ctx.input(|input| input.events.clone()));
        let mut pasted = false;
        let mut paste_key = false;
        for event in events {
            match event {
                egui::Event::Text(text) if document => {
                    actions.push(Action::Doc(Request::Type(text)));
                }
                egui::Event::Copy if document => {
                    actions.push(Action::Doc(Request::Copy { cut: false }));
                }
                egui::Event::Cut if document => {
                    actions.push(Action::Doc(Request::Copy { cut: true }));
                }
                egui::Event::Paste(text) if document => {
                    pasted = true;
                    // Text this editor copied with drawings in it brings
                    // them back.
                    let same = |clip: &crate::worker::ClipObjects| {
                        clip.text.replace("\r\n", "\n") == text.replace("\r\n", "\n")
                    };
                    match self.clip.as_ref().filter(|clip| same(clip)) {
                        Some(clip) => {
                            actions.push(Action::Doc(Request::PasteObjects(clip.clone())));
                        }
                        None => actions.push(Action::Doc(Request::Paste(text))),
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let command = modifiers.command;
                    let shift = modifiers.shift;
                    paste_key |= command && key == Key::V;
                    // Window-wide shortcuts first.
                    if command && !typing_elsewhere {
                        let global = match key {
                            Key::N => Some(Action::New),
                            Key::O => Some(Action::OpenDialog),
                            Key::S if shift => Some(Action::SaveAs),
                            Key::S => Some(Action::Save),
                            Key::W | Key::F4 => self.active.map(Action::CloseTab),
                            Key::P => Some(Action::PrintDialog),
                            Key::F => Some(Action::Open(Dialog::Find { replace: false })),
                            Key::H => Some(Action::Open(Dialog::Find { replace: true })),
                            Key::Tab => {
                                let count = self.tabs.len();
                                (count > 0).then(|| {
                                    let next = self.active.map_or(0, |index| {
                                        if shift {
                                            (index + count - 1) % count
                                        } else {
                                            (index + 1) % count
                                        }
                                    });
                                    Action::Activate(Some(next))
                                })
                            }
                            _ => None,
                        };
                        if let Some(action) = global {
                            actions.push(action);
                            continue;
                        }
                    }
                    if key == Key::Escape {
                        if self.backstage {
                            self.backstage = false;
                        } else if self.find_open && !modal {
                            self.find_open = false;
                        } else if document
                            && self
                                .active_tab()
                                .and_then(|tab| tab.cursor.as_ref())
                                .is_some_and(|cursor| cursor.header_footer.is_some())
                        {
                            // Esc leaves the header or footer.
                            actions.push(Action::Doc(Request::HeaderFooter(
                                HeaderFooterCommand::Exit,
                            )));
                        }
                        continue;
                    }
                    if !document {
                        continue;
                    }
                    let motion = |plain: Motion, word: Motion| if command { word } else { plain };
                    let request = match key {
                        Key::ArrowLeft => Some(Request::Move {
                            motion: motion(Motion::Left, Motion::WordLeft),
                            extend: shift,
                        }),
                        Key::ArrowRight => Some(Request::Move {
                            motion: motion(Motion::Right, Motion::WordRight),
                            extend: shift,
                        }),
                        Key::ArrowUp => Some(Request::Move {
                            motion: Motion::Up,
                            extend: shift,
                        }),
                        Key::ArrowDown => Some(Request::Move {
                            motion: Motion::Down,
                            extend: shift,
                        }),
                        Key::Home => Some(Request::Move {
                            motion: motion(Motion::Home, Motion::DocStart),
                            extend: shift,
                        }),
                        Key::End => Some(Request::Move {
                            motion: motion(Motion::End, Motion::DocEnd),
                            extend: shift,
                        }),
                        Key::PageUp => Some(Request::Move {
                            motion: Motion::PageUp,
                            extend: shift,
                        }),
                        Key::PageDown => Some(Request::Move {
                            motion: Motion::PageDown,
                            extend: shift,
                        }),
                        Key::Backspace => Some(Request::Backspace),
                        Key::Delete => Some(Request::Delete),
                        Key::Enter if command => Some(Request::Insert(Insert::PageBreak)),
                        Key::Enter => Some(Request::Enter),
                        Key::Tab if !command => Some(Request::Tab { back: shift }),
                        Key::B if command => Some(Request::Format(FormatChange::Bold)),
                        Key::I if command => Some(Request::Format(FormatChange::Italic)),
                        Key::U if command => Some(Request::Format(FormatChange::Underline)),
                        Key::E if command => Some(Request::Align(Align::Center)),
                        Key::L if command => Some(Request::Align(Align::Left)),
                        Key::R if command => Some(Request::Align(Align::Right)),
                        Key::J if command => Some(Request::Align(Align::Justify)),
                        Key::Z if command && shift => Some(Request::Redo),
                        Key::Z if command => Some(Request::Undo),
                        Key::Y if command => Some(Request::Redo),
                        Key::A if command => Some(Request::SelectAll),
                        Key::Equals | Key::Plus if command && shift => {
                            Some(Request::Format(FormatChange::Superscript))
                        }
                        Key::Equals if command => Some(Request::Format(FormatChange::Subscript)),
                        Key::CloseBracket if command => Some(Request::Format(FormatChange::Grow)),
                        Key::OpenBracket if command => Some(Request::Format(FormatChange::Shrink)),
                        Key::Space if command => Some(Request::Format(FormatChange::Clear)),
                        _ => None,
                    };
                    if let Some(request) = request {
                        actions.push(Action::Doc(request));
                    }
                }
                _ => {}
            }
        }
        // Ctrl+V with no text to paste: a copied picture of ours.
        if paste_key && !pasted && document {
            if let Some(request) = self.drawings_paste() {
                actions.push(Action::Doc(request));
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context, now: f64) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        for path in dropped {
            self.open_path(ctx, path, now);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        if self.exit_confirmed {
            self.settings.save();
            return;
        }
        if self.tabs.iter().any(|tab| tab.ready() && tab.modified()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::ConfirmExit);
        } else {
            self.settings.save();
        }
    }
}

/// Loads the installed dictionaries off the interface thread.
fn load_speller(ctx: &egui::Context) -> std::sync::mpsc::Receiver<crate::spell::Speller> {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(crate::spell::Speller::load());
        ctx.request_repaint();
    });
    rx
}

impl App {
    /// The speller the tabs mark words with: none while checking is off.
    fn speller_for_tabs(&self) -> Option<Arc<crate::spell::Speller>> {
        if self.settings.spelling {
            self.speller.clone()
        } else {
            None
        }
    }

    /// Dictionaries loaded or fetched since the last frame.
    fn poll_spelling(&mut self, ctx: &egui::Context, now: f64) {
        if let Some(speller) = self
            .speller_loading
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.speller_loading = None;
            self.speller = Some(Arc::new(speller));
            let speller = self.speller_for_tabs();
            for tab in &mut self.tabs {
                tab.set_speller(speller.clone());
            }
        }
        if let Some((code, result)) = self
            .dictionary_download
            .as_ref()
            .and_then(|(code, rx)| rx.try_recv().ok().map(|result| (*code, result)))
        {
            self.dictionary_download = None;
            match result {
                Ok(()) => {
                    let name = crate::spell::language(code).map_or("", |l| l.name_key);
                    let text = self
                        .i18n
                        .tf("spell.installed", &[("language", &self.t(name))]);
                    self.say(text, false, now);
                    self.speller_loading = Some(load_speller(ctx));
                }
                Err(error) => {
                    let text = self.i18n.tf("spell.download_failed", &[("error", &error)]);
                    self.say(text, true, now);
                }
            }
        }
    }

    fn download_dictionary(&mut self, ctx: &egui::Context, code: &'static str, now: f64) {
        if self.dictionary_download.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::spell::download(code));
            ctx.request_repaint();
        });
        self.dictionary_download = Some((code, rx));
        let name = crate::spell::language(code).map_or("", |l| l.name_key);
        let text = self
            .i18n
            .tf("spell.downloading", &[("language", &self.t(name))]);
        self.say(text, false, now);
    }

    /// Offers a dictionary for a script the document has words in.
    fn offer_dictionaries(&mut self, scripts: &[crate::spell::Script]) {
        if !self.settings.spelling || self.dialog.is_some() || self.dictionary_download.is_some() {
            return;
        }
        for script in scripts {
            let code = crate::spell::language_for(*script);
            if self.dictionaries_offered.contains(&code) || crate::spell::installed(code) {
                continue;
            }
            self.dictionaries_offered.push(code);
            self.dialog = Some(Dialog::Dictionary { code });
            return;
        }
    }

    /// A dialog by the name `MUFOED_DIALOG` gives, to look at.
    fn open_named_dialog(&mut self, name: &str, actions: &mut Vec<Action>) {
        self.dialog = match name {
            "table" => Some(Dialog::Table {
                rows: 3,
                columns: 4,
            }),
            "columns" => Some(Dialog::Columns {
                count: 2,
                space_cm: 1.25,
                separator: false,
            }),
            "page-start" => Some(Dialog::PageStart {
                start: 1,
                restart: false,
            }),
            "bookmarks" => Some(Dialog::Bookmarks {
                name: String::new(),
                selected: None,
            }),
            "symbols" => Some(Dialog::Symbols),
            "words" => Some(Dialog::WordCount),
            "settings" => Some(Dialog::Settings),
            "about" => Some(Dialog::About),
            "exit" => Some(Dialog::ConfirmExit),
            "dictionary" => Some(Dialog::Dictionary { code: "ru" }),
            "print" => {
                actions.push(Action::PrintDialog);
                None
            }
            _ => None,
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let input = input_summary(&ctx);
        crate::trace::frame(&input);
        if !input.is_empty() {
            crate::trace::log(|| format!("input {input}"));
        }
        if self.applied_dark != Some(self.settings.dark) {
            theme::apply(&ctx, self.settings.dark);
            self.applied_dark = Some(self.settings.dark);
        }
        let now = ctx.input(|input| input.time);
        for path in std::mem::take(&mut self.startup) {
            self.open_path(&ctx, path, now);
        }
        if let Some(shot) = self.shot.as_mut() {
            shot.frame(&ctx);
        }
        let startup_dialog = self.startup_dialog.take();
        self.poll_tabs(&ctx, now);
        self.poll_spelling(&ctx, now);
        self.handle_drops(&ctx, now);
        self.handle_close_request(&ctx);
        let mut actions = Vec::new();
        if let Some(name) = startup_dialog {
            self.open_named_dialog(&name, &mut actions);
        }
        self.keyboard(&ctx, &mut actions);
        let palette = self.palette();

        egui::Panel::top("title-strip")
            .exact_size(36.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.title_bar)
                    .inner_margin(egui::Margin::symmetric(6, 0)),
            )
            .show(ui, |ui| self.title_strip(ui, palette, &mut actions));

        if self.backstage {
            egui::CentralPanel::no_frame()
                .show(ui, |ui| self.backstage_ui(ui, palette, &mut actions));
        } else {
            egui::Panel::top("ribbon")
                .frame(
                    egui::Frame::new()
                        .fill(palette.ribbon)
                        .inner_margin(egui::Margin {
                            left: 8,
                            right: 8,
                            top: 4,
                            bottom: 2,
                        })
                        .stroke(Stroke::new(1.0, palette.ribbon_line)),
                )
                .show(ui, |ui| self.ribbon_ui(ui, palette, &mut actions));
            egui::Panel::bottom("status")
                .exact_size(26.0)
                .frame(
                    egui::Frame::new()
                        .fill(palette.status)
                        .inner_margin(egui::Margin::symmetric(10, 2))
                        .stroke(Stroke::new(1.0, palette.ribbon_line)),
                )
                .show(ui, |ui| self.status_bar(ui, palette, now, &mut actions));
            if self.active_tab().is_some() {
                if self.settings.navigation {
                    egui::Panel::left("navigation")
                        .resizable(true)
                        .default_size(220.0)
                        .size_range(140.0..=420.0)
                        .frame(egui::Frame::new().fill(palette.ribbon).inner_margin(8))
                        .show(ui, |ui| self.navigation_ui(ui, &mut actions));
                }
                let has_comments = self
                    .active_tab()
                    .is_some_and(|tab| !tab.comments.is_empty());
                let _ = has_comments;
                if self.comments_open && self.active_tab().is_some() {
                    egui::Panel::right("comments")
                        .resizable(true)
                        .default_size(300.0)
                        .size_range(220.0..=560.0)
                        .frame(egui::Frame::new().fill(palette.ribbon).inner_margin(10))
                        .show(ui, |ui| self.comments_ui(ui, palette));
                }
                if self.report_open {
                    egui::Panel::right("report")
                        .resizable(true)
                        .default_size(360.0)
                        .size_range(220.0..=720.0)
                        .frame(egui::Frame::new().fill(palette.ribbon).inner_margin(8))
                        .show(ui, |ui| self.report_ui(ui));
                }
            }
            egui::CentralPanel::no_frame().show(ui, |ui| {
                let dialog_open = self.dialog.is_some();
                match self.active {
                    Some(index) => {
                        if let Some(tab) = self.tabs.get_mut(index) {
                            match &tab.status {
                                // Opened, but no page laid out yet.
                                Status::Ready if tab.pages.is_empty() && tab.busy.is_some() => {
                                    loading_ui(ui, tab, &self.i18n, palette);
                                }
                                Status::Ready => crate::ruler::desk(
                                    ui,
                                    tab,
                                    palette,
                                    !dialog_open,
                                    self.settings.ruler,
                                    &self.i18n,
                                ),
                                Status::Loading => loading_ui(ui, tab, &self.i18n, palette),
                                Status::Failed(error) => {
                                    let error = error.clone();
                                    failed_ui(ui, &error, &self.i18n, palette);
                                }
                            }
                        }
                    }
                    None => self.start_page(ui, palette, &mut actions),
                }
                if self.find_open && self.active_tab().is_some() {
                    self.find_bar(&ctx, ui.max_rect(), &mut actions);
                }
            });
        }
        self.dialogs(&ctx, &mut actions);
        let title = self.active_tab().map_or_else(
            || "MultiFormatEditor".to_owned(),
            |tab| {
                let mark = if tab.modified() { "• " } else { "" };
                format!("{mark}{} — MultiFormatEditor", tab.title)
            },
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
        for action in actions {
            self.run(&ctx, action, now);
        }
    }

    /// Tab belongs to the document (a tab character, the next cell) and to
    /// Ctrl+Tab between documents; egui would move keyboard focus to the
    /// next button with it before the frame starts, and Enter would then
    /// press that button. Unless a text field has focus, Tab is taken out
    /// of egui's input here and handled with the other keys.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if ctx.egui_wants_keyboard_input() || self.dialog.is_some() {
            return;
        }
        let is_tab = |event: &egui::Event| matches!(event, egui::Event::Key { key: Key::Tab, .. });
        if !raw_input.events.iter().any(is_tab) {
            return;
        }
        // The keys are handled in the order they came: a Tab between two
        // letters typed in one frame stays between them.
        self.frame_events = Some(raw_input.events.clone());
        raw_input.events.retain(|event| !is_tab(event));
    }

    fn on_exit(&mut self) {
        self.settings.save();
    }
}

// ----- parts of the window ----------------------------------------------

impl App {
    fn title_strip(&mut self, ui: &mut egui::Ui, palette: Palette, actions: &mut Vec<Action>) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // The start page tab, with the application's mark.
            let home_selected = self.active.is_none();
            let (rect, response) = ui.allocate_exact_size(vec2(118.0, 30.0), Sense::click());
            if home_selected {
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius {
                        nw: 6,
                        ne: 6,
                        sw: 0,
                        se: 0,
                    },
                    palette.tab_active,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 6.0, palette.hover);
            }
            let mark =
                egui::Rect::from_min_size(rect.left_center() + vec2(8.0, -10.0), vec2(20.0, 20.0));
            ui.painter().rect_filled(mark, 4.0, theme::ACCENT);
            ui.painter().text(
                mark.center(),
                egui::Align2::CENTER_CENTER,
                "M",
                FontId::new(13.0, FontFamily::Name(theme::BOLD.into())),
                Color32::WHITE,
            );
            ui.painter().text(
                mark.right_center() + vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "MuFoEd",
                FontId::new(14.0, FontFamily::Name(theme::BOLD.into())),
                palette.tab_text,
            );
            if response.on_hover_text(self.t("tip.start_page")).clicked() {
                actions.push(Action::Activate(None));
            }
            ui.add_space(4.0);
            let mut close = None;
            for (index, tab) in self.tabs.iter().enumerate() {
                let selected = self.active == Some(index);
                let title = if tab.title.chars().count() > 26 {
                    format!("{}…", tab.title.chars().take(25).collect::<String>())
                } else {
                    tab.title.clone()
                };
                let galley = ui.painter().layout_no_wrap(
                    title,
                    FontId::proportional(13.0),
                    palette.tab_text,
                );
                let width = (galley.size().x + 70.0).clamp(120.0, 260.0);
                let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
                if selected {
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius {
                            nw: 6,
                            ne: 6,
                            sw: 0,
                            se: 0,
                        },
                        palette.tab_active,
                    );
                    ui.painter().line_segment(
                        [
                            rect.left_top() + vec2(6.0, 0.0),
                            rect.right_top() - vec2(6.0, 0.0),
                        ],
                        Stroke::new(2.0, theme::ACCENT),
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 6.0, palette.hover);
                }
                let kind = tab.kind.unwrap_or(Kind::Docx);
                let badge = egui::Rect::from_min_size(
                    rect.left_center() + vec2(8.0, -8.0),
                    vec2(16.0, 16.0),
                );
                let badge_color = match kind {
                    Kind::Pdf => Color32::from_rgb(200, 40, 40),
                    Kind::Djvu => Color32::from_rgb(120, 80, 160),
                    Kind::Rtf => Color32::from_rgb(40, 140, 120),
                    _ => theme::ACCENT,
                };
                ui.painter().rect_filled(badge, 3.0, badge_color);
                ui.painter().text(
                    badge.center(),
                    egui::Align2::CENTER_CENTER,
                    kind.badge(),
                    FontId::new(11.0, FontFamily::Name(theme::BOLD.into())),
                    Color32::WHITE,
                );
                ui.painter().galley(
                    rect.left_center() + vec2(30.0, -galley.size().y / 2.0),
                    galley,
                    palette.tab_text,
                );
                let close_rect = egui::Rect::from_center_size(
                    rect.right_center() - vec2(14.0, 0.0),
                    vec2(18.0, 18.0),
                );
                let close_response =
                    ui.interact(close_rect, ui.id().with(("close", tab.id)), Sense::click());
                if tab.modified() && !close_response.hovered() {
                    ui.painter()
                        .circle_filled(close_rect.center(), 3.5, palette.tab_text);
                } else {
                    if close_response.hovered() {
                        ui.painter().rect_filled(
                            close_rect,
                            3.0,
                            Color32::from_rgba_unmultiplied(200, 60, 60, 60),
                        );
                    }
                    icons::paint(
                        ui.painter(),
                        close_rect.shrink(2.0),
                        Icon::Close,
                        palette.tab_text,
                        theme::ACCENT,
                    );
                }
                if close_response.clicked() {
                    close = Some(index);
                } else if response.clicked() {
                    actions.push(Action::Activate(Some(index)));
                } else if response.middle_clicked() {
                    close = Some(index);
                }
                let tip = tab
                    .source
                    .as_ref()
                    .map_or_else(|| tab.title.clone(), |path| path.display().to_string());
                response.on_hover_text(tip);
            }
            if let Some(index) = close {
                actions.push(Action::CloseTab(index));
            }
            let plus = ui.add(
                egui::Button::new(RichText::new("+").size(18.0))
                    .frame(false)
                    .min_size(vec2(28.0, 28.0)),
            );
            if plus.on_hover_text(self.t("tip.new_tab")).clicked() {
                actions.push(Action::New);
            }
        });
    }

    fn ribbon_ui(&mut self, ui: &mut egui::Ui, palette: Palette, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            // Tools for what is being worked on: a selected drawing, or a
            // header or footer being edited. They open when it starts and
            // give the tab that was open back when it ends.
            let cursor = self.active_tab().and_then(|tab| tab.cursor.as_ref());
            let tool = if cursor.is_some_and(|cursor| cursor.object.is_some()) {
                Some(RibbonTab::Picture)
            } else if cursor.is_some_and(|cursor| cursor.header_footer.is_some()) {
                Some(RibbonTab::HeaderFooter)
            } else {
                None
            };
            if tool != self.tool_shown {
                let on_tool = Some(self.ribbon_tab) == self.tool_shown;
                if let Some(tool) = tool {
                    if !on_tool {
                        self.tab_before_tool = self.ribbon_tab;
                    }
                    self.ribbon_tab = tool;
                } else if on_tool {
                    self.ribbon_tab = self.tab_before_tool;
                }
                self.tool_shown = tool;
            }
            if ribbon::tab_strip(ui, &mut self.ribbon_tab, tool, &self.i18n, palette) {
                actions.push(Action::Backstage(true));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let state = self
                    .active_tab()
                    .map(|tab| (tab.ready(), tab.state.clone()));
                let ready = state.as_ref().is_some_and(|(ready, _)| *ready);
                let edit = state.map(|(_, state)| state).unwrap_or_default();
                // The quick access toolbar.
                for (icon, tip, enabled, action) in [
                    (
                        Icon::Redo,
                        "tip.redo",
                        ready && edit.can_redo,
                        Action::Doc(Request::Redo),
                    ),
                    (
                        Icon::Undo,
                        "tip.undo",
                        ready && edit.can_undo,
                        Action::Doc(Request::Undo),
                    ),
                    (Icon::Save, "tip.save", ready, Action::Save),
                ] {
                    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
                    let hovered = enabled && response.hovered();
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        if hovered {
                            palette.hover
                        } else {
                            palette.button
                        },
                    );
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        Stroke::new(
                            1.0,
                            if hovered {
                                theme::ACCENT.gamma_multiply(0.55)
                            } else {
                                palette.button_border
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let color = if enabled {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    let accent = if enabled {
                        theme::ACCENT
                    } else {
                        Color32::from_gray(150)
                    };
                    icons::paint(
                        ui.painter(),
                        rect.shrink2(vec2(5.0, 4.0)),
                        icon,
                        color,
                        accent,
                    );
                    if response.on_hover_text(self.i18n.t(tip)).clicked() && enabled {
                        actions.push(action);
                    }
                }
            });
        });
        let tab = self.active_tab();
        let empty_format = CaretFormat::default();
        let empty_state = EditState::default();
        let ready = tab.is_some_and(DocTab::ready);
        let state = RibbonState {
            i18n: &self.i18n,
            palette,
            format: tab
                .and_then(|tab| tab.cursor.as_ref())
                .map_or(&empty_format, |cursor| &cursor.format),
            styles: tab
                .and_then(|tab| tab.info.as_ref())
                .map_or(&[], |info| info.styles.as_slice()),
            state: tab.map_or(&empty_state, |tab| &tab.state),
            document: ready,
            navigation: self.settings.navigation,
            ruler: self.settings.ruler,
            report: self.report_open,
            dark: self.settings.dark,
            zoom: tab.map_or(1.0, |tab| tab.zoom),
            system_fonts: self.settings.system_fonts,
            font_color: self.font_color,
            object: tab
                .and_then(|tab| tab.cursor.as_ref())
                .and_then(|cursor| cursor.object.as_ref()),
            crop: tab.is_some_and(|tab| tab.view.crop),
            comments_open: self.comments_open,
            spelling: self.settings.spelling,
            header_footer: tab
                .and_then(|tab| tab.cursor.as_ref())
                .and_then(|cursor| cursor.header_footer.as_ref()),
            fonts: &self.doc_fonts,
            document_fonts: tab
                .and_then(|tab| tab.info.as_ref())
                .map_or(&[], |info| info.fonts.as_slice()),
        };
        ui.add_space(2.0);
        ribbon::body(ui, self.ribbon_tab, &state, actions);
    }

    #[allow(clippy::too_many_lines)]
    fn status_bar(
        &mut self,
        ui: &mut egui::Ui,
        palette: Palette,
        now: f64,
        actions: &mut Vec<Action>,
    ) {
        ui.horizontal_centered(|ui| {
            let text = |s: String| RichText::new(s).size(12.0).color(palette.status_text);
            if let Some(tab) = self.active_tab() {
                if tab.ready() {
                    let pages = tab.pages.len().max(1);
                    let page = (tab.view.current_page + 1).min(pages);
                    // While the whole document is laid out, the pages shown
                    // are the first few.
                    let counting = tab.busy == Some(Busy::Layout);
                    ui.label(text(if counting {
                        self.i18n.tf("status.page_counting", &[("page", &page)])
                    } else {
                        self.i18n
                            .tf("status.page", &[("page", &page), ("pages", &pages)])
                    }));
                    ui.separator();
                    if let Some(busy) = tab.busy {
                        ui.add(egui::Spinner::new().size(12.0).color(palette.status_text));
                        ui.label(text(
                            self.t(match busy {
                                Busy::Layout => "status.layout",
                                Busy::Map => "status.map",
                                Busy::Recognize => "status.recognizing",
                            })
                            .to_owned(),
                        ));
                        ui.separator();
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    ui.label(text(
                        self.i18n.tf("status.words", &[("words", &tab.state.words)]),
                    ));
                    ui.separator();
                    if let Some(kind) = tab.kind {
                        ui.label(text(self.t(kind.key()).to_owned()));
                        ui.separator();
                    }
                    if tab.state.read_only.is_some() {
                        ui.label(text(self.t("status.read_only").to_owned()))
                            .on_hover_text(tab.state.read_only.clone().unwrap_or_default());
                        ui.separator();
                    }
                    if !tab.warnings.is_empty() {
                        let label = ui.label(
                            RichText::new(
                                self.i18n
                                    .tf("status.warnings", &[("count", &tab.warnings.len())]),
                            )
                            .size(12.0)
                            .color(Color32::from_rgb(200, 120, 0)),
                        );
                        if label.on_hover_text(self.t("tip.warnings")).clicked() {
                            actions.push(Action::ToggleReport);
                        }
                        ui.separator();
                    }
                }
            }
            if let Some(status) = &self.status {
                if now - status.at < 12.0 || status.error {
                    let color = if status.error {
                        Color32::from_rgb(200, 50, 50)
                    } else {
                        palette.status_text
                    };
                    let shown: String = status.text.chars().take(160).collect();
                    let label = ui.label(RichText::new(shown).size(12.0).color(color));
                    let full = status.text.clone();
                    if label.on_hover_text(full).clicked() {
                        self.status = None;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let Some(index) = self.active else {
                    return;
                };
                let Some(tab) = self.tabs.get_mut(index) else {
                    return;
                };
                if !tab.ready() {
                    return;
                }
                #[allow(clippy::cast_possible_truncation)]
                let percent = (tab.zoom * 100.0).round() as i32;
                if ui
                    .add(egui::Button::new(text(format!("{percent}%"))).frame(false))
                    .clicked()
                {
                    actions.push(Action::Zoom(Zoom::Set(1.0)));
                }
                if ui
                    .add(egui::Button::new(text("+".to_owned())).frame(false))
                    .clicked()
                {
                    actions.push(Action::Zoom(Zoom::In));
                }
                let mut zoom = tab.zoom;
                let slider = ui.add_sized(
                    vec2(120.0, 18.0),
                    egui::Slider::new(&mut zoom, 0.25..=5.0)
                        .logarithmic(true)
                        .show_value(false),
                );
                if slider.changed() {
                    tab.zoom = zoom;
                    tab.view.zoom_changed = now;
                }
                if ui
                    .add(egui::Button::new(text("−".to_owned())).frame(false))
                    .clicked()
                {
                    actions.push(Action::Zoom(Zoom::Out));
                }
                ui.separator();
                for (icon, tip, zoom) in [
                    (Icon::PageWidth, "tip.page_width", Zoom::PageWidth),
                    (Icon::OnePage, "tip.one_page", Zoom::OnePage),
                ] {
                    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 3.0, palette.hover);
                    }
                    icons::paint(
                        ui.painter(),
                        rect.shrink(3.0),
                        icon,
                        palette.status_text,
                        theme::ACCENT,
                    );
                    if response.on_hover_text(self.i18n.t(tip)).clicked() {
                        actions.push(Action::Zoom(zoom));
                    }
                }
            });
        });
    }

    fn navigation_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(index) = self.active else {
            return;
        };
        ui.horizontal(|ui| {
            icons::icon_widget(ui, Icon::Navigation, 16.0, theme::ACCENT);
            ui.label(
                RichText::new(self.t("nav.title")).family(FontFamily::Name(theme::BOLD.into())),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").clicked() {
                    actions.push(Action::ToggleNavigation);
                }
            });
        });
        ui.separator();
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let headings = self.i18n.t("nav.headings");
        let pages_label = self.i18n.t("nav.pages");
        egui::ScrollArea::vertical()
            .id_salt(("nav", tab.id))
            .show(ui, |ui| {
                egui::CollapsingHeader::new(headings)
                    .default_open(true)
                    .show(ui, |ui| {
                        if tab.outline.is_empty() {
                            ui.label(
                                RichText::new(self.i18n.t("nav.no_headings"))
                                    .weak()
                                    .size(12.0),
                            );
                        }
                        let dark = self.settings.dark;
                        for entry in &tab.outline {
                            ui.horizontal(|ui| {
                                ui.add_space(f32::from(entry.level) * 12.0);
                                let text = heading_text(
                                    ui.ctx(),
                                    &mut self.doc_fonts.borrow_mut(),
                                    entry,
                                    dark,
                                );
                                if ui
                                    .add(egui::Button::new(text).frame(false).wrap())
                                    .clicked()
                                {
                                    tab.send(Request::GoTo(entry.paragraph.clone()));
                                }
                            });
                        }
                    });
                egui::CollapsingHeader::new(pages_label)
                    .default_open(true)
                    .show(ui, |ui| {
                        for page in 0..tab.pages.len() {
                            let selected = tab.view.current_page == page;
                            let label = self.i18n.tf("nav.page", &[("page", &(page + 1))]);
                            if ui.selectable_label(selected, label).clicked() {
                                tab.view.scroll_to_page = Some(page);
                            }
                        }
                    });
            });
    }

    /// The comments pane: a card per comment, in the order of the text; the
    /// one being edited has a text box.
    fn comments_ui(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let close = self.t("button.close").to_owned();
        let title = self.t("comments.title").to_owned();
        let empty = self.t("comments.none").to_owned();
        let edit_label = self.t("comments.edit").to_owned();
        let delete_label = self.t("comments.delete").to_owned();
        let done_label = self.t("comments.done").to_owned();
        let hint = self.t("comments.hint").to_owned();
        let mut closing = false;
        ui.horizontal(|ui| {
            icons::icon_widget(ui, Icon::Comment, 16.0, theme::ACCENT);
            ui.label(RichText::new(&title).family(FontFamily::Name(theme::BOLD.into())));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(&close).clicked() {
                    closing = true;
                }
            });
        });
        if closing {
            self.comments_open = false;
            return;
        }
        ui.add_space(6.0);
        let Some(index) = self.active else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let mut requests = Vec::new();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if tab.comments.is_empty() {
                ui.label(RichText::new(&empty).weak());
            }
            for comment in &tab.comments {
                let editing = tab
                    .editing_comment
                    .as_ref()
                    .is_some_and(|(id, _)| *id == comment.id);
                let frame = egui::Frame::new()
                    .fill(if editing {
                        palette.pressed
                    } else {
                        ui.visuals().extreme_bg_color
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if editing {
                            theme::ACCENT
                        } else {
                            palette.button_border
                        },
                    ))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 8));
                let response = frame
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if comment.author.is_empty() {
                                    "—"
                                } else {
                                    &comment.author
                                })
                                .family(FontFamily::Name(theme::BOLD.into())),
                            );
                            ui.label(
                                RichText::new(comment.date.replace('T', " ").trim_end_matches('Z'))
                                    .weak()
                                    .size(11.0),
                            );
                        });
                        ui.add_space(4.0);
                        if editing {
                            if let Some((_, draft)) = tab.editing_comment.as_mut() {
                                let field = ui.add(
                                    egui::TextEdit::multiline(draft)
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY)
                                        .hint_text(&hint),
                                );
                                field.request_focus();
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if ui.button(&done_label).clicked() {
                                        requests.push(CommentCommand::Edit {
                                            id: comment.id.clone(),
                                            text: draft.clone(),
                                        });
                                    }
                                });
                            }
                        } else {
                            ui.label(&comment.text);
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.small_button(&edit_label).clicked() {
                                    requests.push(CommentCommand::Select(comment.id.clone()));
                                    tab.editing_comment =
                                        Some((comment.id.clone(), comment.text.clone()));
                                }
                                if ui.small_button(&delete_label).clicked() {
                                    requests.push(CommentCommand::Delete(comment.id.clone()));
                                }
                            });
                        }
                    })
                    .response;
                if response.interact(Sense::click()).clicked() && !editing {
                    requests.push(CommentCommand::Select(comment.id.clone()));
                }
                ui.add_space(8.0);
            }
        });
        for request in requests {
            if matches!(
                request,
                CommentCommand::Edit { .. } | CommentCommand::Delete(_)
            ) {
                tab.editing_comment = None;
            }
            tab.send(Request::Comments(request));
        }
    }

    fn report_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            icons::icon_widget(ui, Icon::Report, 16.0, theme::ACCENT);
            ui.label(
                RichText::new(self.t("report.title")).family(FontFamily::Name(theme::BOLD.into())),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").clicked() {
                    self.report_open = false;
                }
            });
        });
        ui.separator();
        let Some(tab) = self.active_tab() else {
            return;
        };
        let mut text = String::new();
        if let Some(info) = &tab.info {
            text.push_str(&info.report);
            if !info.log.trim().is_empty() {
                text.push_str("\n\n");
                text.push_str(self.t("report.import_log"));
                text.push_str(":\n");
                text.push_str(&info.log);
            }
        }
        if !tab.warnings.is_empty() {
            text.push_str("\n\n");
            text.push_str(self.t("report.render_warnings"));
            text.push_str(":\n");
            for warning in &tab.warnings {
                text.push_str(warning);
                text.push('\n');
            }
        }
        if let Some(info) = tab.info.as_ref().filter(|info| !info.defects.is_empty()) {
            text.push_str("\n\n");
            text.push_str(self.t("report.source_defects"));
            text.push_str(":\n");
            for defect in &info.defects {
                text.push_str("  ");
                text.push_str(defect);
                text.push('\n');
            }
        }
        if let Some(reason) = &tab.state.read_only {
            text.push_str("\n\n");
            text.push_str(self.t("report.read_only"));
            text.push_str(":\n");
            text.push_str(reason);
        }
        egui::ScrollArea::both()
            .id_salt(("report", tab.id))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(text).monospace().size(11.5)).selectable(true),
                );
            });
    }

    fn start_page(&mut self, ui: &mut egui::Ui, palette: Palette, actions: &mut Vec<Action>) {
        egui::Frame::new().fill(palette.desk).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(36.0);
                    ui.label(
                        RichText::new("MultiFormatEditor")
                            .size(30.0)
                            .family(FontFamily::Name(theme::BOLD.into()))
                            .color(theme::ACCENT),
                    );
                    ui.label(RichText::new(self.t("start.subtitle")).size(14.0));
                    ui.add_space(24.0);
                    ui.horizontal(|ui| {
                        let width = 4.0 * 150.0 + 3.0 * 14.0;
                        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                        ui.spacing_mut().item_spacing.x = 14.0;
                        for (icon, key, tip, action) in [
                            (Icon::New, "start.new", "tip.new", Action::New),
                            (Icon::Open, "start.open", "tip.open", Action::OpenDialog),
                            (
                                Icon::Import,
                                "start.import",
                                "tip.import_pdf",
                                Action::ImportDialog { ocr: false },
                            ),
                            (
                                Icon::Scan,
                                "start.recognize",
                                "tip.recognize",
                                Action::ImportDialog { ocr: true },
                            ),
                        ] {
                            if tile(ui, icon, self.i18n.t(key), palette)
                                .on_hover_text(self.i18n.t(tip))
                                .clicked()
                            {
                                actions.push(action);
                            }
                        }
                    });
                    ui.add_space(28.0);
                    let width = 640.0_f32.min(ui.available_width() - 32.0);
                    egui::Frame::new()
                        .fill(palette.ribbon)
                        .corner_radius(6)
                        .inner_margin(14)
                        .stroke(Stroke::new(1.0, palette.ribbon_line))
                        .show(ui, |ui| {
                            ui.set_width(width);
                            ui.label(
                                RichText::new(self.t("start.recent"))
                                    .size(15.0)
                                    .family(FontFamily::Name(theme::BOLD.into())),
                            );
                            ui.add_space(6.0);
                            if self.settings.recent.is_empty() {
                                ui.label(RichText::new(self.t("start.no_recent")).weak());
                            }
                            for path in self.settings.recent.clone() {
                                let exists = path.exists();
                                let name = file_name(&path);
                                let response = ui.add_enabled(
                                    exists,
                                    egui::Button::new(
                                        RichText::new(format!(
                                            "{name}\n{}",
                                            path.parent()
                                                .map(|p| p.display().to_string())
                                                .unwrap_or_default()
                                        ))
                                        .size(12.5),
                                    )
                                    .frame(false)
                                    .wrap(),
                                );
                                if response.clicked() {
                                    actions.push(Action::OpenPath(path.clone()));
                                }
                            }
                        });
                    ui.add_space(16.0);
                    ui.label(RichText::new(self.t("start.hint")).weak().size(12.0));
                });
            });
        });
    }

    fn backstage_ui(&mut self, ui: &mut egui::Ui, palette: Palette, actions: &mut Vec<Action>) {
        let ready = self.active_tab().is_some_and(DocTab::ready);
        egui::Panel::left("backstage-menu")
            .exact_size(210.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::ACCENT)
                    .inner_margin(egui::Margin::symmetric(0, 10)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let item = |ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool| -> bool {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
                    if enabled && response.hovered() {
                        ui.painter()
                            .rect_filled(rect, 0.0, Color32::from_white_alpha(30));
                    }
                    let color = if enabled {
                        Color32::WHITE
                    } else {
                        Color32::from_white_alpha(110)
                    };
                    icons::paint(
                        ui.painter(),
                        egui::Rect::from_min_size(
                            rect.left_center() + vec2(18.0, -9.0),
                            vec2(18.0, 18.0),
                        ),
                        icon,
                        color,
                        Color32::from_rgb(170, 200, 245),
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(48.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(14.0),
                        color,
                    );
                    enabled && response.clicked()
                };
                if item(ui, Icon::Undo, self.i18n.t("backstage.back"), true) {
                    actions.push(Action::Backstage(false));
                }
                ui.add_space(8.0);
                if item(ui, Icon::New, self.i18n.t("cmd.new"), true) {
                    actions.push(Action::New);
                }
                if item(ui, Icon::Open, self.i18n.t("cmd.open"), true) {
                    actions.push(Action::OpenDialog);
                }
                if item(ui, Icon::Import, self.i18n.t("cmd.import_pdf"), true) {
                    actions.push(Action::ImportDialog { ocr: false });
                }
                if item(
                    ui,
                    Icon::Scan,
                    self.i18n.t("cmd.recognize"),
                    cfg!(feature = "ocr"),
                ) {
                    actions.push(Action::ImportDialog { ocr: true });
                }
                if item(ui, Icon::Save, self.i18n.t("cmd.save"), ready) {
                    actions.push(Action::Save);
                }
                if item(ui, Icon::SaveAs, self.i18n.t("cmd.save_as"), ready) {
                    actions.push(Action::SaveAs);
                }
                if item(ui, Icon::Print, self.i18n.t("cmd.print"), ready) {
                    actions.push(Action::PrintDialog);
                    actions.push(Action::Backstage(false));
                }
                if item(ui, Icon::Pdf, self.i18n.t("cmd.export_pdf"), ready) {
                    actions.push(Action::Export(SaveFormat::Pdf));
                }
                if item(ui, Icon::Svg, self.i18n.t("cmd.export_svg"), ready) {
                    actions.push(Action::Export(SaveFormat::Svg));
                }
                if item(ui, Icon::Txt, self.i18n.t("cmd.export_txt"), ready) {
                    actions.push(Action::Export(SaveFormat::Text));
                }
                if item(
                    ui,
                    Icon::Close,
                    self.i18n.t("cmd.close"),
                    self.active.is_some(),
                ) {
                    if let Some(index) = self.active {
                        actions.push(Action::CloseTab(index));
                    }
                    actions.push(Action::Backstage(false));
                }
                ui.add_space(8.0);
                if item(ui, Icon::Settings, self.i18n.t("cmd.settings"), true) {
                    actions.push(Action::Open(Dialog::Settings));
                }
                if item(ui, Icon::About, self.i18n.t("cmd.about"), true) {
                    actions.push(Action::Open(Dialog::About));
                }
                if item(ui, Icon::Close, self.i18n.t("cmd.exit"), true) {
                    actions.push(Action::Exit);
                }
            });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            egui::Frame::new()
                .fill(palette.ribbon)
                .inner_margin(24)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(RichText::new(self.t("backstage.info")).size(24.0));
                    ui.add_space(10.0);
                    if let Some(tab) = self.active_tab() {
                        egui::Grid::new("info")
                            .num_columns(2)
                            .spacing(vec2(18.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(self.t("info.name")).weak());
                                ui.label(&tab.title);
                                ui.end_row();
                                ui.label(RichText::new(self.t("info.location")).weak());
                                ui.label(tab.source.as_ref().map_or_else(
                                    || "—".to_owned(),
                                    |path| path.display().to_string(),
                                ));
                                ui.end_row();
                                ui.label(RichText::new(self.t("info.format")).weak());
                                ui.label(tab.kind.map_or("—", |kind| self.i18n.t(kind.key())));
                                ui.end_row();
                                ui.label(RichText::new(self.t("info.pages")).weak());
                                ui.label(tab.pages.len().to_string());
                                ui.end_row();
                                ui.label(RichText::new(self.t("info.words")).weak());
                                ui.label(tab.state.words.to_string());
                                ui.end_row();
                                ui.label(RichText::new(self.t("info.characters")).weak());
                                ui.label(tab.state.characters.to_string());
                                ui.end_row();
                                ui.label(RichText::new(self.t("info.save_target")).weak());
                                ui.label(tab.path.as_ref().map_or_else(
                                    || self.t("info.not_saved").to_owned(),
                                    |path| path.display().to_string(),
                                ));
                                ui.end_row();
                            });
                    } else {
                        ui.label(self.t("backstage.no_document"));
                    }
                    ui.add_space(24.0);
                    ui.label(RichText::new(self.t("start.recent")).size(18.0));
                    ui.add_space(6.0);
                    for path in self.settings.recent.clone() {
                        if ui
                            .add_enabled(
                                path.exists(),
                                egui::Button::new(path.display().to_string()).frame(false),
                            )
                            .clicked()
                        {
                            actions.push(Action::OpenPath(path.clone()));
                        }
                    }
                });
        });
    }

    fn find_bar(&mut self, ctx: &egui::Context, area: egui::Rect, actions: &mut Vec<Action>) {
        let mut open = true;
        let title = if self.find_replace {
            self.t("find.replace_title")
        } else {
            self.t("find.title")
        }
        .to_owned();
        egui::Window::new(title)
            .id(egui::Id::new("find-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(area.right_top() + vec2(-380.0, 12.0))
            .show(ctx, |ui| {
                let query = |find: &FindState| FindQuery {
                    text: find.text.clone(),
                    case_sensitive: find.case_sensitive,
                    whole_word: find.whole_word,
                };
                ui.horizontal(|ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.find.text)
                            .hint_text(self.i18n.t("find.what"))
                            .desired_width(200.0),
                    );
                    if self.find.focus {
                        edit.request_focus();
                        self.find.focus = false;
                    }
                    let enter =
                        edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                    if ui
                        .button("▲")
                        .on_hover_text(self.i18n.t("find.previous"))
                        .clicked()
                    {
                        actions.push(Action::Doc(Request::Find {
                            query: query(&self.find),
                            backwards: true,
                        }));
                    }
                    if ui
                        .button("▼")
                        .on_hover_text(self.i18n.t("find.next"))
                        .clicked()
                        || enter
                    {
                        actions.push(Action::Doc(Request::Find {
                            query: query(&self.find),
                            backwards: false,
                        }));
                        if enter {
                            self.find.focus = true;
                        }
                    }
                });
                if self.find_replace {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.find.with)
                                .hint_text(self.i18n.t("find.with"))
                                .desired_width(200.0),
                        );
                        if ui.button(self.i18n.t("find.replace")).clicked() {
                            actions.push(Action::Doc(Request::Replace {
                                query: query(&self.find),
                                with: self.find.with.clone(),
                            }));
                        }
                        if ui.button(self.i18n.t("find.replace_all")).clicked() {
                            actions.push(Action::Doc(Request::ReplaceAll {
                                query: query(&self.find),
                                with: self.find.with.clone(),
                            }));
                        }
                    });
                } else if ui.small_button(self.i18n.t("find.show_replace")).clicked() {
                    self.find_replace = true;
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.find.case_sensitive, self.i18n.t("find.case"));
                    ui.checkbox(&mut self.find.whole_word, self.i18n.t("find.whole"));
                });
            });
        if !open {
            self.find_open = false;
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut next: Option<Dialog> = None;
        let t = |key: &'static str| self.i18n.t(key).to_owned();
        let modal = |id: &str| crate::dialog::modal(ctx, id);
        match dialog {
            Dialog::Table {
                mut rows,
                mut columns,
            } => {
                let response = modal("table").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("table.title"));
                    // A grid to pick the size from, as in Word.
                    let cell = 18.0;
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(cell * 10.0, cell * 8.0), Sense::click());
                    if let Some(pos) = response.hover_pos() {
                        let local = pos - rect.min;
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        {
                            columns = ((local.x / cell).floor() as usize + 1).clamp(1, 10);
                            rows = ((local.y / cell).floor() as usize + 1).clamp(1, 8);
                        }
                    }
                    for row in 0..8 {
                        for column in 0..10 {
                            #[allow(clippy::cast_precision_loss)]
                            let r = egui::Rect::from_min_size(
                                rect.min + vec2(column as f32 * cell, row as f32 * cell),
                                vec2(cell - 2.0, cell - 2.0),
                            );
                            let on = row < rows && column < columns;
                            ui.painter().rect(
                                r,
                                1.0,
                                if on {
                                    theme::ACCENT_LIGHT
                                } else {
                                    ui.visuals().extreme_bg_color
                                },
                                Stroke::new(
                                    1.0,
                                    if on {
                                        theme::ACCENT
                                    } else {
                                        Color32::from_gray(160)
                                    },
                                ),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    ui.label(format!("{columns} × {rows}"));
                    let mut chosen = response.clicked();
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut rows)
                                .range(1..=200)
                                .prefix(format!("{} ", t("table.rows"))),
                        );
                        ui.add(
                            egui::DragValue::new(&mut columns)
                                .range(1..=40)
                                .prefix(format!("{} ", t("table.columns"))),
                        );
                    });
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.insert")).clicked() {
                            chosen = true;
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            ui.close();
                        }
                    });
                    chosen
                });
                if response.inner {
                    actions.push(Action::Doc(Request::Insert(Insert::Table {
                        rows,
                        columns,
                    })));
                    keep = false;
                } else if response.should_close() {
                    keep = false;
                } else {
                    next = Some(Dialog::Table { rows, columns });
                }
            }
            Dialog::Columns {
                mut count,
                mut space_cm,
                mut separator,
            } => {
                let mut apply = false;
                let response = modal("columns").show(ctx, |ui| {
                    ui.set_min_width(300.0);
                    crate::dialog::title(ui, &t("columns.title"));
                    crate::dialog::form("columns-grid").show(ui, |ui| {
                        ui.label(t("columns.count"));
                        ui.add(egui::DragValue::new(&mut count).range(1..=6));
                        ui.end_row();
                        ui.label(t("columns.space"));
                        ui.add(
                            egui::DragValue::new(&mut space_cm)
                                .range(0.0..=5.0)
                                .speed(0.05)
                                .fixed_decimals(2)
                                .suffix(format!(" {}", t("unit.cm"))),
                        );
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut separator, t("columns.separator"));
                        ui.end_row();
                    });
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.ok")).clicked() {
                            apply = true;
                            ui.close();
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            ui.close();
                        }
                    });
                });
                if apply {
                    #[allow(clippy::cast_possible_truncation)]
                    let space = (space_cm * 566.929).round() as i32;
                    actions.push(Action::Doc(Request::Page(PageChange::Columns {
                        count,
                        space,
                        separator,
                        widths: None,
                    })));
                }
                if response.should_close() || apply {
                    keep = false;
                } else {
                    next = Some(Dialog::Columns {
                        count,
                        space_cm,
                        separator,
                    });
                }
            }
            Dialog::Dictionary { code } => {
                let mut fetch = false;
                let name = crate::spell::language(code).map_or_else(String::new, |l| t(l.name_key));
                let size = crate::spell::language(code).map_or(0.0, |l| l.size_mb);
                let text = self.i18n.tf(
                    "spell.offer_text",
                    &[("language", &name), ("size", &format!("{size:.1}"))],
                );
                let response = modal("dictionary").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("spell.offer_title"));
                    ui.set_max_width(420.0);
                    ui.label(text);
                    ui.label(RichText::new(t("spell.offer_source")).weak().size(12.0));
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("spell.download")).clicked() {
                            fetch = true;
                            ui.close();
                        }
                        if crate::dialog::button(ui, &t("button.not_now")).clicked() {
                            ui.close();
                        }
                    });
                });
                if fetch {
                    actions.push(Action::DownloadDictionary(code));
                }
                if response.should_close() || fetch {
                    keep = false;
                } else {
                    next = Some(Dialog::Dictionary { code });
                }
            }
            Dialog::Bookmarks {
                mut name,
                mut selected,
            } => {
                let names = self
                    .active_tab()
                    .map(|tab| tab.state.bookmarks.clone())
                    .unwrap_or_default();
                let mut request = None;
                let response = modal("bookmarks").show(ctx, |ui| {
                    ui.set_min_width(340.0);
                    crate::dialog::title(ui, &t("bookmark.title"));
                    ui.label(t("bookmark.name"));
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .desired_width(f32::INFINITY)
                            .hint_text(t("bookmark.hint")),
                    );
                    if field.changed() {
                        selected = None;
                    }
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .stroke(Stroke::new(
                            1.0,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        ))
                        .corner_radius(4)
                        .inner_margin(4)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    if names.is_empty() {
                                        ui.label(RichText::new(t("bookmark.none")).weak());
                                    }
                                    for (index, existing) in names.iter().enumerate() {
                                        let chosen =
                                            ui.selectable_label(selected == Some(index), existing);
                                        if chosen.clicked() {
                                            selected = Some(index);
                                            name.clone_from(existing);
                                        }
                                        if chosen.double_clicked() {
                                            request = Some(ReferenceCommand::GoToBookmark(
                                                existing.clone(),
                                            ));
                                        }
                                    }
                                });
                        });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let known = names.iter().any(|n| n.eq_ignore_ascii_case(name.trim()));
                        if ui
                            .add_enabled(
                                !name.trim().is_empty(),
                                egui::Button::new(t("bookmark.add")),
                            )
                            .clicked()
                        {
                            request = Some(ReferenceCommand::AddBookmark(name.trim().to_owned()));
                        }
                        if ui
                            .add_enabled(known, egui::Button::new(t("bookmark.go")))
                            .clicked()
                        {
                            request = Some(ReferenceCommand::GoToBookmark(name.trim().to_owned()));
                        }
                        if ui
                            .add_enabled(known, egui::Button::new(t("bookmark.delete")))
                            .clicked()
                        {
                            request =
                                Some(ReferenceCommand::DeleteBookmark(name.trim().to_owned()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            crate::dialog::buttons(ui, |ui| {
                                if crate::dialog::button(ui, &t("button.close")).clicked() {
                                    ui.close();
                                }
                            });
                        });
                    });
                });
                let close = matches!(
                    request,
                    Some(ReferenceCommand::GoToBookmark(_) | ReferenceCommand::AddBookmark(_))
                );
                if let Some(request) = request {
                    actions.push(Action::Doc(Request::References(request)));
                }
                if response.should_close() || close {
                    keep = false;
                } else {
                    next = Some(Dialog::Bookmarks { name, selected });
                }
            }
            Dialog::Print(mut dialog) => {
                let mut go = false;
                let pages_total = self.active_tab().map_or(0, |tab| tab.pages.len());
                let laying_out = self
                    .active_tab()
                    .is_some_and(|tab| tab.busy == Some(Busy::Layout));
                let response = modal("print").show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    crate::dialog::title(ui, &t("print.title"));
                    crate::dialog::form("print-grid").show(ui, |ui| {
                        ui.label(t("print.printer"));
                        let selected = dialog
                            .printers
                            .get(dialog.printer)
                            .cloned()
                            .unwrap_or_default();
                        egui::ComboBox::from_id_salt("print-printer")
                            .width(240.0)
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (index, name) in dialog.printers.iter().enumerate() {
                                    ui.selectable_value(&mut dialog.printer, index, name);
                                }
                            });
                        ui.end_row();
                        ui.label(t("print.copies"));
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut dialog.copies).range(1..=999));
                            ui.checkbox(&mut dialog.collate, t("print.collate"));
                        });
                        ui.end_row();
                        crate::dialog::label_top(ui, &t("print.range"));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.radio_value(
                                &mut dialog.range,
                                PrintRange::All,
                                self.i18n.tf("print.all", &[("count", &pages_total)]),
                            );
                            ui.radio_value(
                                &mut dialog.range,
                                PrintRange::Current,
                                t("print.current"),
                            );
                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut dialog.range,
                                    PrintRange::Pages,
                                    t("print.pages"),
                                );
                                let field = ui.add_enabled(
                                    dialog.range == PrintRange::Pages,
                                    egui::TextEdit::singleline(&mut dialog.pages)
                                        .hint_text("1-3, 5")
                                        .desired_width(120.0),
                                );
                                if field.gained_focus() {
                                    dialog.range = PrintRange::Pages;
                                }
                            });
                        });
                        ui.end_row();
                    });
                    if laying_out {
                        ui.label(RichText::new(t("print.wait_layout")).weak());
                    }
                    if let Some(error) = &dialog.error {
                        ui.colored_label(Color32::from_rgb(200, 50, 50), error);
                    }
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary_enabled(ui, !laying_out, &t("print.print"))
                            .clicked()
                        {
                            go = true;
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            ui.close();
                        }
                    });
                });
                let mut done = false;
                if go {
                    let current = self.active_tab().map_or(0, |tab| tab.view.current_page);
                    let pages = match dialog.range {
                        PrintRange::All => Some((0..pages_total).collect()),
                        PrintRange::Current => Some(vec![current]),
                        PrintRange::Pages => crate::print::parse_range(&dialog.pages, pages_total),
                    };
                    match pages {
                        Some(pages) => {
                            let job = crate::print::Job {
                                printer: dialog
                                    .printers
                                    .get(dialog.printer)
                                    .cloned()
                                    .unwrap_or_default(),
                                copies: dialog.copies,
                                collate: dialog.collate,
                                pages,
                                title: self
                                    .active_tab()
                                    .map_or_else(String::new, |tab| tab.title.clone()),
                                output: None,
                            };
                            self.start_print(job, ctx);
                            done = true;
                        }
                        None => dialog.error = Some(t("print.bad_range")),
                    }
                }
                if response.should_close() || done {
                    keep = false;
                } else {
                    next = Some(Dialog::Print(dialog));
                }
            }
            Dialog::PageStart {
                mut start,
                mut restart,
            } => {
                let mut apply = false;
                let response = modal("page-start").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("page_number.format_title"));
                    ui.radio_value(&mut restart, false, t("page_number.continue"));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut restart, true, t("page_number.start_at"));
                        ui.add_enabled(restart, egui::DragValue::new(&mut start).range(0..=9999));
                    });
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.ok")).clicked() {
                            apply = true;
                            ui.close();
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            ui.close();
                        }
                    });
                });
                if apply {
                    actions.push(Action::Doc(Request::HeaderFooter(
                        HeaderFooterCommand::Start(restart.then_some(start)),
                    )));
                }
                if response.should_close() || apply {
                    keep = false;
                } else {
                    next = Some(Dialog::PageStart { start, restart });
                }
            }
            Dialog::Adjust(mut dialog) => {
                let mut apply = false;
                let response = modal("adjust").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("adjust.title"));
                    let stale = dialog.texture.as_ref().is_none_or(|(_, b, c)| {
                        (*b - dialog.brightness).abs() > f32::EPSILON
                            || (*c - dialog.contrast).abs() > f32::EPSILON
                    });
                    if stale {
                        if let Some(base) = &dialog.base {
                            let pixels = crate::raster::adjusted_pixmap(
                                base,
                                dialog.brightness / 100.0,
                                dialog.contrast / 100.0,
                            );
                            let image = egui::ColorImage::from_rgba_premultiplied(
                                [pixels.width() as usize, pixels.height() as usize],
                                pixels.data(),
                            );
                            let texture = ui.ctx().load_texture(
                                "adjust-preview",
                                image,
                                egui::TextureOptions::LINEAR,
                            );
                            dialog.texture = Some((texture, dialog.brightness, dialog.contrast));
                        }
                    }
                    match &dialog.texture {
                        Some((texture, _, _)) => {
                            ui.add(egui::Image::new(texture).max_size(vec2(320.0, 240.0)));
                        }
                        None => {
                            ui.add_sized(vec2(320.0, 120.0), egui::Spinner::new());
                        }
                    }
                    ui.add_space(6.0);
                    egui::Grid::new("adjust-grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label(t("cmd.brightness"));
                            ui.add(
                                egui::Slider::new(&mut dialog.brightness, -100.0..=100.0)
                                    .suffix(" %"),
                            );
                            ui.end_row();
                            ui.label(t("cmd.contrast"));
                            ui.add(
                                egui::Slider::new(&mut dialog.contrast, -100.0..=100.0)
                                    .suffix(" %"),
                            );
                            ui.end_row();
                        });
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.reset")).clicked() {
                            dialog.brightness = 0.0;
                            dialog.contrast = 0.0;
                        }
                        if crate::dialog::primary(ui, &t("button.ok")).clicked() {
                            apply = true;
                            ui.close();
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            ui.close();
                        }
                    });
                });
                if apply && (dialog.brightness != 0.0 || dialog.contrast != 0.0) {
                    actions.push(Action::Doc(Request::Object(ObjectCommand::Edit(
                        PictureEdit::Adjust {
                            brightness: dialog.brightness / 100.0,
                            contrast: dialog.contrast / 100.0,
                        },
                    ))));
                }
                if response.should_close() || apply {
                    keep = false;
                } else {
                    next = Some(Dialog::Adjust(dialog));
                }
            }
            Dialog::Symbols => {
                let response = modal("symbols").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("symbols.title"));
                    let mut picked = None;
                    egui::Grid::new("symbols-grid")
                        .spacing(vec2(2.0, 2.0))
                        .show(ui, |ui| {
                            for (index, symbol) in SYMBOLS.chars().enumerate() {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(symbol.to_string()).size(18.0),
                                        )
                                        .min_size(vec2(32.0, 32.0)),
                                    )
                                    .clicked()
                                {
                                    picked = Some(symbol);
                                }
                                if index % 12 == 11 {
                                    ui.end_row();
                                }
                            }
                        });
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::button(ui, &t("button.close")).clicked() {
                            ui.close();
                        }
                    });
                    picked
                });
                if let Some(symbol) = response.inner {
                    actions.push(Action::Doc(Request::Type(symbol.to_string())));
                }
                keep = !response.should_close();
                if keep {
                    next = Some(Dialog::Symbols);
                }
            }
            Dialog::WordCount => {
                let (pages, words, characters) = self.active_tab().map_or((0, 0, 0), |tab| {
                    (tab.pages.len(), tab.state.words, tab.state.characters)
                });
                let response = modal("words").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("words.title"));
                    crate::dialog::form("words-grid").show(ui, |ui| {
                        ui.label(t("info.pages"));
                        ui.label(pages.to_string());
                        ui.end_row();
                        ui.label(t("info.words"));
                        ui.label(words.to_string());
                        ui.end_row();
                        ui.label(t("info.characters"));
                        ui.label(characters.to_string());
                        ui.end_row();
                    });
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::button(ui, &t("button.close")).clicked() {
                            ui.close();
                        }
                    });
                });
                keep = !response.should_close();
                if keep {
                    next = Some(Dialog::WordCount);
                }
            }
            Dialog::Settings => {
                let mut language = None;
                let response = modal("settings").show(ctx, |ui| {
                    ui.set_width(420.0);
                    crate::dialog::title(ui, &t("settings.title"));
                    crate::dialog::form("settings-grid").show(ui, |ui| {
                        ui.label(t("settings.language"));
                        let current = self.i18n.language().to_owned();
                        egui::ComboBox::from_id_salt("settings-language")
                            .selected_text(
                                self.i18n
                                    .languages()
                                    .find(|(code, _)| *code == current)
                                    .map_or(current.clone(), |(_, name)| name.to_owned()),
                            )
                            .show_ui(ui, |ui| {
                                for (code, name) in self.i18n.languages() {
                                    if ui.selectable_label(code == current, name).clicked() {
                                        language = Some(code.to_owned());
                                    }
                                }
                            });
                        ui.end_row();
                        ui.label(t("settings.theme"));
                        ui.checkbox(&mut self.settings.dark, t("settings.dark"));
                        ui.end_row();
                        ui.label(t("settings.fonts"));
                        ui.checkbox(&mut self.settings.system_fonts, t("cmd.system_fonts"));
                        ui.end_row();
                        ui.label(t("settings.ocr_endpoint"));
                        ui.text_edit_singleline(&mut self.settings.ocr_endpoint);
                        ui.end_row();
                        ui.label(t("settings.ocr_model"));
                        ui.text_edit_singleline(&mut self.settings.ocr_model);
                        ui.end_row();
                        ui.label(t("settings.author"));
                        ui.text_edit_singleline(&mut self.settings.author);
                        ui.end_row();
                        ui.label(t("settings.ocr_language"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.ocr_language)
                                .hint_text("ru, en, de…"),
                        );
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(t("settings.locales_hint")).weak().size(11.5));
                    if let Some(dir) = settings::config_dir() {
                        ui.label(
                            RichText::new(dir.join("locales").display().to_string())
                                .monospace()
                                .size(11.0),
                        );
                    }
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.ok")).clicked() {
                            ui.close();
                        }
                    });
                });
                if let Some(code) = language {
                    actions.push(Action::Language(code));
                }
                keep = !response.should_close();
                if keep {
                    next = Some(Dialog::Settings);
                } else {
                    self.settings.save();
                    for tab in &self.tabs {
                        tab.send(Request::SystemFonts(self.settings.system_fonts));
                    }
                }
            }
            Dialog::About => {
                let response = modal("about").show(ctx, |ui| {
                    ui.set_width(440.0);
                    crate::dialog::title(ui, "MultiFormatEditor (MuFoEd)");
                    ui.label(format!(
                        "{} {}",
                        t("about.version"),
                        env!("CARGO_PKG_VERSION")
                    ));
                    ui.add_space(6.0);
                    ui.label(t("about.text"));
                    ui.add_space(6.0);
                    ui.label(RichText::new(t("about.features")).size(12.0));
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::button(ui, &t("button.close")).clicked() {
                            ui.close();
                        }
                    });
                });
                keep = !response.should_close();
                if keep {
                    next = Some(Dialog::About);
                }
            }
            Dialog::Import(mut import) => {
                let mut start = false;
                let response = modal("import").show(ctx, |ui| {
                    ui.set_width(460.0);
                    crate::dialog::title(ui, &t("import.title"));
                    ui.label(t(import.kind.key()));
                    ui.label(
                        RichText::new(import.path.display().to_string())
                            .monospace()
                            .size(11.5),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(t("import.mode"))
                            .family(FontFamily::Name(theme::BOLD.into())),
                    );
                    ui.radio_value(&mut import.visual, false, t("import.semantic"));
                    ui.radio_value(&mut import.visual, true, t("import.visual"));
                    ui.add_space(4.0);
                    ui.checkbox(&mut import.images, t("import.images"));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut import.all_pages, t("import.all_pages"));
                        ui.add_enabled_ui(!import.all_pages, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut import.first)
                                    .range(1..=100_000)
                                    .prefix(format!("{} ", t("import.from"))),
                            );
                            ui.add(
                                egui::DragValue::new(&mut import.last)
                                    .range(1..=100_000)
                                    .prefix(format!("{} ", t("import.to"))),
                            );
                        });
                    });
                    ui.add_space(8.0);
                    ui.add_enabled_ui(cfg!(feature = "ocr"), |ui| {
                        ui.checkbox(&mut import.ocr, t("import.ocr"));
                        ui.add_enabled_ui(import.ocr, |ui| {
                            ui.indent("ocr-options", |ui| {
                                ui.checkbox(&mut import.layout, t("import.ocr_layout"));
                                ui.label(
                                    RichText::new(format!(
                                        "{}: {} @ {}",
                                        t("import.ocr_model"),
                                        self.settings.ocr_model,
                                        self.settings.ocr_endpoint
                                    ))
                                    .weak()
                                    .size(11.5),
                                );
                            });
                        });
                    });
                    if !cfg!(feature = "ocr") {
                        ui.label(RichText::new(t("import.no_ocr_build")).weak());
                    }
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.convert")).clicked() {
                            start = true;
                            ui.close();
                        }
                        if crate::dialog::button(ui, &t("cmd.ocr_settings")).clicked() {
                            next = Some(Dialog::Settings);
                            ui.close();
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            ui.close();
                        }
                    });
                });
                if start {
                    self.start_import(ctx, &import);
                }
                keep = !response.should_close();
                if keep {
                    next = Some(Dialog::Import(import));
                }
            }
            Dialog::ConfirmClose { tab: id } => {
                let title = self
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .map(|tab| tab.title.clone())
                    .unwrap_or_default();
                let mut choice = None;
                let response = modal("confirm-close").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("confirm.title"));
                    ui.label(self.i18n.tf("confirm.save_changes", &[("file", &title)]));
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.save")).clicked() {
                            choice = Some(0);
                        }
                        if crate::dialog::button(ui, &t("button.dont_save")).clicked() {
                            choice = Some(1);
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            choice = Some(2);
                        }
                    });
                });
                let index = self.tabs.iter().position(|tab| tab.id == id);
                match (choice, index) {
                    (Some(0), Some(index)) => {
                        if let Some(tab) = self.tabs.get_mut(index) {
                            tab.after_save = Some(AfterSave::Close);
                        }
                        self.active = Some(index);
                        actions.push(Action::Save);
                        keep = false;
                    }
                    (Some(1), Some(index)) => {
                        self.close_tab(index, true);
                        keep = false;
                    }
                    (Some(_) | None, _) => {
                        keep = choice.is_none() && !response.should_close() && index.is_some();
                    }
                }
                if keep {
                    next = Some(Dialog::ConfirmClose { tab: id });
                }
            }
            Dialog::ConfirmExit => {
                let mut choice = None;
                let names: Vec<String> = self
                    .tabs
                    .iter()
                    .filter(|tab| tab.ready() && tab.modified())
                    .map(|tab| tab.title.clone())
                    .collect();
                let response = modal("confirm-exit").show(ctx, |ui| {
                    crate::dialog::title(ui, &t("confirm.exit_title"));
                    ui.label(t("confirm.exit_text"));
                    for name in &names {
                        ui.label(format!("• {name}"));
                    }
                    crate::dialog::buttons(ui, |ui| {
                        if crate::dialog::primary(ui, &t("button.exit_anyway")).clicked() {
                            choice = Some(true);
                        }
                        if crate::dialog::button(ui, &t("button.cancel")).clicked() {
                            choice = Some(false);
                        }
                    });
                });
                match choice {
                    Some(true) => {
                        self.exit_confirmed = true;
                        self.settings.save();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        keep = false;
                    }
                    Some(false) => keep = false,
                    None => keep = !response.should_close(),
                }
                if keep {
                    next = Some(Dialog::ConfirmExit);
                }
            }
            Dialog::Find { .. } => keep = false,
        }
        let _ = keep;
        if self.dialog.is_none() {
            self.dialog = next;
        }
    }
}

impl ImportDialog {
    fn new(path: PathBuf, kind: Kind, ocr: bool) -> Self {
        Self {
            path,
            kind,
            visual: false,
            images: true,
            all_pages: true,
            first: 1,
            last: 10,
            ocr: ocr && cfg!(feature = "ocr"),
            layout: false,
        }
    }
}

/// The keyboard and clipboard part of this frame's input, for the trace.
fn input_summary(ctx: &egui::Context) -> String {
    ctx.input(|input| {
        let mut out = Vec::new();
        for event in &input.events {
            match event {
                egui::Event::Text(text) => out.push(format!("text {text:?}")),
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                } => out.push(format!(
                    "key {key:?}/{physical_key:?} {} {}{}",
                    if *pressed { "down" } else { "up" },
                    if *repeat { "repeat " } else { "" },
                    modifier_names(*modifiers)
                )),
                egui::Event::Copy => out.push("copy".to_owned()),
                egui::Event::Cut => out.push("cut".to_owned()),
                egui::Event::Paste(text) => {
                    out.push(format!("paste {} chars", text.chars().count()));
                }
                egui::Event::Ime(ime) => out.push(format!("ime {ime:?}")),
                egui::Event::WindowFocused(focused) => out.push(format!("focused {focused}")),
                _ => {}
            }
        }
        out.join("; ")
    })
}

fn modifier_names(modifiers: egui::Modifiers) -> String {
    let mut out = String::new();
    for (on, name) in [
        (modifiers.ctrl, "ctrl+"),
        (modifiers.alt, "alt+"),
        (modifiers.shift, "shift+"),
        (modifiers.mac_cmd, "cmd+"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out
}

fn tile(ui: &mut egui::Ui, icon: Icon, label: &str, palette: Palette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(150.0, 120.0), Sense::click());
    let fill = if response.hovered() {
        palette.hover
    } else {
        palette.ribbon
    };
    ui.painter().rect(
        rect,
        8.0,
        fill,
        Stroke::new(1.0, palette.ribbon_line),
        egui::StrokeKind::Inside,
    );
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center() - vec2(0.0, 14.0), vec2(48.0, 48.0)),
        icon,
        ui.visuals().text_color(),
        theme::ACCENT,
    );
    let galley = ui.painter().layout(
        label.to_owned(),
        FontId::proportional(13.0),
        ui.visuals().text_color(),
        rect.width() - 12.0,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.bottom() - 32.0,
        ),
        galley,
        ui.visuals().text_color(),
    );
    response
}

/// A heading in the navigation pane, in the face, style and colour the
/// page draws it with, at a size the pane can hold.
fn heading_text(
    ctx: &egui::Context,
    fonts: &mut crate::docfonts::DocumentFonts,
    entry: &crate::worker::OutlineEntry,
    dark: bool,
) -> RichText {
    let fallback = if entry.level == 0 { 13.0 } else { 12.5 };
    let mut text = RichText::new(&entry.text).size(fallback);
    let Some(font) = &entry.font else {
        return text;
    };
    // Document sizes run from body text to titles; the pane keeps them
    // between its own text and a little larger.
    text = text.size((font.size_pt * 0.9).clamp(12.0, 17.0));
    match fonts.family(ctx, font) {
        Some(family) => text = text.family(family),
        // Until the face is in (or when there is none), the interface's own.
        None if font.bold => text = text.family(FontFamily::Name(theme::BOLD.into())),
        None if font.italic => text = text.family(FontFamily::Name(theme::ITALIC.into())),
        None => {}
    }
    // Page colours are chosen against white; on a dark pane they are left
    // to the theme.
    if let (Some([r, g, b]), false) = (font.color, dark) {
        text = text.color(Color32::from_rgb(r, g, b));
    }
    text
}

fn loading_ui(ui: &mut egui::Ui, tab: &DocTab, i18n: &I18n, palette: Palette) {
    egui::Frame::new().fill(palette.desk).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.35);
            ui.spinner();
            ui.label(RichText::new(i18n.tf("loading.title", &[("file", &tab.title)])).size(15.0));
            let progress = tab.control.progress();
            let stage = format!("{:?}", progress.stage);
            #[allow(clippy::cast_precision_loss)]
            let line = if tab.ready() {
                i18n.t("loading.layout").to_owned()
            } else if progress.total > 0 {
                format!(
                    "{stage}: {:.0}%",
                    progress.done as f64 * 100.0 / progress.total as f64
                )
            } else if progress.done > 0 {
                format!("{stage}: {}", progress.done)
            } else {
                i18n.t("loading.converting").to_owned()
            };
            ui.label(RichText::new(line).weak());
            ui.label(
                RichText::new(format!("{:.0} s", tab.started.elapsed().as_secs_f32()))
                    .weak()
                    .size(11.0),
            );
        });
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
}

fn failed_ui(ui: &mut egui::Ui, error: &str, i18n: &I18n, palette: Palette) {
    egui::Frame::new().fill(palette.desk).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(
                RichText::new(i18n.t("failed.title"))
                    .size(18.0)
                    .color(Color32::from_rgb(200, 50, 50)),
            );
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(error).monospace()).wrap());
        });
    });
}

/// Symbols the Insert ▸ Symbol dialog offers.
const SYMBOLS: &str = "©®™§¶°±×÷≈≠≤≥∞€£¥₽¢—–«»„“”‘’…•·→←↑↓↔✓✗αβγδεθλμπσφωΣΩΔ√∑∫∂½¼¾¹²³№†‡";

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), str::to_owned)
}

fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
    {
        path
    } else {
        let mut path = path.into_os_string();
        path.push(".");
        path.push(extension);
        PathBuf::from(path)
    }
}

/// The zoom steps Word's zoom buttons walk.
const ZOOM_STEPS: &[f32] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

fn next_zoom(zoom: f32, up: bool) -> f32 {
    if up {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|step| *step > zoom + 0.001)
            .unwrap_or(5.0)
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < zoom - 0.001)
            .unwrap_or(0.25)
    }
}

/// Today's date, written the way the interface language writes it.
fn today(language: &str) -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() / 86_400);
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    match language {
        "en" => format!("{year:04}-{month:02}-{day:02}"),
        _ => format!("{day:02}.{month:02}.{year:04}"),
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_742), (2026, 10, 16));
    }

    #[test]
    fn zoom_steps_walk_both_ways() {
        assert!((next_zoom(1.0, true) - 1.1).abs() < 1e-6);
        assert!((next_zoom(1.0, false) - 0.9).abs() < 1e-6);
        assert!((next_zoom(5.0, true) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn extensions_are_added_once() {
        assert_eq!(
            with_extension(PathBuf::from("a.docx"), "docx"),
            PathBuf::from("a.docx")
        );
        assert_eq!(
            with_extension(PathBuf::from("a"), "pdf"),
            PathBuf::from("a.pdf")
        );
    }
}
