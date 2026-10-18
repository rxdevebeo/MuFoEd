//! Document integration for the independent chemistry widget. Only public
//! StrictLib APIs; the drawing is written and read through `chem-editor`'s
//! model API, and the host keeps the last source for faithful reopening.
use super::scheme::frame;
use super::{DrawingPosition, Edit, EditLimits, Logical, Outcome, VisualRect, Worker};
use chem_editor::ChemDocument;
use strict_ooxml::model::{Drawing, DrawingKind, Graphic, Inline};

#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub(crate) document: ChemDocument,
    revision: u64,
    replace: Option<(DrawingPosition, Drawing)>,
    caret: Logical,
    selection: Option<(Logical, Logical)>,
    pub(crate) issues: Vec<String>,
}

#[cfg(test)]
pub(crate) fn preview_plan() -> Plan {
    Plan {
        document: chem_editor::ChemEditor::sample().document().clone(),
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

pub(crate) fn is_chemistry(drawing: &Drawing) -> bool {
    matches!(graphic(drawing), Some(Graphic::Group(g))
        if g.name.as_deref() == Some(chem_editor::DRAWING_GROUP))
}

impl Worker<'_, '_> {
    /// Text inside a grouped drawing is outside the host paragraph's caret
    /// flow. Use the drawing boundary instead of repeating its child anchors.
    pub(super) fn chem_carets(&self, original: Vec<VisualRect>) -> Vec<VisualRect> {
        let Some(caret) = &self.caret else {
            return original;
        };
        let Some(map) = &self.map else {
            return original;
        };
        for (offset, at) in self.objects_of(&caret.paragraph) {
            if caret.offset != offset && caret.offset != offset + 1 {
                continue;
            }
            let Some(drawing) = self.drawing_at(&at).filter(is_chemistry) else {
                continue;
            };
            let Ok(rects) = map.drawing_rects(self.session.revision(), &at) else {
                continue;
            };
            let boundary = frame(&drawing, &rects, self.options.scale);
            if boundary.is_empty() {
                continue;
            }
            return boundary
                .into_iter()
                .map(|r| VisualRect {
                    x: if caret.offset == offset {
                        r.x - 2.0
                    } else {
                        r.x + r.width + 2.0
                    },
                    width: 1.0,
                    ..r
                })
                .collect();
        }
        original
    }

    pub(super) fn chem_plan(&self) -> Result<Plan, String> {
        if self.read_only.is_some() {
            return Err("Document is read-only".into());
        }
        let caret = self
            .caret
            .clone()
            .ok_or("Place the caret in the document")?;
        let selected = self.object.as_ref().and_then(|p| {
            self.drawing_at(p)
                .filter(is_chemistry)
                .map(|d| (p.clone(), d))
        });
        let (document, issues) = if let Some((_, drawing)) = &selected {
            if let Some((_, source)) = self
                .chem_sources
                .iter()
                .rev()
                .find(|(g, _)| Some(g) == graphic(drawing))
            {
                (source.clone(), vec![])
            } else {
                ChemDocument::from_drawing(drawing)?
            }
        } else {
            (chem_editor::ChemEditor::sample().document().clone(), vec![])
        };
        Ok(Plan {
            document,
            revision: self.session.revision(),
            replace: selected,
            caret,
            selection: self.selection(),
            issues,
        })
    }

    pub(super) fn put_chemistry(&mut self, plan: Plan) -> Outcome {
        if self.session.revision() != plan.revision
            || plan
                .replace
                .as_ref()
                .is_some_and(|(p, d)| self.drawing_at(p).as_ref() != Some(d))
        {
            self.note("chemistry.stale", "", true);
            return Outcome::Quiet;
        }
        if plan.document.objects.is_empty() {
            self.note("chemistry.failed", "Empty chemistry document", true);
            return Outcome::Quiet;
        }
        let result = (|| -> Result<(Vec<Edit>, Drawing, Vec<String>), String> {
            let mut drawing = plan.document.to_drawing()?;
            if let Some((position, original)) = &plan.replace {
                let payload = graphic(&drawing)
                    .cloned()
                    .ok_or("Missing chemistry graphic")?;
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
                    vec![],
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
            Ok((commands, drawing, vec![]))
        })();
        let (commands, drawing, issues) = match result {
            Ok(v) => v,
            Err(e) => {
                self.note("chemistry.failed", e, true);
                return Outcome::Quiet;
            }
        };
        if !self.transact(&commands) {
            return Outcome::Quiet;
        }
        if let Some(g) = graphic(&drawing) {
            self.chem_sources.push((g.clone(), plan.document));
            if self.chem_sources.len() > 16 {
                self.chem_sources.remove(0);
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
            self.note("chemistry.applied", issues.join("; "), false);
        }
        Outcome::Edited
    }

    pub(super) fn chem_hit(&self, page: usize, x: f64, y: f64) -> Option<DrawingPosition> {
        let map = self.map.as_ref()?;
        for paragraph in self.paragraphs(&self.story) {
            for (_, at) in self.objects_of(&paragraph) {
                let Some(drawing) = self.drawing_at(&at) else {
                    continue;
                };
                if !is_chemistry(&drawing) {
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

#[cfg(test)]
mod tests {
    use super::super::{tests::with_worker_replies, Request};
    use super::*;

    #[test]
    fn chemistry_insert_reopen_and_undo() {
        with_worker_replies(|worker, _| {
            let before = worker.session.document().body.clone();
            let plan = worker.chem_plan().unwrap();
            let expected = plan.document.clone();
            assert!(matches!(
                worker.handle(Request::PutChemistry(Box::new(plan))),
                Outcome::Edited
            ));
            assert_eq!(worker.undo_depth, 1);
            worker.relayout();
            let view = worker.object_view().expect("selectable chemistry");
            assert!(view.chemistry);
            assert_eq!(worker.chem_plan().unwrap().document, expected);
            let inserted = worker.session.document().body.clone();
            worker.undo(true);
            assert_eq!(worker.session.document().body, before);
            worker.undo(false);
            assert_eq!(worker.session.document().body, inserted);
        });
    }
}
