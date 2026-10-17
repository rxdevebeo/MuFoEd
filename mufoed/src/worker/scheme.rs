//! Document integration for the independent diagram widget. Only public StrictLib APIs.
use super::{
    DrawingPosition, Edit, EditLimits, Logical, OpenOptions, Outcome, StrictDocument, VisualRect,
    Worker, WriteOptions,
};
use diagram_editor::Diagram;
use strict_ooxml::model::{Block, Drawing, DrawingKind, Graphic, Inline};

#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub(crate) diagram: Diagram,
    revision: u64,
    replace: Option<(DrawingPosition, Drawing)>,
    caret: Logical,
    selection: Option<(Logical, Logical)>,
    pub(crate) issues: Vec<String>,
}
#[cfg(test)]
pub(crate) fn preview_plan() -> Plan {
    Plan {
        diagram: diagram_editor::DiagramEditor::sample().diagram().clone(),
        revision: 0,
        replace: None,
        caret: Logical {
            paragraph: super::Address::body(0),
            offset: 0,
        },
        selection: None,
        issues: vec![],
    }
}
pub(crate) fn graphic(drawing: &Drawing) -> Option<&Graphic> {
    match &drawing.kind {
        DrawingKind::Inline(d) => Some(d.graphic.as_ref()),
        DrawingKind::Anchor(d) => Some(d.graphic.as_ref()),
        DrawingKind::Opaque(_) => None,
    }
}
pub(crate) fn is_scheme(drawing: &Drawing) -> bool {
    matches!(graphic(drawing), Some(Graphic::Group(_)))
}
fn from_drawing(drawing: &Drawing) -> Result<diagram_editor::DocxImport, String> {
    let seed =
        StrictDocument::import_rtf(br"{\rtf1\ansi\par}", &strict_ooxml::RtfOptions::default())
            .map_err(|e| e.to_string())?;
    let mut document = seed.document.document().clone();
    let mut paragraph = document.body.blocks[0]
        .as_paragraph()
        .ok_or("No seed paragraph")?
        .clone();
    let mut inline = drawing.clone();
    // The source's placement in the host document does not change its internal scene.
    if let DrawingKind::Anchor(d) = &inline.kind {
        inline.kind = DrawingKind::Inline(strict_ooxml::model::InlineDrawing {
            extent: d.extent,
            effect_extent: None,
            doc_pr: d.doc_pr.clone(),
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            graphic_uri: d.graphic_uri.clone(),
            graphic: d.graphic.clone(),
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        });
    }
    // A drawing resized on the page retains its original scene coordinates.
    if let DrawingKind::Inline(d) = &mut inline.kind {
        if let Graphic::Group(g) = d.graphic.as_mut() {
            if let Some(x) = g.xfrm {
                if let Some(extent) = x.extent {
                    d.extent = Some(extent);
                }
            }
        }
    }
    paragraph.inlines = vec![Inline::Drawing(inline)];
    document.body.blocks = vec![Block::Paragraph(paragraph)];
    let written = strict_ooxml::write_package(
        &document,
        Some(seed.document.package()),
        &WriteOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    written.report.verify_no_silent_loss()?;
    if strict_ooxml::dropped_count(&written.report) > 0 {
        return Err(written.report.to_string());
    }
    Diagram::from_docx(&written.bytes)
}
fn to_drawing(diagram: &Diagram) -> Result<(Drawing, Vec<String>), String> {
    let output = diagram.to_docx()?;
    let document = StrictDocument::open_reader(&output.bytes[..], &OpenOptions::default())
        .map_err(|e| e.to_string())?;
    let drawing = document
        .document()
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .flat_map(|p| &p.inlines)
        .find_map(|i| {
            if let Inline::Drawing(d) = i {
                Some(d.clone())
            } else {
                None
            }
        })
        .ok_or("No diagram drawing was generated")?;
    Ok((drawing, output.issues))
}
impl Worker<'_, '_> {
    pub(super) fn scheme_plan(&self) -> Result<Plan, String> {
        if self.read_only.is_some() {
            return Err("Document is read-only".into());
        }
        let caret = self
            .caret
            .clone()
            .ok_or("Place the caret in the document")?;
        let selected = self
            .object
            .as_ref()
            .and_then(|p| self.drawing_at(p).filter(is_scheme).map(|d| (p.clone(), d)));
        let (diagram, issues) = if let Some((_, drawing)) = &selected {
            if let Some((_, source)) = self
                .scheme_sources
                .iter()
                .rev()
                .find(|(g, _)| Some(g) == graphic(drawing))
            {
                (source.clone(), vec![])
            } else {
                let imported = from_drawing(drawing)?;
                (imported.diagram, imported.issues)
            }
        } else {
            (
                diagram_editor::DiagramEditor::sample().diagram().clone(),
                vec![],
            )
        };
        Ok(Plan {
            diagram,
            revision: self.session.revision(),
            replace: selected,
            caret,
            selection: self.selection(),
            issues,
        })
    }
    pub(super) fn put_scheme(&mut self, plan: Plan) -> Outcome {
        if self.session.revision() != plan.revision
            || plan
                .replace
                .as_ref()
                .is_some_and(|(p, d)| self.drawing_at(p).as_ref() != Some(d))
        {
            self.note("scheme.stale", "", true);
            return Outcome::Quiet;
        }
        if plan.diagram.nodes.is_empty() {
            self.note("scheme.failed", "Empty diagram", true);
            return Outcome::Quiet;
        }
        let result = (|| -> Result<(Vec<Edit>, Drawing, Vec<String>), String> {
            let (mut drawing, issues) = to_drawing(&plan.diagram)?;
            if let Some((position, original)) = &plan.replace {
                let payload = graphic(&drawing)
                    .cloned()
                    .ok_or("Missing diagram graphic")?;
                drawing = original.clone();
                match &mut drawing.kind {
                    DrawingKind::Inline(d) => *d.graphic = payload,
                    DrawingKind::Anchor(d) => *d.graphic = payload,
                    DrawingKind::Opaque(_) => return Err("Unsupported drawing".into()),
                }
                return Ok((
                    vec![Edit::Drawing {
                        at: position.paragraph.clone(),
                        inline: position.inline.clone(),
                        content: position.content,
                        drawing: Box::new(drawing.clone()),
                    }],
                    drawing,
                    issues,
                ));
            }
            super::set_drawing_id(
                &mut drawing,
                strict_ooxml::edit::unused_drawing_id(self.session.document()),
            );
            let max_width = i64::from(self.text_width().max(1440)) * 635;
            if let DrawingKind::Inline(d) = &mut drawing.kind {
                if let Some(e) = &mut d.extent {
                    if e.cx.0 > max_width {
                        e.cy.0 = (e.cy.0 as f64 * max_width as f64 / e.cx.0 as f64).round() as i64;
                        e.cx.0 = max_width;
                    }
                }
            }
            let mut commands;
            if let Some((start, end)) = &plan.selection {
                commands = self
                    .replace_commands(start, end, "")
                    .map_err(str::to_owned)?;
                let mut model = self.session.document().clone();
                let mut stage = strict_ooxml::edit::Editor::new(
                    &mut model,
                    EditLimits {
                        history_transactions: 0,
                        ..EditLimits::default()
                    },
                )
                .map_err(|e| e.to_string())?;
                stage
                    .transact(stage.revision(), &commands)
                    .map_err(|e| e.to_string())?;
                let p = stage
                    .paragraph(&start.paragraph)
                    .map_err(|e| e.to_string())?;
                commands.extend(Self::inlines_in_paragraph(
                    start,
                    p,
                    vec![Inline::Drawing(drawing.clone())],
                ));
            } else {
                commands = self.inline_at_caret(&plan.caret, Inline::Drawing(drawing.clone()));
            }
            Ok((commands, drawing, issues))
        })();
        let (commands, drawing, issues) = match result {
            Ok(v) => v,
            Err(e) => {
                self.note("scheme.failed", e, true);
                return Outcome::Quiet;
            }
        };
        if !self.transact(&commands) {
            return Outcome::Quiet;
        }
        if let Some(g) = graphic(&drawing) {
            self.scheme_sources.push((g.clone(), plan.diagram));
            if self.scheme_sources.len() > 16 {
                self.scheme_sources.remove(0);
            }
        }
        self.object = if let Some((at, _)) = plan.replace {
            Some(at)
        } else {
            let caret = plan.selection.map_or(plan.caret, |(start, _)| start);
            self.caret = Some(caret.clone());
            self.objects_of(&caret.paragraph)
                .into_iter()
                .find(|(offset, _)| *offset == caret.offset)
                .map(|(_, p)| p)
        };
        self.anchor = None;
        self.formula = None;
        self.cells = None;
        if !issues.is_empty() {
            self.note("scheme.applied", issues.join("; "), false);
        }
        Outcome::Edited
    }
    pub(super) fn scheme_hit(&self, page: usize, x: f64, y: f64) -> Option<DrawingPosition> {
        let map = self.map.as_ref()?;
        for paragraph in self.paragraphs(&self.story) {
            for (_, at) in self.objects_of(&paragraph) {
                let Some(drawing) = self.drawing_at(&at) else {
                    continue;
                };
                if !is_scheme(&drawing) {
                    continue;
                }
                let Ok(rects) = map.drawing_rects(self.session.revision(), &at) else {
                    continue;
                };
                if frame(&drawing, &rects, self.options.scale).iter().any(|r| {
                    r.page == page
                        && x >= r.x
                        && x <= r.x + r.width
                        && y >= r.y
                        && y <= r.y + r.height
                }) {
                    return Some(at);
                }
            }
        }
        None
    }
}
/// Merge primitive rectangles into one frame per page. For our standard canvas,
/// recover the full drawing extent from its child coordinate transform.
pub(crate) fn frame(drawing: &Drawing, rects: &[VisualRect], scale: f64) -> Vec<VisualRect> {
    fn min_nodes(g: &Graphic, min: &mut (i64, i64)) {
        match g {
            Graphic::Shape(s) if !s.connector => {
                if let Some((x, y)) = s.offset.or_else(|| s.xfrm.and_then(|x| x.offset)) {
                    min.0 = min.0.min(x.0);
                    min.1 = min.1.min(y.0);
                }
            }
            Graphic::Group(g) => {
                for c in &g.children {
                    min_nodes(c, min);
                }
            }
            _ => {}
        }
    }
    let mut merged: Vec<VisualRect> = vec![];
    for rect in rects {
        if let Some(old) = merged.iter_mut().find(|r| r.page == rect.page) {
            let right = (old.x + old.width).max(rect.x + rect.width);
            let bottom = (old.y + old.height).max(rect.y + rect.height);
            old.x = old.x.min(rect.x);
            old.y = old.y.min(rect.y);
            old.width = right - old.x;
            old.height = bottom - old.y;
        } else {
            merged.push(*rect);
        }
    }
    let Some(Graphic::Group(g)) = graphic(drawing) else {
        return merged;
    };
    if g.name.as_deref() != Some("Diagram") {
        return merged;
    }
    let Some(transform) = g.xfrm else {
        return merged;
    };
    let Some(child_extent) = transform.child_extent else {
        return merged;
    };
    let Some((ox, oy)) = transform.child_offset else {
        return merged;
    };
    let extent = match &drawing.kind {
        DrawingKind::Inline(d) => d.extent,
        DrawingKind::Anchor(d) => d.extent,
        DrawingKind::Opaque(_) => None,
    };
    let Some(extent) = extent else {
        return merged;
    };
    let mut min = (i64::MAX, i64::MAX);
    if let Some(g) = graphic(drawing) {
        min_nodes(g, &mut min);
    }
    if min.0 == i64::MAX || child_extent.cx.0 <= 0 || child_extent.cy.0 <= 0 {
        return merged;
    }
    for r in &mut merged {
        r.x -=
            (min.0 - ox.0) as f64 * extent.cx.0 as f64 / child_extent.cx.0 as f64 / 9525.0 * scale;
        r.y -=
            (min.1 - oy.0) as f64 * extent.cy.0 as f64 / child_extent.cy.0 as f64 / 9525.0 * scale;
        r.width = extent.cx.0 as f64 / 9525.0 * scale;
        r.height = extent.cy.0 as f64 / 9525.0 * scale;
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::super::{
        tests::with_worker_replies, Address, ContextObject, ObjectCommand, Reply, Request, Session,
    };
    use super::*;

    #[test]
    fn scheme_insert_edit_undo_redo_and_reopen_docx() {
        with_worker_replies(|worker, _| {
            let before = worker.session.document().body.clone();
            let plan = worker.scheme_plan().unwrap();
            let expected = plan.diagram.clone();
            assert!(matches!(
                worker.handle(Request::PutScheme(Box::new(plan))),
                Outcome::Edited
            ));
            assert_eq!(worker.undo_depth, 1);
            worker.relayout();
            let view = worker.object_view().expect("selectable diagram");
            assert!(view.scheme);
            assert_eq!(view.rects.len(), 1);
            assert!(
                (view.rects[0].width - view.width as f64 / 9525.0 * worker.options.scale).abs()
                    < 0.1,
                "{:?}; extent {} scale {}",
                view.rects,
                view.width,
                worker.options.scale
            );
            assert_eq!(worker.scheme_plan().unwrap().diagram, expected);
            let inserted = worker.session.document().body.clone();
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
            worker.undo(false);
            assert_eq!(worker.session.document().body, inserted);
            worker.object = worker
                .objects_of(&Address::body(0))
                .first()
                .map(|(_, at)| at.clone());
            let mut edit = worker.scheme_plan().unwrap();
            edit.diagram.nodes[0].text = "Изменено в документе".into();
            assert!(matches!(worker.put_scheme(edit.clone()), Outcome::Edited));
            assert_eq!(worker.undo_depth, 2);
            let edited = worker.session.document().body.clone();
            worker.undo(true);
            assert_eq!(worker.session.document().body, inserted);
            worker.undo(false);
            assert_eq!(worker.session.document().body, edited);
            let bytes = worker.session.package_bytes().unwrap();
            let review = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../target/scheme-integration-review");
            std::fs::create_dir_all(&review).unwrap();
            std::fs::write(review.join("inserted.docx"), &bytes).unwrap();
            let mut reopened =
                StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            let editor = reopened.edit(EditLimits::default()).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = eframe::egui::Context::default();
            let mut host = Worker::new(
                Session::Edit(editor),
                strict_ooxml::RenderOptions::default(),
                &tx,
                &ctx,
                None,
            );
            host.caret = Some(Logical {
                paragraph: Address::body(0),
                offset: 0,
            });
            host.object = host
                .objects_of(&Address::body(0))
                .first()
                .map(|(_, at)| at.clone());
            host.relayout();
            assert!(host.object_view().unwrap().scheme);
            let restored = host.scheme_plan().unwrap();
            assert_eq!(restored.diagram.nodes[0].text, "Изменено в документе");
            assert_eq!(
                restored.diagram.connections.len(),
                expected.connections.len()
            );
            assert!(matches!(host.put_scheme(restored), Outcome::Edited));
            for reply in rx.try_iter() {
                if let Reply::Note(n) = reply {
                    assert!(!n.error, "{}: {}", n.key, n.detail);
                }
            }
        });
    }

    #[test]
    fn scheme_padding_hit_context_resize_and_edit_keep_placement() {
        with_worker_replies(|worker, replies| {
            let plan = worker.scheme_plan().unwrap();
            worker.put_scheme(plan);
            worker.relayout();
            let original = worker.object.clone();
            let rect = worker.object_view().unwrap().rects[0];
            worker.object = None;
            worker.context_click(rect.page, rect.x + 2.0, rect.y + 2.0);
            assert_eq!(worker.object, original);
            assert!(replies
                .try_iter()
                .any(|r| matches!(r,Reply::Context(c) if c.object==Some(ContextObject::Scheme))));
            // Double-click works without waiting for the UI selection reply.
            worker.object = None;
            worker.handle(Request::SelectWord {
                page: rect.page,
                x: rect.x + 2.0,
                y: rect.y + 2.0,
            });
            assert!(replies.try_iter().any(|r| matches!(r, Reply::Scheme(_))));
            let view = worker.object_view().unwrap();
            assert!(matches!(
                worker.object_command(ObjectCommand::Resize {
                    width: view.width / 2,
                    height: view.height / 2
                }),
                Outcome::Edited
            ));
            worker.relayout();
            let resized = worker.object_view().unwrap();
            let mut plan = worker.scheme_plan().unwrap();
            plan.diagram.nodes[0].text = "new".into();
            worker.put_scheme(plan);
            worker.relayout();
            let edited = worker.object_view().unwrap();
            assert_eq!(
                (edited.width, edited.height),
                (resized.width, resized.height)
            );
            worker.scheme_sources.clear();
            assert_eq!(worker.scheme_plan().unwrap().diagram.nodes[0].text, "new");
        });
    }

    #[test]
    fn scheme_selection_replacement_is_one_undo() {
        with_worker_replies(|worker, _| {
            worker.handle(Request::Paste("before\nreplace\nafter".into()));
            worker.anchor = Some(Logical {
                paragraph: Address::body(0),
                offset: 3,
            });
            worker.caret = Some(Logical {
                paragraph: Address::body(2),
                offset: 2,
            });
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            let plan = worker.scheme_plan().unwrap();
            assert!(matches!(worker.put_scheme(plan), Outcome::Edited));
            assert_eq!(worker.undo_depth, depth + 1);
            assert!(worker.object.is_some());
            let after = worker.session.document().body.clone();
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
            worker.undo(false);
            assert_eq!(worker.session.document().body, after);
        });
    }

    #[test]
    fn scheme_draft_cancel_and_stale_apply_do_not_mutate_document() {
        with_worker_replies(|worker, replies| {
            let mut draft = worker.scheme_plan().unwrap();
            draft.diagram.nodes[0].text = "draft".into();
            assert_eq!(worker.session.revision(), draft.revision);
            assert_eq!(worker.undo_depth, 0);
            worker.handle(Request::Type("new content".into()));
            let before = worker.session.document().body.clone();
            let depth = worker.undo_depth;
            assert!(matches!(worker.put_scheme(draft), Outcome::Quiet));
            assert_eq!(worker.session.document().body, before);
            assert_eq!(worker.undo_depth, depth);
            assert!(replies
                .try_iter()
                .any(|r| matches!(r,Reply::Note(n) if n.key=="scheme.stale")));
        });
    }
}
