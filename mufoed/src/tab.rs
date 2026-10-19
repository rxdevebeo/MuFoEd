//! A document tab: the worker's channels and what it last said.
#[cfg(test)]
mod scheme_tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use strict_ooxml::OpenControl;

use crate::formats::{Kind, OriginalSource, Source};
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
    OpenLink(String),
    Scheme(Box<crate::worker::scheme::Plan>),
    Chemistry(Box<crate::worker::chem::Plan>),
    FormulaRecognition(Box<crate::worker::FormulaRecognition>),
    CleanupResult(bool),
    CommentSelected,
    ParagraphFormat(Box<crate::paragraph::Format>),
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
    /// Dictionary languages the document asks to fetch.
    SpellingMissing(Vec<&'static str>),
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
    /// The document as the chat's tools read it.
    ChatSnapshot {
        /// The id the request carried.
        job: u64,
        /// Paragraphs, formulas, tables and the caret.
        view: Box<crate::chat::View>,
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
    /// Source-page pictures for a recognition, beside the recognized pages.
    pub(crate) original_tx: Sender<(usize, Result<Bitmap, String>)>,
    pub(crate) original_rx: Receiver<(usize, Result<Bitmap, String>)>,
    pub(crate) original_textures: HashMap<usize, egui::TextureHandle>,
    pub(crate) original_pending: HashSet<usize>,
    pub(crate) original_failed: HashSet<usize>,
    /// When the zoom last changed; rasterizing waits until it settles.
    pub(crate) zoom_changed: f64,
    pub(crate) scroll_to_page: Option<usize>,
    pub(crate) scroll_to_caret: bool,
    pub(crate) caret_at_top: bool,
    pub(crate) last_drag: Option<egui::Pos2>,
    /// Visible size of the canvas, for fit-to-width and one-page zoom.
    pub(crate) viewport: egui::Vec2,
    /// Several pages in a row, when the width and zoom leave room.
    pub(crate) pages_across: bool,
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
        let (original_tx, original_rx) = std::sync::mpsc::channel();
        Self {
            textures: HashMap::new(),
            pending: HashSet::new(),
            raster_tx,
            raster_rx,
            original_tx,
            original_rx,
            original_textures: HashMap::new(),
            original_pending: HashSet::new(),
            original_failed: HashSet::new(),
            zoom_changed: -1.0,
            scroll_to_page: None,
            scroll_to_caret: false,
            caret_at_top: false,
            last_drag: None,
            viewport: egui::vec2(800.0, 600.0),
            pages_across: false,
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
    /// The scan a recognition was started from, shown beside its text.
    pub(crate) original: Option<OriginalSource>,
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
    pub(crate) links: Vec<crate::worker::navigation::Link>,
    pub(crate) navigation_target: Option<strict_ooxml::edit::VisualRect>,
    /// The comment whose text the pane edits, and the text so far.
    pub(crate) editing_comment: Option<(String, String)>,
    pub(crate) focus_comment: bool,
    pub(crate) selected_comment: Option<String>,
    pub(crate) scroll_comment: Option<String>,
    pub(crate) context: Option<crate::worker::ContextState>,
    pub(crate) ui_actions: Vec<crate::app::Action>,
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
        let (path, original) = match &source {
            Source::File { path, import } if import.ocr.is_some() => (
                Some(path.clone()),
                crate::formats::detect(path)
                    .filter(|kind| kind.converted())
                    .map(|kind| OriginalSource {
                        path: path.clone(),
                        kind,
                        first: import.pages.map(|(start, _)| start).unwrap_or(1),
                    }),
            ),
            Source::File { path, .. } => (Some(path.clone()), None),
            Source::New => (None, None),
        };
        let (requests, replies) = worker::spawn(source, system_fonts, control.clone(), ctx.clone());
        Self {
            id,
            title,
            path: None,
            source: path,
            original,
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
            links: Vec::new(),
            navigation_target: None,
            editing_comment: None,
            focus_comment: false,
            selected_comment: None,
            scroll_comment: None,
            context: None,
            ui_actions: Vec::new(),
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

    pub(crate) fn finish_comment(&mut self) {
        if let Some((id, text)) = self.editing_comment.take() {
            self.send(Request::Comments(crate::worker::CommentCommand::Edit {
                id,
                text,
            }));
        }
        self.focus_comment = false;
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
            Reply::Links(links) => self.links = links,
            Reply::NavigateTo(rect) => self.navigation_target = Some(rect),
            Reply::OpenLink(url) => events.push(TabEvent::OpenLink(url)),
            Reply::FormulaRecognition(plan) => events.push(TabEvent::FormulaRecognition(plan)),
            Reply::CleanupResult(applied) => events.push(TabEvent::CleanupResult(applied)),
            Reply::Context(context) => self.context = Some(context),
            Reply::CommentFocused(id) => {
                self.selected_comment = Some(id.clone());
                self.scroll_comment = Some(id);
                events.push(TabEvent::CommentSelected);
            }
            Reply::ParagraphFormat(format) => events.push(TabEvent::ParagraphFormat(format)),
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
                    self.view.caret_at_top |= cursor.at_top;
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
            Reply::ChatSnapshot { job, view } => {
                events.push(TabEvent::ChatSnapshot { job, view });
            }
            Reply::ChatFind { .. } => {}
            Reply::Formula(found) => events.push(TabEvent::Formula(found)),
            Reply::Chart(data) => events.push(TabEvent::Chart(data)),
            Reply::Scheme(plan) => events.push(TabEvent::Scheme(plan)),
            Reply::Chemistry(plan) => events.push(TabEvent::Chemistry(plan)),
            Reply::Spelling { errors, missing } => {
                self.spelling = errors;
                if !missing.is_empty() {
                    events.push(TabEvent::SpellingMissing(missing));
                }
            }
            Reply::CommentAdded(id) => {
                self.selected_comment = Some(id.clone());
                self.scroll_comment = Some(id.clone());
                self.editing_comment = Some((id, String::new()));
                self.focus_comment = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn until(tab: &mut DocTab, ready: impl Fn(&DocTab) -> bool) {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let events = tab.poll();
            for event in events {
                if let TabEvent::Note(note) = event {
                    assert!(!note.error, "{}: {}", note.key, note.detail);
                }
            }
            if ready(tab) {
                break;
            }
            assert!(Instant::now() < deadline, "worker did not respond");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn chart_mouse_selection_resize_and_caret_work_with_rulers() {
        use crate::worker::{Motion, ObjectView};
        use strict_ooxml::model::chart::*;
        let file =
            std::env::temp_dir().join(format!("mufoed-chart-mouse-{}.rtf", std::process::id()));
        std::fs::write(&file, br"{\rtf1\ansi\pard\par}").unwrap();
        for source in [
            Source::New,
            Source::File {
                path: file.clone(),
                import: crate::formats::ImportOptions::default(),
            },
        ] {
            let ctx = egui::Context::default();
            let i18n = crate::i18n::I18n::load(&[]);
            let mut tab = DocTab::open(4, source, "test".into(), 1.0, false, &ctx);
            until(&mut tab, |t| t.cursor.is_some());
            tab.send(Request::PutChart(Box::new(ChartData {
                kind: ChartKind::Bar {
                    horizontal: false,
                    grouping: BarGrouping::Clustered,
                },
                title: Some("Chart".into()),
                categories: vec!["A".into(), "B".into()],
                series: vec![ChartSeries {
                    name: Some("Series".into()),
                    values: vec![Some(1.0), Some(2.0)],
                    ..Default::default()
                }],
                legend: true,
                legend_position: LegendPosition::Bottom,
                unsupported: vec![],
            })));
            until(&mut tab, |t| t.state.can_undo);
            let view = tab
                .cursor
                .as_ref()
                .unwrap()
                .object
                .clone()
                .expect("chart frame");
            assert!(view.chart);
            tab.send(Request::Move {
                motion: Motion::DocEnd,
                extend: false,
            });
            until(&mut tab, |t| t.cursor.as_ref().unwrap().object.is_none());
            let mut frame = 0_u32;
            let mut paint = |tab: &mut DocTab, events| {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1000.0, 850.0),
                        )),
                        time: Some(f64::from(frame) * 0.1),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        crate::ruler::desk(
                            ui,
                            tab,
                            crate::theme::palette(false),
                            true,
                            true,
                            &i18n,
                        );
                    },
                );
                frame += 1;
            };
            for _ in 0..3 {
                paint(&mut tab, vec![]);
            }
            let rect_on_screen = |tab: &DocTab, view: &ObjectView| {
                let r = view.rects[0];
                let page = tab.view.page_screens[r.page].unwrap();
                #[allow(clippy::cast_possible_truncation)]
                egui::Rect::from_min_size(
                    page.min + egui::vec2(r.x as f32, r.y as f32),
                    egui::vec2(r.width as f32, r.height as f32),
                )
            };
            let pointer_event = |pos, pressed| {
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            // A blank corner of the chart must select the complete drawing.
            let corner = rect_on_screen(&tab, &view).min + egui::vec2(3.0, 3.0);
            paint(&mut tab, pointer_event(corner, true));
            paint(&mut tab, pointer_event(corner, false));
            until(&mut tab, |t| {
                t.cursor
                    .as_ref()
                    .unwrap()
                    .object
                    .as_ref()
                    .is_some_and(|o| o.chart)
            });
            paint(&mut tab, vec![]);
            let handle = rect_on_screen(&tab, &view).right_bottom();
            let destination = handle - egui::vec2(60.0, 36.0);
            paint(&mut tab, pointer_event(handle, true));
            paint(&mut tab, vec![egui::Event::PointerMoved(destination)]);
            paint(&mut tab, pointer_event(destination, false));
            until(&mut tab, |t| {
                t.cursor
                    .as_ref()
                    .unwrap()
                    .object
                    .as_ref()
                    .is_some_and(|o| o.width < view.width)
            });
            paint(&mut tab, vec![]);
            let resized = tab.cursor.as_ref().unwrap().object.as_ref().unwrap();
            let point = rect_on_screen(&tab, resized).right_bottom() + egui::vec2(15.0, 15.0);
            paint(&mut tab, pointer_event(point, true));
            paint(&mut tab, pointer_event(point, false));
            until(&mut tab, |t| {
                let c = t.cursor.as_ref().unwrap();
                c.object.is_none() && c.carets.iter().any(|r| r.page == view.rects[0].page)
            });
            tab.send(Request::Type("Typed beside chart".into()));
            until(&mut tab, |t| t.state.words == 3);
        }
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn navigation_aligns_a_caret_near_the_last_page_bottom_to_the_view_top() {
        let ctx = egui::Context::default();
        let mut tab = DocTab::open(1, Source::New, "test".into(), 1.0, false, &ctx);
        tab.pages = vec![Arc::new(PageData {
            width: 800.0,
            height: 1100.0,
            generation: 1,
            placed: strict_ooxml::PlacedPage {
                width_px: 800.0,
                height_px: 1100.0,
                items: vec![],
                section_index: 0,
                off_page: Vec::new(),
            },
        })];
        tab.cursor = Some(Box::new(Cursor {
            object: None,
            ruler: None,
            header_footer: None,
            carets: vec![strict_ooxml::edit::VisualRect {
                page: 0,
                x: 100.0,
                y: 1000.0,
                width: 1.0,
                height: 16.0,
            }],
            selection: vec![],
            scroll: true,
            at_top: true,
            format: crate::worker::CaretFormat::default(),
        }));
        tab.view.scroll_to_caret = true;
        tab.view.caret_at_top = true;
        let i18n = crate::i18n::I18n::load(&[]);
        let mut top = 0.0;
        for frame in 0..20 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 400.0),
                    )),
                    time: Some(f64::from(frame) / 20.0),
                    ..Default::default()
                },
                |ui| {
                    top = ui.max_rect().top();
                    crate::canvas::show(ui, &mut tab, crate::theme::palette(false), true, &i18n);
                },
            );
        }
        let page = tab.view.page_screens[0].unwrap();
        assert!(
            (page.top() + 1000.0 - top).abs() <= 4.0,
            "caret at {}, viewport top {top}",
            page.top() + 1000.0
        );
    }

    #[test]
    fn clicking_comment_text_requests_the_card_and_focus_reply_selects_it() {
        let ctx = egui::Context::default();
        let mut tab = DocTab::open(3, Source::New, "test".into(), 1.0, false, &ctx);
        tab.pages = vec![Arc::new(PageData {
            width: 800.0,
            height: 1100.0,
            generation: 1,
            placed: strict_ooxml::PlacedPage {
                width_px: 800.0,
                height_px: 1100.0,
                items: vec![],
                section_index: 0,
                off_page: Vec::new(),
            },
        })];
        tab.comments = vec![crate::worker::CommentView {
            id: "7".into(),
            author: "Ann".into(),
            date: String::new(),
            text: "Comment".into(),
            page: 0,
            y: 100.0,
            rects: vec![strict_ooxml::edit::VisualRect {
                page: 0,
                x: 100.0,
                y: 100.0,
                width: 60.0,
                height: 20.0,
            }],
        }];
        let i18n = crate::i18n::I18n::load(&[]);
        let mut pointer = egui::Pos2::ZERO;
        for frame in 0..5 {
            let mut events = Vec::new();
            if frame == 2 || frame == 3 {
                events.push(egui::Event::PointerMoved(pointer));
                events.push(egui::Event::PointerButton {
                    pos: pointer,
                    button: egui::PointerButton::Primary,
                    pressed: frame == 2,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 400.0),
                    )),
                    time: Some(f64::from(frame) * 0.1),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::canvas::show(ui, &mut tab, crate::theme::palette(false), true, &i18n);
                },
            );
            pointer = tab.view.page_screens[0].unwrap().min + egui::vec2(125.0, 110.0);
        }
        assert!(tab
            .ui_actions
            .iter()
            .any(|action| matches!(action, crate::app::Action::ShowComment(id) if id == "7")));
        let mut events = Vec::new();
        tab.apply(Reply::CommentFocused("7".into()), &mut events);
        assert_eq!(tab.selected_comment.as_deref(), Some("7"));
        assert_eq!(tab.scroll_comment.as_deref(), Some("7"));
        assert!(events
            .iter()
            .any(|e| matches!(e, TabEvent::CommentSelected)));
    }

    #[test]
    fn leaving_a_comment_commits_the_draft_and_releases_editing() {
        let ctx = egui::Context::default();
        let mut tab = DocTab::open(2, Source::New, "test".into(), 1.0, false, &ctx);
        until(&mut tab, |t| t.cursor.is_some());
        tab.send(Request::Comments(crate::worker::CommentCommand::Add {
            author: "Ann".into(),
            initials: "A".into(),
        }));
        until(&mut tab, |t| t.editing_comment.is_some());
        tab.editing_comment.as_mut().unwrap().1 = "Saved draft".into();
        tab.finish_comment();
        assert!(tab.editing_comment.is_none());
        assert!(!tab.focus_comment);
        until(&mut tab, |t| {
            t.comments.first().is_some_and(|c| c.text == "Saved draft")
        });
        tab.send(Request::Type("Back in document".into()));
        until(&mut tab, |t| t.state.words == 3);
    }
}
