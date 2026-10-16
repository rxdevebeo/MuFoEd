//! One document, one thread.
//!
//! The worker owns the opened `StrictDocument` and the editing session over
//! it, with its undo history, the visual map that turns page coordinates into
//! text positions, the caret and the selection. The interface sends requests
//! and draws what comes back: pages as SVG, caret and selection rectangles in
//! page pixels, the state of the ribbon's toggles. Layout, editing and saving
//! never run on the interface thread.
//!
//! The caret is kept as a paragraph address and a scalar offset into the
//! paragraph's text ([`Logical`]) rather than as a kernel `TextPosition`: an
//! edit that merges or splits runs moves the kernel position, the paragraph
//! offset stays where the user put it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use strict_ooxml::edit::{
    Address, Container, Edit, EditLimits, FormatPatch, OperationLimits, Operations, ReplacePolicy,
    SavePolicy, SearchQuery, SearchScope, Story, TextPosition, TextQuery, VisualMap, VisualRect,
};
use strict_ooxml::edit::{DrawingPosition, ListLevel, ListPreset};
use strict_ooxml::model::props::{
    CellProperties, PageSize, ParagraphProperties, RowProperties, SectionProperties,
    TableProperties,
};
use strict_ooxml::model::values::{
    Border, BorderStyle, Borders, Color, EighthsPoint, Fonts, HalfPoints, Highlight, Justification,
    PageOrientation, TriState, Twips, Underline, VertAlign,
};
use strict_ooxml::model::{
    Block, Document, GridCol, Paragraph, StyleId, Table, TableCell, TableRow,
};
use strict_ooxml::{
    DocumentEditor, OpenControl, OpenOptions, PageSelection, PlacedPage, Placement, RenderOptions,
    StrictDocument, WriteOptions,
};
use strict_ooxml_core::error::SourceLocation;

use crate::formats::{self, Kind, Source};
use crate::nav;

/// What the interface asks of a document.
#[derive(Debug)]
pub(crate) enum Request {
    /// Put the caret at a page point; `extend` keeps the selection anchor,
    /// `column` selects the table column under the point.
    Click {
        page: usize,
        x: f64,
        y: f64,
        extend: bool,
        column: bool,
    },
    /// Select the word at a page point.
    SelectWord {
        page: usize,
        x: f64,
        y: f64,
    },
    Move {
        motion: Motion,
        extend: bool,
    },
    SelectAll,
    Type(String),
    Backspace,
    Delete,
    Enter,
    /// Tab, or Shift+Tab with `back`.
    Tab {
        back: bool,
    },
    Table(TableCommand),
    Format(FormatChange),
    Align(Align),
    /// Work on drawings.
    Object(ObjectCommand),
    /// Put the selected paragraphs in a list of this format, or (`None`)
    /// take them out of their list.
    List(Option<ListPreset>),
    Style(String),
    Insert(Insert),
    Page(PageChange),
    Undo,
    Redo,
    Copy {
        cut: bool,
    },
    Paste(String),
    /// Paste text and put the copied drawings back where they were in it.
    PasteObjects(Box<ClipObjects>),
    Find {
        query: FindQuery,
        backwards: bool,
    },
    Replace {
        query: FindQuery,
        with: String,
    },
    ReplaceAll {
        query: FindQuery,
        with: String,
    },
    Revisions {
        accept: bool,
    },
    GoTo(Address),
    SystemFonts(bool),
    Save {
        path: PathBuf,
        format: SaveFormat,
    },
    Shutdown,
}

/// What can be done to a drawing.
#[derive(Debug)]
pub(crate) enum ObjectCommand {
    /// Insert a picture at the caret.
    InsertPicture { bytes: Vec<u8>, name: String },
    /// Resize the selected drawing, in EMU.
    Resize { width: i64, height: i64 },
    /// Give the selected picture its own pixel size back.
    ResetSize,
    /// Delete the selected drawing.
    Delete,
    /// Change the selected picture's pixels.
    Edit(PictureEdit),
    /// How text flows around the selected drawing.
    Wrap(WrapStyle),
    /// Move the selected floating drawing by page pixels.
    Move { dx: f64, dy: f64 },
    /// Move the selected drawing in the text to where a click at this page
    /// point would put the caret.
    MoveTo { page: usize, x: f64, y: f64 },
    /// Hand the selected picture's bytes over: to save, to edit elsewhere,
    /// or to preview an adjustment.
    Export { purpose: ExportPurpose },
    /// The picture with this drawing id gets new bytes (edited elsewhere);
    /// its width stays, its height follows the new proportions.
    Replace { id: u32, bytes: Vec<u8> },
    /// Recognise the selected picture: its text, or (`formula`) the
    /// formula it shows, put in its place, through the Ollama model at
    /// `endpoint`.
    Recognize {
        formula: bool,
        endpoint: String,
        model: String,
    },
}

/// What a picture's bytes are handed over for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportPurpose {
    Save,
    External,
    Preview,
}

/// How text flows around a drawing, as Word's Wrap Text menu offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WrapStyle {
    /// In line with text: a character of its line.
    Inline,
    /// Floating, text around its frame.
    Square,
    /// Floating, text close around its outline.
    Tight,
    /// Floating, text above and below only.
    TopAndBottom,
    /// Floating behind the text.
    Behind,
    /// Floating in front of the text.
    Front,
}

impl WrapStyle {
    pub(crate) const ALL: [Self; 6] = [
        Self::Inline,
        Self::Square,
        Self::Tight,
        Self::TopAndBottom,
        Self::Behind,
        Self::Front,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Inline => "wrap.inline",
            Self::Square => "wrap.square",
            Self::Tight => "wrap.tight",
            Self::TopAndBottom => "wrap.top_bottom",
            Self::Behind => "wrap.behind",
            Self::Front => "wrap.front",
        }
    }
}

/// What a recognised picture is replaced by.
#[cfg(feature = "ocr")]
enum Replacement {
    Text(String),
    Formula(strict_ooxml::model::MathExpression),
}

/// What the editor does to a picture's pixels; the picture gets new bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PictureEdit {
    /// A quarter turn.
    Rotate { clockwise: bool },
    /// A mirror image.
    Flip { vertical: bool },
    /// Brightness and contrast, each in `-1..=1`.
    Adjust { brightness: f32, contrast: f32 },
    /// The fractions taken off the left, top, right and bottom.
    Crop { sides: [f32; 4] },
}

/// Table commands of the Layout tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableCommand {
    SelectRow,
    SelectColumn,
    SelectTable,
    RowAbove,
    RowBelow,
    DeleteRows,
}

impl TableCommand {
    fn selects(self) -> bool {
        matches!(
            self,
            Self::SelectRow | Self::SelectColumn | Self::SelectTable
        )
    }
}

/// A table: its story, the containers around it, its block index there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TableRef {
    story: Story,
    parents: Vec<Container>,
    block: usize,
}

impl TableRef {
    fn address(&self) -> Address {
        Address {
            story: self.story.clone(),
            containers: self.parents.clone(),
            block: self.block,
        }
    }
}

/// A cell of a table.
#[derive(Clone, Debug)]
struct CellRef {
    table: TableRef,
    row: usize,
    cell: usize,
}

/// The innermost table cell a paragraph is in.
fn cell_of(paragraph: &Address) -> Option<CellRef> {
    let (last, parents) = paragraph.containers.split_last()?;
    let Container::Cell { table, row, cell } = last else {
        return None;
    };
    Some(CellRef {
        table: TableRef {
            story: paragraph.story.clone(),
            parents: parents.to_vec(),
            block: *table,
        },
        row: *row,
        cell: *cell,
    })
}

/// A rectangle of selected cells, inclusive.
#[derive(Clone, Debug)]
struct CellSelection {
    table: TableRef,
    rows: (usize, usize),
    cols: (usize, usize),
}

impl CellSelection {
    fn contains(&self, row: usize, cell: usize) -> bool {
        (self.rows.0..=self.rows.1).contains(&row) && (self.cols.0..=self.cols.1).contains(&cell)
    }

    fn corner(&self) -> CellRef {
        CellRef {
            table: self.table.clone(),
            row: self.rows.0,
            cell: self.cols.0,
        }
    }
}

/// Caret movements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    DocStart,
    DocEnd,
}

/// Character formatting commands.
#[derive(Clone, Debug)]
pub(crate) enum FormatChange {
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
    Size(f32),
    Grow,
    Shrink,
    Font(String),
    Color(Option<[u8; 3]>),
    Highlight(Option<Highlight>),
    Clear,
}

/// Paragraph alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
    Justify,
}

/// Things the Insert tab puts in.
#[derive(Clone, Debug)]
pub(crate) enum Insert {
    PageBreak,
    Table {
        rows: usize,
        columns: usize,
    },
    /// Another document's body, through the editor's import.
    Document(PathBuf),
}

/// Page setup of the caret's section.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PageChange {
    Orientation {
        landscape: bool,
    },
    /// Portrait width and height, in twips.
    Size {
        width: i32,
        height: i32,
    },
    /// Top, right, bottom, left, in twips.
    Margins([i32; 4]),
    SectionBreak,
}

/// A literal search.
#[derive(Clone, Debug, Default)]
pub(crate) struct FindQuery {
    pub(crate) text: String,
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
}

/// Output formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveFormat {
    Docx,
    Pdf,
    /// One SVG file per page; the path is the first page's name.
    Svg,
    Text,
}

/// One rendered page.
pub(crate) struct PageData {
    pub(crate) width: f64,
    pub(crate) height: f64,
    /// The laid-out page; it is painted only when it comes into view.
    pub(crate) placed: PlacedPage,
    /// New with every layout of the page. A bitmap made from an older
    /// generation is checked against the new one's SVG before it is reused.
    pub(crate) generation: u64,
}

/// A generation no page has had yet.
fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Pages laid out first, so a long document shows something at once.
const PREVIEW_PAGES: usize = 3;

/// What the worker is busy with, for the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    /// Laying the pages out.
    Layout,
    /// Building the caret map; clicks and typing wait for it.
    Map,
    /// A vision model is reading a picture.
    #[cfg_attr(not(feature = "ocr"), allow(dead_code))]
    Recognize,
}

/// A heading for the navigation pane.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutlineEntry {
    pub(crate) level: u8,
    pub(crate) text: String,
    pub(crate) paragraph: Address,
    /// How the page draws the heading's text.
    pub(crate) font: Option<HeadingFont>,
}

/// The face, size and colour of a heading's first run, through its styles.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeadingFont {
    /// The family the page paints with (a bundled substitute or the
    /// document's own font).
    pub(crate) family: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) size_pt: f32,
    pub(crate) color: Option<[u8; 3]>,
}

/// How the page draws `paragraph`'s text, through its styles: its first run
/// with text, else its paragraph mark.
fn heading_font(
    document: &Document,
    programs: &Arc<strict_ooxml_render_svg::font::embedded::FontPrograms>,
    paragraph: &Paragraph,
    text: &str,
) -> HeadingFont {
    use strict_ooxml::model::{Inline, RunContent};
    use strict_ooxml_render_svg::style;
    let computed = style::compute_paragraph(document, paragraph);
    let run = paragraph.inlines.iter().find_map(|inline| match inline {
        Inline::Run(run) if run.content.iter().any(|c| matches!(c, RunContent::Text(_))) => {
            Some(run)
        }
        _ => None,
    });
    let mut run = run.map_or_else(
        || computed.mark_run().clone(),
        |run| style::compute_run(document, &computed, run),
    );
    run.font_programs = Arc::clone(programs);
    let color = run.color.as_deref().and_then(|hex| {
        let hex = hex.strip_prefix('#')?;
        let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
        Some([channel(0)?, channel(2)?, channel(4)?])
    });
    #[allow(clippy::cast_possible_truncation)]
    HeadingFont {
        family: style::installed_paint_family(&run, text),
        bold: run.bold,
        italic: run.italic,
        size_pt: run.size_pt as f32,
        color,
    }
}

/// What the ribbon shows for the text at the caret.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CaretFormat {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) superscript: bool,
    pub(crate) subscript: bool,
    /// Points, when set directly.
    pub(crate) size: Option<f32>,
    pub(crate) font: Option<String>,
    pub(crate) align: Option<Align>,
    /// Paragraph style id.
    pub(crate) style: Option<String>,
    /// The list the caret's paragraph is in.
    pub(crate) list: Option<CaretList>,
}

/// The list at the caret, as the ribbon shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CaretList {
    pub(crate) bullet: bool,
    /// The preset the list looks like, if it is one.
    pub(crate) preset: Option<ListPreset>,
    /// The level's marker as its first item shows it: `1.`, `a)`, `•`.
    pub(crate) sample: String,
    /// Zero-based level.
    pub(crate) level: u8,
}

impl CaretList {
    fn of(level: &ListLevel) -> Self {
        let mut sample = level.text.clone();
        for index in 1..=9 {
            let digit = match level.format.as_str() {
                "lowerLetter" => "a",
                "upperLetter" => "A",
                "lowerRoman" => "i",
                "upperRoman" => "I",
                _ => "1",
            };
            sample = sample.replace(&format!("%{index}"), digit);
        }
        Self {
            bullet: level.is_bullet(),
            preset: ListPreset::matching(level),
            sample,
            level: level.ilvl,
        }
    }
}

/// Caret and selection geometry, in page pixels.
pub(crate) struct Cursor {
    /// The selected drawing, when one is.
    pub(crate) object: Option<ObjectView>,
    pub(crate) carets: Vec<VisualRect>,
    pub(crate) selection: Vec<VisualRect>,
    /// Bring the caret into view.
    pub(crate) scroll: bool,
    pub(crate) format: CaretFormat,
}

/// What a copy carried besides its text: its drawings and, for a lone
/// picture, the picture itself for other programs.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClipObjects {
    /// The copied text, drawings left out.
    pub(crate) text: String,
    pub(crate) objects: Vec<ClipObject>,
    /// A lone picture's pixels: width, height, premultiplied RGBA.
    pub(crate) image: Option<(usize, usize, Vec<u8>)>,
}

/// One copied drawing and where it was in the copied text.
#[derive(Clone, Debug)]
pub(crate) struct ClipObject {
    /// The line of the copied text.
    pub(crate) line: usize,
    /// The character offset in that line.
    pub(crate) offset: usize,
    pub(crate) drawing: strict_ooxml::model::Drawing,
    /// The picture's bytes, so it can go into another document.
    pub(crate) bytes: Option<Vec<u8>>,
}

/// A selected drawing, as the canvas and the ribbon show it.
#[derive(Clone, Debug)]
pub(crate) struct ObjectView {
    /// Where it is drawn (more than one rect for a header's repeats).
    pub(crate) rects: Vec<VisualRect>,
    /// Its size, in EMU.
    pub(crate) width: i64,
    pub(crate) height: i64,
    /// A picture (not a shape or a chart).
    pub(crate) picture: bool,
    /// How text flows around it.
    pub(crate) wrap: WrapStyle,
}

/// Counters for the status bar and the quick access toolbar.
#[derive(Clone, Debug, Default)]
pub(crate) struct EditState {
    pub(crate) can_undo: bool,
    pub(crate) can_redo: bool,
    pub(crate) modified: bool,
    pub(crate) words: usize,
    pub(crate) characters: usize,
    /// Why the document is read-only, when it is.
    pub(crate) read_only: Option<String>,
}

/// A paragraph style the gallery offers.
#[derive(Clone, Debug)]
pub(crate) struct StyleEntry {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// What the document is, once it is open.
pub(crate) struct DocInfo {
    pub(crate) kind: Kind,
    pub(crate) report: String,
    pub(crate) log: String,
    pub(crate) styles: Vec<StyleEntry>,
    /// What the document breaks of the editing model's invariants; edits
    /// keep these but may not add to them.
    pub(crate) defects: Vec<String>,
}

/// A note for the status bar: a localization key and a detail.
pub(crate) struct Note {
    pub(crate) key: &'static str,
    pub(crate) detail: String,
    pub(crate) error: bool,
}

/// What a document tells the interface.
pub(crate) enum Reply {
    Loaded(Box<DocInfo>),
    Failed(String),
    Pages {
        pages: Vec<Arc<PageData>>,
        warnings: Vec<String>,
    },
    Cursor(Box<Cursor>),
    State(EditState),
    Outline(Vec<OutlineEntry>),
    Busy(Option<Busy>),
    Note(Note),
    Clipboard(String),
    /// The drawings of what was copied, after its text.
    ClipboardObjects(Box<ClipObjects>),
    /// A picture's bytes, asked for by `ObjectCommand::Export`.
    Picture {
        id: u32,
        bytes: Vec<u8>,
        extension: &'static str,
        purpose: ExportPurpose,
    },
    Saved {
        path: PathBuf,
        format: SaveFormat,
    },
}

/// Starts a worker; replies arrive on the returned receiver.
pub(crate) fn spawn(
    source: Source,
    system_fonts: bool,
    control: OpenControl,
    ctx: eframe::egui::Context,
) -> (Sender<Request>, Receiver<Reply>) {
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    // Layout is budgeted for a deep stack; a spawned thread's default is small.
    let spawned = std::thread::Builder::new()
        .name("mufoed-document".to_owned())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            run(
                &source,
                system_fonts,
                &control,
                &request_rx,
                &reply_tx,
                &ctx,
            );
        });
    // A thread that could not start drops its sender: the interface sees a
    // closed channel and reports the tab as failed.
    drop(spawned);
    (request_tx, reply_rx)
}

fn run(
    source: &Source,
    system_fonts: bool,
    control: &OpenControl,
    requests: &Receiver<Request>,
    replies: &Sender<Reply>,
    ctx: &eframe::egui::Context,
) {
    let send = |reply: Reply| {
        let _ = replies.send(reply);
        ctx.request_repaint();
    };
    let loaded = match formats::open(source, control) {
        Ok(loaded) => loaded,
        Err(error) => {
            send(Reply::Failed(error));
            return;
        }
    };
    let mut document = loaded.document;
    let options = RenderOptions {
        font_assets: installed_fonts(document.document(), system_fonts),
        ..RenderOptions::default()
    };
    let path = match source {
        Source::File { path, .. } if !loaded.kind.converted() && loaded.kind != Kind::Rtf => {
            Some(path.clone())
        }
        _ => None,
    };
    let info = DocInfo {
        kind: loaded.kind,
        report: document.report_text(),
        log: loaded.log,
        styles: styles_of(document.document()),
        defects: Vec::new(),
    };
    let read_only = match document.edit(EditLimits::default()) {
        Ok(editor) => {
            let defects = editor
                .source_defects()
                .iter()
                .map(ToString::to_string)
                .collect();
            send(Reply::Loaded(Box::new(DocInfo { defects, ..info })));
            let mut worker = Worker::new(Session::Edit(editor), options, replies, ctx, path);
            worker.serve(requests);
            return;
        }
        Err(error) => {
            send(Reply::Loaded(Box::new(info)));
            format!("{error:?}")
        }
    };
    let mut worker = Worker::new(Session::View(&document), options, replies, ctx, path);
    worker.read_only = Some(read_only);
    worker.serve(requests);
}

/// The installed fonts a document is laid out with: always those that draw
/// the scripts no bundled face has (without them that text is measured with
/// a face that lacks it, and the rasterizer searches every installed font
/// for each glyph), and with `named` the families the document names.
fn installed_fonts(document: &Document, named: bool) -> Option<Arc<strict_ooxml::FontAssets>> {
    use strict_ooxml::system_fonts as system;
    let limits = strict_ooxml::FontAssetLimits::default();
    let faces = system::index();
    let fallbacks = system::script_fallbacks(document, faces, limits);
    let mut assets = if named {
        system::assets_for(&system::document_families(document), faces, limits).unwrap_or_default()
    } else {
        Vec::new()
    };
    let key = |asset: &strict_ooxml::FontAsset| {
        let (family, bold, italic) = asset.key();
        (family.to_lowercase(), bold, italic)
    };
    for fallback in fallbacks.iter().cloned() {
        if !assets.iter().any(|asset| key(asset) == key(&fallback)) {
            assets.push(fallback);
        }
    }
    if assets.is_empty() {
        return None;
    }
    strict_ooxml::FontAssets::new(assets, limits)
        .or_else(|_| strict_ooxml::FontAssets::new(fallbacks, limits))
        .ok()
        .map(Arc::new)
}

fn styles_of(document: &Document) -> Vec<StyleEntry> {
    use strict_ooxml::model::values::StyleType;
    let mut quick = Vec::new();
    let mut other = Vec::new();
    for style in document.styles.iter() {
        if style.style_type != StyleType::Paragraph || style.hidden || style.semi_hidden {
            continue;
        }
        let entry = StyleEntry {
            id: style.id.to_string(),
            name: style
                .name
                .as_deref()
                .map_or_else(|| style.id.to_string(), str::to_owned),
        };
        if style.q_format || style.is_default {
            quick.push(entry);
        } else {
            other.push(entry);
        }
    }
    quick.extend(other);
    quick
}

/// The editing session, or the document alone when it cannot be edited.
///
/// One session per worker thread, never moved: the size difference between
/// the variants costs nothing.
#[allow(clippy::large_enum_variant)]
enum Session<'a> {
    Edit(DocumentEditor<'a>),
    View(&'a StrictDocument),
}

impl Session<'_> {
    fn document(&self) -> &Document {
        match self {
            Self::Edit(editor) => editor.document(),
            Self::View(document) => document.document(),
        }
    }

    fn revision(&self) -> u64 {
        match self {
            Self::Edit(editor) => editor.revision(),
            Self::View(_) => 0,
        }
    }

    fn place(&self, options: &RenderOptions) -> Result<Placement, String> {
        match self {
            Self::Edit(editor) => editor.place(options),
            Self::View(document) => document.place(options),
        }
        .map_err(|error| error.to_string())
    }

    /// A Strict package of the current model.
    fn package_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Edit(editor) => editor
                .save(
                    editor.revision(),
                    &WriteOptions::default(),
                    SavePolicy::AllowDegraded,
                )
                .map(|saved| saved.bytes)
                .map_err(|error| error.to_string()),
            Self::View(document) => strict_ooxml::write_package(
                document.document(),
                Some(document.package()),
                &WriteOptions::default(),
            )
            .map(|written| written.bytes)
            .map_err(|error| error.to_string()),
        }
    }
}

/// A caret: a paragraph and a scalar offset into its text.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Logical {
    paragraph: Address,
    offset: usize,
}

/// What a request did.
enum Outcome {
    /// Nothing visible changed.
    Quiet,
    /// The caret or the selection moved.
    Moved,
    /// The model changed.
    Edited,
    Quit,
}

struct Worker<'a, 's> {
    session: Session<'a>,
    options: RenderOptions,
    replies: &'s Sender<Reply>,
    ctx: &'s eframe::egui::Context,
    map: Option<VisualMap>,
    pages: Vec<Arc<PageData>>,
    page_sizes: Vec<(f64, f64)>,
    caret: Option<Logical>,
    anchor: Option<Logical>,
    /// Whole cells selected, instead of text.
    cells: Option<CellSelection>,
    /// The cell a click started in, for selecting cells by dragging.
    cell_origin: Option<CellRef>,
    goal_x: Option<f64>,
    /// The selected drawing.
    object: Option<DrawingPosition>,
    undo_depth: usize,
    redo_depth: usize,
    saved_revision: u64,
    /// The layout no longer matches the model.
    stale: bool,
    path: Option<PathBuf>,
    outline: Vec<OutlineEntry>,
    read_only: Option<String>,
}

impl<'a, 's> Worker<'a, 's> {
    fn new(
        session: Session<'a>,
        options: RenderOptions,
        replies: &'s Sender<Reply>,
        ctx: &'s eframe::egui::Context,
        path: Option<PathBuf>,
    ) -> Self {
        let saved_revision = session.revision();
        Self {
            session,
            options,
            replies,
            ctx,
            map: None,
            pages: Vec::new(),
            page_sizes: Vec::new(),
            caret: None,
            anchor: None,
            cells: None,
            cell_origin: None,
            goal_x: None,
            object: None,
            undo_depth: 0,
            redo_depth: 0,
            saved_revision,
            stale: true,
            path,
            outline: Vec::new(),
            read_only: None,
        }
    }

    fn send(&self, reply: Reply) {
        let _ = self.replies.send(reply);
        self.ctx.request_repaint();
    }

    fn note(&self, key: &'static str, detail: impl Into<String>, error: bool) {
        self.send(Reply::Note(Note {
            key,
            detail: detail.into(),
            error,
        }));
    }

    fn serve(&mut self, requests: &Receiver<Request>) {
        self.preview();
        self.relayout();
        self.caret = self.first_paragraph().map(|paragraph| Logical {
            paragraph,
            offset: 0,
        });
        self.send_cursor(false);
        self.send_state();
        while let Ok(first) = requests.recv() {
            // Everything queued behind the first request is handled before the
            // next layout: typing faster than layout costs one layout, not one
            // per key.
            let mut batch = vec![first];
            while let Ok(next) = requests.try_recv() {
                batch.push(next);
            }
            let mut moved = false;
            let mut edited = false;
            for request in batch {
                let started = std::time::Instant::now();
                let described = format!("{request:?}");
                let described: String = described.chars().take(160).collect();
                let name = crate::trace::enabled().then(|| described.clone());
                let guard = crate::trace::busy(|| described);
                let outcome = self.handle(request);
                drop(guard);
                if let Some(name) = name {
                    crate::trace::write(&format!(
                        "request {} took {} ms",
                        name.chars().take(120).collect::<String>(),
                        started.elapsed().as_millis()
                    ));
                }
                match outcome {
                    Outcome::Quiet => {}
                    Outcome::Moved => moved = true,
                    Outcome::Edited => {
                        edited = true;
                        moved = true;
                    }
                    Outcome::Quit => return,
                }
            }
            if edited || self.stale {
                self.relayout();
            }
            if moved {
                self.send_cursor(true);
            }
            if edited {
                self.send_state();
            }
        }
    }

    fn handle(&mut self, request: Request) -> Outcome {
        // A drawing stays selected for what acts on it; anything else (a
        // click selects anew) leaves it.
        let keeps_object = matches!(
            request,
            Request::Object(_)
                | Request::Backspace
                | Request::Delete
                | Request::Type(_)
                | Request::Copy { .. }
                | Request::Click { .. }
        );
        if !keeps_object {
            self.object = None;
        }
        if self.object.is_some() {
            match &request {
                Request::Backspace | Request::Delete => return self.delete_object(),
                // Typing over a selected picture replaces it.
                Request::Type(_) => {
                    if matches!(self.delete_object(), Outcome::Quiet) {
                        return Outcome::Quiet;
                    }
                }
                _ => {}
            }
        }
        match request {
            Request::Shutdown => Outcome::Quit,
            Request::Click {
                page,
                x,
                y,
                extend,
                column,
            } => self.click(page, x, y, extend, column),
            Request::Table(command) if command.selects() => self.table_command(command),
            Request::SelectWord { page, x, y } => self.select_word(page, x, y),
            Request::Move { motion, extend } => self.motion(motion, extend),
            Request::SelectAll => self.select_all(),
            Request::GoTo(paragraph) => {
                self.anchor = None;
                self.cells = None;
                self.caret = Some(Logical {
                    paragraph,
                    offset: 0,
                });
                Outcome::Moved
            }
            Request::Copy { cut } => self.copy(cut),
            Request::Save { path, format } => {
                self.save(path, format);
                Outcome::Quiet
            }
            Request::SystemFonts(enabled) => {
                self.options.font_assets = installed_fonts(self.session.document(), enabled);
                self.stale = true;
                Outcome::Moved
            }
            Request::Find { query, backwards } => self.find(&query, backwards),
            request => {
                if let Some(reason) = &self.read_only {
                    self.note("note.read_only", reason.clone(), true);
                    return Outcome::Quiet;
                }
                self.edit_request(request)
            }
        }
    }

    fn edit_request(&mut self, request: Request) -> Outcome {
        match request {
            Request::Type(text) => self.type_text(&text),
            Request::Backspace => self.backspace(),
            Request::Delete => self.delete_forward(),
            Request::Enter => self.enter(),
            Request::Tab { back } => self.tab(back),
            Request::Table(command) => self.table_command(command),
            Request::Format(change) => self.format(&change),
            Request::Align(align) => self.align(align),
            Request::List(preset) => self.list(preset),
            Request::Object(command) => self.object_command(command),
            Request::Style(id) => self.set_style(&id),
            Request::Insert(insert) => self.insert(insert),
            Request::Page(change) => self.page_setup(change),
            Request::Undo => self.undo(true),
            Request::Redo => self.undo(false),
            Request::Paste(text) => self.paste(&text),
            Request::PasteObjects(clip) => self.paste_objects(&clip),
            Request::Replace { query, with } => self.replace(&query, &with),
            Request::ReplaceAll { query, with } => self.replace_all(&query, &with),
            Request::Revisions { accept } => self.revisions(accept),
            _ => Outcome::Quiet,
        }
    }

    // ----- layout -------------------------------------------------------

    fn relayout(&mut self) {
        let started = std::time::Instant::now();
        let guard = crate::trace::busy(|| "layout".to_owned());
        self.relayout_inner();
        drop(guard);
        crate::trace::log(|| format!("layout took {} ms", started.elapsed().as_millis()));
    }

    /// Lays out the first pages alone and shows them, before the whole
    /// document is laid out.
    fn preview(&mut self) {
        let options = self.options.clone().pages(PageSelection::Range {
            start: 1,
            end: PREVIEW_PAGES,
        });
        self.send(Reply::Busy(Some(Busy::Layout)));
        let started = std::time::Instant::now();
        if let Ok(placement) = self.session.place(&options) {
            self.show(placement);
        }
        crate::trace::log(|| format!("preview took {} ms", started.elapsed().as_millis()));
    }

    /// Sends a placement's pages to the interface.
    fn show(&mut self, placement: Placement) {
        self.pages = placement
            .pages
            .into_iter()
            .map(|page| {
                Arc::new(PageData {
                    width: page.width_px,
                    height: page.height_px,
                    placed: page,
                    generation: next_generation(),
                })
            })
            .collect();
        self.page_sizes = self
            .pages
            .iter()
            .map(|page| (page.width, page.height))
            .collect();
        self.send(Reply::Pages {
            pages: self.pages.clone(),
            warnings: placement.warnings,
        });
    }

    fn relayout_inner(&mut self) {
        self.stale = false;
        // One layout serves the pages and the caret map: it places caret
        // anchors, which painting leaves out.
        let editing = matches!(self.session, Session::Edit(_));
        let options = self.options.clone().caret_anchors(editing);
        self.send(Reply::Busy(Some(Busy::Layout)));
        let placed = match self.session.place(&options) {
            Ok(placement) => {
                self.show(placement);
                true
            }
            Err(error) => {
                self.note("note.render_failed", error, true);
                false
            }
        };
        if editing {
            self.send(Reply::Busy(Some(Busy::Map)));
        }
        let pages_at = std::time::Instant::now();
        let original: Vec<&PlacedPage> = self.pages.iter().map(|page| &page.placed).collect();
        self.map = match &self.session {
            Session::Edit(_) if !placed => None,
            Session::Edit(editor) => {
                match editor.visual_map_placed(editor.revision(), &self.options, &original) {
                    Ok(map) => Some(map),
                    Err(error) => {
                        if self.read_only.is_none() {
                            self.note("note.no_caret_map", error.to_string(), true);
                        }
                        None
                    }
                }
            }
            Session::View(_) => None,
        };
        crate::trace::log(|| format!("caret map took {} ms", pages_at.elapsed().as_millis()));
        self.send(Reply::Busy(None));
        let outline = self.build_outline();
        if outline != self.outline {
            self.outline.clone_from(&outline);
            self.send(Reply::Outline(outline));
        }
    }

    fn ensure_layout(&mut self) {
        if self.stale {
            self.relayout();
        }
    }

    fn build_outline(&self) -> Vec<OutlineEntry> {
        let document = self.session.document();
        let programs = strict_ooxml_render_svg::font_programs(document, &self.options, None);
        let mut out = Vec::new();
        for (index, block) in document.body.blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else {
                continue;
            };
            let Some(level) = heading_level(document, paragraph) else {
                continue;
            };
            let text: String = nav::pieces(paragraph)
                .iter()
                .map(|node| node.text.as_str())
                .collect();
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            out.push(OutlineEntry {
                level,
                font: Some(heading_font(document, &programs, paragraph, text)),
                text: text.chars().take(80).collect(),
                paragraph: Address::body(index),
            });
        }
        out
    }

    // ----- model access -------------------------------------------------

    fn paragraph(&self, at: &Address) -> Option<&Paragraph> {
        match &self.session {
            Session::Edit(editor) => editor.paragraph(at).ok(),
            Session::View(document) => {
                if at.story == Story::Body && at.containers.is_empty() {
                    document
                        .document()
                        .body
                        .blocks
                        .get(at.block)?
                        .as_paragraph()
                } else {
                    None
                }
            }
        }
    }

    fn paragraphs(&self, story: &Story) -> Vec<Address> {
        match &self.session {
            Session::Edit(editor) => editor.paragraphs(story).unwrap_or_default(),
            Session::View(document) => document
                .document()
                .body
                .blocks
                .iter()
                .enumerate()
                .filter(|(_, block)| matches!(block, Block::Paragraph(_)))
                .map(|(index, _)| Address::body(index))
                .collect(),
        }
    }

    fn first_paragraph(&self) -> Option<Address> {
        self.paragraphs(&Story::Body).into_iter().next()
    }

    fn pieces(&self, at: &Address) -> Vec<nav::Piece> {
        self.paragraph(at).map(nav::pieces).unwrap_or_default()
    }

    /// The paragraph's text, a tab or a break counting as one character.
    fn text_of(&self, at: &Address) -> String {
        self.pieces(at)
            .iter()
            .map(|piece| piece.text.as_str())
            .collect()
    }

    /// The kernel point of a caret, where the kernel has one.
    fn point(&self, logical: &Logical) -> Option<strict_ooxml::edit::InlinePoint> {
        nav::point_at(&self.pieces(&logical.paragraph), logical.offset)
    }

    /// The text position the visual map knows a caret by: in the text node
    /// at the offset, or the nearest one before it (after a tab that ends a
    /// paragraph there is no text to stand in).
    fn position(&self, logical: &Logical) -> TextPosition {
        let pieces = self.pieces(&logical.paragraph);
        let mut before = 0;
        let mut fallback = None;
        for piece in &pieces {
            let end = before + piece.len();
            if piece.kind == nav::PieceKind::Text {
                if (before..=end).contains(&logical.offset) {
                    return TextPosition {
                        paragraph: logical.paragraph.clone(),
                        inline: piece.inline.clone(),
                        content: piece.content,
                        offset: logical.offset - before,
                    };
                }
                if before <= logical.offset || fallback.is_none() {
                    let offset = if before <= logical.offset {
                        piece.len()
                    } else {
                        0
                    };
                    fallback = Some(TextPosition {
                        paragraph: logical.paragraph.clone(),
                        inline: piece.inline.clone(),
                        content: piece.content,
                        offset,
                    });
                }
            }
            before = end;
        }
        // Beside a tab with no text there, the map knows the position by the
        // tab itself: offset 0 before it, 1 after it.
        let mut before = 0;
        for piece in &pieces {
            let end = before + piece.len();
            if piece.text == "\t" && (end == logical.offset || before == logical.offset) {
                return TextPosition {
                    paragraph: logical.paragraph.clone(),
                    inline: piece.inline.clone(),
                    content: piece.content,
                    offset: usize::from(end == logical.offset),
                };
            }
            before = end;
        }
        fallback.unwrap_or(TextPosition {
            paragraph: logical.paragraph.clone(),
            inline: Vec::new(),
            content: 0,
            offset: 0,
        })
    }

    /// The caret a kernel position stands for.
    fn logical(&self, position: &TextPosition) -> Logical {
        let mut before = 0;
        for piece in self.pieces(&position.paragraph) {
            if piece.inline == position.inline && piece.content == position.content {
                return Logical {
                    paragraph: position.paragraph.clone(),
                    offset: before + position.offset,
                };
            }
            before += piece.len();
        }
        Logical {
            paragraph: position.paragraph.clone(),
            offset: 0,
        }
    }

    /// `(paragraph index in story order, offset)`, for ordering carets.
    fn key(&self, logical: &Logical) -> (usize, usize) {
        let index = self
            .paragraphs(&logical.paragraph.story)
            .iter()
            .position(|address| *address == logical.paragraph)
            .unwrap_or(0);
        (index, logical.offset)
    }

    /// The selection, start first, when there is one.
    fn selection(&self) -> Option<(Logical, Logical)> {
        let anchor = self.anchor.as_ref()?;
        let caret = self.caret.as_ref()?;
        if anchor == caret || anchor.paragraph.story != caret.paragraph.story {
            return None;
        }
        if self.key(anchor) <= self.key(caret) {
            Some((anchor.clone(), caret.clone()))
        } else {
            Some((caret.clone(), anchor.clone()))
        }
    }

    // ----- cursor -------------------------------------------------------

    fn caret_rect(&self) -> Option<VisualRect> {
        let caret = self.caret.as_ref()?;
        let map = self.map.as_ref()?;
        map.caret(self.session.revision(), &self.position(caret))
            .ok()?
            .into_iter()
            .next()
    }

    fn send_cursor(&mut self, scroll: bool) {
        self.ensure_layout();
        let revision = self.session.revision();
        let (carets, selection) = match (&self.map, &self.caret) {
            (Some(map), Some(caret)) => {
                let carets = map
                    .caret(revision, &self.position(caret))
                    .unwrap_or_default();
                let selection = self
                    .anchor
                    .as_ref()
                    .filter(|anchor| *anchor != caret)
                    .map_or_else(Vec::new, |anchor| {
                        map.selection_between(
                            revision,
                            &self.position(anchor),
                            &self.position(caret),
                        )
                        .unwrap_or_default()
                    });
                (carets, selection)
            }
            _ => (Vec::new(), Vec::new()),
        };
        // Selected cells show as cells, without a caret.
        let (carets, selection) = if self.cells.is_some() {
            (Vec::new(), self.cell_rects())
        } else {
            (carets, selection)
        };
        // Right after a drawing that no text follows, the caret stands on
        // the drawing's right edge; the map would put it at the text before.
        let beside = self.caret_beside_object();
        let carets = if carets.is_empty() || (!beside.is_empty() && !self.text_at_caret()) {
            beside
        } else {
            carets
        };
        // Drawings inside the selection are selected too.
        let mut selection = selection;
        selection.extend(self.selected_object_rects());
        let object = self.object_view();
        // A selected drawing shows its frame, not a caret.
        let carets = if object.is_some() { Vec::new() } else { carets };
        let format = self.caret_format();
        self.send(Reply::Cursor(Box::new(Cursor {
            object,
            carets,
            selection,
            scroll,
            format,
        })));
    }

    fn caret_format(&self) -> CaretFormat {
        let Some(caret) = &self.caret else {
            return CaretFormat::default();
        };
        let Some(paragraph) = self.paragraph(&caret.paragraph) else {
            return CaretFormat::default();
        };
        let position = self.position(caret);
        let props = nav::run_at(paragraph, &position.inline)
            .map(|run| &run.props)
            .or(paragraph.props.run_props.as_ref());
        let mut format = CaretFormat {
            align: paragraph
                .props
                .alignment
                .and_then(|alignment| match alignment {
                    Justification::Start => Some(Align::Left),
                    Justification::Center => Some(Align::Center),
                    Justification::End => Some(Align::Right),
                    Justification::Both | Justification::Justify | Justification::Distribute => {
                        Some(Align::Justify)
                    }
                    _ => None,
                }),
            style: paragraph.props.style.as_ref().map(ToString::to_string),
            list: strict_ooxml::edit::list_level(self.session.document(), paragraph)
                .map(|level| CaretList::of(&level)),
            ..CaretFormat::default()
        };
        if let Some(props) = props {
            format.bold = props.bold == TriState::On;
            format.italic = props.italic == TriState::On;
            format.underline = props
                .underline
                .is_some_and(|underline| underline != Underline::None);
            format.strike = props.strike == TriState::On;
            format.superscript = props.vert_align == Some(VertAlign::Superscript);
            format.subscript = props.vert_align == Some(VertAlign::Subscript);
            #[allow(clippy::cast_precision_loss)]
            {
                format.size = props.size.map(|size| size.0 as f32 / 2.0);
            }
            format.font = props
                .fonts
                .as_ref()
                .and_then(|fonts| fonts.ascii.as_deref().or(fonts.h_ansi.as_deref()))
                .map(str::to_owned);
        }
        format
    }

    fn send_state(&self) {
        let mut words = 0;
        let mut characters = 0;
        for address in self.paragraphs(&Story::Body) {
            let text = self.text_of(&address);
            let text = text.replace(nav::OBJECT, " ");
            words += text.split_whitespace().count();
            characters += text.chars().filter(|c| !c.is_whitespace()).count();
        }
        let revision = self.session.revision();
        self.send(Reply::State(EditState {
            can_undo: self.undo_depth > 0,
            can_redo: self.redo_depth > 0,
            modified: revision != self.saved_revision,
            words,
            characters,
            read_only: self.read_only.clone(),
        }));
    }

    // ----- navigation ---------------------------------------------------

    fn click(&mut self, page: usize, x: f64, y: f64, extend: bool, column: bool) -> Outcome {
        self.ensure_layout();
        let revision = self.session.revision();
        let Some(map) = &self.map else {
            return Outcome::Quiet;
        };
        // A click on a drawing selects it, the caret before it.
        if !extend && !column {
            if let Ok(Some(object)) = map.hit_drawing(revision, page, x, y) {
                if let Some(offset) = self.object_offset(&object) {
                    self.caret = Some(Logical {
                        paragraph: object.paragraph.clone(),
                        offset,
                    });
                }
                self.anchor = None;
                self.cells = None;
                self.object = Some(object);
                return Outcome::Moved;
            }
        }
        self.object = None;
        let Ok(Some(position)) = map.hit_test(revision, page, x, y) else {
            return Outcome::Quiet;
        };
        let line_start = map
            .caret(revision, &position)
            .ok()
            .and_then(|rects| rects.into_iter().find(|rect| rect.page == page))
            .map(|rect| rect.x - position.offset as f64);
        let mut logical = self.logical(&position);
        // Right of a drawing on the line, the caret goes after it: the map
        // knows only text, which snaps the click to before the drawing.
        for (offset, object) in self.objects_of(&logical.paragraph) {
            let Ok(rects) = map.drawing_rects(revision, &object) else {
                continue;
            };
            let past = rects.iter().any(|rect| {
                rect.page == page
                    && x > rect.x + rect.width
                    && y >= rect.y
                    && y <= rect.y + rect.height
            });
            if past && logical.offset <= offset {
                logical.offset = offset + 1;
            }
        }
        let logical = logical;
        self.goal_x = None;
        let cell = cell_of(&logical.paragraph);
        // Ctrl+click: the column under the pointer.
        if column {
            if let Some(cell) = cell {
                self.caret = Some(logical);
                self.cell_origin = None;
                let rows = self
                    .table_cells(&cell.table)
                    .keys()
                    .map(|(row, _)| row + 1)
                    .max()
                    .unwrap_or(1);
                return self.select_cells(&cell.table, (0, rows - 1), (cell.cell, cell.cell));
            }
        }
        if extend {
            // Dragging from one cell into another selects whole cells.
            if let (Some(origin), Some(cell)) = (self.cell_origin.clone(), cell.clone()) {
                if origin.table == cell.table && (origin.row, origin.cell) != (cell.row, cell.cell)
                {
                    self.caret = Some(logical);
                    return self.select_cells(
                        &cell.table,
                        (origin.row, cell.row),
                        (origin.cell, cell.cell),
                    );
                }
            }
            self.cells = None;
            self.place(logical, true);
            return Outcome::Moved;
        }
        self.cells = None;
        self.cell_origin.clone_from(&cell);
        // A click left of a row's first cell, in the margin, selects the row.
        if let (Some(cell), Some(start)) = (cell, line_start) {
            if cell.cell == 0 && logical.offset == 0 && x < start - 0.12 * self.options.scale {
                self.caret = Some(logical);
                let columns = self
                    .table_cells(&cell.table)
                    .keys()
                    .map(|(_, c)| c + 1)
                    .max()
                    .unwrap_or(1);
                return self.select_cells(&cell.table, (cell.row, cell.row), (0, columns - 1));
            }
        }
        self.place(logical, false);
        Outcome::Moved
    }

    fn select_word(&mut self, page: usize, x: f64, y: f64) -> Outcome {
        if matches!(self.click(page, x, y, false, false), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let text = self.text_of(&caret.paragraph);
        if let Some(range) = nav::word_at(&text, caret.offset) {
            self.anchor = Some(Logical {
                paragraph: caret.paragraph.clone(),
                offset: range.start,
            });
            self.caret = Some(Logical {
                paragraph: caret.paragraph,
                offset: range.end,
            });
        }
        Outcome::Moved
    }

    fn place(&mut self, logical: Logical, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor.clone_from(&self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = Some(logical);
    }

    fn select_all(&mut self) -> Outcome {
        self.cells = None;
        let paragraphs = self.paragraphs(&Story::Body);
        let (Some(first), Some(last)) = (paragraphs.first(), paragraphs.last()) else {
            return Outcome::Quiet;
        };
        let end = self.text_of(last).chars().count();
        self.anchor = Some(Logical {
            paragraph: first.clone(),
            offset: 0,
        });
        self.caret = Some(Logical {
            paragraph: last.clone(),
            offset: end,
        });
        Outcome::Moved
    }

    /// The paragraph before or after `at` in story order.
    fn neighbour(&self, at: &Address, forward: bool) -> Option<Address> {
        let paragraphs = self.paragraphs(&at.story);
        let index = paragraphs.iter().position(|address| address == at)?;
        if forward {
            paragraphs.get(index + 1).cloned()
        } else {
            index
                .checked_sub(1)
                .and_then(|index| paragraphs.get(index).cloned())
        }
    }

    #[allow(clippy::too_many_lines)]
    fn motion(&mut self, motion: Motion, extend: bool) -> Outcome {
        self.ensure_layout();
        if self.cells.is_some() {
            self.collapse_cells();
        }
        // Without extending, a horizontal move collapses the selection to its edge.
        if !extend {
            if let Some((start, end)) = self.selection() {
                if matches!(motion, Motion::Left | Motion::Right) {
                    self.anchor = None;
                    self.caret = Some(if motion == Motion::Left { start } else { end });
                    return Outcome::Moved;
                }
            }
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let text = self.text_of(&caret.paragraph);
        let len = text.chars().count();
        let target = match motion {
            Motion::Left => {
                if caret.offset > 0 {
                    Some(Logical {
                        offset: nav::previous_boundary(&text, caret.offset),
                        ..caret.clone()
                    })
                } else {
                    self.neighbour(&caret.paragraph, false).map(|paragraph| {
                        let offset = self.text_of(&paragraph).chars().count();
                        Logical { paragraph, offset }
                    })
                }
            }
            Motion::Right => {
                if caret.offset < len {
                    Some(Logical {
                        offset: nav::next_boundary(&text, caret.offset),
                        ..caret.clone()
                    })
                } else {
                    self.neighbour(&caret.paragraph, true)
                        .map(|paragraph| Logical {
                            paragraph,
                            offset: 0,
                        })
                }
            }
            Motion::WordLeft => {
                let offset = word_left(&text, caret.offset);
                if caret.offset == 0 {
                    self.neighbour(&caret.paragraph, false).map(|paragraph| {
                        let offset = self.text_of(&paragraph).chars().count();
                        Logical { paragraph, offset }
                    })
                } else {
                    Some(Logical {
                        offset,
                        ..caret.clone()
                    })
                }
            }
            Motion::WordRight => {
                if caret.offset >= len {
                    self.neighbour(&caret.paragraph, true)
                        .map(|paragraph| Logical {
                            paragraph,
                            offset: 0,
                        })
                } else {
                    Some(Logical {
                        offset: word_right(&text, caret.offset),
                        ..caret.clone()
                    })
                }
            }
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                return self.vertical(motion, extend);
            }
            Motion::Home | Motion::End => {
                let Some(rect) = self.caret_rect() else {
                    return Outcome::Quiet;
                };
                let width = self.page_sizes.get(rect.page).map_or(rect.x, |size| size.0);
                let x = if motion == Motion::Home { 0.0 } else { width };
                self.hit(rect.page, x, rect.y + rect.height / 2.0)
            }
            Motion::DocStart => self.first_paragraph().map(|paragraph| Logical {
                paragraph,
                offset: 0,
            }),
            Motion::DocEnd => self
                .paragraphs(&Story::Body)
                .last()
                .cloned()
                .map(|paragraph| {
                    let offset = self.text_of(&paragraph).chars().count();
                    Logical { paragraph, offset }
                }),
        };
        if !matches!(motion, Motion::Up | Motion::Down) {
            self.goal_x = None;
        }
        match target {
            Some(target) => {
                self.place(target, extend);
                Outcome::Moved
            }
            None => Outcome::Quiet,
        }
    }

    fn hit(&self, page: usize, x: f64, y: f64) -> Option<Logical> {
        let map = self.map.as_ref()?;
        let position = map
            .hit_test(self.session.revision(), page, x, y)
            .ok()
            .flatten()?;
        Some(self.logical(&position))
    }

    fn vertical(&mut self, motion: Motion, extend: bool) -> Outcome {
        let Some(rect) = self.caret_rect() else {
            return Outcome::Quiet;
        };
        let x = *self.goal_x.get_or_insert(rect.x);
        let pages = self.page_sizes.len();
        let height = |page: usize| self.page_sizes.get(page).map_or(0.0, |size| size.1);
        let down = matches!(motion, Motion::Down | Motion::PageDown);
        // Page keys jump most of a page first; then, for every key, the probe
        // walks on in small steps until it reaches another line. One step of
        // half a line is not enough: a table cell's spacing after its text,
        // or a paragraph's, is often taller than that.
        let (mut page, mut y) = match motion {
            Motion::PageUp => (rect.page, rect.y - height(rect.page) * 0.8),
            Motion::PageDown => (rect.page, rect.y + rect.height + height(rect.page) * 0.8),
            Motion::Up => (rect.page, rect.y),
            _ => (rect.page, rect.y + rect.height),
        };
        // Far enough for any gap between lines, short of skipping a page.
        for _ in 0..400 {
            y += if down { PROBE_STEP } else { -PROBE_STEP };
            if y < 0.0 {
                if page == 0 {
                    break;
                }
                page -= 1;
                y += height(page);
            } else if y > height(page) {
                if page + 1 >= pages {
                    break;
                }
                y -= height(page);
                page += 1;
            }
            let Some(target) = self.hit(page, x, y) else {
                continue;
            };
            if self.caret.as_ref() != Some(&target) && !self.same_line(&target, &rect) {
                self.place(target, extend);
                return Outcome::Moved;
            }
        }
        Outcome::Quiet
    }

    fn same_line(&self, target: &Logical, rect: &VisualRect) -> bool {
        let Some(map) = &self.map else {
            return false;
        };
        map.caret(self.session.revision(), &self.position(target))
            .ok()
            .and_then(|rects| rects.into_iter().next())
            .is_some_and(|other| other.page == rect.page && (other.y - rect.y).abs() < 1.0)
    }

    // ----- editing ------------------------------------------------------

    fn editor(&mut self) -> Option<&mut DocumentEditor<'a>> {
        match &mut self.session {
            Session::Edit(editor) => Some(editor),
            Session::View(_) => None,
        }
    }

    /// Runs one transaction; on success the redo branch is gone.
    fn transact(&mut self, commands: &[Edit]) -> bool {
        if commands.is_empty() {
            return false;
        }
        let Some(editor) = self.editor() else {
            return false;
        };
        let revision = editor.revision();
        let before = revision;
        match editor.transact(revision, commands) {
            Ok(change) => {
                if change.revision != before {
                    self.undo_depth += 1;
                    self.redo_depth = 0;
                    self.stale = true;
                }
                true
            }
            Err(error) => {
                self.note("note.edit_refused", format!("{error:?}"), true);
                false
            }
        }
    }

    /// Commands that put `text` in place of the offsets `start..end` of one
    /// paragraph.
    ///
    /// Inside runs that is one `ReplaceAt` between kernel points; it removes
    /// tabs and breaks inside runs as well. Tabs and breaks between runs have
    /// no kernel point beside them, so a range of only those is deleted
    /// inline by inline, and text typed there goes in as a run of its own.
    fn paragraph_replace(
        &self,
        at: &Address,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<Vec<Edit>, &'static str> {
        let pieces = self.pieces(at);
        if pieces.is_empty() {
            return Ok(if text.is_empty() {
                Vec::new()
            } else {
                vec![Edit::Text {
                    at: at.clone(),
                    range: 0..0,
                    text: text.to_owned(),
                }]
            });
        }
        if let (Some(from), Some(to)) = (nav::point_at(&pieces, start), nav::point_at(&pieces, end))
        {
            if start == end && text.is_empty() {
                return Ok(Vec::new());
            }
            return Ok(vec![Edit::ReplaceAt {
                at: at.clone(),
                from,
                to,
                text: text.to_owned(),
            }]);
        }
        let mut atoms = Vec::new();
        let mut insert_at = None;
        let mut before = 0;
        for piece in &pieces {
            let piece_end = before + piece.len();
            let top = piece.inline.first().copied().unwrap_or(0);
            if piece.kind == nav::PieceKind::InlineAtom && before >= start && piece_end <= end {
                atoms.push(top);
            } else if before < end && piece_end > start {
                return Err("note.cannot_edit_here");
            }
            if piece_end == start && insert_at.is_none() {
                insert_at = Some(top + 1);
            }
            if before == end && insert_at.is_none() {
                insert_at = Some(top);
            }
            before = piece_end;
        }
        let mut commands: Vec<Edit> = atoms
            .iter()
            .rev()
            .map(|index| Edit::DeleteInline {
                at: at.clone(),
                index: *index,
            })
            .collect();
        if !text.is_empty() {
            let index = atoms.first().copied().or(insert_at).unwrap_or(0);
            let props = self
                .paragraph(at)
                .and_then(|paragraph| {
                    paragraph.inlines.iter().find_map(|inline| match inline {
                        strict_ooxml::model::Inline::Run(run) => Some(run.props.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_default();
            commands.push(Edit::InsertInline {
                at: at.clone(),
                index,
                inline: Box::new(strict_ooxml::model::Inline::Run(text_run(props, text))),
            });
        }
        Ok(commands)
    }

    /// Commands that replace `start..end` with `text`.
    fn replace_commands(
        &self,
        start: &Logical,
        end: &Logical,
        text: &str,
    ) -> Result<Vec<Edit>, &'static str> {
        if start.paragraph == end.paragraph {
            return self.paragraph_replace(&start.paragraph, start.offset, end.offset, text);
        }
        // Across paragraphs: only siblings of one container, one after the other.
        let paragraphs = self.paragraphs(&start.paragraph.story);
        let from = paragraphs
            .iter()
            .position(|address| *address == start.paragraph)
            .ok_or("note.no_paragraph")?;
        let to = paragraphs
            .iter()
            .position(|address| *address == end.paragraph)
            .ok_or("note.no_paragraph")?;
        let span = paragraphs.get(from..=to).ok_or("note.no_paragraph")?;
        let siblings = span.windows(2).all(|pair| match pair {
            [a, b] => a.containers == b.containers && b.block == a.block + 1,
            _ => true,
        });
        if !siblings {
            return Err("note.selection_structure");
        }
        let first_len = self.text_of(&start.paragraph).chars().count();
        let mut commands = self.paragraph_replace(&end.paragraph, 0, end.offset, "")?;
        for middle in span.iter().skip(1).take(span.len().saturating_sub(2)).rev() {
            commands.push(Edit::Delete { at: middle.clone() });
        }
        commands.extend(self.paragraph_replace(&start.paragraph, start.offset, first_len, text)?);
        commands.push(Edit::Join {
            at: start.paragraph.clone(),
        });
        Ok(commands)
    }

    /// Replaces the selection (or inserts at the caret) with `text`.
    fn put_text(&mut self, text: &str) -> bool {
        if self.cells.is_some() {
            self.collapse_cells();
        }
        let Some(caret) = self.caret.clone() else {
            return false;
        };
        let (start, end) = self.selection().unwrap_or_else(|| (caret.clone(), caret));
        if start == end && text.is_empty() {
            return false;
        }
        let commands = match self.replace_commands(&start, &end, text) {
            Ok(commands) => commands,
            Err(key) => {
                self.note(key, String::new(), true);
                return false;
            }
        };
        if commands.is_empty() || self.transact(&commands) {
            self.anchor = None;
            self.caret = Some(Logical {
                offset: start.offset + text.chars().count(),
                paragraph: start.paragraph,
            });
            true
        } else {
            false
        }
    }

    fn type_text(&mut self, text: &str) -> Outcome {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return Outcome::Quiet;
        }
        if self.put_text(&clean) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Tab: the next cell in a table (a new row after the last), otherwise a
    /// tab character. Shift+Tab: the previous cell.
    fn tab(&mut self, back: bool) -> Outcome {
        if self.in_cell().is_some() {
            return self.next_cell(back);
        }
        if back {
            return Outcome::Quiet;
        }
        if self.selection().is_some() && matches!(self.delete_selection(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let at = caret.paragraph.clone();
        let commands = self.inline_at_caret(&caret, strict_ooxml::model::Inline::Tab);
        if self.transact(&commands) {
            self.anchor = None;
            self.caret = Some(Logical {
                paragraph: at,
                offset: caret.offset + 1,
            });
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Commands that put `inline` at the caret, between runs.
    fn inline_at_caret(&self, caret: &Logical, inline: strict_ooxml::model::Inline) -> Vec<Edit> {
        let at = caret.paragraph.clone();
        // Split there, put the inline first in the right half, join again:
        // the kernel inserts inlines between runs, and this makes the caret's
        // point a boundary between runs wherever it was.
        if let Some(point) = self.point(caret) {
            let mut right = at.clone();
            right.block += 1;
            return vec![
                Edit::SplitAt {
                    at: at.clone(),
                    point,
                },
                Edit::InsertInline {
                    at: right,
                    index: 0,
                    inline: Box::new(inline),
                },
                Edit::Join { at },
            ];
        }
        // An empty paragraph, or the caret beside atoms between runs.
        let pieces = self.pieces(&at);
        let mut before = 0;
        let mut index = 0;
        for piece in &pieces {
            before += piece.len();
            if before == caret.offset {
                index = piece.inline.first().copied().unwrap_or(0) + 1;
            }
        }
        vec![Edit::InsertInline {
            at,
            index,
            inline: Box::new(inline),
        }]
    }

    // ----- drawings -----------------------------------------------------

    /// The caret offset of a drawing's atom.
    fn object_offset(&self, object: &DrawingPosition) -> Option<usize> {
        let mut before = 0;
        for piece in self.pieces(&object.paragraph) {
            let atom = piece.text.starts_with(nav::OBJECT) && piece.inline == object.inline;
            let here = match object.content {
                Some(content) => piece.kind == nav::PieceKind::RunAtom && piece.content == content,
                None => piece.kind == nav::PieceKind::InlineAtom,
            };
            if atom && here {
                return Some(before);
            }
            before += piece.len();
        }
        None
    }

    /// The drawing a position names.
    fn drawing_at(&self, object: &DrawingPosition) -> Option<strict_ooxml::model::Drawing> {
        use strict_ooxml::model::{Inline, RunContent};
        let paragraph = self.paragraph(&object.paragraph)?;
        let (last, wrappers) = object.inline.split_last()?;
        let mut inlines = &paragraph.inlines;
        for index in wrappers {
            inlines = match inlines.get(*index)? {
                Inline::Hyperlink(v) => &v.inlines,
                Inline::SdtInline(v) => &v.inlines,
                Inline::Directional(v) => &v.inlines,
                _ => return None,
            };
        }
        match (inlines.get(*last)?, object.content) {
            (Inline::Drawing(drawing), None) => Some(drawing.clone()),
            (Inline::Run(run), Some(content)) => match run.content.get(content)? {
                RunContent::Drawing(drawing) => Some(drawing.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn object_view(&self) -> Option<ObjectView> {
        use strict_ooxml::model::{DrawingKind, Graphic};
        let object = self.object.as_ref()?;
        let map = self.map.as_ref()?;
        let rects = map
            .drawing_rects(self.session.revision(), object)
            .ok()
            .filter(|rects| !rects.is_empty())?;
        let drawing = self.drawing_at(object)?;
        let (extent, graphic) = match &drawing.kind {
            DrawingKind::Inline(v) => (v.extent, Some(&*v.graphic)),
            DrawingKind::Anchor(v) => (v.extent, Some(&*v.graphic)),
            DrawingKind::Opaque(_) => (None, None),
        };
        Some(ObjectView {
            rects,
            width: extent.map_or(0, |e| e.cx.0),
            height: extent.map_or(0, |e| e.cy.0),
            picture: matches!(graphic, Some(Graphic::Picture(_))),
            wrap: wrap_style(&drawing),
        })
    }

    /// Whether a text node holds the caret's offset.
    fn text_at_caret(&self) -> bool {
        let Some(caret) = &self.caret else {
            return false;
        };
        let mut before = 0;
        for piece in self.pieces(&caret.paragraph) {
            let end = before + piece.len();
            if piece.kind == nav::PieceKind::Text && (before..=end).contains(&caret.offset) {
                return true;
            }
            before = end;
        }
        false
    }

    /// The drawings of a paragraph with their caret offsets.
    fn objects_of(&self, paragraph: &Address) -> Vec<(usize, DrawingPosition)> {
        let mut out = Vec::new();
        let mut before = 0;
        for piece in self.pieces(paragraph) {
            if piece.text.starts_with(nav::OBJECT) {
                out.push((
                    before,
                    DrawingPosition {
                        paragraph: paragraph.clone(),
                        inline: piece.inline.clone(),
                        content: (piece.kind == nav::PieceKind::RunAtom).then_some(piece.content),
                    },
                ));
            }
            before += piece.len();
        }
        out
    }

    /// Where the drawings inside the selection are drawn.
    fn selected_object_rects(&self) -> Vec<VisualRect> {
        let (Some((start, end)), Some(map)) = (self.selection(), &self.map) else {
            return Vec::new();
        };
        let revision = self.session.revision();
        let paragraphs = self.paragraphs(&start.paragraph.story);
        let from = paragraphs.iter().position(|a| *a == start.paragraph);
        let to = paragraphs.iter().position(|a| *a == end.paragraph);
        let (Some(from), Some(to)) = (from, to) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (index, paragraph) in paragraphs.iter().enumerate().take(to + 1).skip(from) {
            for (offset, object) in self.objects_of(paragraph) {
                let after_start = index > from || offset >= start.offset;
                let before_end = index < to || offset < end.offset;
                if after_start && before_end {
                    out.extend(map.drawing_rects(revision, &object).unwrap_or_default());
                }
            }
        }
        out
    }

    /// The caret right after a drawing that no text follows: on the
    /// drawing's right edge.
    fn caret_beside_object(&self) -> Vec<VisualRect> {
        let (Some(caret), Some(map)) = (&self.caret, &self.map) else {
            return Vec::new();
        };
        let mut before = 0;
        for piece in self.pieces(&caret.paragraph) {
            let end = before + piece.len();
            if piece.text.starts_with(nav::OBJECT) && end == caret.offset {
                let object = DrawingPosition {
                    paragraph: caret.paragraph.clone(),
                    inline: piece.inline.clone(),
                    content: (piece.kind == nav::PieceKind::RunAtom).then_some(piece.content),
                };
                return map
                    .drawing_rects(self.session.revision(), &object)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|rect| VisualRect {
                        x: rect.x + rect.width,
                        width: 1.0,
                        ..rect
                    })
                    .collect();
            }
            before = end;
        }
        Vec::new()
    }

    fn delete_object(&mut self) -> Outcome {
        let Some(object) = self.object.take() else {
            return Outcome::Quiet;
        };
        let Some(offset) = self.object_offset(&object) else {
            return Outcome::Quiet;
        };
        self.anchor = Some(Logical {
            paragraph: object.paragraph.clone(),
            offset,
        });
        self.caret = Some(Logical {
            paragraph: object.paragraph,
            offset: offset + 1,
        });
        self.delete_selection()
    }

    fn object_command(&mut self, command: ObjectCommand) -> Outcome {
        match command {
            ObjectCommand::Delete => self.delete_object(),
            ObjectCommand::Edit(edit) => self.edit_picture(edit),
            ObjectCommand::Wrap(style) => self.wrap_object(style),
            ObjectCommand::Move { dx, dy } => self.move_floating(dx, dy),
            ObjectCommand::MoveTo { page, x, y } => self.move_in_text(page, x, y),
            ObjectCommand::Export { purpose } => {
                let id = self
                    .object
                    .as_ref()
                    .and_then(|object| self.drawing_at(object))
                    .map_or(0, |drawing| drawing_id(&drawing));
                if let Some(bytes) = self.object_bytes() {
                    let extension = strict_ooxml::edit::picture_format(&bytes)
                        .map_or("bin", |(_, _, extension)| extension);
                    self.send(Reply::Picture {
                        id,
                        bytes,
                        extension,
                        purpose,
                    });
                }
                Outcome::Quiet
            }
            ObjectCommand::Replace { id, bytes } => self.replace_picture(id, bytes),
            ObjectCommand::Resize { width, height } => self.resize_object(width, height),
            ObjectCommand::ResetSize => {
                let size = self.object_pixels();
                match size {
                    Some((width, height)) => self.resize_object(
                        i64::from(width) * strict_ooxml::edit::EMU_PER_PX,
                        i64::from(height) * strict_ooxml::edit::EMU_PER_PX,
                    ),
                    None => Outcome::Quiet,
                }
            }
            ObjectCommand::InsertPicture { bytes, name } => self.insert_picture(bytes, &name),
            ObjectCommand::Recognize {
                formula,
                endpoint,
                model,
            } => self.recognize_object(formula, &endpoint, &model),
        }
    }

    /// The pixel size of the selected picture's image.
    fn object_pixels(&self) -> Option<(u32, u32)> {
        let bytes = self.object_bytes()?;
        strict_ooxml::edit::picture_pixels(&bytes)
    }

    /// The image bytes of the selected picture.
    fn object_bytes(&self) -> Option<Vec<u8>> {
        use strict_ooxml::model::{DrawingKind, Graphic};
        let drawing = self.drawing_at(self.object.as_ref()?)?;
        let graphic = match &drawing.kind {
            DrawingKind::Inline(v) => &v.graphic,
            DrawingKind::Anchor(v) => &v.graphic,
            DrawingKind::Opaque(_) => return None,
        };
        let Graphic::Picture(picture) = graphic.as_ref() else {
            return None;
        };
        let part = picture.blip.as_ref()?.resolved.as_ref()?;
        match &self.session {
            Session::Edit(editor) => editor.media_bytes(part).ok(),
            Session::View(document) => document.package().read_part(part).ok(),
        }
    }

    /// Recognises the selected picture with a vision model and puts what it
    /// reads in its place: text, or the formula as a Word equation.
    #[cfg(feature = "ocr")]
    fn recognize_object(&mut self, formula: bool, endpoint: &str, model: &str) -> Outcome {
        use strict_ooxml::{OcrImage, TextRecovery};
        let Some(bytes) = self.object_bytes() else {
            self.note("note.recognize_failed", "no picture bytes", true);
            return Outcome::Quiet;
        };
        let image = match strict_ooxml::edit::picture_format(&bytes) {
            Some((strict_ooxml::model::MediaKind::Png, ..)) => OcrImage::png(bytes),
            Some((strict_ooxml::model::MediaKind::Jpeg, ..)) => OcrImage::jpeg(bytes),
            Some(_) => {
                let Some(png) = crate::raster::picture_png(&bytes) else {
                    self.note(
                        "note.recognize_failed",
                        "the picture could not be read",
                        true,
                    );
                    return Outcome::Quiet;
                };
                OcrImage::png(png)
            }
            None => {
                self.note("note.recognize_failed", "not a picture format", true);
                return Outcome::Quiet;
            }
        };
        let mut config = strict_ooxml::OllamaConfig::from_env();
        endpoint.clone_into(&mut config.endpoint);
        model.clone_into(&mut config.model);
        let vision = strict_ooxml::OllamaVision::new(config);
        self.send(Reply::Busy(Some(Busy::Recognize)));
        let answer = if formula {
            vision.recover_formula(&image)
        } else {
            vision.recover_region(&image)
        };
        self.send(Reply::Busy(None));
        let text = match answer {
            Ok(Some(recovered)) => recovered.text,
            Ok(None) => {
                self.note("note.recognized_nothing", String::new(), false);
                return Outcome::Quiet;
            }
            Err(error) => {
                self.note("note.recognize_failed", error.to_string(), true);
                return Outcome::Quiet;
            }
        };
        let replacement = if formula {
            match strict_ooxml::math_from_mathml(&text) {
                Ok(expression) => Replacement::Formula(expression),
                Err(error) => {
                    self.note("note.recognize_failed", error.to_string(), true);
                    return Outcome::Quiet;
                }
            }
        } else {
            Replacement::Text(text.clone())
        };
        // A drawing between runs is replaced in one step, one undo.
        if let Some(outcome) = self.replace_object_whole(&replacement) {
            return outcome;
        }
        if let Replacement::Formula(expression) = replacement {
            if matches!(self.delete_object(), Outcome::Quiet) {
                return Outcome::Quiet;
            }
            let Some(caret) = self.caret.clone() else {
                return Outcome::Edited;
            };
            let commands =
                self.inline_at_caret(&caret, strict_ooxml::model::Inline::Math(expression));
            if self.transact(&commands) {
                self.caret = Some(Logical {
                    paragraph: caret.paragraph,
                    offset: caret.offset,
                });
            }
            return Outcome::Edited;
        }
        if matches!(self.delete_object(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        self.paste(&text);
        Outcome::Edited
    }

    /// Replaces the selected drawing, when it stands between runs, by text
    /// (a paragraph split at each line break) or a formula, in one
    /// transaction. `None` for a drawing inside a run.
    #[cfg(feature = "ocr")]
    fn replace_object_whole(&mut self, replacement: &Replacement) -> Option<Outcome> {
        use strict_ooxml::edit::InlinePoint;
        use strict_ooxml::model::{Inline, Run, RunContent, Space, TextNode};
        let object = self.object.clone()?;
        let ([index], None) = (object.inline.as_slice(), object.content) else {
            return None;
        };
        let index = *index;
        let at = object.paragraph.clone();
        let offset = self.object_offset(&object)?;
        let paragraph = self.paragraph(&at)?;
        // New text takes the look of the run before the drawing, or after.
        let props = paragraph
            .inlines
            .get(..index)
            .and_then(|before| {
                before.iter().rev().find_map(|inline| match inline {
                    Inline::Run(run) => Some(run.props.clone()),
                    _ => None,
                })
            })
            .or_else(|| {
                paragraph
                    .inlines
                    .get(index + 1..)?
                    .iter()
                    .find_map(|inline| match inline {
                        Inline::Run(run) => Some(run.props.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_default();
        let run = |text: &str| {
            Box::new(Inline::Run(Run {
                props: props.clone(),
                content: vec![RunContent::Text(TextNode {
                    text: text.to_owned(),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::unknown(),
            }))
        };
        let mut commands = vec![Edit::DeleteInline {
            at: at.clone(),
            index,
        }];
        let mut caret = Logical {
            paragraph: at.clone(),
            offset,
        };
        match replacement {
            Replacement::Formula(expression) => {
                commands.push(Edit::InsertInline {
                    at: at.clone(),
                    index,
                    inline: Box::new(Inline::Math(expression.clone())),
                });
            }
            Replacement::Text(text) => {
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                let lines: Vec<String> = text
                    .split('\n')
                    .map(|line| {
                        line.chars()
                            .map(|c| if c == '\t' { ' ' } else { c })
                            .filter(|c| !c.is_control())
                            .collect()
                    })
                    .collect();
                let mut here = at.clone();
                let mut slot = index;
                for (number, line) in lines.iter().enumerate() {
                    if number > 0 {
                        // The rest of the paragraph goes on after this line.
                        let previous = lines.get(number - 1).map_or(0, |l| l.chars().count());
                        commands.push(Edit::SplitAt {
                            at: here.clone(),
                            point: InlinePoint {
                                inline: vec![slot],
                                content: 0,
                                offset: previous,
                            },
                        });
                        here.block += 1;
                        slot = 0;
                    }
                    commands.push(Edit::InsertInline {
                        at: here.clone(),
                        index: slot,
                        inline: run(line),
                    });
                }
                caret = Logical {
                    paragraph: here,
                    offset: if lines.len() > 1 { 0 } else { offset }
                        + lines.last().map_or(0, |l| l.chars().count()),
                };
            }
        }
        if !self.transact(&commands) {
            return Some(Outcome::Quiet);
        }
        self.object = None;
        self.anchor = None;
        self.caret = Some(caret);
        Some(Outcome::Edited)
    }

    #[cfg(not(feature = "ocr"))]
    fn recognize_object(&mut self, _formula: bool, _endpoint: &str, _model: &str) -> Outcome {
        self.note(
            "note.recognize_failed",
            "a build without the `ocr` feature",
            true,
        );
        Outcome::Quiet
    }

    /// Changes the selected picture's pixels: the picture shows new bytes,
    /// its frame turned or cut to match. A crop the document gave the picture
    /// is made in its pixels first.
    fn edit_picture(&mut self, edit: PictureEdit) -> Outcome {
        use strict_ooxml::edit::Definition;
        use strict_ooxml::model::{DrawingKind, Emu, Extent, Graphic};
        let Some(object) = self.object.clone() else {
            return Outcome::Quiet;
        };
        let (Some(mut drawing), Some(bytes)) = (self.drawing_at(&object), self.object_bytes())
        else {
            return Outcome::Quiet;
        };
        let (frame, graphic) = match &mut drawing.kind {
            DrawingKind::Inline(v) => (&mut v.extent, &mut v.graphic),
            DrawingKind::Anchor(v) => (&mut v.extent, &mut v.graphic),
            DrawingKind::Opaque(_) => return Outcome::Quiet,
        };
        let Graphic::Picture(picture) = graphic.as_mut() else {
            return Outcome::Quiet;
        };
        #[allow(clippy::cast_precision_loss)]
        let bytes = match picture.src_rect.take() {
            Some(crop) => {
                let side = |value: i32| value as f32 / 100_000.0;
                let sides = [
                    side(crop.left),
                    side(crop.top),
                    side(crop.right),
                    side(crop.bottom),
                ];
                crate::raster::picture_cropped(&bytes, sides).unwrap_or(bytes)
            }
            None => bytes,
        };
        let size = frame.map_or((0, 0), |extent| (extent.cx.0, extent.cy.0));
        let (edited, size) = match edit {
            PictureEdit::Rotate { clockwise } => (
                crate::raster::picture_rotated(&bytes, clockwise),
                (size.1, size.0),
            ),
            PictureEdit::Flip { vertical } => {
                (crate::raster::picture_flipped(&bytes, vertical), size)
            }
            PictureEdit::Adjust {
                brightness,
                contrast,
            } => (
                crate::raster::picture_adjusted(&bytes, brightness, contrast),
                size,
            ),
            PictureEdit::Crop { sides } => {
                let [left, top, right, bottom] = sides.map(|s| f64::from(s.clamp(0.0, 0.95)));
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                let keep =
                    |value: i64, cut: f64| ((value as f64) * (1.0 - cut).max(0.02)).round() as i64;
                (
                    crate::raster::picture_cropped(&bytes, sides),
                    (keep(size.0, left + right), keep(size.1, top + bottom)),
                )
            }
        };
        let Some(edited) = edited else {
            self.note(
                "note.picture_refused",
                "the picture could not be read",
                true,
            );
            return Outcome::Quiet;
        };
        let extent = Extent {
            cx: Emu(size.0.max(1)),
            cy: Emu(size.1.max(1)),
        };
        *frame = Some(extent);
        if picture.extent.is_some() {
            picture.extent = Some(extent);
        }
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let Ok(item) = editor.add_media(edited) else {
            return Outcome::Quiet;
        };
        set_picture_part(&mut drawing, &item.part);
        let command = Edit::Drawing {
            at: object.paragraph.clone(),
            inline: object.inline.clone(),
            content: object.content,
            drawing: Box::new(drawing),
        };
        let outcome = self.define_and_transact(&[Definition::AddMedia(Box::new(item))], &[command]);
        self.object = Some(object);
        outcome
    }

    /// Gives the picture with drawing id `id` new bytes: its width stays,
    /// its height follows the new picture's proportions.
    fn replace_picture(&mut self, id: u32, bytes: Vec<u8>) -> Outcome {
        use strict_ooxml::edit::{picture_pixels, Definition};
        use strict_ooxml::model::{DrawingKind, Emu, Extent, Graphic};
        let found = self
            .paragraphs(&Story::Body)
            .into_iter()
            .find_map(|paragraph| {
                self.objects_of(&paragraph)
                    .into_iter()
                    .find_map(|(_, object)| {
                        let drawing = self.drawing_at(&object)?;
                        (drawing_id(&drawing) == id).then_some((object, drawing))
                    })
            });
        let Some((object, mut drawing)) = found else {
            self.note(
                "note.picture_refused",
                "the picture is no longer there",
                true,
            );
            return Outcome::Quiet;
        };
        let pixels = picture_pixels(&bytes);
        let (frame, graphic) = match &mut drawing.kind {
            DrawingKind::Inline(v) => (&mut v.extent, &mut v.graphic),
            DrawingKind::Anchor(v) => (&mut v.extent, &mut v.graphic),
            DrawingKind::Opaque(_) => return Outcome::Quiet,
        };
        if let (Some(extent), Some((w, h))) = (frame.as_mut(), pixels) {
            extent.cy = Emu((extent.cx.0 * i64::from(h) / i64::from(w.max(1))).max(1));
            let extent: Extent = *extent;
            if let Graphic::Picture(picture) = graphic.as_mut() {
                picture.src_rect = None;
                if picture.extent.is_some() {
                    picture.extent = Some(extent);
                }
            }
        }
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let item = match editor.add_media(bytes) {
            Ok(item) => item,
            Err(error) => {
                self.note("note.picture_refused", format!("{error:?}"), true);
                return Outcome::Quiet;
            }
        };
        set_picture_part(&mut drawing, &item.part);
        let command = Edit::Drawing {
            at: object.paragraph.clone(),
            inline: object.inline.clone(),
            content: object.content,
            drawing: Box::new(drawing),
        };
        let outcome = self.define_and_transact(&[Definition::AddMedia(Box::new(item))], &[command]);
        if matches!(outcome, Outcome::Edited) {
            self.object = Some(object);
        }
        outcome
    }

    /// Moves the selected floating drawing by page pixels. An offset it has
    /// grows; a position given by alignment becomes an offset where it is
    /// drawn: from the column across, from the page down.
    fn move_floating(&mut self, dx: f64, dy: f64) -> Outcome {
        use strict_ooxml::model::drawing::Position;
        use strict_ooxml::model::{DrawingKind, Emu};
        let Some(object) = self.object.clone() else {
            return Outcome::Quiet;
        };
        let (Some(mut drawing), Some(rect)) = (
            self.drawing_at(&object),
            self.object_view()
                .and_then(|view| view.rects.first().copied()),
        ) else {
            return Outcome::Quiet;
        };
        let DrawingKind::Anchor(anchor) = &mut drawing.kind else {
            return Outcome::Quiet;
        };
        #[allow(clippy::cast_possible_truncation)]
        let emu = |px: f64| (px * 9525.0).round() as i64;
        let column = f64::from(self.text_left()) / 15.0;
        let across = anchor.position_h.get_or_insert_with(Position::default);
        match across.offset {
            Some(offset) if across.align.is_none() => across.offset = Some(Emu(offset.0 + emu(dx))),
            _ => {
                *across = Position {
                    relative_from: Some(std::sync::Arc::from("column")),
                    offset: Some(Emu(emu(rect.x - column + dx))),
                    ..Position::default()
                };
            }
        }
        let down = anchor.position_v.get_or_insert_with(Position::default);
        match down.offset {
            Some(offset) if down.align.is_none() => down.offset = Some(Emu(offset.0 + emu(dy))),
            _ => {
                *down = Position {
                    relative_from: Some(std::sync::Arc::from("page")),
                    offset: Some(Emu(emu(rect.y + dy))),
                    ..Position::default()
                };
            }
        }
        anchor.simple_pos = false;
        let command = Edit::Drawing {
            at: object.paragraph.clone(),
            inline: object.inline.clone(),
            content: object.content,
            drawing: Box::new(drawing),
        };
        if self.transact(&[command]) {
            self.object = Some(object);
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Moves the selected drawing to another place in the text.
    fn move_in_text(&mut self, page: usize, x: f64, y: f64) -> Outcome {
        let Some(object) = self.object.clone() else {
            return Outcome::Quiet;
        };
        let revision = self.session.revision();
        let Some(target) = self
            .map
            .as_ref()
            .and_then(|map| map.hit_test(revision, page, x, y).ok().flatten())
        else {
            return Outcome::Quiet;
        };
        let mut target = self.logical(&target);
        let (Some(drawing), Some(offset)) = (self.drawing_at(&object), self.object_offset(&object))
        else {
            return Outcome::Quiet;
        };
        // Dropped on itself: nothing moves.
        if target.paragraph == object.paragraph && (offset..=offset + 1).contains(&target.offset) {
            return Outcome::Quiet;
        }
        if matches!(self.delete_object(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        if target.paragraph == object.paragraph && target.offset > offset {
            target.offset -= 1;
        }
        // The media part stays indexed after the delete, so the drawing
        // goes back as it is.
        if self.insert_drawing(&target, drawing, None) {
            self.caret = Some(target.clone());
            self.object = self
                .objects_of(&target.paragraph)
                .into_iter()
                .find(|(at, _)| *at == target.offset)
                .map(|(_, object)| object);
        }
        Outcome::Edited
    }

    /// Sets how text flows around the selected drawing. A drawing taken out
    /// of the line floats where it was drawn, from its column and paragraph.
    fn wrap_object(&mut self, style: WrapStyle) -> Outcome {
        let Some(object) = self.object.clone() else {
            return Outcome::Quiet;
        };
        let Some(drawing) = self.drawing_at(&object) else {
            return Outcome::Quiet;
        };
        if wrap_style(&drawing) == style {
            return Outcome::Quiet;
        }
        // Where it stands now, from the column's left edge, in EMU.
        let left = self
            .object_view()
            .and_then(|view| view.rects.first().copied())
            .map_or(0, |rect| {
                let column = f64::from(self.text_left()) / 15.0;
                #[allow(clippy::cast_possible_truncation)]
                let emu = ((rect.x - column) * 9525.0).round() as i64;
                emu.max(0)
            });
        let drawing = rewrapped(drawing, style, left);
        let command = Edit::Drawing {
            at: object.paragraph.clone(),
            inline: object.inline.clone(),
            content: object.content,
            drawing: Box::new(drawing),
        };
        if self.transact(&[command]) {
            self.object = Some(object);
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// The left margin of the last section, in twips.
    fn text_left(&self) -> i32 {
        self.session
            .document()
            .sections
            .last()
            .and_then(|section| section.properties.page_margins.as_ref()?.left)
            .map_or(1440, |twips| twips.0)
    }

    fn resize_object(&mut self, width: i64, height: i64) -> Outcome {
        use strict_ooxml::model::{DrawingKind, Emu, Extent, Graphic};
        let Some(object) = self.object.clone() else {
            return Outcome::Quiet;
        };
        let Some(mut drawing) = self.drawing_at(&object) else {
            return Outcome::Quiet;
        };
        if width <= 0 || height <= 0 {
            return Outcome::Quiet;
        }
        let extent = Extent {
            cx: Emu(width),
            cy: Emu(height),
        };
        let (frame, graphic) = match &mut drawing.kind {
            DrawingKind::Inline(v) => (&mut v.extent, &mut v.graphic),
            DrawingKind::Anchor(v) => (&mut v.extent, &mut v.graphic),
            DrawingKind::Opaque(_) => return Outcome::Quiet,
        };
        *frame = Some(extent);
        if let Graphic::Picture(picture) = graphic.as_mut() {
            if picture.extent.is_some() {
                picture.extent = Some(extent);
            }
        }
        let command = Edit::Drawing {
            at: object.paragraph.clone(),
            inline: object.inline.clone(),
            content: object.content,
            drawing: Box::new(drawing),
        };
        if self.transact(&[command]) {
            self.object = Some(object);
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn insert_picture(&mut self, bytes: Vec<u8>, name: &str) -> Outcome {
        use strict_ooxml::edit::{
            picture_drawing, picture_pixels, unused_drawing_id, Definition, EMU_PER_PX,
        };
        use strict_ooxml::model::{Emu, Inline};
        if self.selection().is_some() && matches!(self.delete_selection(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let (pixels_wide, pixels_high) = picture_pixels(&bytes).unwrap_or((96, 96));
        // At its own size, but no wider than the text.
        let room = i64::from(self.text_width().max(1440)) * 635;
        let mut width = i64::from(pixels_wide) * EMU_PER_PX;
        let mut height = i64::from(pixels_high) * EMU_PER_PX;
        if width > room {
            height = height * room / width;
            width = room;
        }
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let item = match editor.add_media(bytes) {
            Ok(item) => item,
            Err(error) => {
                self.note("note.picture_refused", format!("{error:?}"), true);
                return Outcome::Quiet;
            }
        };
        let id = unused_drawing_id(self.session.document());
        let drawing = picture_drawing(&item, Emu(width), Emu(height.max(1)), id, name);
        let commands = self.inline_at_caret(&caret, Inline::Drawing(drawing));
        let outcome = self.define_and_transact(&[Definition::AddMedia(Box::new(item))], &commands);
        if matches!(outcome, Outcome::Edited) {
            // The new picture is selected, the caret before it.
            self.anchor = None;
            self.caret = Some(caret.clone());
            self.object = self
                .pieces(&caret.paragraph)
                .iter()
                .scan(0, |before, piece| {
                    let start = *before;
                    *before += piece.len();
                    Some((start, piece.clone()))
                })
                .find(|(start, piece)| {
                    *start == caret.offset && piece.text.starts_with(nav::OBJECT)
                })
                .map(|(_, piece)| DrawingPosition {
                    paragraph: caret.paragraph.clone(),
                    inline: piece.inline.clone(),
                    content: (piece.kind == nav::PieceKind::RunAtom).then_some(piece.content),
                });
        }
        outcome
    }

    fn backspace(&mut self) -> Outcome {
        if self.cells.is_some() {
            return self.clear_cells();
        }
        if self.selection().is_some() {
            return self.delete_selection();
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        if caret.offset > 0 {
            let text = self.text_of(&caret.paragraph);
            self.anchor = Some(Logical {
                offset: nav::previous_boundary(&text, caret.offset),
                ..caret
            });
            return self.delete_selection();
        }
        // At the start of a paragraph: join it to the one before.
        let Some(previous) = self.neighbour(&caret.paragraph, false) else {
            return Outcome::Quiet;
        };
        if previous.containers != caret.paragraph.containers
            || previous.block + 1 != caret.paragraph.block
        {
            return Outcome::Quiet;
        }
        let offset = self.text_of(&previous).chars().count();
        if self.transact(&[Edit::Join {
            at: previous.clone(),
        }]) {
            self.anchor = None;
            self.caret = Some(Logical {
                paragraph: previous,
                offset,
            });
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn delete_forward(&mut self) -> Outcome {
        if self.cells.is_some() {
            return self.clear_cells();
        }
        if self.selection().is_some() {
            return self.delete_selection();
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let text = self.text_of(&caret.paragraph);
        if caret.offset < text.chars().count() {
            self.anchor = Some(Logical {
                offset: nav::next_boundary(&text, caret.offset),
                ..caret.clone()
            });
            let outcome = self.delete_selection();
            self.caret = Some(caret);
            return outcome;
        }
        let Some(next) = self.neighbour(&caret.paragraph, true) else {
            return Outcome::Quiet;
        };
        if next.containers != caret.paragraph.containers || caret.paragraph.block + 1 != next.block
        {
            return Outcome::Quiet;
        }
        if self.transact(&[Edit::Join {
            at: caret.paragraph.clone(),
        }]) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn delete_selection(&mut self) -> Outcome {
        let Some((start, end)) = self.selection() else {
            self.anchor = None;
            return Outcome::Quiet;
        };
        match self.replace_commands(&start, &end, "") {
            Ok(commands) => {
                if commands.is_empty() || self.transact(&commands) {
                    self.anchor = None;
                    self.caret = Some(start);
                    Outcome::Edited
                } else {
                    self.anchor = None;
                    Outcome::Moved
                }
            }
            Err(key) => {
                self.note(key, String::new(), true);
                Outcome::Quiet
            }
        }
    }

    fn enter(&mut self) -> Outcome {
        if self.cells.is_some() {
            self.collapse_cells();
        }
        if self.selection().is_some() && matches!(self.delete_selection(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        if self.split() {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Splits the caret's paragraph and puts the caret at the new one's start.
    /// The kernel point at a caret, made where there is none: beside tabs
    /// between runs an empty run goes in first, in the same transaction, and
    /// the point is its start.
    fn point_or_run(
        &self,
        logical: &Logical,
    ) -> Option<(Vec<Edit>, strict_ooxml::edit::InlinePoint)> {
        if let Some(point) = self.point(logical) {
            return Some((Vec::new(), point));
        }
        let pieces = self.pieces(&logical.paragraph);
        let mut before = 0;
        let mut index = None;
        for piece in &pieces {
            let end = before + piece.len();
            let top = piece.inline.first().copied()?;
            if end == logical.offset {
                index = Some(top + 1);
            } else if before == logical.offset && index.is_none() {
                index = Some(top);
            }
            before = end;
        }
        let index = index?;
        let props = self
            .paragraph(&logical.paragraph)?
            .inlines
            .iter()
            .find_map(|inline| match inline {
                strict_ooxml::model::Inline::Run(run) => Some(run.props.clone()),
                _ => None,
            })
            .unwrap_or_default();
        Some((
            vec![Edit::InsertInline {
                at: logical.paragraph.clone(),
                index,
                inline: Box::new(strict_ooxml::model::Inline::Run(text_run(props, ""))),
            }],
            strict_ooxml::edit::InlinePoint {
                inline: vec![index],
                content: 0,
                offset: 0,
            },
        ))
    }

    fn split(&mut self) -> bool {
        let Some(caret) = self.caret.clone() else {
            return false;
        };
        let commands = if self.pieces(&caret.paragraph).is_empty() {
            vec![Edit::Split {
                at: caret.paragraph.clone(),
                offset: 0,
            }]
        } else if let Some((mut commands, point)) = self.point_or_run(&caret) {
            commands.push(Edit::SplitAt {
                at: caret.paragraph.clone(),
                point,
            });
            commands
        } else {
            self.note("note.cannot_edit_here", String::new(), true);
            return false;
        };
        if !self.transact(&commands) {
            return false;
        }
        let mut next = caret.paragraph;
        next.block += 1;
        self.anchor = None;
        self.caret = Some(Logical {
            paragraph: next,
            offset: 0,
        });
        true
    }

    /// `(paragraph, from, to)` ranges a character format applies to: the
    /// selected cells, the selection, or the word at the caret.
    fn format_ranges(
        &self,
    ) -> Vec<(
        Address,
        strict_ooxml::edit::InlinePoint,
        strict_ooxml::edit::InlinePoint,
    )> {
        let whole = |address: &Address| {
            let pieces = self.pieces(address);
            let len: usize = pieces.iter().map(nav::Piece::len).sum();
            Some((
                address.clone(),
                nav::point_at(&pieces, 0)?,
                nav::point_at(&pieces, len)?,
            ))
        };
        if self.cells.is_some() {
            return self.cell_paragraphs().iter().filter_map(whole).collect();
        }
        let mut out = Vec::new();
        if let Some((start, end)) = self.selection() {
            let paragraphs = self.paragraphs(&start.paragraph.story);
            let from = paragraphs.iter().position(|a| *a == start.paragraph);
            let to = paragraphs.iter().position(|a| *a == end.paragraph);
            let (Some(from), Some(to)) = (from, to) else {
                return out;
            };
            for (index, address) in paragraphs.iter().enumerate().take(to + 1).skip(from) {
                let pieces = self.pieces(address);
                let len: usize = pieces.iter().map(nav::Piece::len).sum();
                let a = if index == from { start.offset } else { 0 };
                let b = if index == to { end.offset } else { len };
                if let (Some(a), Some(b)) = (nav::point_at(&pieces, a), nav::point_at(&pieces, b)) {
                    out.push((address.clone(), a, b));
                }
            }
        } else if let Some(caret) = &self.caret {
            let text = self.text_of(&caret.paragraph);
            if let Some(range) = nav::word_at(&text, caret.offset) {
                let pieces = self.pieces(&caret.paragraph);
                if let (Some(a), Some(b)) = (
                    nav::point_at(&pieces, range.start),
                    nav::point_at(&pieces, range.end),
                ) {
                    out.push((caret.paragraph.clone(), a, b));
                }
            }
        }
        out
    }

    fn format(&mut self, change: &FormatChange) -> Outcome {
        let current = self.caret_format();
        let toggle = |on: bool| Some(if on { TriState::Off } else { TriState::On });
        let mut patch = FormatPatch::default();
        match change {
            FormatChange::Bold => patch.bold = toggle(current.bold),
            FormatChange::Italic => patch.italic = toggle(current.italic),
            FormatChange::Strike => patch.strike = toggle(current.strike),
            FormatChange::Underline => {
                patch.underline = Some((!current.underline).then_some(Underline::Single));
            }
            FormatChange::Superscript => {
                patch.vert_align = Some((!current.superscript).then_some(VertAlign::Superscript));
            }
            FormatChange::Subscript => {
                patch.vert_align = Some((!current.subscript).then_some(VertAlign::Subscript));
            }
            FormatChange::Size(points) => patch.size = Some(Some(half_points(*points))),
            FormatChange::Grow | FormatChange::Shrink => {
                let size = current.size.unwrap_or(11.0);
                let next = if matches!(change, FormatChange::Grow) {
                    FONT_SIZES
                        .iter()
                        .copied()
                        .find(|s| *s > size + 0.01)
                        .unwrap_or(size + 2.0)
                } else {
                    FONT_SIZES
                        .iter()
                        .rev()
                        .copied()
                        .find(|s| *s < size - 0.01)
                        .unwrap_or((size - 1.0).max(1.0))
                };
                patch.size = Some(Some(half_points(next)));
            }
            FormatChange::Font(name) => {
                let name: Arc<str> = Arc::from(name.as_str());
                patch.fonts = Some(Some(Fonts {
                    ascii: Some(name.clone()),
                    h_ansi: Some(name.clone()),
                    complex_script: Some(name.clone()),
                    east_asia: Some(name),
                    ..Fonts::default()
                }));
            }
            FormatChange::Color(rgb) => {
                patch.color =
                    Some(rgb.map(|[r, g, b]| Color::new(format!("{r:02X}{g:02X}{b:02X}"))));
            }
            FormatChange::Highlight(highlight) => patch.highlight = Some(*highlight),
            FormatChange::Clear => {
                patch = FormatPatch {
                    bold: Some(TriState::Absent),
                    italic: Some(TriState::Absent),
                    underline: Some(None),
                    character_style: Some(None),
                    strike: Some(TriState::Absent),
                    double_strike: Some(TriState::Absent),
                    caps: Some(TriState::Absent),
                    small_caps: Some(TriState::Absent),
                    size: Some(None),
                    color: Some(None),
                    highlight: Some(None),
                    fonts: Some(None),
                    vert_align: Some(None),
                };
            }
        }
        let ranges = self.format_ranges();
        if ranges.is_empty() {
            self.note("note.select_text", String::new(), false);
            return Outcome::Quiet;
        }
        let commands: Vec<Edit> = ranges
            .into_iter()
            .map(|(at, from, to)| Edit::FormatAt {
                at,
                from,
                to,
                patch: patch.clone(),
            })
            .collect();
        if self.transact(&commands) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Paragraphs the selection touches, or the caret's.
    fn selected_paragraphs(&self) -> Vec<Address> {
        if self.cells.is_some() {
            return self.cell_paragraphs();
        }
        if let Some((start, end)) = self.selection() {
            let paragraphs = self.paragraphs(&start.paragraph.story);
            let from = paragraphs.iter().position(|a| *a == start.paragraph);
            let to = paragraphs.iter().position(|a| *a == end.paragraph);
            if let (Some(from), Some(to)) = (from, to) {
                return paragraphs
                    .get(from..=to)
                    .map(<[Address]>::to_vec)
                    .unwrap_or_default();
            }
        }
        self.caret
            .as_ref()
            .map(|caret| vec![caret.paragraph.clone()])
            .unwrap_or_default()
    }

    fn paragraph_edit(&mut self, change: impl Fn(&mut ParagraphProperties)) -> Outcome {
        let mut commands = Vec::new();
        for at in self.selected_paragraphs() {
            let Some(paragraph) = self.paragraph(&at) else {
                continue;
            };
            let mut properties = paragraph.props.clone();
            change(&mut properties);
            if properties != paragraph.props {
                commands.push(Edit::ParagraphProperties {
                    at,
                    properties: Box::new(properties),
                });
            }
        }
        if self.transact(&commands) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn align(&mut self, align: Align) -> Outcome {
        let alignment = match align {
            Align::Left => Justification::Start,
            Align::Center => Justification::Center,
            Align::Right => Justification::End,
            Align::Justify => Justification::Both,
        };
        self.paragraph_edit(|properties| properties.alignment = Some(alignment))
    }

    /// Lists the selected paragraphs in `preset`'s format, or takes them out
    /// of their list. Paragraphs already in one list change that list's
    /// format, all its items with them; others join the list of the
    /// paragraph before them when it has this format, else a new list. One
    /// undo step either way.
    fn list(&mut self, preset: Option<ListPreset>) -> Outcome {
        use strict_ooxml::edit::{
            list_definition, list_instance, list_level, unused_abstract_num_id, unused_num_id,
            Definition,
        };
        use strict_ooxml::model::props::NumPr;
        use strict_ooxml::model::{Ilvl, NumId};
        let targets = self.selected_paragraphs();
        let document = self.session.document();
        let levels: Vec<(Address, Paragraph, Option<ListLevel>)> = targets
            .iter()
            .filter_map(|at| {
                let paragraph = self.paragraph(at)?.clone();
                let level = list_level(document, &paragraph);
                Some((at.clone(), paragraph, level))
            })
            .collect();
        let Some(preset) = preset else {
            let commands: Vec<Edit> = levels
                .iter()
                .filter(|(_, _, level)| level.is_some())
                .map(|(at, paragraph, level)| {
                    let mut properties = paragraph.props.clone();
                    // A list the style gives is switched off with list 0.
                    properties.numbering =
                        level
                            .as_ref()
                            .filter(|level| level.from_style)
                            .map(|_| NumPr {
                                num_id: Some(NumId(0)),
                                ilvl: None,
                            });
                    Edit::ParagraphProperties {
                        at: at.clone(),
                        properties: Box::new(properties),
                    }
                })
                .collect();
            return if self.transact(&commands) {
                Outcome::Edited
            } else {
                Outcome::Quiet
            };
        };
        // Every paragraph already in one list: that list takes the format.
        let lists: Vec<&ListLevel> = levels.iter().filter_map(|(_, _, l)| l.as_ref()).collect();
        if let Some(first) = lists.first() {
            if lists.len() == levels.len() && lists.iter().all(|l| l.num_id == first.num_id) {
                let definition = list_definition(first.abstract_id, preset);
                return self.define_and_transact(
                    &[Definition::ReplaceAbstractNum(Box::new(definition))],
                    &[],
                );
            }
        }
        // The list of the paragraph before, if it has this format.
        let before = targets.first().and_then(|first| {
            let paragraphs = self.paragraphs(&first.story);
            let index = paragraphs.iter().position(|a| a == first)?;
            let previous = paragraphs.get(index.checked_sub(1)?)?;
            let level = list_level(document, self.paragraph(previous)?)?;
            (ListPreset::matching(&level) == Some(preset) && level.ilvl == 0)
                .then_some(level.num_id)
        });
        let mut definitions = Vec::new();
        let num_id = before.unwrap_or_else(|| {
            let abstract_id = unused_abstract_num_id(document);
            let num_id = unused_num_id(document);
            definitions.push(Definition::CreateAbstractNum(Box::new(list_definition(
                abstract_id,
                preset,
            ))));
            definitions.push(Definition::CreateNum(Box::new(list_instance(
                num_id,
                abstract_id,
            ))));
            num_id
        });
        let commands: Vec<Edit> = levels
            .iter()
            .map(|(at, paragraph, level)| {
                let mut properties = paragraph.props.clone();
                properties.numbering = Some(NumPr {
                    num_id: Some(num_id),
                    ilvl: Some(Ilvl(level.as_ref().map_or(0, |level| level.ilvl))),
                });
                // The list's indents replace the paragraph's own, as in Word.
                if level.is_none() {
                    properties.indentation = None;
                }
                Edit::ParagraphProperties {
                    at: at.clone(),
                    properties: Box::new(properties),
                }
            })
            .collect();
        self.define_and_transact(&definitions, &commands)
    }

    fn define_and_transact(
        &mut self,
        definitions: &[strict_ooxml::edit::Definition],
        commands: &[Edit],
    ) -> Outcome {
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let revision = editor.revision();
        match editor.define_and_transact(revision, definitions, commands) {
            Ok(change) => {
                if change.revision == revision {
                    return Outcome::Quiet;
                }
                self.undo_depth += 1;
                self.redo_depth = 0;
                self.stale = true;
                Outcome::Edited
            }
            Err(failure) => {
                self.note("note.edit_refused", format!("{:?}", failure.error), true);
                Outcome::Quiet
            }
        }
    }

    fn set_style(&mut self, id: &str) -> Outcome {
        let style = StyleId::new(id);
        if !self.session.document().styles.contains(&style) {
            self.note("note.no_style", id.to_owned(), true);
            return Outcome::Quiet;
        }
        self.paragraph_edit(|properties| properties.style = Some(style.clone()))
    }

    fn insert(&mut self, insert: Insert) -> Outcome {
        match insert {
            Insert::PageBreak => {
                if !self.split() {
                    return Outcome::Quiet;
                }
                let Some(caret) = self.caret.clone() else {
                    return Outcome::Edited;
                };
                let Some(paragraph) = self.paragraph(&caret.paragraph) else {
                    return Outcome::Edited;
                };
                let mut properties = paragraph.props.clone();
                properties.page_break_before = TriState::On;
                self.transact(&[Edit::ParagraphProperties {
                    at: caret.paragraph,
                    properties: Box::new(properties),
                }]);
                Outcome::Edited
            }
            Insert::Table { rows, columns } => {
                let Some(caret) = self.caret.clone() else {
                    return Outcome::Quiet;
                };
                let width = self.text_width();
                let table = new_table(rows.max(1), columns.max(1), width);
                let mut at = caret.paragraph.clone();
                at.block += 1;
                let mut after = at.clone();
                after.block += 1;
                // A paragraph after the table, so typing can continue below it.
                let commands = [
                    Edit::Insert {
                        at: at.clone(),
                        block: Box::new(Block::Table(table)),
                    },
                    Edit::Insert {
                        at: after,
                        block: Box::new(Block::Paragraph(empty_paragraph())),
                    },
                ];
                if self.transact(&commands) {
                    let mut first_cell = Address {
                        story: at.story.clone(),
                        containers: at.containers.clone(),
                        block: 0,
                    };
                    first_cell
                        .containers
                        .push(strict_ooxml::edit::Container::Cell {
                            table: at.block,
                            row: 0,
                            cell: 0,
                        });
                    self.anchor = None;
                    self.caret = Some(Logical {
                        paragraph: first_cell,
                        offset: 0,
                    });
                    Outcome::Edited
                } else {
                    Outcome::Quiet
                }
            }
            Insert::Document(path) => self.insert_document(path),
        }
    }

    /// A top-level body paragraph of the caret's section.
    fn section_anchor(&self) -> Option<Address> {
        self.caret
            .as_ref()
            .map(|caret| caret.paragraph.clone())
            .filter(|at| at.story == Story::Body && at.containers.is_empty())
            .or_else(|| self.first_paragraph())
    }

    /// The properties of the section a body block belongs to.
    fn section_of(&self, block: usize) -> SectionProperties {
        let document = self.session.document();
        for candidate in document.body.blocks.iter().skip(block) {
            if let Block::Paragraph(paragraph) = candidate {
                if let Some(section) = &paragraph.props.section {
                    return section.clone();
                }
            }
        }
        document
            .sections
            .last()
            .map(|section| section.properties.clone())
            .unwrap_or_default()
    }

    fn page_setup(&mut self, change: PageChange) -> Outcome {
        let Some(at) = self.section_anchor() else {
            return Outcome::Quiet;
        };
        if let PageChange::SectionBreak = change {
            return if self.transact(&[Edit::InsertSectionBreak { at }]) {
                Outcome::Edited
            } else {
                Outcome::Quiet
            };
        }
        let mut properties = self.section_of(at.block);
        let mut size = properties.page_size.unwrap_or(PageSize {
            width: Some(Twips(11906)),
            height: Some(Twips(16838)),
            ..PageSize::default()
        });
        let width = size.width.map_or(11906, |twips| twips.0);
        let height = size.height.map_or(16838, |twips| twips.0);
        let landscape = size.orientation == Some(PageOrientation::Landscape) || width > height;
        match change {
            PageChange::Orientation { landscape: wanted } => {
                let (short, long) = (width.min(height), width.max(height));
                let (w, h) = if wanted { (long, short) } else { (short, long) };
                size.width = Some(Twips(w));
                size.height = Some(Twips(h));
                size.orientation = Some(if wanted {
                    PageOrientation::Landscape
                } else {
                    PageOrientation::Portrait
                });
                // The margins turn with the page.
                if wanted != landscape {
                    if let Some(margins) = properties.page_margins.as_mut() {
                        let (top, right, bottom, left) =
                            (margins.top, margins.right, margins.bottom, margins.left);
                        if wanted {
                            margins.top = left;
                            margins.right = top;
                            margins.bottom = right;
                            margins.left = bottom;
                        } else {
                            margins.top = right;
                            margins.right = bottom;
                            margins.bottom = left;
                            margins.left = top;
                        }
                    }
                }
            }
            PageChange::Size {
                width: w,
                height: h,
            } => {
                let (w, h) = if landscape { (h, w) } else { (w, h) };
                size.width = Some(Twips(w));
                size.height = Some(Twips(h));
                size.code = None;
            }
            PageChange::Margins([top, right, bottom, left]) => {
                let mut margins = properties.page_margins.unwrap_or_default();
                margins.top = Some(Twips(top));
                margins.right = Some(Twips(right));
                margins.bottom = Some(Twips(bottom));
                margins.left = Some(Twips(left));
                margins.header = margins.header.or(Some(Twips(709)));
                margins.footer = margins.footer.or(Some(Twips(709)));
                properties.page_margins = Some(margins);
            }
            PageChange::SectionBreak => {}
        }
        properties.page_size = Some(size);
        if self.transact(&[Edit::SectionProperties {
            at,
            properties: Box::new(properties),
        }]) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn text_width(&self) -> i32 {
        let document = self.session.document();
        document
            .sections
            .last()
            .and_then(|section| {
                let size = section.properties.page_size.as_ref()?;
                let margins = section.properties.page_margins.as_ref()?;
                let width = size.width?.0;
                let left = margins.left.map_or(0, |twips| twips.0);
                let right = margins.right.map_or(0, |twips| twips.0);
                Some(width - left - right)
            })
            .filter(|width| *width > 1440)
            .unwrap_or(9355)
    }

    fn insert_document(&mut self, path: PathBuf) -> Outcome {
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let source = Source::File {
            path,
            import: formats::ImportOptions::default(),
        };
        let loaded = match formats::open(&source, &OpenControl::new()) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.note("note.import_failed", error, true);
                return Outcome::Quiet;
            }
        };
        let other = loaded.document;
        let mut at = caret.paragraph.clone();
        at.block += 1;
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let revision = editor.revision();
        match editor.import(
            revision,
            &at,
            other.document(),
            &other.document().body.blocks,
        ) {
            Ok(_) => {
                self.undo_depth += 1;
                self.redo_depth = 0;
                self.stale = true;
                self.note("note.imported", String::new(), false);
                Outcome::Edited
            }
            Err(error) => {
                self.note("note.import_failed", format!("{error:?}"), true);
                Outcome::Quiet
            }
        }
    }

    fn undo(&mut self, undo: bool) -> Outcome {
        let depth = if undo {
            self.undo_depth
        } else {
            self.redo_depth
        };
        if depth == 0 {
            return Outcome::Quiet;
        }
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let revision = editor.revision();
        let result = if undo {
            editor.undo(revision)
        } else {
            editor.redo(revision)
        };
        match result {
            Ok(_) => {
                if undo {
                    self.undo_depth -= 1;
                    self.redo_depth += 1;
                } else {
                    self.redo_depth -= 1;
                    self.undo_depth += 1;
                }
                self.stale = true;
                self.anchor = None;
                // The caret's paragraph may be gone; keep what still exists.
                let keep = self.caret.as_ref().and_then(|caret| {
                    let len: usize = nav::pieces(self.paragraph(&caret.paragraph)?)
                        .iter()
                        .map(nav::Piece::len)
                        .sum();
                    Some(Logical {
                        paragraph: caret.paragraph.clone(),
                        offset: caret.offset.min(len),
                    })
                });
                self.caret = keep.or_else(|| {
                    self.first_paragraph().map(|paragraph| Logical {
                        paragraph,
                        offset: 0,
                    })
                });
                Outcome::Edited
            }
            Err(error) => {
                self.note("note.edit_refused", format!("{error:?}"), true);
                Outcome::Quiet
            }
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let paragraphs = self.paragraphs(&start.paragraph.story);
        let from = paragraphs.iter().position(|a| *a == start.paragraph)?;
        let to = paragraphs.iter().position(|a| *a == end.paragraph)?;
        let mut out = String::new();
        for (index, address) in paragraphs.iter().enumerate().take(to + 1).skip(from) {
            let text = self.text_of(address);
            let len = text.chars().count();
            let a = if index == from { start.offset } else { 0 };
            let b = if index == to { end.offset } else { len };
            if index > from {
                out.push('\n');
            }
            out.push_str(&nav::slice(&text, a..b.max(a)));
        }
        Some(out)
    }

    fn copy(&mut self, cut: bool) -> Outcome {
        // A selected drawing alone.
        if let Some(object) = self.object.clone() {
            let objects: Vec<ClipObject> = self.clip_object(&object, 0, 0).into_iter().collect();
            let image = objects
                .first()
                .and_then(|o| o.bytes.as_deref())
                .and_then(crate::raster::picture_pixels_rgba);
            self.send(Reply::ClipboardObjects(Box::new(ClipObjects {
                text: String::new(),
                objects,
                image,
            })));
            if cut && self.read_only.is_none() {
                return self.delete_object();
            }
            return Outcome::Quiet;
        }
        let Some(text) = self.cells_text().or_else(|| self.selected_text()) else {
            return Outcome::Quiet;
        };
        // A picture has no text to carry; it goes along as a drawing.
        let text: String = text.chars().filter(|c| *c != nav::OBJECT).collect();
        let objects = if self.cells.is_none() {
            self.selected_clip_objects()
        } else {
            Vec::new()
        };
        self.send(Reply::Clipboard(text.clone()));
        if !objects.is_empty() {
            self.send(Reply::ClipboardObjects(Box::new(ClipObjects {
                text,
                objects,
                image: None,
            })));
        }
        if cut {
            if self.read_only.is_some() {
                return Outcome::Quiet;
            }
            if self.cells.is_some() {
                return self.clear_cells();
            }
            return self.delete_selection();
        }
        Outcome::Quiet
    }

    /// A drawing as the clipboard keeps it.
    fn clip_object(
        &self,
        object: &DrawingPosition,
        line: usize,
        offset: usize,
    ) -> Option<ClipObject> {
        let drawing = self.drawing_at(object)?;
        let bytes = picture_part(&drawing).and_then(|part| self.media_bytes(&part));
        Some(ClipObject {
            line,
            offset,
            drawing,
            bytes,
        })
    }

    /// A media part's bytes, the session's added pictures included.
    fn media_bytes(&self, part: &strict_ooxml_core::part::PartId) -> Option<Vec<u8>> {
        match &self.session {
            Session::Edit(editor) => editor.media_bytes(part).ok(),
            Session::View(document) => document.package().read_part(part).ok(),
        }
    }

    /// The drawings inside the selection, where each is in the copied text.
    fn selected_clip_objects(&self) -> Vec<ClipObject> {
        let Some((start, end)) = self.selection() else {
            return Vec::new();
        };
        let paragraphs = self.paragraphs(&start.paragraph.story);
        let from = paragraphs.iter().position(|a| *a == start.paragraph);
        let to = paragraphs.iter().position(|a| *a == end.paragraph);
        let (Some(from), Some(to)) = (from, to) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (index, paragraph) in paragraphs.iter().enumerate().take(to + 1).skip(from) {
            let line_start = if index == from { start.offset } else { 0 };
            let mut earlier = 0;
            for (offset, object) in self.objects_of(paragraph) {
                let inside =
                    (index > from || offset >= start.offset) && (index < to || offset < end.offset);
                if !inside {
                    continue;
                }
                // Offsets in the copied text, which leaves drawings out.
                let at = offset - line_start - earlier;
                earlier += 1;
                if let Some(clip) = self.clip_object(&object, index - from, at) {
                    out.push(clip);
                }
            }
        }
        out
    }

    /// Pastes the text, then puts each drawing back at its place in it.
    fn paste_objects(&mut self, clip: &ClipObjects) -> Outcome {
        if self.selection().is_some() && matches!(self.delete_selection(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        let Some(start) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let mut outcome = if clip.text.is_empty() {
            Outcome::Quiet
        } else {
            self.paste(&clip.text)
        };
        let end = self.caret.clone();
        let paragraphs = self.paragraphs(&start.paragraph.story);
        let Some(first) = paragraphs.iter().position(|a| *a == start.paragraph) else {
            return outcome;
        };
        // Last first, so earlier offsets stay where they are.
        for object in clip.objects.iter().rev() {
            let Some(paragraph) = paragraphs.get(first + object.line).cloned() else {
                continue;
            };
            let offset = if object.line == 0 {
                start.offset + object.offset
            } else {
                object.offset
            };
            if self.insert_drawing(
                &Logical { paragraph, offset },
                object.drawing.clone(),
                object.bytes.clone(),
            ) {
                outcome = Outcome::Edited;
            }
        }
        // The caret after what was pasted, the drawings counted.
        if let Some(mut end) = end {
            end.offset += clip
                .objects
                .iter()
                .filter(|o| {
                    o.line + first
                        == paragraphs
                            .iter()
                            .position(|a| *a == end.paragraph)
                            .unwrap_or(0)
                })
                .count();
            self.caret = Some(end);
        }
        self.anchor = None;
        outcome
    }

    /// Puts a drawing at a caret, its picture's bytes kept as a new media
    /// part, under a drawing id of its own.
    fn insert_drawing(
        &mut self,
        at: &Logical,
        mut drawing: strict_ooxml::model::Drawing,
        bytes: Option<Vec<u8>>,
    ) -> bool {
        use strict_ooxml::edit::{unused_drawing_id, Definition};
        let id = unused_drawing_id(self.session.document());
        let mut definitions = Vec::new();
        if let Some(bytes) = bytes {
            let Some(editor) = self.editor() else {
                return false;
            };
            let Ok(item) = editor.add_media(bytes) else {
                return false;
            };
            set_picture_part(&mut drawing, &item.part);
            definitions.push(Definition::AddMedia(Box::new(item)));
        }
        set_drawing_id(&mut drawing, id);
        let commands = self.inline_at_caret(at, strict_ooxml::model::Inline::Drawing(drawing));
        matches!(
            self.define_and_transact(&definitions, &commands),
            Outcome::Edited
        )
    }

    fn paste(&mut self, text: &str) -> Outcome {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut edited = false;
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                if !self.split() {
                    break;
                }
                edited = true;
            }
            let clean: String = line
                .chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .collect();
            if !clean.is_empty() || self.selection().is_some() {
                edited |= self.put_text(&clean);
            }
        }
        if edited {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    // ----- tables -------------------------------------------------------

    /// The table cell the caret is in: the story, the containers around the
    /// table, the table's block index, the row and the cell.
    fn in_cell(&self) -> Option<CellRef> {
        let caret = self.caret.as_ref()?;
        cell_of(&caret.paragraph)
    }

    /// Every paragraph of a table, by `(row, cell)`, in story order.
    fn table_cells(&self, table: &TableRef) -> BTreeMap<(usize, usize), Vec<Address>> {
        let mut cells: BTreeMap<(usize, usize), Vec<Address>> = BTreeMap::new();
        for address in self.paragraphs(&table.story) {
            let depth = table.parents.len();
            if address.containers.len() <= depth
                || address.containers.get(..depth) != Some(table.parents.as_slice())
            {
                continue;
            }
            if let Some(Container::Cell {
                table: index,
                row,
                cell,
            }) = address.containers.get(depth)
            {
                if *index == table.block {
                    cells.entry((*row, *cell)).or_default().push(address);
                }
            }
        }
        cells
    }

    /// The table model itself, for building rows like its own.
    fn table_model(&self, table: &TableRef) -> Option<Table> {
        let document = self.session.document();
        let mut blocks: &[Block] = match &table.story {
            Story::Body => &document.body.blocks,
            Story::HeaderFooter(part) => &document.header_footer(part)?.blocks,
            _ => return None,
        };
        for container in &table.parents {
            blocks = match container {
                Container::Cell { table, row, cell } => match blocks.get(*table)? {
                    Block::Table(t) => &t.rows.get(*row)?.cells.get(*cell)?.blocks,
                    _ => return None,
                },
                Container::Sdt(index) => match blocks.get(*index)? {
                    Block::SdtBlock(sdt) => &sdt.blocks,
                    _ => return None,
                },
                Container::TextBox { .. } => return None,
            };
        }
        match blocks.get(table.block)? {
            Block::Table(t) => Some(t.clone()),
            _ => None,
        }
    }

    /// Puts the caret in a cell, its text selected as Word does on Tab.
    fn enter_cell(&mut self, paragraphs: &[Address], select: bool) {
        let (Some(first), Some(last)) = (paragraphs.first(), paragraphs.last()) else {
            return;
        };
        let end = self.text_of(last).chars().count();
        self.cells = None;
        self.anchor = (select && (end > 0 || first != last)).then(|| Logical {
            paragraph: first.clone(),
            offset: 0,
        });
        self.caret = Some(if self.anchor.is_some() {
            Logical {
                paragraph: last.clone(),
                offset: end,
            }
        } else {
            Logical {
                paragraph: first.clone(),
                offset: 0,
            }
        });
    }

    fn next_cell(&mut self, back: bool) -> Outcome {
        let Some(here) = self.in_cell() else {
            return Outcome::Quiet;
        };
        let cells = self.table_cells(&here.table);
        let key = (here.row, here.cell);
        let target = if back {
            cells.range(..key).next_back()
        } else {
            cells
                .range((std::ops::Bound::Excluded(key), std::ops::Bound::Unbounded))
                .next()
        };
        if let Some((_, paragraphs)) = target {
            let paragraphs = paragraphs.clone();
            self.enter_cell(&paragraphs, true);
            return Outcome::Moved;
        }
        if back {
            return Outcome::Quiet;
        }
        // Tab in the last cell adds a row, as in Word.
        let rows = cells.keys().map(|(row, _)| row + 1).max().unwrap_or(0);
        if self.insert_row(&here.table, rows) {
            let mut first = here.table.address();
            first.containers.push(Container::Cell {
                table: here.table.block,
                row: rows,
                cell: 0,
            });
            first.block = 0;
            self.cells = None;
            self.anchor = None;
            self.caret = Some(Logical {
                paragraph: first,
                offset: 0,
            });
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Inserts a row shaped like its neighbour, with empty cells, at `index`.
    fn insert_row(&mut self, table: &TableRef, index: usize) -> bool {
        let Some(model) = self.table_model(table) else {
            return false;
        };
        let template = model
            .rows
            .get(index.saturating_sub(1))
            .or_else(|| model.rows.first());
        let Some(template) = template else {
            return false;
        };
        let mut row = template.clone();
        row.location = SourceLocation::unknown();
        for cell in &mut row.cells {
            let props = cell
                .blocks
                .iter()
                .find_map(|block| block.as_paragraph().map(|p| p.props.clone()))
                .unwrap_or_default();
            let mut paragraph = empty_paragraph();
            paragraph.props = props;
            paragraph.props.section = None;
            cell.blocks = vec![Block::Paragraph(paragraph)];
            cell.location = SourceLocation::unknown();
        }
        self.transact(&[Edit::InsertRow {
            at: table.address(),
            index,
            row: Box::new(row),
        }])
    }

    fn table_command(&mut self, command: TableCommand) -> Outcome {
        let here = self
            .in_cell()
            .or_else(|| self.cells.as_ref().map(CellSelection::corner));
        let Some(here) = here else {
            self.note("note.not_in_table", String::new(), false);
            return Outcome::Quiet;
        };
        let cells = self.table_cells(&here.table);
        let rows = cells.keys().map(|(row, _)| row + 1).max().unwrap_or(0);
        let columns = cells.keys().map(|(_, cell)| cell + 1).max().unwrap_or(0);
        let row_span = self.cells.as_ref().map_or((here.row, here.row), |s| s.rows);
        match command {
            TableCommand::SelectRow => self.select_cells(
                &here.table,
                (here.row, here.row),
                (0, columns.saturating_sub(1)),
            ),
            TableCommand::SelectColumn => self.select_cells(
                &here.table,
                (0, rows.saturating_sub(1)),
                (here.cell, here.cell),
            ),
            TableCommand::SelectTable => self.select_cells(
                &here.table,
                (0, rows.saturating_sub(1)),
                (0, columns.saturating_sub(1)),
            ),
            TableCommand::RowAbove | TableCommand::RowBelow => {
                let index = if command == TableCommand::RowAbove {
                    row_span.0
                } else {
                    row_span.1 + 1
                };
                if self.insert_row(&here.table, index) {
                    self.cells = None;
                    let mut first = here.table.address();
                    first.containers.push(Container::Cell {
                        table: here.table.block,
                        row: index,
                        cell: 0,
                    });
                    first.block = 0;
                    self.anchor = None;
                    self.caret = Some(Logical {
                        paragraph: first,
                        offset: 0,
                    });
                    Outcome::Edited
                } else {
                    Outcome::Quiet
                }
            }
            TableCommand::DeleteRows => {
                let (first, last) = row_span;
                let commands: Vec<Edit> = if first == 0 && last + 1 >= rows {
                    // Every row: the table goes.
                    vec![Edit::Delete {
                        at: here.table.address(),
                    }]
                } else {
                    (first..=last)
                        .rev()
                        .map(|index| Edit::DeleteRow {
                            at: here.table.address(),
                            index,
                        })
                        .collect()
                };
                if self.transact(&commands) {
                    self.cells = None;
                    self.anchor = None;
                    let fallback = here.table.address();
                    let target = if first == 0 && last + 1 >= rows {
                        fallback
                    } else {
                        let row = if first < rows - (last - first + 1) {
                            first
                        } else {
                            first.saturating_sub(1)
                        };
                        let mut at = fallback;
                        at.containers.push(Container::Cell {
                            table: here.table.block,
                            row,
                            cell: 0,
                        });
                        at.block = 0;
                        at
                    };
                    self.caret = Some(Logical {
                        paragraph: target,
                        offset: 0,
                    });
                    self.clamp_caret();
                    Outcome::Edited
                } else {
                    Outcome::Quiet
                }
            }
        }
    }

    fn select_cells(
        &mut self,
        table: &TableRef,
        rows: (usize, usize),
        cols: (usize, usize),
    ) -> Outcome {
        let rows = (rows.0.min(rows.1), rows.0.max(rows.1));
        let cols = (cols.0.min(cols.1), cols.0.max(cols.1));
        self.cells = Some(CellSelection {
            table: table.clone(),
            rows,
            cols,
        });
        self.anchor = None;
        Outcome::Moved
    }

    /// Paragraphs of the selected cells, in story order.
    fn cell_paragraphs(&self) -> Vec<Address> {
        let Some(selection) = &self.cells else {
            return Vec::new();
        };
        self.table_cells(&selection.table)
            .into_iter()
            .filter(|((row, cell), _)| selection.contains(*row, *cell))
            .flat_map(|(_, paragraphs)| paragraphs)
            .collect()
    }

    /// Leaves cell selection for a caret at the start of its first cell.
    fn collapse_cells(&mut self) {
        let first = self.cell_paragraphs().into_iter().next();
        self.cells = None;
        if let Some(paragraph) = first {
            self.anchor = None;
            self.caret = Some(Logical {
                paragraph,
                offset: 0,
            });
        }
    }

    /// Empties the selected cells, keeping the cells themselves.
    fn clear_cells(&mut self) -> Outcome {
        let mut commands = Vec::new();
        for address in self.cell_paragraphs() {
            let len = self.text_of(&address).chars().count();
            match self.paragraph_replace(&address, 0, len, "") {
                Ok(more) => commands.extend(more),
                Err(key) => {
                    self.note(key, String::new(), true);
                    return Outcome::Quiet;
                }
            }
        }
        if commands.is_empty() || self.transact(&commands) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// The selected cells' text: tabs between cells, lines between rows.
    fn cells_text(&self) -> Option<String> {
        let selection = self.cells.as_ref()?;
        let mut out = String::new();
        let mut last_row = None;
        for ((row, cell), paragraphs) in self.table_cells(&selection.table) {
            if !selection.contains(row, cell) {
                continue;
            }
            match last_row {
                Some(previous) if previous != row => out.push('\n'),
                Some(_) => out.push('\t'),
                None => {}
            }
            last_row = Some(row);
            let text: Vec<String> = paragraphs.iter().map(|p| self.text_of(p)).collect();
            out.push_str(&text.join(" "));
        }
        Some(out)
    }

    /// Rectangles of the selected cells, from the carets of their lines and
    /// the table's grid.
    fn cell_rects(&self) -> Vec<VisualRect> {
        let Some(selection) = &self.cells else {
            return Vec::new();
        };
        let Some(map) = &self.map else {
            return Vec::new();
        };
        let revision = self.session.revision();
        let cells = self.table_cells(&selection.table);
        // The caret boxes of every cell, by (row, cell).
        let mut boxes: BTreeMap<(usize, usize), VisualRect> = BTreeMap::new();
        for (key, paragraphs) in &cells {
            let mut found: Option<VisualRect> = None;
            for paragraph in paragraphs {
                let len = self.text_of(paragraph).chars().count();
                for offset in [0, len] {
                    let position = self.position(&Logical {
                        paragraph: paragraph.clone(),
                        offset,
                    });
                    for rect in map.caret(revision, &position).unwrap_or_default() {
                        found = Some(match found {
                            None => rect,
                            Some(f) if f.page == rect.page => {
                                let left = f.x.min(rect.x);
                                let top = f.y.min(rect.y);
                                let right = (f.x + f.width).max(rect.x + rect.width);
                                let bottom = (f.y + f.height).max(rect.y + rect.height);
                                VisualRect {
                                    page: f.page,
                                    x: left,
                                    y: top,
                                    width: right - left,
                                    height: bottom - top,
                                }
                            }
                            Some(f) => f,
                        });
                    }
                }
            }
            if let Some(found) = found {
                boxes.insert(*key, found);
            }
        }
        let grid: Vec<f64> = self
            .table_model(&selection.table)
            .map(|t| {
                t.grid
                    .iter()
                    .map(|col| {
                        f64::from(col.width.map_or(1440, |w| w.0)) * self.options.scale / 1440.0
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Text starts a cell margin in from the cell's edge.
        let pad = 0.075 * self.options.scale;
        let mut out = Vec::new();
        for ((row, cell), rect) in &boxes {
            if !selection.contains(*row, *cell) {
                continue;
            }
            let left = rect.x - pad;
            let right = boxes
                .get(&(*row, cell + 1))
                .filter(|next| next.page == rect.page)
                .map_or_else(
                    || left + grid.get(*cell).copied().unwrap_or(rect.width + 2.0 * pad),
                    |next| next.x - pad,
                );
            let top = boxes
                .iter()
                .filter(|((r, _), b)| r == row && b.page == rect.page)
                .map(|(_, b)| b.y)
                .fold(rect.y, f64::min)
                - 2.0;
            let bottom = boxes
                .iter()
                .filter(|((r, _), b)| *r == row + 1 && b.page == rect.page)
                .map(|(_, b)| b.y - 2.0)
                .reduce(f64::min)
                .unwrap_or_else(|| {
                    // The last row: as tall as the one above it, at least.
                    let content = boxes
                        .iter()
                        .filter(|((r, _), b)| r == row && b.page == rect.page)
                        .map(|(_, b)| b.y + b.height)
                        .fold(rect.y + rect.height, f64::max)
                        + 4.0;
                    let above = row.checked_sub(1).and_then(|previous| {
                        let top_of = |r: usize| {
                            boxes
                                .iter()
                                .filter(|((rr, _), b)| *rr == r && b.page == rect.page)
                                .map(|(_, b)| b.y)
                                .reduce(f64::min)
                        };
                        Some(top + (top_of(*row)? - top_of(previous)?))
                    });
                    above.map_or(content, |above| above.max(content))
                });
            out.push(VisualRect {
                page: rect.page,
                x: left,
                y: top,
                width: (right - left).max(1.0),
                height: (bottom - top).max(1.0),
            });
        }
        out
    }

    // ----- search -------------------------------------------------------

    fn hits(&mut self, query: &FindQuery) -> Vec<(Logical, Logical)> {
        if query.text.is_empty() {
            return Vec::new();
        }
        let text_query = TextQuery {
            text: query.text.clone(),
            case_sensitive: query.case_sensitive,
            whole_word: query.whole_word,
        };
        let hits = match &mut self.session {
            Session::Edit(editor) => {
                let revision = editor.revision();
                let operations = Operations::new(editor, OperationLimits::default());
                match operations.search(
                    revision,
                    &SearchScope::Story(Story::Body),
                    &SearchQuery::Text(text_query),
                ) {
                    Ok(result) => result.hits,
                    Err(error) => {
                        self.note("note.search_failed", format!("{error:?}"), true);
                        return Vec::new();
                    }
                }
            }
            Session::View(_) => return self.view_hits(query),
        };
        hits.into_iter()
            .filter_map(|hit| {
                let first = hit.slices.first()?;
                let last = hit.slices.last()?;
                let start = TextPosition {
                    paragraph: hit.paragraph.clone(),
                    inline: first.inline.clone(),
                    content: first.content,
                    offset: first.range.start,
                };
                let end = TextPosition {
                    paragraph: hit.paragraph.clone(),
                    inline: last.inline.clone(),
                    content: last.content,
                    offset: last.range.end,
                };
                Some((self.logical(&start), self.logical(&end)))
            })
            .collect()
    }

    /// Search without an editing session: the paragraphs' own text.
    fn view_hits(&self, query: &FindQuery) -> Vec<(Logical, Logical)> {
        let needle: Vec<char> = if query.case_sensitive {
            query.text.chars().collect()
        } else {
            query.text.to_lowercase().chars().collect()
        };
        let mut out = Vec::new();
        for address in self.paragraphs(&Story::Body) {
            let text = self.text_of(&address);
            let hay: Vec<char> = if query.case_sensitive {
                text.chars().collect()
            } else {
                text.to_lowercase().chars().collect()
            };
            if needle.is_empty() || hay.len() < needle.len() {
                continue;
            }
            for start in 0..=hay.len() - needle.len() {
                if hay.get(start..start + needle.len()) == Some(needle.as_slice()) {
                    out.push((
                        Logical {
                            paragraph: address.clone(),
                            offset: start,
                        },
                        Logical {
                            paragraph: address.clone(),
                            offset: start + needle.len(),
                        },
                    ));
                }
            }
        }
        out
    }

    fn find(&mut self, query: &FindQuery, backwards: bool) -> Outcome {
        let hits = self.hits(query);
        if hits.is_empty() {
            self.note("note.not_found", query.text.clone(), false);
            return Outcome::Quiet;
        }
        let here = self.caret.as_ref().map_or((0, 0), |caret| self.key(caret));
        let start_here = self.selection().map_or(here, |(start, _)| self.key(&start));
        let keys: Vec<(usize, usize)> = hits.iter().map(|(start, _)| self.key(start)).collect();
        let index = if backwards {
            keys.iter()
                .rposition(|key| *key < start_here)
                .unwrap_or(hits.len() - 1)
        } else {
            keys.iter().position(|key| *key > start_here).unwrap_or(0)
        };
        let Some((start, end)) = hits.get(index).cloned() else {
            return Outcome::Quiet;
        };
        self.anchor = Some(start);
        self.caret = Some(end);
        self.note("note.found", format!("{}/{}", index + 1, hits.len()), false);
        Outcome::Moved
    }

    fn replace(&mut self, query: &FindQuery, with: &str) -> Outcome {
        let selected = self.selected_text().unwrap_or_default();
        let matches = if query.case_sensitive {
            selected == query.text
        } else {
            selected.to_lowercase() == query.text.to_lowercase()
        };
        if matches && !selected.is_empty() {
            if !self.put_text(with) && !with.is_empty() {
                return Outcome::Quiet;
            }
            if with.is_empty() {
                let _ = self.delete_selection();
            }
            self.ensure_layout();
            let _ = self.find(query, false);
            return Outcome::Edited;
        }
        self.find(query, false)
    }

    fn replace_all(&mut self, query: &FindQuery, with: &str) -> Outcome {
        if query.text.is_empty() {
            return Outcome::Quiet;
        }
        let text_query = TextQuery {
            text: query.text.clone(),
            case_sensitive: query.case_sensitive,
            whole_word: query.whole_word,
        };
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let revision = editor.revision();
        let result = Operations::new(editor, OperationLimits::default()).replace_all(
            revision,
            &SearchScope::All,
            &text_query,
            with,
            ReplacePolicy::EditableOnly,
        );
        match result {
            Ok(report) => {
                if report.change.revision != revision {
                    self.undo_depth += 1;
                    self.redo_depth = 0;
                    self.stale = true;
                }
                self.anchor = None;
                self.note("note.replaced", format!("{}", report.replaced), false);
                self.clamp_caret();
                Outcome::Edited
            }
            Err(error) => {
                self.note("note.edit_refused", format!("{error:?}"), true);
                Outcome::Quiet
            }
        }
    }

    fn clamp_caret(&mut self) {
        let Some(caret) = self.caret.clone() else {
            return;
        };
        let len = self.text_of(&caret.paragraph).chars().count();
        self.caret = Some(Logical {
            offset: caret.offset.min(len),
            ..caret
        });
    }

    fn revisions(&mut self, accept: bool) -> Outcome {
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let revision = editor.revision();
        let mut operations = Operations::new(editor, OperationLimits::default());
        let result = if accept {
            operations.accept_all(revision)
        } else {
            operations.reject_all(revision)
        };
        match result {
            Ok(change) => {
                if change.revision == revision {
                    self.note("note.no_revisions", String::new(), false);
                } else {
                    self.undo_depth += 1;
                    self.redo_depth = 0;
                    self.stale = true;
                    self.note("note.revisions_done", String::new(), false);
                }
                self.anchor = None;
                self.clamp_caret();
                Outcome::Edited
            }
            Err(error) => {
                self.note("note.edit_refused", format!("{error:?}"), true);
                Outcome::Quiet
            }
        }
    }

    // ----- saving -------------------------------------------------------

    fn save(&mut self, path: PathBuf, format: SaveFormat) {
        let result = match format {
            SaveFormat::Docx => self
                .session
                .package_bytes()
                .and_then(|bytes| write_atomic(&path, &bytes)),
            SaveFormat::Pdf => self
                .pdf_bytes()
                .and_then(|bytes| write_atomic(&path, &bytes)),
            SaveFormat::Text => {
                let text = self
                    .paragraphs(&Story::Body)
                    .iter()
                    .map(|address| self.text_of(address))
                    .collect::<Vec<_>>()
                    .join("\n");
                write_atomic(&path, text.as_bytes())
            }
            SaveFormat::Svg => self.save_svg(&path),
        };
        match result {
            Ok(()) => {
                let docx = format == SaveFormat::Docx;
                if docx {
                    self.saved_revision = self.session.revision();
                    self.path = Some(path.clone());
                }
                self.send(Reply::Saved { path, format });
                if docx {
                    self.send_state();
                }
            }
            Err(error) => self.note("note.save_failed", error, true),
        }
    }

    /// The PDF of exactly what a save would write: the package is written and
    /// read back, and the PDF is rendered from that.
    fn pdf_bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = self.session.package_bytes()?;
        let reopened = StrictDocument::open_reader(&bytes[..], &OpenOptions::default())
            .map_err(|error| error.to_string())?;
        reopened
            .render_pdf(&self.options)
            .map(|output| output.bytes)
            .map_err(|error| error.to_string())
    }

    fn save_svg(&self, path: &std::path::Path) -> Result<(), String> {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("page")
            .to_owned();
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        for (index, page) in self.pages.iter().enumerate() {
            let name = if self.pages.len() == 1 {
                format!("{stem}.svg")
            } else {
                format!("{stem}-{:03}.svg", index + 1)
            };
            let svg = strict_ooxml::paint_page(&page.placed, &self.options);
            write_atomic(&dir.join(name), svg.as_bytes())?;
        }
        Ok(())
    }
}

/// A drawing's id (`wp:docPr/@id`), 0 when it has none.
fn drawing_id(drawing: &strict_ooxml::model::Drawing) -> u32 {
    use strict_ooxml::model::DrawingKind;
    let doc_pr = match &drawing.kind {
        DrawingKind::Inline(v) => v.doc_pr.as_ref(),
        DrawingKind::Anchor(v) => v.doc_pr.as_ref(),
        DrawingKind::Opaque(_) => None,
    };
    doc_pr.and_then(|doc_pr| doc_pr.id).unwrap_or(0)
}

/// How text flows around a drawing now.
fn wrap_style(drawing: &strict_ooxml::model::Drawing) -> WrapStyle {
    use strict_ooxml::model::drawing::WrapKind;
    use strict_ooxml::model::DrawingKind;
    match &drawing.kind {
        DrawingKind::Anchor(anchor) => match anchor.wrap.as_ref().map(|wrap| wrap.kind) {
            Some(WrapKind::Square) => WrapStyle::Square,
            Some(WrapKind::Tight | WrapKind::Through) => WrapStyle::Tight,
            Some(WrapKind::TopAndBottom) => WrapStyle::TopAndBottom,
            _ if anchor.behind_doc => WrapStyle::Behind,
            _ => WrapStyle::Front,
        },
        _ => WrapStyle::Inline,
    }
}

/// The drawing with text flowing around it as `style` says: in line, or
/// floating `left` EMU from its column's edge, at its paragraph's top.
fn rewrapped(
    drawing: strict_ooxml::model::Drawing,
    style: WrapStyle,
    left: i64,
) -> strict_ooxml::model::Drawing {
    use strict_ooxml::model::drawing::{AnchorDrawing, Position, Wrap, WrapKind};
    use strict_ooxml::model::{Drawing, DrawingKind, Emu, InlineDrawing};
    let location = drawing.location.clone();
    let (extent, effect_extent, doc_pr, graphic_uri, graphic, anchor) = match drawing.kind {
        DrawingKind::Inline(v) => (
            v.extent,
            v.effect_extent,
            v.doc_pr,
            v.graphic_uri,
            v.graphic,
            None,
        ),
        DrawingKind::Anchor(v) => (
            v.extent,
            v.effect_extent,
            v.doc_pr.clone(),
            v.graphic_uri.clone(),
            v.graphic.clone(),
            Some(v),
        ),
        DrawingKind::Opaque(o) => {
            return Drawing {
                kind: DrawingKind::Opaque(o),
                location,
            }
        }
    };
    if style == WrapStyle::Inline {
        return Drawing {
            kind: DrawingKind::Inline(InlineDrawing {
                extent,
                effect_extent,
                doc_pr,
                dist_top: Some(0),
                dist_bottom: Some(0),
                dist_left: Some(0),
                dist_right: Some(0),
                graphic_uri,
                graphic,
                location: location.clone(),
            }),
            location,
        };
    }
    let kind = match style {
        WrapStyle::Square => WrapKind::Square,
        WrapStyle::Tight => WrapKind::Tight,
        WrapStyle::TopAndBottom => WrapKind::TopAndBottom,
        _ => WrapKind::None,
    };
    let wrap = Wrap {
        kind,
        wrap_text: matches!(kind, WrapKind::Square | WrapKind::Tight)
            .then(|| std::sync::Arc::from("bothSides")),
        dist_left: None,
        dist_right: None,
        dist_top: None,
        dist_bottom: None,
        polygon: Vec::new(),
        polygon_edited: None,
    };
    // An anchored drawing keeps its place and only changes its wrap.
    let mut anchor = anchor.unwrap_or_else(|| AnchorDrawing {
        extent,
        effect_extent,
        doc_pr,
        simple_pos: false,
        simple_pos_point: None,
        hidden: None,
        position_h: Some(Position {
            relative_from: Some(std::sync::Arc::from("column")),
            offset: Some(Emu(left)),
            ..Position::default()
        }),
        position_v: Some(Position {
            relative_from: Some(std::sync::Arc::from("paragraph")),
            offset: Some(Emu(0)),
            ..Position::default()
        }),
        wrap: None,
        behind_doc: false,
        relative_height: Some(251_658_240),
        dist_top: Some(0),
        dist_bottom: Some(0),
        dist_left: Some(114_300),
        dist_right: Some(114_300),
        allow_overlap: true,
        layout_in_cell: true,
        locked: false,
        graphic_uri,
        graphic,
        size_rel_h: None,
        size_rel_v: None,
        location: location.clone(),
    });
    anchor.wrap = Some(wrap);
    anchor.behind_doc = style == WrapStyle::Behind;
    Drawing {
        kind: DrawingKind::Anchor(anchor),
        location,
    }
}

/// The media part a picture drawing shows.
fn picture_part(drawing: &strict_ooxml::model::Drawing) -> Option<strict_ooxml_core::part::PartId> {
    use strict_ooxml::model::{DrawingKind, Graphic};
    let graphic = match &drawing.kind {
        DrawingKind::Inline(v) => &v.graphic,
        DrawingKind::Anchor(v) => &v.graphic,
        DrawingKind::Opaque(_) => return None,
    };
    match graphic.as_ref() {
        Graphic::Picture(picture) => picture.blip.as_ref()?.resolved.clone(),
        _ => None,
    }
}

/// Points a picture drawing at another media part.
fn set_picture_part(
    drawing: &mut strict_ooxml::model::Drawing,
    part: &strict_ooxml_core::part::PartId,
) {
    use strict_ooxml::model::{DrawingKind, Graphic};
    let graphic = match &mut drawing.kind {
        DrawingKind::Inline(v) => &mut v.graphic,
        DrawingKind::Anchor(v) => &mut v.graphic,
        DrawingKind::Opaque(_) => return,
    };
    if let Graphic::Picture(picture) = graphic.as_mut() {
        if let Some(blip) = picture.blip.as_mut() {
            blip.resolved = Some(part.clone());
            blip.embed = None;
        }
    }
}

/// Gives a drawing a `wp:docPr/@id`.
fn set_drawing_id(drawing: &mut strict_ooxml::model::Drawing, id: u32) {
    use strict_ooxml::model::DrawingKind;
    let doc_pr = match &mut drawing.kind {
        DrawingKind::Inline(v) => &mut v.doc_pr,
        DrawingKind::Anchor(v) => &mut v.doc_pr,
        DrawingKind::Opaque(_) => return,
    };
    if let Some(doc_pr) = doc_pr.as_mut() {
        doc_pr.id = Some(id);
    }
}

/// How far apart, in page pixels, the caret probes the lines above or below.
const PROBE_STEP: f64 = 3.0;

/// Word-processor font sizes, for grow and shrink.
pub(crate) const FONT_SIZES: &[f32] = &[
    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0,
    72.0,
];

#[allow(clippy::cast_possible_truncation)]
fn half_points(points: f32) -> HalfPoints {
    // Clamped to Word's range (1..1638 pt), so the cast cannot overflow.
    HalfPoints((points.clamp(1.0, 1638.0) * 2.0).round() as i32)
}

fn word_left(text: &str, offset: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut index = offset.min(chars.len());
    while index > 0 && chars.get(index - 1).is_some_and(|c| !c.is_alphanumeric()) {
        index -= 1;
    }
    while index > 0 && chars.get(index - 1).is_some_and(|c| c.is_alphanumeric()) {
        index -= 1;
    }
    index
}

fn word_right(text: &str, offset: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut index = offset;
    while chars.get(index).is_some_and(|c| c.is_alphanumeric()) {
        index += 1;
    }
    while chars.get(index).is_some_and(|c| !c.is_alphanumeric()) {
        index += 1;
    }
    index.min(chars.len())
}

/// The heading level of a paragraph, from its outline level or its style.
fn heading_level(document: &Document, paragraph: &Paragraph) -> Option<u8> {
    if let Some(level) = paragraph.props.outline_level {
        return (level < 9).then_some(level);
    }
    let mut style = paragraph.props.style.clone();
    // Follow the style chain a few steps; headings are often based on headings.
    for _ in 0..4 {
        let id = style.take()?;
        let definition = document.styles.get(&id)?;
        if let Some(level) = definition.paragraph.outline_level {
            return (level < 9).then_some(level);
        }
        let name = definition
            .name
            .as_deref()
            .unwrap_or(id.as_str())
            .to_lowercase();
        if name == "title" {
            return Some(0);
        }
        for prefix in ["heading ", "heading", "заголовок "] {
            if let Some(rest) = name.strip_prefix(prefix) {
                if let Ok(level) = rest.trim().parse::<u8>() {
                    return (1..=9).contains(&level).then(|| level - 1);
                }
            }
        }
        style.clone_from(&definition.based_on);
    }
    None
}

/// A run holding `text` with `props`.
fn text_run(
    props: strict_ooxml::model::props::RunProperties,
    text: &str,
) -> strict_ooxml::model::Run {
    strict_ooxml::model::Run {
        props,
        content: vec![strict_ooxml::model::RunContent::Text(
            strict_ooxml::model::TextNode {
                text: text.to_owned(),
                space: strict_ooxml::model::Space::Preserve,
            },
        )],
        revision: None,
        location: SourceLocation::unknown(),
    }
}

fn empty_paragraph() -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: Vec::new(),
        rsids: strict_ooxml::model::values::Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::unknown(),
    }
}

fn new_table(rows: usize, columns: usize, width: i32) -> Table {
    let line = Border {
        style: Some(BorderStyle::Single),
        size: Some(EighthsPoint(4)),
        color: Some(Color::new("auto")),
        ..Border::default()
    };
    let borders = Borders {
        top: Some(line.clone()),
        bottom: Some(line.clone()),
        start: Some(line.clone()),
        end: Some(line.clone()),
        inside_horizontal: Some(line.clone()),
        inside_vertical: Some(line),
        ..Borders::default()
    };
    let columns_i32 = i32::try_from(columns).unwrap_or(1).max(1);
    let column = width / columns_i32;
    Table {
        props: TableProperties {
            borders,
            ..TableProperties::default()
        },
        grid: vec![
            GridCol {
                width: Some(Twips(column)),
            };
            columns
        ],
        grid_change: None,
        rows: (0..rows)
            .map(|_| TableRow {
                props: RowProperties::default(),
                cells: (0..columns)
                    .map(|_| TableCell {
                        props: CellProperties::default(),
                        blocks: vec![Block::Paragraph(empty_paragraph())],
                        sdt: None,
                        location: SourceLocation::unknown(),
                    })
                    .collect(),
                sdt: None,
                location: SourceLocation::unknown(),
            })
            .collect(),
        location: SourceLocation::unknown(),
    }
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".mufoed-tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        error.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Waits for the next reply the predicate accepts, skipping the others.
    fn wait<T>(replies: &Receiver<Reply>, mut pick: impl FnMut(Reply) -> Option<T>) -> T {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let reply = replies.recv_timeout(left).expect("worker reply");
            if let Some(value) = pick(reply) {
                return value;
            }
        }
    }

    fn state(replies: &Receiver<Reply>) -> EditState {
        wait(replies, |reply| match reply {
            Reply::State(state) => Some(state),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        })
    }

    /// The text of the body's paragraphs, through a copy of everything.
    fn body_text(requests: &Sender<Request>, replies: &Receiver<Reply>) -> String {
        requests.send(Request::SelectAll).unwrap();
        requests.send(Request::Copy { cut: false }).unwrap();
        let text = wait(replies, |reply| match reply {
            Reply::Clipboard(text) => Some(text),
            _ => None,
        });
        requests
            .send(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            })
            .unwrap();
        text
    }

    fn new_document() -> (Sender<Request>, Receiver<Reply>) {
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(Source::New, false, OpenControl::new(), ctx);
        let _ = state(&replies);
        (requests, replies)
    }

    /// A small picture's PNG bytes.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().expect("header");
        let pixels = vec![200u8; (width * height * 4) as usize];
        writer.write_image_data(&pixels).expect("pixels");
        drop(writer);
        bytes
    }

    fn object(replies: &Receiver<Reply>) -> Option<ObjectView> {
        wait(replies, |reply| match reply {
            Reply::Cursor(cursor) => Some(cursor.object),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        })
    }

    /// A picture goes in at the caret selected; a click on it selects it,
    /// its size changes, and Delete takes it out leaving the text.
    #[test]
    fn a_picture_is_inserted_selected_resized_and_deleted() {
        let (requests, replies) = new_document();
        for c in "ab".chars() {
            requests.send(Request::Type(c.to_string())).unwrap();
        }
        requests
            .send(Request::Move {
                motion: Motion::Left,
                extend: false,
            })
            .unwrap();
        requests
            .send(Request::Object(ObjectCommand::InsertPicture {
                bytes: png(40, 20),
                name: "dot".to_owned(),
            }))
            .unwrap();
        let inserted = loop {
            if let Some(view) = object(&replies) {
                break view;
            }
        };
        assert!(inserted.picture);
        assert_eq!((inserted.width, inserted.height), (40 * 9525, 20 * 9525));
        let rect = *inserted.rects.first().expect("drawn");
        assert!((rect.width - 40.0).abs() < 0.5, "{rect:?}");
        // Elsewhere, then back on it.
        requests
            .send(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            })
            .unwrap();
        assert!(object(&replies).is_none());
        requests
            .send(Request::Click {
                page: rect.page,
                x: rect.x + rect.width / 2.0,
                y: rect.y + rect.height / 2.0,
                extend: false,
                column: false,
            })
            .unwrap();
        assert!(object(&replies).is_some());
        requests
            .send(Request::Object(ObjectCommand::Resize {
                width: 80 * 9525,
                height: 40 * 9525,
            }))
            .unwrap();
        let resized = loop {
            if let Some(view) = object(&replies).filter(|v| v.width == 80 * 9525) {
                break view;
            }
        };
        assert!((resized.rects[0].width - 80.0).abs() < 0.5);
        requests.send(Request::Delete).unwrap();
        assert_eq!(body_text(&requests, &replies), "ab");
        requests.send(Request::Shutdown).unwrap();
    }

    fn cursor(replies: &Receiver<Reply>) -> Box<Cursor> {
        wait(replies, |reply| match reply {
            Reply::Cursor(cursor) => Some(cursor),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        })
    }

    fn picture_selected(replies: &Receiver<Reply>) -> ObjectView {
        loop {
            if let Some(view) = object(replies) {
                return view;
            }
        }
    }

    /// A picture at the end of the document: the caret goes after it, a
    /// copy of text and picture pastes both, its pixels turn, crop and
    /// change, and it floats and comes back in line.
    #[test]
    fn pictures_take_carets_copies_edits_and_wrapping() {
        let (requests, replies) = new_document();
        send_all(
            &requests,
            vec![
                Request::Type("x".to_owned()),
                Request::Object(ObjectCommand::InsertPicture {
                    bytes: png(40, 20),
                    name: "dot".to_owned(),
                }),
            ],
        );
        let view = picture_selected(&replies);
        let frame = view.rects[0];
        // End of the document: after the picture, on its right edge.
        requests
            .send(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            })
            .unwrap();
        let at_end = loop {
            let cursor = cursor(&replies);
            if cursor.object.is_none() {
                break cursor;
            }
        };
        let caret = at_end.carets.first().expect("a caret");
        assert!(
            (caret.x - (frame.x + frame.width)).abs() < 0.5,
            "{caret:?} {frame:?}"
        );
        // Text and picture copied together paste together.
        requests.send(Request::SelectAll).unwrap();
        requests.send(Request::Copy { cut: false }).unwrap();
        let clip = wait(&replies, |reply| match reply {
            Reply::ClipboardObjects(clip) => Some(clip),
            _ => None,
        });
        assert_eq!((clip.text.as_str(), clip.objects.len()), ("x", 1));
        assert_eq!((clip.objects[0].line, clip.objects[0].offset), (0, 1));
        send_all(
            &requests,
            vec![
                Request::Move {
                    motion: Motion::DocEnd,
                    extend: false,
                },
                Request::PasteObjects(clip),
                Request::SelectAll,
            ],
        );
        let all = loop {
            let cursor = cursor(&replies);
            if cursor.selection.len() >= 3 {
                break cursor;
            }
        };
        // Two pictures inside the selection are lit with it.
        let pictures = all
            .selection
            .iter()
            .filter(|r| (r.width - 40.0).abs() < 0.5 && (r.height - 20.0).abs() < 0.5)
            .count();
        assert_eq!(pictures, 2, "{:?}", all.selection);
        // The first picture: turned a quarter, its frame turns too.
        requests
            .send(Request::Click {
                page: frame.page,
                x: frame.x + 5.0,
                y: frame.y + 5.0,
                extend: false,
                column: false,
            })
            .unwrap();
        let _ = picture_selected(&replies);
        requests
            .send(Request::Object(ObjectCommand::Edit(PictureEdit::Rotate {
                clockwise: true,
            })))
            .unwrap();
        let turned = loop {
            let view = picture_selected(&replies);
            if view.height > view.width {
                break view;
            }
        };
        assert_eq!((turned.width, turned.height), (20 * 9525, 40 * 9525));
        // A crop of the bottom half keeps the top half's frame.
        requests
            .send(Request::Object(ObjectCommand::Edit(PictureEdit::Crop {
                sides: [0.0, 0.0, 0.0, 0.5],
            })))
            .unwrap();
        let cropped = loop {
            let view = picture_selected(&replies);
            if view.height == 20 * 9525 {
                break view;
            }
        };
        assert_eq!(cropped.width, 20 * 9525);
        requests
            .send(Request::Object(ObjectCommand::Edit(PictureEdit::Adjust {
                brightness: 0.2,
                contrast: 0.2,
            })))
            .unwrap();
        let _ = picture_selected(&replies);
        // Floating with text around it, then back in the line.
        let mut last = None;
        for style in [WrapStyle::Square, WrapStyle::Behind, WrapStyle::Inline] {
            requests
                .send(Request::Object(ObjectCommand::Wrap(style)))
                .unwrap();
            loop {
                let view = picture_selected(&replies);
                if view.wrap == style {
                    last = Some(view);
                    break;
                }
            }
        }
        // In the text, dragged before the "x": it is first in the line.
        let here = last.expect("in line").rects[0];
        requests
            .send(Request::Object(ObjectCommand::MoveTo {
                page: here.page,
                x: here.x - 30.0,
                y: here.y + here.height - 2.0,
            }))
            .unwrap();
        let moved = loop {
            let view = picture_selected(&replies);
            if view.rects[0].x < here.x - 1.0 {
                break view;
            }
        };
        // Floating, dragged 50 px right and 30 down.
        requests
            .send(Request::Object(ObjectCommand::Wrap(WrapStyle::Square)))
            .unwrap();
        let floating = loop {
            let view = picture_selected(&replies);
            if view.wrap == WrapStyle::Square {
                break view;
            }
        };
        let before = floating.rects[0];
        requests
            .send(Request::Object(ObjectCommand::Move { dx: 50.0, dy: 30.0 }))
            .unwrap();
        loop {
            let view = picture_selected(&replies);
            let after = view.rects[0];
            if (after.x - before.x - 50.0).abs() < 1.0 && (after.y - before.y - 30.0).abs() < 1.0 {
                break;
            }
        }
        let _ = moved;
        // Edited elsewhere: exported, then replaced by its id; the width
        // stays and the height follows the new picture.
        requests
            .send(Request::Object(ObjectCommand::Export {
                purpose: ExportPurpose::External,
            }))
            .unwrap();
        let (id, extension) = wait(&replies, |reply| match reply {
            Reply::Picture { id, extension, .. } => Some((id, extension)),
            _ => None,
        });
        assert_eq!(extension, "png");
        assert!(id > 0);
        requests
            .send(Request::Object(ObjectCommand::Replace {
                id,
                bytes: png(30, 30),
            }))
            .unwrap();
        loop {
            let view = picture_selected(&replies);
            if view.height == view.width {
                break;
            }
        }
        requests.send(Request::Shutdown).unwrap();
    }

    /// What Punto Switcher does when it corrects a word: one backspace per
    /// letter, as fast as it can, then the word again in the other layout.
    #[test]
    fn a_word_erased_key_by_key_and_retyped_comes_out_right() {
        let (requests, replies) = new_document();
        for c in "ghbdtn ".chars() {
            requests.send(Request::Type(c.to_string())).unwrap();
        }
        for _ in 0.."ghbdtn ".chars().count() {
            requests.send(Request::Backspace).unwrap();
        }
        for c in "привет ".chars() {
            requests.send(Request::Type(c.to_string())).unwrap();
        }
        assert_eq!(body_text(&requests, &replies), "привет ");
        requests.send(Request::Delete).unwrap();
        requests
            .send(Request::Move {
                motion: Motion::Left,
                extend: false,
            })
            .unwrap();
        requests
            .send(Request::Move {
                motion: Motion::Left,
                extend: false,
            })
            .unwrap();
        requests.send(Request::Delete).unwrap();
        assert_eq!(body_text(&requests, &replies), "приве ");
        requests.send(Request::Shutdown).unwrap();
    }

    /// After inserting a table the caret map must still build: without it
    /// the mouse, the arrows and the caret are gone.
    #[test]
    fn the_caret_map_survives_inserted_tables() {
        for (rows, columns) in [(1, 6), (6, 1), (4, 4), (8, 2)] {
            let ctx = eframe::egui::Context::default();
            let (requests, replies) = spawn(Source::New, false, OpenControl::new(), ctx);
            let errors = |replies: &Receiver<Reply>| -> (EditState, Vec<String>) {
                let mut errors = Vec::new();
                let state = wait(replies, |reply| match reply {
                    Reply::State(state) => Some(state),
                    Reply::Note(note) if note.error => {
                        errors.push(format!("{} {}", note.key, note.detail));
                        None
                    }
                    _ => None,
                });
                (state, errors)
            };
            let _ = errors(&replies);
            requests.send(Request::Type("До".into())).unwrap();
            let _ = errors(&replies);
            requests
                .send(Request::Insert(Insert::Table { rows, columns }))
                .unwrap();
            let (_, problems) = errors(&replies);
            assert!(problems.is_empty(), "{rows}x{columns}: {problems:?}");
            requests.send(Request::Type("ячейка".into())).unwrap();
            let (state, problems) = errors(&replies);
            assert!(problems.is_empty(), "{rows}x{columns}: {problems:?}");
            assert_eq!(state.words, 2, "{rows}x{columns}");
            requests.send(Request::Shutdown).unwrap();
        }
    }

    /// Down from a table's last row reaches the paragraph after it, as in
    /// Word; so does typing there.
    #[test]
    fn the_caret_leaves_a_table_downwards() {
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(Source::New, false, OpenControl::new(), ctx);
        let _ = state(&replies);
        requests.send(Request::Type("before".into())).unwrap();
        let _ = state(&replies);
        requests
            .send(Request::Insert(Insert::Table {
                rows: 2,
                columns: 3,
            }))
            .unwrap();
        let _ = state(&replies);
        requests.send(Request::Type("cell".into())).unwrap();
        let _ = state(&replies);
        for _ in 0..3 {
            requests
                .send(Request::Move {
                    motion: Motion::Down,
                    extend: false,
                })
                .unwrap();
        }
        requests.send(Request::Type("after".into())).unwrap();
        let _ = state(&replies);
        let text = body_text(&requests, &replies);
        assert!(text.ends_with("after"), "{text:?}");
        assert!(!text.contains("cellafter"), "{text:?}");
        requests.send(Request::Shutdown).unwrap();
    }

    fn send_all(requests: &Sender<Request>, all: Vec<Request>) {
        for request in all {
            requests.send(request).unwrap();
        }
    }

    fn typed(text: &str) -> Vec<Request> {
        text.chars().map(|c| Request::Type(c.to_string())).collect()
    }

    /// Waits until the worker has handled everything sent so far.
    fn settle(requests: &Sender<Request>, replies: &Receiver<Reply>) -> String {
        body_text(requests, replies)
    }

    #[test]
    fn tab_inserts_a_tab_that_backspace_removes() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("ab"));
        send_all(
            &requests,
            vec![Request::Move {
                motion: Motion::Left,
                extend: false,
            }],
        );
        requests.send(Request::Tab { back: false }).unwrap();
        assert_eq!(settle(&requests, &replies), "a\tb");
        // At the end of the paragraph, then typing after it.
        requests.send(Request::Tab { back: false }).unwrap();
        send_all(&requests, typed("c"));
        assert_eq!(settle(&requests, &replies), "a\tb\tc");
        // Backspace takes the c, then the tab.
        send_all(&requests, vec![Request::Backspace, Request::Backspace]);
        assert_eq!(settle(&requests, &replies), "a\tb");
        // Delete in front of the first tab.
        send_all(
            &requests,
            vec![
                Request::Move {
                    motion: Motion::Home,
                    extend: false,
                },
                Request::Move {
                    motion: Motion::Right,
                    extend: false,
                },
                Request::Delete,
            ],
        );
        assert_eq!(settle(&requests, &replies), "ab");
        // A tab at the very end, removed again.
        send_all(
            &requests,
            vec![Request::Tab { back: false }, Request::Backspace],
        );
        assert_eq!(settle(&requests, &replies), "ab");
        requests.send(Request::Shutdown).unwrap();
    }

    /// Enter beside tabs between runs, where the kernel has no point: after
    /// a tab that ends a line, and before a tab that starts one.
    #[test]
    fn enter_splits_beside_tabs() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("name"));
        send_all(
            &requests,
            vec![
                Request::Tab { back: false },
                Request::Enter,
                Request::Tab { back: false },
                Request::Tab { back: false },
            ],
        );
        send_all(&requests, typed("x"));
        assert_eq!(settle(&requests, &replies), "name\t\n\t\tx");
        send_all(
            &requests,
            vec![
                Request::Move {
                    motion: Motion::Home,
                    extend: false,
                },
                Request::Enter,
            ],
        );
        send_all(&requests, typed("y"));
        assert_eq!(settle(&requests, &replies), "name\t\n\ny\t\tx");
        requests.send(Request::Shutdown).unwrap();
    }

    #[test]
    fn a_tab_inside_a_run_of_a_loaded_document_is_removable() {
        let dir = std::env::temp_dir().join(format!("mufoed-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tab.rtf");
        std::fs::write(&path, b"{\\rtf1\\ansi one\\tab two\\par}").unwrap();
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(
            Source::File {
                path,
                import: formats::ImportOptions::default(),
            },
            false,
            OpenControl::new(),
            ctx,
        );
        let _ = state(&replies);
        assert_eq!(settle(&requests, &replies), "one\ttwo");
        send_all(
            &requests,
            vec![
                Request::Move {
                    motion: Motion::DocStart,
                    extend: false,
                },
                Request::Move {
                    motion: Motion::WordRight,
                    extend: false,
                },
            ],
        );
        // WordRight stops after the tab, at "two".
        requests.send(Request::Backspace).unwrap();
        assert_eq!(settle(&requests, &replies), "onetwo");
        requests.send(Request::Shutdown).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tab_walks_the_cells_and_adds_a_row_after_the_last() {
        let (requests, replies) = new_document();
        send_all(
            &requests,
            vec![Request::Insert(Insert::Table {
                rows: 1,
                columns: 2,
            })],
        );
        send_all(&requests, typed("a"));
        requests.send(Request::Tab { back: false }).unwrap();
        send_all(&requests, typed("b"));
        requests.send(Request::Tab { back: false }).unwrap();
        send_all(&requests, typed("c"));
        // Shift+Tab goes back and selects "b"; typing replaces it.
        send_all(
            &requests,
            vec![Request::Tab { back: true }, Request::Tab { back: true }],
        );
        send_all(&requests, typed("B"));
        let text = settle(&requests, &replies);
        assert_eq!(text, "\nB\nb\nc\n\n", "{text:?}");
        requests.send(Request::Shutdown).unwrap();
    }

    #[test]
    fn rows_and_columns_are_selected_copied_and_cleared() {
        let (requests, replies) = new_document();
        send_all(
            &requests,
            vec![Request::Insert(Insert::Table {
                rows: 2,
                columns: 2,
            })],
        );
        for text in ["a", "b", "c", "d"] {
            send_all(&requests, typed(text));
            requests.send(Request::Tab { back: false }).unwrap();
        }
        // Tab after the last cell added a third row; go back to "a".
        send_all(
            &requests,
            vec![Request::Move {
                motion: Motion::DocStart,
                extend: false,
            }],
        );
        send_all(
            &requests,
            vec![
                Request::Move {
                    motion: Motion::Down,
                    extend: false,
                },
                Request::Table(TableCommand::SelectColumn),
                Request::Copy { cut: false },
            ],
        );
        let column = wait(&replies, |reply| match reply {
            Reply::Clipboard(text) => Some(text),
            _ => None,
        });
        assert_eq!(column, "a\nc\n");
        send_all(
            &requests,
            vec![
                Request::Table(TableCommand::SelectRow),
                Request::Copy { cut: false },
            ],
        );
        let row = wait(&replies, |reply| match reply {
            Reply::Clipboard(text) => Some(text),
            _ => None,
        });
        assert_eq!(row, "a\tb");
        requests.send(Request::Backspace).unwrap();
        let text = settle(&requests, &replies);
        assert_eq!(text, "\n\n\nc\nd\n\n\n", "{text:?}");
        send_all(
            &requests,
            vec![
                Request::Move {
                    motion: Motion::DocStart,
                    extend: false,
                },
                Request::Move {
                    motion: Motion::Down,
                    extend: false,
                },
                Request::Table(TableCommand::SelectRow),
                Request::Table(TableCommand::DeleteRows),
            ],
        );
        let text = settle(&requests, &replies);
        assert!(text.starts_with("\nc\nd"), "{text:?}");
        requests.send(Request::Shutdown).unwrap();
    }

    #[test]
    fn a_new_document_is_typed_formatted_undone_and_saved() {
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(Source::New, false, OpenControl::new(), ctx);
        wait(&replies, |reply| match reply {
            Reply::Loaded(info) => Some(info),
            Reply::Failed(error) => panic!("{error}"),
            _ => None,
        });
        let initial = state(&replies);
        assert!(!initial.modified && !initial.can_undo);

        requests.send(Request::Type("Привет мир".into())).unwrap();
        assert_eq!(state(&replies).words, 2);
        requests.send(Request::Enter).unwrap();
        assert_eq!(state(&replies).words, 2);
        requests
            .send(Request::Type("Вторая строка".into()))
            .unwrap();
        let typed = state(&replies);
        assert!(typed.modified && typed.can_undo);
        assert_eq!(typed.words, 4);

        // Select "Вторая" with Shift+Home, Shift+WordRight would also do.
        requests
            .send(Request::Move {
                motion: Motion::Home,
                extend: true,
            })
            .unwrap();
        requests.send(Request::Format(FormatChange::Bold)).unwrap();
        let _ = state(&replies);
        requests.send(Request::Copy { cut: false }).unwrap();
        let copied = wait(&replies, |reply| match reply {
            Reply::Clipboard(text) => Some(text),
            _ => None,
        });
        assert_eq!(copied, "Вторая строка");

        requests.send(Request::Backspace).unwrap();
        assert_eq!(state(&replies).words, 2);
        requests.send(Request::Undo).unwrap();
        assert_eq!(state(&replies).words, 4);

        let dir = std::env::temp_dir().join(format!("mufoed-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("typed.docx");
        requests
            .send(Request::Save {
                path: path.clone(),
                format: SaveFormat::Docx,
            })
            .unwrap();
        wait(&replies, |reply| match reply {
            Reply::Saved { .. } => Some(()),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        });
        assert!(!state(&replies).modified);
        requests
            .send(Request::Save {
                path: dir.join("typed.pdf"),
                format: SaveFormat::Pdf,
            })
            .unwrap();
        wait(&replies, |reply| match reply {
            Reply::Saved { .. } => Some(()),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        });
        requests.send(Request::Shutdown).unwrap();

        let reopened = StrictDocument::open_path(&path, &OpenOptions::default()).unwrap();
        let texts: Vec<String> = reopened
            .document()
            .body
            .blocks
            .iter()
            .filter_map(Block::as_paragraph)
            .map(|paragraph| {
                nav::pieces(paragraph)
                    .iter()
                    .map(|node| node.text.as_str())
                    .collect()
            })
            .collect();
        assert_eq!(texts, ["Привет мир", "Вторая строка"]);
        let second = reopened.document().body.blocks[1].as_paragraph().unwrap();
        assert!(second.inlines.iter().any(|inline| matches!(
            inline,
            strict_ooxml::model::Inline::Run(run) if run.props.bold == TriState::On
        )));
        assert!(std::fs::read(dir.join("typed.pdf"))
            .unwrap()
            .starts_with(b"%PDF"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn word_jumps_skip_spaces_and_punctuation() {
        let text = "one, two  three";
        assert_eq!(word_right(text, 0), 5);
        assert_eq!(word_right(text, 5), 10);
        assert_eq!(word_left(text, 10), 5);
        assert_eq!(word_left(text, 5), 0);
        assert_eq!(word_right(text, 10), 15);
    }

    #[test]
    fn half_points_round_and_clamp() {
        assert_eq!(half_points(10.5).0, 21);
        assert_eq!(half_points(0.0).0, 2);
        assert_eq!(half_points(5000.0).0, 3276);
    }
}
