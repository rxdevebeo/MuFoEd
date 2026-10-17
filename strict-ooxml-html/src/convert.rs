use crate::{images, tree::Tree, HtmlError, HtmlImported, HtmlOptions, HtmlReport, HtmlResolver};
use std::sync::Arc;
use strict_ooxml_core::{error::SourceLocation, part::PartId};
use strict_ooxml_wml::model::props::{NumPr, ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::{
    numbering, props, AbstractNumId, Block, Body, BreakKind, Color, Document, DocumentSource,
    Fonts, GridCol, HalfPoints, Ilvl, Indentation, Inline, Justification, MediaIndex, MediaItem,
    NumId, NumberingTable, Paragraph, Run, RunContent, Space, Table, TableCell, TableRow, TextNode,
    TriState, Twips, Underline, VertAlign,
};

#[derive(Clone)]
struct Style {
    run: RunProperties,
    paragraph: ParagraphProperties,
    pre: bool,
    hidden: bool,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            run: RunProperties {
                fonts: Some(Fonts {
                    ascii: Some("Calibri".into()),
                    h_ansi: Some("Calibri".into()),
                    ..Default::default()
                }),
                size: Some(HalfPoints(22)),
                bold: TriState::Off,
                italic: TriState::Off,
                strike: TriState::Off,
                ..Default::default()
            },
            paragraph: ParagraphProperties::default(),
            pre: false,
            hidden: false,
        }
    }
}
struct Builder<'a> {
    tree: Tree,
    options: &'a HtmlOptions,
    resolver: Option<&'a dyn HtmlResolver>,
    report: HtmlReport,
    media: Vec<(PartId, Vec<u8>)>,
    index: MediaIndex,
    numbering: NumberingTable,
    total: usize,
    image_count: usize,
}

pub(crate) fn convert(
    tree: Tree,
    options: &HtmlOptions,
    resolver: Option<&dyn HtmlResolver>,
) -> Result<HtmlImported, HtmlError> {
    let mut b = Builder {
        tree,
        options,
        resolver,
        report: HtmlReport::default(),
        media: vec![],
        index: MediaIndex::new(),
        numbering: NumberingTable::new(),
        total: 0,
        image_count: 0,
    };
    let children = b.tree.nodes[0].children.clone();
    let mut blocks = b.blocks(&children, &Style::default(), None)?;
    if blocks.is_empty() {
        blocks.push(Block::Paragraph(paragraph(ParagraphProperties::default())));
    }
    let mut document = Document {
        body: Body { blocks },
        styles: strict_ooxml_wml::model::StyleTable::default(),
        numbering: b.numbering,
        footnotes: strict_ooxml_wml::model::NoteTable::default(),
        endnotes: strict_ooxml_wml::model::NoteTable::default(),
        comments: vec![],
        settings: strict_ooxml_wml::model::settings::Settings::default(),
        theme: None,
        font_table: None,
        sections: vec![props::Section {
            properties: props::SectionProperties::default(),
            location: SourceLocation::unknown(),
        }],
        headers_footers: vec![],
        media: b.index,
        support: strict_ooxml_wml::model::SupportModel::default(),
        source: DocumentSource {
            main_document: PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            font_table: None,
            footnotes: None,
            endnotes: None,
            comments: None,
            theme: None,
        },
    };
    strict_ooxml_wml::resolve::refresh_definitions(&mut document);
    Ok(HtmlImported {
        document,
        media: b.media,
        report: b.report,
    })
}
fn paragraph(props: ParagraphProperties) -> Paragraph {
    Paragraph {
        props,
        inlines: vec![],
        rsids: strict_ooxml_wml::model::Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::unknown(),
    }
}
fn text(run: RunProperties, value: String) -> Inline {
    Inline::Run(Run {
        props: run,
        content: vec![RunContent::Text(TextNode {
            text: value,
            space: Space::Preserve,
        })],
        revision: None,
        location: SourceLocation::unknown(),
    })
}
fn flush(out: &mut Vec<Block>, current: &mut Option<Paragraph>) {
    if let Some(p) = current.take() {
        out.push(Block::Paragraph(p));
    }
}
impl Builder<'_> {
    fn blocks(
        &mut self,
        ids: &[usize],
        style: &Style,
        list: Option<NumPr>,
    ) -> Result<Vec<Block>, HtmlError> {
        let mut out = vec![];
        let mut current = None;
        for &id in ids {
            let name = self.tree.nodes[id].name.clone();
            if [
                "script", "style", "head", "iframe", "object", "canvas", "svg", "video", "audio",
            ]
            .contains(&name.as_str())
            {
                self.report
                    .loss(&format!("element:{name}"), "content not imported");
                continue;
            }
            let block = [
                "p",
                "div",
                "li",
                "blockquote",
                "pre",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "section",
                "article",
                "header",
                "footer",
            ]
            .contains(&name.as_str());
            if name == "table" {
                flush(&mut out, &mut current);
                let s = self.style(id, style);
                out.push(Block::Table(self.table(id, &s)?));
            } else if name == "ol" || name == "ul" {
                flush(&mut out, &mut current);
                let s = self.style(id, style);
                let children = self.tree.nodes[id].children.clone();
                let num = self.list(id, name == "ol");
                out.extend(self.blocks(&children, &s, Some(num))?);
            } else if block {
                flush(&mut out, &mut current);
                let s = self.style(id, style);
                if s.hidden {
                    continue;
                }
                let children = self.tree.nodes[id].children.clone();
                let mut nested = self.blocks(&children, &s, list)?;
                if nested.is_empty() {
                    nested.push(Block::Paragraph(paragraph(s.paragraph.clone())));
                }
                if name == "li" {
                    if let Some(Block::Paragraph(p)) = nested.first_mut() {
                        p.props.numbering = list;
                    }
                }
                out.extend(nested);
            } else if name == "html" || name == "body" {
                flush(&mut out, &mut current);
                let s = self.style(id, style);
                let children = self.tree.nodes[id].children.clone();
                out.extend(self.blocks(&children, &s, list)?);
            } else {
                let p = current.get_or_insert_with(|| paragraph(style.paragraph.clone()));
                self.inline(id, style, &mut p.inlines)?;
            }
        }
        if let Some(p) = current {
            if !p.inlines.is_empty() {
                out.push(Block::Paragraph(p));
            }
        }
        Ok(out)
    }
    fn inline(
        &mut self,
        id: usize,
        parent: &Style,
        out: &mut Vec<Inline>,
    ) -> Result<(), HtmlError> {
        let node = &self.tree.nodes[id];
        let name = node.name.clone();
        if name == "#text" {
            let value = if parent.pre {
                node.text.clone()
            } else {
                collapse(&node.text, out)
            };
            if !value.is_empty() {
                let content = if parent.pre {
                    let mut content = vec![];
                    let value = value.replace("\r\n", "\n").replace('\r', "\n");
                    for (i, line) in value.split('\n').enumerate() {
                        if i > 0 {
                            content.push(RunContent::Break(BreakKind::TextWrapping));
                        }
                        for (j, segment) in line.split('\t').enumerate() {
                            if j > 0 {
                                content.push(RunContent::Tab);
                            }
                            content.push(RunContent::Text(TextNode {
                                text: segment.into(),
                                space: Space::Preserve,
                            }));
                        }
                    }
                    Some(content)
                } else {
                    None
                };
                if let Some(content) = content {
                    out.push(Inline::Run(Run {
                        props: parent.run.clone(),
                        content,
                        revision: None,
                        location: SourceLocation::unknown(),
                    }));
                } else {
                    out.push(text(parent.run.clone(), value));
                }
            }
            return Ok(());
        }
        let style = self.style(id, parent);
        if style.hidden {
            return Ok(());
        }
        if ["script", "style", "iframe", "object", "svg", "canvas"].contains(&name.as_str()) {
            self.report
                .loss(&format!("element:{name}"), "content not imported");
            return Ok(());
        }
        if name == "img" {
            self.image(id, out)?;
            return Ok(());
        }
        if name == "br" {
            out.push(Inline::Run(Run {
                props: style.run,
                content: vec![RunContent::Break(BreakKind::TextWrapping)],
                revision: None,
                location: SourceLocation::unknown(),
            }));
            return Ok(());
        }
        if name == "a" && self.tree.nodes[id].attr("href").is_some() {
            self.report.loss(
                "hyperlink-target",
                "link text retained; external target not represented",
            );
        }
        if ![
            "span", "b", "strong", "i", "em", "u", "s", "strike", "del", "sup", "sub", "font",
            "code", "a", "small", "big", "label", "mark",
        ]
        .contains(&name.as_str())
        {
            self.report
                .loss(&format!("element:{name}"), "child text retained");
        }
        let children = self.tree.nodes[id].children.clone();
        for child in children {
            self.inline(child, &style, out)?;
        }
        Ok(())
    }
    fn style(&mut self, id: usize, parent: &Style) -> Style {
        let mut s = parent.clone();
        let n = &self.tree.nodes[id];
        match n.name.as_str() {
            "b" | "strong" | "th" => s.run.bold = TriState::On,
            "i" | "em" => s.run.italic = TriState::On,
            "u" => s.run.underline = Some(Underline::Single),
            "s" | "strike" | "del" => s.run.strike = TriState::On,
            "sub" => s.run.vert_align = Some(VertAlign::Subscript),
            "sup" => s.run.vert_align = Some(VertAlign::Superscript),
            "pre" | "code" => {
                s.pre = n.name == "pre";
                s.run.fonts = Some(Fonts {
                    ascii: Some("Consolas".into()),
                    h_ansi: Some("Consolas".into()),
                    ..Default::default()
                });
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = n.name[1..].parse::<i32>().unwrap_or(3);
                s.run.bold = TriState::On;
                s.run.size = Some(HalfPoints((36 - level * 3).max(22)));
            }
            _ => {}
        }
        if let Some(face) = n.attr("face") {
            set_font(&mut s.run, face);
        }
        if let Some(c) = n.attr("color") {
            s.run.color = parse_color(c).map(Color::new);
        }
        if let Some(a) = n.attr("align") {
            s.paragraph.alignment = Some(align(a));
        }
        if n.attr("class").is_some() {
            self.report
                .loss("css:class", "class stylesheet not evaluated");
        }
        if let Some(css) = n.attr("style") {
            for (key, value) in css.split(';').filter_map(|p| p.split_once(':')) {
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim();
                let supported = match key.as_str() {
                    "font-family" => {
                        set_font(&mut s.run, value);
                        true
                    }
                    "font-size" => {
                        if let Some(p) = points(value) {
                            s.run.size =
                                Some(HalfPoints((p * 2.0).round().clamp(1.0, 3276.0) as i32));
                            true
                        } else {
                            false
                        }
                    }
                    "font-weight" => {
                        s.run.bold =
                            if value == "bold" || value.parse::<u32>().is_ok_and(|v| v >= 600) {
                                TriState::On
                            } else {
                                TriState::Off
                            };
                        true
                    }
                    "font-style" => {
                        s.run.italic = if value == "italic" {
                            TriState::On
                        } else {
                            TriState::Off
                        };
                        true
                    }
                    "color" => {
                        if let Some(color) = parse_color(value) {
                            s.run.color = Some(Color::new(color));
                            true
                        } else {
                            false
                        }
                    }
                    "text-align" => {
                        s.paragraph.alignment = Some(align(value));
                        true
                    }
                    "text-decoration" => {
                        if value.contains("underline") {
                            s.run.underline = Some(Underline::Single);
                        }
                        if value.contains("line-through") {
                            s.run.strike = TriState::On;
                        }
                        true
                    }
                    "white-space" => {
                        s.pre = value.starts_with("pre");
                        true
                    }
                    "display" if value == "none" => {
                        s.hidden = true;
                        true
                    }
                    "margin-left" | "margin-right" | "text-indent" => {
                        if let Some(pt) = points(value) {
                            let indent =
                                s.paragraph.indentation.get_or_insert_with(Default::default);
                            let v =
                                Some(Twips((pt * 20.0).round().clamp(-31680.0, 31680.0) as i32));
                            match key.as_str() {
                                "margin-left" => indent.start = v,
                                "margin-right" => indent.end = v,
                                _ => indent.first_line = v,
                            }
                            true
                        } else {
                            false
                        }
                    }
                    "width" | "height" if n.name == "img" => true,
                    _ => false,
                };
                if !supported {
                    self.report.loss(&format!("css:{key}"), value);
                }
            }
        }
        s
    }
    fn list(&mut self, id: usize, ordered: bool) -> NumPr {
        let n = self.numbering.nums().count() as u32 + 1;
        let abstract_id = AbstractNumId(n);
        let num = NumId(n);
        let level = numbering::Level {
            ilvl: Ilvl(0),
            start: Some(
                self.tree.nodes[id]
                    .attr("start")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
            ),
            format: Some(if ordered { "decimal" } else { "bullet" }.into()),
            text: Some(if ordered { "%1." } else { "•" }.into()),
            paragraph: ParagraphProperties {
                indentation: Some(Indentation {
                    start: Some(Twips(720)),
                    hanging: Some(Twips(360)),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        self.numbering.insert_abstract(numbering::AbstractNum {
            id: abstract_id,
            name: None,
            multi_level_type: Some("singleLevel".into()),
            num_style_link: None,
            style_link: None,
            levels: vec![level],
            location: SourceLocation::unknown(),
        });
        self.numbering.insert_num(numbering::Num {
            num_id: num,
            abstract_num_id: abstract_id,
            overrides: vec![],
            location: SourceLocation::unknown(),
        });
        NumPr {
            num_id: Some(num),
            ilvl: Some(Ilvl(0)),
        }
    }
    fn table(&mut self, id: usize, style: &Style) -> Result<Table, HtmlError> {
        let mut row_ids = vec![];
        collect(&self.tree, id, "tr", &mut row_ids);
        let mut rows = vec![];
        let mut columns = 0;
        for row in row_ids {
            let cell_ids: Vec<_> = self.tree.nodes[row]
                .children
                .iter()
                .copied()
                .filter(|i| ["td", "th"].contains(&self.tree.nodes[*i].name.as_str()))
                .collect();
            let mut cells = vec![];
            for cell in cell_ids {
                let s = self.style(cell, style);
                let children = self.tree.nodes[cell].children.clone();
                let mut blocks = self.blocks(&children, &s, None)?;
                if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
                    blocks.push(Block::Paragraph(paragraph(s.paragraph.clone())));
                }
                let span = self.tree.nodes[cell]
                    .attr("colspan")
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                if self.tree.nodes[cell]
                    .attr("rowspan")
                    .is_some_and(|v| v != "1")
                {
                    self.report
                        .loss("table:rowspan", "vertical merging not represented");
                }
                cells.push(TableCell {
                    props: props::CellProperties {
                        grid_span: Some(span as u16),
                        ..Default::default()
                    },
                    blocks,
                    sdt: None,
                    location: SourceLocation::unknown(),
                });
            }
            let count = cells
                .iter()
                .map(|c| c.props.grid_span.unwrap_or(1) as usize)
                .sum::<usize>();
            columns = columns.max(count);
            rows.push(TableRow {
                props: props::RowProperties::default(),
                cells,
                sdt: None,
                location: SourceLocation::unknown(),
            });
        }
        Ok(Table {
            props: props::TableProperties::default(),
            grid: vec![
                GridCol {
                    width: Some(Twips(
                        9000 / i32::try_from(columns.max(1)).unwrap_or(i32::MAX)
                    ))
                };
                columns
            ],
            grid_change: None,
            rows,
            location: SourceLocation::unknown(),
        })
    }
    fn image(&mut self, id: usize, out: &mut Vec<Inline>) -> Result<(), HtmlError> {
        if self.image_count >= self.options.max_images {
            return Err(HtmlError::Limit("images"));
        }
        self.image_count += 1;
        let n = &self.tree.nodes[id];
        let src = n.attr("src").unwrap_or("").to_owned();
        let alt = n.attr("alt").unwrap_or("[image]").to_owned();
        let loaded = images::load(
            &src,
            self.options,
            self.resolver,
            self.options.max_media_bytes.saturating_sub(self.total),
        );
        let bytes = match loaded {
            Ok(bytes) => bytes,
            Err(error) => {
                self.report.loss("image:unavailable", error);
                out.push(text(RunProperties::default(), alt));
                return Ok(());
            }
        };
        let Some((kind, mime, extension, pw, ph)) = images::format(&bytes) else {
            self.report
                .loss("image:format", "unsupported or invalid image");
            out.push(text(RunProperties::default(), alt));
            return Ok(());
        };
        if u64::from(pw) * u64::from(ph) > self.options.max_image_pixels {
            return Err(HtmlError::Limit("image pixels"));
        }
        let dimension = |name: &str| {
            n.attr("style")
                .and_then(|s| {
                    s.split(';')
                        .filter_map(|v| v.split_once(':'))
                        .find(|(k, _)| k.trim() == name)
                        .and_then(|(_, v)| pixels(v.trim()))
                })
                .or_else(|| n.attr(name).and_then(pixels))
        };
        let (w, h) = match (dimension("width"), dimension("height")) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * f64::from(ph) / f64::from(pw)),
            (None, Some(h)) => (h * f64::from(pw) / f64::from(ph), h),
            _ => (f64::from(pw), f64::from(ph)),
        };
        let number = self.media.len() as u32 + 1;
        let part = PartId::new(format!("/word/media/html{number}.{extension}"));
        self.total += bytes.len();
        self.media.push((part.clone(), bytes));
        self.index.insert(MediaItem {
            part: part.clone(),
            content_type: Some(mime.into()),
            kind,
        });
        out.push(Inline::Drawing(images::drawing(
            part,
            (w * 9525.0).round().clamp(1.0, 20_000_000.0) as i64,
            (h * 9525.0).round().clamp(1.0, 20_000_000.0) as i64,
            number,
            &alt,
        )));
        Ok(())
    }
}
fn collect(tree: &Tree, id: usize, name: &str, out: &mut Vec<usize>) {
    for &child in &tree.nodes[id].children {
        if tree.nodes[child].name == name {
            out.push(child);
        } else if tree.nodes[child].name != "table" {
            collect(tree, child, name, out);
        }
    }
}
fn collapse(value: &str, out: &[Inline]) -> String {
    let mut text = String::new();
    let mut space=out.last().is_some_and(|i|matches!(i,Inline::Run(r) if matches!(r.content.last(),Some(RunContent::Text(t)) if t.text.ends_with(' '))));
    for ch in value.chars() {
        if ch.is_whitespace() && ch != '\u{a0}' {
            if !space && (!text.is_empty() || !out.is_empty()) {
                text.push(' ');
            }
            space = true;
        } else {
            text.push(ch);
            space = false;
        }
    }
    text
}
fn set_font(props: &mut RunProperties, name: &str) {
    let face: Arc<str> = name
        .split(',')
        .next()
        .unwrap_or(name)
        .trim()
        .trim_matches(['\'', '"'])
        .into();
    props.fonts = Some(Fonts {
        ascii: Some(face.clone()),
        h_ansi: Some(face),
        ..Default::default()
    });
}
fn align(value: &str) -> Justification {
    match value.trim() {
        "center" => Justification::Center,
        "right" | "end" => Justification::End,
        "justify" => Justification::Both,
        _ => Justification::Start,
    }
}
fn points(value: &str) -> Option<f64> {
    let (value, factor) = if let Some(v) = value.strip_suffix("pt") {
        (v, 1.0)
    } else if let Some(v) = value.strip_suffix("px") {
        (v, 0.75)
    } else if let Some(v) = value.strip_suffix("cm") {
        (v, 72.0 / 2.54)
    } else {
        return None;
    };
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v * factor)
}
fn pixels(value: &str) -> Option<f64> {
    value
        .trim()
        .trim_end_matches("px")
        .parse::<f64>()
        .ok()
        .or_else(|| points(value).map(|p| p / 0.75))
        .filter(|v| v.is_finite() && *v > 0.0)
}
fn parse_color(value: &str) -> Option<String> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hex.into());
        }
        if hex.len() == 3 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hex.chars().flat_map(|c| [c, c]).collect());
        }
    }
    if let Some(rgb) = v.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let n: Vec<u8> = rgb
            .split(',')
            .map(|v| v.trim().parse())
            .collect::<Result<_, _>>()
            .ok()?;
        if n.len() == 3 {
            return Some(format!("{:02X}{:02X}{:02X}", n[0], n[1], n[2]));
        }
    }
    match v {
        "black" => Some("000000".into()),
        "white" => Some("FFFFFF".into()),
        "red" => Some("FF0000".into()),
        "blue" => Some("0000FF".into()),
        "green" => Some("008000".into()),
        _ => None,
    }
}
