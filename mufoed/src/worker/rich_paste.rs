//! Clipboard import uses the library's import remapping and one final
//! define-and-transact operation. Pictures use the normal added-media API.
use super::{
    nav, next_generation, picture_part, rewrapped, set_drawing_id, set_picture_part, text_run,
    Address, Block, Edit, EditLimits, Logical, Outcome, StrictDocument, Worker, WrapStyle,
};
use strict_ooxml::edit::Definition;
use strict_ooxml::model::{Inline, RunContent};

struct Picture {
    marker: String,
    drawing: strict_ooxml::model::Drawing,
    bytes: Vec<u8>,
    standalone: bool,
}

impl Worker<'_, '_> {
    pub(super) fn paste_rich(
        &mut self,
        content: crate::clipboard::Rich,
        fallback: &str,
    ) -> Outcome {
        let mut failures = Vec::new();
        if let Some(bytes) = content.rtf {
            let end = bytes.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
            match StrictDocument::import_rtf(&bytes[..end], &strict_ooxml::RtfOptions::default()) {
                Ok(opened) => {
                    let losses = opened.import_report.loss_count;
                    match self.insert_rich_document(&opened.document) {
                        Ok(outcome) => {
                            if losses > 0 {
                                self.note("note.clipboard_partial", losses.to_string(), false);
                            }
                            return outcome;
                        }
                        Err(error) => failures.push(error),
                    }
                }
                Err(error) => failures.push(error.to_string()),
            }
        }
        if let Some(bytes) = content.html {
            let imported = (|| {
                let (fragment, base) = crate::clipboard::html_fragment(&bytes)?;
                let options = strict_ooxml::HtmlOptions {
                    base_url: base,
                    ..Default::default()
                };
                let resources = crate::clipboard::Resources::new();
                StrictDocument::import_html(fragment, &options, Some(&resources))
                    .map_err(|e| e.to_string())
            })();
            match imported {
                Ok(opened) => {
                    let losses = opened.import_report.loss_count;
                    match self.insert_rich_document(&opened.document) {
                        Ok(outcome) => {
                            if losses > 0 {
                                self.note("note.clipboard_partial", losses.to_string(), false);
                            }
                            return outcome;
                        }
                        Err(error) => failures.push(error),
                    }
                }
                Err(error) => failures.push(error),
            }
        }
        // A picture as pixels (a screenshot, an image copied from a viewer)
        // when no formatted text came with it, or that text did not import.
        if let Some(png) = content.image {
            if !failures.is_empty() {
                self.note("note.clipboard_failed", failures.join("; "), true);
            }
            return self.insert_picture(png, "clipboard");
        }
        if !failures.is_empty() {
            self.note("note.clipboard_failed", failures.join("; "), true);
        }
        if fallback.is_empty() {
            Outcome::Quiet
        } else {
            self.paste(fallback)
        }
    }

    pub(super) fn insert_rich_document(
        &mut self,
        source: &StrictDocument,
    ) -> Result<Outcome, String> {
        let Some(caret) = self.caret.clone() else {
            return Ok(Outcome::Quiet);
        };
        let (start, end) = if let Some(object) = &self.object {
            let offset = self
                .object_offset(object)
                .ok_or("selected picture position unavailable")?;
            let start = Logical {
                paragraph: object.paragraph.clone(),
                offset,
            };
            (
                start.clone(),
                Logical {
                    offset: offset + 1,
                    ..start
                },
            )
        } else if let Some(formula) = &self.formula {
            let start = self.logical(formula);
            (
                start.clone(),
                Logical {
                    offset: start.offset + 1,
                    ..start
                },
            )
        } else {
            self.selection().unwrap_or((caret.clone(), caret))
        };
        let target = self.session.document();
        let mut clean = source.document().clone();
        let mut pictures = Vec::new();
        let prefix = format!("\u{e000}mufoed-{}-", next_generation());
        sanitize(&mut clean.body.blocks, source, &prefix, &mut pictures)?;
        let count = clean.body.blocks.len();
        if count == 0 {
            return Ok(Outcome::Quiet);
        }
        let mut candidate = target.clone();
        let mut stage = strict_ooxml::edit::Editor::new(
            &mut candidate,
            EditLimits {
                history_transactions: 0,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        stage
            .import(
                stage.revision(),
                &Address::body(0),
                &clean,
                &clean.body.blocks,
            )
            .map_err(|e| e.to_string())?;
        let staged = stage.document();
        let mut definitions = Vec::new();
        for style in staged
            .styles
            .iter()
            .filter(|s| !target.styles.contains(&s.id))
        {
            definitions.push(Definition::CreateStyle(Box::new(style.clone())));
        }
        for n in staged
            .numbering
            .abstracts()
            .filter(|n| target.numbering.abstract_num(n.id).is_none())
        {
            definitions.push(Definition::CreateAbstractNum(Box::new(n.clone())));
        }
        for n in staged
            .numbering
            .nums()
            .filter(|n| target.numbering.num(n.num_id).is_none())
        {
            definitions.push(Definition::CreateNum(Box::new(n.clone())));
        }
        let mut blocks = staged.body.blocks[..count].to_vec();
        let first_id = strict_ooxml::edit::unused_drawing_id(target);
        for (number, picture) in pictures.iter_mut().enumerate() {
            let editor = self.editor().ok_or("document is read-only")?;
            let item = editor
                .add_media(picture.bytes.clone())
                .map_err(|e| e.to_string())?;
            set_picture_part(&mut picture.drawing, &item.part);
            set_drawing_id(
                &mut picture.drawing,
                first_id
                    .checked_add(number as u32)
                    .ok_or("drawing ids exhausted")?,
            );
            picture.drawing = rewrapped(picture.drawing.clone(), WrapStyle::Inline, 0);
            definitions.push(Definition::AddMedia(Box::new(item)));
        }
        restore(&mut blocks, &pictures);
        // Plan selection removal and the split against a disposable model.
        let mut document = self.session.document().clone();
        let mut plan = strict_ooxml::edit::Editor::new(
            &mut document,
            EditLimits {
                history_transactions: 0,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        let mut commands = self
            .replace_commands(&start, &end, "")
            .map_err(str::to_owned)?;
        if !commands.is_empty() {
            plan.transact(plan.revision(), &commands)
                .map_err(|e| e.to_string())?;
        }
        let paragraph = plan
            .paragraph(&start.paragraph)
            .map_err(|e| e.to_string())?;
        let split = if nav::pieces(paragraph).is_empty() {
            vec![Edit::Split {
                at: start.paragraph.clone(),
                offset: 0,
            }]
        } else {
            let (mut edits, point) = Self::point_or_run_in(&start, paragraph)
                .ok_or("cannot split clipboard destination")?;
            edits.push(Edit::SplitAt {
                at: start.paragraph.clone(),
                point,
            });
            edits
        };
        commands.extend(split);
        let first_props = blocks
            .first()
            .and_then(Block::as_paragraph)
            .map(|p| p.props.clone());
        let first_paragraph = first_props.is_some();
        let last_length = blocks
            .last()
            .and_then(Block::as_paragraph)
            .map(|p| nav::pieces(p).iter().map(nav::Piece::len).sum::<usize>());
        for (offset, block) in blocks.into_iter().enumerate() {
            let mut at = start.paragraph.clone();
            at.block += 1 + offset;
            commands.push(Edit::Insert {
                at,
                block: Box::new(block),
            });
        }
        if first_paragraph {
            commands.push(Edit::Join {
                at: start.paragraph.clone(),
            });
            if start.offset == 0 {
                commands.push(Edit::ParagraphProperties {
                    at: start.paragraph.clone(),
                    properties: Box::new(first_props.unwrap()),
                });
            }
        }
        let mut final_at = start.paragraph.clone();
        final_at.block += count - usize::from(first_paragraph);
        let final_offset = if let Some(length) = last_length {
            commands.push(Edit::Join {
                at: final_at.clone(),
            });
            length
                + if count == 1 && first_paragraph {
                    start.offset
                } else {
                    0
                }
        } else {
            final_at.block += 1;
            0
        };
        if !first_paragraph && start.offset == 0 {
            commands.push(Edit::Delete {
                at: start.paragraph.clone(),
            });
            final_at.block -= 1;
        }
        let editor = self.editor().ok_or("document is read-only")?;
        let revision = editor.revision();
        let change = editor
            .define_and_transact(revision, &definitions, &commands)
            .map_err(|e| format!("{:?}", e.error))?;
        let outcome = if change.revision == revision {
            Outcome::Quiet
        } else {
            self.undo_depth += 1;
            self.redo_depth = 0;
            self.stale = true;
            Outcome::Edited
        };
        if matches!(outcome, Outcome::Edited) {
            self.caret = Some(Logical {
                paragraph: final_at,
                offset: final_offset,
            });
            self.anchor = None;
            self.object = None;
            self.formula = None;
            self.cells = None;
        }
        Ok(outcome)
    }
}

fn sanitize(
    blocks: &mut [Block],
    source: &StrictDocument,
    prefix: &str,
    pictures: &mut Vec<Picture>,
) -> Result<(), String> {
    for block in blocks {
        match block {
            Block::Paragraph(p) => sanitize_inlines(&mut p.inlines, source, prefix, pictures)?,
            Block::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        sanitize(&mut cell.blocks, source, prefix, pictures)?;
                    }
                }
            }
            Block::SdtBlock(s) => sanitize(&mut s.blocks, source, prefix, pictures)?,
            _ => {}
        }
    }
    Ok(())
}
fn keep(
    drawing: &strict_ooxml::model::Drawing,
    source: &StrictDocument,
    prefix: &str,
    pictures: &mut Vec<Picture>,
    standalone: bool,
) -> Result<String, String> {
    let part = picture_part(drawing).ok_or("clipboard drawing is not a picture")?;
    let bytes = source
        .package()
        .read_part(&part)
        .map_err(|e| e.to_string())?;
    let marker = format!("{prefix}{}\u{e001}", pictures.len());
    pictures.push(Picture {
        marker: marker.clone(),
        drawing: drawing.clone(),
        bytes,
        standalone,
    });
    Ok(marker)
}
fn sanitize_inlines(
    inlines: &mut [Inline],
    source: &StrictDocument,
    prefix: &str,
    pictures: &mut Vec<Picture>,
) -> Result<(), String> {
    for inline in inlines {
        match inline {
            Inline::Drawing(d) => {
                let marker = keep(d, source, prefix, pictures, true)?;
                *inline = Inline::Run(text_run(
                    strict_ooxml::model::props::RunProperties::default(),
                    &marker,
                ));
            }
            Inline::Run(r) => {
                for item in &mut r.content {
                    if let RunContent::Drawing(d) = item {
                        let marker = keep(d, source, prefix, pictures, false)?;
                        *item = RunContent::Text(strict_ooxml::model::TextNode {
                            text: marker,
                            space: strict_ooxml::model::Space::Preserve,
                        });
                    }
                }
            }
            Inline::Hyperlink(h) => {
                h.rel_id = None;
                sanitize_inlines(&mut h.inlines, source, prefix, pictures)?;
            }
            Inline::Field(f) => sanitize_inlines(&mut f.inlines, source, prefix, pictures)?,
            Inline::Directional(d) => sanitize_inlines(&mut d.inlines, source, prefix, pictures)?,
            Inline::SdtInline(s) => sanitize_inlines(&mut s.inlines, source, prefix, pictures)?,
            _ => {}
        }
    }
    Ok(())
}
fn restore(blocks: &mut [Block], pictures: &[Picture]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => restore_inlines(&mut p.inlines, pictures),
            Block::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        restore(&mut cell.blocks, pictures);
                    }
                }
            }
            Block::SdtBlock(s) => restore(&mut s.blocks, pictures),
            _ => {}
        }
    }
}
fn restore_inlines(inlines: &mut [Inline], pictures: &[Picture]) {
    for inline in inlines {
        match inline {
            Inline::Run(r) => {
                if let [RunContent::Text(t)] = r.content.as_slice() {
                    if let Some(p) = pictures.iter().find(|p| p.standalone && p.marker == t.text) {
                        *inline = Inline::Drawing(p.drawing.clone());
                        continue;
                    }
                }
                for item in &mut r.content {
                    if let RunContent::Text(t) = item {
                        if let Some(p) = pictures
                            .iter()
                            .find(|p| !p.standalone && p.marker == t.text)
                        {
                            *item = RunContent::Drawing(p.drawing.clone());
                        }
                    }
                }
            }
            Inline::Hyperlink(h) => restore_inlines(&mut h.inlines, pictures),
            Inline::Field(f) => restore_inlines(&mut f.inlines, pictures),
            Inline::Directional(d) => restore_inlines(&mut d.inlines, pictures),
            Inline::SdtInline(s) => restore_inlines(&mut s.inlines, pictures),
            _ => {}
        }
    }
}
