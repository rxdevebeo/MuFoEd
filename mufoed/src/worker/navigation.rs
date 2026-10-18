//! Click targets belong to the editor, not to the document format. Attribute
//! their ink on a disposable colour-only copy using the public renderer.
use super::*;
use strict_ooxml::model::{Inline, Run, RunContent};
use strict_ooxml_core::part::PartId;
use strict_ooxml_render_svg::{Item, MediaSource, TextItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Bookmark(String),
    Paragraph(Address),
    Note(Story),
    Reference(Story),
    External(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Link {
    pub(crate) target: Target,
    pub(crate) rects: Vec<VisualRect>,
    source: Logical,
}

impl MediaSource for Session<'_> {
    fn read_media(&self, part: &PartId) -> strict_ooxml_core::error::Result<Vec<u8>> {
        match self {
            Self::Edit(editor) => editor.media_bytes(part),
            Self::View(document) => document.package().read_part(part),
        }
    }
}

#[derive(Default)]
struct FieldState {
    instruction: String,
    result: bool,
    target: Option<Target>,
}

struct Tags<'a> {
    source: &'a dyn MediaSource,
    links: Vec<Link>,
    colors: HashMap<String, usize>,
    used: std::collections::HashSet<String>,
    rels: HashMap<PartId, Vec<strict_ooxml_core::opc::rels::Relationship>>,
    fields: Vec<FieldState>,
    toc: Vec<(Address, Address)>,
}

impl Tags<'_> {
    fn color(&mut self, target: Target, source: &Logical) -> String {
        let mut value = self.colors.len() + 1;
        let color = loop {
            let color = format!("#{value:06x}");
            if self.used.insert(color.clone()) {
                break color;
            }
            value += 1;
        };
        self.colors.insert(color.clone(), self.links.len());
        self.links.push(Link {
            target,
            source: source.clone(),
            rects: vec![],
        });
        color
    }

    fn hyperlink(&mut self, link: &strict_ooxml::model::Hyperlink) -> Option<Target> {
        let target = link.target.as_deref().map(str::to_owned).or_else(|| {
            let id = link.rel_id.as_ref()?;
            let part = &link.location.part;
            if !self.rels.contains_key(part) {
                let (dir, name) = part.as_str().rsplit_once('/')?;
                let rel_part = PartId::new(format!("{dir}/_rels/{name}.rels"));
                let rels = self
                    .source
                    .read_media(&rel_part)
                    .ok()
                    .and_then(|bytes| {
                        strict_ooxml_core::opc::rels::parse_relationships(
                            bytes,
                            part,
                            &Default::default(),
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                self.rels.insert(part.clone(), rels);
            }
            self.rels
                .get(part)?
                .iter()
                .find(|r| r.id == id.as_str())
                .map(|r| r.target.clone())
        });
        match target.filter(|t| !t.is_empty()) {
            Some(mut url) => {
                if let Some(anchor) = &link.anchor {
                    if !url.contains('#') {
                        url.push('#');
                        url.push_str(anchor);
                    }
                }
                Some(url_target(&url))
            }
            None => link
                .anchor
                .as_deref()
                .map(|s| Target::Bookmark(s.to_owned())),
        }
    }

    fn blocks(&mut self, blocks: &mut [Block], story: &Story, parents: &[Container]) {
        for (block, item) in blocks.iter_mut().enumerate() {
            let mut path = parents.to_vec();
            match item {
                Block::Paragraph(p) => {
                    let at = Address {
                        story: story.clone(),
                        containers: path,
                        block,
                    };
                    let inherited = self
                        .toc
                        .iter()
                        .find(|(a, _)| *a == at)
                        .map(|(_, to)| Target::Paragraph(to.clone()));
                    let mut offset = 0;
                    self.inlines(&mut p.inlines, &at, &mut offset, inherited);
                }
                Block::Table(t) => {
                    for (row, r) in t.rows.iter_mut().enumerate() {
                        for (cell, c) in r.cells.iter_mut().enumerate() {
                            path.push(Container::Cell {
                                table: block,
                                row,
                                cell,
                            });
                            self.blocks(&mut c.blocks, story, &path);
                            path.pop();
                        }
                    }
                }
                Block::SdtBlock(s) => {
                    path.push(Container::Sdt(block));
                    self.blocks(&mut s.blocks, story, &path);
                }
                _ => {}
            }
        }
    }

    fn inlines(
        &mut self,
        inlines: &mut Vec<Inline>,
        at: &Address,
        offset: &mut usize,
        inherited: Option<Target>,
    ) {
        for inline in std::mem::take(inlines) {
            let mut inline = inline;
            match &mut inline {
                Inline::Run(run) => {
                    for content in std::mem::take(&mut run.content) {
                        let source = Logical {
                            paragraph: at.clone(),
                            offset: *offset,
                        };
                        match &content {
                            RunContent::FieldChar(c) => {
                                use strict_ooxml::model::values::FieldCharType;
                                match c.kind {
                                    FieldCharType::Begin => self.fields.push(FieldState::default()),
                                    FieldCharType::Separate => {
                                        if let Some(f) = self.fields.last_mut() {
                                            f.result = true;
                                            f.target = field_target(&f.instruction);
                                        }
                                    }
                                    FieldCharType::End => {
                                        self.fields.pop();
                                    }
                                }
                            }
                            RunContent::InstrText(s) => {
                                if let Some(f) = self.fields.last_mut().filter(|f| !f.result) {
                                    f.instruction.push_str(s);
                                }
                            }
                            _ => {}
                        }
                        let note = match &content {
                            RunContent::FootnoteRef(id) => i32::try_from(*id)
                                .ok()
                                .map(|id| Target::Note(Story::Footnote(id))),
                            RunContent::EndnoteRef(id) => i32::try_from(*id)
                                .ok()
                                .map(|id| Target::Note(Story::Endnote(id))),
                            RunContent::NoteRef => Some(Target::Reference(at.story.clone())),
                            _ => None,
                        };
                        let active = note
                            .or_else(|| self.fields.iter().rev().find_map(|f| f.target.clone()))
                            .or_else(|| inherited.clone());
                        let mut piece = run.clone();
                        piece.content = vec![content];
                        if let Some(target) = active {
                            let color = self.color(target, &source);
                            piece.props.color = Some(Color::new(color.trim_start_matches('#')));
                            piece.props.color_theme = None;
                        }
                        *offset += nav::pieces(&Paragraph {
                            inlines: vec![Inline::Run(piece.clone())],
                            ..empty_paragraph()
                        })
                        .iter()
                        .map(nav::Piece::len)
                        .sum::<usize>();
                        inlines.push(Inline::Run(piece));
                    }
                    continue;
                }
                Inline::Hyperlink(h) => {
                    let target = self.hyperlink(h).or_else(|| inherited.clone());
                    self.inlines(&mut h.inlines, at, offset, target);
                }
                Inline::Field(f) => {
                    let target = f
                        .instruction
                        .as_deref()
                        .and_then(field_target)
                        .or_else(|| inherited.clone());
                    // Cached simple-field text is protected by the editing
                    // map and consumes no editable scalar positions.
                    let mut local = *offset;
                    self.inlines(&mut f.inlines, at, &mut local, target);
                }
                Inline::SdtInline(s) => self.inlines(&mut s.inlines, at, offset, inherited.clone()),
                Inline::Directional(d) => {
                    self.inlines(&mut d.inlines, at, offset, inherited.clone())
                }
                Inline::FootnoteRef(_) | Inline::EndnoteRef(_) => {
                    // The two standard spellings render identically. A run
                    // supplies colour without changing the reference number.
                    let foot = matches!(inline, Inline::FootnoteRef(_));
                    let id = match &inline {
                        Inline::FootnoteRef(id) | Inline::EndnoteRef(id) => *id,
                        _ => unreachable!(),
                    };
                    let content = if foot {
                        RunContent::FootnoteRef(id)
                    } else {
                        RunContent::EndnoteRef(id)
                    };
                    let mut one = vec![Inline::Run(Run {
                        props: Default::default(),
                        content: vec![content],
                        revision: None,
                        location: SourceLocation::unknown(),
                    })];
                    self.inlines(&mut one, at, offset, inherited.clone());
                    inlines.extend(one);
                    continue;
                }
                _ => {
                    *offset += nav::pieces(&Paragraph {
                        inlines: vec![inline.clone()],
                        ..empty_paragraph()
                    })
                    .iter()
                    .map(nav::Piece::len)
                    .sum::<usize>();
                }
            }
            inlines.push(inline);
        }
    }
}

fn empty_paragraph() -> Paragraph {
    Paragraph {
        props: Default::default(),
        inlines: vec![],
        rsids: Default::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::unknown(),
    }
}

fn url_target(url: &str) -> Target {
    url.strip_prefix('#').map_or_else(
        || Target::External(url.to_owned()),
        |name| Target::Bookmark(name.to_owned()),
    )
}

/// Quoted field operands can contain spaces; a backslash starts a switch.
fn field_target(instruction: &str) -> Option<Target> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    for ch in instruction.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            c => token.push(c),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    let kind = tokens.first()?.to_ascii_uppercase();
    match kind.as_str() {
        "HYPERLINK" => {
            let anchor = tokens
                .iter()
                .position(|s| s.eq_ignore_ascii_case("\\l"))
                .and_then(|i| tokens.get(i + 1));
            let url = tokens.get(1).filter(|s| !s.starts_with('\\'));
            match (url, anchor) {
                (Some(url), anchor) => Some(url_target(&format!(
                    "{url}{}",
                    anchor.map_or(String::new(), |a| format!("#{a}"))
                ))),
                (_, Some(anchor)) => Some(Target::Bookmark(anchor.clone())),
                _ => None,
            }
        }
        "REF" | "PAGEREF" | "NOTEREF" => tokens
            .get(1)
            .filter(|s| !s.starts_with('\\'))
            .map(|s| Target::Bookmark(s.clone())),
        _ => None,
    }
}

fn texts(page: &PlacedPage) -> impl Iterator<Item = &TextItem> {
    page.items
        .iter()
        .flat_map(Item::leaves)
        .filter_map(|i| match i {
            Item::Text(t) => Some(t),
            _ => None,
        })
}

impl Worker<'_, '_> {
    pub(super) fn navigation_links(&self, options: &RenderOptions) -> Vec<Link> {
        let mut document = self.session.document().clone();
        let mut tags = Tags {
            source: &self.session,
            links: vec![],
            colors: HashMap::new(),
            rels: HashMap::new(),
            fields: vec![],
            used: self
                .pages
                .iter()
                .flat_map(|p| texts(&p.placed))
                .filter_map(|t| t.run.color.clone())
                .collect(),
            toc: if self.toc_block().is_some() {
                self.paragraphs(&Story::Body)
                    .into_iter()
                    .filter_map(|at| self.toc_target(&at).map(|to| (at, to)))
                    .collect()
            } else {
                vec![]
            },
        };
        tags.blocks(&mut document.body.blocks, &Story::Body, &[]);
        for hf in &mut document.headers_footers {
            tags.fields.clear();
            tags.blocks(&mut hf.blocks, &Story::HeaderFooter(hf.part.clone()), &[]);
        }
        for endnote in [false, true] {
            let table = if endnote {
                &mut document.endnotes
            } else {
                &mut document.footnotes
            };
            let notes: Vec<_> = table.iter().cloned().collect();
            for mut note in notes {
                tags.fields.clear();
                tags.blocks(
                    &mut note.blocks,
                    &if endnote {
                        Story::Endnote(note.id)
                    } else {
                        Story::Footnote(note.id)
                    },
                    &[],
                );
                table.insert(note);
            }
        }
        if tags.links.is_empty() {
            return vec![];
        }
        let Ok(pages) =
            strict_ooxml_render_svg::place_pages(&document, options, Some(&self.session))
        else {
            return vec![];
        };
        if pages.len() != self.pages.len() {
            return vec![];
        }
        // Never trust hit boxes when the attribution layout differs.
        for (page, original) in pages.iter().zip(&self.pages) {
            let a: Vec<_> = texts(page).filter(|t| !t.is_caret_anchor()).collect();
            let b: Vec<_> = texts(&original.placed)
                .filter(|t| !t.is_caret_anchor())
                .collect();
            if a.len() != b.len()
                || a.iter().zip(b).any(|(a, b)| {
                    a.text != b.text
                        || (a.x - b.x).abs() > 0.05
                        || (a.baseline - b.baseline).abs() > 0.05
                        || (a.width - b.width).abs() > 0.05
                })
            {
                return vec![];
            }
        }
        for (page, placed) in pages.iter().enumerate() {
            for text in texts(placed).filter(|t| !t.is_caret_anchor()) {
                if let Some(index) = text.run.color.as_ref().and_then(|c| tags.colors.get(c)) {
                    tags.links[*index].rects.push(VisualRect {
                        page,
                        x: text.x,
                        y: text.baseline - text.size_px,
                        width: text.width,
                        height: text.size_px * 1.2,
                    });
                }
            }
        }
        tags.links.retain(|l| !l.rects.is_empty());
        tags.links
    }

    pub(super) fn follow_link(&mut self, target: Target) -> Outcome {
        self.ensure_layout();
        let into_note = matches!(target, Target::Note(_));
        match target {
            Target::External(url) => {
                self.send(Reply::OpenLink(url));
                Outcome::Quiet
            }
            Target::Bookmark(name) => {
                if name.eq_ignore_ascii_case("_top") {
                    return self
                        .first_paragraph()
                        .map_or(Outcome::Quiet, |at| self.handle(Request::GoTo(at)));
                }
                if !self
                    .bookmark_marks()
                    .iter()
                    .any(|(n, ..)| n.eq_ignore_ascii_case(&name))
                {
                    self.note("note.link_missing", name, true);
                    return Outcome::Quiet;
                }
                self.go_to_bookmark(&name)
            }
            Target::Paragraph(at) => self.handle(Request::GoTo(at)),
            Target::Note(story) | Target::Reference(story) => {
                let opposite = if into_note {
                    Target::Reference(story.clone())
                } else {
                    Target::Note(story.clone())
                };
                let Some(link) = self.links.iter().find(|l| l.target == opposite).cloned() else {
                    if into_note {
                        if let Some(at) = self.paragraphs(&story).into_iter().next() {
                            return self.handle(Request::GoTo(at));
                        }
                    }
                    self.note("note.link_missing", format!("{story:?}"), true);
                    return Outcome::Quiet;
                };
                self.scroll_at_top = true;
                self.story = link.source.paragraph.story.clone();
                self.caret = Some(link.source);
                self.anchor = None;
                self.cells = None;
                self.object = None;
                self.formula = None;
                if let Some(rect) = link.rects.first() {
                    self.send(Reply::NavigateTo(*rect));
                }
                Outcome::Moved
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Inline {
        Inline::Run(Run {
            props: Default::default(),
            content: vec![RunContent::Text(strict_ooxml::model::TextNode {
                text: s.into(),
                space: strict_ooxml::model::values::Space::Preserve,
            })],
            revision: None,
            location: SourceLocation::unknown(),
        })
    }

    #[test]
    fn quoted_fields_and_internal_anchors() {
        assert_eq!(
            field_target(r#" HYPERLINK "https://example.com/a b" \l "section" "#),
            Some(Target::External("https://example.com/a b#section".into()))
        );
        assert_eq!(
            field_target(r#"HYPERLINK \l "Chapter""#),
            Some(Target::Bookmark("Chapter".into()))
        );
        assert_eq!(
            field_target(r#"PAGEREF Chapter \h"#),
            Some(Target::Bookmark("Chapter".into()))
        );
        assert_eq!(field_target("PAGE"), None);
    }

    #[test]
    fn links_wrap_and_survive_docx_roundtrip_without_modifying_content() {
        super::super::tests::with_worker_replies(|worker, rx| {
            let at = Logical {
                paragraph: Address::body(0),
                offset: 0,
            };
            let content = Inline::Hyperlink(strict_ooxml::model::Hyperlink {
                target: Some(Arc::from("https://example.com/test")),
                rel_id: None,
                anchor: None,
                tooltip: None,
                history: false,
                tgt_frame: None,
                inlines: vec![text(&"wrapped link ".repeat(30))],
                location: SourceLocation::unknown(),
            });
            let commands =
                worker.inlines_at_caret(&at, vec![text("prefix "), content, text(" suffix")]);
            assert!(worker.transact(&commands));
            let before = worker.session.package_bytes().unwrap();
            worker.relayout();
            assert!(!worker.links.is_empty());
            let rects: Vec<_> = worker.links.iter().flat_map(|l| &l.rects).collect();
            assert!(rects.iter().any(|r| (r.y - rects[0].y).abs() > 1.0));
            assert_eq!(worker.session.package_bytes().unwrap(), before);
            worker.follow_link(Target::External("https://example.com/test".into()));
            assert!(rx
                .try_iter()
                .any(|r| matches!(r, Reply::OpenLink(url) if url == "https://example.com/test")));
            let reopened =
                StrictDocument::open_reader(&before[..], &OpenOptions::default()).unwrap();
            let session = Session::View(&reopened);
            let ctx = eframe::egui::Context::default();
            let (tx, _) = std::sync::mpsc::channel();
            let mut view = Worker::new(session, RenderOptions::default(), &tx, &ctx, None);
            view.relayout();
            assert!(
                !view.links.is_empty(),
                "read-only DOCX relationships resolve"
            );
            assert!(view
                .links
                .iter()
                .all(|l| l.target == Target::External("https://example.com/test".into())));
        });
    }

    #[test]
    fn note_numbers_navigate_in_both_directions_and_keep_undo_history() {
        for endnote in [false, true] {
            super::super::tests::with_worker_replies(|worker, rx| {
                worker.handle(Request::Type("Body before reference".into()));
                worker.insert_note(endnote);
                let story = worker.story.clone();
                worker.handle(Request::Type("Note text".into()));
                worker.relayout();
                let depth = worker.undo_depth;
                assert!(worker
                    .links
                    .iter()
                    .any(|l| l.target == Target::Note(story.clone())));
                assert!(worker
                    .links
                    .iter()
                    .any(|l| l.target == Target::Reference(story.clone())));
                assert!(matches!(
                    worker.follow_link(Target::Reference(story.clone())),
                    Outcome::Moved
                ));
                assert_eq!(worker.story, Story::Body);
                assert_eq!(
                    worker.caret.as_ref().unwrap().offset,
                    "Body before reference".len()
                );
                assert!(matches!(
                    worker.follow_link(Target::Note(story.clone())),
                    Outcome::Moved
                ));
                assert_eq!(worker.story, story);
                assert_eq!(worker.undo_depth, depth);
                assert!(rx.try_iter().any(|r| matches!(r, Reply::NavigateTo(_))));
            });
        }
    }

    #[test]
    fn contents_entries_and_bookmark_links_go_to_the_start() {
        super::super::tests::with_worker_replies(|worker, _| {
            worker.handle(Request::Type("Chapter".into()));
            let style = styles_of(worker.session.document())
                .into_iter()
                .find(|s| s.name.eq_ignore_ascii_case("heading 1"))
                .unwrap()
                .id;
            worker.handle(Request::Style(style));
            worker.anchor = Some(Logical {
                paragraph: Address::body(0),
                offset: 0,
            });
            worker.add_bookmark("Chapter");
            worker.relayout();
            worker.follow_link(Target::Bookmark("Chapter".into()));
            assert_eq!(worker.caret.as_ref().unwrap().offset, 0);
            assert!(worker.scroll_at_top);
            worker.references(ReferenceCommand::TableOfContents {
                title: "Contents".into(),
            });
            worker.relayout();
            let target = worker
                .links
                .iter()
                .find_map(|l| match &l.target {
                    Target::Paragraph(at) => Some(at.clone()),
                    _ => None,
                })
                .expect("TOC has a clickable entry");
            worker.follow_link(Target::Paragraph(target.clone()));
            assert_eq!(worker.caret.as_ref().unwrap().paragraph, target);
            assert!(worker.scroll_at_top);
        });
    }

    #[test]
    fn simple_and_complex_link_fields_have_only_their_result_clickable() {
        use strict_ooxml::model::values::FieldCharType;
        use strict_ooxml::model::{Field, FieldChar};
        super::super::tests::with_worker_replies(|worker, _| {
            let field_char = |kind| {
                Inline::Run(Run {
                    content: vec![RunContent::FieldChar(FieldChar {
                        kind,
                        dirty: false,
                        form_data: None,
                    })],
                    props: Default::default(),
                    revision: None,
                    location: SourceLocation::unknown(),
                })
            };
            let mut instr = text("unused");
            if let Inline::Run(run) = &mut instr {
                run.content = vec![RunContent::InstrText(
                    "HYPERLINK \"https://example.com/complex\"".into(),
                )];
            }
            let commands = worker.inlines_at_caret(
                &Logical {
                    paragraph: Address::body(0),
                    offset: 0,
                },
                vec![
                    Inline::Field(Field {
                        instruction: Some(Arc::from("HYPERLINK \"https://example.com/simple\"")),
                        inlines: vec![text("simple")],
                        location: SourceLocation::unknown(),
                    }),
                    text(" gap "),
                    field_char(FieldCharType::Begin),
                    instr,
                    field_char(FieldCharType::Separate),
                    text("complex"),
                    field_char(FieldCharType::End),
                    text(" plain"),
                ],
            );
            assert!(worker.transact(&commands));
            worker.relayout();
            assert!(worker
                .links
                .iter()
                .any(|l| l.target == Target::External("https://example.com/simple".into())));
            assert!(worker
                .links
                .iter()
                .any(|l| l.target == Target::External("https://example.com/complex".into())));
            let total: f64 = worker
                .links
                .iter()
                .flat_map(|l| &l.rects)
                .map(|r| r.width)
                .sum();
            let width: f64 = worker
                .pages
                .iter()
                .flat_map(|p| texts(&p.placed))
                .map(|t| t.width)
                .sum();
            assert!(total < width, "ordinary text stays outside link regions");
        });
    }

    #[test]
    fn canvas_shows_a_hand_and_dispatches_a_link_at_each_zoom() {
        use eframe::egui;
        for zoom in [0.75, 1.0, 1.5] {
            let ctx = egui::Context::default();
            let mut tab =
                crate::tab::DocTab::open(811, Source::New, "links".into(), zoom, false, &ctx);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while tab.cursor.is_none() {
                tab.poll();
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            tab.links = vec![Link {
                target: Target::External("https://example.com/click".into()),
                source: Logical {
                    paragraph: Address::body(0),
                    offset: 0,
                },
                rects: vec![VisualRect {
                    page: 0,
                    x: 100.0,
                    y: 100.0,
                    width: 80.0,
                    height: 20.0,
                }],
            }];
            let i18n = crate::i18n::I18n::load(&[]);
            let mut pointer = egui::Pos2::ZERO;
            for frame in 0..6 {
                let mut events = vec![];
                if frame >= 2 {
                    events.push(egui::Event::PointerMoved(pointer));
                }
                if frame == 3 || frame == 4 {
                    events.push(egui::Event::PointerButton {
                        pos: pointer,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 3,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 700.0),
                        )),
                        time: Some(f64::from(frame) * 0.1),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        crate::canvas::show(ui, &mut tab, crate::theme::palette(false), true, &i18n)
                    },
                );
                pointer = tab.view.page_screens[0].unwrap().min + egui::vec2(130.0, 110.0) * zoom;
                if frame == 2 {
                    assert_eq!(
                        output.platform_output.cursor_icon,
                        egui::CursorIcon::PointingHand
                    );
                }
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if tab.poll().iter().any(|e| matches!(e, crate::tab::TabEvent::OpenLink(url) if url == "https://example.com/click")) { break; }
                assert!(std::time::Instant::now() < deadline, "click must open link");
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }
}
