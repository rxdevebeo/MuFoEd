//! One document, one thread.
//!
//! The worker owns the opened `StrictDocument` and the editing session over
//! it, with its undo history, the visual map that turns page coordinates into
//! text positions, the caret and the selection. The interface sends requests
//! and draws what comes back: pages as SVG, caret and selection rectangles in
//! page pixels, the state of the ribbon's toggles. Layout, editing and saving
//! never run on the interface thread. Spelling runs on a thread of its own
//! and never ahead of a waiting key.
//!
//! The caret is kept as a paragraph address and a scalar offset into the
//! paragraph's text ([`Logical`]) rather than as a kernel `TextPosition`: an
//! edit that merges or splits runs moves the kernel position, the paragraph
//! offset stays where the user put it.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use strict_ooxml::edit::{
    Address, Container, Edit, EditError, EditLimits, FormatPatch, Invariant, OperationError,
    OperationLimits, Operations, ReplacePolicy, SavePolicy, SearchQuery, SearchScope, Story,
    TextPosition, TextQuery, Unsupported, VisualError, VisualMap, VisualRect,
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
mod chat_apply;
pub(crate) mod chem;
pub(crate) mod navigation;
mod rich_paste;
pub(crate) mod scheme;

/// A recognition result tied to the original drawing and document revision.
#[derive(Clone, Debug)]
pub(crate) struct FormulaRecognition {
    revision: u64,
    object: DrawingPosition,
    drawing: strict_ooxml::model::Drawing,
    pub(crate) mathml: String,
}

/// What the interface asks of a document.
#[derive(Debug)]
pub(crate) enum Request {
    FollowLink(navigation::Target),
    SchemeOfObject,
    PutScheme(Box<scheme::Plan>),
    ChemistryOfObject,
    PutChemistry(Box<chem::Plan>),
    PasteRich {
        content: crate::clipboard::Rich,
        fallback: String,
    },
    ConfirmFormulaRecognition(Box<FormulaRecognition>),
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
    ParagraphFormat,
    SetParagraphFormat(Box<crate::paragraph::Patch>),
    ContextClick {
        page: usize,
        x: f64,
        y: f64,
    },
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
    /// Headers, footers and page numbers.
    HeaderFooter(HeaderFooterCommand),
    /// Footnotes, bookmarks, the table of contents.
    References(ReferenceCommand),
    /// Comments.
    Comments(CommentCommand),
    /// Indent the selected paragraphs one step more (or, `false`, less): a
    /// list item a level deeper or shallower.
    IndentStep(bool),
    /// Indents of the selected paragraphs, in twips: left, right and the
    /// first line's (negative: hanging).
    Indent([i32; 3]),
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
    /// The speller to mark words with; `None` turns checking off.
    Speller(Option<Arc<crate::spell::Speller>>),
    /// The words again, after the user's dictionary changed.
    Respell,
    /// The text the AI tab works on: the selection's, else (or with
    /// `whole`) every body paragraph's. `job` comes back with it.
    AiSource {
        whole: bool,
        job: u64,
    },
    CleanupSource {
        job: u64,
    },
    ApplyCleanup(Box<crate::cleanup::Plan>),
    /// A paragraph's text replaced, if it still reads `old`.
    SetParagraphText {
        at: Address,
        old: String,
        new: String,
    },
    /// A stretch of text replaced (new lines making paragraphs), if it
    /// still reads `old`.
    ReplaceRange {
        start: Logical,
        end: Logical,
        old: String,
        new: String,
    },
    /// The formula beside the caret, to edit: answered with
    /// [`Reply::Formula`].
    FormulaAtCaret,
    /// The selected chart's data, to edit: answered with [`Reply::Chart`].
    ChartOfObject,
    /// A chart from `data`: in place of the selected chart's, else new at
    /// the caret.
    PutChart(Box<strict_ooxml::model::chart::ChartData>),
    /// A formula put at the caret, or in place of the one at
    /// (paragraph, inline index).
    PutFormula {
        expression: Box<strict_ooxml::model::math::MathExpression>,
        replace: Option<(Address, usize)>,
    },
    /// Paragraph styles by kind, the styles made when the document lacks
    /// them.
    ApplyKinds(Vec<(Address, crate::ai::Kind)>),
    /// The document as the chat's tools read it. The caret stays put.
    ChatSnapshot {
        /// Comes back with the picture.
        job: u64,
    },
    /// Every occurrence of `query`. The caret stays put and the text is unchanged.
    /// Tests drive the search this way; the chat tool searches its snapshot itself.
    #[cfg_attr(not(test), allow(dead_code))]
    ChatFind {
        /// The text to find.
        query: String,
    },
    /// One chat proposal, applied as a single undo step.
    ApplyChat(Box<crate::chat::Batch>),
    /// A misspelt word replaced, if it is still there.
    ReplaceWord {
        start: Logical,
        end: Logical,
        word: String,
        with: String,
    },
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
    /// Its text translated and written back in.
    Translate,
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
    /// An underline of this style, or none.
    UnderlineStyle(Option<Underline>),
    /// No strike, a single or a double one.
    StrikeStyle(u8),
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
    /// A new line in the same paragraph (Shift+Enter).
    LineBreak,
    /// The text after the caret starts the next column.
    ColumnBreak,
    Table {
        rows: usize,
        columns: usize,
    },
    /// Another document's body, through the editor's import.
    Document(PathBuf),
}

/// Page setup of the caret's section.
#[derive(Clone, Debug)]
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
    /// Text columns: how many, the space between them, a line between;
    /// `widths` for columns of their own widths (twips), else equal.
    Columns {
        count: u16,
        space: i32,
        separator: bool,
        widths: Option<Vec<i32>>,
    },
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
    /// The underline's style, when underlined.
    pub(crate) underline_style: Option<Underline>,
    pub(crate) double_strike: bool,
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
    /// What the rulers show for the caret.
    pub(crate) ruler: Option<RulerInfo>,
    /// The header or footer being edited, when one is.
    pub(crate) header_footer: Option<HeaderFooterView>,
    pub(crate) carets: Vec<VisualRect>,
    pub(crate) selection: Vec<VisualRect>,
    /// Bring the caret into view.
    pub(crate) scroll: bool,
    pub(crate) at_top: bool,
    pub(crate) format: CaretFormat,
}

/// What a copy carried besides its text: its drawings and, for a lone
/// picture, the picture itself for other programs.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClipObjects {
    /// The copied text, drawings left out.
    pub(crate) text: String,
    pub(crate) objects: Vec<ClipObject>,
    pub(crate) formula: Option<strict_ooxml::model::Inline>,
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

/// What can be done with comments.
#[derive(Clone, Debug)]
pub(crate) enum CommentCommand {
    /// A comment on the selection (or the word at the caret), empty, by
    /// this author.
    Add {
        author: String,
        initials: String,
    },
    /// The comment's text.
    Edit {
        id: String,
        text: String,
    },
    Delete(String),
    /// Select the text the comment is about.
    Select(String),
    /// The next (or previous) comment's text selected.
    Step {
        back: bool,
    },
}

/// The text an AI job works on.
#[derive(Clone, Debug, Default)]
pub(crate) struct AiSource {
    pub(crate) cleanup: Option<crate::cleanup::Snapshot>,
    /// The paragraphs, each with its text.
    pub(crate) paragraphs: Vec<(Address, String)>,
    /// The selection, when the job is about it: its ends and its text.
    pub(crate) selection: Option<(Logical, Logical, String)>,
}

/// A word the speller does not know, and where it is drawn.
#[derive(Clone, Debug)]
pub(crate) struct SpellError {
    pub(crate) word: String,
    pub(crate) start: Logical,
    pub(crate) end: Logical,
    pub(crate) rects: Vec<VisualRect>,
}

/// A paragraph's text checked: its wrong words and the scripts unchecked.
type Checked = Arc<(Vec<crate::spell::Word>, Vec<crate::spell::Script>)>;

/// Shared with the spelling thread: which pass may publish, and the texts
/// already checked.
struct SpellSync {
    epoch: u64,
    cache: HashMap<String, Checked>,
}

/// One body paragraph copied for the spelling thread.
struct SpellSource {
    address: Address,
    pieces: Vec<nav::Piece>,
    text: String,
}

struct SpellDone {
    errors: Vec<SpellError>,
    /// Language codes whose dictionary the text asks for.
    missing: Vec<&'static str>,
    cache: HashMap<String, Checked>,
}

fn spell_gate(sync: &Mutex<SpellSync>) -> std::sync::MutexGuard<'_, SpellSync> {
    sync.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Writes a finished pass's verdicts only when it is still the current pass.
/// A dictionary loaded since the pass started has moved the epoch, and its
/// verdicts must not replace the new check.
fn publish_spelling(gate: &mut SpellSync, epoch: u64, cache: HashMap<String, Checked>) -> bool {
    if gate.epoch != epoch {
        return false;
    }
    gate.cache = cache;
    true
}

/// `None` when a newer pass has taken the epoch.
fn spell_document(
    speller: &crate::spell::Speller,
    paragraphs: &[SpellSource],
    map: Option<&VisualMap>,
    revision: u64,
    cache: &HashMap<String, Checked>,
    sync: &Mutex<SpellSync>,
    epoch: u64,
) -> Option<SpellDone> {
    let mut errors = Vec::new();
    let mut unchecked: Vec<crate::spell::Script> = Vec::new();
    let mut seen: HashMap<String, Checked> = HashMap::new();
    for source in paragraphs {
        if spell_gate(sync).epoch != epoch {
            return None;
        }
        let cached = cache
            .get(&source.text)
            .or_else(|| seen.get(&source.text))
            .cloned();
        let checked = cached.unwrap_or_else(|| verdicts(speller, &source.text));
        seen.insert(source.text.clone(), Arc::clone(&checked));
        for script in &checked.1 {
            if !unchecked.contains(script) {
                unchecked.push(*script);
            }
        }
        for word in &checked.0 {
            if spell_gate(sync).epoch != epoch {
                return None;
            }
            let start = Logical {
                paragraph: source.address.clone(),
                offset: word.start,
            };
            let end = Logical {
                paragraph: source.address.clone(),
                offset: word.end,
            };
            let rects = map
                .and_then(|map| {
                    map.selection_between(
                        revision,
                        &text_position(&source.pieces, &start),
                        &text_position(&source.pieces, &end),
                    )
                    .ok()
                })
                .unwrap_or_default();
            errors.push(SpellError {
                word: word.text.clone(),
                start,
                end,
                rects,
            });
        }
    }
    let mut sample = Vec::new();
    for source in paragraphs {
        if sample.len() >= 400 {
            break;
        }
        for word in crate::spell::words(&source.text) {
            sample.push(word.text);
            if sample.len() >= 400 {
                break;
            }
        }
    }
    let mut missing = Vec::new();
    let words: Vec<&str> = sample.iter().map(String::as_str).collect();
    let detected = crate::spell::detect_language(words.iter().copied())
        .or_else(|| crate::spell::language_hint(words.iter().copied()));
    if let Some(code) = detected {
        missing.push(code);
    }
    let covered = detected
        .and_then(crate::spell::language)
        .map(|language| language.script);
    for script in unchecked {
        if covered == Some(script) {
            continue;
        }
        let code = crate::spell::language_for(script);
        if !missing.contains(&code) {
            missing.push(code);
        }
    }
    Some(SpellDone {
        errors,
        missing,
        cache: seen,
    })
}

fn verdicts(speller: &crate::spell::Speller, text: &str) -> Checked {
    let mut wrong = Vec::new();
    let mut unknown = Vec::new();
    for word in crate::spell::words(text) {
        match speller.verdict(&word.text) {
            crate::spell::Verdict::Right => {}
            crate::spell::Verdict::Wrong => wrong.push(word),
            crate::spell::Verdict::Unknown(script) => {
                if !unknown.contains(&script) {
                    unknown.push(script);
                }
            }
        }
    }
    Arc::new((wrong, unknown))
}

/// The text position the visual map knows a caret by.
fn text_position(pieces: &[nav::Piece], logical: &Logical) -> TextPosition {
    if pieces.iter().all(|piece| piece.text.is_empty()) {
        return TextPosition {
            paragraph: logical.paragraph.clone(),
            inline: vec![],
            content: 0,
            offset: 0,
        };
    }
    let mut before = 0;
    let mut fallback = None;
    for piece in pieces {
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
    for piece in pieces {
        let end = before + piece.len();
        if (piece.text == "\t" || piece.text.starts_with(nav::OBJECT))
            && (end == logical.offset || before == logical.offset)
        {
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

/// A comment as the pane shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommentView {
    pub(crate) id: String,
    pub(crate) author: String,
    pub(crate) date: String,
    pub(crate) text: String,
    /// The page its text is on, and where on it, for the order of the pane.
    pub(crate) page: usize,
    pub(crate) y: f64,
    /// Where the text it is about is drawn.
    pub(crate) rects: Vec<VisualRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextObject {
    Scheme,
    Chemistry,
    Picture,
    Chart,
    Formula,
    Shape,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextState {
    pub editable: bool,
    pub selection: bool,
    pub table: bool,
    pub object: Option<ContextObject>,
    pub comment: Option<String>,
}

/// Footnotes, bookmarks and the table of contents.
#[derive(Clone, Debug)]
pub(crate) enum ReferenceCommand {
    /// A footnote (or endnote) at the caret, the caret into its text.
    InsertNote {
        endnote: bool,
    },
    /// The caret to the next (or previous) note reference.
    NextNote {
        back: bool,
    },
    /// A bookmark around the selection (or at the caret).
    AddBookmark(String),
    GoToBookmark(String),
    DeleteBookmark(String),
    /// A table of contents of the headings, before the caret's paragraph,
    /// titled `title`; or, when there is one, made again.
    TableOfContents {
        title: String,
    },
    RemoveTableOfContents,
}

/// A bookmark's name, the paragraph and inline index of its start, and of
/// its end when it has one.
type BookmarkMarks = (String, Address, usize, Option<(Address, usize)>);

/// What can be done with headers and footers.
#[derive(Clone, Debug)]
pub(crate) enum HeaderFooterCommand {
    /// Edit the header (or footer) of the page's section, made if missing.
    Enter { page: usize, header: bool },
    /// Back to the body.
    Exit,
    /// Page numbers in the header (`top`) or footer, aligned; with
    /// `of_total`, the words around "X of Y".
    Insert {
        top: bool,
        align: Align,
        of_total: Option<(String, String)>,
    },
    /// Take the page numbers out of every header and footer.
    Remove,
    /// A header and footer of its own for the first page of the section.
    FirstPage(bool),
    /// Different headers and footers for even and odd pages.
    EvenOdd(bool),
    /// The first page number of the caret's section; `None` continues.
    Start(Option<u32>),
}

/// The header or footer being edited.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeaderFooterView {
    pub(crate) header: bool,
    /// Which pages it is for.
    pub(crate) kind: strict_ooxml::model::props::HeaderFooterKind,
    /// The section has a first page header of its own.
    pub(crate) first_page: bool,
    /// The document has even page headers.
    pub(crate) even_odd: bool,
    /// The body's band on each page (top, bottom, in page pixels), dimmed
    /// while a header or footer is edited.
    pub(crate) body: Vec<(f64, f64)>,
}

/// The caret's page and paragraph, as the rulers show them; twips.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RulerInfo {
    /// The page the caret is on.
    pub(crate) page: usize,
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// Top, right, bottom, left.
    pub(crate) margins: [i32; 4],
    /// The paragraph's left, right and first-line indents (negative:
    /// hanging), for a body paragraph outside tables.
    pub(crate) indent: Option<[i32; 3]>,
    /// The text columns: each one's left and right edge, twips from the
    /// page's left edge.
    pub(crate) columns: Vec<(i32, i32)>,
    /// The column the caret is in, which the indents count from.
    pub(crate) column: usize,
    /// The columns have widths of their own (not equal).
    pub(crate) unequal: bool,
    /// The space between columns, for equal ones.
    pub(crate) column_space: i32,
    pub(crate) separator: bool,
}

/// A selected drawing, as the canvas and the ribbon show it.
#[derive(Clone, Debug)]
pub(crate) struct ObjectView {
    pub(crate) scheme: bool,
    pub(crate) chemistry: bool,
    /// Where it is drawn (more than one rect for a header's repeats).
    pub(crate) rects: Vec<VisualRect>,
    /// Its size, in EMU.
    pub(crate) width: i64,
    pub(crate) height: i64,
    /// A picture (not a shape or a chart).
    pub(crate) picture: bool,
    /// A chart.
    pub(crate) chart: bool,
    pub(crate) formula: bool,
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
    /// The document's bookmarks, by name (hidden ones left out).
    pub(crate) bookmarks: Vec<String>,
    /// It has a table of contents.
    pub(crate) table_of_contents: bool,
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
    /// The fonts the document names (its font table).
    pub(crate) fonts: Vec<String>,
}

/// A note for the status bar: a localization key and a detail.
pub(crate) struct Note {
    pub(crate) key: &'static str,
    pub(crate) detail: String,
    pub(crate) error: bool,
}

/// What a document tells the interface.
pub(crate) enum Reply {
    Links(Vec<navigation::Link>),
    OpenLink(String),
    NavigateTo(VisualRect),
    Scheme(Box<scheme::Plan>),
    Chemistry(Box<chem::Plan>),
    FormulaRecognition(Box<FormulaRecognition>),
    CleanupResult(bool),
    Context(ContextState),
    CommentFocused(String),
    ParagraphFormat(Box<crate::paragraph::Format>),
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
    /// The comments, in the order of the text.
    Comments(Vec<CommentView>),
    /// The text an AI job asked for.
    AiSource {
        job: u64,
        source: Box<AiSource>,
    },
    /// The document as the chat's tools read it.
    ChatSnapshot {
        /// The id the request carried.
        job: u64,
        /// Paragraphs, formulas, tables and the caret.
        view: Box<crate::chat::View>,
    },
    /// Where `query` occurs. The caret was not moved.
    /// Only tests read it; the open document searches through its own snapshot.
    #[cfg_attr(not(test), allow(dead_code))]
    ChatFind {
        /// One entry per occurrence.
        hits: Vec<crate::chat::Hit>,
        /// Caret block index and scalar offset, as they were.
        caret: Option<(usize, usize)>,
    },
    /// The selected chart's data, or none (a new chart).
    Chart(Option<Box<strict_ooxml::model::chart::ChartData>>),
    /// The formula beside the caret (where it is and what it says), or none.
    Formula(Option<Box<(Address, usize, strict_ooxml::model::math::MathExpression)>>),
    /// Font packs that would draw text nothing draws now (their ids).
    FontsMissing(Vec<&'static str>),
    /// The misspelt words, and dictionary languages the text asks to fetch.
    Spelling {
        errors: Vec<SpellError>,
        missing: Vec<&'static str>,
    },
    /// A comment was added: the pane edits its text.
    CommentAdded(String),
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
    let (font_assets, missing_fonts) = installed_fonts(document.document(), system_fonts);
    if !missing_fonts.is_empty() {
        send(Reply::FontsMissing(missing_fonts));
    }
    let options = RenderOptions {
        font_assets,
        ..RenderOptions::default()
    };
    let path = match source {
        Source::File { path, .. }
            if !loaded.kind.converted() && !matches!(loaded.kind, Kind::Rtf | Kind::Latex) =>
        {
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
        fonts: document
            .document()
            .font_table
            .as_ref()
            .map(|table| {
                table
                    .fonts
                    .iter()
                    .map(|font| font.name.to_string())
                    .collect()
            })
            .unwrap_or_default(),
    };
    let read_only = match document.edit(EditLimits::default()) {
        Ok(mut editor) => {
            // Room for whole-document changes of a long document (a new
            // header, a list definition): each keeps two copies of it.
            editor.set_history_byte_limit(768 * 1024 * 1024);
            let defects = editor
                .source_defects()
                .iter()
                .map(ToString::to_string)
                .collect();
            send(Reply::Loaded(Box::new(DocInfo { defects, ..info })));
            let mut worker = Worker::new(Session::Edit(editor), options, replies, ctx, path);
            worker.system_fonts = system_fonts;
            worker.serve(requests);
            return;
        }
        Err(error) => {
            send(Reply::Loaded(Box::new(info)));
            stored_notice(&error)
        }
    };
    let mut worker = Worker::new(Session::View(&document), options, replies, ctx, path);
    worker.system_fonts = system_fonts;
    worker.read_only = Some(read_only);
    worker.serve(requests);
}

/// A locale key and, when the sentence has a `{detail}` hole, the text that fills it.
fn edit_notice(error: &EditError) -> (&'static str, String) {
    match error {
        EditError::UnsupportedContent(kind) => (unsupported_notice(*kind), String::new()),
        EditError::StaleRevision => ("note.stale_edit", String::new()),
        EditError::InvalidParagraph => ("note.invalid_paragraph", String::new()),
        EditError::InvalidRange => ("note.invalid_range", String::new()),
        EditError::InvalidText => ("note.invalid_text", String::new()),
        EditError::UnknownStyle(id) => ("note.unknown_style", id.as_str().to_owned()),
        EditError::InvalidModel(invariant) => (model_notice(*invariant), String::new()),
        EditError::LimitExceeded => ("note.edit_limit", String::new()),
        EditError::EmptyHistory => ("note.empty_history", String::new()),
        _ => ("note.edit_refused", error.to_string()),
    }
}

fn unsupported_notice(kind: Unsupported) -> &'static str {
    match kind {
        Unsupported::FieldResult => "note.field_text",
        Unsupported::TrackedChange => "note.tracked_text",
        Unsupported::RunContent => "note.mixed_run",
        Unsupported::Inline => "note.inline_object",
        Unsupported::OpaqueDrawing => "note.opaque_drawing",
        Unsupported::Target => "note.wrong_target",
        Unsupported::Indivisible => "note.indivisible",
        Unsupported::Import => "note.import_failed",
        _ => "note.unsupported",
    }
}

fn model_notice(invariant: Invariant) -> &'static str {
    match invariant {
        Invariant::DuplicateId => "note.model.duplicate_id",
        Invariant::TextIdWithoutParaId => "note.model.text_id",
        Invariant::Sections => "note.model.sections",
        Invariant::SectionBoundary => "note.model.section_boundary",
        Invariant::Bookmark => "note.model.bookmark",
        Invariant::Comment => "note.model.comment",
        Invariant::Field => "note.model.field",
        Invariant::TableTopology => "note.model.table",
        Invariant::Numbering => "note.model.numbering",
        Invariant::StyleChain => "note.model.style_chain",
        Invariant::HeaderFooterReference => "note.model.header",
        Invariant::Frame => "note.model.frame",
        Invariant::Grid => "note.model.grid",
        Invariant::NoteReference => "note.model.note_ref",
        Invariant::Drawing => "note.model.drawing",
        Invariant::DefinitionExists => "note.model.definition_exists",
        Invariant::DefinitionMissing => "note.model.definition_missing",
        Invariant::DefinitionInUse => "note.model.definition_in_use",
        _ => "note.model.other",
    }
}

/// The read-only reason stored on the tab: a locale key, and a detail after a newline.
fn stored_notice(error: &EditError) -> String {
    let (key, detail) = edit_notice(error);
    if detail.is_empty() {
        key.to_owned()
    } else {
        format!("{key}\n{detail}")
    }
}

/// Includes direct and style formatting added after the font table was read.
fn used_font_families(document: &Document) -> Vec<String> {
    fn blocks(list: &[Block], add: &mut impl FnMut(&strict_ooxml::model::props::RunProperties)) {
        for block in list {
            match block {
                Block::Paragraph(p) => {
                    if let Some(props) = &p.props.run_props {
                        add(props);
                    }
                    for piece in nav::pieces(p) {
                        if let Some(run) = nav::run_at(p, &piece.inline) {
                            add(&run.props);
                        }
                    }
                }
                Block::Table(t) => {
                    for c in t.rows.iter().flat_map(|r| &r.cells) {
                        blocks(&c.blocks, add);
                    }
                }
                Block::SdtBlock(s) => blocks(&s.blocks, add),
                _ => {}
            }
        }
    }
    let mut families = strict_ooxml::system_fonts::document_families(document);
    let mut add = |props: &strict_ooxml::model::props::RunProperties| {
        if let Some(fonts) = &props.fonts {
            for name in [
                &fonts.ascii,
                &fonts.h_ansi,
                &fonts.east_asia,
                &fonts.complex_script,
            ]
            .into_iter()
            .flatten()
            {
                if !families
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(name))
                {
                    families.push(name.to_string());
                }
            }
        }
    };
    if let Some(defaults) = document.styles.defaults() {
        add(&defaults.run);
    }
    for style in document.styles.iter() {
        add(&style.run);
    }
    blocks(&document.body.blocks, &mut add);
    for h in &document.headers_footers {
        blocks(&h.blocks, &mut add);
    }
    for n in document.footnotes.iter().chain(document.endnotes.iter()) {
        blocks(&n.blocks, &mut add);
    }
    families.sort_unstable();
    families
}

/// Named system faces when enabled, plus script fallbacks and downloaded packs.
pub(crate) fn installed_fonts(
    document: &Document,
    named: bool,
) -> (Option<Arc<strict_ooxml::FontAssets>>, Vec<&'static str>) {
    use strict_ooxml::font_packs;
    use strict_ooxml::system_fonts as system;
    let limits = strict_ooxml::FontAssetLimits::default();
    let faces = system::index();
    let mut assets = if named {
        system::assets_for(&used_font_families(document), faces, limits).unwrap_or_default()
    } else {
        Vec::new()
    };
    let key = |asset: &strict_ooxml::FontAsset| {
        let (family, bold, italic) = asset.key();
        (family.to_lowercase(), bold, italic)
    };
    let add = |more: Vec<strict_ooxml::FontAsset>, assets: &mut Vec<strict_ooxml::FontAsset>| {
        for asset in more {
            if !assets.iter().any(|known| key(known) == key(&asset)) {
                assets.push(asset);
            }
        }
    };
    // Packs fetched before, for the text the bundled faces do not draw.
    let undrawn = system::undrawn(document, &[]);
    let root = font_packs::cache_dir();
    let wanted: Vec<&'static font_packs::Pack> = font_packs::CATALOG
        .iter()
        .filter(|pack| undrawn.iter().any(|ch| pack.covers(*ch)))
        .collect();
    for pack in &wanted {
        if let Some(root) = root.as_ref().filter(|root| pack.is_installed(root)) {
            add(pack.assets(root, limits).unwrap_or_default(), &mut assets);
        }
    }
    let fallbacks = system::script_fallbacks(document, faces, limits);
    add(fallbacks.clone(), &mut assets);
    // What still has no glyph: a pack that has it is offered.
    let still = system::undrawn(document, &assets);
    let missing = wanted
        .iter()
        .filter(|pack| still.iter().any(|ch| pack.covers(*ch)))
        .map(|pack| pack.id)
        .collect();
    if assets.is_empty() {
        return (None, missing);
    }
    let set = strict_ooxml::FontAssets::new(assets, limits)
        .or_else(|_| strict_ooxml::FontAssets::new(fallbacks, limits))
        .ok()
        .map(Arc::new);
    (set, missing)
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
pub(crate) struct Logical {
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
    links: Vec<navigation::Link>,
    scheme_sources: Vec<(strict_ooxml::model::Graphic, diagram_editor::Diagram)>,
    chem_sources: Vec<(strict_ooxml::model::Graphic, chem_editor::ChemDocument)>,
    session: Session<'a>,
    options: RenderOptions,
    system_fonts: bool,
    font_families: Vec<String>,
    replies: &'s Sender<Reply>,
    ctx: &'s eframe::egui::Context,
    map: Option<Arc<VisualMap>>,
    pages: Vec<Arc<PageData>>,
    page_sizes: Vec<(f64, f64)>,
    caret: Option<Logical>,
    anchor: Option<Logical>,
    /// Whole cells selected, instead of text.
    cells: Option<CellSelection>,
    /// The cell a click started in, for selecting cells by dragging.
    cell_origin: Option<CellRef>,
    goal_x: Option<f64>,
    /// The line a caret was put on, by its page and a y on it: a position at
    /// a soft line break is both the end of one line and the start of the
    /// next, and only one of them shows. Holds while the caret stays there.
    line_hint: Option<(Logical, usize, f64)>,
    scroll_at_top: bool,
    /// The selected drawing.
    object: Option<DrawingPosition>,
    formula: Option<TextPosition>,
    /// A superscript or subscript switched on (or off) with no selection:
    /// the next text typed takes it. `Some(None)`: back to the baseline.
    #[allow(clippy::option_option)]
    pending_script: Option<Option<VertAlign>>,
    /// The story the caret edits: the body, or a header or footer.
    story: Story,
    /// The page whose header or footer is open. The same story is painted on
    /// every page that shows it; the caret stays on this page.
    header_page: Option<usize>,
    /// Where the caret was in the body before a header was entered.
    body_caret: Option<Logical>,
    undo_depth: usize,
    redo_depth: usize,
    saved_revision: u64,
    /// The layout no longer matches the model.
    stale: bool,
    path: Option<PathBuf>,
    outline: Vec<OutlineEntry>,
    read_only: Option<String>,
    speller: Option<Arc<crate::spell::Speller>>,
    /// Set when underlines should be rebuilt, and cleared once a pass starts.
    spell_dirty: bool,
    /// The epoch and word cache shared with the spelling thread. A pass sends
    /// underlines only when the epoch is still the one it was started with.
    spell_sync: Arc<Mutex<SpellSync>>,
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
            scheme_sources: vec![],
            chem_sources: vec![],
            options,
            system_fonts: false,
            font_families: Vec::new(),
            replies,
            ctx,
            map: None,
            links: Vec::new(),
            pages: Vec::new(),
            page_sizes: Vec::new(),
            caret: None,
            anchor: None,
            cells: None,
            cell_origin: None,
            goal_x: None,
            line_hint: None,
            scroll_at_top: false,
            object: None,
            formula: None,
            pending_script: None,
            speller: None,
            spell_dirty: false,
            spell_sync: Arc::new(Mutex::new(SpellSync {
                epoch: 0,
                cache: HashMap::new(),
            })),
            story: Story::Body,
            header_page: None,
            body_caret: None,
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

    /// Tells the user why an edit of the text was refused.
    fn note_edit(&self, error: &EditError) {
        let (key, detail) = edit_notice(error);
        self.note(key, detail, true);
    }

    /// Tells the user why a search or a whole-document change was refused.
    fn note_operation(&self, error: &OperationError) {
        match error {
            OperationError::Edit(error) => self.note_edit(error),
            OperationError::InvalidQuery => self.note("note.invalid_query", String::new(), true),
            OperationError::LimitExceeded => self.note("note.edit_limit", String::new(), true),
            OperationError::ProtectedContent => self.note("note.protected", String::new(), true),
            OperationError::UnsupportedMove => {
                self.note("note.unsupported_move", String::new(), true);
            }
        }
    }

    /// A refusal that arrived as a locale key (`note.…`) or as a loose sentence.
    fn note_text(&self, error: &str) {
        match error {
            "note.cannot_edit_here" => self.note("note.cannot_edit_here", String::new(), true),
            "note.no_paragraph" => self.note("note.no_paragraph", String::new(), true),
            "note.selection_structure" => {
                self.note("note.selection_structure", String::new(), true);
            }
            _ => self.note("note.edit_refused", error.to_owned(), true),
        }
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
        let mut queued = None;
        while let Some(first) = queued.take().or_else(|| requests.recv().ok()) {
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
            // Spelling waits until this burst is done, then runs beside the
            // caret instead of ahead of the next key.
            match requests.try_recv() {
                Ok(next) => queued = Some(next),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    if self.spell_dirty {
                        self.schedule_spelling();
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
    }

    fn handle(&mut self, request: Request) -> Outcome {
        // A drawing stays selected for what acts on it; anything else (a
        // click selects anew) leaves it.
        let keeps_object = matches!(
            request,
            Request::Object(_)
                | Request::PasteRich { .. }
                | Request::Backspace
                | Request::Delete
                | Request::Type(_)
                | Request::Copy { .. }
                | Request::Click { .. }
                | Request::ChartOfObject
                | Request::SchemeOfObject
                | Request::PutScheme(_)
                | Request::ChemistryOfObject
                | Request::PutChemistry(_)
                | Request::PutChart(_)
                | Request::Save { .. }
                | Request::SystemFonts(_)
                | Request::Speller(_)
                | Request::Respell
                | Request::ContextClick { .. }
                | Request::ChatSnapshot { .. }
                | Request::ChatFind { .. }
        );
        if !keeps_object {
            self.object = None;
        }
        if !matches!(
            request,
            Request::Object(_)
                | Request::PasteRich { .. }
                | Request::Format(_)
                | Request::FormulaAtCaret
                | Request::PutFormula { .. }
                | Request::Copy { .. }
                | Request::Click { .. }
                | Request::Delete
                | Request::Backspace
                | Request::Type(_)
                | Request::Save { .. }
                | Request::SystemFonts(_)
                | Request::Speller(_)
                | Request::Respell
                | Request::ContextClick { .. }
                | Request::ChatSnapshot { .. }
                | Request::ChatFind { .. }
        ) {
            self.formula = None;
        }
        if self.formula.is_some()
            && matches!(
                request,
                Request::Delete | Request::Backspace | Request::Type(_)
            )
        {
            let at = self.formula.take().unwrap();
            let start = self.logical(&at);
            self.anchor = Some(start.clone());
            self.caret = Some(Logical {
                offset: start.offset + 1,
                ..start
            });
        }
        // Moving the caret drops a script switched on for the next text.
        if matches!(
            request,
            Request::Move { .. }
                | Request::Click { .. }
                | Request::SelectAll
                | Request::SelectWord { .. }
        ) {
            self.pending_script = None;
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
            Request::FollowLink(target) => self.follow_link(target),
            Request::PasteRich { content, fallback } => self.paste_rich(content, &fallback),
            Request::ConfirmFormulaRecognition(plan) => self.confirm_formula_recognition(*plan),
            Request::Shutdown => Outcome::Quit,
            Request::ParagraphFormat => {
                if let Some(p) = self
                    .caret
                    .as_ref()
                    .and_then(|c| self.paragraph(&c.paragraph))
                {
                    let computed = strict_ooxml_render_svg::style::compute_paragraph(
                        self.session.document(),
                        p,
                    );
                    self.send(Reply::ParagraphFormat(Box::new(
                        crate::paragraph::Format::from_computed(&computed),
                    )));
                }
                Outcome::Quiet
            }
            Request::SetParagraphFormat(patch) => self.paragraph_edit(|p| patch.apply(p)),
            Request::ContextClick { page, x, y } => self.context_click(page, x, y),
            Request::Click {
                page,
                x,
                y,
                extend,
                column,
            } => {
                let outcome = self.click(page, x, y, extend, column);
                self.line_hint = self.caret.clone().map(|caret| (caret, page, y));
                outcome
            }
            Request::Table(command) if command.selects() => self.table_command(command),
            Request::SelectWord { page, x, y } => self.select_word(page, x, y),
            Request::Move { motion, extend } => self.motion(motion, extend),
            Request::SelectAll => self.select_all(),
            Request::GoTo(paragraph) => {
                self.scroll_at_top = true;
                self.story = paragraph.story.clone();
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
                self.system_fonts = enabled;
                self.font_families = used_font_families(self.session.document());
                let (assets, missing) = installed_fonts(self.session.document(), enabled);
                self.options.font_assets = assets;
                if !missing.is_empty() {
                    self.send(Reply::FontsMissing(missing));
                }
                self.stale = true;
                Outcome::Moved
            }
            Request::Speller(speller) => {
                self.speller = speller;
                self.forget_spelling();
                self.respell();
                Outcome::Quiet
            }
            Request::ChartOfObject => {
                let data = self.selected_chart().map(|data| Box::new((*data).clone()));
                self.send(Reply::Chart(data));
                Outcome::Quiet
            }
            Request::SchemeOfObject => {
                match self.scheme_plan() {
                    Ok(plan) => self.send(Reply::Scheme(Box::new(plan))),
                    Err(error) => self.note("scheme.failed", error, true),
                }
                Outcome::Quiet
            }
            Request::PutScheme(plan) => self.put_scheme(*plan),
            Request::ChemistryOfObject => {
                match self.chem_plan() {
                    Ok(plan) => self.send(Reply::Chemistry(Box::new(plan))),
                    Err(error) => self.note("chem.failed", error, true),
                }
                Outcome::Quiet
            }
            Request::PutChemistry(plan) => self.put_chemistry(*plan),
            Request::FormulaAtCaret => {
                let found = self.formula_at_caret().map(Box::new);
                self.send(Reply::Formula(found));
                Outcome::Quiet
            }
            Request::AiSource { whole, job } => {
                let source = self.ai_source(whole);
                self.send(Reply::AiSource {
                    job,
                    source: Box::new(source),
                });
                Outcome::Quiet
            }
            Request::CleanupSource { job } => {
                let snapshot =
                    crate::cleanup::snapshot(self.session.document(), self.session.revision());
                let paragraphs = snapshot
                    .entries
                    .iter()
                    .map(|entry| {
                        let text = crate::ai::expose(&entry.paragraph)
                            .unwrap_or_else(|| crate::cleanup::text(&entry.paragraph));
                        (Address::body(entry.block), text)
                    })
                    .collect();
                self.send(Reply::AiSource {
                    job,
                    source: Box::new(AiSource {
                        paragraphs,
                        selection: None,
                        cleanup: Some(snapshot),
                    }),
                });
                Outcome::Quiet
            }
            Request::Respell => {
                self.forget_spelling();
                self.respell();
                Outcome::Quiet
            }
            Request::Find { query, backwards } => self.find(&query, backwards),
            request => {
                if let Some(reason) = &self.read_only {
                    self.note("note.read_only", reason.clone(), true);
                    if matches!(request, Request::ApplyCleanup(_)) {
                        self.send(Reply::CleanupResult(false));
                    }
                    return Outcome::Quiet;
                }
                self.edit_request(request)
            }
        }
    }

    fn edit_request(&mut self, request: Request) -> Outcome {
        match request {
            Request::ApplyCleanup(plan) => {
                let outcome = match crate::cleanup::commands(
                    self.session.document(),
                    self.session.revision(),
                    &plan,
                ) {
                    Ok(commands) if !commands.is_empty() && self.transact(&commands) => {
                        self.object = None;
                        self.formula = None;
                        self.anchor = None;
                        self.cells = None;
                        self.story = Story::Body;
                        self.caret = self.first_paragraph().map(|paragraph| Logical {
                            paragraph,
                            offset: 0,
                        });
                        self.note("cleanup.applied", String::new(), false);
                        Outcome::Edited
                    }
                    Ok(_) => Outcome::Quiet,
                    Err(key) => {
                        self.note(key, String::new(), true);
                        Outcome::Quiet
                    }
                };
                self.send(Reply::CleanupResult(matches!(outcome, Outcome::Edited)));
                outcome
            }
            Request::Type(text) => self.type_text(&text),
            Request::SetParagraphText { at, old, new } => {
                if let Some(paragraph) = self.paragraph(&at).cloned() {
                    if crate::ai::expose(&paragraph).as_deref() == Some(old.as_str())
                        && crate::ai::protected(&old)
                    {
                        let Some(inlines) = crate::ai::restore(&paragraph, &new) else {
                            return Outcome::Quiet;
                        };
                        return self.replace_paragraph_inlines(at, inlines);
                    }
                }
                let text = self.text_of(&at);
                if text != old || text.contains(nav::OBJECT) {
                    return Outcome::Quiet;
                }
                self.story = Story::Body;
                self.cells = None;
                self.anchor = Some(Logical {
                    paragraph: at.clone(),
                    offset: 0,
                });
                self.caret = Some(Logical {
                    paragraph: at,
                    offset: text.chars().count(),
                });
                self.type_text(&new)
            }
            Request::ReplaceRange {
                start,
                end,
                old,
                new,
            } => {
                self.story = Story::Body;
                self.cells = None;
                self.anchor = Some(start);
                self.caret = Some(end);
                if self.selected_text().unwrap_or_default() != old {
                    return Outcome::Moved;
                }
                self.paste(&new)
            }
            Request::ApplyKinds(kinds) => self.apply_kinds(&kinds),
            Request::ChatSnapshot { job } => {
                self.chat_snapshot(job);
                Outcome::Quiet
            }
            Request::ChatFind { query } => {
                self.chat_find(&query);
                Outcome::Quiet
            }
            Request::ApplyChat(batch) => self.apply_chat(&batch),
            Request::PutChart(data) => self.put_chart(*data),
            Request::PutFormula {
                expression,
                replace,
            } => self.put_formula(*expression, replace),
            Request::ReplaceWord {
                start,
                end,
                word,
                with,
            } => {
                let text = self.text_of(&start.paragraph);
                let there: String = text
                    .chars()
                    .skip(start.offset)
                    .take(end.offset.saturating_sub(start.offset))
                    .collect();
                if start.paragraph != end.paragraph || there.replace('\u{2019}', "'") != word {
                    return Outcome::Quiet;
                }
                self.story = Story::Body;
                self.cells = None;
                self.anchor = Some(start);
                self.caret = Some(end);
                self.type_text(&with)
            }
            Request::Backspace => self.backspace(),
            Request::Delete => self.delete_forward(),
            Request::Enter => self.enter(),
            Request::Tab { back } => self.tab(back),
            Request::Table(command) => self.table_command(command),
            Request::Format(change) => self.format(&change),
            Request::Align(align) => self.align(align),
            Request::List(preset) => self.list(preset),
            Request::HeaderFooter(command) => self.header_footer(command),
            Request::References(command) => self.references(command),
            Request::Comments(command) => self.comments(command),
            Request::IndentStep(more) => self.indent_step(more),
            Request::Indent([left, right, first]) => {
                use strict_ooxml::model::values::Indentation;
                self.paragraph_edit(|properties| {
                    properties.indentation = Some(Indentation {
                        start: Some(Twips(left)),
                        end: Some(Twips(right)),
                        first_line: (first >= 0).then_some(Twips(first)),
                        hanging: (first < 0).then_some(Twips(-first)),
                        ..Indentation::default()
                    });
                })
            }
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
        self.links.clear();
        self.send(Reply::Links(Vec::new()));
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
        if self.system_fonts {
            let families = used_font_families(self.session.document());
            if families != self.font_families {
                self.font_families = families;
                let (assets, missing) = installed_fonts(self.session.document(), true);
                self.options.font_assets = assets;
                if !missing.is_empty() {
                    self.send(Reply::FontsMissing(missing));
                }
            }
        }
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
                    Ok(map) => Some(Arc::new(map)),
                    Err(error) => {
                        if self.read_only.is_none() {
                            match &error {
                                VisualError::Attribution => {
                                    self.note("note.no_caret_map", String::new(), true);
                                }
                                VisualError::Render(error) => {
                                    self.note("note.render_failed", error.to_string(), true);
                                }
                                VisualError::Edit(error) => self.note_edit(error),
                            }
                        }
                        None
                    }
                }
            }
            Session::View(_) => None,
        };
        crate::trace::log(|| format!("caret map took {} ms", pages_at.elapsed().as_millis()));
        self.links = self.navigation_links(&options);
        self.send(Reply::Links(self.links.clone()));
        self.send(Reply::Busy(None));
        let comments = self.comment_views();
        self.send(Reply::Comments(comments));
        self.spell_dirty = self.speller.is_some();
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
        text_position(&self.pieces(&logical.paragraph), logical)
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
        self.caret_rects().into_iter().next()
    }

    /// Caret rectangles of the active story. A repeated header or footer
    /// keeps only the page that was opened for editing.
    fn caret_rects(&self) -> Vec<VisualRect> {
        let Some(caret) = &self.caret else {
            return Vec::new();
        };
        let Some(map) = &self.map else {
            return Vec::new();
        };
        let rects = map
            .caret(self.session.revision(), &self.position(caret))
            .unwrap_or_default();
        let rects = if matches!(self.story, Story::HeaderFooter(_)) {
            rects
        } else {
            self.one_line(caret, rects)
        };
        self.on_edited_page(self.chem_carets(self.scheme_carets(rects)))
    }

    /// One caret per page where a position stands on two lines of it: at a
    /// soft line break the end of one line is the start of the next. The line
    /// it was put on when that is known (a click, End, Up/Down), else the
    /// later one - the next line's start, where typing goes on, as Word shows
    /// it. Other pages keep theirs (a table's repeated header row).
    fn one_line(&self, caret: &Logical, rects: Vec<VisualRect>) -> Vec<VisualRect> {
        if rects.len() < 2 {
            return rects;
        }
        let hint = self
            .line_hint
            .as_ref()
            .filter(|(at, _, _)| at == caret)
            .map(|(_, page, y)| (*page, *y));
        let mut pages: Vec<usize> = rects.iter().map(|rect| rect.page).collect();
        pages.dedup();
        pages
            .into_iter()
            .filter_map(|page| {
                let here = rects.iter().copied().filter(|rect| rect.page == page);
                let hinted = hint.filter(|(at, _)| *at == page).and_then(|(_, y)| {
                    here.clone()
                        .find(|rect| y >= rect.y - 0.5 && y <= rect.y + rect.height + 0.5)
                });
                hinted.or_else(|| here.max_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x))))
            })
            .collect()
    }

    /// Drops copies of an open header or footer that belong to other pages.
    fn on_edited_page(&self, rects: Vec<VisualRect>) -> Vec<VisualRect> {
        let Some(page) = self
            .header_page
            .filter(|_| matches!(self.story, Story::HeaderFooter(_)))
        else {
            return rects;
        };
        let here: Vec<_> = rects
            .iter()
            .copied()
            .filter(|rect| rect.page == page)
            .collect();
        if here.is_empty() {
            rects
        } else {
            here
        }
    }

    fn send_cursor(&mut self, scroll: bool) {
        self.ensure_layout();
        let revision = self.session.revision();
        let (carets, selection) = match (&self.map, &self.caret) {
            (Some(map), Some(caret)) => {
                let carets = map
                    .caret(revision, &self.position(caret))
                    .unwrap_or_default();
                let carets = if matches!(self.story, Story::HeaderFooter(_)) {
                    carets
                } else {
                    self.one_line(caret, carets)
                };
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
        let (carets, selection) = (
            self.on_edited_page(self.chem_carets(self.scheme_carets(carets))),
            self.on_edited_page(selection),
        );
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
        for rect in self.selected_object_rects() {
            if !selection.contains(&rect) {
                selection.push(rect);
            }
        }
        let object = self.object_view();
        let ruler = self.ruler_info(
            carets
                .first()
                .or(selection.first())
                .map_or(0, |rect| rect.page),
        );
        // A selected drawing shows its frame, not a caret.
        let carets = if object.is_some() { Vec::new() } else { carets };
        let format = self.caret_format();
        let header_footer = self.header_footer_view();
        let at_top = std::mem::take(&mut self.scroll_at_top);
        self.send(Reply::Cursor(Box::new(Cursor {
            object,
            ruler,
            header_footer,
            carets,
            selection,
            scroll,
            at_top,
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
        let document = self.session.document();
        let computed_paragraph =
            strict_ooxml_render_svg::style::compute_paragraph(document, paragraph);
        let computed_run = nav::run_at(paragraph, &position.inline).map_or_else(
            || computed_paragraph.mark_run().clone(),
            |run| strict_ooxml_render_svg::style::compute_run(document, &computed_paragraph, run),
        );
        let props = nav::run_at(paragraph, &position.inline)
            .map(|run| &run.props)
            .or(paragraph.props.run_props.as_ref());
        let pending = self.pending_script;
        let mut format = CaretFormat {
            size: Some(computed_run.size_pt as f32),
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
            format.font = props
                .fonts
                .as_ref()
                .and_then(|fonts| fonts.ascii.as_deref().or(fonts.h_ansi.as_deref()))
                .map(str::to_owned);
            format.underline_style = props.underline.filter(|u| *u != Underline::None);
            format.double_strike = props.double_strike == TriState::On;
        }
        if let Some(script) = pending {
            format.superscript = script == Some(VertAlign::Superscript);
            format.subscript = script == Some(VertAlign::Subscript);
        }
        if self.formula.is_some() {
            if let Some((_, _, expression)) = self.formula_at_caret() {
                format.size = crate::formula::size(&expression)
                    .map(|size| size.0 as f32 / 2.0)
                    .or(Some(
                        strict_ooxml_render_svg::style::compute_paragraph(
                            self.session.document(),
                            paragraph,
                        )
                        .default_run
                        .size_pt as f32,
                    ));
            }
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
        let (bookmarks, table_of_contents) = self.bookmarks_and_toc();
        self.send(Reply::State(EditState {
            bookmarks,
            table_of_contents,
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
        // A click on a drawing selects it, the caret before it; a click on
        // the text of a text box puts the caret in that text, as Word does.
        let mut in_box = None;
        if !extend && !column {
            if let Ok(Some(formula)) = map.hit_formula(revision, page, x, y) {
                self.caret = Some(self.logical(&formula));
                self.formula = Some(formula);
                self.object = None;
                self.anchor = None;
                self.cells = None;
                return Outcome::Moved;
            }
            in_box = map.hit_box_text(revision, page, x, y).ok().flatten();
            if let Some(object) = map
                .hit_drawing(revision, page, x, y)
                .ok()
                .flatten()
                .or_else(|| self.scheme_hit(page, x, y))
                .filter(|_| in_box.is_none())
            {
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
        self.formula = None;
        let Some(position) = in_box.or_else(|| map.hit_test(revision, page, x, y).ok().flatten())
        else {
            return Outcome::Quiet;
        };
        // The body and the headers are edited one at a time: a click in the
        // header while the body is edited does nothing (a double click
        // enters it), and a click in the body leaves a header.
        let header = |story: &Story| matches!(story, Story::HeaderFooter(_));
        if position.paragraph.story != self.story {
            if header(&position.paragraph.story) && !header(&self.story) {
                return Outcome::Quiet;
            }
            if header(&self.story) && !header(&position.paragraph.story) {
                self.body_caret = None;
                self.header_page = None;
            }
            self.story = position.paragraph.story.clone();
        }
        if header(&self.story) {
            self.header_page = Some(page);
        }
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
        // Ctrl+click on a table of contents entry goes to its heading.
        if column {
            if let Some(heading) = self.toc_target(&logical.paragraph) {
                self.scroll_at_top = true;
                self.story = Story::Body;
                self.anchor = None;
                self.cells = None;
                self.caret = Some(Logical {
                    paragraph: heading,
                    offset: 0,
                });
                self.send(Reply::Cursor(Box::new(Cursor {
                    object: None,
                    ruler: None,
                    header_footer: None,
                    carets: Vec::new(),
                    selection: Vec::new(),
                    scroll: true,
                    at_top: true,
                    format: CaretFormat::default(),
                })));
                return Outcome::Moved;
            }
        }
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

    fn context_click(&mut self, page: usize, x: f64, y: f64) -> Outcome {
        self.ensure_layout();
        let contains = |r: &VisualRect| {
            r.page == page
                && x >= r.x
                && x <= r.x + r.width.max(3.0)
                && y >= r.y
                && y <= r.y + r.height
        };
        let comment = self
            .comment_views()
            .into_iter()
            .find(|c| c.rects.iter().any(contains))
            .map(|c| c.id);
        let selected = self
            .selection()
            .zip(self.map.as_ref())
            .is_some_and(|((a, b), map)| {
                map.selection_between(
                    self.session.revision(),
                    &self.position(&a),
                    &self.position(&b),
                )
                .unwrap_or_default()
                .iter()
                .any(contains)
            })
            || self.cell_rects().iter().any(contains);
        let object_hit = self.scheme_hit(page, x, y).is_some()
            || self.map.as_ref().is_some_and(|map| {
                map.hit_drawing(self.session.revision(), page, x, y)
                    .ok()
                    .flatten()
                    .is_some()
                    || map
                        .hit_formula(self.session.revision(), page, x, y)
                        .ok()
                        .flatten()
                        .is_some()
            });
        if object_hit || !selected {
            self.click(page, x, y, false, false);
        }
        let object = self.object_view().map(|o| {
            if o.formula {
                ContextObject::Formula
            } else if o.chart {
                ContextObject::Chart
            } else if o.scheme {
                ContextObject::Scheme
            } else if o.chemistry {
                ContextObject::Chemistry
            } else if o.picture {
                ContextObject::Picture
            } else {
                ContextObject::Shape
            }
        });
        self.send(Reply::Context(ContextState {
            editable: self.read_only.is_none(),
            selection: self.selection().is_some() || self.cells.is_some(),
            table: self.in_cell().is_some(),
            object,
            comment,
        }));
        Outcome::Moved
    }

    fn select_word(&mut self, page: usize, x: f64, y: f64) -> Outcome {
        self.ensure_layout();
        if self.scheme_hit(page, x, y).is_some() {
            self.click(page, x, y, false, false);
            match self.scheme_plan() {
                Ok(plan) => self.send(Reply::Scheme(Box::new(plan))),
                Err(error) => self.note("scheme.failed", error, true),
            }
            return Outcome::Moved;
        }
        if self.chem_hit(page, x, y).is_some() {
            self.click(page, x, y, false, false);
            match self.chem_plan() {
                Ok(plan) => self.send(Reply::Chemistry(Box::new(plan))),
                Err(error) => self.note("chem.failed", error, true),
            }
            return Outcome::Moved;
        }
        if let Some(at) = self.map.as_ref().and_then(|map| {
            map.hit_formula(self.session.revision(), page, x, y)
                .ok()
                .flatten()
        }) {
            self.caret = Some(self.logical(&at));
            self.formula = Some(at);
            self.object = None;
            self.anchor = None;
            self.send(Reply::Formula(self.formula_at_caret().map(Box::new)));
            return Outcome::Moved;
        }
        // A double click in a header or footer, or in the margin where one
        // goes, edits it.
        if !matches!(self.story, Story::HeaderFooter(_)) {
            if let Some(header) = self.margin_band(page, y) {
                return self.enter_header_footer(page, header, Some((x, y)));
            }
        }
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
        let paragraphs = self.paragraphs(&self.story.clone());
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
                let y = rect.y + rect.height / 2.0;
                let target = self.hit(rect.page, x, y);
                // End stays on this line even where its end is the next
                // line's start.
                self.line_hint = target.clone().map(|target| (target, rect.page, y));
                target
            }
            Motion::DocStart => self
                .paragraphs(&self.story)
                .first()
                .cloned()
                .map(|paragraph| Logical {
                    paragraph,
                    offset: 0,
                }),
            Motion::DocEnd => self
                .paragraphs(&self.story)
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
            .flatten()
            .filter(|position| position.paragraph.story == self.story)?;
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
                self.line_hint = Some((target.clone(), page, y));
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
                self.note_edit(&error);
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
        Self::paragraph_replace_in(
            at,
            self.paragraph(at).ok_or("note.no_paragraph")?,
            start,
            end,
            text,
        )
    }

    fn paragraph_replace_in(
        at: &Address,
        paragraph: &Paragraph,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<Vec<Edit>, &'static str> {
        let pieces = nav::pieces(paragraph);
        if pieces.is_empty() {
            if text.is_empty() {
                return Ok(Vec::new());
            }
            // A paragraph with no text but other inlines (a note's number
            // mark, a bookmark): the text is a run of its own after them.
            let inlines = paragraph.inlines.len();
            if inlines > 0 {
                use strict_ooxml::model::{Inline, Run, RunContent, Space, TextNode};
                return Ok(vec![Edit::InsertInline {
                    at: at.clone(),
                    index: inlines,
                    inline: Box::new(Inline::Run(Run {
                        props: strict_ooxml::model::props::RunProperties::default(),
                        content: vec![RunContent::Text(TextNode {
                            text: text.to_owned(),
                            space: Space::Preserve,
                        })],
                        revision: None,
                        location: SourceLocation::unknown(),
                    })),
                }]);
            }
            return Ok(vec![Edit::Text {
                at: at.clone(),
                range: 0..0,
                text: text.to_owned(),
            }]);
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
            let props = paragraph
                .inlines
                .iter()
                .find_map(|inline| match inline {
                    strict_ooxml::model::Inline::Run(run) => Some(run.props.clone()),
                    _ => None,
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

    /// Puts `inlines` in place of the paragraph's, keeping the paragraph.
    fn replace_paragraph_inlines(
        &mut self,
        at: Address,
        inlines: Vec<strict_ooxml::model::Inline>,
    ) -> Outcome {
        let Some(paragraph) = self.paragraph(&at) else {
            return Outcome::Quiet;
        };
        let mut commands = Vec::new();
        for index in (0..paragraph.inlines.len()).rev() {
            commands.push(Edit::DeleteInline {
                at: at.clone(),
                index,
            });
        }
        for (index, inline) in inlines.into_iter().enumerate() {
            commands.push(Edit::InsertInline {
                at: at.clone(),
                index,
                inline: Box::new(inline),
            });
        }
        if commands.is_empty() || self.transact(&commands) {
            self.story = Story::Body;
            self.cells = None;
            self.anchor = None;
            self.object = None;
            self.formula = None;
            let offset = self.text_of(&at).chars().count();
            self.caret = Some(Logical {
                paragraph: at,
                offset,
            });
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn type_text(&mut self, text: &str) -> Outcome {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return Outcome::Quiet;
        }
        if self.selection().is_none() {
            if let Some(script) = self.pending_script.take() {
                return self.type_with_script(&clean, script);
            }
        }
        if self.put_text(&clean) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Types text as a run of its own, raised or lowered (or neither) as
    /// switched on: the text it continues keeps its look otherwise.
    fn type_with_script(&mut self, text: &str, script: Option<VertAlign>) -> Outcome {
        use strict_ooxml::model::{Inline, Run, RunContent, Space, TextNode};
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let position = self.position(&caret);
        let mut props = self
            .paragraph(&caret.paragraph)
            .and_then(|paragraph| nav::run_at(paragraph, &position.inline))
            .map(|run| run.props.clone())
            .unwrap_or_default();
        props.vert_align = script;
        let run = Inline::Run(Run {
            props,
            content: vec![RunContent::Text(TextNode {
                text: text.to_owned(),
                space: Space::Preserve,
            })],
            revision: None,
            location: SourceLocation::unknown(),
        });
        let commands = self.inline_at_caret(&caret, run);
        if self.transact(&commands) {
            self.caret = Some(Logical {
                paragraph: caret.paragraph,
                offset: caret.offset + text.chars().count(),
            });
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// One indent step more or less: a list item's level, else the left
    /// indent by a default tab stop.
    fn indent_step(&mut self, more: bool) -> Outcome {
        use strict_ooxml::model::values::Indentation;
        use strict_ooxml::model::Ilvl;
        let step = self
            .session
            .document()
            .settings
            .default_tab_stop
            .map_or(720, |t| t.0.max(1));
        let document = self.session.document();
        let mut commands = Vec::new();
        for at in self.selected_paragraphs() {
            let Some(paragraph) = self.paragraph(&at) else {
                continue;
            };
            let mut properties = paragraph.props.clone();
            if let Some(level) = strict_ooxml::edit::list_level(document, paragraph) {
                let next = if more {
                    (level.ilvl + 1).min(Ilvl::MAX)
                } else {
                    level.ilvl.saturating_sub(1)
                };
                properties.numbering = Some(strict_ooxml::model::props::NumPr {
                    num_id: Some(level.num_id),
                    ilvl: Some(Ilvl(next)),
                });
            } else {
                let computed =
                    strict_ooxml_render_svg::style::compute_paragraph(document, paragraph);
                #[allow(clippy::cast_possible_truncation)]
                let left = (computed.indent_start_pt * 20.0).round() as i32;
                let next = if more {
                    (left / step + 1) * step
                } else {
                    ((left + step - 1) / step - 1).max(0) * step
                };
                let mut indentation = properties.indentation.unwrap_or_default();
                indentation.start = Some(Twips(next));
                indentation.start_chars = None;
                properties.indentation = Some(indentation);
            }
            if properties != paragraph.props {
                commands.push(Edit::ParagraphProperties {
                    at,
                    properties: Box::new(properties),
                });
            }
        }
        let _ = Indentation::default;
        if self.transact(&commands) {
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
        self.inlines_at_caret(caret, vec![inline])
    }

    /// Commands that put `inlines`, in order, at the caret, between runs.
    fn inlines_at_caret(
        &self,
        caret: &Logical,
        inlines: Vec<strict_ooxml::model::Inline>,
    ) -> Vec<Edit> {
        let Some(paragraph) = self.paragraph(&caret.paragraph) else {
            return vec![];
        };
        Self::inlines_in_paragraph(caret, paragraph, inlines)
    }

    fn inlines_in_paragraph(
        caret: &Logical,
        paragraph: &Paragraph,
        inlines: Vec<strict_ooxml::model::Inline>,
    ) -> Vec<Edit> {
        let at = caret.paragraph.clone();
        let pieces = nav::pieces(paragraph);
        // Split there, put the inline first in the right half, join again:
        // the kernel inserts inlines between runs, and this makes the caret's
        // point a boundary between runs wherever it was.
        if let Some(point) = nav::point_at(&pieces, caret.offset) {
            let mut right = at.clone();
            right.block += 1;
            let mut commands = vec![Edit::SplitAt {
                at: at.clone(),
                point,
            }];
            for (index, inline) in inlines.into_iter().enumerate() {
                commands.push(Edit::InsertInline {
                    at: right.clone(),
                    index,
                    inline: Box::new(inline),
                });
            }
            commands.push(Edit::Join { at });
            return commands;
        }
        // An empty paragraph, or the caret beside atoms between runs.
        let mut before = 0;
        let mut index = 0;
        for piece in &pieces {
            before += piece.len();
            if before == caret.offset {
                index = piece.inline.first().copied().unwrap_or(0) + 1;
            }
        }
        inlines
            .into_iter()
            .enumerate()
            .map(|(offset, inline)| Edit::InsertInline {
                at: at.clone(),
                index: index + offset,
                inline: Box::new(inline),
            })
            .collect()
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
        if let Some(formula) = &self.formula {
            let rects = self
                .map
                .as_ref()?
                .formula_rects(self.session.revision(), formula)
                .ok()?;
            let rect = rects.first()?;
            return Some(ObjectView {
                width: (rect.width * 9525.0).round() as i64,
                height: (rect.height * 9525.0).round() as i64,
                rects,
                picture: false,
                chart: false,
                scheme: false,
                chemistry: false,
                formula: true,
                wrap: WrapStyle::Inline,
            });
        }
        let object = self.object.as_ref()?;
        let map = self.map.as_ref()?;
        let mut rects = map
            .drawing_rects(self.session.revision(), object)
            .ok()
            .filter(|rects| !rects.is_empty())?;
        let drawing = self.drawing_at(object)?;
        if scheme::is_scheme(&drawing) || chem::is_chemistry(&drawing) {
            rects = scheme::frame(&drawing, &rects, self.options.scale);
        }
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
            chart: matches!(graphic, Some(Graphic::Chart(_))),
            scheme: scheme::is_scheme(&drawing),
            chemistry: chem::is_chemistry(&drawing),
            formula: false,
            wrap: wrap_style(&drawing),
        })
    }

    // ----- charts -------------------------------------------------------

    /// The data of the selected drawing, when it is a chart.
    fn selected_chart(&self) -> Option<Arc<strict_ooxml::model::chart::ChartData>> {
        use strict_ooxml::model::{DrawingKind, Graphic};
        let drawing = self.drawing_at(self.object.as_ref()?)?;
        let graphic = match &drawing.kind {
            DrawingKind::Inline(v) => &*v.graphic,
            DrawingKind::Anchor(v) => &*v.graphic,
            DrawingKind::Opaque(_) => return None,
        };
        match graphic {
            Graphic::Chart(refs) => refs.chart.clone(),
            _ => None,
        }
    }

    /// A chart part for `data`; the selected chart shows it, or a new chart
    /// goes in at the caret, as wide as the text and three fifths as high.
    fn put_chart(&mut self, data: strict_ooxml::model::chart::ChartData) -> Outcome {
        use strict_ooxml::edit::{chart_drawing, unused_drawing_id};
        use strict_ooxml::model::{DrawingKind, Emu, ForeignRefs, Graphic, Inline};
        let data = Arc::new(data);
        let selected = self
            .selected_chart()
            .is_some()
            .then(|| self.object.clone())
            .flatten();
        let Some(editor) = self.editor() else {
            return Outcome::Quiet;
        };
        let id = editor.add_chart(&data);
        if let Some(object) = selected {
            let Some(mut drawing) = self.drawing_at(&object) else {
                return Outcome::Quiet;
            };
            let graphic = match &mut drawing.kind {
                DrawingKind::Inline(v) => &mut v.graphic,
                DrawingKind::Anchor(v) => &mut v.graphic,
                DrawingKind::Opaque(_) => return Outcome::Quiet,
            };
            **graphic = Graphic::Chart(ForeignRefs {
                rels: vec![Arc::from(id.as_str())],
                chart: Some(data),
                diagram: None,
                location: SourceLocation::unknown(),
            });
            let command = Edit::Drawing {
                at: object.paragraph.clone(),
                inline: object.inline.clone(),
                content: object.content,
                drawing: Box::new(drawing),
            };
            let outcome = if self.transact(&[command]) {
                Outcome::Edited
            } else {
                Outcome::Quiet
            };
            self.object = Some(object);
            return outcome;
        }
        if self.selection().is_some() && matches!(self.delete_selection(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let width = (i64::from(self.text_width().max(1440)) * 635).min(5_486_400);
        let drawing = chart_drawing(
            &id,
            data,
            Emu(width),
            Emu(width * 3 / 5),
            unused_drawing_id(self.session.document()),
            "Chart",
        );
        let commands = self.inline_at_caret(&caret, Inline::Drawing(drawing));
        if self.transact(&commands) {
            self.anchor = None;
            self.object = self
                .objects_of(&caret.paragraph)
                .into_iter()
                .find(|(offset, _)| *offset == caret.offset)
                .map(|(_, at)| at);
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    // ----- formulas -----------------------------------------------------

    /// An inline formula at the caret's place (formulas take no caret
    /// position of their own, so the caret is just before or after one).
    fn formula_at_caret(
        &self,
    ) -> Option<(Address, usize, strict_ooxml::model::math::MathExpression)> {
        if let Some(at) = &self.formula {
            let index = *at.inline.first()?;
            if let Some(expression) =
                crate::formula::expression(self.paragraph(&at.paragraph)?.inlines.get(index)?)
            {
                return Some((at.paragraph.clone(), index, expression.clone()));
            }
        }
        let caret = self.caret.as_ref()?;
        let paragraph = self.paragraph(&caret.paragraph)?;
        let find = |after: usize| {
            paragraph
                .inlines
                .iter()
                .enumerate()
                .find_map(|(index, inline)| match inline {
                    inline
                        if self.offset_of_inline(&caret.paragraph, index) + after
                            == caret.offset =>
                    {
                        crate::formula::expression(inline)
                            .map(|expression| (caret.paragraph.clone(), index, expression.clone()))
                    }
                    _ => None,
                })
        };
        find(0).or_else(|| find(1))
    }

    fn put_formula(
        &mut self,
        mut expression: strict_ooxml::model::math::MathExpression,
        replace: Option<(Address, usize)>,
    ) -> Outcome {
        if let Some((at, index)) = replace {
            if let Some(old) = self
                .paragraph(&at)
                .and_then(|p| p.inlines.get(index))
                .and_then(crate::formula::expression)
            {
                if let Some(size) = crate::formula::size(old) {
                    crate::formula::set_size(&mut expression, size);
                }
            }
            return self.replace_formula(at, index, expression);
        }
        self.formula = None;
        self.delete_selection();
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let commands = self.inline_at_caret(&caret, strict_ooxml::model::Inline::Math(expression));
        if self.transact(&commands) {
            if let Some((at, index, _)) = self.formula_at_caret() {
                self.formula = Some(TextPosition {
                    paragraph: at,
                    inline: vec![index],
                    content: 0,
                    offset: 0,
                });
            }
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn replace_formula(
        &mut self,
        at: Address,
        index: usize,
        expression: strict_ooxml::model::math::MathExpression,
    ) -> Outcome {
        let Some(old) = self.paragraph(&at).and_then(|p| p.inlines.get(index)) else {
            return Outcome::Quiet;
        };
        let replacement = match old {
            strict_ooxml::model::Inline::Math(_) => strict_ooxml::model::Inline::Math(expression),
            strict_ooxml::model::Inline::MathParagraph(paragraph) => {
                let mut paragraph = paragraph.clone();
                let Some(first) = paragraph.equations.first_mut() else {
                    return Outcome::Quiet;
                };
                *first = expression;
                strict_ooxml::model::Inline::MathParagraph(paragraph)
            }
            _ => return Outcome::Quiet,
        };
        let commands = [
            Edit::DeleteInline {
                at: at.clone(),
                index,
            },
            Edit::InsertInline {
                at: at.clone(),
                index,
                inline: Box::new(replacement),
            },
        ];
        if self.transact(&commands) {
            self.formula = Some(TextPosition {
                paragraph: at,
                inline: vec![index],
                content: 0,
                offset: 0,
            });
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    // ----- AI -----------------------------------------------------------

    fn ai_source(&self, whole: bool) -> AiSource {
        let selection = if whole {
            None
        } else {
            self.selection().and_then(|(start, end)| {
                let text = self.selected_text()?;
                Some((start, end, text))
            })
        };
        let addresses = if selection.is_some() {
            self.selected_paragraphs()
        } else {
            self.paragraphs(&Story::Body)
        };
        let paragraphs = addresses
            .into_iter()
            .map(|at| {
                let text = self
                    .paragraph(&at)
                    .and_then(crate::ai::expose)
                    .unwrap_or_else(|| self.text_of(&at));
                (at, text)
            })
            .collect();
        AiSource {
            cleanup: None,
            paragraphs,
            selection,
        }
    }

    /// Sets each paragraph's style from its kind, making the styles a
    /// document lacks from its default paragraph style.
    fn apply_kinds(&mut self, kinds: &[(Address, crate::ai::Kind)]) -> Outcome {
        let (definitions, commands) = self.kind_plan(kinds);
        if commands.is_empty() {
            return Outcome::Quiet;
        }
        self.define_and_transact(&definitions, &commands)
    }

    /// Style definitions and paragraph commands for `kinds`, without applying them.
    fn kind_plan(
        &self,
        kinds: &[(Address, crate::ai::Kind)],
    ) -> (Vec<strict_ooxml::edit::Definition>, Vec<Edit>) {
        use crate::ai::Kind;
        use strict_ooxml::model::values::StyleType;
        let document = self.session.document();
        let default = document
            .styles
            .iter()
            .find(|style| style.style_type == StyleType::Paragraph && style.is_default)
            .cloned();
        let default_id = default
            .as_ref()
            .map_or_else(|| StyleId::new("Normal"), |style| style.id.clone());
        let mut definitions = Vec::new();
        let mut made: Vec<StyleId> = Vec::new();
        let mut style_for = |kind: Kind| -> Option<StyleId> {
            let (id, name) = match kind {
                Kind::Body => return Some(default_id.clone()),
                Kind::Title => ("Title".to_owned(), "Title".to_owned()),
                Kind::Heading(level) => (format!("Heading{level}"), format!("heading {level}")),
                Kind::Quote => ("Quote".to_owned(), "Quote".to_owned()),
                Kind::Caption => ("Caption".to_owned(), "caption".to_owned()),
            };
            let id = StyleId::new(id);
            if document.styles.contains(&id) || made.contains(&id) {
                return Some(id);
            }
            let mut style = default.clone()?;
            style.id = id.clone();
            style.name = Some(name.into());
            style.is_default = false;
            style.q_format = true;
            style.based_on = Some(default_id.clone());
            style.next = Some(default_id.clone());
            style.based_on_chain = vec![default_id.clone()];
            style.paragraph = ParagraphProperties::default();
            style.run = strict_ooxml::model::props::RunProperties::default();
            match kind {
                Kind::Title => {
                    style.run.size = Some(HalfPoints(52));
                    style.paragraph.alignment = Some(Justification::Center);
                }
                Kind::Heading(level) => {
                    style.run.bold = TriState::On;
                    style.run.size = Some(HalfPoints(match level {
                        1 => 32,
                        2 => 28,
                        _ => 26,
                    }));
                    style.paragraph.keep_next = TriState::On;
                    style.paragraph.outline_level = Some(level.saturating_sub(1));
                    style.paragraph.spacing = Some(strict_ooxml::model::values::Spacing {
                        before: Some(Twips(240)),
                        after: Some(Twips(120)),
                        ..Default::default()
                    });
                }
                Kind::Quote => {
                    style.run.italic = TriState::On;
                    style.paragraph.indentation = Some(strict_ooxml::model::values::Indentation {
                        start: Some(Twips(720)),
                        end: Some(Twips(720)),
                        ..Default::default()
                    });
                }
                Kind::Caption => {
                    style.run.italic = TriState::On;
                    style.run.size = Some(HalfPoints(18));
                }
                Kind::Body => {}
            }
            made.push(id.clone());
            definitions.push(strict_ooxml::edit::Definition::CreateStyle(Box::new(style)));
            Some(id)
        };
        let mut commands = Vec::new();
        for (at, kind) in kinds {
            let Some(style) = style_for(*kind) else {
                continue;
            };
            let Some(paragraph) = self.paragraph(at) else {
                continue;
            };
            let current = paragraph
                .props
                .style
                .clone()
                .unwrap_or_else(|| default_id.clone());
            // Only a paragraph in the default style, or in a heading style,
            // takes the model's kind: one in a style of its own (body text
            // in Times, a list) keeps it, and body text leaves a heading
            // style only.
            let heading = self
                .session
                .document()
                .styles
                .get(&current)
                .is_some_and(|s| {
                    s.paragraph.outline_level.is_some()
                        || s.id.as_str().starts_with("Heading")
                        || s.id.as_str() == "Title"
                });
            let plain = current == default_id && paragraph.props.numbering.is_none();
            let takes = match kind {
                Kind::Body => heading,
                Kind::Title | Kind::Heading(_) => plain || heading,
                Kind::Quote | Kind::Caption => plain,
            };
            if current == style || !takes {
                continue;
            }
            let mut properties = paragraph.props.clone();
            properties.style = Some(style);
            commands.push(Edit::ParagraphProperties {
                at: at.clone(),
                properties: Box::new(properties),
            });
        }
        (definitions, commands)
    }

    // ----- spelling -----------------------------------------------------

    /// Drops remembered verdicts so the next pass checks every paragraph.
    ///
    /// The epoch moves with the cache. A pass already running still holds the
    /// old number; it must not write its verdicts back after this, or the next
    /// pass would trust them. That is what left every word underlined after a
    /// dictionary had just been fetched: the new speller never saw the text.
    fn forget_spelling(&self) {
        let mut gate = spell_gate(&self.spell_sync);
        gate.cache.clear();
        gate.epoch = gate.epoch.wrapping_add(1);
    }

    /// Asks for underlines again. Layout runs first when the pages are stale;
    /// the pass itself starts only once the request queue is empty.
    fn respell(&mut self) {
        if self.stale {
            self.ensure_layout();
        }
        if self.speller.is_none() {
            self.spell_dirty = false;
            {
                let mut gate = spell_gate(&self.spell_sync);
                gate.epoch = gate.epoch.wrapping_add(1);
            }
            self.send(Reply::Spelling {
                errors: Vec::new(),
                missing: Vec::new(),
            });
            return;
        }
        self.spell_dirty = true;
    }

    /// Copies the body and checks it on another thread. A newer pass, or
    /// spelling switched off, makes this one throw its result away.
    fn schedule_spelling(&mut self) {
        self.spell_dirty = false;
        let Some(speller) = self.speller.clone() else {
            return;
        };
        let (epoch, cache) = {
            let mut gate = spell_gate(&self.spell_sync);
            gate.epoch = gate.epoch.wrapping_add(1);
            (gate.epoch, gate.cache.clone())
        };
        let paragraphs: Vec<SpellSource> = self
            .paragraphs(&Story::Body)
            .into_iter()
            .map(|address| {
                let pieces = self.pieces(&address);
                let text: String = pieces.iter().map(|piece| piece.text.as_str()).collect();
                SpellSource {
                    address,
                    pieces,
                    text,
                }
            })
            .collect();
        let map = self.map.clone();
        let revision = self.session.revision();
        let replies = self.replies.clone();
        let ctx = self.ctx.clone();
        let sync = Arc::clone(&self.spell_sync);
        let spawned = std::thread::Builder::new()
            .name("mufoed-spelling".to_owned())
            .spawn(move || {
                let Some(done) = spell_document(
                    &speller,
                    &paragraphs,
                    map.as_deref(),
                    revision,
                    &cache,
                    &sync,
                    epoch,
                ) else {
                    return;
                };
                let mut gate = spell_gate(&sync);
                if !publish_spelling(&mut gate, epoch, done.cache) {
                    return;
                }
                let _ = replies.send(Reply::Spelling {
                    errors: done.errors,
                    missing: done.missing,
                });
                ctx.request_repaint();
            });
        if spawned.is_err() {
            self.spell_dirty = true;
        }
    }

    // ----- comments -----------------------------------------------------

    /// Each comment's marks in the body: its range start, range end and
    /// reference, as (paragraph, top-level inline index).
    fn comment_marks(&self, id: &str) -> Vec<(Address, usize, CommentMark)> {
        use strict_ooxml::model::{Inline, RunContent};
        let mut out = Vec::new();
        for address in self.paragraphs(&Story::Body) {
            let Some(paragraph) = self.paragraph(&address) else {
                continue;
            };
            for (index, inline) in paragraph.inlines.iter().enumerate() {
                let mark = match inline {
                    Inline::CommentRangeStart(c) if c.as_str() == id => Some(CommentMark::Start),
                    Inline::CommentRangeEnd(c) if c.as_str() == id => Some(CommentMark::End),
                    Inline::CommentReference(c) if c.as_str() == id => Some(CommentMark::Reference),
                    Inline::Run(run)
                        if run.content.len() == 1
                            && matches!(run.content.first(), Some(RunContent::CommentReference(c)) if c.to_string() == id) =>
                    {
                        Some(CommentMark::Reference)
                    }
                    _ => None,
                };
                if let Some(mark) = mark {
                    out.push((address.clone(), index, mark));
                }
            }
        }
        out
    }

    /// The text a comment is about, as carets: from its range start to its
    /// range end (or its reference).
    fn comment_range(&self, id: &str) -> Option<(Logical, Logical)> {
        let marks = self.comment_marks(id);
        let at = |kind: CommentMark| {
            marks
                .iter()
                .find(|(_, _, mark)| *mark == kind)
                .map(|(paragraph, index, _)| Logical {
                    offset: self.offset_of_inline(paragraph, *index),
                    paragraph: paragraph.clone(),
                })
        };
        let end = at(CommentMark::End).or_else(|| at(CommentMark::Reference))?;
        let start = at(CommentMark::Start).unwrap_or_else(|| end.clone());
        Some((start, end))
    }

    fn comment_views(&self) -> Vec<CommentView> {
        let document = self.session.document();
        let revision = self.session.revision();
        let mut views: Vec<CommentView> = document
            .comments
            .iter()
            .map(|comment| {
                let id = comment.id.as_str().to_owned();
                let rects = self
                    .comment_range(&id)
                    .zip(self.map.as_ref())
                    .map(|((start, end), map)| {
                        let (a, b) = (self.position(&start), self.position(&end));
                        if start == end {
                            map.caret(revision, &a).unwrap_or_default()
                        } else {
                            map.selection_between(revision, &a, &b).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default();
                let first = rects.first().copied();
                CommentView {
                    text: comment_text(&comment.blocks),
                    author: comment.author.as_deref().unwrap_or_default().to_owned(),
                    date: comment.date.as_deref().unwrap_or_default().to_owned(),
                    page: first.map_or(usize::MAX, |r| r.page),
                    y: first.map_or(0.0, |r| r.y),
                    rects,
                    id,
                }
            })
            .collect();
        views.sort_by(|a, b| {
            (a.page, a.y)
                .partial_cmp(&(b.page, b.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        views
    }

    fn comments(&mut self, command: CommentCommand) -> Outcome {
        use strict_ooxml::edit::{comment_blocks, new_comment, Definition};
        use strict_ooxml::model::{CommentId, Inline};
        match command {
            CommentCommand::Add { author, initials } => {
                let Some(caret) = self.caret.clone() else {
                    return Outcome::Quiet;
                };
                if caret.paragraph.story != Story::Body {
                    self.note("note.comments_in_body", String::new(), false);
                    return Outcome::Quiet;
                }
                // The selection, else the word at the caret.
                let (start, end) = self.selection().unwrap_or_else(|| {
                    let text = self.text_of(&caret.paragraph);
                    nav::word_at(&text, caret.offset).map_or_else(
                        || (caret.clone(), caret.clone()),
                        |range| {
                            (
                                Logical {
                                    paragraph: caret.paragraph.clone(),
                                    offset: range.start,
                                },
                                Logical {
                                    paragraph: caret.paragraph.clone(),
                                    offset: range.end,
                                },
                            )
                        },
                    )
                });
                let comment =
                    new_comment(self.session.document(), &author, &initials, &now_iso(), "");
                let id = comment.id.clone();
                let end_marks = vec![
                    Inline::CommentRangeEnd(id.clone()),
                    Inline::CommentReference(id.clone()),
                ];
                let commands = if start == end {
                    let mut marks = vec![Inline::CommentRangeStart(id.clone())];
                    marks.extend(end_marks);
                    self.inlines_at_caret(&start, marks)
                } else {
                    let mut commands = self.inlines_at_caret(&end, end_marks);
                    commands.extend(
                        self.inline_at_caret(&start, Inline::CommentRangeStart(id.clone())),
                    );
                    commands
                };
                let outcome = self
                    .define_and_transact(&[Definition::AddComment(Box::new(comment))], &commands);
                if matches!(outcome, Outcome::Edited) {
                    self.anchor = None;
                    self.send(Reply::CommentAdded(id.as_str().to_owned()));
                }
                outcome
            }
            CommentCommand::Edit { id, text } => {
                let Some(mut comment) = self
                    .session
                    .document()
                    .comments
                    .iter()
                    .find(|c| c.id.as_str() == id)
                    .cloned()
                else {
                    return Outcome::Quiet;
                };
                if comment_text(&comment.blocks) == text {
                    return Outcome::Quiet;
                }
                comment.blocks = comment_blocks(&text);
                self.define_and_transact(&[Definition::ReplaceComment(Box::new(comment))], &[])
            }
            CommentCommand::Delete(id) => {
                let paragraphs = self.paragraphs(&Story::Body);
                let order = |a: &Address| paragraphs.iter().position(|p| p == a).unwrap_or(0);
                let mut marks = self.comment_marks(&id);
                marks.sort_by_key(|(at, index, _)| std::cmp::Reverse((order(at), *index)));
                let commands: Vec<Edit> = marks
                    .into_iter()
                    .map(|(at, index, _)| Edit::DeleteInline { at, index })
                    .collect();
                self.define_and_transact(
                    &[Definition::DeleteComment(CommentId::new(id))],
                    &commands,
                )
            }
            CommentCommand::Select(id) => self.select_comment(&id),
            CommentCommand::Step { back } => {
                let views = self.comment_views();
                let here = self
                    .caret
                    .as_ref()
                    .and_then(|caret| {
                        let revision = self.session.revision();
                        self.map
                            .as_ref()?
                            .caret(revision, &self.position(caret))
                            .ok()?
                            .first()
                            .map(|r| (r.page, r.y))
                    })
                    .unwrap_or((0, 0.0));
                let target = if back {
                    views.iter().rev().find(|v| (v.page, v.y) < here)
                } else {
                    views
                        .iter()
                        .find(|v| (v.page, v.y) > (here.0, here.1 + 0.5))
                };
                match target.map(|v| v.id.clone()) {
                    Some(id) => self.select_comment(&id),
                    None => Outcome::Quiet,
                }
            }
        }
    }

    fn select_comment(&mut self, id: &str) -> Outcome {
        let Some((start, end)) = self.comment_range(id) else {
            return Outcome::Quiet;
        };
        self.story = Story::Body;
        self.object = None;
        self.cells = None;
        self.anchor = (start != end).then_some(start);
        self.caret = Some(end);
        self.scroll_at_top = true;
        self.send(Reply::CommentFocused(id.to_owned()));
        Outcome::Moved
    }

    // ----- references ---------------------------------------------------

    fn references(&mut self, command: ReferenceCommand) -> Outcome {
        match command {
            ReferenceCommand::InsertNote { endnote } => self.insert_note(endnote),
            ReferenceCommand::NextNote { back } => self.next_note(back),
            ReferenceCommand::AddBookmark(name) => self.add_bookmark(&name),
            ReferenceCommand::GoToBookmark(name) => self.go_to_bookmark(&name),
            ReferenceCommand::DeleteBookmark(name) => self.delete_bookmark(&name),
            ReferenceCommand::TableOfContents { title } => self.table_of_contents(&title),
            ReferenceCommand::RemoveTableOfContents => match self.toc_block() {
                Some(block) => {
                    if self.transact(&[Edit::Delete {
                        at: Address::body(block),
                    }]) {
                        Outcome::Edited
                    } else {
                        Outcome::Quiet
                    }
                }
                None => Outcome::Quiet,
            },
        }
    }

    /// A footnote or endnote at the caret: the reference goes in between
    /// runs (split, insert, join), the caret into the note's text.
    fn insert_note(&mut self, endnote: bool) -> Outcome {
        if self.selection().is_some() {
            self.anchor = None;
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        if caret.paragraph.story != Story::Body {
            self.note("note.notes_in_body", String::new(), false);
            return Outcome::Quiet;
        }
        let at = caret.paragraph.clone();
        let before: std::collections::BTreeSet<i32> = self.note_ids(endnote);
        let blocks = Vec::new();
        let commands = if let Some(point) = self.point(&caret) {
            {
                let mut right = at.clone();
                right.block += 1;
                vec![
                    Edit::SplitAt {
                        at: at.clone(),
                        point,
                    },
                    Edit::InsertNote {
                        at: right,
                        index: 0,
                        endnote,
                        blocks,
                    },
                    Edit::Join { at: at.clone() },
                ]
            }
        } else {
            {
                let pieces = self.pieces(&at);
                let mut offset = 0;
                let mut index = 0;
                for piece in &pieces {
                    offset += piece.len();
                    if offset == caret.offset {
                        index = piece.inline.first().copied().unwrap_or(0) + 1;
                    }
                }
                vec![Edit::InsertNote {
                    at: at.clone(),
                    index,
                    endnote,
                    blocks,
                }]
            }
        };
        if !self.transact(&commands) {
            return Outcome::Quiet;
        }
        // The new note is the id that was not there before.
        let id = self.note_ids(endnote).difference(&before).next().copied();
        if let Some(id) = id {
            let story = if endnote {
                Story::Endnote(id)
            } else {
                Story::Footnote(id)
            };
            self.story = story.clone();
            self.caret = self.paragraphs(&story).last().cloned().map(|paragraph| {
                let offset = self.text_of(&paragraph).chars().count();
                Logical { paragraph, offset }
            });
        }
        Outcome::Edited
    }

    fn note_ids(&self, endnote: bool) -> std::collections::BTreeSet<i32> {
        let document = self.session.document();
        let table = if endnote {
            &document.endnotes
        } else {
            &document.footnotes
        };
        table
            .iter()
            .map(|note| note.id)
            .filter(|id| *id > 0)
            .collect()
    }

    /// The body paragraphs with a note reference, and each reference's
    /// caret offset.
    fn note_references(&self) -> Vec<(Address, usize, Story)> {
        use strict_ooxml::model::{Inline, RunContent};
        let mut out = Vec::new();
        for address in self.paragraphs(&Story::Body) {
            let Some(paragraph) = self.paragraph(&address) else {
                continue;
            };
            let mut offset = 0;
            let pieces = self.pieces(&address);
            for (index, inline) in paragraph.inlines.iter().enumerate() {
                let note = match inline {
                    Inline::Run(run) => run.content.iter().find_map(|c| match c {
                        RunContent::FootnoteRef(id) => {
                            Some(Story::Footnote(i32::try_from(*id).unwrap_or(0)))
                        }
                        RunContent::EndnoteRef(id) => {
                            Some(Story::Endnote(i32::try_from(*id).unwrap_or(0)))
                        }
                        _ => None,
                    }),
                    Inline::FootnoteRef(id) => {
                        Some(Story::Footnote(i32::try_from(*id).unwrap_or(0)))
                    }
                    Inline::EndnoteRef(id) => Some(Story::Endnote(i32::try_from(*id).unwrap_or(0))),
                    _ => None,
                };
                if let Some(note) = note {
                    offset = pieces
                        .iter()
                        .filter(|piece| piece.inline.first().is_some_and(|i| *i < index))
                        .map(nav::Piece::len)
                        .sum();
                    out.push((address.clone(), offset, note));
                }
            }
            let _ = offset;
        }
        out
    }

    fn next_note(&mut self, back: bool) -> Outcome {
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let paragraphs = self.paragraphs(&Story::Body);
        let order = |at: &Address| paragraphs.iter().position(|a| a == at).unwrap_or(0);
        let references = self.note_references();
        // In a note, its reference is where the caret counts from; going back
        // from a note lands on its own reference.
        let own = references.iter().find(|(_, _, note)| *note == self.story);
        let here = own.map_or(
            (order(&caret.paragraph), caret.offset),
            |(at, offset, _)| (order(at), *offset + usize::from(back)),
        );
        let target = if back {
            references
                .iter()
                .rev()
                .find(|(at, offset, _)| (order(at), *offset) < here)
        } else {
            references
                .iter()
                .find(|(at, offset, _)| (order(at), *offset) > here)
        };
        let Some((paragraph, offset, _)) = target.cloned() else {
            return Outcome::Quiet;
        };
        self.story = Story::Body;
        self.anchor = None;
        self.caret = Some(Logical { paragraph, offset });
        self.scroll_at_top = true;
        Outcome::Moved
    }

    /// Every bookmark of the body: name, paragraph, top-level inline index
    /// of its start, and the same of its end.
    fn bookmark_marks(&self) -> Vec<BookmarkMarks> {
        use strict_ooxml::model::Inline;
        let mut starts = Vec::new();
        let mut ends: Vec<(strict_ooxml::model::BookmarkId, Address, usize)> = Vec::new();
        for address in self.paragraphs(&Story::Body) {
            let Some(paragraph) = self.paragraph(&address) else {
                continue;
            };
            for (index, inline) in paragraph.inlines.iter().enumerate() {
                match inline {
                    Inline::BookmarkStart(mark) => {
                        starts.push((
                            mark.id.clone(),
                            mark.name.to_string(),
                            address.clone(),
                            index,
                        ));
                    }
                    Inline::BookmarkEnd(id) => ends.push((id.clone(), address.clone(), index)),
                    _ => {}
                }
            }
        }
        starts
            .into_iter()
            .map(|(id, name, at, index)| {
                let end = ends
                    .iter()
                    .find(|(end, _, _)| *end == id)
                    .map(|(_, at, index)| (at.clone(), *index));
                (name, at, index, end)
            })
            .collect()
    }

    fn bookmarks_and_toc(&self) -> (Vec<String>, bool) {
        let mut names: Vec<String> = self
            .bookmark_marks()
            .into_iter()
            .map(|(name, ..)| name)
            .filter(|name| !name.starts_with('_'))
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        (names, self.toc_block().is_some())
    }

    /// The caret offset just before a top-level inline.
    fn offset_of_inline(&self, at: &Address, index: usize) -> usize {
        self.pieces(at)
            .iter()
            .filter(|piece| piece.inline.first().is_some_and(|i| *i < index))
            .map(nav::Piece::len)
            .sum()
    }

    /// A bookmark around the selection, or at the caret: its end first, so
    /// the start's place does not move.
    fn add_bookmark(&mut self, name: &str) -> Outcome {
        use strict_ooxml::model::{Bookmark, BookmarkId, Inline};
        let name = name.trim();
        if !valid_bookmark_name(name) {
            self.note("note.bookmark_name", name.to_owned(), true);
            return Outcome::Quiet;
        }
        if self
            .bookmark_marks()
            .iter()
            .any(|(existing, ..)| existing.eq_ignore_ascii_case(name))
        {
            self.delete_bookmark(name);
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let (start, end) = self.selection().unwrap_or_else(|| (caret.clone(), caret));
        let id = self
            .bookmark_ids()
            .into_iter()
            .max()
            .map_or(0, |top| top + 1);
        let start_mark = Inline::BookmarkStart(Bookmark {
            id: BookmarkId::new(id.to_string()),
            name: std::sync::Arc::from(name),
            col_first: None,
            col_last: None,
        });
        let end_mark = Inline::BookmarkEnd(BookmarkId::new(id.to_string()));
        // At one point both go in together; else the end first, so the
        // start's place does not move.
        let commands = if start == end {
            self.inlines_at_caret(&start, vec![start_mark, end_mark])
        } else {
            let mut commands = self.inline_at_caret(&end, end_mark);
            commands.extend(self.inline_at_caret(&start, start_mark));
            commands
        };
        if self.transact(&commands) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn bookmark_ids(&self) -> Vec<u32> {
        use strict_ooxml::model::Inline;
        let mut ids = Vec::new();
        for address in self.paragraphs(&Story::Body) {
            if let Some(paragraph) = self.paragraph(&address) {
                for inline in &paragraph.inlines {
                    if let Inline::BookmarkStart(mark) = inline {
                        if let Ok(id) = mark.id.as_str().parse::<u32>() {
                            ids.push(id);
                        }
                    }
                }
            }
        }
        ids
    }

    fn go_to_bookmark(&mut self, name: &str) -> Outcome {
        let Some((_, at, index, end)) = self
            .bookmark_marks()
            .into_iter()
            .find(|(existing, ..)| existing.eq_ignore_ascii_case(name))
        else {
            return Outcome::Quiet;
        };
        self.scroll_at_top = true;
        let start = Logical {
            offset: self.offset_of_inline(&at, index),
            paragraph: at,
        };
        self.story = Story::Body;
        self.anchor = None;
        if let Some((end_at, end_index)) = end {
            let end = Logical {
                offset: self.offset_of_inline(&end_at, end_index),
                paragraph: end_at,
            };
            if end != start {
                self.anchor = Some(end);
                self.caret = Some(start);
                return Outcome::Moved;
            }
        }
        self.caret = Some(start);
        Outcome::Moved
    }

    fn delete_bookmark(&mut self, name: &str) -> Outcome {
        let Some((_, at, index, end)) = self
            .bookmark_marks()
            .into_iter()
            .find(|(existing, ..)| existing.eq_ignore_ascii_case(name))
        else {
            return Outcome::Quiet;
        };
        // The later mark first, so the earlier one's index holds.
        let mut marks = vec![(at, index)];
        marks.extend(end);
        let paragraphs = self.paragraphs(&Story::Body);
        let order = |a: &Address| paragraphs.iter().position(|p| p == a).unwrap_or(0);
        marks.sort_by_key(|(a, i)| std::cmp::Reverse((order(a), *i)));
        let commands: Vec<Edit> = marks
            .into_iter()
            .map(|(at, index)| Edit::DeleteInline { at, index })
            .collect();
        if self.transact(&commands) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// The body block of the table of contents, when there is one: a
    /// content control of the "Table of Contents" gallery, as Word makes it.
    fn toc_block(&self) -> Option<usize> {
        self.session
            .document()
            .body
            .blocks
            .iter()
            .position(|block| matches!(block, Block::SdtBlock(sdt) if sdt.doc_part_gallery.as_deref() == Some("Table of Contents")))
    }

    /// A table of contents of the headings: one paragraph per heading,
    /// indented by its level, its page after a dotted tab. An existing one
    /// is made again in its place; else it goes before the caret's
    /// paragraph.
    #[allow(clippy::too_many_lines)]
    fn table_of_contents(&mut self, title: &str) -> Outcome {
        use strict_ooxml::model::values::{Indentation, TabAlignment, TabLeader, TabStop};
        use strict_ooxml::model::{Inline, Run, RunContent, Space, TextNode};
        self.ensure_layout();
        let existing = self.toc_block();
        let at_block = existing.or_else(|| {
            self.caret
                .as_ref()
                .filter(|c| c.paragraph.story == Story::Body)
                .map(|c| {
                    c.paragraph
                        .containers
                        .first()
                        .map_or(c.paragraph.block, |container| match container {
                            strict_ooxml::edit::Container::Cell { table, .. } => *table,
                            strict_ooxml::edit::Container::Sdt(index) => *index,
                            strict_ooxml::edit::Container::TextBox { paragraph, .. } => *paragraph,
                        })
                })
        });
        let Some(at_block) = at_block else {
            return Outcome::Quiet;
        };
        let revision = self.session.revision();
        let headings: Vec<(u8, String, usize)> = self
            .build_outline()
            .into_iter()
            .filter(|entry| entry.level < 3)
            .map(|entry| {
                let position = self.position(&Logical {
                    paragraph: entry.paragraph.clone(),
                    offset: 0,
                });
                let page = self
                    .map
                    .as_ref()
                    .and_then(|map| map.caret(revision, &position).ok())
                    .and_then(|rects| rects.first().map(|r| r.page + 1))
                    .unwrap_or(1);
                (entry.level, entry.text, page)
            })
            .collect();
        let text_width = self.text_width();
        let style_of = |level: u8| {
            let id = strict_ooxml::model::StyleId::new(format!("TOC{}", level + 1));
            self.session.document().styles.contains(&id).then_some(id)
        };
        let paragraph = |inlines: Vec<Inline>, properties: ParagraphProperties| {
            strict_ooxml::model::Paragraph {
                props: properties,
                inlines,
                rsids: strict_ooxml::model::Rsids::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location: SourceLocation::unknown(),
            }
        };
        let run = |text: &str, bold: bool| {
            let mut props = strict_ooxml::model::props::RunProperties::default();
            if bold {
                props.bold = TriState::On;
                props.size = Some(HalfPoints(32));
            }
            Inline::Run(Run {
                props,
                content: vec![RunContent::Text(TextNode {
                    text: text.to_owned(),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::unknown(),
            })
        };
        let mut blocks = Vec::new();
        let mut heading_props = ParagraphProperties::default();
        let toc_heading = strict_ooxml::model::StyleId::new("TOCHeading");
        if self.session.document().styles.contains(&toc_heading) {
            heading_props.style = Some(toc_heading);
            blocks.push(Block::Paragraph(paragraph(
                vec![run(title, false)],
                heading_props,
            )));
        } else {
            blocks.push(Block::Paragraph(paragraph(
                vec![run(title, true)],
                heading_props,
            )));
        }
        for (level, text, page) in &headings {
            let mut properties = ParagraphProperties::default();
            match style_of(*level) {
                Some(style) => properties.style = Some(style),
                None => {
                    properties.indentation = Some(Indentation {
                        start: Some(Twips(220 * i32::from(*level))),
                        ..Indentation::default()
                    });
                }
            }
            properties.tabs = vec![TabStop {
                position: Twips(text_width.max(1440)),
                alignment: TabAlignment::End,
                leader: Some(TabLeader::Dot),
            }];
            let entry = vec![
                run(text, false),
                Inline::Run(Run {
                    props: strict_ooxml::model::props::RunProperties::default(),
                    content: vec![RunContent::Tab],
                    revision: None,
                    location: SourceLocation::unknown(),
                }),
                run(&page.to_string(), false),
            ];
            blocks.push(Block::Paragraph(paragraph(entry, properties)));
        }
        if headings.is_empty() {
            blocks.push(Block::Paragraph(paragraph(
                Vec::new(),
                ParagraphProperties::default(),
            )));
        }
        let sdt = strict_ooxml::model::SdtContainer {
            tag: None,
            alias: None,
            id: None,
            placeholder: None,
            showing_placeholder: false,
            run_props: None,
            end_run_props: None,
            has_end_pr: false,
            doc_part_gallery: Some(std::sync::Arc::from("Table of Contents")),
            doc_part_unique: true,
            doc_part_category: None,
            lock: None,
            temporary: false,
            data_binding: None,
            label: None,
            tab_index: None,
            control: None,
            unmodelled: Vec::new(),
            blocks,
            inlines: Vec::new(),
            location: SourceLocation::unknown(),
        };
        let mut commands = Vec::new();
        if existing.is_some() {
            commands.push(Edit::Delete {
                at: Address::body(at_block),
            });
        }
        commands.push(Edit::Insert {
            at: Address::body(at_block),
            block: Box::new(Block::SdtBlock(sdt)),
        });
        if self.transact(&commands) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// The heading a table of contents entry names, for a Ctrl+click on it.
    fn toc_target(&self, at: &Address) -> Option<Address> {
        let block = self.toc_block()?;
        let inside = at.story == Story::Body
            && matches!(at.containers.first(), Some(strict_ooxml::edit::Container::Sdt(index)) if *index == block);
        if !inside {
            return None;
        }
        let text = self.text_of(at);
        let entry = text.split('\t').next()?.trim().to_owned();
        self.build_outline()
            .into_iter()
            .find(|heading| heading.text.trim() == entry)
            .map(|heading| heading.paragraph)
    }

    // ----- headers and footers -------------------------------------------

    /// The section a page shows, by index.
    fn page_section(&self, page: usize) -> usize {
        self.pages
            .get(page)
            .map_or(0, |data| data.placed.section_index)
    }

    /// Whether a page point is in the header band (`Some(true)`), the
    /// footer band, or neither: above the top margin or below the bottom.
    fn margin_band(&self, page: usize, y: f64) -> Option<bool> {
        let section = self
            .session
            .document()
            .sections
            .get(self.page_section(page))?
            .properties
            .clone();
        let height = self.pages.get(page)?.height;
        let margins = section.page_margins.unwrap_or_default();
        let top = f64::from(margins.top.map_or(1440, |t| t.0.abs())) / 15.0;
        let bottom = f64::from(margins.bottom.map_or(1440, |t| t.0.abs())) / 15.0;
        if y < top {
            Some(true)
        } else if y > height - bottom {
            Some(false)
        } else {
            None
        }
    }

    /// The header (or footer) part of `kind` a section uses: its own, or
    /// the one it takes from the sections before it.
    fn header_footer_of(
        &self,
        section: usize,
        header: bool,
        kind: strict_ooxml::model::props::HeaderFooterKind,
    ) -> Option<strict_ooxml_core::part::PartId> {
        let sections = &self.session.document().sections;
        (0..=section).rev().find_map(|index| {
            let properties = &sections.get(index)?.properties;
            let references = if header {
                &properties.headers
            } else {
                &properties.footers
            };
            references
                .iter()
                .find(|r| r.kind == kind)
                .and_then(|r| r.part.clone())
        })
    }

    /// Which header or footer a page shows: the first page's when its
    /// section has one of its own (`w:titlePg`) and it is the section's first
    /// page, the even pages' when the document has them and the page is even,
    /// else the default one.
    fn page_kind(&self, page: usize) -> strict_ooxml::model::props::HeaderFooterKind {
        use strict_ooxml::model::props::HeaderFooterKind;
        let section = self.page_section(page);
        let document = self.session.document();
        let title_page = document
            .sections
            .get(section)
            .is_some_and(|s| s.properties.title_page);
        let first_of_section = page == 0 || self.page_section(page - 1) != section;
        if title_page && first_of_section {
            HeaderFooterKind::First
        } else if document.settings.even_and_odd_headers && (page + 1).is_multiple_of(2) {
            HeaderFooterKind::Even
        } else {
            HeaderFooterKind::Default
        }
    }

    /// A body paragraph of a section, for editing its properties.
    fn section_paragraph(&self, section: usize) -> Option<Address> {
        let mut index = 0;
        for (block, item) in self.session.document().body.blocks.iter().enumerate() {
            if let Block::Paragraph(paragraph) = item {
                if index == section {
                    return Some(Address::body(block));
                }
                if paragraph.props.section.is_some() {
                    index += 1;
                }
            }
        }
        None
    }

    /// The header (or footer) of a section, made (empty, for every later
    /// section that has none of its own) when it has none.
    fn ensure_header_footer(
        &mut self,
        section: usize,
        header: bool,
        kind: strict_ooxml::model::props::HeaderFooterKind,
    ) -> Option<strict_ooxml_core::part::PartId> {
        use strict_ooxml::edit::{header_footer_reference, new_header_footer, Definition};
        if let Some(part) = self.header_footer_of(section, header, kind) {
            return Some(part);
        }
        let at = self.section_paragraph(section)?;
        let mut properties = self
            .session
            .document()
            .sections
            .get(section)?
            .properties
            .clone();
        let made = new_header_footer(self.session.document(), header);
        let part = made.part.clone();
        let reference = header_footer_reference(&part, kind);
        if header {
            properties.headers.push(reference);
        } else {
            properties.footers.push(reference);
        }
        let outcome = self.define_and_transact(
            &[Definition::AddHeaderFooter(Box::new(made))],
            &[Edit::SectionProperties {
                at,
                properties: Box::new(properties),
            }],
        );
        matches!(outcome, Outcome::Edited).then_some(part)
    }

    /// Starts editing the header (or footer) of a page's section, the caret
    /// at the point clicked when it is in its text, else at its end.
    fn enter_header_footer(
        &mut self,
        page: usize,
        header: bool,
        point: Option<(f64, f64)>,
    ) -> Outcome {
        let section = self.page_section(page);
        let kind = self.page_kind(page);
        let Some(part) = self.ensure_header_footer(section, header, kind) else {
            return Outcome::Quiet;
        };
        self.ensure_layout();
        let story = Story::HeaderFooter(part);
        if !matches!(self.story, Story::HeaderFooter(_)) {
            self.body_caret.clone_from(&self.caret);
        }
        self.story = story.clone();
        self.header_page = Some(page);
        self.object = None;
        self.cells = None;
        self.anchor = None;
        let revision = self.session.revision();
        let hit = point.and_then(|(x, y)| {
            let map = self.map.as_ref()?;
            map.hit_test(revision, page, x, y)
                .ok()
                .flatten()
                .filter(|position| position.paragraph.story == story)
        });
        self.caret = match hit {
            Some(position) => Some(self.logical(&position)),
            None => self.paragraphs(&story).last().cloned().map(|paragraph| {
                let offset = self.text_of(&paragraph).chars().count();
                Logical { paragraph, offset }
            }),
        };
        Outcome::Edited
    }

    fn exit_header_footer(&mut self) -> Outcome {
        if !matches!(self.story, Story::HeaderFooter(_)) {
            return Outcome::Quiet;
        }
        self.story = Story::Body;
        self.header_page = None;
        self.anchor = None;
        self.caret = self.body_caret.take().or_else(|| {
            self.first_paragraph().map(|paragraph| Logical {
                paragraph,
                offset: 0,
            })
        });
        Outcome::Moved
    }

    fn header_footer_view(&self) -> Option<HeaderFooterView> {
        let Story::HeaderFooter(part) = &self.story else {
            return None;
        };
        let header = self.session.document().header_footer(part)?.is_header;
        let sections = &self.session.document().sections;
        let kind = sections
            .iter()
            .flat_map(|s| {
                s.properties
                    .headers
                    .iter()
                    .chain(s.properties.footers.iter())
            })
            .find(|r| r.part.as_ref() == Some(part))
            .map_or(strict_ooxml::model::props::HeaderFooterKind::Default, |r| {
                r.kind
            });
        let section = self.caret_page_section();
        let first_page = sections
            .get(section)
            .is_some_and(|s| s.properties.title_page);
        let even_odd = self.session.document().settings.even_and_odd_headers;
        let body = self
            .pages
            .iter()
            .map(|page| {
                let margins = sections
                    .get(page.placed.section_index)
                    .and_then(|s| s.properties.page_margins)
                    .unwrap_or_default();
                let top = f64::from(margins.top.map_or(1440, |t| t.0.abs())) / 15.0;
                let bottom = f64::from(margins.bottom.map_or(1440, |t| t.0.abs())) / 15.0;
                (top, page.height - bottom)
            })
            .collect();
        Some(HeaderFooterView {
            header,
            kind,
            first_page,
            even_odd,
            body,
        })
    }

    fn header_footer(&mut self, command: HeaderFooterCommand) -> Outcome {
        match command {
            HeaderFooterCommand::Enter { page, header } => {
                self.enter_header_footer(page, header, None)
            }
            HeaderFooterCommand::Exit => self.exit_header_footer(),
            HeaderFooterCommand::Insert {
                top,
                align,
                of_total,
            } => self.insert_page_numbers(top, align, of_total.as_ref()),
            HeaderFooterCommand::Remove => self.remove_page_numbers(),
            HeaderFooterCommand::FirstPage(on) => {
                let Some(at) = self.section_anchor_of_story() else {
                    return Outcome::Quiet;
                };
                let mut properties = self.section_of(at.block);
                properties.title_page = on;
                if self.transact(&[Edit::SectionProperties {
                    at,
                    properties: Box::new(properties),
                }]) {
                    Outcome::Edited
                } else {
                    Outcome::Quiet
                }
            }
            HeaderFooterCommand::EvenOdd(on) => self.define_and_transact(
                &[strict_ooxml::edit::Definition::EvenAndOddHeaders(on)],
                &[],
            ),
            HeaderFooterCommand::Start(start) => {
                let Some(at) = self.section_anchor() else {
                    return Outcome::Quiet;
                };
                let mut properties = self.section_of(at.block);
                let mut numbering = properties.page_number.clone().unwrap_or_default();
                numbering.start = start;
                properties.page_number = Some(numbering);
                if self.transact(&[Edit::SectionProperties {
                    at,
                    properties: Box::new(properties),
                }]) {
                    Outcome::Edited
                } else {
                    Outcome::Quiet
                }
            }
        }
    }

    /// Puts the page number in the header (or footer) of the caret's
    /// section, in a paragraph of its own aligned as asked: the header's only
    /// paragraph when it is empty, else a new one after its text.
    fn insert_page_numbers(
        &mut self,
        top: bool,
        align: Align,
        of_total: Option<&(String, String)>,
    ) -> Outcome {
        use strict_ooxml::model::props::HeaderFooterKind;
        // Every kind the section's pages show gets the number: the default
        // and even pages always, the first page when the caret is on it.
        let page = self.caret_page();
        let section = self.page_section(page);
        let mut kinds = vec![HeaderFooterKind::Default];
        if self.session.document().settings.even_and_odd_headers {
            kinds.push(HeaderFooterKind::Even);
        }
        if self.page_kind(page) == HeaderFooterKind::First {
            kinds.push(HeaderFooterKind::First);
        }
        let mut edited = false;
        for kind in kinds {
            if let Some(part) = self.ensure_header_footer(section, top, kind) {
                edited |= self.number_story(Story::HeaderFooter(part), align, of_total);
            }
        }
        if edited {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// Puts the page number in a header or footer story.
    fn number_story(
        &mut self,
        story: Story,
        align: Align,
        of_total: Option<&(String, String)>,
    ) -> bool {
        use strict_ooxml::model::{Field, Inline, Run, RunContent, Space, TextNode};
        let paragraphs = self.paragraphs(&story);
        let text = |value: &str| {
            Inline::Run(Run {
                props: strict_ooxml::model::props::RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: value.to_owned(),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::unknown(),
            })
        };
        let field = |instruction: &str, shown: &str| {
            Inline::Field(Field {
                instruction: Some(std::sync::Arc::from(instruction)),
                inlines: vec![text(shown)],
                location: SourceLocation::unknown(),
            })
        };
        let mut inlines = Vec::new();
        match of_total {
            Some((before, between)) => {
                if !before.is_empty() {
                    inlines.push(text(before));
                }
                inlines.push(field(" PAGE ", "1"));
                inlines.push(text(between));
                inlines.push(field(" NUMPAGES ", "1"));
            }
            None => inlines.push(field(" PAGE ", "1")),
        }
        let alignment = match align {
            Align::Left => Justification::Start,
            Align::Center => Justification::Center,
            Align::Right => Justification::End,
            Align::Justify => Justification::Both,
        };
        let empty = paragraphs.len() == 1
            && paragraphs
                .first()
                .is_some_and(|only| self.paragraph(only).is_some_and(|p| p.inlines.is_empty()));
        let commands = if let (true, Some(only)) = (empty, paragraphs.first()) {
            let Some(paragraph) = self.paragraph(only) else {
                return false;
            };
            let mut properties = paragraph.props.clone();
            properties.alignment = Some(alignment);
            let mut commands = vec![Edit::ParagraphProperties {
                at: only.clone(),
                properties: Box::new(properties),
            }];
            for (index, inline) in inlines.into_iter().enumerate() {
                commands.push(Edit::InsertInline {
                    at: only.clone(),
                    index,
                    inline: Box::new(inline),
                });
            }
            commands
        } else {
            let mut paragraph = strict_ooxml::model::Paragraph {
                props: ParagraphProperties::default(),
                inlines,
                rsids: strict_ooxml::model::Rsids::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location: SourceLocation::unknown(),
            };
            paragraph.props.alignment = Some(alignment);
            vec![Edit::Insert {
                at: Address {
                    story,
                    containers: Vec::new(),
                    block: paragraphs.len(),
                },
                block: Box::new(Block::Paragraph(paragraph)),
            }]
        };
        self.transact(&commands)
    }

    /// The section of the page the caret is on.
    fn caret_page_section(&self) -> usize {
        self.page_section(self.caret_page())
    }

    /// A body paragraph of the section the caret's page shows (the caret may
    /// be in a header).
    fn section_anchor_of_story(&self) -> Option<Address> {
        if self.story == Story::Body {
            return self.section_anchor();
        }
        self.section_paragraph(self.caret_page_section())
    }

    /// The page the caret is drawn on.
    fn caret_page(&self) -> usize {
        self.caret_rects().first().map_or(0, |rect| rect.page)
    }

    /// Takes the page numbers (and their paragraphs, when nothing else is
    /// in them) out of every header and footer.
    fn remove_page_numbers(&mut self) -> Outcome {
        use strict_ooxml::model::Inline;
        let is_number = |inline: &Inline| {
            matches!(inline, Inline::Field(field) if field.instruction.as_deref().is_some_and(|i| {
                let i = i.trim().to_ascii_uppercase();
                i.starts_with("PAGE") || i.starts_with("NUMPAGES") || i.starts_with("SECTIONPAGES")
            }))
        };
        let parts: Vec<_> = self
            .session
            .document()
            .headers_footers
            .iter()
            .map(|h| h.part.clone())
            .collect();
        let mut commands = Vec::new();
        for part in parts {
            let story = Story::HeaderFooter(part);
            let paragraphs = self.paragraphs(&story);
            let single = paragraphs.len() == 1;
            for at in paragraphs.iter().rev() {
                let Some(paragraph) = self.paragraph(at) else {
                    continue;
                };
                if !paragraph.inlines.iter().any(is_number) {
                    continue;
                }
                if at.containers.is_empty() && !single {
                    commands.push(Edit::Delete { at: at.clone() });
                    continue;
                }
                for (index, inline) in paragraph.inlines.iter().enumerate().rev() {
                    if is_number(inline) {
                        commands.push(Edit::DeleteInline {
                            at: at.clone(),
                            index,
                        });
                    }
                }
            }
        }
        if self.transact(&commands) {
            if self.story != Story::Body {
                return self.exit_header_footer();
            }
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// The page size, margins and caret paragraph indents for the rulers.
    fn ruler_info(&self, page: usize) -> Option<RulerInfo> {
        let caret = self.caret.as_ref()?;
        // The section the page shows.
        let section = self
            .pages
            .get(page)
            .map(|data| data.placed.section_index)
            .and_then(|index| self.session.document().sections.get(index))
            .map(|section| section.properties.clone())
            .or_else(|| self.section_anchor().map(|at| self.section_of(at.block)))?;
        let size = section.page_size.unwrap_or_default();
        let width = size.width.map_or(11906, |twips| twips.0);
        let height = size.height.map_or(16838, |twips| twips.0);
        let margins = section.page_margins.unwrap_or_default();
        let twips = |value: Option<Twips>, default: i32| value.map_or(default, |t| t.0);
        let indent = (caret.paragraph.story == Story::Body
            && caret.paragraph.containers.is_empty())
        .then(|| self.paragraph(&caret.paragraph))
        .flatten()
        .map(|paragraph| {
            let computed = strict_ooxml_render_svg::style::compute_paragraph(
                self.session.document(),
                paragraph,
            );
            #[allow(clippy::cast_possible_truncation)]
            let tw = |points: f64| (points * 20.0).round() as i32;
            [
                tw(computed.indent_start_pt),
                tw(computed.indent_end_pt),
                tw(computed.first_line_pt),
            ]
        });
        // The columns across the text.
        let left_margin = margins.left.map_or(1440, |t| t.0);
        let right_margin = margins.right.map_or(1440, |t| t.0);
        let text = (width - left_margin - right_margin).max(1);
        let columns = section.columns.clone().unwrap_or_default();
        let count = i32::from(columns.count.unwrap_or(1).max(1));
        let space = columns.space.map_or(720, |t| t.0);
        let unequal = !columns.equal_width && !columns.columns.is_empty();
        let mut edges = Vec::new();
        if unequal {
            let mut x = left_margin;
            for spec in &columns.columns {
                let w = spec.width.map_or(text / count, |t| t.0);
                edges.push((x, x + w));
                x += w + spec.space.map_or(space, |t| t.0);
            }
        } else {
            let w = (text - space * (count - 1)) / count;
            for index in 0..count {
                let x = left_margin + index * (w + space);
                edges.push((x, x + w));
            }
        }
        // The caret's column, by where it is drawn.
        let caret_x = self
            .map
            .as_ref()
            .and_then(|map| {
                map.caret(self.session.revision(), &self.position(caret))
                    .ok()
            })
            .and_then(|rects| rects.first().map(|r| r.x * 15.0));
        #[allow(clippy::cast_possible_truncation)]
        let column = caret_x
            .and_then(|x| {
                edges
                    .iter()
                    .position(|(a, b)| x >= f64::from(*a) - 30.0 && x <= f64::from(*b) + 30.0)
            })
            .unwrap_or(0);
        Some(RulerInfo {
            columns: edges,
            column,
            unequal,
            column_space: space,
            separator: columns.separator,
            page,
            width,
            height,
            margins: [
                twips(margins.top.map(|t| Twips(t.0.abs())), 1440),
                twips(margins.right, 1440),
                twips(margins.bottom.map(|t| Twips(t.0.abs())), 1440),
                twips(margins.left, 1440),
            ],
            indent,
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
                let rects = map
                    .drawing_rects(self.session.revision(), &object)
                    .unwrap_or_default();
                // A grouped drawing has one rect per shape; the caret sits after the
                // whole drawing, so merge them into one frame per page first.
                let mut merged: Vec<VisualRect> = Vec::new();
                for rect in rects {
                    if let Some(old) = merged.iter_mut().find(|r| r.page == rect.page) {
                        let right = (old.x + old.width).max(rect.x + rect.width);
                        let bottom = (old.y + old.height).max(rect.y + rect.height);
                        old.x = old.x.min(rect.x);
                        old.y = old.y.min(rect.y);
                        old.width = right - old.x;
                        old.height = bottom - old.y;
                    } else {
                        merged.push(rect);
                    }
                }
                return merged
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
        if self.formula.is_some() {
            match command {
                ObjectCommand::MoveTo { page, x, y } => return self.move_in_text(page, x, y),
                ObjectCommand::Resize { width, height } => {
                    let Some(view) = self.object_view() else {
                        return Outcome::Quiet;
                    };
                    let sx = width as f64 / view.width.max(1) as f64;
                    let sy = height as f64 / view.height.max(1) as f64;
                    let factor = if (sx - 1.0).abs() >= (sy - 1.0).abs() {
                        sx
                    } else {
                        sy
                    };
                    let Some((at, index, mut expression)) = self.formula_at_caret() else {
                        return Outcome::Quiet;
                    };
                    let size = crate::formula::size(&expression).map_or_else(
                        || {
                            self.paragraph(&at).map_or(11.0, |p| {
                                strict_ooxml_render_svg::style::compute_paragraph(
                                    self.session.document(),
                                    p,
                                )
                                .default_run
                                .size_pt
                            })
                        },
                        |size| f64::from(size.0) / 2.0,
                    );
                    crate::formula::set_size(&mut expression, half_points((size * factor) as f32));
                    return self.replace_formula(at, index, expression);
                }
                ObjectCommand::Delete => {
                    let at = self.formula.take().unwrap();
                    let start = self.logical(&at);
                    self.anchor = Some(start.clone());
                    self.caret = Some(Logical {
                        offset: start.offset + 1,
                        ..start
                    });
                    return self.delete_selection();
                }
                _ => return Outcome::Quiet,
            }
        }
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

    /// Uses the rendered preview when the original media cannot be decoded
    /// directly (for example a legacy metafile already rendered by the toolkit).
    #[cfg(feature = "ocr")]
    fn object_preview_png(&self) -> Option<Vec<u8>> {
        let rect = *self.object_view()?.rects.first()?;
        let page = self.pages.get(rect.page)?;
        let svg = strict_ooxml::paint_page(&page.placed, &self.options);
        crate::raster::region_png(&svg, rect.x, rect.y, rect.width, rect.height)
    }

    /// Keeps recognition separate from editing: the picture survives cancel.
    #[cfg(feature = "ocr")]
    fn review_formula_recognition(&self, mathml: String) -> Outcome {
        if let Err(error) = strict_ooxml::math_from_mathml(&mathml) {
            self.note("note.recognize_failed", error.to_string(), true);
            return Outcome::Quiet;
        }
        if let Some((object, drawing)) = self.object.as_ref().and_then(|object| {
            self.drawing_at(object)
                .map(|drawing| (object.clone(), drawing))
        }) {
            self.send(Reply::FormulaRecognition(Box::new(FormulaRecognition {
                revision: self.session.revision(),
                object,
                drawing,
                mathml,
            })));
        }
        Outcome::Quiet
    }

    /// Replaces even a preview inside a run in one undoable transaction.
    fn confirm_formula_recognition(&mut self, plan: FormulaRecognition) -> Outcome {
        if self.session.revision() != plan.revision
            || self.drawing_at(&plan.object).as_ref() != Some(&plan.drawing)
        {
            self.note("note.recognition_stale", "", false);
            return Outcome::Quiet;
        }
        let Some(offset) = self.object_offset(&plan.object) else {
            return Outcome::Quiet;
        };
        let start = Logical {
            paragraph: plan.object.paragraph,
            offset,
        };
        let end = Logical {
            offset: offset + 1,
            ..start.clone()
        };
        let planned = (|| -> Result<Vec<Edit>, String> {
            let expression =
                strict_ooxml::math_from_mathml(&plan.mathml).map_err(|e| e.to_string())?;
            let mut commands = self
                .replace_commands(&start, &end, "")
                .map_err(str::to_owned)?;
            let mut document = self.session.document().clone();
            let mut stage = strict_ooxml::edit::Editor::new(
                &mut document,
                EditLimits {
                    history_transactions: 0,
                    ..EditLimits::default()
                },
            )
            .map_err(|e| e.to_string())?;
            stage
                .transact(stage.revision(), &commands)
                .map_err(|e| e.to_string())?;
            let paragraph = stage
                .paragraph(&start.paragraph)
                .map_err(|e| e.to_string())?;
            commands.extend(Self::inlines_in_paragraph(
                &start,
                paragraph,
                vec![strict_ooxml::model::Inline::Math(expression)],
            ));
            Ok(commands)
        })();
        let commands = match planned {
            Ok(commands) => commands,
            Err(error) => {
                self.note("note.recognize_failed", error, true);
                return Outcome::Quiet;
            }
        };
        if !self.transact(&commands) {
            return Outcome::Quiet;
        }
        self.object = None;
        self.anchor = None;
        self.cells = None;
        self.story = start.paragraph.story.clone();
        self.caret = Some(start);
        self.formula = self
            .formula_at_caret()
            .map(|(paragraph, index, _)| TextPosition {
                paragraph,
                inline: vec![index],
                content: 0,
                offset: 0,
            });
        Outcome::Edited
    }

    /// Recognises the selected picture with a vision model and puts what it
    /// reads in its place: text, or the formula as a Word equation.
    #[cfg(feature = "ocr")]
    fn recognize_object(&mut self, formula: bool, endpoint: &str, model: &str) -> Outcome {
        use strict_ooxml::{OcrImage, TextRecovery};
        self.ensure_layout();
        let Some(bytes) = self.object_bytes() else {
            self.note("note.recognize_failed", "no picture bytes", true);
            return Outcome::Quiet;
        };
        let image = match strict_ooxml::edit::picture_format(&bytes) {
            Some((strict_ooxml::model::MediaKind::Png, ..)) => OcrImage::png(bytes),
            Some((strict_ooxml::model::MediaKind::Jpeg, ..)) => OcrImage::jpeg(bytes),
            _ => {
                let Some(png) =
                    crate::raster::picture_png(&bytes).or_else(|| self.object_preview_png())
                else {
                    self.note(
                        "note.recognize_failed",
                        "the picture could not be read",
                        true,
                    );
                    return Outcome::Quiet;
                };
                OcrImage::png(png)
            }
        };
        let mut config = strict_ooxml::OllamaConfig::from_env();
        endpoint.clone_into(&mut config.endpoint);
        model.clone_into(&mut config.model);
        let vision = crate::model_runtime::ManagedVision::new(config);
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
        if formula {
            return self.review_formula_recognition(text);
        }
        // A drawing between runs is replaced in one step, one undo.
        if let Some(outcome) = self.replace_object_whole(&text) {
            return outcome;
        }
        if matches!(self.delete_object(), Outcome::Quiet) {
            return Outcome::Quiet;
        }
        self.paste(&text);
        Outcome::Edited
    }

    /// Replaces the selected drawing, when it stands between runs, by text
    /// (a paragraph split at each line break), in one
    /// transaction. `None` for a drawing inside a run.
    #[cfg(feature = "ocr")]
    fn replace_object_whole(&mut self, text: &str) -> Option<Outcome> {
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
        let caret = Logical {
            paragraph: here,
            offset: if lines.len() > 1 { 0 } else { offset }
                + lines.last().map_or(0, |l| l.chars().count()),
        };
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
                self.note_edit(&error);
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
        use strict_ooxml::model::Inline;
        self.ensure_layout();
        let revision = self.session.revision();
        let Some(target) = self
            .map
            .as_ref()
            .and_then(|map| map.hit_test(revision, page, x, y).ok().flatten())
        else {
            return Outcome::Quiet;
        };
        let mut target = self.logical(&target);
        let (source, inline) = if let Some(at) = &self.formula {
            let Some(index) = at.inline.first() else {
                return Outcome::Quiet;
            };
            let Some(inline) = self
                .paragraph(&at.paragraph)
                .and_then(|p| p.inlines.get(*index))
                .cloned()
            else {
                return Outcome::Quiet;
            };
            (self.logical(at), inline)
        } else if let Some(object) = &self.object {
            let (Some(drawing), Some(offset)) =
                (self.drawing_at(object), self.object_offset(object))
            else {
                return Outcome::Quiet;
            };
            (
                Logical {
                    paragraph: object.paragraph.clone(),
                    offset,
                },
                Inline::Drawing(drawing),
            )
        } else {
            return Outcome::Quiet;
        };
        // Dropped on itself: nothing moves.
        if target.paragraph == source.paragraph
            && (source.offset..=source.offset + 1).contains(&target.offset)
        {
            return Outcome::Quiet;
        }
        if target.paragraph == source.paragraph && target.offset > source.offset {
            target.offset -= 1;
        }
        let end = Logical {
            offset: source.offset + 1,
            ..source.clone()
        };
        let planned = (|| -> Result<Vec<Edit>, String> {
            let mut commands = self
                .replace_commands(&source, &end, "")
                .map_err(str::to_owned)?;
            let mut document = self.session.document().clone();
            let mut stage = strict_ooxml::edit::Editor::new(
                &mut document,
                EditLimits {
                    history_transactions: 0,
                    ..EditLimits::default()
                },
            )
            .map_err(|e| e.to_string())?;
            stage
                .transact(stage.revision(), &commands)
                .map_err(|e| e.to_string())?;
            let paragraph = stage
                .paragraph(&target.paragraph)
                .map_err(|e| e.to_string())?;
            commands.extend(Self::inlines_in_paragraph(
                &target,
                paragraph,
                vec![inline.clone()],
            ));
            Ok(commands)
        })();
        let commands = match planned {
            Ok(commands) => commands,
            Err(error) => {
                self.note_text(&error);
                return Outcome::Quiet;
            }
        };
        if !self.transact(&commands) {
            return Outcome::Quiet;
        }
        self.caret = Some(target.clone());
        self.anchor = None;
        self.cells = None;
        self.story = target.paragraph.story.clone();
        self.object = None;
        self.formula = None;
        if matches!(inline, Inline::Math(_) | Inline::MathParagraph(_)) {
            if let Some((paragraph, index, _)) = self.formula_at_caret() {
                self.formula = Some(TextPosition {
                    paragraph,
                    inline: vec![index],
                    content: 0,
                    offset: 0,
                });
            }
        } else {
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
                self.note_edit(&error);
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
        Self::point_or_run_in(logical, self.paragraph(&logical.paragraph)?)
    }

    fn point_or_run_in(
        logical: &Logical,
        paragraph: &Paragraph,
    ) -> Option<(Vec<Edit>, strict_ooxml::edit::InlinePoint)> {
        let pieces = nav::pieces(paragraph);
        if let Some(point) = nav::point_at(&pieces, logical.offset) {
            return Some((Vec::new(), point));
        }
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
        let props = paragraph
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
            FormatChange::Superscript | FormatChange::Subscript
                if self.selection().is_none() && self.cells.is_none() =>
            {
                // No selection: the caret switches over; the next text
                // typed takes it.
                let (on, script) = if matches!(change, FormatChange::Superscript) {
                    (current.superscript, VertAlign::Superscript)
                } else {
                    (current.subscript, VertAlign::Subscript)
                };
                self.pending_script = Some((!on).then_some(script));
                return Outcome::Moved;
            }
            FormatChange::Superscript => {
                patch.vert_align = Some((!current.superscript).then_some(VertAlign::Superscript));
            }
            FormatChange::Subscript => {
                patch.vert_align = Some((!current.subscript).then_some(VertAlign::Subscript));
            }
            FormatChange::UnderlineStyle(style) => patch.underline = Some(*style),
            FormatChange::StrikeStyle(lines) => {
                patch.strike = Some(if *lines == 1 {
                    TriState::On
                } else {
                    TriState::Off
                });
                patch.double_strike = Some(if *lines == 2 {
                    TriState::On
                } else {
                    TriState::Off
                });
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
        if self.formula.is_some() {
            if let (Some(Some(size)), Some((at, index, mut expression))) =
                (patch.size, self.formula_at_caret())
            {
                crate::formula::set_size(&mut expression, size);
                return self.replace_formula(at, index, expression);
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
                self.note_edit(&failure.error);
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
            Insert::ColumnBreak | Insert::LineBreak => {
                use strict_ooxml::model::BreakKind;
                let kind = if matches!(insert, Insert::LineBreak) {
                    BreakKind::TextWrapping
                } else {
                    BreakKind::Column
                };
                let Some(caret) = self.caret.clone() else {
                    return Outcome::Quiet;
                };
                self.delete_selection();
                let caret = self.caret.clone().unwrap_or(caret);
                let commands =
                    self.inline_at_caret(&caret, strict_ooxml::model::Inline::Break(kind));
                if self.transact(&commands) {
                    self.caret = Some(Logical {
                        offset: caret.offset + 1,
                        paragraph: caret.paragraph,
                    });
                }
                Outcome::Edited
            }
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
            PageChange::Columns {
                count,
                space,
                separator,
                widths,
            } => {
                use strict_ooxml::model::props::{ColumnSpec, Columns};
                let count = count.clamp(1, 16);
                let specs = widths.as_ref().map_or_else(Vec::new, |widths| {
                    widths
                        .iter()
                        .enumerate()
                        .map(|(index, width)| ColumnSpec {
                            width: Some(Twips(*width)),
                            space: (index + 1 < widths.len()).then_some(Twips(space)),
                        })
                        .collect()
                });
                properties.columns = Some(Columns {
                    count: Some(count),
                    space: Some(Twips(space)),
                    equal_width: widths.is_none(),
                    separator: separator && count > 1,
                    columns: specs,
                });
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
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
        {
            return match read_text_file(&path) {
                Ok(text) => self.paste(&text),
                Err(error) => {
                    self.note("note.import_failed", error, true);
                    Outcome::Quiet
                }
            };
        }
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
        if matches!(
            loaded.kind,
            formats::Kind::Html | formats::Kind::Rtf | formats::Kind::Latex
        ) {
            return match self.insert_rich_document(&loaded.document) {
                Ok(outcome) => {
                    self.note("note.imported", loaded.log, false);
                    outcome
                }
                Err(error) => {
                    self.note("note.import_failed", error, true);
                    Outcome::Quiet
                }
            };
        }
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
                self.anchor = None;
                self.caret = Some(Logical {
                    paragraph: at,
                    offset: 0,
                });
                self.note("note.imported", String::new(), false);
                Outcome::Edited
            }
            Err(error) => {
                if matches!(
                    error,
                    strict_ooxml::edit::EditError::UnsupportedContent(
                        strict_ooxml::edit::Unsupported::Import
                    )
                ) {
                    let text = document_text(&other.document().body.blocks);
                    let outcome = self.paste(&text);
                    self.note("note.imported_text", String::new(), false);
                    return outcome;
                }
                self.note_edit(&error);
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
                self.note_edit(&error);
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
        if self.formula.is_some() {
            if let Some((at, index, expression)) = self.formula_at_caret() {
                self.send(Reply::ClipboardObjects(Box::new(ClipObjects {
                    text: crate::formula::to_latex(&expression),
                    formula: self
                        .paragraph(&at)
                        .and_then(|p| p.inlines.get(index))
                        .cloned(),
                    objects: vec![],
                    image: None,
                })));
                if cut && self.read_only.is_none() {
                    return self.object_command(ObjectCommand::Delete);
                }
            }
            return Outcome::Quiet;
        }
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
                formula: None,
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
                formula: None,
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
        if let Some(inline) = &clip.formula {
            self.formula = None;
            self.delete_selection();
            let Some(caret) = self.caret.clone() else {
                return Outcome::Quiet;
            };
            let commands = self.inline_at_caret(&caret, inline.clone());
            return if self.transact(&commands) {
                if let Some((paragraph, index, _)) = self.formula_at_caret() {
                    self.formula = Some(TextPosition {
                        paragraph,
                        inline: vec![index],
                        content: 0,
                        offset: 0,
                    });
                }
                Outcome::Edited
            } else {
                Outcome::Quiet
            };
        }
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
        let graphic = match &mut drawing.kind {
            strict_ooxml::model::DrawingKind::Inline(v) => Some(v.graphic.as_mut()),
            strict_ooxml::model::DrawingKind::Anchor(v) => Some(v.graphic.as_mut()),
            strict_ooxml::model::DrawingKind::Opaque(_) => None,
        };
        if let Some(strict_ooxml::model::Graphic::Chart(refs)) = graphic {
            if let Some(data) = &refs.chart {
                let Some(editor) = self.editor() else {
                    return false;
                };
                let id = editor.add_chart(data);
                refs.rels = vec![Arc::from(id.as_str())];
            }
        }
        set_drawing_id(&mut drawing, id);
        let commands = self.inline_at_caret(at, strict_ooxml::model::Inline::Drawing(drawing));
        matches!(
            self.define_and_transact(&definitions, &commands),
            Outcome::Edited
        )
    }

    fn paste(&mut self, text: &str) -> Outcome {
        if self.cells.is_some() {
            self.collapse_cells();
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if !text.contains('\n') {
            let clean: String = text
                .chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .collect();
            return if self.put_text(&clean) {
                Outcome::Edited
            } else {
                Outcome::Quiet
            };
        }
        let Some(caret) = self.caret.clone() else {
            return Outcome::Quiet;
        };
        let (start, end) = self.selection().unwrap_or_else(|| (caret.clone(), caret));
        let planned = (|| -> Result<(Vec<Edit>, Logical), String> {
            let mut document = self.session.document().clone();
            let mut stage = strict_ooxml::edit::Editor::new(
                &mut document,
                EditLimits {
                    history_transactions: 0,
                    ..EditLimits::default()
                },
            )
            .map_err(|e| e.to_string())?;
            let mut commands = Vec::new();
            let mut apply = |stage: &mut strict_ooxml::edit::Editor<'_>,
                             next: Vec<Edit>|
             -> Result<(), String> {
                if !next.is_empty() {
                    stage
                        .transact(stage.revision(), &next)
                        .map_err(|e| e.to_string())?;
                    commands.extend(next);
                }
                Ok(())
            };
            let removal = self
                .replace_commands(&start, &end, "")
                .map_err(str::to_owned)?;
            apply(&mut stage, removal)?;
            let mut caret = start;
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    let paragraph = stage
                        .paragraph(&caret.paragraph)
                        .map_err(|e| e.to_string())?;
                    let next = if nav::pieces(paragraph).is_empty() {
                        vec![Edit::Split {
                            at: caret.paragraph.clone(),
                            offset: 0,
                        }]
                    } else {
                        let (mut next, point) =
                            Self::point_or_run_in(&caret, paragraph).ok_or("no split point")?;
                        next.push(Edit::SplitAt {
                            at: caret.paragraph.clone(),
                            point,
                        });
                        next
                    };
                    apply(&mut stage, next)?;
                    caret.paragraph.block += 1;
                    caret.offset = 0;
                }
                let clean: String = line
                    .chars()
                    .map(|c| if c == '\t' { ' ' } else { c })
                    .filter(|c| !c.is_control())
                    .collect();
                let next = Self::paragraph_replace_in(
                    &caret.paragraph,
                    stage
                        .paragraph(&caret.paragraph)
                        .map_err(|e| e.to_string())?,
                    caret.offset,
                    caret.offset,
                    &clean,
                )
                .map_err(str::to_owned)?;
                apply(&mut stage, next)?;
                caret.offset += clean.chars().count();
            }
            Ok((commands, caret))
        })();
        match planned {
            Ok((commands, caret)) if !commands.is_empty() && self.transact(&commands) => {
                self.caret = Some(caret);
                self.anchor = None;
                Outcome::Edited
            }
            Ok(_) => Outcome::Quiet,
            Err(error) => {
                self.note_text(&error);
                Outcome::Quiet
            }
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
                        self.note_operation(&error);
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
                self.note_operation(&error);
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
                self.note_operation(&error);
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

/// A comment mark in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommentMark {
    Start,
    End,
    Reference,
}

/// A comment's text, one line per paragraph.
fn comment_text(blocks: &[Block]) -> String {
    let lines: Vec<String> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(
                nav::pieces(paragraph)
                    .iter()
                    .map(|piece| piece.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    lines.join("\n")
}

/// Now, as an ISO 8601 UTC time (`2026-10-17T09:30:00Z`).
fn now_iso() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
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

pub(crate) fn valid_bookmark_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().is_some_and(char::is_alphabetic)
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && name.chars().count() <= 40
}

fn read_text_file(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        let data = &bytes[2..];
        if data.len() % 2 != 0 {
            return Err("Incomplete UTF-16 text".to_owned());
        }
        let units: Vec<u16> = data
            .chunks_exact(2)
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|e| e.to_string())
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&bytes)
                .to_vec(),
        )
        .map_err(|e| e.to_string())
    }
}

/// Text import fallback for package-bound content the structured importer cannot carry.
fn document_text(blocks: &[Block]) -> String {
    use strict_ooxml::model::{Inline, RunContent};
    fn inlines(list: &[Inline]) -> String {
        let mut text = String::new();
        for inline in list {
            match inline {
                Inline::Run(run) => {
                    for c in &run.content {
                        match c {
                            RunContent::Text(t) => text.push_str(&t.text),
                            RunContent::Tab => text.push('\t'),
                            RunContent::Break(_) | RunContent::CarriageReturn => text.push('\n'),
                            _ => {}
                        }
                    }
                }
                Inline::Hyperlink(h) => text.push_str(&inlines(&h.inlines)),
                Inline::SdtInline(s) => text.push_str(&inlines(&s.inlines)),
                Inline::Directional(d) => text.push_str(&inlines(&d.inlines)),
                Inline::Field(f) => text.push_str(&inlines(&f.inlines)),
                Inline::Math(m) => text.push_str(&m.text()),
                Inline::Tab => text.push('\t'),
                Inline::Break(_) => text.push('\n'),
                _ => {}
            }
        }
        text
    }
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(inlines(&p.inlines)),
            Block::Table(t) => Some(
                t.rows
                    .iter()
                    .map(|r| {
                        r.cells
                            .iter()
                            .map(|c| document_text(&c.blocks))
                            .collect::<Vec<_>>()
                            .join("\t")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Block::SdtBlock(s) => Some(document_text(&s.blocks)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    fn with_empty_worker(check: impl FnOnce(&mut Worker<'_, '_>)) {
        with_worker_replies(|worker, _| check(worker));
    }

    #[test]
    fn rich_clipboard_replaces_selection_with_images_and_one_undo() {
        for html in [false, true] {
            with_worker_replies(|worker, rx| {
                worker.handle(Request::Type("ABCDE".into()));
                worker.anchor = Some(Logical {
                    paragraph: Address::body(0),
                    offset: 1,
                });
                worker.caret = Some(Logical {
                    paragraph: Address::body(0),
                    offset: 4,
                });
                let before = worker.session.document().body.clone();
                let depth = worker.undo_depth;
                let image = png(30, 15);
                let content = if html {
                    crate::clipboard::Rich{html:Some(format!("<p><b>left</b><img src='data:image/png;base64,{}'>right</p><p>end</p>",crate::raster::base64(&image)).into_bytes()),rtf:None,image:None,image_size:None}
                } else {
                    use std::fmt::Write as _;
                    let hex = image.iter().fold(String::new(), |mut hex, b| {
                        write!(&mut hex, "{b:02x}").unwrap();
                        hex
                    });
                    crate::clipboard::Rich{rtf:Some(format!("{{\\rtf1\\ansi {{\\b left}}{{\\pict\\pngblip\\picwgoal450\\pichgoal225 {hex}}}right\\par end}}").into_bytes()),html:None,image:None,image_size:None}
                };
                assert!(matches!(
                    worker.handle(Request::PasteRich {
                        content,
                        fallback: "fallback".into()
                    }),
                    Outcome::Edited
                ));
                assert_eq!(worker.undo_depth, depth + 1);
                assert_eq!(
                    document_text(&worker.session.document().body.blocks),
                    "Aleftright\nendE"
                );
                assert_eq!(worker.objects_of(&Address::body(0)).len(), 1);
                worker.relayout();
                let object = worker.objects_of(&Address::body(0))[0].1.clone();
                worker.object = Some(object);
                assert_eq!(worker.object_bytes().unwrap(), image);
                assert!(worker.object_view().unwrap().picture);
                let after = worker.session.document().body.clone();
                let bytes = worker.session.package_bytes().unwrap();
                let reopened =
                    StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
                assert!(reopened.place(&RenderOptions::default()).is_ok());
                worker.undo(true);
                assert_eq!(worker.session.document().body, before);
                worker.undo(false);
                assert_eq!(worker.session.document().body, after);
                for reply in rx.try_iter() {
                    if let Reply::Note(n) = reply {
                        assert!(!n.error, "{}: {}", n.key, n.detail);
                    }
                }
            });
        }
    }

    #[test]
    fn html_clipboard_tables_and_lists_paste_in_a_cell() {
        with_worker_replies(|worker, rx| {
            worker.insert(Insert::Table {
                rows: 1,
                columns: 1,
            });
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            let content=crate::clipboard::Rich{html:Some(b"<p style='text-align:center'>before</p><table><tr><td>A</td><td>B</td></tr></table><ol><li>one<li>two</ol>".to_vec()),rtf:None,image:None,image_size:None};
            assert!(matches!(
                worker.handle(Request::PasteRich {
                    content,
                    fallback: String::new()
                }),
                Outcome::Edited
            ));
            assert_eq!(worker.undo_depth, depth + 1);
            assert_eq!(worker.session.document().numbering.nums().count(), 1);
            let bytes = worker.session.package_bytes().unwrap();
            assert!(StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).is_ok());
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
            assert_eq!(worker.session.document().numbering.nums().count(), 0);
            for reply in rx.try_iter() {
                if let Reply::Note(n) = reply {
                    assert!(!n.error, "{}: {}", n.key, n.detail);
                }
            }
        });
    }

    fn with_ole_worker(check: impl FnOnce(&mut Worker<'_, '_>, &Receiver<Reply>)) {
        let source = Source::File {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ole-equation.docx"),
            import: formats::ImportOptions::default(),
        };
        let mut loaded = formats::open(&source, &OpenControl::new()).unwrap();
        assert!(loaded.log.contains("OLE"));
        let editor = loaded.document.edit(EditLimits::default()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = eframe::egui::Context::default();
        let mut worker = Worker::new(
            Session::Edit(editor),
            RenderOptions::default(),
            &tx,
            &ctx,
            None,
        );
        worker.relayout();
        let object = worker.objects_of(&Address::body(0)).pop().unwrap().1;
        // Transitional w:object previews are drawings inside a run.
        assert!(object.content.is_some());
        worker.object = Some(object);
        check(&mut worker, &rx);
    }

    #[test]
    fn display_formula_from_article_selects_edits_resizes_moves_and_undoes() {
        let path = std::env::var_os("MUFOED_FORMULA_TEST_FILE").map_or_else(
            || {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/display-formula.docx")
            },
            PathBuf::from,
        );
        let mut loaded = formats::open(
            &Source::File {
                path,
                import: formats::ImportOptions::default(),
            },
            &OpenControl::new(),
        )
        .unwrap();
        let editor = loaded.document.edit(EditLimits::default()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = eframe::egui::Context::default();
        let mut worker = Worker::new(
            Session::Edit(editor),
            RenderOptions::default(),
            &tx,
            &ctx,
            None,
        );
        let (at, index) = worker
            .paragraphs(&Story::Body)
            .into_iter()
            .find_map(|at| {
                worker
                    .paragraph(&at)?
                    .inlines
                    .iter()
                    .position(|i| matches!(i, strict_ooxml::model::Inline::MathParagraph(_)))
                    .map(|i| (at, i))
            })
            .unwrap();
        worker.relayout();
        let position = TextPosition {
            paragraph: at.clone(),
            inline: vec![index],
            content: 0,
            offset: 0,
        };
        let rect = worker
            .map
            .as_ref()
            .unwrap()
            .formula_rects(worker.session.revision(), &position)
            .unwrap()[0];
        worker.context_click(
            rect.page,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert!(
            worker
                .object_view()
                .expect("display formula selects")
                .formula
        );
        assert!(rx
            .try_iter()
            .any(|r| matches!(r, Reply::Context(c) if c.object == Some(ContextObject::Formula))));
        let original = worker.paragraph(&at).unwrap().inlines[index].clone();
        let depth = worker.undo_depth;
        worker.object_command(ObjectCommand::Resize {
            width: (rect.width * 9525.0 * 1.5) as i64,
            height: (rect.height * 9525.0 * 1.5) as i64,
        });
        worker.relayout();
        assert!(worker.object_view().unwrap().width as f64 > rect.width * 9525.0 * 1.2);
        assert!(matches!(
            &worker.paragraph(&at).unwrap().inlines[index],
            strict_ooxml::model::Inline::MathParagraph(_)
        ));
        worker.undo(true);
        assert_eq!(worker.paragraph(&at).unwrap().inlines[index], original);
        worker.undo(false);
        worker.relayout();
        worker.click(
            rect.page,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            false,
            false,
        );
        worker.put_formula(
            crate::formula::from_latex("x^3").unwrap(),
            Some((at.clone(), index)),
        );
        assert_eq!(worker.undo_depth, depth + 2);
        let strict_ooxml::model::Inline::MathParagraph(updated) =
            &worker.paragraph(&at).unwrap().inlines[index]
        else {
            panic!("display wrapper preserved");
        };
        assert!(updated.equations[0].text().contains('3'));
        worker.relayout();
        let edited = worker.session.document().body.clone();
        let target_at = worker
            .paragraphs(&Story::Body)
            .into_iter()
            .find(|a| a != &at)
            .unwrap();
        let target = Logical {
            paragraph: target_at,
            offset: 0,
        };
        let target_rect = worker
            .map
            .as_ref()
            .unwrap()
            .caret(worker.session.revision(), &worker.position(&target))
            .unwrap()[0];
        assert!(matches!(
            worker.move_in_text(
                target_rect.page,
                target_rect.x,
                target_rect.y + target_rect.height / 2.0
            ),
            Outcome::Edited
        ));
        worker.relayout();
        assert!(worker.object_view().unwrap().formula);
        let selected = worker.formula.as_ref().unwrap();
        assert!(matches!(
            &worker.paragraph(&selected.paragraph).unwrap().inlines[selected.inline[0]],
            strict_ooxml::model::Inline::MathParagraph(_)
        ));
        let bytes = worker.session.package_bytes().unwrap();
        let reopened = StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
        assert!(reopened.place(&RenderOptions::default()).is_ok());
        worker.undo(true);
        assert_eq!(worker.session.document().body, edited);
        worker.relayout();
        let rect = worker
            .map
            .as_ref()
            .unwrap()
            .formula_rects(worker.session.revision(), &position)
            .unwrap()[0];
        worker.click(
            rect.page,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
            false,
            false,
        );
        assert!(matches!(
            worker.object_command(ObjectCommand::Delete),
            Outcome::Edited
        ));
        worker.undo(true);
        assert_eq!(worker.session.document().body, edited);
        for reply in rx.try_iter() {
            if let Reply::Note(n) = reply {
                assert!(!n.error, "{}: {}", n.key, n.detail);
            }
        }
    }

    #[test]
    fn ole_preview_selects_resizes_wraps_moves_and_roundtrips_as_a_picture() {
        with_ole_worker(|worker, rx| {
            let view = worker.object_view().expect("OLE preview frame");
            assert!(view.picture);
            let rect = view.rects[0];
            worker.object = None;
            worker.context_click(
                rect.page,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0,
            );
            assert!(rx.try_iter().any(
                |r| matches!(r, Reply::Context(c) if c.object == Some(ContextObject::Picture))
            ));
            worker.object_command(ObjectCommand::Resize {
                width: view.width * 2,
                height: view.height * 2,
            });
            worker.relayout();
            assert_eq!(worker.object_view().unwrap().width, view.width * 2);
            worker.wrap_object(WrapStyle::Square);
            worker.relayout();
            assert_eq!(worker.object_view().unwrap().wrap, WrapStyle::Square);
            worker.wrap_object(WrapStyle::Inline);
            worker.relayout();
            let target = Logical {
                paragraph: Address::body(1),
                offset: 3,
            };
            let rect = worker
                .map
                .as_ref()
                .unwrap()
                .caret(worker.session.revision(), &worker.position(&target))
                .unwrap()[0];
            assert!(matches!(
                worker.move_in_text(rect.page, rect.x, rect.y + rect.height / 2.0),
                Outcome::Edited
            ));
            worker.relayout();
            assert_eq!(worker.text_of(&Address::body(0)), "leftright");
            assert_eq!(
                worker.text_of(&Address::body(1)),
                format!("tar{}get", nav::OBJECT)
            );
            assert!(worker.object_view().unwrap().picture);
            let bytes = worker.session.package_bytes().unwrap();
            let reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            assert!(reopened.place(&RenderOptions::default()).is_ok());
        });
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn ole_formula_recognition_waits_for_confirmation_and_replaces_with_one_undo() {
        with_ole_worker(|worker, rx| {
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            assert!(worker.object_preview_png().is_some());
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let expected_image = crate::raster::base64(&worker.object_bytes().unwrap());
            let server = crate::model_runtime::test_support::server(
                listener,
                "test-vision",
                move |json| {
                    assert_eq!(json["messages"][0]["images"][0], expected_image);
                    serde_json::json!({"done": true, "message": {"content": "<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><msup><mi>x</mi><mn>2</mn></msup></math>"}})
                },
            );
            worker.recognize_object(true, &endpoint, "test-vision");
            server.join().unwrap();
            let plan = rx
                .try_iter()
                .find_map(|r| {
                    if let Reply::FormulaRecognition(p) = r {
                        Some(p)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(
                worker.session.document().body,
                before,
                "review and cancellation leave the picture intact"
            );
            assert_eq!(worker.undo_depth, depth);
            // Exercise the real modal: opening it never accepts; both buttons
            // remain clickable at the normal application window size.
            for accept in [false, true] {
                use eframe::egui;
                let ctx = egui::Context::default();
                let i18n = crate::i18n::I18n::load(&[]);
                let dialog =
                    crate::app::recognition_ui::RecognitionDialog::new(&ctx, 1, plan.clone());
                let mut state = (false, false);
                let mut target = None;
                let label = i18n.t(if accept {
                    "recognition.convert"
                } else {
                    "button.cancel"
                });
                for frame in 0..3 {
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1280.0, 860.0),
                            )),
                            time: Some(f64::from(frame) * 0.1),
                            ..Default::default()
                        },
                        |_| {
                            state = dialog.show(&ctx, &i18n);
                        },
                    );
                    assert_eq!(state, (false, false));
                    for shape in output.shapes {
                        if let egui::epaint::Shape::Text(text) = shape.shape {
                            if text.galley.text() == label {
                                let rect = text.galley.rect.translate(text.pos.to_vec2());
                                assert!(
                                    rect.width() > 30.0
                                        && rect.right() < 1280.0
                                        && rect.bottom() < 860.0
                                );
                                target = Some(rect.center());
                            }
                        }
                    }
                }
                let pos = target.expect("review button is visible");
                for (frame, pressed) in [true, false].into_iter().enumerate() {
                    let _ = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1280.0, 860.0),
                            )),
                            time: Some(0.4 + frame as f64 * 0.1),
                            events: vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            ..Default::default()
                        },
                        |_| {
                            state = dialog.show(&ctx, &i18n);
                        },
                    );
                }
                assert_eq!(state, (true, accept));
            }
            assert!(matches!(
                worker.handle(Request::ConfirmFormulaRecognition(plan)),
                Outcome::Edited
            ));
            worker.relayout();
            assert!(worker.object_view().unwrap().formula);
            assert_eq!(
                worker.text_of(&Address::body(0)),
                format!("left{}right", nav::OBJECT)
            );
            assert_eq!(worker.undo_depth, depth + 1);
            let after = worker.session.document().body.clone();
            let bytes = worker.session.package_bytes().unwrap();
            let reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            assert!(reopened.place(&RenderOptions::default()).is_ok());
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
            worker.undo(false);
            assert_eq!(worker.session.document().body, after);
        });
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn formula_recognition_rejects_changed_documents() {
        with_ole_worker(|worker, rx| {
            worker.review_formula_recognition(
                "<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mi>x</mi></math>".into(),
            );
            let plan = rx
                .try_iter()
                .find_map(|r| {
                    if let Reply::FormulaRecognition(p) = r {
                        Some(p)
                    } else {
                        None
                    }
                })
                .unwrap();
            worker.handle(Request::Type("changed".into()));
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            assert!(matches!(
                worker.handle(Request::ConfirmFormulaRecognition(plan)),
                Outcome::Quiet
            ));
            assert_eq!(worker.session.document().body, before);
            assert_eq!(worker.undo_depth, depth);
            assert!(rx
                .try_iter()
                .any(|r| matches!(r, Reply::Note(n) if n.key == "note.recognition_stale")));
        });
    }

    pub(super) fn with_worker_replies(check: impl FnOnce(&mut Worker<'_, '_>, &Receiver<Reply>)) {
        let mut loaded = formats::open(&Source::New, &OpenControl::new()).unwrap();
        let editor = loaded.document.edit(EditLimits::default()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = eframe::egui::Context::default();
        let mut worker = Worker::new(
            Session::Edit(editor),
            RenderOptions::default(),
            &tx,
            &ctx,
            None,
        );
        worker.caret = Some(Logical {
            paragraph: Address::body(0),
            offset: 0,
        });
        worker.relayout();
        check(&mut worker, &rx);
        for reply in rx.try_iter() {
            if let Reply::Note(note) = reply {
                assert!(!note.error, "{}: {}", note.key, note.detail);
            }
        }
    }

    #[test]
    fn copying_citations_includes_cached_fields_and_keeps_edit_points_protected() {
        let mut loaded = StrictDocument::import_latex(
            br"\begin{document}Before [\ref{missing}], \cite{one,two} after.
            \begin{thebibliography}{2}\bibitem{one}First\bibitem{two}Second\end{thebibliography}
            \end{document}",
            &strict_ooxml::LatexOptions::default(),
        )
        .unwrap();
        let editor = loaded.document.edit(EditLimits::default()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = eframe::egui::Context::default();
        let mut worker = Worker::new(
            Session::Edit(editor),
            RenderOptions::default(),
            &tx,
            &ctx,
            None,
        );
        worker.relayout();
        worker.handle(Request::SelectAll);
        worker.copy(false);
        let text = rx
            .try_iter()
            .find_map(|reply| match reply {
                Reply::Clipboard(text) => Some(text),
                _ => None,
            })
            .expect("copied text");
        assert!(text.contains("Before [??], [1, 2] after."), "{text}");
        let paragraph = worker.paragraph(&Address::body(0)).unwrap();
        let pieces = nav::pieces(paragraph);
        let mut offset = 0;
        for piece in pieces {
            if piece.kind == nav::PieceKind::FieldResult {
                if piece.len() > 1 {
                    assert!(nav::point_at(&nav::pieces(paragraph), offset + 1).is_none());
                }
                assert!(nav::run_at(paragraph, &piece.inline).is_none());
            }
            offset += piece.len();
        }
    }

    #[test]
    fn regression_bookmark_on_empty_document() {
        with_empty_worker(|worker| {
            assert!(matches!(worker.add_bookmark("Start"), Outcome::Edited));
            assert_eq!(worker.bookmarks_and_toc().0, ["Start"]);
            worker.relayout();
            assert!(worker.caret_rect().is_some());
            let bytes = worker.session.package_bytes().unwrap();
            let reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            assert!(
                matches!(&reopened.document().body.blocks[0].as_paragraph().unwrap().inlines[0],
                strict_ooxml::model::Inline::BookmarkStart(b) if b.name.as_ref() == "Start")
            );
        });
    }

    #[test]
    fn regression_formula_on_empty_document() {
        with_empty_worker(|worker| {
            let expression = crate::formula::from_latex(r"\frac{x^{2}}{y}").unwrap();
            assert!(matches!(
                worker.put_formula(expression, None),
                Outcome::Edited
            ));
            worker.relayout();
            assert!(worker.caret_rect().is_some(), "caret after formula");
            let view = worker.object_view().expect("formula selected");
            assert!(view.formula);
            let rect = view.rects[0];
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.send_cursor(false);
            assert!(worker.caret_rect().is_some());
            worker.click(
                rect.page,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0,
                false,
                false,
            );
            assert!(worker.object_view().unwrap().formula);
            assert!(matches!(
                worker.handle(Request::Object(ObjectCommand::Resize {
                    width: view.width * 2,
                    height: view.height * 2
                })),
                Outcome::Edited
            ));
            worker.relayout();
            let enlarged = worker.object_view().unwrap();
            assert!(
                (enlarged.rects[0].width / rect.width - 2.0).abs() < 0.05,
                "{:?}",
                enlarged.rects
            );
            assert!((enlarged.rects[0].height / rect.height - 2.0).abs() < 0.05);
            let bytes = worker.session.package_bytes().unwrap();
            let reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            let math = reopened.document().body.blocks[0]
                .as_paragraph()
                .unwrap()
                .inlines
                .iter()
                .find_map(|i| {
                    if let strict_ooxml::model::Inline::Math(m) = i {
                        Some(m)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(crate::formula::size(math), Some(HalfPoints(44)));
            assert!(matches!(worker.handle(Request::Delete), Outcome::Edited));
            assert!(worker.text_of(&Address::body(0)).is_empty());
        });
    }

    #[test]
    fn caret_size_follows_paragraph_and_character_styles_during_navigation() {
        with_empty_worker(|worker| {
            worker.handle(Request::Paste("normal\nheading".into()));
            worker.handle(Request::Move {
                motion: Motion::DocStart,
                extend: false,
            });
            assert!(
                worker.caret_format().size.is_some(),
                "document default size is visible"
            );
            let mut paragraph_style = worker
                .session
                .document()
                .styles
                .iter()
                .find(|s| s.is_default)
                .unwrap()
                .clone();
            paragraph_style.id = StyleId::new("TestParagraphSize");
            paragraph_style.is_default = false;
            paragraph_style.run.size = Some(HalfPoints(38));
            let mut character_style = paragraph_style.clone();
            character_style.id = StyleId::new("TestCharacterSize");
            character_style.style_type = strict_ooxml::model::values::StyleType::Character;
            character_style.run.size = Some(HalfPoints(44));
            let mut properties = worker.paragraph(&Address::body(1)).unwrap().props.clone();
            properties.style = Some(paragraph_style.id.clone());
            let mut run_props = nav::run_at(worker.paragraph(&Address::body(0)).unwrap(), &[0])
                .unwrap()
                .props
                .clone();
            run_props.style = Some(character_style.id.clone());
            assert!(matches!(
                worker.define_and_transact(
                    &[
                        strict_ooxml::edit::Definition::CreateStyle(Box::new(paragraph_style)),
                        strict_ooxml::edit::Definition::CreateStyle(Box::new(character_style)),
                    ],
                    &[
                        Edit::ParagraphProperties {
                            at: Address::body(1),
                            properties: Box::new(properties)
                        },
                        Edit::RunProperties {
                            at: Address::body(0),
                            inline: vec![0],
                            properties: Box::new(run_props)
                        },
                    ]
                ),
                Outcome::Edited
            ));
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            assert_eq!(worker.caret_format().size, Some(19.0));
            worker.handle(Request::Move {
                motion: Motion::DocStart,
                extend: false,
            });
            assert_eq!(worker.caret_format().size, Some(22.0));
        });
    }

    #[test]
    fn cleanup_preserves_formatting_and_is_one_undo_with_stale_plan_rejection() {
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Paste("  Alpha  , beta   \n\n\nnext  text".into()));
            let original = worker.session.document().body.clone();
            let revision = worker.session.revision();
            let snapshot = crate::cleanup::snapshot(worker.session.document(), revision);
            let plan = crate::cleanup::build(&snapshot, &crate::cleanup::Options::default(), &[]);
            assert!(plan.changes.len() >= 3);
            let depth = worker.undo_depth;
            assert!(matches!(
                worker.handle(Request::ApplyCleanup(Box::new(plan.clone()))),
                Outcome::Edited
            ));
            assert_eq!(worker.undo_depth, depth + 1);
            assert_eq!(
                document_text(&worker.session.document().body.blocks),
                "\tAlpha, beta\n\nnext text"
            );
            let cleaned = worker.session.document().body.clone();
            worker.undo(true);
            assert_eq!(worker.session.document().body, original);
            worker.undo(false);
            assert_eq!(worker.session.document().body, cleaned);
            assert!(crate::cleanup::commands(
                worker.session.document(),
                worker.session.revision(),
                &plan
            )
            .is_err());
            for reply in replies.try_iter() {
                if let Reply::Note(note) = reply {
                    assert!(!note.error, "{}", note.detail);
                }
            }
        });
    }

    #[test]
    fn cleanup_joins_keep_both_runs_and_save_as_standard_docx() {
        with_empty_worker(|worker| {
            worker.handle(Request::Paste("Part of a\nsentence continues.".into()));
            let original = worker.session.document().body.clone();
            let snapshot =
                crate::cleanup::snapshot(worker.session.document(), worker.session.revision());
            let mut plan =
                crate::cleanup::build(&snapshot, &crate::cleanup::Options::default(), &[0]);
            assert_eq!(plan.changes.len(), 1);
            assert!(!plan.changes[0].selected);
            plan.changes[0].selected = true;
            assert!(matches!(
                worker.handle(Request::ApplyCleanup(Box::new(plan))),
                Outcome::Edited
            ));
            assert_eq!(
                document_text(&worker.session.document().body.blocks),
                "Part of a sentence continues."
            );
            let bytes = worker.session.package_bytes().unwrap();
            let reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            assert_eq!(
                document_text(&reopened.document().body.blocks),
                "Part of a sentence continues."
            );
            worker.undo(true);
            assert_eq!(worker.session.document().body, original);
        });
    }

    #[test]
    fn regression_chart_on_empty_document() {
        use strict_ooxml::model::chart::*;
        for kind in [
            ChartKind::Bar {
                horizontal: false,
                grouping: BarGrouping::Clustered,
            },
            ChartKind::Bar {
                horizontal: true,
                grouping: BarGrouping::Clustered,
            },
            ChartKind::Line,
            ChartKind::Area,
            ChartKind::Pie { doughnut: false },
            ChartKind::Pie { doughnut: true },
        ] {
            with_empty_worker(|worker| {
                let data = ChartData {
                    kind,
                    title: None,
                    categories: vec!["A".into(), "B".into()],
                    series: vec![ChartSeries {
                        values: vec![Some(1.0), Some(2.0)],
                        ..Default::default()
                    }],
                    legend: true,
                    legend_position: LegendPosition::Bottom,
                    unsupported: vec![],
                };
                assert!(matches!(worker.put_chart(data), Outcome::Edited));
                let saved_before_mapping = worker.session.package_bytes().unwrap();
                let model_before_mapping = worker.session.document().body.clone();
                worker.relayout();
                assert_eq!(worker.session.document().body, model_before_mapping);
                assert_eq!(
                    worker.session.package_bytes().unwrap(),
                    saved_before_mapping,
                    "interaction mapping must not alter the saved DOCX"
                );
                assert!(
                    worker.object_view().is_some(),
                    "inserted chart selected; map present: {}",
                    worker.map.is_some()
                );
                let view = worker.object_view().unwrap();
                assert!(view.chart);
                let data = worker.selected_chart().unwrap();
                worker.handle(Request::ChartOfObject);
                assert!(
                    worker.selected_chart().is_some(),
                    "opening dialog keeps selection"
                );
                assert!(matches!(
                    worker.handle(Request::PutChart(Box::new((*data).clone()))),
                    Outcome::Edited
                ));
                assert_eq!(
                    worker.objects_of(&Address::body(0)).len(),
                    1,
                    "edit replaces chart"
                );
                worker.relayout();
                let rect = view.rects[0];
                worker.handle(Request::Move {
                    motion: Motion::DocEnd,
                    extend: false,
                });
                worker.click(
                    rect.page,
                    rect.x + rect.width / 2.0,
                    rect.y + rect.height / 2.0,
                    false,
                    false,
                );
                assert!(worker.object_view().unwrap().chart);
                assert!(matches!(
                    worker.handle(Request::Object(ObjectCommand::Resize {
                        width: view.width / 2,
                        height: view.height / 2
                    })),
                    Outcome::Edited
                ));
                worker.relayout();
                assert_eq!(worker.object_view().unwrap().width, view.width / 2);
                worker.wrap_object(WrapStyle::Front);
                worker.relayout();
                let floating = worker.object_view().expect("floating chart frame");
                let rect = floating.rects[0];
                assert!((rect.width * 9525.0 - floating.width as f64).abs() < 0.01);
                worker.click(rect.page, rect.x + 1.0, rect.y + 1.0, false, false);
                assert!(worker.object_view().unwrap().chart);
            });
        }
    }

    #[test]
    fn regression_insert_document() {
        let dir = std::env::temp_dir().join(format!("mufoed-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("source.rtf");
        std::fs::write(&path, br"{\rtf1\ansi Imported text\par}").unwrap();
        with_empty_worker(|worker| {
            assert!(matches!(worker.insert_document(path), Outcome::Edited));
            worker.relayout();
            assert!(worker
                .session
                .document()
                .body
                .blocks
                .iter()
                .filter_map(Block::as_paragraph)
                .any(|p| nav::pieces(p)
                    .iter()
                    .any(|s| s.text.contains("Imported text"))));
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn html_and_rtf_files_keep_images_and_replace_selection_in_one_undo() {
        use std::fmt::Write as _;
        let dir = std::env::temp_dir().join(format!("mufoed-rich-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let image = png(30, 15);
        std::fs::write(dir.join("image.png"), &image).unwrap();
        let html_path = dir.join("source.html");
        std::fs::write(
            &html_path,
            "<meta charset='utf-8'><p><b>До</b><img src='image.png'>После</p>",
        )
        .unwrap();
        let rtf_path = dir.join("source.rtf");
        let hex = image.iter().fold(String::new(), |mut hex, b| {
            write!(&mut hex, "{b:02x}").unwrap();
            hex
        });
        std::fs::write(&rtf_path, format!("{{\\rtf1\\ansi {{\\b Before}}{{\\pict\\pngblip\\picwgoal450\\pichgoal225 {hex}}}After\\par}}")).unwrap();
        for path in [html_path, rtf_path] {
            let loaded = formats::open(
                &Source::File {
                    path: path.clone(),
                    import: formats::ImportOptions::default(),
                },
                &OpenControl::new(),
            )
            .unwrap();
            assert_eq!(loaded.document.document().media.iter().count(), 1);
            let part = &loaded.document.document().media.iter().next().unwrap().part;
            assert_eq!(loaded.document.package().read_part(part).unwrap(), image);
            with_empty_worker(|worker| {
                worker.handle(Request::Type("replace me".into()));
                worker.handle(Request::SelectAll);
                let before = worker.session.document().body.clone();
                let depth = worker.undo_depth;
                assert!(matches!(
                    worker.insert_document(path.clone()),
                    Outcome::Edited
                ));
                worker.relayout();
                assert_eq!(worker.undo_depth, depth + 1);
                assert_eq!(worker.session.document().media.iter().count(), 1);
                let text = document_text(&worker.session.document().body.blocks);
                assert!(!text.contains("replace me"));
                assert!(text.contains("После") || text.contains("After"));
                let inserted = worker.session.document().body.clone();
                worker.undo(true);
                assert_eq!(worker.session.document().body, before);
                worker.undo(false);
                assert_eq!(worker.session.document().body, inserted);
            });
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_import_over_the_history_limit_is_one_undo_and_redo() {
        let dir = std::env::temp_dir().join(format!("mufoed-undo-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("many-lines.txt");
        let text = (0..180)
            .map(|n| format!("Line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, &text).unwrap();
        with_empty_worker(|worker| {
            worker.handle(Request::Type("AB".into()));
            worker.handle(Request::Move {
                motion: Motion::Left,
                extend: false,
            });
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            assert!(matches!(
                worker.handle(Request::Insert(Insert::Document(path))),
                Outcome::Edited
            ));
            assert_eq!(worker.undo_depth, depth + 1);
            assert_eq!(
                document_text(&worker.session.document().body.blocks),
                format!("A{text}B")
            );
            let inserted = worker.session.document().body.clone();
            assert!(matches!(worker.undo(true), Outcome::Edited));
            assert_eq!(worker.session.document().body, before);
            worker.undo(false);
            assert_eq!(worker.session.document().body, inserted);
            worker.undo(true);
            worker.undo(true);
            assert_eq!(document_text(&worker.session.document().body.blocks), "");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn multiline_paste_replaces_a_selection_with_one_undo() {
        with_empty_worker(|worker| {
            worker.handle(Request::Paste("Начало\nКонец".into()));
            worker.handle(Request::SelectAll);
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            worker.handle(Request::Paste("α\r\nβ\tγ\rδ".into()));
            assert_eq!(
                document_text(&worker.session.document().body.blocks),
                "α\nβ γ\nδ"
            );
            assert_eq!(worker.undo_depth, depth + 1);
            let after = worker.session.document().body.clone();
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
            worker.undo(false);
            assert_eq!(worker.session.document().body, after);
        });
    }

    #[test]
    fn pictures_charts_and_formulas_move_in_one_transaction() {
        for kind in 0..3 {
            with_empty_worker(|worker| {
                worker.handle(Request::Paste("left\nright".into()));
                worker.caret = Some(Logical {
                    paragraph: Address::body(0),
                    offset: 2,
                });
                match kind {
                    0 => {
                        worker.insert_picture(png(30, 15), "dot");
                    }
                    1 => {
                        use strict_ooxml::model::chart::*;
                        worker.put_chart(ChartData {
                            kind: ChartKind::Line,
                            title: None,
                            categories: vec!["A".into(), "B".into()],
                            series: vec![ChartSeries {
                                values: vec![Some(1.0), Some(2.0)],
                                ..Default::default()
                            }],
                            legend: false,
                            legend_position: LegendPosition::Bottom,
                            unsupported: vec![],
                        });
                    }
                    _ => {
                        worker.put_formula(crate::formula::from_latex("x^2").unwrap(), None);
                    }
                }
                worker.relayout();
                let before = worker.session.document().body.clone();
                let target = Logical {
                    paragraph: Address::body(1),
                    offset: 2,
                };
                let rect = worker
                    .map
                    .as_ref()
                    .unwrap()
                    .caret(worker.session.revision(), &worker.position(&target))
                    .unwrap()[0];
                let depth = worker.undo_depth;
                assert!(matches!(
                    worker.handle(Request::Object(ObjectCommand::MoveTo {
                        page: rect.page,
                        x: rect.x,
                        y: rect.y + rect.height / 2.0
                    })),
                    Outcome::Edited
                ));
                assert_eq!(worker.undo_depth, depth + 1);
                worker.relayout();
                let view = worker
                    .object_view()
                    .expect("object stays selected after moving");
                assert_eq!(
                    (view.picture, view.chart, view.formula),
                    (kind == 0, kind == 1, kind == 2)
                );
                assert_eq!(worker.text_of(&Address::body(0)), "left");
                assert_eq!(
                    worker.text_of(&Address::body(1)),
                    format!("ri{}ght", nav::OBJECT)
                );
                let moved = worker.session.document().body.clone();
                let bytes = worker.session.package_bytes().unwrap();
                let reopened =
                    StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
                assert!(reopened.place(&RenderOptions::default()).is_ok());
                worker.undo(true);
                assert_eq!(worker.session.document().body, before);
                worker.undo(false);
                assert_eq!(worker.session.document().body, moved);
            });
        }
    }

    #[test]
    fn paragraph_dialog_patch_is_one_undo_and_preserves_untouched_fields() {
        with_empty_worker(|worker| {
            worker.handle(Request::Paste("first\nsecond".into()));
            worker.caret = Some(Logical {
                paragraph: Address::body(0),
                offset: 0,
            });
            worker.paragraph_edit(|p| p.keep_next = TriState::On);
            worker.handle(Request::SelectAll);
            let before = worker.session.document().body.clone();
            let computed = strict_ooxml_render_svg::style::compute_paragraph(
                worker.session.document(),
                worker.paragraph(&Address::body(0)).unwrap(),
            );
            let original = crate::paragraph::Format::from_computed(&computed);
            let mut changed = original.clone();
            changed.left = 720;
            changed.line = 480;
            changed.widow = false;
            let depth = worker.undo_depth;
            worker.handle(Request::SetParagraphFormat(Box::new(
                crate::paragraph::Patch {
                    before: original,
                    after: changed,
                },
            )));
            assert_eq!(worker.undo_depth, depth + 1);
            for block in &worker.session.document().body.blocks {
                let p = block.as_paragraph().unwrap();
                assert_eq!(p.props.indentation.unwrap().start, Some(Twips(720)));
                assert_eq!(p.props.spacing.unwrap().line, Some(Twips(480)));
            }
            assert_eq!(
                worker.paragraph(&Address::body(0)).unwrap().props.keep_next,
                TriState::On
            );
            assert_eq!(
                worker.paragraph(&Address::body(1)).unwrap().props.keep_next,
                TriState::Absent
            );
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
        });
    }

    #[test]
    fn context_click_preserves_selection_and_classifies_the_clicked_object() {
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Type("alpha beta".into()));
            worker.anchor = Some(Logical {
                paragraph: Address::body(0),
                offset: 0,
            });
            worker.caret = Some(Logical {
                paragraph: Address::body(0),
                offset: 5,
            });
            worker.relayout();
            let selection = worker.selection();
            let rect = worker
                .map
                .as_ref()
                .unwrap()
                .caret(
                    worker.session.revision(),
                    &worker.position(&Logical {
                        paragraph: Address::body(0),
                        offset: 2,
                    }),
                )
                .unwrap()[0];
            worker.context_click(rect.page, rect.x, rect.y + rect.height / 2.0);
            assert_eq!(worker.selection(), selection);
            let state = replies
                .try_iter()
                .find_map(|r| {
                    if let Reply::Context(state) = r {
                        Some(state)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(state.selection);
            assert!(state.object.is_none());
            worker.anchor = None;
            worker.put_formula(crate::formula::from_latex("x^2").unwrap(), None);
            worker.relayout();
            let rect = worker.object_view().unwrap().rects[0];
            worker.context_click(
                rect.page,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0,
            );
            let state = replies
                .try_iter()
                .find_map(|r| {
                    if let Reply::Context(state) = r {
                        Some(state)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(state.object, Some(ContextObject::Formula));
            worker.copy(false);
            let clip = replies
                .try_iter()
                .find_map(|r| {
                    if let Reply::ClipboardObjects(clip) = r {
                        Some(clip)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(clip.formula.is_some());
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.paste_objects(&clip);
            assert_eq!(
                worker.session.document().body.blocks[0]
                    .as_paragraph()
                    .unwrap()
                    .inlines
                    .iter()
                    .filter(|i| matches!(i, strict_ooxml::model::Inline::Math(_)))
                    .count(),
                2
            );
        });
    }

    #[test]
    fn regression_insert_plain_text_and_document_with_picture() {
        let dir = std::env::temp_dir().join(format!("mufoed-text-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            (
                "utf8.txt",
                [vec![0xef, 0xbb, 0xbf], "Текст\r\nNext".as_bytes().to_vec()].concat(),
            ),
            (
                "utf16.txt",
                [
                    vec![0xff, 0xfe],
                    "Текст\r\nNext"
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                ]
                .concat(),
            ),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            with_empty_worker(|worker| {
                worker.handle(Request::Type("AB".into()));
                worker.handle(Request::Move {
                    motion: Motion::Left,
                    extend: false,
                });
                assert!(matches!(worker.insert_document(path), Outcome::Edited));
                assert_eq!(
                    document_text(&worker.session.document().body.blocks),
                    "AТекст\nNextB"
                );
            });
        }
        let path = dir.join("with-picture.docx");
        with_empty_worker(|source| {
            source.handle(Request::Type("Visible text".into()));
            source.handle(Request::Object(ObjectCommand::InsertPicture {
                bytes: png(20, 10),
                name: "picture".into(),
            }));
            std::fs::write(&path, source.session.package_bytes().unwrap()).unwrap();
        });
        with_empty_worker(|target| {
            assert!(matches!(target.insert_document(path), Outcome::Edited));
            assert_eq!(
                document_text(&target.session.document().body.blocks),
                "Visible text"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn regression_system_fonts_follow_font_changes() {
        with_empty_worker(|worker| {
            worker.handle(Request::Type("Testing fonts".into()));
            worker.handle(Request::Format(FormatChange::Font("Georgia".into())));
            worker.relayout();
            let bundled = strict_ooxml::paint_page(&worker.pages[0].placed, &worker.options);
            worker.handle(Request::SystemFonts(true));
            worker.relayout();
            assert!(worker
                .options
                .font_assets
                .as_ref()
                .unwrap()
                .families()
                .iter()
                .any(|s| s.eq_ignore_ascii_case("Georgia")));
            let installed = strict_ooxml::paint_page(&worker.pages[0].placed, &worker.options);
            assert_ne!(bundled, installed, "toggle changes actual SVG");
            worker.handle(Request::Format(FormatChange::Font("Verdana".into())));
            worker.relayout();
            assert!(worker
                .options
                .font_assets
                .as_ref()
                .unwrap()
                .families()
                .iter()
                .any(|s| s.eq_ignore_ascii_case("Verdana")));
            worker.handle(Request::SystemFonts(false));
            worker.relayout();
            assert!(worker
                .options
                .font_assets
                .as_ref()
                .is_none_or(|assets| !assets
                    .families()
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("Verdana"))));
        });
    }

    #[test]
    fn regression_comments_survive_save_and_navigation_goes_to_top() {
        with_empty_worker(|worker| {
            worker.handle(Request::Type("Chapter".into()));
            assert!(matches!(
                worker.comments(CommentCommand::Add {
                    author: "Ann".into(),
                    initials: "A".into()
                }),
                Outcome::Edited
            ));
            let id = worker.session.document().comments[0].id.as_str().to_owned();
            worker.comments(CommentCommand::Edit {
                id,
                text: "Saved comment".into(),
            });
            let bytes = worker.session.package_bytes().unwrap();
            let reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            assert_eq!(
                comment_text(&reopened.document().comments[0].blocks),
                "Saved comment"
            );
            worker.handle(Request::GoTo(Address::body(0)));
            assert!(worker.scroll_at_top);
            worker.send_cursor(true);
            assert!(!worker.scroll_at_top, "top alignment is consumed once");
        });
    }

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

    /// A one-page PDF with two blocks of text, set apart: opened as the
    /// original laid it out, each block is a text box.
    fn two_block_pdf() -> Vec<u8> {
        let content = "BT /F1 10 Tf 72 700 Td (Alpha Alpha) Tj ET\n\
                       BT /F1 10 Tf 300 500 Td (Bravo Charlie) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
                .to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        let count = objects.len() + 1;
        pdf.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n")
                .as_bytes(),
        );
        pdf
    }

    fn selected_text(requests: &Sender<Request>, replies: &Receiver<Reply>) -> String {
        requests.send(Request::Copy { cut: false }).unwrap();
        wait(replies, |reply| match reply {
            Reply::Clipboard(text) => Some(text),
            _ => None,
        })
    }

    /// A PDF opens as text boxes; a click on a box's text puts the caret in
    /// it rather than selecting the box, a double click takes the word, and
    /// typing replaces it there.
    #[test]
    fn a_click_on_a_text_box_text_edits_it() {
        let path = std::env::temp_dir().join(format!("mufoed-box-{}.pdf", std::process::id()));
        std::fs::write(&path, two_block_pdf()).expect("pdf");
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(
            Source::File {
                path: path.clone(),
                import: crate::formats::ImportOptions::as_original(),
            },
            false,
            OpenControl::new(),
            ctx,
        );
        let _ = state(&replies);
        // "Bravo" stands at 300 pt from the left, its baseline 500 pt up a
        // 792 pt page: in pixels (96 per inch) its middle is about here.
        let (x, y) = (310.0 * 4.0 / 3.0, (792.0 - 504.0) * 4.0 / 3.0);
        requests
            .send(Request::Click {
                page: 0,
                x,
                y,
                extend: false,
                column: false,
            })
            .unwrap();
        assert!(object(&replies).is_none(), "the click went into the text");
        requests
            .send(Request::SelectWord { page: 0, x, y })
            .unwrap();
        assert!(object(&replies).is_none());
        assert_eq!(selected_text(&requests, &replies).trim(), "Bravo");
        for c in "Zulu".chars() {
            requests.send(Request::Type(c.to_string())).unwrap();
        }
        requests
            .send(Request::SelectWord { page: 0, x, y })
            .unwrap();
        assert!(object(&replies).is_none());
        assert_eq!(selected_text(&requests, &replies).trim(), "Zulu");
        requests.send(Request::Shutdown).unwrap();
        let _ = std::fs::remove_file(&path);
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

    /// A page drawn in bands is the page drawn whole, and an edit redraws only
    /// the bands it changed: put over the old bitmap they give the new page.
    #[test]
    fn a_page_drawn_in_bands_matches_it_drawn_whole() {
        let (requests, replies) = new_document();
        for line in 0..12 {
            requests
                .send(Request::Type(format!("Line {line} with some words in it ")))
                .unwrap();
            requests.send(Request::Enter).unwrap();
        }
        let pages = |requests: &Sender<Request>| {
            requests.send(Request::Type(String::new())).unwrap();
            loop {
                if let Some(pages) = wait(&replies, |reply| match reply {
                    Reply::Pages { pages, .. } => Some(Some(pages)),
                    Reply::Cursor(_) => Some(None),
                    _ => None,
                }) {
                    break pages;
                }
            }
        };
        // As the editor paints: text as glyph outlines.
        let options = strict_ooxml::RenderOptions::default()
            .embed_fonts(false)
            .text_outlines(true);
        let whole = |page: &PageData| {
            crate::raster::rasterize(&strict_ooxml::paint_page(&page.placed, &options), 1.5)
                .expect("whole")
        };
        let before = pages(&requests);
        let first =
            crate::raster::rasterize_page(&before[0].placed, &options, 1.5, None).expect("bands");
        assert!(first.full);
        let mut bitmap = first.assemble();
        let reference = whole(&before[0]);
        assert_eq!(
            (bitmap.width, bitmap.height),
            (reference.width, reference.height)
        );
        // Drawn in bands, a glyph's antialiasing can differ by a shade in a few
        // pixels (an integer row shift); nothing is missing.
        let differing = bitmap
            .rgba
            .iter()
            .zip(&reference.rgba)
            .filter(|(a, b)| a != b)
            .count();
        let worst = bitmap
            .rgba
            .iter()
            .zip(&reference.rgba)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            differing * 2000 <= bitmap.rgba.len() && worst <= 48,
            "bands differ from the whole page: {differing} bytes, by up to {worst}"
        );
        // One word more on the last line.
        requests.send(Request::Type("more".into())).unwrap();
        let after = pages(&requests);
        let changed =
            crate::raster::rasterize_page(&after[0].placed, &options, 1.5, Some(&first.hashes))
                .expect("changes");
        assert!(!changed.full);
        assert!(!changed.bands.is_empty(), "the edit changed a band");
        assert!(
            changed.bands.len() < first.hashes.len() / 2,
            "{} of {} bands redrawn",
            changed.bands.len(),
            first.hashes.len()
        );
        let stride = bitmap.width * 4;
        for (row, band) in &changed.bands {
            let start = row * stride;
            bitmap.rgba[start..start + band.rgba.len()].copy_from_slice(&band.rgba);
        }
        // The old bitmap with the changed bands put in is the new page drawn
        // in bands, to the byte: edits never drift.
        let fresh = crate::raster::rasterize_page(&after[0].placed, &options, 1.5, None)
            .expect("new page")
            .assemble();
        assert!(
            bitmap.rgba == fresh.rgba,
            "patched bitmap differs from the new page"
        );
        requests.send(Request::Shutdown).unwrap();
    }

    /// A position at a soft line break is the end of one line and the start of
    /// the next: one caret shows, on the line the caret was put on.
    #[test]
    fn a_soft_line_break_shows_one_caret() {
        let (requests, replies) = new_document();
        requests.send(Request::Type("word ".repeat(60))).unwrap();
        let carets = |motion: Motion| {
            requests
                .send(Request::Move {
                    motion,
                    extend: false,
                })
                .unwrap();
            wait(&replies, |reply| match reply {
                Reply::Cursor(cursor) => Some(cursor.carets),
                _ => None,
            })
        };
        let start = carets(Motion::DocStart);
        assert_eq!(start.len(), 1);
        let end = carets(Motion::End);
        assert_eq!(end.len(), 1, "End shows one caret: {end:?}");
        assert!((end[0].y - start[0].y).abs() < 0.5, "End stays on line 1");
        assert!(end[0].x > start[0].x + 100.0, "at the line's end");
        let _ = carets(Motion::Home);
        let down = carets(Motion::Down);
        assert_eq!(down.len(), 1);
        assert!(down[0].y > start[0].y + 5.0, "Down is on line 2");
        requests.send(Request::Shutdown).unwrap();
    }

    /// Shift+Enter breaks the line inside the paragraph; Undo takes it back.
    #[test]
    fn a_line_break_stays_in_its_paragraph() {
        with_worker_replies(|worker, _| {
            worker.handle(Request::Type("ab".into()));
            worker.handle(Request::Insert(Insert::LineBreak));
            worker.handle(Request::Type("cd".into()));
            let blocks = &worker.session.document().body.blocks;
            assert_eq!(blocks.len(), 1, "one paragraph");
            let Block::Paragraph(paragraph) = &blocks[0] else {
                panic!("a paragraph");
            };
            let breaks = paragraph
                .inlines
                .iter()
                .filter(|inline| {
                    matches!(
                        inline,
                        strict_ooxml::model::Inline::Run(run)
                            if run.content.iter().any(|content| matches!(
                                content,
                                strict_ooxml::model::RunContent::Break(
                                    strict_ooxml::model::BreakKind::TextWrapping
                                )
                            ))
                    ) || matches!(
                        inline,
                        strict_ooxml::model::Inline::Break(
                            strict_ooxml::model::BreakKind::TextWrapping
                        )
                    )
                })
                .count();
            assert_eq!(breaks, 1, "{:?}", paragraph.inlines);
            assert_eq!(
                document_text(blocks),
                "ab
cd"
            );
        });
    }

    /// A picture on the clipboard as pixels (a screenshot) goes in at the
    /// caret, selected, at its own size.
    #[test]
    fn a_clipboard_picture_is_pasted_as_a_picture() {
        let (requests, replies) = new_document();
        requests.send(Request::Type("ab".to_owned())).unwrap();
        requests
            .send(Request::PasteRich {
                content: crate::clipboard::Rich {
                    image: Some(png(40, 20)),
                    image_size: Some((40, 20)),
                    ..crate::clipboard::Rich::default()
                },
                fallback: String::new(),
            })
            .unwrap();
        let pasted = loop {
            if let Some(view) = object(&replies) {
                break view;
            }
        };
        assert!(pasted.picture);
        assert_eq!((pasted.width, pasted.height), (40 * 9525, 20 * 9525));
        requests.send(Request::Shutdown).unwrap();
    }

    /// Formatted text that comes with a picture of itself (a spreadsheet's
    /// cells, a page's selection) is pasted as text, not as the picture.
    #[test]
    fn clipboard_html_wins_over_its_picture() {
        let (requests, replies) = new_document();
        requests
            .send(Request::PasteRich {
                content: crate::clipboard::Rich {
                    html: Some(b"<p>From the page</p>".to_vec()),
                    image: Some(png(40, 20)),
                    image_size: Some((40, 20)),
                    ..crate::clipboard::Rich::default()
                },
                fallback: String::new(),
            })
            .unwrap();
        assert_eq!(body_text(&requests, &replies).trim(), "From the page");
        requests.send(Request::Shutdown).unwrap();
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

    /// The placed text of the last pages reply.
    fn page_texts(replies: &Receiver<Reply>) -> Vec<(f64, String)> {
        let pages = wait(replies, |reply| match reply {
            Reply::Pages { pages, .. } => Some(pages),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        });
        pages
            .first()
            .map(|page| {
                page.placed
                    .items
                    .iter()
                    .flat_map(strict_ooxml_render_svg::Item::leaves)
                    .filter_map(|item| match item {
                        strict_ooxml_render_svg::Item::Text(text) => {
                            Some((text.baseline, text.text.clone()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Arrows and a single click stay in the body. A double click in the
    /// top margin, or the header button, opens that page's header.
    #[test]
    fn the_caret_enters_a_header_only_on_purpose() {
        with_empty_worker(|worker| {
            worker.handle(Request::Type("body".into()));
            worker.handle(Request::HeaderFooter(HeaderFooterCommand::Insert {
                top: true,
                align: Align::Left,
                of_total: None,
            }));
            for _ in 0..2 {
                worker.handle(Request::Move {
                    motion: Motion::Up,
                    extend: false,
                });
            }
            assert!(
                matches!(worker.story, Story::Body),
                "an arrow moved the caret into the header"
            );
            assert!(matches!(
                worker.caret.as_ref().map(|caret| &caret.paragraph.story),
                Some(Story::Body)
            ));
            worker.handle(Request::Type("Q".into()));
            let body = worker.text_of(&Address::body(0));
            assert!(body.contains('Q'), "{body}");
            let header_has_q = worker
                .session
                .document()
                .headers_footers
                .iter()
                .any(|header| {
                    worker
                        .paragraphs(&Story::HeaderFooter(header.part.clone()))
                        .iter()
                        .any(|at| worker.text_of(at).contains('Q'))
                });
            assert!(!header_has_q, "typing went into the header");

            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.handle(Request::Insert(Insert::PageBreak));
            worker.relayout();
            assert!(worker.pages.len() >= 2, "{}", worker.pages.len());
            worker.handle(Request::Click {
                page: 1,
                x: 200.0,
                y: 20.0,
                extend: false,
                column: false,
            });
            assert!(matches!(worker.story, Story::Body));

            let entered = worker.handle(Request::SelectWord {
                page: 1,
                x: 200.0,
                y: 20.0,
            });
            assert!(matches!(entered, Outcome::Edited));
            assert!(matches!(worker.story, Story::HeaderFooter(_)));
            assert_eq!(worker.header_page, Some(1));
            worker.handle(Request::Type("Head".into()));
            worker.relayout();
            let rects = worker.caret_rects();
            assert!(!rects.is_empty(), "{:?}", worker.caret);
            assert!(rects.iter().all(|rect| rect.page == 1), "{rects:?}");

            worker.handle(Request::HeaderFooter(HeaderFooterCommand::Exit));
            assert!(matches!(worker.story, Story::Body));
            assert_eq!(worker.header_page, None);

            worker.handle(Request::HeaderFooter(HeaderFooterCommand::Enter {
                page: 0,
                header: true,
            }));
            assert!(matches!(worker.story, Story::HeaderFooter(_)));
            assert_eq!(worker.header_page, Some(0));
            let rects = worker.caret_rects();
            assert!(!rects.is_empty());
            assert!(rects.iter().all(|rect| rect.page == 0), "{rects:?}");
        });
    }

    /// Page numbers go into a footer made for them; a double click above
    /// the text makes and enters a header, typing goes there, Esc leaves.
    #[test]
    fn headers_footers_and_page_numbers() {
        let (requests, replies) = new_document();
        requests.send(Request::Type("body".to_owned())).unwrap();
        requests
            .send(Request::HeaderFooter(HeaderFooterCommand::Insert {
                top: false,
                align: Align::Center,
                of_total: Some(("Page ".to_owned(), " of ".to_owned())),
            }))
            .unwrap();
        let texts = loop {
            let texts = page_texts(&replies);
            if texts.iter().any(|(_, t)| t.contains("of")) {
                break texts;
            }
        };
        let body_y = texts.iter().find(|(_, t)| t == "body").expect("body").0;
        let number = texts
            .iter()
            .find(|(_, t)| t.trim() == "1")
            .unwrap_or_else(|| panic!("{texts:?}"));
        assert!(number.0 > body_y + 500.0, "{texts:?}");
        // Into a header made by a double click in the top margin.
        requests
            .send(Request::SelectWord {
                page: 0,
                x: 200.0,
                y: 20.0,
            })
            .unwrap();
        let view = loop {
            let cursor = cursor(&replies);
            if let Some(view) = cursor.header_footer {
                break view;
            }
        };
        assert!(view.header);
        requests.send(Request::Type("Head".to_owned())).unwrap();
        let texts = loop {
            let texts = page_texts(&replies);
            if texts.iter().any(|(_, t)| t == "Head") {
                break texts;
            }
        };
        let head_y = texts.iter().find(|(_, t)| t == "Head").expect("head").0;
        assert!(head_y < body_y);
        requests
            .send(Request::HeaderFooter(HeaderFooterCommand::Exit))
            .unwrap();
        loop {
            if cursor(&replies).header_footer.is_none() {
                break;
            }
        }
        // Typing goes to the body again.
        requests.send(Request::Type("!".to_owned())).unwrap();
        loop {
            let texts = page_texts(&replies);
            if texts.iter().any(|(_, t)| t.contains("body!")) {
                break;
            }
        }
        // A first page of its own: its header is another one, empty.
        requests
            .send(Request::HeaderFooter(HeaderFooterCommand::FirstPage(true)))
            .unwrap();
        loop {
            let texts = page_texts(&replies);
            if !texts.iter().any(|(_, t)| t == "Head") {
                break;
            }
        }
        requests
            .send(Request::HeaderFooter(HeaderFooterCommand::Enter {
                page: 0,
                header: true,
            }))
            .unwrap();
        let view = loop {
            if let Some(view) = cursor(&replies).header_footer {
                break view;
            }
        };
        assert_eq!(
            view.kind,
            strict_ooxml::model::props::HeaderFooterKind::First
        );
        assert!(view.first_page);
        requests.send(Request::Shutdown).unwrap();
    }

    /// Page numbers on real files: `MUFOED_PROBE_FILES` (`;`-separated).
    #[test]
    #[ignore = "probe over files named in MUFOED_PROBE_FILES"]
    fn page_numbers_on_files() {
        let files = std::env::var("MUFOED_PROBE_FILES").unwrap_or_default();
        for file in files.split(';').filter(|f| !f.is_empty()) {
            let ctx = eframe::egui::Context::default();
            let (requests, replies) = spawn(
                Source::File {
                    path: std::path::PathBuf::from(file),
                    import: crate::formats::ImportOptions::default(),
                },
                false,
                OpenControl::new(),
                ctx,
            );
            let _ = state(&replies);
            requests
                .send(Request::HeaderFooter(HeaderFooterCommand::Insert {
                    top: false,
                    align: Align::Right,
                    of_total: Some(("PPP ".to_owned(), " / ".to_owned())),
                }))
                .unwrap();
            let mut found = false;
            let mut notes = Vec::new();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
            while std::time::Instant::now() < deadline {
                match replies.recv_timeout(std::time::Duration::from_secs(120)) {
                    Ok(Reply::Pages { pages, .. }) => {
                        found = pages.first().is_some_and(|page| {
                            page.placed
                                .items
                                .iter()
                                .flat_map(strict_ooxml_render_svg::Item::leaves)
                                .any(|i| matches!(i, strict_ooxml_render_svg::Item::Text(t) if t.text.contains("PPP")))
                        });
                        if found {
                            break;
                        }
                    }
                    Ok(Reply::Note(note)) => notes.push(format!("{} {}", note.key, note.detail)),
                    Ok(Reply::State(_)) if !notes.is_empty() => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            eprintln!("{file}: found {found} notes {notes:?}");
            requests.send(Request::Shutdown).unwrap();
        }
    }

    /// A superscript switched on with nothing selected raises what is typed
    /// next, not the word before it; indent steps move the paragraph; an
    /// underline style shows at the caret.
    #[test]
    fn scripts_indents_and_underline_styles() {
        let (requests, replies) = new_document();
        send_all(
            &requests,
            vec![
                Request::Type("x".to_owned()),
                Request::Format(FormatChange::Superscript),
                Request::Type("2".to_owned()),
            ],
        );
        let texts = loop {
            let texts = page_texts(&replies);
            if texts.iter().any(|(_, t)| t == "2") {
                break texts;
            }
        };
        let x = texts.iter().find(|(_, t)| t == "x").expect("x").0;
        let two = texts.iter().find(|(_, t)| t == "2").expect("2").0;
        assert!(two < x - 1.0, "{texts:?}");
        // Indent one step in and out.
        let left = |replies: &Receiver<Reply>| loop {
            let cursor = cursor(replies);
            if let Some(indent) = cursor.ruler.and_then(|r| r.indent) {
                break indent[0];
            }
        };
        requests.send(Request::IndentStep(true)).unwrap();
        loop {
            if left(&replies) == 720 {
                break;
            }
        }
        requests.send(Request::IndentStep(false)).unwrap();
        loop {
            if left(&replies) == 0 {
                break;
            }
        }
        // A wavy underline on the selection.
        send_all(
            &requests,
            vec![
                Request::SelectAll,
                Request::Format(FormatChange::UnderlineStyle(Some(Underline::Wave))),
            ],
        );
        loop {
            if cursor(&replies).format.underline_style == Some(Underline::Wave) {
                break;
            }
        }
        requests.send(Request::Shutdown).unwrap();
    }

    /// A footnote takes the caret into its text; bookmarks are added, found
    /// and deleted; a table of contents lists the headings and a Ctrl+click
    /// on an entry goes to its heading.
    #[test]
    fn notes_bookmarks_and_contents() {
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(Source::New, false, OpenControl::new(), ctx);
        let info = wait(&replies, |reply| match reply {
            Reply::Loaded(info) => Some(info),
            _ => None,
        });
        let heading = info
            .styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case("heading 1"))
            .expect("a heading style")
            .id
            .clone();
        let _ = state(&replies);
        send_all(
            &requests,
            vec![
                Request::Type("Chapter".to_owned()),
                Request::Style(heading),
                Request::Enter,
                Request::Type("Text".to_owned()),
                Request::References(ReferenceCommand::InsertNote { endnote: false }),
                Request::Type("Note".to_owned()),
            ],
        );
        loop {
            let texts = page_texts(&replies);
            if texts.iter().any(|(_, t)| t.contains("Note")) {
                break;
            }
        }
        // Back to the body: a bookmark on the heading.
        send_all(
            &requests,
            vec![
                Request::References(ReferenceCommand::NextNote { back: true }),
                Request::Move {
                    motion: Motion::DocStart,
                    extend: false,
                },
                Request::References(ReferenceCommand::AddBookmark("Start".to_owned())),
            ],
        );
        loop {
            if state(&replies).bookmarks == ["Start".to_owned()] {
                break;
            }
        }
        // The table of contents, before the first paragraph.
        requests
            .send(Request::References(ReferenceCommand::TableOfContents {
                title: "Contents".to_owned(),
            }))
            .unwrap();
        let texts = loop {
            let texts = page_texts(&replies);
            if texts.iter().any(|(_, t)| t.contains("Contents")) {
                break texts;
            }
        };
        assert!(state(&replies).table_of_contents);
        let entry = texts
            .iter()
            .filter(|(_, t)| t.contains("Chapter"))
            .map(|(y, _)| *y)
            .fold(f64::INFINITY, f64::min);
        let chapter = texts
            .iter()
            .filter(|(_, t)| t.contains("Chapter"))
            .map(|(y, _)| *y)
            .fold(0.0, f64::max);
        assert!(entry < chapter, "{texts:?}");
        requests
            .send(Request::References(ReferenceCommand::DeleteBookmark(
                "Start".to_owned(),
            )))
            .unwrap();
        loop {
            if state(&replies).bookmarks.is_empty() {
                break;
            }
        }
        requests.send(Request::Shutdown).unwrap();
    }

    fn comment_list(replies: &Receiver<Reply>) -> Vec<CommentView> {
        wait(replies, |reply| match reply {
            Reply::Comments(list) => Some(list),
            Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
            _ => None,
        })
    }

    /// A comment on the word at the caret: added empty, given its text,
    /// listed with where its word is, deleted with its marks.
    #[test]
    fn comments_are_added_edited_and_deleted() {
        let (requests, replies) = new_document();
        send_all(
            &requests,
            vec![
                Request::Type("one two".to_owned()),
                Request::Comments(CommentCommand::Add {
                    author: "Ann".to_owned(),
                    initials: "A".to_owned(),
                }),
            ],
        );
        let id = wait(&replies, |reply| match reply {
            Reply::CommentAdded(id) => Some(id),
            _ => None,
        });
        requests
            .send(Request::Comments(CommentCommand::Edit {
                id: id.clone(),
                text: "Why two?".to_owned(),
            }))
            .unwrap();
        let list = loop {
            let list = comment_list(&replies);
            if list.first().is_some_and(|c| c.text == "Why two?") {
                break list;
            }
        };
        let comment = list.first().expect("the comment");
        assert_eq!(comment.author, "Ann");
        assert!(
            comment.date.ends_with('Z') && comment.date.len() == 20,
            "{}",
            comment.date
        );
        // The word, on one line (one rect per run it falls in).
        let width: f64 = comment.rects.iter().map(|r| r.width).sum();
        assert!(width > 10.0, "{:?}", comment.rects);
        assert!(comment
            .rects
            .iter()
            .all(|r| (r.y - comment.rects[0].y).abs() < 0.5));
        requests
            .send(Request::Comments(CommentCommand::Delete(id)))
            .unwrap();
        loop {
            if comment_list(&replies).is_empty() {
                break;
            }
        }
        assert_eq!(body_text(&requests, &replies), "one two");
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

    /// Columns set from the Layout tab reach the ruler: two equal ones
    /// with their gap, then ones of their own widths.
    #[test]
    fn columns_reach_the_ruler() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("abc"));
        requests
            .send(Request::Page(PageChange::Columns {
                count: 2,
                space: 708,
                separator: true,
                widths: None,
            }))
            .unwrap();
        let ruler = wait(&replies, |reply| match reply {
            Reply::Cursor(cursor) => cursor.ruler.filter(|r| r.columns.len() == 2),
            _ => None,
        });
        let [(_, a_end), (b_start, _)] = ruler.columns[..] else {
            panic!("{:?}", ruler.columns);
        };
        assert_eq!(b_start - a_end, 708);
        assert!(ruler.separator && !ruler.unequal);
        requests
            .send(Request::Page(PageChange::Columns {
                count: 2,
                space: 708,
                separator: false,
                widths: Some(vec![2835, 5953]),
            }))
            .unwrap();
        let ruler = wait(&replies, |reply| match reply {
            Reply::Cursor(cursor) => cursor.ruler.filter(|r| r.unequal),
            _ => None,
        });
        let widths: Vec<i32> = ruler.columns.iter().map(|(a, b)| b - a).collect();
        assert_eq!(widths, vec![2835, 5953]);
        // A column break goes in at the caret, which stays after it.
        send_all(&requests, vec![Request::Insert(Insert::ColumnBreak)]);
        send_all(&requests, typed("d"));
        let text = settle(&requests, &replies);
        assert_eq!(text.chars().filter(|c| *c == 'd').count(), 1, "{text:?}");
        assert!(
            text.starts_with("abc") && text.ends_with('d') && text.len() > 4,
            "{text:?}"
        );
        requests.send(Request::Shutdown).unwrap();
    }

    /// Misspelt words come back marked, and one is replaced from the menu.
    #[test]
    fn misspelt_words_are_marked_and_replaced() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("кот котт"));
        let speller = crate::spell::Speller::new(
            vec![(
                crate::spell::Script::Cyrillic,
                crate::spell::Dictionary::parse(
                    "SET UTF-8
",
                    "1
кот
",
                ),
            )],
            std::collections::HashSet::new(),
            None,
        );
        requests
            .send(Request::Speller(Some(Arc::new(speller))))
            .unwrap();
        let (errors, missing) = wait(&replies, |reply| match reply {
            Reply::Spelling { errors, missing } if !errors.is_empty() => Some((errors, missing)),
            _ => None,
        });
        assert!(missing.is_empty());
        let [error] = &errors[..] else {
            panic!("{errors:?}");
        };
        assert_eq!(error.word, "котт");
        assert!(!error.rects.is_empty());
        requests
            .send(Request::ReplaceWord {
                start: error.start.clone(),
                end: error.end.clone(),
                word: error.word.clone(),
                with: "кот".to_owned(),
            })
            .unwrap();
        assert_eq!(settle(&requests, &replies), "кот кот");
        requests.send(Request::Shutdown).unwrap();
    }

    /// A dictionary fetched while a pass is in flight has to be the one that
    /// marks the open document. The pass that started earlier must not put its
    /// verdicts back for the next one to reuse.
    #[test]
    fn a_fetched_dictionary_is_applied_to_the_open_document() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("und Haus"));
        let latin = |dic: &str| {
            crate::spell::Speller::new(
                vec![(
                    crate::spell::Script::Latin,
                    crate::spell::Dictionary::parse("SET UTF-8\n", dic),
                )],
                std::collections::HashSet::new(),
                None,
            )
        };
        requests
            .send(Request::Speller(Some(Arc::new(latin("1\nthe\n")))))
            .unwrap();
        let (errors, _) = wait(&replies, |reply| match reply {
            Reply::Spelling { errors, missing } if errors.len() == 2 => Some((errors, missing)),
            _ => None,
        });
        assert_eq!(
            errors
                .iter()
                .map(|error| error.word.as_str())
                .collect::<Vec<_>>(),
            ["und", "Haus"]
        );
        requests
            .send(Request::Speller(Some(Arc::new(latin("2\nund\nHaus\n")))))
            .unwrap();
        let errors = wait(&replies, |reply| match reply {
            Reply::Spelling { errors, .. } if errors.is_empty() => Some(errors),
            _ => None,
        });
        assert!(errors.is_empty(), "{errors:?}");
        requests.send(Request::Shutdown).unwrap();
    }

    /// The pass that started before a dictionary arrived must not put its
    /// verdicts back afterwards.
    #[test]
    fn an_older_spelling_pass_does_not_keep_its_verdicts() {
        let sync = std::sync::Mutex::new(SpellSync {
            epoch: 1,
            cache: std::collections::HashMap::new(),
        });
        let stale = std::collections::HashMap::from([(
            "und".to_owned(),
            Arc::new((
                vec![crate::spell::Word {
                    start: 0,
                    end: 3,
                    text: "und".to_owned(),
                }],
                Vec::new(),
            )),
        )]);
        {
            let mut gate = spell_gate(&sync);
            gate.cache.clear();
            gate.epoch = gate.epoch.wrapping_add(1);
        }
        let mut gate = spell_gate(&sync);
        assert!(!publish_spelling(&mut gate, 1, stale));
        assert!(gate.cache.is_empty());
    }

    /// A paragraph of words and an inline formula is sent with the formula
    /// as a fragment, and a rewrite that keeps the fragment keeps the formula.
    #[test]
    fn a_mixed_paragraph_keeps_its_formula_when_the_words_change() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("энергия  достаточна"));
        for _ in 0.." достаточна".chars().count() {
            requests
                .send(Request::Move {
                    motion: Motion::Left,
                    extend: false,
                })
                .unwrap();
        }
        requests
            .send(Request::PutFormula {
                expression: Box::new(crate::formula::from_latex("E=mc^2").unwrap()),
                replace: None,
            })
            .unwrap();
        requests
            .send(Request::AiSource {
                whole: true,
                job: 4,
            })
            .unwrap();
        let source = wait(&replies, |reply| match reply {
            Reply::AiSource { job: 4, source } => Some(source),
            _ => None,
        });
        let (at, text) = source
            .paragraphs
            .iter()
            .find(|(_, text)| text.contains("энергия") && text.contains('⟦'))
            .unwrap()
            .clone();
        assert!(text.contains("достаточна"), "{text}");
        assert!(!text.contains(nav::OBJECT), "{text}");
        let rewritten = text
            .replace("энергия", "energy")
            .replace("достаточна", "enough");
        requests
            .send(Request::SetParagraphText {
                at: at.clone(),
                old: text.clone(),
                new: rewritten.clone(),
            })
            .unwrap();
        requests
            .send(Request::SetParagraphText {
                at: at.clone(),
                old: rewritten.clone(),
                new: "gone".to_owned(),
            })
            .unwrap();
        requests
            .send(Request::AiSource {
                whole: true,
                job: 5,
            })
            .unwrap();
        let again = wait(&replies, |reply| match reply {
            Reply::AiSource { job: 5, source } => Some(source),
            _ => None,
        });
        let kept = again
            .paragraphs
            .iter()
            .find(|(address, _)| *address == at)
            .map(|(_, text)| text.as_str())
            .unwrap();
        assert_eq!(kept, rewritten);
        requests.send(Request::Shutdown).unwrap();
    }

    /// The AI tab's text comes back by paragraph; a paragraph's text and
    /// its style are set from what the model answered.
    #[test]
    fn ai_results_change_the_text_and_the_styles() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("Введение"));
        send_all(&requests, vec![Request::Enter]);
        send_all(&requests, typed("Текст статьи."));
        requests
            .send(Request::AiSource {
                whole: true,
                job: 7,
            })
            .unwrap();
        let source = wait(&replies, |reply| match reply {
            Reply::AiSource { job: 7, source } => Some(source),
            _ => None,
        });
        let texts: Vec<&str> = source
            .paragraphs
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(texts, ["Введение", "Текст статьи."]);
        let (first, second) = (
            source.paragraphs[0].0.clone(),
            source.paragraphs[1].0.clone(),
        );
        requests
            .send(Request::SetParagraphText {
                at: second.clone(),
                old: "Текст статьи.".to_owned(),
                new: "The article.".to_owned(),
            })
            .unwrap();
        // A paragraph that no longer reads as it did is left alone.
        requests
            .send(Request::SetParagraphText {
                at: first.clone(),
                old: "Другое".to_owned(),
                new: "Nope".to_owned(),
            })
            .unwrap();
        assert_eq!(settle(&requests, &replies), "Введение\nThe article.");
        requests
            .send(Request::ApplyKinds(vec![
                (first.clone(), crate::ai::Kind::Heading(1)),
                (second, crate::ai::Kind::Body),
            ]))
            .unwrap();
        requests.send(Request::GoTo(first)).unwrap();
        let style = wait(&replies, |reply| match reply {
            Reply::Cursor(cursor) => cursor.format.style.clone().filter(|s| s == "Heading1"),
            _ => None,
        });
        assert_eq!(style, "Heading1");
        requests.send(Request::Shutdown).unwrap();
    }

    /// A formula goes in at the caret, is found there again and replaced.
    #[test]
    fn a_formula_is_inserted_found_and_replaced() {
        let (requests, replies) = new_document();
        send_all(&requests, typed("a"));
        let square = crate::formula::from_latex("x^{2}").unwrap();
        requests
            .send(Request::PutFormula {
                expression: Box::new(square),
                replace: None,
            })
            .unwrap();
        requests.send(Request::FormulaAtCaret).unwrap();
        let found = wait(&replies, |reply| match reply {
            Reply::Formula(found) => Some(found),
            _ => None,
        })
        .expect("the formula at the caret");
        let (at, index, expression) = *found;
        assert_eq!(crate::formula::to_latex(&expression), "{x}^{2}");
        let cube = crate::formula::from_latex("x^{3}").unwrap();
        requests
            .send(Request::PutFormula {
                expression: Box::new(cube),
                replace: Some((at, index)),
            })
            .unwrap();
        requests.send(Request::FormulaAtCaret).unwrap();
        let found = wait(&replies, |reply| match reply {
            Reply::Formula(found) => Some(found),
            _ => None,
        })
        .expect("still there");
        assert_eq!(crate::formula::to_latex(&found.2), "{x}^{3}");
        requests.send(Request::Shutdown).unwrap();
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

    #[test]
    fn an_edit_refusal_names_the_reason() {
        use strict_ooxml::edit::{EditError, Invariant, Unsupported};
        assert_eq!(
            edit_notice(&EditError::UnsupportedContent(Unsupported::FieldResult)).0,
            "note.field_text"
        );
        assert_eq!(
            edit_notice(&EditError::UnsupportedContent(Unsupported::TrackedChange)).0,
            "note.tracked_text"
        );
        assert_eq!(
            edit_notice(&EditError::InvalidModel(Invariant::Field)).0,
            "note.model.field"
        );
        let (key, detail) = edit_notice(&EditError::UnknownStyle(
            strict_ooxml::model::StyleId::new("Heading1"),
        ));
        assert_eq!(key, "note.unknown_style");
        assert_eq!(detail, "Heading1");
        assert_eq!(stored_notice(&EditError::LimitExceeded), "note.edit_limit");
    }

    fn take_view(
        worker: &mut Worker<'_, '_>,
        replies: &Receiver<Reply>,
        job: u64,
    ) -> crate::chat::View {
        while replies.try_recv().is_ok() {}
        worker.handle(Request::ChatSnapshot { job });
        replies
            .try_iter()
            .find_map(|reply| match reply {
                Reply::ChatSnapshot { view, .. } => Some(*view),
                Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
                _ => None,
            })
            .expect("chat snapshot")
    }

    fn texts_of(view: &crate::chat::View) -> Vec<&str> {
        view.paragraphs
            .iter()
            .map(|paragraph| paragraph.text.as_str())
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Search through the worker finds both occurrences and does not move the caret.
    #[test]
    fn chat_find_locates_both_occurrences_and_leaves_the_document() {
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Type("alpha here".into()));
            worker.handle(Request::Enter);
            worker.handle(Request::Type("and alpha again".into()));
            let before = take_view(worker, replies, 1);
            worker.handle(Request::ChatFind {
                query: "alpha".into(),
            });
            let (hits, caret) = replies
                .try_iter()
                .find_map(|reply| match reply {
                    Reply::ChatFind { hits, caret } => Some((hits, caret)),
                    Reply::Note(note) if note.error => panic!("{}: {}", note.key, note.detail),
                    _ => None,
                })
                .expect("hits");
            assert_eq!(hits.len(), 2);
            assert_eq!(caret, before.caret);
            let after = take_view(worker, replies, 2);
            assert_eq!(texts_of(&after), texts_of(&before));
        });
    }

    /// A stale `old` is skipped. One undo restores every paragraph the batch changed.
    #[test]
    fn a_chat_batch_skips_a_stale_paragraph_and_undoes_as_one_step() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Type("One".into()));
            worker.handle(Request::Enter);
            worker.handle(Request::Type("Two".into()));
            let view = take_view(worker, replies, 1);
            let one = view
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.text == "One")
                .unwrap();
            let two = view
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.text == "Two")
                .unwrap();
            let depth = worker.undo_depth;
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "replace".into(),
                lines: Vec::new(),
                ops: vec![
                    Op::Replace {
                        index: one.index,
                        old: "Nope".into(),
                        new: "No".into(),
                    },
                    Op::Replace {
                        index: two.index,
                        old: "Two".into(),
                        new: "Dos".into(),
                    },
                ],
            })));
            assert_eq!(worker.undo_depth, depth + 1);
            let changed = take_view(worker, replies, 2);
            assert!(texts_of(&changed).contains(&"One"));
            assert!(texts_of(&changed).contains(&"Dos"));
            assert!(!texts_of(&changed).contains(&"No"));
            worker.handle(Request::Undo);
            assert_eq!(worker.undo_depth, depth);
            let restored = take_view(worker, replies, 3);
            assert!(texts_of(&restored).contains(&"Two"));
            assert!(!texts_of(&restored).contains(&"Dos"));
        });
    }

    /// Underline lands on the named words only. One undo clears the whole batch.
    #[test]
    fn chat_underlines_the_named_spans_and_one_undo_clears_them() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Type("alpha BRAVO tail".into()));
            let bold = worker.caret.clone().unwrap();
            worker.anchor = Some(Logical {
                paragraph: bold.paragraph.clone(),
                offset: 6,
            });
            worker.caret = Some(Logical {
                paragraph: bold.paragraph,
                offset: 11,
            });
            worker.handle(Request::Format(FormatChange::Bold));
            worker.anchor = None;
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.handle(Request::Enter);
            worker.handle(Request::Type("alpha alpha".into()));
            worker.handle(Request::Enter);
            worker.handle(Request::Type("штатный штат".into()));
            worker.handle(Request::Enter);
            worker.handle(Request::Type("alpha".into()));
            let view = take_view(worker, replies, 1);
            let at = |text: &str| {
                view.paragraphs
                    .iter()
                    .find(|paragraph| paragraph.text == text)
                    .unwrap()
                    .index
            };
            let depth = worker.undo_depth;
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "underline".into(),
                lines: Vec::new(),
                ops: vec![
                    Op::Underline {
                        index: at("alpha BRAVO tail"),
                        old: "alpha BRAVO tail".into(),
                        spans: vec![(0, 5)],
                    },
                    Op::Underline {
                        index: at("alpha alpha"),
                        old: "alpha alpha".into(),
                        spans: vec![(0, 5), (6, 11)],
                    },
                    Op::Underline {
                        index: at("штатный штат"),
                        old: "штатный штат".into(),
                        spans: vec![(8, 12)],
                    },
                    Op::Underline {
                        index: at("alpha"),
                        old: "gone".into(),
                        spans: vec![(0, 5)],
                    },
                ],
            })));
            assert_eq!(worker.undo_depth, depth + 1);
            let marked = take_view(worker, replies, 2);
            let runs = |text: &str| {
                let index = marked
                    .paragraphs
                    .iter()
                    .find(|paragraph| paragraph.text == text)
                    .unwrap()
                    .index;
                marked
                    .runs
                    .get(
                        marked
                            .paragraphs
                            .iter()
                            .position(|p| p.index == index)
                            .unwrap(),
                    )
                    .unwrap()
                    .clone()
            };
            let mixed = runs("alpha BRAVO tail");
            assert!(mixed
                .iter()
                .any(|run| run.text == "alpha" && run.underline && !run.bold));
            assert!(mixed
                .iter()
                .any(|run| run.text == "BRAVO" && run.bold && !run.underline));
            let both = runs("alpha alpha");
            assert_eq!(
                both.iter()
                    .filter(|run| run.text == "alpha" && run.underline)
                    .count(),
                2
            );
            let inside = runs("штатный штат");
            assert!(inside
                .iter()
                .any(|run| run.text.starts_with("штатн") && !run.underline));
            assert!(inside.iter().any(|run| run.text == "штат" && run.underline));
            let stale = runs("alpha");
            assert!(stale.iter().all(|run| !run.underline));
            worker.handle(Request::Undo);
            let cleared = take_view(worker, replies, 3);
            assert!(cleared.runs.iter().flatten().all(|run| !run.underline));
            let bravo = cleared
                .runs
                .iter()
                .flatten()
                .find(|run| run.text == "BRAVO")
                .unwrap();
            assert!(bravo.bold);
        });
    }

    /// Confirmed forms are replaced with the case the model supplied. A longer word stays.
    #[test]
    fn chat_replaces_confirmed_forms_and_keeps_a_longer_word() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Type("Штате и штате и штатный".into()));
            let view = take_view(worker, replies, 1);
            let index = view.paragraphs[0].index;
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "forms".into(),
                lines: Vec::new(),
                ops: vec![Op::Forms {
                    index,
                    old: "Штате и штате и штатный".into(),
                    pairs: vec![
                        ("штате".into(), "регионе".into()),
                        ("Штате".into(), "Регионе".into()),
                    ],
                }],
            })));
            let after = take_view(worker, replies, 2);
            assert_eq!(texts_of(&after), vec!["Регионе и регионе и штатный"]);
        });
    }

    /// A comment is anchored to the span. The paragraph's words stay.
    #[test]
    fn a_chat_comment_is_anchored_and_the_words_stay() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            let sentence = "see California now";
            worker.handle(Request::Type(sentence.into()));
            let view = take_view(worker, replies, 1);
            let index = view.paragraphs[0].index;
            let start = sentence.find("California").unwrap();
            let start = sentence[..start].chars().count();
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "comment".into(),
                lines: Vec::new(),
                ops: vec![Op::Comment {
                    index,
                    old: sentence.into(),
                    start,
                    end: start + "California".chars().count(),
                    text: "check the name".into(),
                }],
            })));
            let after = take_view(worker, replies, 2);
            assert_eq!(texts_of(&after), vec![sentence]);
            assert_eq!(after.comments.len(), 1);
            assert_eq!(after.comments[0].text, "check the name");
            assert_eq!(after.comments[0].paragraph, index);
        });
    }

    /// Moving a section keeps the words and changes their order and the heading style.
    #[test]
    fn chat_moves_a_section_and_restyles_its_heading() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            for text in ["Alpha", "body a", "Beta", "body b", "End"] {
                worker.handle(Request::Type(text.into()));
                worker.handle(Request::Enter);
            }
            let before = take_view(worker, replies, 1);
            assert_eq!(
                texts_of(&before),
                vec!["Alpha", "body a", "Beta", "body b", "End"]
            );
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "move".into(),
                lines: Vec::new(),
                ops: vec![Op::Move {
                    start: before.paragraphs[0].index,
                    count: 2,
                    to: before.paragraphs[4].index,
                    level: Some(2),
                }],
            })));
            let after = take_view(worker, replies, 2);
            assert_eq!(
                texts_of(&after),
                vec!["Beta", "body b", "Alpha", "body a", "End"]
            );
            let alpha = after
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.text == "Alpha")
                .unwrap();
            assert_eq!(alpha.style, "Heading2");
        });
    }

    /// Chosen display formulas are numbered in document order. An inline formula is not.
    #[test]
    fn chat_numbers_displayed_formulas_and_a_reference_field() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            assert!(worker.insert_display("a=1"));
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.handle(Request::Enter);
            worker.handle(Request::Type("Between".into()));
            worker.handle(Request::Enter);
            assert!(worker.insert_display("b=2"));
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.handle(Request::Enter);
            worker.handle(Request::Type("and ".into()));
            worker.handle(Request::PutFormula {
                expression: Box::new(crate::formula::from_latex("c=3").unwrap()),
                replace: None,
            });
            worker.handle(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            worker.handle(Request::Enter);
            assert!(worker.insert_display("d=4"));
            let before = take_view(worker, replies, 1);
            let displayed: Vec<_> = before
                .formulas
                .iter()
                .filter(|formula| formula.display)
                .collect();
            assert_eq!(displayed.len(), 3, "{:?}", texts_of(&before));
            let bodies: Vec<String> = displayed
                .iter()
                .map(|formula| formula.latex.clone())
                .collect();
            assert!(before.formulas.iter().any(|formula| !formula.display));
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "numbers".into(),
                lines: Vec::new(),
                ops: vec![Op::Number {
                    ids: vec!["f2".into()],
                }],
            })));
            let numbered = take_view(worker, replies, 2);
            let f2 = numbered
                .formulas
                .iter()
                .find(|formula| formula.id == "f2")
                .unwrap();
            assert_eq!(f2.number.as_deref(), Some("1"));
            let between = numbered
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.text == "Between")
                .unwrap();
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "cite".into(),
                lines: Vec::new(),
                ops: vec![Op::Cite {
                    index: between.index,
                    old: between.text.clone(),
                    formula: "f2".into(),
                }],
            })));
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "renumber".into(),
                lines: Vec::new(),
                ops: vec![Op::Number {
                    ids: vec!["f2".into(), "f1".into()],
                }],
            })));
            let after = take_view(worker, replies, 3);
            let number = |id: &str| {
                after
                    .formulas
                    .iter()
                    .find(|formula| formula.id == id)
                    .and_then(|formula| formula.number.clone())
            };
            assert_eq!(number("f1").as_deref(), Some("1"));
            assert_eq!(number("f2").as_deref(), Some("2"));
            assert_eq!(number("f3"), None);
            assert!(after
                .formulas
                .iter()
                .filter(|formula| !formula.display)
                .all(|formula| formula.number.is_none()));
            let after_bodies: Vec<String> = after
                .formulas
                .iter()
                .filter(|formula| formula.display)
                .map(|formula| formula.latex.clone())
                .collect();
            assert_eq!(after_bodies, bodies);
            assert!(
                after.fields.iter().any(|field| {
                    field
                        .instruction
                        .split_whitespace()
                        .any(|word| word == "eqf2")
                        && field.text == "2"
                }),
                "{:?}",
                after.fields
            );
        });
    }

    /// The first table row becomes a repeating header.
    #[test]
    fn chat_marks_the_first_table_row_as_a_header() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Insert(Insert::Table {
                rows: 2,
                columns: 2,
            }));
            let before = take_view(worker, replies, 1);
            assert_eq!(before.tables.len(), 1);
            assert!(!before.tables[0].header);
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "table_header".into(),
                lines: Vec::new(),
                ops: vec![Op::TableHeader],
            })));
            let after = take_view(worker, replies, 2);
            assert!(after.tables[0].header);
        });
    }

    /// A chart is built from the numbers in the table.
    #[test]
    fn chat_charts_the_numbers_in_a_table() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Insert(Insert::Table {
                rows: 1,
                columns: 2,
            }));
            worker.handle(Request::Type("10".into()));
            worker.handle(Request::Tab { back: false });
            worker.handle(Request::Type("20".into()));
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "chart".into(),
                lines: Vec::new(),
                ops: vec![Op::Chart {
                    title: "Results".into(),
                }],
            })));
            let after = take_view(worker, replies, 1);
            assert!(after.charts >= 1, "charts {}", after.charts);
        });
    }

    /// A caption paragraph is inserted after a picture.
    #[test]
    fn chat_captions_a_picture() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            worker.handle(Request::Type("Above".into()));
            worker.handle(Request::Object(ObjectCommand::InsertPicture {
                bytes: png(8, 8),
                name: "dot.png".into(),
            }));
            let before = take_view(worker, replies, 1);
            let index = *before.pictures.first().expect("picture");
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "caption".into(),
                lines: Vec::new(),
                ops: vec![Op::Caption {
                    index,
                    text: "Figure 1".into(),
                }],
            })));
            let after = take_view(worker, replies, 2);
            assert!(
                after
                    .paragraphs
                    .iter()
                    .any(|paragraph| paragraph.text == "Figure 1"),
                "{:?}",
                texts_of(&after)
            );
            assert!(
                after
                    .paragraphs
                    .iter()
                    .any(|paragraph| paragraph.text.contains("Above")),
                "{:?}",
                texts_of(&after)
            );
        });
    }

    /// New prose lands in an empty document as paragraphs, and one undo removes it.
    #[test]
    fn chat_writes_paragraphs_into_an_empty_document() {
        use crate::chat::{Batch, Op};
        with_worker_replies(|worker, replies| {
            let depth = worker.undo_depth;
            worker.handle(Request::ApplyChat(Box::new(Batch {
                title: "write".into(),
                lines: Vec::new(),
                ops: vec![Op::Write {
                    paragraphs: vec!["Осень.".into(), "Листья падают на дорожки парка.".into()],
                }],
            })));
            assert_eq!(worker.undo_depth, depth + 1);
            let written = take_view(worker, replies, 1);
            assert_eq!(
                texts_of(&written),
                vec!["Осень.", "Листья падают на дорожки парка."]
            );
            worker.handle(Request::Undo);
            let cleared = take_view(worker, replies, 2);
            assert!(texts_of(&cleared).is_empty());
        });
    }
}
