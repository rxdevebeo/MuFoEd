#![allow(
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::default_trait_access
)]
use crate::picture::PictureBuilder;
use crate::{
    Document, InvalidTextPolicy, PartId, RtfError, RtfImported, RtfIssue, RtfOptions, RtfReport,
    RtfSpan, Severity, UnsupportedPolicy,
};
use std::{collections::BTreeMap, sync::Arc};
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::document::{Body, DocumentSource};
use strict_ooxml_wml::model::drawing::{MediaIndex, MediaKind};
use strict_ooxml_wml::model::{
    Block, BreakKind, CellProperties, Color, Fonts, GridCol, HalfPoints, Inline, Justification,
    PageOrientation, Paragraph, ParagraphProperties, RowProperties, Rsids, Run, RunContent,
    RunProperties, Section, SectionProperties, Space, Table, TableCell, TableProperties, TableRow,
    TextNode, TriState, Twips, Underline, VertAlign, VerticalJc, VerticalMerge, Width, WidthKind,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination {
    Body,
    Fonts,
    Colors,
    Picture,
}
struct Font {
    name: String,
    charset: i32,
    codepage: Option<i32>,
}
#[derive(Default, Clone)]
struct RowBuilder {
    left: i32,
    ends: Vec<i32>,
    declarations: Vec<CellProperties>,
    next_props: CellProperties,
    cells: Vec<TableCell>,
    blocks: Vec<Block>,
    props: RowProperties,
}

#[derive(Debug)]
enum Token<'a> {
    Open,
    Close,
    Bytes(&'a [u8]),
    Byte(u8),
    Symbol(u8),
    Word(&'a str, Option<i32>),
    Binary(&'a [u8]),
}
struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Option<(RtfSpan, Token<'a>)>, RtfError> {
        if self.pos == self.bytes.len() {
            return Ok(None);
        }
        let start = self.pos;
        let byte = self.bytes[self.pos];
        self.pos += 1;
        let token = match byte {
            b'{' => Token::Open,
            b'}' => Token::Close,
            b'\\' => {
                let Some(&next) = self.bytes.get(self.pos) else {
                    return malformed(start, "truncated escape");
                };
                self.pos += 1;
                if next == b'\'' {
                    let Some(hex) = self.bytes.get(self.pos..self.pos + 2) else {
                        return malformed(start, "truncated hex escape");
                    };
                    let digit = |x: u8| (x as char).to_digit(16);
                    let (Some(a), Some(b)) = (digit(hex[0]), digit(hex[1])) else {
                        return malformed(start, "invalid hex escape");
                    };
                    self.pos += 2;
                    Token::Byte(u8::try_from(a * 16 + b).expect("two hex digits"))
                } else if next.is_ascii_alphabetic() {
                    let name_start = self.pos - 1;
                    while self
                        .bytes
                        .get(self.pos)
                        .is_some_and(u8::is_ascii_alphabetic)
                    {
                        self.pos += 1;
                    }
                    if self.pos - name_start > 32 {
                        return malformed(start, "control word exceeds 32 letters");
                    }
                    let name = std::str::from_utf8(&self.bytes[name_start..self.pos])
                        .expect("ASCII letters");
                    let parameter_start = self.pos;
                    if self.bytes.get(self.pos) == Some(&b'-') {
                        self.pos += 1;
                    }
                    let digits_start = self.pos;
                    while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                        self.pos += 1;
                    }
                    let parameter = if self.pos == digits_start {
                        if digits_start != parameter_start {
                            self.pos = parameter_start;
                        }
                        None
                    } else {
                        let value = std::str::from_utf8(&self.bytes[parameter_start..self.pos])
                            .expect("ASCII digits")
                            .parse::<i32>()
                            .map_err(|_| RtfError::Malformed {
                                offset: start,
                                reason: "parameter outside signed 32-bit range",
                            })?;
                        Some(value)
                    };
                    if self.bytes.get(self.pos) == Some(&b' ') {
                        self.pos += 1;
                    }
                    if name == "bin" {
                        let Some(n) = parameter.and_then(|n| usize::try_from(n).ok()) else {
                            return malformed(start, "invalid binary length");
                        };
                        let end = self
                            .pos
                            .checked_add(n)
                            .filter(|end| *end <= self.bytes.len())
                            .ok_or(RtfError::Malformed {
                                offset: start,
                                reason: "truncated binary payload",
                            })?;
                        let payload = &self.bytes[self.pos..end];
                        self.pos = end;
                        Token::Binary(payload)
                    } else {
                        Token::Word(name, parameter)
                    }
                } else {
                    Token::Symbol(next)
                }
            }
            _ => {
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|b| !matches!(b, b'{' | b'}' | b'\\'))
                {
                    self.pos += 1;
                }
                Token::Bytes(&self.bytes[start..self.pos])
            }
        };
        Ok(Some((
            RtfSpan {
                start,
                end: self.pos,
            },
            token,
        )))
    }
}
fn malformed<T>(offset: usize, reason: &'static str) -> Result<T, RtfError> {
    Err(RtfError::Malformed { offset, reason })
}
fn bounded(
    observed: usize,
    maximum: usize,
    offset: usize,
    limit: &'static str,
) -> Result<(), RtfError> {
    if observed > maximum {
        Err(RtfError::LimitExceeded {
            offset,
            limit,
            observed,
        })
    } else {
        Ok(())
    }
}

#[derive(Clone)]
struct State {
    run: RunProperties,
    paragraph: ParagraphProperties,
    codepage: i32,
    uc: usize,
    skip: bool,
    first: bool,
    ignorable: bool,
    destination: Destination,
    upr_children: Option<u8>,
    allow_ud: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            run: RunProperties::default(),
            paragraph: ParagraphProperties::default(),
            codepage: 1252,
            uc: 1,
            skip: false,
            first: true,
            ignorable: false,
            destination: Destination::Body,
            upr_children: None,
            allow_ud: false,
        }
    }
}
struct Parser<'a> {
    options: &'a RtfOptions,
    report: RtfReport,
    state: State,
    stack: Vec<State>,
    blocks: Vec<Block>,
    inlines: Vec<Inline>,
    paragraph_props: ParagraphProperties,
    bytes: Vec<u8>,
    bytes_offset: usize,
    fallback: usize,
    high: Option<(u16, usize)>,
    fonts: BTreeMap<i32, Font>,
    font_id: Option<i32>,
    font_name: String,
    font_charset: i32,
    font_codepage: Option<i32>,
    default_font: Option<i32>,
    document_codepage: i32,
    colors: Vec<Option<Color>>,
    rgb: [u8; 3],
    color_set: bool,
    section: SectionProperties,
    row: Option<RowBuilder>,
    table: Option<Table>,
    table_rows: usize,
    table_cells: usize,
    row_template: Option<RowBuilder>,
    picture: Option<PictureBuilder>,
    media: Vec<(PartId, Vec<u8>)>,
    media_index: MediaIndex,
    media_bytes: usize,
}
impl Parser<'_> {
    fn finish_picture(&mut self, span: RtfSpan) -> Result<(), RtfError> {
        let Some(picture) = self.picture.take() else {
            return malformed(span.start, "missing picture collector");
        };
        if self.state.skip {
            return Ok(());
        }
        if picture.width_goal.is_none() || picture.height_goal.is_none() {
            self.issue(span, Severity::Loss, "picture:assumed-96dpi-extent")?;
        }
        let (drawing, item, bytes) = picture.build(self.media.len() + 1, span.start)?;
        self.media_bytes += bytes.len();
        self.media.push((item.part.clone(), bytes));
        self.media_index.insert(item);
        self.content(RunContent::Drawing(drawing), span.start)
    }
    fn finish_table(&mut self) {
        if let Some(table) = self.table.take() {
            self.blocks.push(Block::Table(table));
        }
    }
    fn cell(&mut self, offset: usize) -> Result<(), RtfError> {
        if self.row.is_none() {
            return malformed(offset, "cell outside a table row");
        }
        if !self.inlines.is_empty() || self.row.as_ref().is_some_and(|r| r.blocks.is_empty()) {
            self.paragraph(offset)?;
        }
        self.table_cells += 1;
        bounded(
            self.table_cells,
            self.options.limits.table_cells,
            offset,
            "table_cells",
        )?;
        self.node(1, offset)?;
        let row = self.row.as_mut().expect("checked row");
        let index = row.cells.len();
        let props = row
            .declarations
            .get(index)
            .cloned()
            .ok_or(RtfError::Malformed {
                offset,
                reason: "cell has no cellx declaration",
            })?;
        row.cells.push(TableCell {
            props,
            blocks: std::mem::take(&mut row.blocks),
            sdt: None,
            location: SourceLocation::unknown(),
        });
        Ok(())
    }
    fn finish_row(&mut self, span: RtfSpan) -> Result<(), RtfError> {
        if !self.inlines.is_empty() || self.row.as_ref().is_some_and(|r| !r.blocks.is_empty()) {
            return malformed(span.start, "unterminated table cell");
        }
        let row = self.row.take().ok_or(RtfError::Malformed {
            offset: span.start,
            reason: "row outside a table",
        })?;
        self.row_template = Some(RowBuilder {
            left: row.left,
            ends: row.ends.clone(),
            declarations: row.declarations.clone(),
            props: row.props.clone(),
            ..RowBuilder::default()
        });
        if row.cells.len() != row.declarations.len() || row.cells.is_empty() {
            return malformed(span.start, "table cell count differs from declarations");
        }
        self.table_rows += 1;
        bounded(
            self.table_rows,
            self.options.limits.table_rows,
            span.start,
            "table_rows",
        )?;
        self.node(1, span.start)?;
        let mut previous = row.left;
        let grid: Vec<_> = row
            .ends
            .iter()
            .map(|end| {
                let width = *end - previous;
                previous = *end;
                GridCol {
                    width: Some(Twips(width)),
                }
            })
            .collect();
        if self.table.as_ref().is_some_and(|table| table.grid != grid) {
            self.issue(span, Severity::Loss, "table:unequal-row-grids")?;
        }
        if self.table.is_none() {
            self.node(1, span.start)?;
            self.table = Some(Table {
                props: TableProperties {
                    indent: Some(Twips(row.left)),
                    ..Default::default()
                },
                grid,
                rows: vec![],
                location: SourceLocation::unknown(),
            });
        }
        self.table
            .as_mut()
            .expect("table initialized")
            .rows
            .push(TableRow {
                props: row.props,
                cells: row.cells,
                sdt: None,
                location: SourceLocation::unknown(),
            });
        Ok(())
    }
    fn finish_font(&mut self, offset: usize) -> Result<(), RtfError> {
        if let Some(id) = self.font_id.take() {
            bounded(
                self.fonts.len() + 1,
                self.options.limits.definitions,
                offset,
                "font_definitions",
            )?;
            if self.font_name.trim().is_empty() {
                return malformed(offset, "empty font name");
            }
            self.fonts.insert(
                id,
                Font {
                    name: std::mem::take(&mut self.font_name).trim().to_string(),
                    charset: self.font_charset,
                    codepage: self.font_codepage,
                },
            );
        }
        Ok(())
    }
    fn select_font(&mut self, id: i32, span: RtfSpan) -> Result<(), RtfError> {
        let Some(font) = self.fonts.get(&id) else {
            return self.issue(span, Severity::Loss, format!("undefined-font:{id}"));
        };
        let cp = font
            .codepage
            .or(match font.charset {
                0 | 1 => Some(self.document_codepage),
                77 => Some(10000),
                128 => Some(932),
                129 => Some(949),
                134 => Some(936),
                136 => Some(950),
                161 => Some(1253),
                162 => Some(1254),
                163 => Some(1258),
                177 => Some(1255),
                178 => Some(1256),
                186 => Some(1257),
                204 => Some(1251),
                222 => Some(874),
                238 => Some(1250),
                _ => None,
            })
            .filter(|cp| codepage(*cp).is_some())
            .ok_or(RtfError::InvalidEncoding {
                offset: span.start,
                reason: "unsupported font charset/codepage (including Symbol/OEM)",
            })?;
        let face = Arc::<str>::from(font.name.as_str());
        self.state.run.fonts = Some(Fonts {
            ascii: Some(face.clone()),
            h_ansi: Some(face.clone()),
            east_asia: Some(face.clone()),
            complex_script: Some(face),
            ..Default::default()
        });
        self.state.codepage = cp;
        Ok(())
    }
    fn issue(
        &mut self,
        span: RtfSpan,
        severity: Severity,
        feature: impl Into<String>,
    ) -> Result<(), RtfError> {
        let feature = feature.into();
        if severity == Severity::Loss
            && self.options.unsupported_policy == UnsupportedPolicy::Reject
        {
            return Err(RtfError::UnsupportedRejected { span, feature });
        }
        match severity {
            Severity::Info => self.report.info_count += 1,
            Severity::Loss => self.report.loss_count += 1,
        }
        if self.report.issues.len() < self.options.limits.issues {
            self.report.issues.push(RtfIssue {
                span,
                severity,
                feature,
            });
        } else {
            self.report.details_truncated = true;
        }
        Ok(())
    }
    fn node(&mut self, n: usize, offset: usize) -> Result<(), RtfError> {
        self.report.nodes += n;
        bounded(
            self.report.nodes,
            self.options.limits.nodes,
            offset,
            "nodes",
        )
    }
    fn replacement(&mut self, offset: usize, reason: &'static str) -> Result<(), RtfError> {
        if self.options.invalid_text_policy == InvalidTextPolicy::Error {
            return Err(RtfError::InvalidEncoding { offset, reason });
        }
        self.issue(
            RtfSpan {
                start: offset,
                end: offset + 1,
            },
            Severity::Loss,
            reason,
        )?;
        self.text("\u{fffd}".into(), offset)
    }
    fn finish_high(&mut self) -> Result<(), RtfError> {
        if let Some((_, offset)) = self.high.take() {
            self.replacement(offset, "unpaired UTF-16 surrogate")?;
        }
        Ok(())
    }
    fn text(&mut self, text: String, offset: usize) -> Result<(), RtfError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.state.destination == Destination::Fonts {
            for chunk in text.split_inclusive(';') {
                self.font_name.push_str(chunk.trim_end_matches(';'));
                if chunk.ends_with(';') {
                    self.finish_font(offset)?;
                }
            }
            return Ok(());
        }
        if self.state.destination == Destination::Colors {
            for c in text.chars() {
                if c == ';' {
                    bounded(
                        self.colors.len() + 1,
                        self.options.limits.definitions,
                        offset,
                        "color_definitions",
                    )?;
                    self.colors.push(if self.color_set {
                        Some(Color::new(format!(
                            "{:02X}{:02X}{:02X}",
                            self.rgb[0], self.rgb[1], self.rgb[2]
                        )))
                    } else {
                        None
                    });
                    self.rgb = [0; 3];
                    self.color_set = false;
                } else if !c.is_whitespace() {
                    return malformed(offset, "unexpected color table text");
                }
            }
            return Ok(());
        }
        if self.state.run.fonts.is_none() && !self.fonts.is_empty() {
            if let Some(id) = self.default_font {
                self.select_font(
                    id,
                    RtfSpan {
                        start: offset,
                        end: offset + 1,
                    },
                )?;
            }
        }
        let text = if text.chars().any(|c| !xml_char(c)) {
            if self.options.invalid_text_policy == InvalidTextPolicy::Error {
                return Err(RtfError::InvalidEncoding {
                    offset,
                    reason: "character cannot be represented in XML 1.0",
                });
            }
            self.issue(
                RtfSpan {
                    start: offset,
                    end: offset + 1,
                },
                Severity::Loss,
                "character cannot be represented in XML 1.0",
            )?;
            text.chars()
                .map(|c| if xml_char(c) { c } else { '\u{fffd}' })
                .collect()
        } else {
            text
        };
        self.report.text_scalars += text.chars().count();
        bounded(
            self.report.text_scalars,
            self.options.limits.text_scalars,
            offset,
            "text_scalars",
        )?;
        // Coalesce adjacent text in equal runs, so source escape density cannot
        // multiply model nodes. Never coalesce across explicit paragraph breaks.
        if let Some(Inline::Run(run)) = self.inlines.last_mut() {
            if run.props == self.state.run {
                if let Some(RunContent::Text(previous)) = run.content.last_mut() {
                    previous.text.push_str(&text);
                    return Ok(());
                }
            }
        }
        self.content(
            RunContent::Text(TextNode {
                text,
                space: Space::Preserve,
            }),
            offset,
        )
    }
    fn content(&mut self, content: RunContent, offset: usize) -> Result<(), RtfError> {
        if matches!(
            content,
            RunContent::Tab
                | RunContent::Break(_)
                | RunContent::NoBreakHyphen
                | RunContent::SoftHyphen
        ) {
            self.report.text_scalars += 1;
            bounded(
                self.report.text_scalars,
                self.options.limits.text_scalars,
                offset,
                "text_scalars",
            )?;
        }
        if self.row.is_none() {
            self.finish_table();
        }
        self.node(1, offset)?;
        if self.inlines.is_empty() {
            self.paragraph_props = self.state.paragraph.clone();
        }
        if let Some(Inline::Run(run)) = self.inlines.last_mut() {
            if run.props == self.state.run {
                run.content.push(content);
                return Ok(());
            }
        }
        self.node(1, offset)?;
        self.inlines.push(Inline::Run(Run {
            props: self.state.run.clone(),
            content: vec![content],
            revision: None,
            location: SourceLocation::unknown(),
        }));
        Ok(())
    }
    fn paragraph(&mut self, offset: usize) -> Result<(), RtfError> {
        self.node(1, offset)?;
        let props = if self.inlines.is_empty() {
            self.state.paragraph.clone()
        } else {
            self.paragraph_props.clone()
        };
        let block = Block::Paragraph(Paragraph {
            props,
            inlines: std::mem::take(&mut self.inlines),
            rsids: Rsids::default(),
            revision: None,
            para_id: None,
            text_id: None,
            location: SourceLocation::unknown(),
        });
        if let Some(row) = self.row.as_mut() {
            row.blocks.push(block);
        } else {
            self.finish_table();
            self.blocks.push(block);
        }
        Ok(())
    }
    fn byte(&mut self, byte: u8, offset: usize) -> Result<(), RtfError> {
        if self.fallback != 0 {
            self.fallback -= 1;
            return Ok(());
        }
        if self.state.destination == Destination::Body
            && self.state.run.fonts.is_none()
            && !self.fonts.is_empty()
        {
            if let Some(id) = self.default_font {
                self.select_font(
                    id,
                    RtfSpan {
                        start: offset,
                        end: offset + 1,
                    },
                )?;
            }
        }
        if self.bytes.is_empty() {
            self.finish_high()?;
            self.bytes_offset = offset;
        }
        self.bytes.push(byte);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), RtfError> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        let encoding = codepage(self.state.codepage).ok_or(RtfError::InvalidEncoding {
            offset: self.bytes_offset,
            reason: "unsupported codepage",
        })?;
        let bytes = std::mem::take(&mut self.bytes);
        let (decoded, errors) = encoding.decode_without_bom_handling(&bytes);
        if errors {
            if self.options.invalid_text_policy == InvalidTextPolicy::Error {
                return Err(RtfError::InvalidEncoding {
                    offset: self.bytes_offset,
                    reason: "invalid codepage byte sequence",
                });
            }
            self.issue(
                RtfSpan {
                    start: self.bytes_offset,
                    end: self.bytes_offset + bytes.len(),
                },
                Severity::Loss,
                "invalid codepage byte sequence",
            )?;
        }
        self.text(decoded.into_owned(), self.bytes_offset)
    }
    fn unicode(&mut self, n: Option<i32>, span: RtfSpan) -> Result<(), RtfError> {
        let Some(n) = n.filter(|n| (-32768..=32767).contains(n)) else {
            return malformed(
                span.start,
                "Unicode parameter must be a signed 16-bit integer",
            );
        };
        let unit = i16::try_from(n)
            .expect("validated signed 16-bit parameter")
            .cast_unsigned();
        self.fallback = self.state.uc;
        if (0xd800..=0xdbff).contains(&unit) {
            self.finish_high()?;
            self.high = Some((unit, span.start));
            return Ok(());
        }
        if (0xdc00..=0xdfff).contains(&unit) {
            if let Some((hi, offset)) = self.high.take() {
                let scalar = 0x10000 + ((u32::from(hi) - 0xd800) << 10) + u32::from(unit) - 0xdc00;
                return self.text(
                    char::from_u32(scalar).expect("surrogate pair").to_string(),
                    offset,
                );
            }
            return self.replacement(span.start, "unpaired UTF-16 surrogate");
        }
        self.finish_high()?;
        self.text(
            char::from_u32(u32::from(unit))
                .expect("non-surrogate")
                .to_string(),
            span.start,
        )
    }
    fn word(&mut self, name: &str, n: Option<i32>, span: RtfSpan) -> Result<(), RtfError> {
        let first = self.state.first;
        self.state.first = false;
        if self.state.skip {
            return Ok(());
        }
        if first && name == "ud" && self.state.allow_ud {
            self.state.ignorable = false;
            self.state.allow_ud = false;
            return Ok(());
        }
        if first && self.state.allow_ud {
            return malformed(span.start, "upr Unicode child must start with ud");
        }
        if first && self.state.ignorable {
            self.state.skip = true;
            return self.issue(span, Severity::Loss, format!("destination:{name}"));
        }
        if first && name == "fonttbl" {
            self.state.destination = Destination::Fonts;
            return Ok(());
        }
        if first && name == "colortbl" {
            self.state.destination = Destination::Colors;
            return Ok(());
        }
        if first && name == "pict" {
            if self.picture.is_some() {
                return malformed(span.start, "nested picture destination");
            }
            self.state.destination = Destination::Picture;
            self.picture = Some(PictureBuilder::default());
            return Ok(());
        }
        if self.state.destination == Destination::Picture {
            let picture = self.picture.as_mut().ok_or(RtfError::Malformed {
                offset: span.start,
                reason: "missing picture collector",
            })?;
            match name {
                "pngblip" => picture.kind = Some(MediaKind::Png),
                "jpegblip" => picture.kind = Some(MediaKind::Jpeg),
                "picw" => picture.width = n,
                "pich" => picture.height = n,
                "picwgoal" => picture.width_goal = n,
                "pichgoal" => picture.height_goal = n,
                "picscalex" => picture.scale_x = n,
                "picscaley" => picture.scale_y = n,
                "wmetafile" | "emfblip" | "dibitmap" | "wbitmap" | "macpict" | "pmmetafile" => {
                    self.state.skip = true;
                    self.issue(span, Severity::Loss, format!("picture-format:{name}"))?;
                }
                _ => self.issue(span, Severity::Loss, format!("picture-control:{name}"))?,
            }
            return Ok(());
        }
        if self.state.destination == Destination::Fonts {
            match name {
                "f" => {
                    self.finish_font(span.start)?;
                    self.font_id = Some(n.filter(|id| *id >= 0).ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid font id",
                    })?);
                    self.font_charset = 0;
                    self.font_codepage = None;
                    self.font_name.clear();
                }
                "fcharset" => {
                    self.font_charset = n.ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "missing font charset",
                    })?;
                }
                "cpg" => self.font_codepage = n,
                "fnil" | "froman" | "fswiss" | "fmodern" | "fscript" | "fdecor" | "ftech"
                | "fbidi" | "fprq" => (),
                "uc" => {
                    self.state.uc = n
                        .and_then(|x| usize::try_from(x).ok())
                        .filter(|x| *x <= 32767)
                        .ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "invalid Unicode fallback count",
                        })?;
                }
                "u" => self.unicode(n, span)?,
                _ => self.issue(span, Severity::Loss, format!("font-control:{name}"))?,
            }
            return Ok(());
        }
        if self.state.destination == Destination::Colors {
            if let Some(index) = match name {
                "red" => Some(0),
                "green" => Some(1),
                "blue" => Some(2),
                _ => None,
            } {
                self.rgb[index] =
                    n.and_then(|n| u8::try_from(n).ok())
                        .ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "invalid color component",
                        })?;
                self.color_set = true;
            } else {
                self.issue(span, Severity::Loss, format!("color-control:{name}"))?;
            }
            return Ok(());
        }
        // Destinations have their own content language. Skipping is group scoped;
        // the lexer still validates their syntax, including binary payload length.
        if first
            && matches!(
                name,
                "fonttbl"
                    | "colortbl"
                    | "stylesheet"
                    | "listtable"
                    | "listoverridetable"
                    | "pict"
                    | "object"
                    | "shp"
                    | "shppict"
                    | "nonshppict"
                    | "header"
                    | "headerl"
                    | "headerr"
                    | "headerf"
                    | "footer"
                    | "footerl"
                    | "footerr"
                    | "footerf"
                    | "footnote"
                    | "annotation"
                    | "info"
                    | "filetbl"
                    | "revtbl"
                    | "xmlnstbl"
                    | "generator"
                    | "fldinst"
                    | "listtext"
                    | "pntext"
                    | "datafield"
            )
        {
            self.state.skip = true;
            let severity = if matches!(name, "info" | "generator") {
                Severity::Info
            } else {
                Severity::Loss
            };
            return self.issue(span, severity, format!("destination:{name}"));
        }
        if self.fallback > 0 && name != "u" && name != "uc" {
            self.fallback -= 1;
            return Ok(());
        }
        let toggle = if n == Some(0) {
            TriState::Off
        } else {
            TriState::On
        };
        match name {
            "upr" => self.state.upr_children = Some(0),
            "rtf" if n == Some(1) && self.stack.len() == 1 => (),
            "ansi" => self.state.codepage = 1252,
            "mac" => self.state.codepage = 10000,
            "pc" | "pca" => {
                return Err(RtfError::InvalidEncoding {
                    offset: span.start,
                    reason: "unsupported OEM codepage",
                })
            }
            "ansicpg" => {
                let Some(cp) = n.filter(|cp| codepage(*cp).is_some()) else {
                    return Err(RtfError::InvalidEncoding {
                        offset: span.start,
                        reason: "unsupported codepage",
                    });
                };
                self.state.codepage = cp;
                self.document_codepage = cp;
            }
            "deff" => self.default_font = n,
            "f" => {
                let id = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing font id",
                })?;
                self.select_font(id, span)?;
            }
            "cf" => {
                let index = n
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid color index",
                    })?;
                let Some(color) = self.colors.get(index) else {
                    self.issue(span, Severity::Loss, format!("undefined-color:{index}"))?;
                    return Ok(());
                };
                self.state.run.color = Some(color.clone().unwrap_or_else(|| Color::new("auto")));
            }
            "uc" => {
                let Some(count) = n
                    .and_then(|x| usize::try_from(x).ok())
                    .filter(|x| *x <= 32767)
                else {
                    return malformed(span.start, "invalid Unicode fallback count");
                };
                self.state.uc = count;
            }
            "u" => self.unicode(n, span)?,
            "plain" => {
                self.state.run = RunProperties::default();
                self.state.codepage = self.document_codepage;
                if let Some(id) = self.default_font {
                    self.select_font(id, span)?;
                }
            }
            "pard" => self.state.paragraph = ParagraphProperties::default(),
            "b" => self.state.run.bold = toggle,
            "i" => self.state.run.italic = toggle,
            "strike" => self.state.run.strike = toggle,
            "striked" => self.state.run.double_strike = toggle,
            "caps" => self.state.run.caps = toggle,
            "scaps" => self.state.run.small_caps = toggle,
            "v" => self.state.run.vanish = toggle,
            "rtlch" => self.state.run.rtl = TriState::On,
            "ltrch" => self.state.run.rtl = TriState::Off,
            "ul" => {
                self.state.run.underline = Some(if n == Some(0) {
                    Underline::None
                } else {
                    Underline::Single
                });
            }
            "ulnone" => self.state.run.underline = Some(Underline::None),
            "uldb" => self.state.run.underline = Some(Underline::Double),
            "fs" => {
                let Some(size) = n.filter(|n| *n > 0) else {
                    return malformed(span.start, "invalid font size");
                };
                self.state.run.size = Some(HalfPoints(size));
            }
            "super" => self.state.run.vert_align = Some(VertAlign::Superscript),
            "sub" => self.state.run.vert_align = Some(VertAlign::Subscript),
            "nosupersub" => self.state.run.vert_align = Some(VertAlign::Baseline),
            "ql" => self.state.paragraph.alignment = Some(Justification::Start),
            "qr" => self.state.paragraph.alignment = Some(Justification::End),
            "qc" => self.state.paragraph.alignment = Some(Justification::Center),
            "qj" => self.state.paragraph.alignment = Some(Justification::Both),
            "rtlpar" => self.state.paragraph.bidi = TriState::On,
            "ltrpar" => self.state.paragraph.bidi = TriState::Off,
            "keep" => self.state.paragraph.keep_lines = toggle,
            "keepn" => self.state.paragraph.keep_next = toggle,
            "pagebb" => self.state.paragraph.page_break_before = toggle,
            "trowd" => {
                if self.row.is_none() && !self.inlines.is_empty() {
                    self.paragraph(span.start)?;
                }
                // RTF permits a repeated row definition immediately before
                // the row terminator. Keep content already collected.
                if let Some(row) = self.row.as_mut() {
                    row.left = 0;
                    row.ends.clear();
                    row.declarations.clear();
                    row.next_props = CellProperties::default();
                    row.props = RowProperties::default();
                } else {
                    self.row = Some(RowBuilder::default());
                }
            }
            "trleft" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "table property outside row",
                })?;
                row.left = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing table indent",
                })?;
            }
            "cellx" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cellx outside row",
                })?;
                let end = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing cell boundary",
                })?;
                let start = row.ends.last().copied().unwrap_or(row.left);
                let width = end.checked_sub(start).filter(|width| *width > 0).ok_or(
                    RtfError::Malformed {
                        offset: span.start,
                        reason: "non-increasing cell boundaries",
                    },
                )?;
                bounded(
                    row.ends.len() + 1,
                    self.options.limits.table_cells,
                    span.start,
                    "table_cells",
                )?;
                let mut props = std::mem::take(&mut row.next_props);
                props.width = Some(Width {
                    kind: WidthKind::Dxa,
                    value: Some(width),
                });
                row.ends.push(end);
                row.declarations.push(props);
            }
            "clvmgf" | "clvmrg" | "clvertalt" | "clvertalc" | "clvertalb" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                match name {
                    "clvmgf" => row.next_props.vertical_merge = Some(VerticalMerge::Restart),
                    "clvmrg" => row.next_props.vertical_merge = Some(VerticalMerge::Continue),
                    "clvertalt" => row.next_props.vertical_align = Some(VerticalJc::Top),
                    "clvertalc" => row.next_props.vertical_align = Some(VerticalJc::Center),
                    _ => row.next_props.vertical_align = Some(VerticalJc::Bottom),
                }
            }
            "trhdr" | "trkeep" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "row property outside row",
                })?;
                if name == "trhdr" {
                    row.props.header = n != Some(0);
                } else {
                    row.props.cant_split = n != Some(0);
                }
            }
            "itap" if n.is_some_and(|n| n > 1) => {
                return Err(RtfError::UnsupportedRejected {
                    span,
                    feature: "nested-table-structure".into(),
                })
            }
            "intbl" => {
                if n != Some(0) && self.row.is_none() {
                    self.row = self.row_template.clone();
                }
            }
            "itap" => (),
            "cell" => {
                self.finish_high()?;
                self.cell(span.start)?;
            }
            "row" => {
                self.finish_high()?;
                self.finish_row(span)?;
            }
            "li" | "ri" | "fi" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing indentation",
                })?;
                let props = self
                    .state
                    .paragraph
                    .indentation
                    .get_or_insert_with(Default::default);
                match name {
                    "li" => props.start = Some(Twips(value)),
                    "ri" => props.end = Some(Twips(value)),
                    _ => {
                        if value < 0 {
                            props.hanging =
                                Some(Twips(value.checked_neg().ok_or(RtfError::Malformed {
                                    offset: span.start,
                                    reason: "indentation overflow",
                                })?));
                            props.first_line = None;
                        } else {
                            props.first_line = Some(Twips(value));
                            props.hanging = None;
                        }
                    }
                }
            }
            "sb" | "sa" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing paragraph spacing",
                })?;
                let props = self
                    .state
                    .paragraph
                    .spacing
                    .get_or_insert_with(Default::default);
                if name == "sb" {
                    props.before = Some(Twips(value));
                } else {
                    props.after = Some(Twips(value));
                }
            }
            "paperw" | "paperh" | "pgwsxn" | "pghsxn" => {
                let value = n.filter(|n| *n > 0).ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "invalid page dimension",
                })?;
                let size = self.section.page_size.get_or_insert_with(Default::default);
                if matches!(name, "paperw" | "pgwsxn") {
                    size.width = Some(Twips(value));
                } else {
                    size.height = Some(Twips(value));
                }
            }
            "margl" | "margr" | "margt" | "margb" | "marglsxn" | "margrsxn" | "margtsxn"
            | "margbsxn" | "headery" | "footery" | "gutter" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing page margin",
                })?;
                let margins = self
                    .section
                    .page_margins
                    .get_or_insert_with(Default::default);
                match name {
                    "margl" | "marglsxn" => margins.left = Some(Twips(value)),
                    "margr" | "margrsxn" => margins.right = Some(Twips(value)),
                    "margt" | "margtsxn" => margins.top = Some(Twips(value)),
                    "margb" | "margbsxn" => margins.bottom = Some(Twips(value)),
                    "headery" => margins.header = Some(Twips(value)),
                    "footery" => margins.footer = Some(Twips(value)),
                    _ => margins.gutter = Some(Twips(value)),
                }
            }
            "landscape" | "lndscpsxn" => {
                self.section
                    .page_size
                    .get_or_insert_with(Default::default)
                    .orientation = Some(PageOrientation::Landscape);
            }
            "par" => {
                self.finish_high()?;
                self.paragraph(span.start)?;
            }
            "tab" => {
                self.finish_high()?;
                self.content(RunContent::Tab, span.start)?;
            }
            "line" | "page" | "column" => {
                self.finish_high()?;
                self.content(
                    RunContent::Break(match name {
                        "page" => BreakKind::Page,
                        "column" => BreakKind::Column,
                        _ => BreakKind::TextWrapping,
                    }),
                    span.start,
                )?;
            }
            "emdash" | "endash" | "bullet" | "lquote" | "rquote" | "ldblquote" | "rdblquote"
            | "emspace" | "enspace" | "qmspace" => {
                self.finish_high()?;
                let text = match name {
                    "emdash" => "—",
                    "endash" => "–",
                    "bullet" => "•",
                    "lquote" => "‘",
                    "rquote" => "’",
                    "ldblquote" => "“",
                    "rdblquote" => "”",
                    "emspace" => "\u{2003}",
                    "enspace" => "\u{2002}",
                    _ => "\u{2005}",
                };
                self.text(text.into(), span.start)?;
            }
            "field" | "fldrslt" => {
                self.issue(
                    span,
                    Severity::Loss,
                    format!("field:{name}:cached-result-only"),
                )?;
            }
            _ => {
                self.issue(span, Severity::Loss, format!("control:{name}"))?;
            }
        }
        if !self.inlines.is_empty() {
            self.paragraph_props = self.state.paragraph.clone();
        }
        Ok(())
    }
}
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r')
        || ('\u{20}'..='\u{d7ff}').contains(&c)
        || ('\u{e000}'..='\u{fffd}').contains(&c)
        || c >= '\u{10000}'
}
fn codepage(cp: i32) -> Option<&'static encoding_rs::Encoding> {
    let label = match cp {
        1250 => "windows-1250",
        1251 => "windows-1251",
        1252 => "windows-1252",
        1253 => "windows-1253",
        1254 => "windows-1254",
        1255 => "windows-1255",
        1256 => "windows-1256",
        1257 => "windows-1257",
        1258 => "windows-1258",
        874 => "windows-874",
        932 => "shift_jis",
        936 => "gbk",
        949 => "euc-kr",
        950 => "big5",
        10000 => "macintosh",
        65001 => "utf-8",
        _ => return None,
    };
    encoding_rs::Encoding::for_label(label.as_bytes())
}
pub(super) fn parse(input: &[u8], options: &RtfOptions) -> Result<RtfImported, RtfError> {
    bounded(input.len(), options.limits.input_bytes, 0, "input_bytes")?;
    let mut lexer = Lexer {
        bytes: input,
        pos: 0,
    };
    if !matches!(lexer.next()?, Some((_, Token::Open)))
        || !matches!(lexer.next()?, Some((_, Token::Word("rtf", Some(1)))))
    {
        return Err(RtfError::InvalidHeader);
    }
    bounded(1, options.limits.depth, 0, "depth")?;
    bounded(2, options.limits.tokens, 0, "tokens")?;
    let mut p = Parser {
        options,
        report: RtfReport {
            tokens: 2,
            ..RtfReport::default()
        },
        state: State {
            first: false,
            ..State::default()
        },
        stack: vec![State::default()],
        blocks: vec![],
        inlines: vec![],
        paragraph_props: ParagraphProperties::default(),
        bytes: vec![],
        bytes_offset: 0,
        fallback: 0,
        high: None,
        fonts: BTreeMap::new(),
        font_id: None,
        font_name: String::new(),
        font_charset: 0,
        font_codepage: None,
        default_font: None,
        document_codepage: 1252,
        colors: vec![],
        rgb: [0; 3],
        color_set: false,
        section: SectionProperties::default(),
        row: None,
        table: None,
        table_rows: 0,
        table_cells: 0,
        row_template: None,
        picture: None,
        media: vec![],
        media_index: MediaIndex::new(),
        media_bytes: 0,
    };
    loop {
        let Some((span, token)) = lexer.next()? else {
            return malformed(input.len(), "unclosed root group");
        };
        p.report.tokens += 1;
        bounded(p.report.tokens, options.limits.tokens, span.start, "tokens")?;
        match token {
            Token::Open => {
                p.flush()?;
                p.fallback = 0;
                bounded(p.stack.len() + 1, options.limits.depth, span.start, "depth")?;
                let child_index = if let Some(index) = p.state.upr_children.as_mut() {
                    *index += 1;
                    if *index > 2 {
                        return malformed(span.start, "upr requires exactly two child groups");
                    }
                    Some(*index)
                } else {
                    None
                };
                p.stack.push(p.state.clone());
                p.state.first = true;
                p.state.ignorable = false;
                p.state.upr_children = None;
                p.state.allow_ud = child_index == Some(2);
                if child_index == Some(1) {
                    p.state.skip = true;
                }
            }
            Token::Close => {
                p.flush()?;
                p.fallback = 0;
                if !p.state.skip && p.state.allow_ud {
                    return malformed(span.start, "upr Unicode child must start with ud");
                }
                if p.state.destination == Destination::Picture
                    && p.stack
                        .last()
                        .is_some_and(|s| s.destination != Destination::Picture)
                {
                    p.finish_picture(span)?;
                }
                if !p.state.skip && p.state.upr_children.is_some_and(|n| n != 2) {
                    return malformed(span.start, "upr requires exactly two child groups");
                }
                if !p.state.skip
                    && p.state.destination == Destination::Fonts
                    && p.stack
                        .last()
                        .is_some_and(|s| s.destination != Destination::Fonts)
                {
                    p.finish_font(span.start)?;
                }
                if p.stack.len() == 1 {
                    p.finish_high()?;
                }
                p.state = p.stack.pop().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "unmatched closing group",
                })?;
                if p.stack.is_empty() {
                    break;
                }
            }
            Token::Bytes(bytes) => {
                if !p.state.skip {
                    if p.state.allow_ud && bytes.iter().any(|b| !b.is_ascii_whitespace()) {
                        return malformed(span.start, "upr Unicode child must start with ud");
                    }
                    if p.state.destination == Destination::Picture {
                        let picture = p.picture.as_mut().expect("picture destination");
                        for (index, byte) in bytes.iter().copied().enumerate() {
                            picture.hex(
                                byte,
                                &options.limits,
                                p.media_bytes,
                                span.start + index,
                            )?;
                        }
                        continue;
                    }
                    for (index, byte) in bytes.iter().copied().enumerate() {
                        if !matches!(byte, b'\r' | b'\n') {
                            p.state.first = false;
                            if byte == b'\t'
                                && p.fallback == 0
                                && p.state.destination == Destination::Body
                            {
                                p.flush()?;
                                p.finish_high()?;
                                p.content(RunContent::Tab, span.start + index)?;
                            } else {
                                p.byte(byte, span.start + index)?;
                            }
                        }
                    }
                }
            }
            Token::Byte(byte) => {
                if !p.state.skip {
                    if p.state.destination == Destination::Picture {
                        p.picture.as_mut().expect("picture destination").push(
                            &[byte],
                            &options.limits,
                            p.media_bytes,
                            span.start,
                        )?;
                        continue;
                    }
                    p.state.first = false;
                    p.byte(byte, span.start)?;
                }
            }
            Token::Binary(payload) => {
                p.flush()?;
                if !p.state.skip {
                    if p.fallback > 0 {
                        p.fallback -= 1;
                        continue;
                    }
                    if p.state.destination == Destination::Picture {
                        p.picture.as_mut().expect("picture destination").push(
                            payload,
                            &options.limits,
                            p.media_bytes,
                            span.start,
                        )?;
                        continue;
                    }
                    p.issue(span, Severity::Loss, "binary-outside-supported-destination")?;
                }
            }
            Token::Word(name, n) => {
                p.flush()?;
                p.word(name, n, span)?;
            }
            Token::Symbol(symbol) => {
                if p.state.skip {
                    continue;
                }
                if symbol == b'*' && p.state.first {
                    p.state.ignorable = true;
                    continue;
                }
                if matches!(symbol, b'\r' | b'\n') {
                    if p.fallback > 0 {
                        p.fallback -= 1;
                    } else {
                        p.flush()?;
                        p.finish_high()?;
                        p.paragraph(span.start)?;
                    }
                    continue;
                }
                p.state.first = false;
                if matches!(symbol, b'{' | b'}' | b'\\') {
                    p.byte(symbol, span.start)?;
                    continue;
                }
                if p.fallback > 0 {
                    p.fallback -= 1;
                    continue;
                }
                p.flush()?;
                p.finish_high()?;
                match symbol {
                    b'~' => p.text("\u{a0}".into(), span.start)?,
                    b'-' => p.content(RunContent::SoftHyphen, span.start)?,
                    b'_' => p.content(RunContent::NoBreakHyphen, span.start)?,
                    _ => p.issue(
                        span,
                        Severity::Loss,
                        format!("symbol:{}", char::from(symbol)),
                    )?,
                }
            }
        }
    }
    p.finish_high()?;
    let trailer = &input[lexer.pos..];
    let trimmed = trailer
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if trimmed == [0] {
        p.issue(
            RtfSpan {
                start: lexer.pos,
                end: input.len(),
            },
            Severity::Info,
            "trailing-NUL",
        )?;
    } else if !trimmed.is_empty() {
        return malformed(lexer.pos, "data after root group");
    }
    if p.row.is_some() {
        return malformed(lexer.pos, "unterminated table row");
    }
    p.finish_table();
    if !p.inlines.is_empty() || p.blocks.is_empty() {
        p.paragraph(lexer.pos)?;
    }
    let document = Document {
        body: Body { blocks: p.blocks },
        styles: Default::default(),
        numbering: Default::default(),
        footnotes: Default::default(),
        endnotes: Default::default(),
        settings: Default::default(),
        theme: None,
        font_table: None,
        sections: vec![Section {
            properties: p.section,
            location: SourceLocation::unknown(),
        }],
        headers_footers: vec![],
        media: p.media_index,
        support: Default::default(),
        source: DocumentSource {
            main_document: PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            font_table: None,
            footnotes: None,
            endnotes: None,
            theme: None,
        },
    };
    Ok(RtfImported {
        document,
        media: p.media,
        report: p.report,
    })
}
// The control dispatch mirrors the RTF vocabulary. Independent group-state
// flags describe syntax, inherited skipping and Unicode alternate selection.
