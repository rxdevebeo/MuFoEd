//! Reviewable document housekeeping, using the editor's existing block edits.
use strict_ooxml::edit::{Address, Edit};
use strict_ooxml::model::values::{Indentation, Twips};
use strict_ooxml::model::{Block, Document, Inline, Paragraph, RunContent, Space, TextNode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Indent {
    #[default]
    Keep,
    Tab,
    Paragraph,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Options {
    pub(crate) trailing: bool,
    pub(crate) spaces: bool,
    pub(crate) punctuation: bool,
    pub(crate) tabs: bool,
    pub(crate) empty: bool,
    pub(crate) soft_hyphens: bool,
    pub(crate) zero_width: bool,
    pub(crate) indent: Indent,
    pub(crate) indent_cm: f32,
    pub(crate) joins: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            trailing: true,
            spaces: true,
            punctuation: true,
            tabs: false,
            empty: true,
            soft_hyphens: true,
            zero_width: false,
            indent: Indent::Tab,
            indent_cm: 1.25,
            joins: true,
        }
    }
}

impl Options {
    pub(crate) fn show(
        &mut self,
        ctx: &eframe::egui::Context,
        i18n: &crate::i18n::I18n,
    ) -> (bool, bool) {
        use eframe::egui;
        let mut start = false;
        let mut close = false;
        let response = crate::dialog::modal(ctx, "cleanup-options").show(ctx, |ui| {
            ui.set_width(510.0);
            crate::dialog::title(ui, i18n.t("cleanup.title"));
            ui.label(i18n.t("cleanup.intro"));
            ui.add_space(8.0);
            for (key, value) in [
                ("cleanup.trailing", &mut self.trailing),
                ("cleanup.spaces", &mut self.spaces),
                ("cleanup.punctuation", &mut self.punctuation),
                ("cleanup.tabs", &mut self.tabs),
                ("cleanup.empty", &mut self.empty),
                ("cleanup.soft_hyphens", &mut self.soft_hyphens),
                ("cleanup.zero_width", &mut self.zero_width),
            ] {
                ui.checkbox(value, i18n.t(key));
            }
            ui.horizontal(|ui| {
                ui.label(i18n.t("cleanup.indent"));
                egui::ComboBox::from_id_salt("cleanup-indent")
                    .selected_text(i18n.t(match self.indent {
                        Indent::Keep => "cleanup.indent_keep",
                        Indent::Tab => "cleanup.indent_tab",
                        Indent::Paragraph => "cleanup.indent_paragraph",
                    }))
                    .show_ui(ui, |ui| {
                        for (value, key) in [
                            (Indent::Keep, "cleanup.indent_keep"),
                            (Indent::Tab, "cleanup.indent_tab"),
                            (Indent::Paragraph, "cleanup.indent_paragraph"),
                        ] {
                            ui.selectable_value(&mut self.indent, value, i18n.t(key));
                        }
                    });
            });
            if self.indent == Indent::Paragraph {
                ui.add(
                    egui::DragValue::new(&mut self.indent_cm)
                        .range(0.1..=10.0)
                        .speed(0.05)
                        .suffix(i18n.t("paragraph.cm")),
                );
            }
            ui.separator();
            ui.checkbox(&mut self.joins, i18n.t("cleanup.joins"));
            ui.label(egui::RichText::new(i18n.t("cleanup.protected")).weak());
            crate::dialog::buttons(ui, |ui| {
                if crate::dialog::primary(ui, i18n.t("cleanup.analyze")).clicked() {
                    start = true;
                    close = true;
                }
                if crate::dialog::button(ui, i18n.t("button.cancel")).clicked() {
                    close = true;
                }
            });
        });
        (start, close || response.should_close())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) block: usize,
    pub(crate) paragraph: Paragraph,
    pub(crate) joinable: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    pub(crate) revision: u64,
    pub(crate) entries: Vec<Entry>,
    pub(crate) skipped: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub(crate) block: usize,
    pub(crate) before: Vec<Paragraph>,
    pub(crate) after: Option<Paragraph>,
    pub(crate) key: &'static str,
    pub(crate) selected: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Plan {
    pub(crate) revision: u64,
    pub(crate) changes: Vec<Change>,
    pub(crate) skipped: usize,
}

pub(crate) fn text(p: &Paragraph) -> String {
    crate::nav::pieces(p).into_iter().map(|p| p.text).collect()
}

pub(crate) fn snapshot(document: &Document, revision: u64) -> Snapshot {
    let mut snapshot = Snapshot {
        revision,
        ..Snapshot::default()
    };
    for (block, item) in document.body.blocks.iter().enumerate() {
        let Block::Paragraph(p) = item else {
            snapshot.skipped += 1;
            continue;
        };
        let style = p
            .props
            .style
            .as_ref()
            .and_then(|id| document.styles.get(id));
        let style_name = style
            .and_then(|s| s.name.as_deref())
            .unwrap_or_default()
            .to_lowercase();
        let style_id = p
            .props
            .style
            .as_ref()
            .map_or("", |id| id.as_str())
            .to_lowercase();
        let code = ["code", "preformatted", "код"]
            .iter()
            .any(|name| style_name.contains(name) || style_id.contains(name))
            || p.inlines.iter().any(|inline| match inline {
                Inline::Run(run) => run.props.fonts.as_ref().is_some_and(|fonts| {
                    [&fonts.ascii, &fonts.h_ansi]
                        .into_iter()
                        .flatten()
                        .any(|name| {
                            ["consolas", "courier", "monospace"]
                                .iter()
                                .any(|font| name.to_lowercase().contains(font))
                        })
                }),
                _ => false,
            });
        let safe = !code
            && p.revision.is_none()
            && p.props.section.is_none()
            && p.props.frame.is_none()
            && p.props.page_break_before != strict_ooxml::model::values::TriState::On
            && !style.is_some_and(|s| {
                s.paragraph.page_break_before == strict_ooxml::model::values::TriState::On
            })
            && p.inlines.iter().all(|inline| match inline {
                Inline::Run(run) => {
                    run.revision.is_none()
                        && run
                            .content
                            .iter()
                            .all(|c| matches!(c, RunContent::Text(_) | RunContent::Tab))
                }
                Inline::Tab => true,
                _ => false,
            });
        if !safe {
            snapshot.skipped += 1;
            continue;
        }
        let heading = p.props.outline_level.is_some_and(|n| n < 9)
            || style.is_some_and(|s| s.paragraph.outline_level.is_some_and(|n| n < 9))
            || ["title", "heading", "заголовок"]
                .iter()
                .any(|word| style_name.contains(word));
        snapshot.entries.push(Entry {
            block,
            paragraph: p.clone(),
            joinable: !heading
                && p.props.numbering.is_none()
                && style.is_none_or(|s| s.paragraph.numbering.is_none()),
        });
    }
    snapshot
}

/// Keep each surviving character's run, so housekeeping does not flatten formatting.
fn clean(p: &Paragraph, options: &Options) -> Paragraph {
    let mut result = p.clone();
    let mut chars: Vec<(char, usize)> = Vec::new();
    for (index, inline) in p.inlines.iter().enumerate() {
        match inline {
            Inline::Run(run) => {
                for content in &run.content {
                    match content {
                        RunContent::Text(t) => chars.extend(t.text.chars().map(|c| (c, index))),
                        RunContent::Tab => chars.push(('\t', index)),
                        _ => {}
                    }
                }
            }
            Inline::Tab => chars.push(('\t', index)),
            _ => {}
        }
    }
    chars.retain(|(c, _)| {
        !(options.soft_hyphens && *c == '\u{ad}'
            || options.zero_width && matches!(c, '\u{200b}' | '\u{feff}'))
    });
    if options.trailing {
        while chars.last().is_some_and(|(c, _)| matches!(c, ' ' | '\t')) {
            chars.pop();
        }
    }
    let leading = chars.iter().take_while(|(c, _)| *c == ' ').count();
    if leading >= 2 && options.indent != Indent::Keep {
        let run = chars[0].1;
        chars.drain(..leading);
        match options.indent {
            Indent::Tab => chars.insert(0, ('\t', run)),
            Indent::Paragraph => {
                let indent = result
                    .props
                    .indentation
                    .get_or_insert_with(Indentation::default);
                // An existing indent has meaning of its own: do not add another.
                if indent.first_line.is_none()
                    && indent.hanging.is_none()
                    && indent.first_line_chars.is_none()
                    && indent.hanging_chars.is_none()
                {
                    indent.first_line = Some(Twips(
                        (options.indent_cm.clamp(0.1, 10.0) * 566.929).round() as i32,
                    ));
                } else {
                    chars.splice(0..0, std::iter::repeat_n((' ', run), leading));
                }
            }
            Indent::Keep => {}
        }
    }
    let prefix = chars
        .iter()
        .take_while(|(c, _)| matches!(c, ' ' | '\t'))
        .count();
    let mut out: Vec<(char, usize)> = Vec::new();
    for (i, (c, run)) in chars.into_iter().enumerate() {
        if options.punctuation && matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']') {
            while out.last().is_some_and(|(c, _)| *c == ' ') {
                out.pop();
            }
        }
        let repeated = out.last().is_some_and(|(previous, _)| *previous == c);
        if repeated && (options.spaces && c == ' ' && i >= prefix || options.tabs && c == '\t') {
            continue;
        }
        out.push((c, run));
    }
    // Rebuild only changed run content. Properties and paragraph identity stay.
    if out == units(p) {
        return result;
    }
    for (index, inline) in result.inlines.iter_mut().enumerate() {
        let chars: Vec<char> = out
            .iter()
            .filter(|(_, owner)| *owner == index)
            .map(|(c, _)| *c)
            .collect();
        if let Inline::Run(run) = inline {
            let mut content = Vec::new();
            let mut buffer = String::new();
            for c in chars {
                if c == '\t' {
                    if !buffer.is_empty() {
                        content.push(RunContent::Text(TextNode {
                            text: std::mem::take(&mut buffer),
                            space: Space::Preserve,
                        }));
                    }
                    content.push(RunContent::Tab);
                } else {
                    buffer.push(c);
                }
            }
            if !buffer.is_empty() {
                content.push(RunContent::Text(TextNode {
                    text: buffer,
                    space: Space::Preserve,
                }));
            }
            run.content = content;
        }
    }
    // Top-level tab atoms that were collapsed have no run to rebuild.
    result.inlines = result
        .inlines
        .into_iter()
        .enumerate()
        .filter_map(|(i, inline)| {
            if matches!(inline, Inline::Tab) && !out.iter().any(|(_, owner)| *owner == i) {
                None
            } else {
                Some(inline)
            }
        })
        .collect();
    result
}

fn units(p: &Paragraph) -> Vec<(char, usize)> {
    crate::nav::pieces(p)
        .into_iter()
        .flat_map(|piece| {
            let index = piece.inline[0];
            piece.text.chars().map(|c| (c, index)).collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn can_join(a: &Entry, b: &Entry) -> bool {
    let left = text(&a.paragraph);
    let right = text(&b.paragraph);
    let format = |p: &Paragraph| {
        let mut props = p.props.clone();
        // Source offsets identify distinct XML elements, not formatting.
        props.location = None;
        if let Some(mark) = &mut props.run_props {
            mark.location = None;
        }
        props
    };
    a.block + 1 == b.block
        && a.joinable
        && b.joinable
        && format(&a.paragraph) == format(&b.paragraph)
        && left
            .trim_end()
            .chars()
            .last()
            .is_some_and(|c| c.is_alphabetic() || c == ',')
        && right.chars().next().is_some_and(char::is_lowercase)
}

pub(crate) fn build(snapshot: &Snapshot, options: &Options, joins: &[usize]) -> Plan {
    let mut plan = Plan {
        revision: snapshot.revision,
        skipped: snapshot.skipped,
        changes: Vec::new(),
    };
    let mut i = 0;
    let mut previous_empty = None;
    while let Some(entry) = snapshot.entries.get(i) {
        let empty = text(&entry.paragraph).trim().is_empty();
        if empty
            && entry.joinable
            && options.empty
            && previous_empty.is_some()
            && previous_empty == entry.block.checked_sub(1)
        {
            plan.changes.push(Change {
                block: entry.block,
                before: vec![entry.paragraph.clone()],
                after: None,
                key: "cleanup.remove_empty",
                selected: true,
            });
        } else if options.joins
            && joins.contains(&i)
            && snapshot
                .entries
                .get(i + 1)
                .is_some_and(|next| can_join(entry, next))
        {
            let next = &snapshot.entries[i + 1];
            let mut after = clean(&entry.paragraph, options);
            let mut right = clean(&next.paragraph, options);
            // The joining space inherits the preceding character's format.
            if let Some(Inline::Run(run)) = after.inlines.last_mut() {
                run.content.push(RunContent::Text(TextNode {
                    text: " ".into(),
                    space: Space::Preserve,
                }));
            }
            after.inlines.append(&mut right.inlines);
            plan.changes.push(Change {
                block: entry.block,
                before: vec![entry.paragraph.clone(), next.paragraph.clone()],
                after: Some(after),
                key: "cleanup.join",
                selected: false,
            });
            i += 1;
        } else {
            let after = clean(&entry.paragraph, options);
            if after != entry.paragraph {
                plan.changes.push(Change {
                    block: entry.block,
                    before: vec![entry.paragraph.clone()],
                    after: Some(after),
                    key: "cleanup.normalize",
                    selected: true,
                });
            }
        }
        previous_empty = (empty && entry.joinable).then_some(entry.block);
        i += 1;
    }
    plan
}

pub(crate) fn commands(
    document: &Document,
    revision: u64,
    plan: &Plan,
) -> Result<Vec<Edit>, &'static str> {
    if revision != plan.revision {
        return Err("cleanup.stale");
    }
    let mut commands = Vec::new();
    for change in plan.changes.iter().filter(|c| c.selected).rev() {
        for (offset, old) in change.before.iter().enumerate() {
            if document
                .body
                .blocks
                .get(change.block + offset)
                .and_then(Block::as_paragraph)
                != Some(old)
            {
                return Err("cleanup.stale");
            }
        }
        if let Some(after) = &change.after {
            // Keep the first paragraph and its identity. Deleting a body's
            // last paragraph would make the editor insert a required empty one.
            for offset in (1..change.before.len()).rev() {
                commands.push(Edit::Delete {
                    at: Address::body(change.block + offset),
                });
            }
            let at = Address::body(change.block);
            for index in (0..change.before[0].inlines.len()).rev() {
                commands.push(Edit::DeleteInline {
                    at: at.clone(),
                    index,
                });
            }
            for (index, inline) in after.inlines.iter().enumerate() {
                commands.push(Edit::InsertInline {
                    at: at.clone(),
                    index,
                    inline: Box::new(inline.clone()),
                });
            }
            if after.props != change.before[0].props {
                commands.push(Edit::ParagraphProperties {
                    at,
                    properties: Box::new(after.props.clone()),
                });
            }
        } else {
            for offset in (0..change.before.len()).rev() {
                commands.push(Edit::Delete {
                    at: Address::body(change.block + offset),
                });
            }
        }
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use strict_ooxml::model::values::TriState;

    fn paragraph(value: &str) -> Paragraph {
        let loaded = crate::formats::open(
            &crate::formats::Source::New,
            &strict_ooxml::OpenControl::new(),
        )
        .unwrap();
        let mut p = loaded.document.document().body.blocks[0]
            .as_paragraph()
            .unwrap()
            .clone();
        p.inlines = vec![Inline::Run(strict_ooxml::model::Run {
            props: strict_ooxml::model::props::RunProperties::default(),
            content: vec![RunContent::Text(TextNode {
                text: value.into(),
                space: Space::Preserve,
            })],
            revision: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        })];
        p
    }

    #[test]
    fn cleanup_keeps_run_formatting_nonbreaking_spaces_and_disabled_indents() {
        let mut p = paragraph("  A  ");
        let mut bold = paragraph(" B  !\u{a0}Q   ").inlines.remove(0);
        if let Inline::Run(run) = &mut bold {
            run.props.bold = TriState::On;
        }
        p.inlines.push(bold);
        let result = clean(&p, &Options::default());
        assert_eq!(text(&result), "\tA B!\u{a0}Q");
        let Inline::Run(run) = &result.inlines[1] else {
            panic!("run")
        };
        assert_eq!(run.props.bold, TriState::On);
        assert!(crate::nav::pieces(&result)
            .iter()
            .any(|p| p.text.starts_with('B')));
        let keep = Options {
            indent: Indent::Keep,
            ..Options::default()
        };
        assert_eq!(
            text(&clean(&paragraph("    Word  end  "), &keep)),
            "    Word end"
        );
        let unchanged = paragraph("Ordinary text.");
        assert_eq!(clean(&unchanged, &keep), unchanged);
    }

    #[test]
    fn blank_cleanup_keeps_one_paragraph_and_does_not_cross_protected_blocks() {
        let entries = [0, 1, 3, 4]
            .into_iter()
            .map(|block| Entry {
                block,
                paragraph: paragraph(""),
                joinable: true,
            })
            .collect();
        let snapshot = Snapshot {
            revision: 0,
            entries,
            skipped: 1,
        };
        let plan = build(&snapshot, &Options::default(), &[]);
        assert_eq!(
            plan.changes.iter().map(|c| c.block).collect::<Vec<_>>(),
            [1, 4]
        );
        assert!(plan.changes.iter().all(|c| c.after.is_none()));
    }

    #[test]
    fn first_line_indent_is_a_property_and_ai_cannot_join_across_structure() {
        let options = Options {
            indent: Indent::Paragraph,
            indent_cm: 1.25,
            ..Options::default()
        };
        let result = clean(&paragraph("    Text"), &options);
        assert_eq!(text(&result), "Text");
        assert_eq!(
            result.props.indentation.unwrap().first_line,
            Some(Twips(709))
        );
        let a = Entry {
            block: 0,
            paragraph: paragraph("a sentence"),
            joinable: true,
        };
        let mut b = Entry {
            block: 1,
            paragraph: paragraph("continues"),
            joinable: true,
        };
        assert!(can_join(&a, &b));
        b.block = 2;
        assert!(!can_join(&a, &b));
        b.block = 1;
        b.joinable = false;
        assert!(!can_join(&a, &b));
    }

    #[test]
    fn imported_paragraph_source_offsets_do_not_block_a_valid_join() {
        let path =
            std::env::temp_dir().join(format!("mufoed-cleanup-import-{}.rtf", std::process::id()));
        std::fs::write(
            &path,
            br"{\rtf1\ansi\pard Hello.\par This sentence is\par split here.\par}",
        )
        .unwrap();
        let loaded = crate::formats::open(
            &crate::formats::Source::File {
                path: path.clone(),
                import: crate::formats::ImportOptions::default(),
            },
            &strict_ooxml::OpenControl::new(),
        )
        .unwrap();
        let snapshot = snapshot(loaded.document.document(), 0);
        assert!(can_join(&snapshot.entries[1], &snapshot.entries[2]));
        let plan = build(&snapshot, &Options::default(), &[1]);
        assert!(plan
            .changes
            .iter()
            .any(|change| change.key == "cleanup.join"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn snapshot_excludes_objects_and_page_break_paragraphs() {
        let loaded = crate::formats::open(
            &crate::formats::Source::New,
            &strict_ooxml::OpenControl::new(),
        )
        .unwrap();
        let mut document = loaded.document.document().clone();
        let mut page = paragraph(" ");
        page.props.page_break_before = TriState::On;
        let mut math = paragraph("");
        math.inlines
            .push(Inline::Math(crate::formula::from_latex("x").unwrap()));
        document.body.blocks = vec![
            Block::Paragraph(paragraph("Plain")),
            Block::Paragraph(page),
            Block::Paragraph(math),
        ];
        let source = snapshot(&document, 9);
        assert_eq!(source.entries.len(), 1);
        assert_eq!(source.skipped, 2);
        assert_eq!(source.revision, 9);
    }
}
