//! The chat's view of a document, and applying one proposal as one undo step.

use std::sync::Arc;

use strict_ooxml::edit::{Address, Definition, Edit};
use strict_ooxml::model::chart::{BarGrouping, ChartData, ChartKind, ChartSeries, LegendPosition};
use strict_ooxml::model::math::{MathExpression, MathNode, MathRun, MathRunProperties};
use strict_ooxml::model::props::RunProperties;
use strict_ooxml::model::values::{HalfPoints, StyleType, TriState, Underline};
#[cfg(test)]
use strict_ooxml::model::MathParagraph;
use strict_ooxml::model::{
    Block, Bookmark, BookmarkId, DrawingKind, Field, Graphic, Inline, Paragraph, RunContent,
    StyleId,
};
use strict_ooxml_core::error::SourceLocation;

use super::{text_run, Logical, Outcome, Worker};
use crate::chat::{
    assign_numbers, plan_moves, word_spans, Batch, CommentNote, FieldNote, Formula, Op,
    Paragraph as ChatParagraph, RunMark, Table as ChatTable, View,
};
use crate::nav;

impl Worker<'_, '_> {
    /// A read-only picture of the body. The caret stays where it is.
    pub(crate) fn chat_snapshot(&mut self, job: u64) {
        let view = self.chat_view();
        self.send(super::Reply::ChatSnapshot {
            job,
            view: Box::new(view),
        });
    }

    /// Search, without moving the caret or changing the document.
    pub(crate) fn chat_find(&mut self, query: &str) {
        let view = self.chat_view();
        let hits = crate::chat::find_all(&view, query);
        let caret = view.caret;
        self.send(super::Reply::ChatFind { hits, caret });
    }

    /// Applies one proposal. Every edit in it shares a single undo step.
    /// A chart is the exception that already has its own command: a proposal
    /// that is only a chart is still one step.
    pub(crate) fn apply_chat(&mut self, batch: &Batch) -> Outcome {
        let mut definitions = Vec::new();
        let mut commands = Vec::new();
        let mut chart = None;
        let mut kinds = Vec::new();
        for op in &batch.ops {
            match op {
                Op::Replace { index, old, new } => {
                    self.replace_paragraph(*index, old, new, &mut commands);
                }
                Op::Range {
                    index,
                    old,
                    start,
                    end,
                    new,
                } => self.replace_span(*index, old, *start, *end, new, &mut commands),
                Op::Forms { index, old, pairs } => {
                    self.replace_forms(*index, old, pairs, &mut commands);
                }
                Op::Underline { index, old, spans } => {
                    self.underline_spans(*index, old, spans, &mut commands);
                }
                Op::Kind { index, kind } => kinds.push((Address::body(*index), *kind)),
                Op::Number { ids } => self.number_formulas(ids, &mut commands),
                Op::Comment {
                    index,
                    old,
                    start,
                    end,
                    text,
                } => self.comment_on(
                    *index,
                    old,
                    *start..*end,
                    text,
                    &mut definitions,
                    &mut commands,
                ),
                Op::Bookmark {
                    index,
                    old,
                    start,
                    end,
                    name,
                } => self.bookmark_on(*index, old, *start, *end, name, &mut commands),
                Op::Join { index } => commands.push(Edit::Join {
                    at: Address::body(*index),
                }),
                Op::Split { index, at } => commands.push(Edit::Split {
                    at: Address::body(*index),
                    offset: *at,
                }),
                Op::Move { .. } => {}
                Op::Cite {
                    index,
                    old,
                    formula,
                } => self.cite_formula(*index, old, formula, &mut commands),
                Op::TableHeader => self.header_row(&mut commands),
                Op::Chart { title } => chart = self.chart_from_table(title),
                Op::Caption { index, text } => {
                    self.caption_after(*index, text, &mut definitions, &mut commands);
                }
                Op::Write { paragraphs } => self.write_paragraphs(paragraphs, &mut commands),
            }
        }
        for op in &batch.ops {
            let Op::Move {
                start,
                count,
                to,
                level,
            } = op
            else {
                continue;
            };
            if let Some(level) = level {
                kinds.push((Address::body(*start), crate::ai::Kind::Heading(*level)));
            }
            for (at, dest) in plan_moves(*start, *count, *to) {
                commands.push(Edit::Move {
                    at: Address::body(at),
                    to: Address::body(dest),
                });
            }
        }
        if !kinds.is_empty() {
            let (styles, edits) = self.kind_plan(&kinds);
            definitions.extend(styles);
            commands.splice(0..0, edits);
        }
        let edited = if commands.is_empty() && definitions.is_empty() {
            false
        } else if definitions.is_empty() {
            self.transact(&commands)
        } else {
            matches!(
                self.define_and_transact(&definitions, &commands),
                Outcome::Edited
            )
        };
        if let Some(data) = chart {
            return self.place_chart(data);
        }
        if edited {
            Outcome::Edited
        } else {
            Outcome::Quiet
        }
    }

    /// A displayed formula at the caret, for tests that need one.
    #[cfg(test)]
    pub(crate) fn insert_display(&mut self, latex: &str) -> bool {
        let Ok(expression) = crate::formula::from_latex(latex) else {
            return false;
        };
        let Some(caret) = self.caret.clone() else {
            return false;
        };
        let inline = Inline::MathParagraph(MathParagraph {
            properties: None,
            equations: vec![expression],
            location: SourceLocation::unknown(),
        });
        let commands = self.inline_at_caret(&caret, inline);
        self.transact(&commands)
    }

    fn chat_view(&self) -> View {
        let document = self.session.document();
        let mut paragraphs = Vec::new();
        let mut formulas = Vec::new();
        let mut tables = Vec::new();
        let mut pictures = Vec::new();
        let mut comments = Vec::new();
        let mut fields = Vec::new();
        let mut runs = Vec::new();
        let mut charts = 0;
        let mut displayed = 0_u32;
        let mut inline_count = 0_u32;
        for (index, block) in document.body.blocks.iter().enumerate() {
            match block {
                Block::Paragraph(paragraph) => {
                    let text = shown(paragraph);
                    paragraphs.push(ChatParagraph {
                        index,
                        text,
                        heading: super::heading_level(document, paragraph),
                        style: paragraph
                            .props
                            .style
                            .as_ref()
                            .map(|id| id.as_str().to_owned())
                            .unwrap_or_default(),
                    });
                    runs.push(run_marks(paragraph));
                    let mut picture = false;
                    for inline in &paragraph.inlines {
                        match inline {
                            Inline::Math(expression) => {
                                inline_count += 1;
                                formulas.push(formula_view(
                                    format!("i{inline_count}"),
                                    index,
                                    false,
                                    expression,
                                ));
                            }
                            Inline::MathParagraph(math) => {
                                if let Some(expression) = math.equations.first() {
                                    displayed += 1;
                                    formulas.push(formula_view(
                                        format!("f{displayed}"),
                                        index,
                                        true,
                                        expression,
                                    ));
                                }
                            }
                            Inline::CommentRangeStart(id) => {
                                if let Some(text) = document
                                    .comments
                                    .iter()
                                    .find(|comment| comment.id.as_str() == id.as_str())
                                    .map(|comment| super::comment_text(&comment.blocks))
                                {
                                    comments.push(CommentNote {
                                        paragraph: index,
                                        text,
                                    });
                                }
                            }
                            Inline::Field(field) => fields.push(FieldNote {
                                instruction: field
                                    .instruction
                                    .as_deref()
                                    .unwrap_or_default()
                                    .to_owned(),
                                text: inline_text(&field.inlines),
                            }),
                            _ => {}
                        }
                        if let Some(drawing) = drawing_in(inline) {
                            if is_chart(drawing) {
                                charts += 1;
                            } else {
                                picture = true;
                            }
                        }
                    }
                    if picture {
                        pictures.push(index);
                    }
                }
                Block::Table(table) => {
                    let header = table
                        .rows
                        .first()
                        .is_some_and(|row| row.props.header == TriState::On);
                    let rows = table
                        .rows
                        .iter()
                        .map(|row| {
                            row.cells
                                .iter()
                                .map(|cell| {
                                    cell.blocks
                                        .iter()
                                        .filter_map(|block| match block {
                                            Block::Paragraph(paragraph) => Some(shown(paragraph)),
                                            _ => None,
                                        })
                                        .collect::<String>()
                                })
                                .collect()
                        })
                        .collect();
                    tables.push(ChatTable { rows, header });
                }
                _ => {}
            }
        }
        View {
            paragraphs,
            selection: self.selected_text().filter(|text| !text.is_empty()),
            formulas,
            tables,
            pictures,
            charts,
            comments,
            fields,
            runs,
            caret: self
                .caret
                .as_ref()
                .map(|caret| (caret.paragraph.block, caret.offset)),
        }
    }

    fn paragraph_reads(&self, index: usize, old: &str) -> Option<Address> {
        let at = Address::body(index);
        let paragraph = self.paragraph(&at)?;
        (shown(paragraph) == old).then_some(at)
    }

    fn replace_paragraph(&self, index: usize, old: &str, new: &str, commands: &mut Vec<Edit>) {
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        let Some(paragraph) = self.paragraph(&at) else {
            return;
        };
        if crate::ai::protected(old) {
            let Some(inlines) = crate::ai::restore(paragraph, new) else {
                return;
            };
            replace_inlines(&at, paragraph.inlines.len(), inlines, commands);
            return;
        }
        // A paragraph cannot hold a line break, so a piece is several paragraphs.
        let pieces = if new.chars().any(|c| matches!(c, '\n' | '\r' | '\t')) {
            crate::chat::paragraphs_of(new)
        } else {
            vec![new.to_owned()]
        };
        if pieces.is_empty() {
            return;
        }
        commands.push(Edit::Text {
            at,
            range: 0..old.chars().count(),
            text: pieces[0].clone(),
        });
        insert_paragraphs(index + 1, &pieces[1..], commands);
    }

    fn replace_span(
        &self,
        index: usize,
        old: &str,
        start: usize,
        end: usize,
        new: &str,
        commands: &mut Vec<Edit>,
    ) {
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        if old.contains('⟦') {
            return;
        }
        commands.push(Edit::Text {
            at,
            range: start..end,
            text: new.to_owned(),
        });
    }

    fn replace_forms(
        &self,
        index: usize,
        old: &str,
        pairs: &[(String, String)],
        commands: &mut Vec<Edit>,
    ) {
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        let mut spans = Vec::new();
        for (from, to) in pairs {
            for (start, end) in word_spans(old, from) {
                spans.push((start, end, to.clone()));
            }
        }
        spans.sort_by_key(|span| std::cmp::Reverse(span.0));
        for (start, end, to) in spans {
            commands.push(Edit::Text {
                at: at.clone(),
                range: start..end,
                text: to,
            });
        }
    }

    fn underline_spans(
        &self,
        index: usize,
        old: &str,
        spans: &[(usize, usize)],
        commands: &mut Vec<Edit>,
    ) {
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        let patch = strict_ooxml::edit::FormatPatch {
            underline: Some(Some(Underline::Single)),
            ..strict_ooxml::edit::FormatPatch::default()
        };
        let mut spans = spans.to_vec();
        spans.sort_by_key(|span| std::cmp::Reverse(span.0));
        for (start, end) in spans {
            commands.push(Edit::Format {
                at: at.clone(),
                range: start..end,
                patch: patch.clone(),
            });
        }
    }

    fn comment_on(
        &self,
        index: usize,
        old: &str,
        span: std::ops::Range<usize>,
        text: &str,
        definitions: &mut Vec<Definition>,
        commands: &mut Vec<Edit>,
    ) {
        let std::ops::Range { start, end } = span;
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        let Some(paragraph) = self.paragraph(&at) else {
            return;
        };
        let comment = strict_ooxml::edit::new_comment(
            self.session.document(),
            "MuFoEd",
            "AI",
            &super::now_iso(),
            text,
        );
        let id = comment.id.clone();
        let end_at = Logical {
            paragraph: at.clone(),
            offset: end,
        };
        let start_at = Logical {
            paragraph: at,
            offset: start,
        };
        let mut marks = Vec::new();
        if start == end {
            marks.extend(Worker::inlines_in_paragraph(
                &start_at,
                paragraph,
                vec![
                    Inline::CommentRangeStart(id.clone()),
                    Inline::CommentRangeEnd(id.clone()),
                    Inline::CommentReference(id),
                ],
            ));
        } else {
            marks.extend(Worker::inlines_in_paragraph(
                &end_at,
                paragraph,
                vec![
                    Inline::CommentRangeEnd(id.clone()),
                    Inline::CommentReference(id.clone()),
                ],
            ));
            marks.extend(Worker::inlines_in_paragraph(
                &start_at,
                paragraph,
                vec![Inline::CommentRangeStart(id)],
            ));
        }
        if marks.is_empty() {
            return;
        }
        definitions.push(Definition::AddComment(Box::new(comment)));
        commands.extend(marks);
    }

    fn bookmark_on(
        &self,
        index: usize,
        old: &str,
        start: usize,
        end: usize,
        name: &str,
        commands: &mut Vec<Edit>,
    ) {
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        let Some(paragraph) = self.paragraph(&at) else {
            return;
        };
        let id = self
            .bookmark_ids()
            .into_iter()
            .max()
            .map_or(0, |top| top + 1);
        let id = BookmarkId::new(id.to_string());
        let start_mark = Inline::BookmarkStart(Bookmark {
            id: id.clone(),
            name: Arc::from(name),
            col_first: None,
            col_last: None,
        });
        let end_mark = Inline::BookmarkEnd(id);
        let end_at = Logical {
            paragraph: at.clone(),
            offset: end,
        };
        let start_at = Logical {
            paragraph: at,
            offset: start,
        };
        if start == end {
            commands.extend(Worker::inlines_in_paragraph(
                &start_at,
                paragraph,
                vec![start_mark, end_mark],
            ));
        } else {
            commands.extend(Worker::inlines_in_paragraph(
                &end_at,
                paragraph,
                vec![end_mark],
            ));
            commands.extend(Worker::inlines_in_paragraph(
                &start_at,
                paragraph,
                vec![start_mark],
            ));
        }
    }

    fn cite_formula(&self, index: usize, old: &str, formula: &str, commands: &mut Vec<Edit>) {
        let Some(at) = self.paragraph_reads(index, old) else {
            return;
        };
        let Some(paragraph) = self.paragraph(&at) else {
            return;
        };
        let number = self
            .chat_view()
            .formulas
            .into_iter()
            .find(|item| item.id == formula)
            .and_then(|item| item.number)
            .unwrap_or_else(|| "?".to_owned());
        let name = format!("eq{formula}");
        let field = Inline::Field(Field {
            instruction: Some(Arc::from(format!(" REF {name} \\h "))),
            inlines: vec![Inline::Run(text_run(RunProperties::default(), &number))],
            location: SourceLocation::unknown(),
        });
        let caret = Logical {
            paragraph: at,
            offset: old.chars().count(),
        };
        commands.extend(Worker::inlines_in_paragraph(&caret, paragraph, vec![field]));
    }

    fn number_formulas(&self, ids: &[String], commands: &mut Vec<Edit>) {
        let view = self.chat_view();
        let assigned = assign_numbers(&view.formulas, ids);
        if assigned.is_empty() {
            return;
        }
        let document = self.session.document();
        let mut displayed = 0_u32;
        let mut bookmarks = Vec::new();
        for (index, block) in document.body.blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else {
                continue;
            };
            let at = Address::body(index);
            for (inline_index, inline) in paragraph.inlines.iter().enumerate() {
                let Inline::MathParagraph(math) = inline else {
                    continue;
                };
                let Some(expression) = math.equations.first() else {
                    continue;
                };
                displayed += 1;
                let id = format!("f{displayed}");
                let Some((_, number)) = assigned.iter().find(|(name, _)| name == &id) else {
                    continue;
                };
                let mut expression = expression.clone();
                stamp_number(&mut expression, *number);
                let mut wrapped = math.clone();
                if let Some(first) = wrapped.equations.first_mut() {
                    *first = expression;
                }
                commands.push(Edit::DeleteInline {
                    at: at.clone(),
                    index: inline_index,
                });
                commands.push(Edit::InsertInline {
                    at: at.clone(),
                    index: inline_index,
                    inline: Box::new(Inline::MathParagraph(wrapped)),
                });
                let bookmark = format!("eq{id}");
                if !has_bookmark(paragraph, &bookmark) {
                    bookmarks.push((at.clone(), inline_index, bookmark));
                }
            }
        }
        self.retarget_fields(&assigned, commands);
        bookmarks.sort_by_key(|(_, index, _)| std::cmp::Reverse(*index));
        let mut next_id = self
            .bookmark_ids()
            .into_iter()
            .max()
            .map_or(0, |top| top + 1);
        for (at, index, name) in bookmarks {
            let id = BookmarkId::new(next_id.to_string());
            next_id += 1;
            commands.push(Edit::InsertInline {
                at: at.clone(),
                index: index + 1,
                inline: Box::new(Inline::BookmarkEnd(id.clone())),
            });
            commands.push(Edit::InsertInline {
                at,
                index,
                inline: Box::new(Inline::BookmarkStart(Bookmark {
                    id,
                    name: Arc::from(name),
                    col_first: None,
                    col_last: None,
                })),
            });
        }
    }

    fn retarget_fields(&self, assigned: &[(String, u32)], commands: &mut Vec<Edit>) {
        let document = self.session.document();
        for (index, block) in document.body.blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else {
                continue;
            };
            let at = Address::body(index);
            for (inline_index, inline) in paragraph.inlines.iter().enumerate().rev() {
                let Inline::Field(field) = inline else {
                    continue;
                };
                let Some(instruction) = field.instruction.as_deref() else {
                    continue;
                };
                let Some(number) = assigned.iter().find_map(|(id, number)| {
                    instruction
                        .split_whitespace()
                        .any(|word| word == format!("eq{id}"))
                        .then_some(number.to_string())
                }) else {
                    continue;
                };
                let mut field = field.clone();
                field.inlines = vec![Inline::Run(text_run(RunProperties::default(), &number))];
                commands.push(Edit::DeleteInline {
                    at: at.clone(),
                    index: inline_index,
                });
                commands.push(Edit::InsertInline {
                    at: at.clone(),
                    index: inline_index,
                    inline: Box::new(Inline::Field(field)),
                });
            }
        }
    }

    fn header_row(&self, commands: &mut Vec<Edit>) {
        let document = self.session.document();
        let Some((index, table)) =
            document
                .body
                .blocks
                .iter()
                .enumerate()
                .find_map(|(index, block)| match block {
                    Block::Table(table) if !table.rows.is_empty() => Some((index, table)),
                    _ => None,
                })
        else {
            return;
        };
        let mut row = table.rows[0].clone();
        row.props.header = TriState::On;
        let at = Address::body(index);
        commands.push(Edit::InsertRow {
            at: at.clone(),
            index: 0,
            row: Box::new(row),
        });
        commands.push(Edit::DeleteRow { at, index: 1 });
    }

    fn chart_from_table(&self, title: &str) -> Option<ChartData> {
        let document = self.session.document();
        let table = document.body.blocks.iter().find_map(|block| match block {
            Block::Table(table) => Some(table),
            _ => None,
        })?;
        let mut categories = Vec::new();
        let mut values = Vec::new();
        for row in &table.rows {
            for (column, cell) in row.cells.iter().enumerate() {
                let text: String = cell
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Paragraph(paragraph) => Some(shown(paragraph)),
                        _ => None,
                    })
                    .collect();
                let Ok(number) = text.trim().parse::<f64>() else {
                    continue;
                };
                if !number.is_finite() {
                    continue;
                }
                categories.push((column + 1).to_string());
                values.push(Some(number));
            }
        }
        if values.is_empty() {
            return None;
        }
        Some(ChartData {
            kind: ChartKind::Bar {
                horizontal: false,
                grouping: BarGrouping::Clustered,
            },
            title: (!title.is_empty()).then(|| title.to_owned()),
            categories,
            series: vec![ChartSeries {
                name: Some(title.to_owned()),
                values,
                ..ChartSeries::default()
            }],
            legend: false,
            legend_position: LegendPosition::Bottom,
            unsupported: Vec::new(),
        })
    }

    fn place_chart(&mut self, data: ChartData) -> Outcome {
        let document = self.session.document();
        let after = document
            .body
            .blocks
            .iter()
            .position(|block| matches!(block, Block::Table(_)))
            .map(|index| index + 1);
        if let Some(index) = after {
            let at = Address::body(index);
            if self.paragraph(&at).is_some() {
                self.anchor = None;
                self.caret = Some(Logical {
                    paragraph: at,
                    offset: 0,
                });
            }
        }
        self.put_chart(data)
    }

    /// Puts new paragraphs after the text already there. An empty document
    /// takes the first paragraph in its blank paragraph, unless that paragraph
    /// carries the section: then every paragraph is inserted before it, so the
    /// section stays last.
    fn write_paragraphs(&self, texts: &[String], commands: &mut Vec<Edit>) {
        if texts.is_empty() {
            return;
        }
        let document = self.session.document();
        let blocks = &document.body.blocks;
        let mut last_content = None;
        let mut first_paragraph = None;
        let mut only_empty = true;
        for (index, block) in blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else {
                only_empty = false;
                last_content = Some(index);
                continue;
            };
            if first_paragraph.is_none() {
                first_paragraph = Some(index);
            }
            if !shown(paragraph).is_empty() {
                only_empty = false;
                last_content = Some(index);
            }
        }
        if only_empty {
            if let Some(index) = first_paragraph {
                let boundary = matches!(
                    &blocks[index],
                    Block::Paragraph(paragraph) if paragraph.props.section.is_some()
                );
                if boundary {
                    insert_paragraphs(index, texts, commands);
                    return;
                }
                commands.push(Edit::Text {
                    at: Address::body(index),
                    range: 0..0,
                    text: texts[0].clone(),
                });
                insert_paragraphs(index + 1, &texts[1..], commands);
                return;
            }
        }
        let at = last_content.map_or(0, |index| index + 1);
        insert_paragraphs(at, texts, commands);
    }

    fn caption_after(
        &self,
        index: usize,
        text: &str,
        definitions: &mut Vec<Definition>,
        commands: &mut Vec<Edit>,
    ) {
        let style = self.caption_style(definitions);
        let mut paragraph = super::empty_paragraph();
        paragraph.props.style = Some(style);
        paragraph
            .inlines
            .push(Inline::Run(text_run(RunProperties::default(), text)));
        commands.push(Edit::Insert {
            at: Address::body(index + 1),
            block: Box::new(Block::Paragraph(paragraph)),
        });
    }

    fn caption_style(&self, definitions: &mut Vec<Definition>) -> StyleId {
        let id = StyleId::new("Caption");
        let document = self.session.document();
        if document.styles.contains(&id) {
            return id;
        }
        let Some(default) = document
            .styles
            .iter()
            .find(|style| style.style_type == StyleType::Paragraph && style.is_default)
            .cloned()
        else {
            return id;
        };
        let default_id = default.id.clone();
        let mut style = default;
        style.id = id.clone();
        style.name = Some(Arc::from("caption"));
        style.is_default = false;
        style.q_format = true;
        style.based_on = Some(default_id.clone());
        style.next = Some(default_id.clone());
        style.based_on_chain = vec![default_id];
        style.paragraph = strict_ooxml::model::props::ParagraphProperties::default();
        style.run = RunProperties::default();
        style.run.italic = TriState::On;
        style.run.size = Some(HalfPoints(18));
        definitions.push(Definition::CreateStyle(Box::new(style)));
        id
    }
}

fn insert_paragraphs(start: usize, texts: &[String], commands: &mut Vec<Edit>) {
    for (offset, text) in texts.iter().enumerate() {
        let mut paragraph = super::empty_paragraph();
        paragraph
            .inlines
            .push(Inline::Run(text_run(RunProperties::default(), text)));
        commands.push(Edit::Insert {
            at: Address::body(start + offset),
            block: Box::new(Block::Paragraph(paragraph)),
        });
    }
}

fn shown(paragraph: &Paragraph) -> String {
    crate::ai::expose(paragraph).unwrap_or_else(|| {
        nav::pieces(paragraph)
            .iter()
            .map(|piece| piece.text.as_str())
            .collect()
    })
}

fn run_marks(paragraph: &Paragraph) -> Vec<RunMark> {
    let mut marks = Vec::new();
    for inline in &paragraph.inlines {
        let Inline::Run(run) = inline else {
            continue;
        };
        let text: String = run
            .content
            .iter()
            .filter_map(|content| match content {
                RunContent::Text(node) => Some(node.text.as_str()),
                _ => None,
            })
            .collect();
        if text.is_empty() {
            continue;
        }
        marks.push(RunMark {
            text,
            bold: run.props.bold == TriState::On,
            underline: run
                .props
                .underline
                .is_some_and(|underline| underline != Underline::None),
        });
    }
    marks
}

fn formula_view(
    id: String,
    paragraph: usize,
    display: bool,
    expression: &MathExpression,
) -> Formula {
    let (latex, number) = split_number(expression);
    Formula {
        id,
        paragraph,
        display,
        latex,
        number,
    }
}

fn split_number(expression: &MathExpression) -> (String, Option<String>) {
    let mut nodes = expression.nodes.clone();
    let number = take_number(&mut nodes);
    let body = MathExpression {
        nodes,
        location: expression.location.clone(),
    };
    (crate::formula::to_latex(&body), number)
}

fn take_number(nodes: &mut Vec<MathNode>) -> Option<String> {
    let index = nodes
        .iter()
        .position(|node| matches!(node, MathNode::Run(run) if run.text.contains('#')))?;
    let MathNode::Run(run) = nodes.get(index)?.clone() else {
        return None;
    };
    let (_, after) = run.text.split_once('#')?;
    let mut number = after.trim().to_owned();
    for node in nodes.iter().skip(index + 1) {
        if let MathNode::Run(tail) = node {
            number.push_str(tail.text.trim());
        }
    }
    let before = run
        .text
        .split_once('#')
        .map_or(String::new(), |(before, _)| before.to_owned());
    if before.is_empty() {
        nodes.drain(index..);
    } else {
        let mut kept = run;
        kept.text = before;
        nodes.truncate(index);
        nodes.push(MathNode::Run(kept));
    }
    let number = number.trim().to_owned();
    (!number.is_empty()).then_some(number)
}

fn stamp_number(expression: &mut MathExpression, number: u32) {
    take_number(&mut expression.nodes);
    expression.nodes.push(MathNode::Run(MathRun {
        field_char: None,
        properties: MathRunProperties::default(),
        run_properties: None,
        text: format!("#{number}"),
        location: SourceLocation::unknown(),
    }));
}

fn replace_inlines(at: &Address, count: usize, inlines: Vec<Inline>, commands: &mut Vec<Edit>) {
    for index in (0..count).rev() {
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
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        if let Inline::Run(run) = inline {
            for content in &run.content {
                if let RunContent::Text(node) = content {
                    text.push_str(&node.text);
                }
            }
        }
    }
    text
}

fn drawing_in(inline: &Inline) -> Option<&strict_ooxml::model::Drawing> {
    match inline {
        Inline::Drawing(drawing) => Some(drawing),
        Inline::Run(run) => run.content.iter().find_map(|content| match content {
            RunContent::Drawing(drawing) => Some(drawing),
            _ => None,
        }),
        _ => None,
    }
}

fn is_chart(drawing: &strict_ooxml::model::Drawing) -> bool {
    let graphic = match &drawing.kind {
        DrawingKind::Inline(inline) => &*inline.graphic,
        DrawingKind::Anchor(anchor) => &*anchor.graphic,
        DrawingKind::Opaque(_) => return false,
    };
    matches!(graphic, Graphic::Chart(_))
}

fn has_bookmark(paragraph: &Paragraph, name: &str) -> bool {
    paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::BookmarkStart(mark) if mark.name.as_ref() == name))
}
