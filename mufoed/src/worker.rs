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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use strict_ooxml::edit::{
    Address, Edit, EditLimits, FormatPatch, OperationLimits, Operations, ReplacePolicy, SavePolicy,
    SearchQuery, SearchScope, Story, TextPosition, TextQuery, VisualMap, VisualRect,
};
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
    DocumentEditor, OpenControl, OpenOptions, Page, RenderOptions, StrictDocument, WriteOptions,
};
use strict_ooxml_core::error::SourceLocation;

use crate::formats::{self, Kind, Source};
use crate::nav::{self, TextNodeRef};

/// What the interface asks of a document.
#[derive(Debug)]
pub(crate) enum Request {
    /// Put the caret at a page point; `extend` keeps the selection anchor.
    Click {
        page: usize,
        x: f64,
        y: f64,
        extend: bool,
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
    Format(FormatChange),
    Align(Align),
    Style(String),
    Insert(Insert),
    Page(PageChange),
    Undo,
    Redo,
    Copy {
        cut: bool,
    },
    Paste(String),
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
    pub(crate) svg: Arc<str>,
    /// Changes when the SVG does, so unchanged pages keep their bitmaps.
    pub(crate) hash: u64,
}

/// A heading for the navigation pane.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutlineEntry {
    pub(crate) level: u8,
    pub(crate) text: String,
    pub(crate) paragraph: Address,
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
}

/// Caret and selection geometry, in page pixels.
pub(crate) struct Cursor {
    pub(crate) carets: Vec<VisualRect>,
    pub(crate) selection: Vec<VisualRect>,
    /// Bring the caret into view.
    pub(crate) scroll: bool,
    pub(crate) format: CaretFormat,
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
    Note(Note),
    Clipboard(String),
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
    let mut options = RenderOptions::default();
    if system_fonts {
        if let Ok(assets) = strict_ooxml::system_fonts::system_fonts_for(document.document()) {
            options = options.font_assets(Arc::new(assets));
        }
    }
    let path = match source {
        Source::File { path, .. } if !loaded.kind.converted() && loaded.kind != Kind::Rtf => {
            Some(path.clone())
        }
        _ => None,
    };
    send(Reply::Loaded(Box::new(DocInfo {
        kind: loaded.kind,
        report: document.report_text(),
        log: loaded.log,
        styles: styles_of(document.document()),
    })));
    let read_only = match document.edit(EditLimits::default()) {
        Ok(editor) => {
            let mut worker = Worker::new(Session::Edit(editor), options, replies, ctx, path);
            worker.serve(requests);
            return;
        }
        Err(error) => format!("{error:?}"),
    };
    let mut worker = Worker::new(Session::View(&document), options, replies, ctx, path);
    worker.read_only = Some(read_only);
    worker.serve(requests);
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

    fn render(&self, options: &RenderOptions) -> Result<Vec<Page>, String> {
        match self {
            Self::Edit(editor) => editor.render_svg(options),
            Self::View(document) => document.render_svg(options),
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
    goal_x: Option<f64>,
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
            goal_x: None,
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
                match self.handle(request) {
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
        match request {
            Request::Shutdown => Outcome::Quit,
            Request::Click { page, x, y, extend } => self.click(page, x, y, extend),
            Request::SelectWord { page, x, y } => self.select_word(page, x, y),
            Request::Move { motion, extend } => self.motion(motion, extend),
            Request::SelectAll => self.select_all(),
            Request::GoTo(paragraph) => {
                self.anchor = None;
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
                self.options.font_assets = None;
                if enabled {
                    if let Ok(assets) =
                        strict_ooxml::system_fonts::system_fonts_for(self.session.document())
                    {
                        self.options.font_assets = Some(Arc::new(assets));
                    }
                }
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
            Request::Format(change) => self.format(&change),
            Request::Align(align) => self.align(align),
            Request::Style(id) => self.set_style(&id),
            Request::Insert(insert) => self.insert(insert),
            Request::Page(change) => self.page_setup(change),
            Request::Undo => self.undo(true),
            Request::Redo => self.undo(false),
            Request::Paste(text) => self.paste(&text),
            Request::Replace { query, with } => self.replace(&query, &with),
            Request::ReplaceAll { query, with } => self.replace_all(&query, &with),
            Request::Revisions { accept } => self.revisions(accept),
            _ => Outcome::Quiet,
        }
    }

    // ----- layout -------------------------------------------------------

    fn relayout(&mut self) {
        self.stale = false;
        match self.session.render(&self.options) {
            Ok(pages) => {
                let mut warnings: Vec<String> = Vec::new();
                let mut out = Vec::with_capacity(pages.len());
                for (index, page) in pages.into_iter().enumerate() {
                    for warning in page.warnings {
                        if !warnings.contains(&warning) {
                            warnings.push(warning);
                        }
                    }
                    let mut hasher = DefaultHasher::new();
                    page.svg.hash(&mut hasher);
                    let hash = hasher.finish();
                    let reuse = self
                        .pages
                        .get(index)
                        .filter(|known| known.hash == hash)
                        .cloned();
                    out.push(reuse.unwrap_or_else(|| {
                        Arc::new(PageData {
                            width: page.width_px,
                            height: page.height_px,
                            svg: Arc::from(page.svg),
                            hash,
                        })
                    }));
                }
                self.page_sizes = out.iter().map(|page| (page.width, page.height)).collect();
                self.pages = out;
                self.send(Reply::Pages {
                    pages: self.pages.clone(),
                    warnings,
                });
            }
            Err(error) => self.note("note.render_failed", error, true),
        }
        self.map = match &self.session {
            Session::Edit(editor) => match editor.visual_map(editor.revision(), &self.options) {
                Ok(map) => Some(map),
                Err(error) => {
                    if self.read_only.is_none() {
                        self.note("note.no_caret_map", error.to_string(), true);
                    }
                    None
                }
            },
            Session::View(_) => None,
        };
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
        let mut out = Vec::new();
        for (index, block) in document.body.blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else {
                continue;
            };
            let Some(level) = heading_level(document, paragraph) else {
                continue;
            };
            let text: String = nav::text_nodes(paragraph)
                .iter()
                .map(|node| node.text.as_str())
                .collect();
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            out.push(OutlineEntry {
                level,
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

    fn nodes(&self, at: &Address) -> Vec<TextNodeRef> {
        self.paragraph(at).map(nav::text_nodes).unwrap_or_default()
    }

    fn text_of(&self, at: &Address) -> String {
        self.nodes(at)
            .iter()
            .map(|node| node.text.as_str())
            .collect()
    }

    /// The kernel position of a caret.
    fn position(&self, logical: &Logical) -> TextPosition {
        let nodes = self.nodes(&logical.paragraph);
        let mut before = 0;
        for node in &nodes {
            let len = node.len();
            if logical.offset <= before + len {
                return TextPosition {
                    paragraph: logical.paragraph.clone(),
                    inline: node.inline.clone(),
                    content: node.content,
                    offset: logical.offset - before,
                };
            }
            before += len;
        }
        match nodes.last() {
            Some(node) => TextPosition {
                paragraph: logical.paragraph.clone(),
                inline: node.inline.clone(),
                content: node.content,
                offset: node.len(),
            },
            None => TextPosition {
                paragraph: logical.paragraph.clone(),
                inline: Vec::new(),
                content: 0,
                offset: 0,
            },
        }
    }

    /// The caret a kernel position stands for.
    fn logical(&self, position: &TextPosition) -> Logical {
        let nodes = self.nodes(&position.paragraph);
        let index = nav::node_index(&nodes, position);
        let before: usize = index.map_or(0, |index| {
            nodes.iter().take(index).map(TextNodeRef::len).sum()
        });
        Logical {
            paragraph: position.paragraph.clone(),
            offset: before + if index.is_some() { position.offset } else { 0 },
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
        let format = self.caret_format();
        self.send(Reply::Cursor(Box::new(Cursor {
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

    fn click(&mut self, page: usize, x: f64, y: f64, extend: bool) -> Outcome {
        self.ensure_layout();
        let revision = self.session.revision();
        let Some(map) = &self.map else {
            return Outcome::Quiet;
        };
        let Ok(Some(position)) = map.hit_test(revision, page, x, y) else {
            return Outcome::Quiet;
        };
        let logical = self.logical(&position);
        self.place(logical, extend);
        self.goal_x = None;
        Outcome::Moved
    }

    fn select_word(&mut self, page: usize, x: f64, y: f64) -> Outcome {
        if matches!(self.click(page, x, y, false), Outcome::Quiet) {
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
        let line = rect.height.max(4.0);
        let pages = self.page_sizes.len();
        let height = |page: usize| self.page_sizes.get(page).map_or(0.0, |size| size.1);
        let (mut page, mut y) = match motion {
            Motion::Up => (rect.page, rect.y - line * 0.5),
            Motion::Down => (rect.page, rect.y + rect.height + line * 0.5),
            Motion::PageUp => (rect.page, rect.y - height(rect.page) * 0.8),
            _ => (rect.page, rect.y + height(rect.page) * 0.8),
        };
        if y < 0.0 {
            if page == 0 {
                y = 0.0;
            } else {
                page -= 1;
                y += height(page);
            }
        } else if y > height(page) && page + 1 < pages {
            y -= height(page);
            page += 1;
        }
        let Some(mut target) = self.hit(page, x, y) else {
            return Outcome::Quiet;
        };
        // The closest line to a point above the first line is the first line;
        // keep going to the previous page in that case.
        if self.caret.as_ref() == Some(&target) || self.same_line(&target, &rect) {
            let next = match motion {
                Motion::Up | Motion::PageUp if page > 0 => self.hit(page - 1, x, height(page - 1)),
                Motion::Down | Motion::PageDown if page + 1 < pages => self.hit(page + 1, x, 0.0),
                _ => None,
            };
            match next {
                Some(next) => target = next,
                None => return Outcome::Quiet,
            }
        }
        self.place(target, extend);
        Outcome::Moved
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

    /// Commands that replace `start..end` with `text`, and whether they apply.
    fn replace_commands(
        &self,
        start: &Logical,
        end: &Logical,
        text: &str,
    ) -> Result<Vec<Edit>, &'static str> {
        let first = self
            .paragraph(&start.paragraph)
            .ok_or("note.no_paragraph")?;
        let start_position = self.position(start);
        if start.paragraph == end.paragraph {
            if start_position.inline.is_empty() {
                // An empty paragraph: nothing to remove, only text to add.
                return Ok(if text.is_empty() {
                    Vec::new()
                } else {
                    vec![Edit::Text {
                        at: start.paragraph.clone(),
                        range: 0..0,
                        text: text.to_owned(),
                    }]
                });
            }
            return Ok(vec![Edit::ReplaceAt {
                at: start.paragraph.clone(),
                from: nav::point_of(&start_position),
                to: nav::point_of(&self.position(end)),
                text: text.to_owned(),
            }]);
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
        let mut commands = Vec::new();
        let last = self.paragraph(&end.paragraph).ok_or("note.no_paragraph")?;
        if let Some(from_point) = nav::start_point(last) {
            let end_position = self.position(end);
            if !end_position.inline.is_empty() {
                commands.push(Edit::ReplaceAt {
                    at: end.paragraph.clone(),
                    from: from_point,
                    to: nav::point_of(&end_position),
                    text: String::new(),
                });
            }
        }
        for middle in span.iter().skip(1).take(span.len().saturating_sub(2)).rev() {
            commands.push(Edit::Delete { at: middle.clone() });
        }
        match nav::end_point(first) {
            Some(to_point) if !start_position.inline.is_empty() => {
                commands.push(Edit::ReplaceAt {
                    at: start.paragraph.clone(),
                    from: nav::point_of(&start_position),
                    to: to_point,
                    text: text.to_owned(),
                });
            }
            _ if !text.is_empty() => commands.push(Edit::Text {
                at: start.paragraph.clone(),
                range: 0..0,
                text: text.to_owned(),
            }),
            _ => {}
        }
        commands.push(Edit::Join {
            at: start.paragraph.clone(),
        });
        Ok(commands)
    }

    /// Replaces the selection (or inserts at the caret) with `text`.
    fn put_text(&mut self, text: &str) -> bool {
        let Some(caret) = self.caret.clone() else {
            return false;
        };
        let (start, end) = self.selection().unwrap_or_else(|| (caret.clone(), caret));
        let commands = if start == end {
            let position = self.position(&start);
            if text.is_empty() {
                return false;
            }
            if position.inline.is_empty() {
                vec![Edit::Text {
                    at: start.paragraph.clone(),
                    range: 0..0,
                    text: text.to_owned(),
                }]
            } else {
                vec![Edit::TextNode {
                    at: start.paragraph.clone(),
                    inline: position.inline.clone(),
                    content: position.content,
                    range: position.offset..position.offset,
                    text: text.to_owned(),
                }]
            }
        } else {
            match self.replace_commands(&start, &end, text) {
                Ok(commands) => commands,
                Err(key) => {
                    self.note(key, String::new(), true);
                    return false;
                }
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
        let clean: String = text
            .chars()
            .map(|c| if c == '\t' { ' ' } else { c })
            .filter(|c| !c.is_control())
            .collect();
        if clean.is_empty() {
            return Outcome::Quiet;
        }
        if self.put_text(&clean) {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    fn backspace(&mut self) -> Outcome {
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
    fn split(&mut self) -> bool {
        let Some(caret) = self.caret.clone() else {
            return false;
        };
        let position = self.position(&caret);
        let command = if position.inline.is_empty() {
            Edit::Split {
                at: caret.paragraph.clone(),
                offset: 0,
            }
        } else {
            Edit::SplitAt {
                at: caret.paragraph.clone(),
                point: nav::point_of(&position),
            }
        };
        if !self.transact(&[command]) {
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
    /// selection, or the word at the caret.
    fn format_ranges(
        &self,
    ) -> Vec<(
        Address,
        strict_ooxml::edit::InlinePoint,
        strict_ooxml::edit::InlinePoint,
    )> {
        let mut out = Vec::new();
        if let Some((start, end)) = self.selection() {
            let paragraphs = self.paragraphs(&start.paragraph.story);
            let from = paragraphs.iter().position(|a| *a == start.paragraph);
            let to = paragraphs.iter().position(|a| *a == end.paragraph);
            let (Some(from), Some(to)) = (from, to) else {
                return out;
            };
            for (index, address) in paragraphs.iter().enumerate().take(to + 1).skip(from) {
                let Some(paragraph) = self.paragraph(address) else {
                    continue;
                };
                let range_start = if index == from {
                    let position = self.position(&start);
                    (!position.inline.is_empty()).then(|| nav::point_of(&position))
                } else {
                    nav::start_point(paragraph)
                };
                let range_end = if index == to {
                    let position = self.position(&end);
                    (!position.inline.is_empty()).then(|| nav::point_of(&position))
                } else {
                    nav::end_point(paragraph)
                };
                if let (Some(a), Some(b)) = (range_start, range_end) {
                    out.push((address.clone(), a, b));
                }
            }
        } else if let Some(caret) = &self.caret {
            let text = self.text_of(&caret.paragraph);
            if let Some(range) = nav::word_at(&text, caret.offset) {
                let a = self.position(&Logical {
                    paragraph: caret.paragraph.clone(),
                    offset: range.start,
                });
                // The end of a word is the start of what follows it; take the
                // position inside the word's own node.
                let b = self.position(&Logical {
                    paragraph: caret.paragraph.clone(),
                    offset: range.end,
                });
                if !a.inline.is_empty() && !b.inline.is_empty() {
                    out.push((
                        caret.paragraph.clone(),
                        nav::point_of(&a),
                        nav::point_of(&b),
                    ));
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
                    let len = self.paragraph(&caret.paragraph)?;
                    let len: usize = nav::text_nodes(len).iter().map(TextNodeRef::len).sum();
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
        let Some(text) = self.selected_text() else {
            return Outcome::Quiet;
        };
        self.send(Reply::Clipboard(text));
        if cut {
            if self.read_only.is_some() {
                return Outcome::Quiet;
            }
            return self.delete_selection();
        }
        Outcome::Quiet
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
            write_atomic(&dir.join(name), page.svg.as_bytes())?;
        }
        Ok(())
    }
}

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
                nav::text_nodes(paragraph)
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
