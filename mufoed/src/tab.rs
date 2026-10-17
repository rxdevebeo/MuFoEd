//! A document tab: the worker's channels and what it last said.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use strict_ooxml::OpenControl;

use crate::formats::{Kind, Source};
use crate::raster::Bitmap;
use crate::worker::{
    self, Busy, Cursor, DocInfo, EditState, Note, OutlineEntry, PageData, Reply, Request,
    SaveFormat,
};

/// Where a tab is in its life.
pub(crate) enum Status {
    Loading,
    Ready,
    Failed(String),
}

/// What a tab has to tell the application after a poll.
pub(crate) enum TabEvent {
    Note(Note),
    Clipboard(String),
    ClipboardObjects(Box<crate::worker::ClipObjects>),
    Picture {
        id: u32,
        bytes: Vec<u8>,
        extension: &'static str,
        purpose: crate::worker::ExportPurpose,
    },
    Saved {
        path: PathBuf,
        format: SaveFormat,
    },
    Loaded,
    Failed(String),
    /// A comment was added: show the pane.
    CommentAdded,
    /// The document has words in scripts no dictionary checks.
    SpellingMissing(Vec<crate::spell::Script>),
    /// The document has text no font draws; these packs would.
    FontsMissing(Vec<&'static str>),
    /// The selected chart's data was asked for (none: a new chart).
    Chart(Option<Box<strict_ooxml::model::chart::ChartData>>),
    /// The formula at the caret was asked for, to edit (or none: a new one).
    Formula(
        Option<
            Box<(
                strict_ooxml::edit::Address,
                usize,
                strict_ooxml::model::math::MathExpression,
            )>,
        >,
    ),
    /// The text an AI job asked for.
    AiSource {
        job: u64,
        source: Box<crate::worker::AiSource>,
    },
}

/// What happens once a pending save finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterSave {
    Close,
}

/// A bitmap of a page at one scale.
pub(crate) struct PageTexture {
    /// The layout generation it shows.
    pub(crate) generation: u64,
    /// A hash of the SVG it was made from.
    pub(crate) svg: u64,
    pub(crate) scale: u32,
    pub(crate) texture: egui::TextureHandle,
}

/// One finished rasterization.
pub(crate) struct Rastered {
    pub(crate) index: usize,
    pub(crate) generation: u64,
    pub(crate) svg: u64,
    pub(crate) scale: u32,
    /// `None` when the page's SVG is the one the bitmap already shows.
    pub(crate) bitmap: Option<Result<Bitmap, String>>,
}

/// The canvas's own state.
pub(crate) struct ViewState {
    /// The context menu's misspelt word, when it was opened on one.
    pub(crate) spell_menu: Option<SpellMenu>,
    pub(crate) textures: HashMap<usize, PageTexture>,
    pub(crate) pending: HashSet<(usize, u64, u32)>,
    pub(crate) raster_tx: Sender<Rastered>,
    pub(crate) raster_rx: Receiver<Rastered>,
    /// When the zoom last changed; rasterizing waits until it settles.
    pub(crate) zoom_changed: f64,
    pub(crate) scroll_to_page: Option<usize>,
    pub(crate) scroll_to_caret: bool,
    pub(crate) last_drag: Option<egui::Pos2>,
    /// Visible size of the canvas, for fit-to-width and one-page zoom.
    pub(crate) viewport: egui::Vec2,
    pub(crate) current_page: usize,
    /// When the caret last moved, for the blink phase.
    pub(crate) caret_moved: f64,
    /// A drawing's frame being dragged by a handle.
    pub(crate) resize: Option<Resize>,
    /// The handles crop the selected picture instead of resizing it.
    pub(crate) crop: bool,
    /// The selected drawing being dragged to another place.
    pub(crate) moving: Option<Moving>,
    /// Where each page is on screen this frame (`None` out of view).
    pub(crate) page_screens: Vec<Option<egui::Rect>>,
    /// A ruler's margin or indent being dragged.
    pub(crate) ruler_drag: Option<crate::ruler::Dragging>,
}

/// Dragging a selected drawing by its body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Moving {
    /// The frame on screen when the drag began.
    pub(crate) frame: egui::Rect,
    pub(crate) origin: egui::Pos2,
    pub(crate) pointer: egui::Pos2,
    /// A floating drawing moves on the page; one in the text moves in it.
    pub(crate) floating: bool,
}

/// Resizing a selected drawing by one of its eight handles.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Resize {
    /// 0..8: corners nw, ne, se, sw, then edges n, e, s, w.
    pub(crate) handle: usize,
    /// The frame on screen when the drag began.
    pub(crate) frame: egui::Rect,
    /// Where the pointer is now.
    pub(crate) pointer: egui::Pos2,
    /// Where it began.
    pub(crate) origin: egui::Pos2,
    /// The drawing's size then, in EMU.
    pub(crate) size: (i64, i64),
    /// Shift held: the sides move freely; otherwise the proportions stay.
    pub(crate) free: bool,
    /// Cropping: the sides move freely, inside the picture.
    pub(crate) crop: bool,
}

impl ViewState {
    fn new() -> Self {
        let (raster_tx, raster_rx) = std::sync::mpsc::channel();
        Self {
            textures: HashMap::new(),
            pending: HashSet::new(),
            raster_tx,
            raster_rx,
            zoom_changed: -1.0,
            scroll_to_page: None,
            scroll_to_caret: false,
            last_drag: None,
            viewport: egui::vec2(800.0, 600.0),
            current_page: 0,
            caret_moved: 0.0,
            resize: None,
            crop: false,
            moving: None,
            page_screens: Vec::new(),
            ruler_drag: None,
            spell_menu: None,
        }
    }
}

/// The misspelt word a context menu is open on, and what could replace it.
#[derive(Clone, Debug)]
pub(crate) struct SpellMenu {
    pub(crate) error: crate::worker::SpellError,
    pub(crate) suggestions: Vec<String>,
}

pub(crate) struct DocTab {
    pub(crate) id: u64,
    pub(crate) title: String,
    /// Where Save writes, once there is a Strict DOCX to write to.
    pub(crate) path: Option<PathBuf>,
    /// The file the tab was opened from.
    pub(crate) source: Option<PathBuf>,
    pub(crate) kind: Option<Kind>,
    pub(crate) status: Status,
    pub(crate) control: OpenControl,
    pub(crate) started: Instant,
    pub(crate) info: Option<DocInfo>,
    pub(crate) pages: Vec<Arc<PageData>>,
    pub(crate) warnings: Vec<String>,
    pub(crate) cursor: Option<Box<Cursor>>,
    pub(crate) state: EditState,
    pub(crate) outline: Vec<OutlineEntry>,
    /// What the worker is busy with.
    pub(crate) busy: Option<Busy>,
    /// The document's comments, in the order of the text.
    pub(crate) comments: Vec<crate::worker::CommentView>,
    /// The comment whose text the pane edits, and the text so far.
    pub(crate) editing_comment: Option<(String, String)>,
    /// Comments were there once, so the pane was shown.
    comments_seen: bool,
    /// The misspelt words.
    pub(crate) spelling: Vec<crate::worker::SpellError>,
    /// The speller the worker marks with, for suggestions.
    pub(crate) speller: Option<std::sync::Arc<crate::spell::Speller>>,
    pub(crate) zoom: f32,
    pub(crate) view: ViewState,
    pub(crate) after_save: Option<AfterSave>,
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

impl DocTab {
    /// Hands the worker a speller (or none, to stop marking words).
    pub(crate) fn set_speller(&mut self, speller: Option<std::sync::Arc<crate::spell::Speller>>) {
        self.speller.clone_from(&speller);
        if speller.is_none() {
            self.spelling.clear();
        }
        self.send(Request::Speller(speller));
    }

    pub(crate) fn open(
        id: u64,
        source: Source,
        title: String,
        zoom: f32,
        system_fonts: bool,
        ctx: &egui::Context,
    ) -> Self {
        let control = OpenControl::new();
        let path = match &source {
            Source::File { path, .. } => Some(path.clone()),
            Source::New => None,
        };
        let (requests, replies) = worker::spawn(source, system_fonts, control.clone(), ctx.clone());
        Self {
            id,
            title,
            path: None,
            source: path,
            kind: None,
            status: Status::Loading,
            control,
            started: Instant::now(),
            info: None,
            pages: Vec::new(),
            warnings: Vec::new(),
            cursor: None,
            state: EditState::default(),
            outline: Vec::new(),
            busy: None,
            comments: Vec::new(),
            editing_comment: None,
            comments_seen: false,
            spelling: Vec::new(),
            speller: None,
            zoom,
            view: ViewState::new(),
            after_save: None,
            requests,
            replies,
        }
    }

    pub(crate) fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    pub(crate) fn ready(&self) -> bool {
        matches!(self.status, Status::Ready)
    }

    /// Takes everything the worker said since the last frame.
    pub(crate) fn poll(&mut self) -> Vec<TabEvent> {
        let mut events = Vec::new();
        loop {
            match self.replies.try_recv() {
                Ok(reply) => self.apply(reply, &mut events),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if matches!(self.status, Status::Loading) {
                        let message = "the document thread stopped".to_owned();
                        self.status = Status::Failed(message.clone());
                        events.push(TabEvent::Failed(message));
                    }
                    break;
                }
            }
        }
        events
    }

    fn apply(&mut self, reply: Reply, events: &mut Vec<TabEvent>) {
        match reply {
            Reply::Loaded(info) => {
                self.kind = Some(info.kind);
                if matches!(info.kind, Kind::Docx) {
                    self.path.clone_from(&self.source);
                }
                self.info = Some(*info);
                self.status = Status::Ready;
                events.push(TabEvent::Loaded);
            }
            Reply::Failed(error) => {
                self.status = Status::Failed(error.clone());
                events.push(TabEvent::Failed(error));
            }
            Reply::Pages { pages, warnings } => {
                // Bitmaps of re-laid pages stay until their new SVG is known
                // (and kept if it is the same); pages past the end go now.
                self.view.textures.retain(|index, _| *index < pages.len());
                self.pages = pages;
                self.warnings = warnings;
            }
            Reply::Cursor(cursor) => {
                if cursor.scroll {
                    self.view.scroll_to_caret = true;
                }
                self.cursor = Some(cursor);
            }
            Reply::State(state) => self.state = state,
            Reply::Outline(outline) => self.outline = outline,
            Reply::Comments(comments) => {
                // A document's comments show the pane the first time they
                // are there.
                if self.comments.is_empty() && !comments.is_empty() && !self.comments_seen {
                    self.comments_seen = true;
                    events.push(TabEvent::CommentAdded);
                }
                self.comments = comments;
            }
            Reply::FontsMissing(packs) => events.push(TabEvent::FontsMissing(packs)),
            Reply::AiSource { job, source } => events.push(TabEvent::AiSource { job, source }),
            Reply::Formula(found) => events.push(TabEvent::Formula(found)),
            Reply::Chart(data) => events.push(TabEvent::Chart(data)),
            Reply::Spelling { errors, missing } => {
                self.spelling = errors;
                if !missing.is_empty() {
                    events.push(TabEvent::SpellingMissing(missing));
                }
            }
            Reply::CommentAdded(id) => {
                self.editing_comment = Some((id, String::new()));
                events.push(TabEvent::CommentAdded);
            }
            Reply::Busy(busy) => self.busy = busy,
            Reply::Note(note) => events.push(TabEvent::Note(note)),
            Reply::Clipboard(text) => events.push(TabEvent::Clipboard(text)),
            Reply::ClipboardObjects(clip) => events.push(TabEvent::ClipboardObjects(clip)),
            Reply::Picture {
                id,
                bytes,
                extension,
                purpose,
            } => events.push(TabEvent::Picture {
                id,
                bytes,
                extension,
                purpose,
            }),
            Reply::Saved { path, format } => {
                if format == SaveFormat::Docx {
                    self.path = Some(path.clone());
                    self.kind = Some(Kind::Docx);
                    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                        name.clone_into(&mut self.title);
                    }
                }
                events.push(TabEvent::Saved { path, format });
            }
        }
    }

    /// The name a Save As dialog proposes.
    pub(crate) fn suggested_name(&self, extension: &str) -> String {
        let stem = self
            .source
            .as_ref()
            .or(self.path.as_ref())
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .map_or_else(|| self.title.clone(), str::to_owned);
        format!("{stem}.{extension}")
    }

    pub(crate) fn modified(&self) -> bool {
        self.state.modified || (self.ready() && self.path.is_none() && self.kind != Some(Kind::New))
    }
}

impl Drop for DocTab {
    fn drop(&mut self) {
        self.control.cancel();
        let _ = self.requests.send(Request::Shutdown);
    }
}
