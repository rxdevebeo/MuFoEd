//! CommonMark/GFM preview with theme-aware inline formatting and local images.
#![allow(clippy::cast_precision_loss)] // Indentation is bounded by the parsed nesting depth.
use egui::{text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // Text styles can be combined independently.
struct Run {
    text: String,
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
    link: Option<String>,
}
type Line = Vec<Run>;
enum Block {
    Text(Line, u8, usize, usize),
    Code(String),
    Rule,
    Table(Vec<Vec<Line>>),
    Image(String, String),
}
#[derive(Default)]
struct Build {
    runs: Line,
    blocks: Vec<Block>,
    bold: usize,
    italic: usize,
    strike: usize,
    heading: u8,
    quote: usize,
    lists: Vec<Option<u64>>,
    links: Vec<String>,
    code: Option<String>,
    image: Option<(String, String)>,
    table: Option<Vec<Vec<Line>>>,
    cells: Vec<Line>,
}
impl Build {
    fn text(&mut self, text: &str, code: bool) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if let Some(buffer) = &mut self.code {
            buffer.push_str(text);
            return;
        }
        self.runs.push(Run {
            text: text.into(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
            link: self.links.last().cloned(),
        });
    }
    fn flush(&mut self) {
        if !self.runs.is_empty() {
            self.blocks.push(Block::Text(
                std::mem::take(&mut self.runs),
                self.heading,
                self.lists.len(),
                self.quote,
            ));
        }
    }
}
struct Image {
    loading: Option<crate::preview::Loading>,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
}
pub(super) struct Markdown {
    blocks: Vec<Block>,
    base: PathBuf,
    images: HashMap<PathBuf, Image>,
    anchor: Option<String>,
}
impl Markdown {
    #[allow(clippy::too_many_lines)] // One exhaustive parser event dispatcher.
    pub(super) fn new(text: &str, source: &Path) -> Self {
        let mut b = Build::default();
        for event in Parser::new_ext(
            text,
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
        ) {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        b.flush();
                        b.heading = level as u8;
                    }
                    Tag::Strong => b.bold += 1,
                    Tag::Emphasis => b.italic += 1,
                    Tag::Strikethrough => b.strike += 1,
                    Tag::CodeBlock(_) => {
                        b.flush();
                        b.code = Some(String::new());
                    }
                    Tag::BlockQuote(_) => {
                        b.flush();
                        b.quote += 1;
                    }
                    Tag::List(start) => {
                        b.flush();
                        b.lists.push(start);
                    }
                    Tag::Item => {
                        b.flush();
                        let prefix = match b.lists.last_mut() {
                            Some(Some(number)) => {
                                let prefix = format!("{number}. ");
                                *number += 1;
                                prefix
                            }
                            _ => "• ".into(),
                        };
                        b.text(&prefix, false);
                    }
                    Tag::Link { dest_url, .. } => b.links.push(dest_url.into_string()),
                    Tag::Image { dest_url, .. } => {
                        b.flush();
                        b.image = Some((dest_url.into_string(), String::new()));
                    }
                    Tag::Table(_) => {
                        b.flush();
                        b.table = Some(Vec::new());
                    }
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Paragraph | TagEnd::Item => b.flush(),
                    TagEnd::Heading(_) => {
                        b.flush();
                        b.heading = 0;
                    }
                    TagEnd::Strong => b.bold = b.bold.saturating_sub(1),
                    TagEnd::Emphasis => b.italic = b.italic.saturating_sub(1),
                    TagEnd::Strikethrough => b.strike = b.strike.saturating_sub(1),
                    TagEnd::CodeBlock => {
                        if let Some(code) = b.code.take() {
                            b.blocks.push(Block::Code(code));
                        }
                    }
                    TagEnd::BlockQuote(_) => {
                        b.flush();
                        b.quote = b.quote.saturating_sub(1);
                    }
                    TagEnd::List(_) => {
                        b.flush();
                        b.lists.pop();
                    }
                    TagEnd::Link => {
                        b.links.pop();
                    }
                    TagEnd::Image => {
                        if let Some((path, alt)) = b.image.take() {
                            b.blocks.push(Block::Image(path, alt));
                        }
                    }
                    TagEnd::TableCell => b.cells.push(std::mem::take(&mut b.runs)),
                    TagEnd::TableRow | TagEnd::TableHead => {
                        if let Some(table) = &mut b.table {
                            table.push(std::mem::take(&mut b.cells));
                        }
                    }
                    TagEnd::Table => {
                        if let Some(table) = b.table.take() {
                            b.blocks.push(Block::Table(table));
                        }
                    }
                    _ => {}
                },
                Event::Text(text) => b.text(&text, false),
                Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                    b.text(&text, true);
                }
                Event::SoftBreak => b.text(" ", false),
                Event::HardBreak => b.text("\n", false),
                Event::Rule => {
                    b.flush();
                    b.blocks.push(Block::Rule);
                }
                Event::TaskListMarker(done) => b.text(if done { "☑ " } else { "☐ " }, false),
                _ => {}
            }
        }
        b.flush();
        Self {
            blocks: b.blocks,
            base: source.parent().unwrap_or_else(|| Path::new(".")).into(),
            images: HashMap::new(),
            anchor: None,
        }
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        let mut clicked = None;
        let mut found_anchor = false;
        egui::ScrollArea::both()
            .id_salt("markdown-preview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, block) in self.blocks.iter().enumerate() {
                    ui.push_id(index, |ui| match block {
                        Block::Text(runs, heading, indent, quote) => {
                            let before = ui.cursor().min;
                            if *heading > 0
                                && self.anchor.as_ref().is_some_and(|a| *a == anchor_id(runs))
                            {
                                ui.scroll_to_rect(
                                    egui::Rect::from_min_size(before, egui::vec2(1.0, 1.0)),
                                    Some(egui::Align::Min),
                                );
                                found_anchor = true;
                            }
                            ui.horizontal_top(|ui| {
                                ui.add_space((*indent + *quote) as f32 * 12.0);
                                if let Some(link) = draw_line(ui, runs, *heading, false) {
                                    clicked = Some(link);
                                }
                            });
                            if *quote > 0 {
                                ui.painter().line_segment(
                                    [before, egui::pos2(before.x, ui.cursor().min.y - 2.0)],
                                    Stroke::new(3.0, ui.visuals().weak_text_color()),
                                );
                            }
                            ui.add_space(if *heading > 0 { 8.0 } else { 4.0 });
                        }
                        Block::Code(code) => {
                            egui::Frame::new()
                                .fill(ui.visuals().code_bg_color)
                                .inner_margin(8)
                                .corner_radius(3)
                                .show(ui, |ui| {
                                    ui.monospace(code);
                                });
                            ui.add_space(6.0);
                        }
                        Block::Rule => {
                            ui.separator();
                        }
                        Block::Table(rows) => {
                            let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                            let width =
                                (ui.available_width() / columns as f32 - 18.0).clamp(120.0, 360.0);
                            egui::Grid::new("markdown-table")
                                .min_col_width(width)
                                .max_col_width(width)
                                .striped(true)
                                .spacing([18.0, 6.0])
                                .show(ui, |ui| {
                                    for (index, row) in rows.iter().enumerate() {
                                        for cell in row {
                                            if let Some(link) = draw_line(ui, cell, 0, index == 0) {
                                                clicked = Some(link);
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });
                            ui.add_space(8.0);
                        }
                        Block::Image(destination, alt) => {
                            draw_image(ui, &self.base, &mut self.images, destination, alt);
                        }
                    });
                }
            });
        if found_anchor {
            self.anchor = None;
        }
        if let Some(link) = clicked {
            if let Some(anchor) = link.strip_prefix('#') {
                self.anchor = Some(anchor.to_owned());
                ui.ctx().request_repaint();
            } else if let Ok(destination) = url::Url::parse(&link) {
                if ["https", "http", "mailto"].contains(&destination.scheme()) {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(destination.to_string()));
                }
            } else {
                let (path, fragment) = link
                    .split_once('#')
                    .map_or((link.as_str(), None), |(p, f)| (p, Some(f)));
                if let Ok(mut destination) = url::Url::from_file_path(self.base.join(path)) {
                    destination.set_fragment(fragment);
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(destination.to_string()));
                }
            }
        }
    }
}
fn draw_image(
    ui: &mut egui::Ui,
    base: &Path,
    images: &mut HashMap<PathBuf, Image>,
    destination: &str,
    alt: &str,
) {
    if destination.contains("://") || destination.starts_with("data:") {
        ui.hyperlink_to(if alt.is_empty() { destination } else { alt }, destination);
        return;
    }
    let path = base.join(destination);
    let slot = images.entry(path.clone()).or_insert_with(|| {
        let ctx = ui.ctx().clone();
        Image {
            loading: Some(crate::preview::Loading::start(&path, 1280, move || {
                ctx.request_repaint();
            })),
            texture: None,
            error: None,
        }
    });
    if let Some(content) = slot
        .loading
        .as_ref()
        .and_then(crate::preview::Loading::poll)
    {
        slot.loading = None;
        match content {
            crate::preview::Content::Picture { size, rgba, .. } => {
                slot.texture = Some(ui.ctx().load_texture(
                    "markdown-image",
                    egui::ColorImage::from_rgba_unmultiplied(size, &rgba),
                    egui::TextureOptions::LINEAR,
                ));
            }
            crate::preview::Content::Failed(error) => {
                slot.error = Some(error);
            }
            _ => slot.error = Some(alt.to_owned()),
        }
    }
    if let Some(texture) = &slot.texture {
        ui.add(egui::Image::new(texture).max_width(ui.available_width()));
    } else if let Some(error) = &slot.error {
        ui.weak(format!("{alt}: {error}"));
    } else {
        ui.spinner();
    }
}

fn anchor_id(runs: &[Run]) -> String {
    let title: String = runs.iter().map(|r| r.text.as_str()).collect();
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || c.is_whitespace())
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect()
}
fn draw_line(ui: &mut egui::Ui, runs: &[Run], heading: u8, strong: bool) -> Option<String> {
    let mut job = LayoutJob::default();
    let mut links = Vec::new();
    let mut characters = 0;
    let bold = FontFamily::Name("bold".into());
    let has_bold = ui.ctx().fonts(|fonts| fonts.families().contains(&bold));
    for run in runs {
        let family = if run.code {
            FontFamily::Monospace
        } else if (run.bold || strong || heading > 0) && has_bold {
            bold.clone()
        } else {
            FontFamily::Proportional
        };
        let size = match heading {
            1 => 28.0,
            2 => 23.0,
            3 => 19.0,
            4 => 17.0,
            _ => 14.0,
        };
        let colour = if run.link.is_some() {
            ui.visuals().hyperlink_color
        } else if run.bold || strong {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().text_color()
        };
        let format = TextFormat {
            font_id: FontId::new(size, family),
            color: colour,
            italics: run.italic,
            strikethrough: if run.strike {
                Stroke::new(1.0, colour)
            } else {
                Stroke::NONE
            },
            background: if run.code {
                ui.visuals().code_bg_color
            } else {
                Color32::TRANSPARENT
            },
            ..Default::default()
        };
        let length = run.text.chars().count();
        if let Some(link) = &run.link {
            links.push((characters..characters + length, link));
        }
        characters += length;
        job.append(&run.text, 0.0, format);
    }
    let label = egui::Label::new(job).sense(egui::Sense::click()).wrap();
    let (position, galley, response) = label.layout_in_ui(ui);
    ui.painter()
        .galley(position, galley.clone(), ui.visuals().text_color());
    if response.hovered() {
        if let Some(pointer) = response.hover_pos() {
            let cursor = galley.cursor_from_pos(pointer - position);
            if let Some((_, link)) = links
                .iter()
                .find(|(range, _)| range.contains(&cursor.index.0))
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    return Some((*link).clone());
                }
                response.on_hover_text(*link);
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_cells_keep_readable_width() {
        let mut document = Markdown::new(
            "| Format | Description |\n|---|---|\n| Markdown | Headings and lists |\n",
            Path::new("README.md"),
        );
        let ctx = egui::Context::default();
        let mut seen = false;
        for _ in 0..3 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| document.show(ui),
            );
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape {
                    if text.galley.text() == "Markdown" {
                        assert_eq!(text.galley.rows.len(), 1);
                        seen = true;
                    }
                }
            }
        }
        assert!(seen);
    }

    #[test]
    fn parses_gfm_tables_inline_styles_lists_and_fenced_code() {
        let source = "# Title\n\n**bold** and *italic* and `code`\n\n- [x] Done\n- Next\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nlet x = 1;\n```\n";
        let document = Markdown::new(source, Path::new("README.md"));
        assert!(document
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Text(_, 1, _, _))));
        assert!(document
            .blocks
            .iter()
            .any(|b| matches!(b,Block::Table(rows) if rows.len()==2 && rows[0].len()==2)));
        assert!(document
            .blocks
            .iter()
            .any(|b| matches!(b,Block::Code(code) if code.contains("let x = 1;"))));
        assert!(document
            .blocks
            .iter()
            .any(|b| matches!(b,Block::Text(runs,_,_,_) if runs.iter().any(|r|r.bold))));
    }
}
