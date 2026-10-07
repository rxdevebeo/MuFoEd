#![allow(
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::default_trait_access
)]
use crate::lists::{number_format, LevelDraft, ListDraft, OverrideDraft};
use crate::picture::PictureBuilder;
use crate::{
    CompatibilityProfile, Document, InvalidTextPolicy, NulPolicy, PartId, RtfError, RtfImported,
    RtfIssue, RtfOptions, RtfReport, RtfSpan, Severity, UnicodeParameterPolicy, UnsupportedPolicy,
};
use std::{collections::BTreeMap, sync::Arc};
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_wml::model::document::{Body, DocumentSource};
use strict_ooxml_wml::model::drawing::{MediaIndex, MediaKind};
use strict_ooxml_wml::model::{
    AbstractNum, AbstractNumId, Block, BreakKind, CellProperties, Color, Field, Fonts, GridCol,
    HalfPoints, HeaderFooter, HeaderFooterKind, HeaderFooterRef, Ilvl, Inline, Justification,
    Language, Level, LineSpacingRule, Note, NoteKind, NoteTable, Num, NumId, NumPr, NumberingTable,
    PageMargins, PageOrientation, PageSize, Paragraph, ParagraphProperties, RowProperties, Rsids,
    Run, RunContent, RunProperties, Section, SectionProperties, SectionType, Space, Style, StyleId,
    StyleTable, StyleType, Symbol, TabAlignment, TabLeader, TabStop, Table, TableCell,
    TableProperties, TableRow, TextNode, TriState, Twips, Underline, VertAlign, VerticalJc,
    VerticalMerge, Width, WidthKind,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination {
    Body,
    Fonts,
    Colors,
    Picture,
    Styles,
    FieldInstruction,
    Lists,
    Overrides,
    LevelText,
}
struct StyleDraft {
    depth: usize,
    id: StyleId,
    kind: StyleType,
    name: String,
    parent: Option<StyleId>,
    next: Option<StyleId>,
    link: Option<StyleId>,
    priority: Option<i32>,
    hidden: bool,
    semi_hidden: bool,
    quick: bool,
}
enum RegionKind {
    Header {
        is_header: bool,
        kind: HeaderFooterKind,
    },
    Note {
        endnote: bool,
    },
    Field {
        instruction: String,
    },
}
struct RegionFrame {
    depth: usize,
    automatic_reference: bool,
    kind: RegionKind,
    blocks: Vec<Block>,
    inlines: Vec<Inline>,
    paragraph_props: ParagraphProperties,
    paragraph_pending: bool,
    row: Option<RowBuilder>,
    table: Option<Table>,
    template: Option<RowBuilder>,
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
    merges: Vec<Option<bool>>,
    next_merge: Option<bool>,
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
    associated_font: Option<i32>,
    font_slot: Option<FontSlot>,
    uc: usize,
    skip: bool,
    first: bool,
    ignorable: bool,
    destination: Destination,
    upr_children: Option<u8>,
    allow_ud: bool,
    literal_prefix: bool,
    tab_alignment: TabAlignment,
    tab_leader: Option<TabLeader>,
    line_spacing: i32,
    line_multiple: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            run: RunProperties::default(),
            paragraph: ParagraphProperties::default(),
            codepage: 1252,
            associated_font: None,
            font_slot: None,
            uc: 1,
            skip: false,
            first: true,
            ignorable: false,
            destination: Destination::Body,
            upr_children: None,
            allow_ud: false,
            literal_prefix: false,
            tab_alignment: TabAlignment::Start,
            tab_leader: None,
            line_spacing: 0,
            line_multiple: false,
        }
    }
}

#[derive(Clone, Copy)]
enum FontSlot {
    Ascii,
    HighAnsi,
    EastAsia,
    ComplexScript,
}

#[derive(Clone, Copy)]
enum LanguageSlot {
    Primary,
    EastAsia,
    Bidi,
}

fn assign_font(fonts: &mut Fonts, slot: FontSlot, face: Arc<str>) {
    match slot {
        FontSlot::Ascii => fonts.ascii = Some(face),
        FontSlot::HighAnsi => fonts.h_ansi = Some(face),
        FontSlot::EastAsia => fonts.east_asia = Some(face),
        FontSlot::ComplexScript => fonts.complex_script = Some(face),
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
    paragraph_pending: bool,
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
    pending_note_marker: Option<RtfSpan>,
    document_codepage: i32,
    colors: Vec<Option<Color>>,
    rgb: [u8; 3],
    color_set: bool,
    section: SectionProperties,
    document_page_size: PageSize,
    document_page_margins: PageMargins,
    sections: Vec<Section>,
    row: Option<RowBuilder>,
    table: Option<Table>,
    table_rows: usize,
    table_cells: usize,
    row_template: Option<RowBuilder>,
    picture: Option<PictureBuilder>,
    media: Vec<(PartId, Vec<u8>)>,
    media_index: MediaIndex,
    media_bytes: usize,
    styles: StyleTable,
    style_draft: Option<StyleDraft>,
    regions: Vec<RegionFrame>,
    headers_footers: Vec<HeaderFooter>,
    footnotes: NoteTable,
    endnotes: NoteTable,
    numbering: NumberingTable,
    list_draft: Option<ListDraft>,
    level_draft: Option<LevelDraft>,
    override_draft: Option<OverrideDraft>,
}
impl Parser<'_> {
    fn finish_level(&mut self, offset: usize) -> Result<(), RtfError> {
        let draft = self.level_draft.take().expect("checked level depth");
        let list = self.list_draft.as_mut().ok_or(RtfError::Malformed {
            offset,
            reason: "list level outside list",
        })?;
        if list.levels.len() >= 9 {
            return malformed(offset, "list has more than nine levels");
        }
        let mut chars = draft.text.chars();
        let count = chars.next().ok_or(RtfError::Malformed {
            offset,
            reason: "missing leveltext length",
        })? as usize;
        let mut text = String::new();
        for _ in 0..count {
            let c = chars.next().ok_or(RtfError::Malformed {
                offset,
                reason: "truncated leveltext",
            })?;
            if u32::from(c) <= 8 {
                text.push('%');
                text.push(char::from_u32(u32::from('1') + u32::from(c)).expect("level 0..8"));
            } else if xml_char(c) {
                text.push(c);
            } else {
                return malformed(offset, "invalid leveltext character");
            }
        }
        if chars.any(|c| c != ';' && !c.is_whitespace()) {
            return malformed(offset, "leveltext exceeds declared length");
        }
        list.levels.push(Level {
            ilvl: Ilvl(u8::try_from(list.levels.len()).expect("at most nine")),
            start: Some(draft.start),
            format: Some(Arc::from(draft.format)),
            text: Some(Arc::from(text)),
            justification: Some(draft.justification),
            suffix: Some(Arc::from(draft.suffix)),
            paragraph: self.state.paragraph.clone(),
            run: self.state.run.clone(),
            ..Default::default()
        });
        self.node(1, offset)
    }
    fn finish_list(&mut self, offset: usize) -> Result<(), RtfError> {
        let draft = self.list_draft.take().expect("checked list depth");
        let id = draft.id.ok_or(RtfError::Malformed {
            offset,
            reason: "missing listid",
        })?;
        bounded(
            self.numbering.abstracts().count() + 1,
            self.options.limits.definitions,
            offset,
            "list_definitions",
        )?;
        if !self.numbering.insert_abstract(AbstractNum {
            id: AbstractNumId(id),
            multi_level_type: Some(Arc::from("multilevel")),
            num_style_link: None,
            style_link: None,
            levels: draft.levels,
            location: SourceLocation::unknown(),
        }) {
            return malformed(offset, "duplicate listid");
        }
        self.node(1, offset)
    }
    fn finish_override(&mut self, offset: usize) -> Result<(), RtfError> {
        let draft = self.override_draft.take().expect("checked override depth");
        let instance = draft
            .instance
            .filter(|n| *n != 0)
            .ok_or(RtfError::Malformed {
                offset,
                reason: "missing list instance id",
            })?;
        let list = draft.list.ok_or(RtfError::Malformed {
            offset,
            reason: "missing override listid",
        })?;
        bounded(
            self.numbering.len() + 1,
            self.options.limits.definitions,
            offset,
            "list_instances",
        )?;
        if !self.numbering.insert_num(Num {
            num_id: NumId(instance),
            abstract_num_id: AbstractNumId(list),
            overrides: vec![],
            location: SourceLocation::unknown(),
        }) {
            return malformed(offset, "duplicate list instance");
        }
        self.node(1, offset)
    }
    fn begin_region(&mut self, kind: RegionKind, offset: usize) -> Result<(), RtfError> {
        if matches!(kind, RegionKind::Header { .. }) && !self.regions.is_empty()
            || matches!(kind, RegionKind::Note { .. })
                && self
                    .regions
                    .iter()
                    .any(|r| matches!(r.kind, RegionKind::Note { .. } | RegionKind::Header { .. }))
        {
            return malformed(offset, "unsupported nested document region");
        }
        self.finish_high()?;
        bounded(
            self.regions.len() + 1,
            self.options.limits.depth,
            offset,
            "region_depth",
        )?;
        let automatic_reference = if matches!(kind, RegionKind::Note { .. }) {
            self.pending_note_marker.take().is_some()
        } else {
            self.clear_note_marker()?;
            false
        };
        self.regions.push(RegionFrame {
            depth: self.stack.len(),
            automatic_reference,
            kind,
            blocks: std::mem::take(&mut self.blocks),
            inlines: std::mem::take(&mut self.inlines),
            paragraph_props: std::mem::take(&mut self.paragraph_props),
            paragraph_pending: std::mem::take(&mut self.paragraph_pending),
            row: self.row.take(),
            table: self.table.take(),
            template: self.row_template.take(),
        });
        self.state.destination = Destination::Body;
        self.state.paragraph = ParagraphProperties::default();
        Ok(())
    }
    fn end_region(&mut self, span: RtfSpan) -> Result<Option<Inline>, RtfError> {
        self.finish_high()?;
        if self.row.is_some() {
            return malformed(span.start, "unterminated region table row");
        }
        self.finish_table();
        let ended_paragraph =
            !self.paragraph_pending && self.inlines.is_empty() && !self.blocks.is_empty();
        if ended_paragraph
            && self
                .regions
                .last()
                .is_some_and(|r| matches!(r.kind, RegionKind::Note { .. }))
        {
            self.paragraph(span.start)?;
        }
        if self.paragraph_pending || !self.inlines.is_empty() || self.blocks.is_empty() {
            self.paragraph(span.start)?;
        }
        let blocks = std::mem::take(&mut self.blocks);
        let frame = self.regions.pop().expect("region depth checked");
        self.blocks = frame.blocks;
        self.inlines = frame.inlines;
        self.paragraph_props = frame.paragraph_props;
        self.paragraph_pending = frame.paragraph_pending;
        self.row = frame.row;
        self.table = frame.table;
        self.row_template = frame.template;
        self.node(1, span.start)?;
        match frame.kind {
            RegionKind::Header { is_header, kind } => {
                bounded(
                    self.headers_footers.len() + 1,
                    self.options.limits.definitions,
                    span.start,
                    "regions",
                )?;
                let part = PartId::new(format!(
                    "/word/{}{}.xml",
                    if is_header { "header" } else { "footer" },
                    self.headers_footers.len() + 1
                ));
                let refs = if is_header {
                    &mut self.section.headers
                } else {
                    &mut self.section.footers
                };
                refs.retain(|r| r.kind != kind);
                refs.push(HeaderFooterRef {
                    kind,
                    rel_id: RelId::new(format!("rtfRegion{}", self.headers_footers.len() + 1)),
                    part: Some(part.clone()),
                });
                self.headers_footers.push(HeaderFooter {
                    part,
                    is_header,
                    blocks,
                    location: SourceLocation::unknown(),
                });
                Ok(None)
            }
            RegionKind::Note { endnote } => {
                if !frame.automatic_reference {
                    self.issue(
                        span,
                        Severity::Loss,
                        "note:implicit-reference-marker-assumed",
                    )?;
                }
                let table = if endnote {
                    &mut self.endnotes
                } else {
                    &mut self.footnotes
                };
                bounded(
                    table.len() + 1,
                    self.options.limits.definitions,
                    span.start,
                    "notes",
                )?;
                let id = i32::try_from(table.len() + 1).map_err(|_| RtfError::Malformed {
                    offset: span.start,
                    reason: "note id overflow",
                })?;
                table.insert(Note {
                    id,
                    kind: NoteKind::Normal,
                    blocks,
                    location: SourceLocation::unknown(),
                });
                let content = if endnote {
                    RunContent::EndnoteRef(id.cast_unsigned())
                } else {
                    RunContent::FootnoteRef(id.cast_unsigned())
                };
                Ok(Some(Inline::Run(Run {
                    props: RunProperties::default(),
                    content: vec![content],
                    revision: None,
                    location: SourceLocation::unknown(),
                })))
            }
            RegionKind::Field { instruction } => {
                if blocks.len() > 1 || ended_paragraph {
                    // The shared model supports inline fields only. Retain the
                    // cached paragraphs and explicitly report lost field scope.
                    self.issue(
                        span,
                        Severity::Loss,
                        "field:multi-paragraph-instruction-dropped",
                    )?;
                    let count = blocks.len();
                    for (index, block) in blocks.into_iter().enumerate() {
                        let Block::Paragraph(p) = block else {
                            return malformed(span.start, "table in inline field result");
                        };
                        if self.inlines.is_empty() {
                            self.paragraph_props = p.props;
                        }
                        self.inlines.extend(p.inlines);
                        self.paragraph_pending = true;
                        if index + 1 < count || ended_paragraph {
                            self.paragraph(span.start)?;
                        }
                    }
                    return Ok(None);
                }
                let mut inlines = vec![];
                let count = blocks.len();
                for (index, block) in blocks.into_iter().enumerate() {
                    if let Block::Paragraph(p) = block {
                        if index != 0 {
                            self.issue(
                                span,
                                Severity::Loss,
                                "field:multi-paragraph-result-flattened",
                            )?;
                            inlines.push(Inline::Run(Run {
                                props: RunProperties::default(),
                                content: vec![RunContent::Break(BreakKind::TextWrapping)],
                                revision: None,
                                location: SourceLocation::unknown(),
                            }));
                        }
                        inlines.extend(p.inlines);
                    } else {
                        return malformed(span.start, "table in inline field result");
                    }
                }
                if count == 0 {
                    return malformed(span.start, "empty field builder");
                }
                Ok(Some(Inline::Field(Field {
                    instruction: Some(Arc::from(instruction.trim())),
                    inlines,
                    location: SourceLocation::unknown(),
                })))
            }
        }
    }
    fn finish_style(&mut self, offset: usize) -> Result<(), RtfError> {
        let draft = self.style_draft.take().expect("style depth checked");
        bounded(
            self.styles.len() + 1,
            self.options.limits.definitions,
            offset,
            "styles",
        )?;
        self.node(1, offset)?;
        if self.styles.contains(&draft.id) {
            return malformed(offset, "duplicate style id");
        }
        self.styles.insert(Style {
            is_default: draft.id.as_str() == "rtf-s0",
            id: draft.id,
            style_type: draft.kind,
            name: Some(Arc::from(draft.name.trim().trim_end_matches(';'))),
            based_on: draft.parent,
            next: draft.next,
            link: draft.link,
            semi_hidden: draft.semi_hidden,
            hidden: draft.hidden,
            q_format: draft.quick,
            locked: false,
            unhide_when_used: false,
            ui_priority: draft.priority,
            table: TableProperties::default(),
            paragraph: self.state.paragraph.clone(),
            run: self.state.run.clone(),
            based_on_chain: vec![],
            location: SourceLocation::unknown(),
        });
        Ok(())
    }
    fn resolve_style_chains(&mut self, offset: usize) -> Result<(), RtfError> {
        let ids: Vec<_> = self.styles.ids().cloned().collect();
        for id in ids {
            let mut chain = vec![];
            let mut next = self.styles.get(&id).and_then(|s| s.based_on.clone());
            while let Some(parent) = next {
                if parent == id || chain.contains(&parent) {
                    return malformed(offset, "cyclic style inheritance");
                }
                bounded(
                    chain.len() + 1,
                    self.options.limits.depth.min(64),
                    offset,
                    "style_chain_depth",
                )?;
                if !self.styles.contains(&parent) {
                    self.issue(
                        RtfSpan {
                            start: offset,
                            end: offset,
                        },
                        Severity::Loss,
                        format!("style:undefined-parent:{parent}"),
                    )?;
                    break;
                }
                next = self.styles.get(&parent).and_then(|s| s.based_on.clone());
                chain.push(parent);
            }
            self.styles
                .get_mut(&id)
                .expect("defined style")
                .based_on_chain = chain;
        }
        Ok(())
    }
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
        // The cell mark terminates its final paragraph even after an explicit
        // par: that final paragraph is empty and remains structurally meaningful.
        self.paragraph(offset)?;
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
            merges: row.merges.clone(),
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
        let mut cells: Vec<TableCell> = vec![];
        let mut merge_active = false;
        for (cell, marker) in row.cells.into_iter().zip(row.merges) {
            if marker == Some(false) {
                if !merge_active {
                    return malformed(span.start, "horizontal merge continuation without restart");
                }
                let previous = cells.last_mut().expect("active merge has a cell");
                previous.props.grid_span =
                    Some(previous.props.grid_span.unwrap_or(1).checked_add(1).ok_or(
                        RtfError::Malformed {
                            offset: span.start,
                            reason: "horizontal merge span overflow",
                        },
                    )?);
                if let (Some(width), Some(extra)) = (&mut previous.props.width, &cell.props.width) {
                    if let (Some(n), Some(m)) = (width.value, extra.value) {
                        width.value = Some(n.checked_add(m).ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "horizontal merge width overflow",
                        })?);
                    }
                }
                let sole_empty =
                    matches!(cell.blocks.as_slice(), [Block::Paragraph(p)] if p.inlines.is_empty());
                if !sole_empty {
                    let mut incoming = cell.blocks.into_iter();
                    if let Some(mut first) = incoming.next() {
                        if let (Some(Block::Paragraph(last)), Block::Paragraph(first)) =
                            (previous.blocks.last_mut(), &mut first)
                        {
                            last.inlines.append(&mut first.inlines);
                        } else {
                            previous.blocks.push(first);
                        }
                    }
                    previous.blocks.extend(incoming);
                }
            } else {
                merge_active = marker == Some(true);
                cells.push(cell);
            }
        }
        let mut previous_merges = BTreeMap::new();
        if let Some(previous) = self.table.as_ref().and_then(|t| t.rows.last()) {
            let mut column = 0u32;
            for cell in &previous.cells {
                let end = column
                    .checked_add(u32::from(cell.props.grid_span.unwrap_or(1)))
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "merge grid overflow",
                    })?;
                if let Some(merge) = cell.props.vertical_merge {
                    previous_merges.insert((column, end), merge);
                }
                column = end;
            }
        }
        let mut column = 0u32;
        for cell in &cells {
            let end = column
                .checked_add(u32::from(cell.props.grid_span.unwrap_or(1)))
                .ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "merge grid overflow",
                })?;
            if cell.props.vertical_merge == Some(VerticalMerge::Continue)
                && !previous_merges.contains_key(&(column, end))
            {
                return malformed(
                    span.start,
                    "vertical merge continuation without matching predecessor",
                );
            }
            column = end;
        }
        self.table
            .as_mut()
            .expect("table initialized")
            .rows
            .push(TableRow {
                props: row.props,
                cells,
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
                // Font-encoded glyphs use w:sym, never an ANSI decoder.
                2 => Some(-2),
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
            .filter(|cp| matches!(*cp, -2 | 0) || codepage(*cp).is_some())
            .ok_or(RtfError::InvalidEncoding {
                offset: span.start,
                reason: "unsupported font charset/codepage",
            })?;
        let face = Arc::<str>::from(font.name.as_str());
        if let Some(slot) = self.state.font_slot {
            assign_font(
                self.state.run.fonts.get_or_insert_with(Fonts::default),
                slot,
                face,
            );
            self.state.codepage = cp;
            return Ok(());
        }
        let associated_face = self
            .state
            .associated_font
            .and_then(|associated_id| self.fonts.get(&associated_id))
            .map_or_else(
                || face.clone(),
                |associated| Arc::<str>::from(associated.name.as_str()),
            );
        self.state.run.fonts = Some(Fonts {
            ascii: Some(face.clone()),
            h_ansi: Some(face.clone()),
            east_asia: Some(associated_face),
            complex_script: Some(face),
            ..Default::default()
        });
        self.state.codepage = cp;
        Ok(())
    }
    fn select_associated_font(&mut self, id: i32, span: RtfSpan) -> Result<(), RtfError> {
        let Some(font) = self.fonts.get(&id) else {
            return self.issue(span, Severity::Loss, format!("undefined-font:{id}"));
        };
        let face = Arc::<str>::from(font.name.as_str());
        if let Some(slot) = self.state.font_slot {
            assign_font(
                self.state.run.fonts.get_or_insert_with(Fonts::default),
                slot,
                face,
            );
            return Ok(());
        }
        self.state.associated_font = Some(id);
        self.state
            .run
            .fonts
            .get_or_insert_with(Default::default)
            .east_asia = Some(face);
        Ok(())
    }
    fn set_language(&mut self, id: i32, span: RtfSpan, slot: LanguageSlot) -> Result<(), RtfError> {
        let Some(tag) = rtf_lcid_tag(id) else {
            return self.issue(span, Severity::Loss, format!("language:unknown-lcid:{id}"));
        };
        let language = self
            .state
            .run
            .language
            .get_or_insert_with(Language::default);
        match slot {
            LanguageSlot::Primary => language.val = Some(Arc::from(tag)),
            LanguageSlot::EastAsia => language.east_asia = Some(Arc::from(tag)),
            LanguageSlot::Bidi => language.bidi = Some(Arc::from(tag)),
        }
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
        if let Some(counts) = self.report.feature_counts.get_mut(&feature) {
            match severity {
                Severity::Info => counts.info_count += 1,
                Severity::Loss => counts.loss_count += 1,
            }
        } else if feature.len() <= super::MAX_FEATURE_NAME_BYTES
            && self.report.feature_counts.len() < super::MAX_FEATURE_COUNT_KEYS
        {
            let counts = self
                .report
                .feature_counts
                .entry(feature.clone())
                .or_default();
            match severity {
                Severity::Info => counts.info_count += 1,
                Severity::Loss => counts.loss_count += 1,
            }
        } else {
            self.report.feature_counts_truncated = true;
            match severity {
                Severity::Info => self.report.unattributed_info_count += 1,
                Severity::Loss => self.report.unattributed_loss_count += 1,
            }
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
        if self.state.destination == Destination::LevelText {
            let draft = self.level_draft.as_mut().ok_or(RtfError::Malformed {
                offset,
                reason: "leveltext outside list level",
            })?;
            bounded(
                draft.text.len() + text.len(),
                self.options.limits.input_bytes,
                offset,
                "leveltext_bytes",
            )?;
            draft.text.push_str(&text);
            return Ok(());
        }
        if matches!(
            self.state.destination,
            Destination::Lists | Destination::Overrides
        ) {
            if !text.trim().is_empty() {
                self.issue(
                    RtfSpan {
                        start: offset,
                        end: offset + 1,
                    },
                    Severity::Loss,
                    "list:unbound-text",
                )?;
            }
            return Ok(());
        }
        if self.state.destination == Destination::Styles {
            if text.chars().any(|c| !xml_char(c)) {
                return Err(RtfError::InvalidEncoding {
                    offset,
                    reason: "invalid style name character",
                });
            }
            if let Some(draft) = &mut self.style_draft {
                draft.name.push_str(&text);
            } else if !text.trim().is_empty() {
                self.issue(
                    RtfSpan {
                        start: offset,
                        end: offset + 1,
                    },
                    Severity::Loss,
                    "stylesheet:unbound-text",
                )?;
            }
            return Ok(());
        }
        if self.state.destination == Destination::FieldInstruction {
            if text.chars().any(|c| !xml_char(c)) {
                return Err(RtfError::InvalidEncoding {
                    offset,
                    reason: "invalid field instruction character",
                });
            }
            if let Some(RegionFrame {
                kind: RegionKind::Field { instruction },
                ..
            }) = self.regions.last_mut()
            {
                bounded(
                    instruction.len() + text.len(),
                    self.options.limits.input_bytes,
                    offset,
                    "field_instruction_bytes",
                )?;
                instruction.push_str(&text);
                return Ok(());
            }
            return malformed(offset, "field instruction outside a field");
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
        self.paragraph_pending = true;
        let nuls = text.chars().filter(|c| *c == '\0').count();
        let text = if nuls != 0 && self.options.nul_policy == NulPolicy::DropAndReport {
            self.report.discarded_nuls += nuls;
            self.issue(
                RtfSpan {
                    start: offset,
                    end: offset + 1,
                },
                Severity::Loss,
                "text:NUL:dropped",
            )?;
            text.chars().filter(|c| *c != '\0').collect::<String>()
        } else {
            text
        };
        if text.is_empty() {
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
    fn clear_note_marker(&mut self) -> Result<(), RtfError> {
        if let Some(span) = self.pending_note_marker.take() {
            self.issue(span, Severity::Loss, "note:unpaired-reference-marker")?;
        }
        Ok(())
    }
    fn content(&mut self, content: RunContent, offset: usize) -> Result<(), RtfError> {
        self.clear_note_marker()?;
        if matches!(
            content,
            RunContent::Tab
                | RunContent::Break(_)
                | RunContent::NoBreakHyphen
                | RunContent::SoftHyphen
                | RunContent::Symbol(_)
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
        self.clear_note_marker()?;
        self.paragraph_pending = false;
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
        if self.state.codepage == -2 {
            if !matches!(
                self.state.destination,
                Destination::Body | Destination::LevelText
            ) {
                let bytes = std::mem::take(&mut self.bytes);
                let encoding =
                    codepage(self.document_codepage).unwrap_or(encoding_rs::WINDOWS_1252);
                let (decoded, errors) = encoding.decode_without_bom_handling(&bytes);
                if errors {
                    return Err(RtfError::InvalidEncoding {
                        offset: self.bytes_offset,
                        reason: "invalid definition name encoding",
                    });
                }
                return self.text(decoded.into_owned(), self.bytes_offset);
            }
            let font = self
                .state
                .run
                .fonts
                .as_ref()
                .and_then(|f| f.ascii.clone())
                .expect("symbol encoding is set by a defined font");
            let bytes = std::mem::take(&mut self.bytes);
            if self.state.destination == Destination::LevelText {
                let text: String = bytes
                    .into_iter()
                    .map(|b| {
                        if b < 32 {
                            char::from(b)
                        } else {
                            char::from_u32(0xf000 + u32::from(b)).expect("PUA byte")
                        }
                    })
                    .collect();
                return self.text(text, self.bytes_offset);
            }
            for byte in bytes {
                self.content(
                    RunContent::Symbol(Symbol {
                        font: font.clone(),
                        character: char::from(byte),
                    }),
                    self.bytes_offset,
                )?;
            }
            return Ok(());
        }
        // An unspecified ANSI codepage is irrelevant for ASCII/Unicode-only
        // documents. Never guess a platform-dependent encoding for high bytes.
        if self.state.codepage == 0 && self.bytes.is_ascii() {
            let bytes = std::mem::take(&mut self.bytes);
            return self.text(
                String::from_utf8(bytes).expect("ASCII bytes"),
                self.bytes_offset,
            );
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
        let allow_unsigned =
            self.options.unicode_parameter_policy == UnicodeParameterPolicy::AllowUnsignedAndReport;
        let Some(n) = n.filter(|n| {
            (-32768..=32767).contains(n) || (allow_unsigned && (32768..=65535).contains(n))
        }) else {
            return malformed(
                span.start,
                "Unicode parameter must be a signed 16-bit integer",
            );
        };
        let unit = if n > 32767 {
            self.issue(span, Severity::Info, "unicode:unsigned-parameter")?;
            u16::try_from(n).expect("validated unsigned 16-bit parameter")
        } else {
            i16::try_from(n)
                .expect("validated signed 16-bit parameter")
                .cast_unsigned()
        };
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
        if first && name == "stylesheet" {
            self.state.ignorable = false;
            self.state.destination = Destination::Styles;
            return Ok(());
        }
        if first && matches!(name, "listtable" | "listoverridetable") {
            self.state.ignorable = false;
            self.state.destination = if name == "listtable" {
                Destination::Lists
            } else {
                Destination::Overrides
            };
            return Ok(());
        }
        if self.state.destination == Destination::Lists {
            match name {
                "list" => {
                    if self.list_draft.is_some() {
                        return malformed(span.start, "nested list declaration");
                    }
                    self.list_draft = Some(ListDraft {
                        depth: self.stack.len(),
                        id: None,
                        levels: vec![],
                    });
                    return Ok(());
                }
                "listid" => {
                    let id = n.map(i32::cast_unsigned).ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid listid",
                    })?;
                    self.list_draft
                        .as_mut()
                        .ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "listid outside list",
                        })?
                        .id = Some(id);
                    return Ok(());
                }
                "listlevel" => {
                    if self.level_draft.is_some() {
                        return malformed(span.start, "nested list level");
                    }
                    self.state.run = RunProperties::default();
                    self.state.paragraph = ParagraphProperties::default();
                    self.level_draft = Some(LevelDraft {
                        depth: self.stack.len(),
                        start: 1,
                        format: "decimal",
                        text: String::new(),
                        justification: Justification::Start,
                        suffix: "tab",
                    });
                    return Ok(());
                }
                "leveltext" => {
                    self.state.ignorable = false;
                    self.state.destination = Destination::LevelText;
                    return Ok(());
                }
                "levelnumbers" | "listname" => {
                    self.state.skip = true;
                    return Ok(());
                }
                "listtemplateid" | "listhybrid" | "listsimple" => return Ok(()),
                _ => (),
            }
            if let Some(level) = &mut self.level_draft {
                match name {
                    "levelnfc" | "levelnfcn" => {
                        if let Some(format) = n.and_then(number_format) {
                            level.format = format;
                        } else {
                            self.issue(
                                span,
                                Severity::Loss,
                                format!("list:unsupported-number-format:{n:?}"),
                            )?;
                        }
                        return Ok(());
                    }
                    "levelstartat" => {
                        level.start =
                            n.and_then(|n| u32::try_from(n).ok())
                                .ok_or(RtfError::Malformed {
                                    offset: span.start,
                                    reason: "invalid list start",
                                })?;
                        return Ok(());
                    }
                    "leveljc" | "leveljcn" => {
                        level.justification = match n {
                            Some(0) => Justification::Start,
                            Some(1) => Justification::Center,
                            Some(2) => Justification::End,
                            _ => return malformed(span.start, "invalid list alignment"),
                        };
                        return Ok(());
                    }
                    "levelfollow" => {
                        level.suffix = match n {
                            Some(0) => "tab",
                            Some(1) => "space",
                            Some(2) => "nothing",
                            _ => return malformed(span.start, "invalid list suffix"),
                        };
                        return Ok(());
                    }
                    "leveltemplateid" => return Ok(()),
                    _ => (),
                }
            }
        }
        if self.state.destination == Destination::Overrides {
            match name {
                "listoverride" => {
                    if self.override_draft.is_some() {
                        return malformed(span.start, "nested list override");
                    }
                    self.override_draft = Some(OverrideDraft {
                        depth: self.stack.len(),
                        list: None,
                        instance: None,
                    });
                    return Ok(());
                }
                "listid" | "ls" => {
                    let id = n
                        .and_then(|n| {
                            if name == "listid" {
                                Some(n.cast_unsigned())
                            } else {
                                u32::try_from(n).ok()
                            }
                        })
                        .ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "invalid list override id",
                        })?;
                    let draft = self.override_draft.as_mut().ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "override property outside declaration",
                    })?;
                    if name == "listid" {
                        draft.list = Some(id);
                    } else {
                        draft.instance = Some(id);
                    }
                    return Ok(());
                }
                "listoverridecount" if n == Some(0) => return Ok(()),
                _ => {
                    return self.issue(
                        span,
                        Severity::Loss,
                        format!("list-override-control:{name}"),
                    );
                }
            }
        }
        if self.state.destination == Destination::Styles {
            if matches!(name, "s" | "cs") {
                if self.style_draft.is_some() {
                    return malformed(span.start, "nested style declaration");
                }
                let id = n.filter(|n| *n >= 0).ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "invalid style id",
                })?;
                self.state.ignorable = false;
                self.state.run = RunProperties::default();
                self.state.paragraph = ParagraphProperties::default();
                self.style_draft = Some(StyleDraft {
                    depth: self.stack.len(),
                    id: StyleId::new(format!("rtf-{name}{id}")),
                    kind: if name == "s" {
                        StyleType::Paragraph
                    } else {
                        StyleType::Character
                    },
                    name: String::new(),
                    parent: None,
                    next: None,
                    link: None,
                    priority: None,
                    hidden: false,
                    semi_hidden: false,
                    quick: false,
                });
                return Ok(());
            }
            if let Some(draft) = &mut self.style_draft {
                let prefix = if draft.kind == StyleType::Paragraph {
                    "s"
                } else {
                    "cs"
                };
                match name {
                    "sbasedon" => {
                        draft.parent = n
                            .filter(|n| *n >= 0)
                            .map(|n| StyleId::new(format!("rtf-{prefix}{n}")));
                        return Ok(());
                    }
                    "snext" => {
                        draft.next = n
                            .filter(|n| *n >= 0)
                            .map(|n| StyleId::new(format!("rtf-{prefix}{n}")));
                        return Ok(());
                    }
                    "slink" => {
                        let other = if prefix == "s" { "cs" } else { "s" };
                        draft.link = n
                            .filter(|n| *n >= 0)
                            .map(|n| StyleId::new(format!("rtf-{other}{n}")));
                        return Ok(());
                    }
                    "spriority" => {
                        draft.priority = n;
                        return Ok(());
                    }
                    "shidden" => {
                        draft.hidden = n != Some(0);
                        return Ok(());
                    }
                    "ssemihidden" => {
                        draft.semi_hidden = n != Some(0);
                        return Ok(());
                    }
                    "sqformat" => {
                        draft.quick = n != Some(0);
                        return Ok(());
                    }
                    _ => (),
                }
            }
        }
        if first && self.state.destination == Destination::Body {
            if matches!(
                name,
                "header"
                    | "headerl"
                    | "headerr"
                    | "headerf"
                    | "footer"
                    | "footerl"
                    | "footerr"
                    | "footerf"
            ) {
                let kind = if name.ends_with('f') {
                    HeaderFooterKind::First
                } else if name.ends_with('l') {
                    HeaderFooterKind::Even
                } else {
                    HeaderFooterKind::Default
                };
                self.state.ignorable = false;
                return self.begin_region(
                    RegionKind::Header {
                        is_header: name.starts_with("header"),
                        kind,
                    },
                    span.start,
                );
            }
            if name == "footnote" {
                self.state.ignorable = false;
                return self.begin_region(RegionKind::Note { endnote: false }, span.start);
            }
            if name == "field" {
                self.state.ignorable = false;
                return self.begin_region(
                    RegionKind::Field {
                        instruction: String::new(),
                    },
                    span.start,
                );
            }
            if name == "fldinst"
                && self
                    .regions
                    .last()
                    .is_some_and(|r| matches!(r.kind, RegionKind::Field { .. }))
            {
                self.state.ignorable = false;
                self.state.destination = Destination::FieldInstruction;
                return Ok(());
            }
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
                "wmetafile" => {
                    if !n.is_some_and(|mode| (1..=8).contains(&mode)) {
                        return malformed(span.start, "invalid WMF mapping mode");
                    }
                    picture.kind = Some(MediaKind::Wmf);
                    self.issue(
                        span,
                        Severity::Info,
                        "picture:wmf-retained-rendering-separate",
                    )?;
                }
                "emfblip" => {
                    picture.kind = Some(MediaKind::Emf);
                    self.issue(
                        span,
                        Severity::Info,
                        "picture:emf-retained-rendering-separate",
                    )?;
                }
                "picw" => picture.width = n,
                "pich" => picture.height = n,
                "picwgoal" => picture.width_goal = n,
                "pichgoal" => picture.height_goal = n,
                "picscalex" => picture.scale_x = n,
                "picscaley" => picture.scale_y = n,
                "dibitmap" | "wbitmap" | "macpict" | "pmmetafile" => {
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
        if self.state.destination == Destination::FieldInstruction {
            match name {
                "u" => return self.unicode(n, span),
                "uc" => {
                    self.state.uc = n
                        .and_then(|n| usize::try_from(n).ok())
                        .filter(|n| *n <= 32767)
                        .ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "invalid field fallback count",
                        })?;
                    return Ok(());
                }
                _ => {
                    return self.issue(
                        span,
                        Severity::Loss,
                        format!("field-instruction-control:{name}"),
                    );
                }
            }
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
        if (!self.regions.is_empty() || self.state.destination == Destination::Styles)
            && matches!(
                name,
                "sectd"
                    | "sbknone"
                    | "sbkpage"
                    | "sbkcol"
                    | "sbkeven"
                    | "sbkodd"
                    | "paperw"
                    | "paperh"
                    | "pgwsxn"
                    | "pghsxn"
                    | "margl"
                    | "margr"
                    | "margt"
                    | "margb"
                    | "marglsxn"
                    | "margrsxn"
                    | "margtsxn"
                    | "margbsxn"
                    | "headery"
                    | "footery"
                    | "gutter"
                    | "landscape"
                    | "lndscpsxn"
            )
        {
            return self.issue(
                span,
                Severity::Loss,
                format!("section-control-outside-body:{name}"),
            );
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
                let Some(cp) = n.filter(|cp| *cp == 0 || codepage(*cp).is_some()) else {
                    return Err(RtfError::InvalidEncoding {
                        offset: span.start,
                        reason: "unsupported codepage",
                    });
                };
                self.state.codepage = cp;
                self.document_codepage = cp;
            }
            "deff" => self.default_font = n,
            "loch" => self.state.font_slot = Some(FontSlot::Ascii),
            "hich" => self.state.font_slot = Some(FontSlot::HighAnsi),
            "dbch" => self.state.font_slot = Some(FontSlot::EastAsia),
            "fcs" => match n {
                Some(0) => self.state.font_slot = None,
                Some(1) => self.state.font_slot = Some(FontSlot::ComplexScript),
                Some(value) => {
                    self.state.font_slot = None;
                    self.issue(span, Severity::Loss, format!("font-script-class:{value}"))?;
                }
                None => {
                    return malformed(span.start, "missing font script class");
                }
            },
            "lang" | "langnp" | "langfe" | "langfenp" | "alang" => {
                let id = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing language id",
                })?;
                let slot = match name {
                    "langfe" | "langfenp" => LanguageSlot::EastAsia,
                    "alang" => LanguageSlot::Bidi,
                    _ => LanguageSlot::Primary,
                };
                self.set_language(id, span, slot)?;
            }
            "af" => {
                let id = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing associated font id",
                })?;
                self.select_associated_font(id, span)?;
            }
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
                self.state.associated_font = None;
                self.state.font_slot = None;
                self.state.codepage = self.document_codepage;
                if let Some(id) = self.default_font {
                    self.select_font(id, span)?;
                }
            }
            "pard" => {
                self.state.paragraph = ParagraphProperties::default();
                self.state.tab_alignment = TabAlignment::Start;
                self.state.tab_leader = None;
                self.state.line_spacing = 0;
                self.state.line_multiple = false;
            }
            "s" | "cs" => {
                let id = n.filter(|n| *n >= 0).ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "invalid style reference",
                })?;
                let id = StyleId::new(format!("rtf-{name}{id}"));
                if !self.styles.contains(&id) {
                    self.issue(span, Severity::Loss, format!("style:undefined:{id}"))?;
                }
                if name == "s" {
                    self.state.paragraph.style = Some(id);
                } else {
                    self.state.run.style = Some(id);
                }
            }
            "ls" => {
                let id = n
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid list reference",
                    })?;
                self.state
                    .paragraph
                    .numbering
                    .get_or_insert_with(NumPr::default)
                    .num_id = Some(NumId(id));
                if id != 0
                    && self.state.destination == Destination::Body
                    && self.numbering.num(NumId(id)).is_none()
                {
                    self.issue(
                        span,
                        Severity::Loss,
                        format!("list:undefined-instance:{id}"),
                    )?;
                }
            }
            "ilvl" => {
                let level = n
                    .and_then(|n| u8::try_from(n).ok())
                    .filter(|n| *n < 9)
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid list level",
                    })?;
                self.state
                    .paragraph
                    .numbering
                    .get_or_insert_with(NumPr::default)
                    .ilvl = Some(Ilvl(level));
            }
            "ftnalt" => {
                if let Some(RegionFrame {
                    kind: RegionKind::Note { endnote },
                    ..
                }) = self.regions.last_mut()
                {
                    *endnote = true;
                } else {
                    self.issue(span, Severity::Loss, "ftnalt:outside-note")?;
                }
            }
            "chftn" => {
                if self
                    .regions
                    .last()
                    .is_some_and(|r| matches!(r.kind, RegionKind::Note { .. }))
                {
                    self.content(RunContent::NoteRef, span.start)?;
                } else {
                    self.clear_note_marker()?;
                    self.pending_note_marker = Some(span);
                }
            }
            "tqc" => self.state.tab_alignment = TabAlignment::Center,
            "tqr" => self.state.tab_alignment = TabAlignment::End,
            "tqdec" => self.state.tab_alignment = TabAlignment::Decimal,
            "tb" => self.state.tab_alignment = TabAlignment::Bar,
            "tldot" => self.state.tab_leader = Some(TabLeader::Dot),
            "tlhyph" => self.state.tab_leader = Some(TabLeader::Hyphen),
            "tlul" => self.state.tab_leader = Some(TabLeader::Underscore),
            "tx" => {
                let position = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing tab position",
                })?;
                bounded(
                    self.state.paragraph.tabs.len() + 1,
                    self.options.limits.definitions,
                    span.start,
                    "tab_stops",
                )?;
                self.state
                    .paragraph
                    .tabs
                    .retain(|t| t.position != Twips(position));
                self.state.paragraph.tabs.push(TabStop {
                    position: Twips(position),
                    alignment: self.state.tab_alignment,
                    leader: self.state.tab_leader,
                });
                self.state.tab_alignment = TabAlignment::Start;
                self.state.tab_leader = None;
            }
            "widctlpar" => self.state.paragraph.widow_control = toggle,
            "nowidctlpar" => self.state.paragraph.widow_control = TriState::Off,
            "outlinelevel" => {
                self.state.paragraph.outline_level =
                    n.and_then(|n| u8::try_from(n).ok()).filter(|n| *n <= 9);
            }
            "sectd" => {
                self.section = SectionProperties {
                    page_size: Some(self.document_page_size),
                    page_margins: Some(self.document_page_margins),
                    ..Default::default()
                }
            }
            "sbknone" | "sbkpage" | "sbkcol" | "sbkeven" | "sbkodd" => {
                self.section.section_type = Some(match name {
                    "sbknone" => SectionType::Continuous,
                    "sbkcol" => SectionType::NextColumn,
                    "sbkeven" => SectionType::EvenPage,
                    "sbkodd" => SectionType::OddPage,
                    _ => SectionType::NextPage,
                });
            }
            "sect" => {
                if !self.regions.is_empty() {
                    return malformed(span.start, "section break inside region");
                }
                if self.row.is_some() {
                    return malformed(span.start, "section break inside a table row");
                }
                self.finish_high()?;
                self.paragraph(span.start)?;
                let Block::Paragraph(paragraph) = self.blocks.last_mut().expect("new paragraph")
                else {
                    unreachable!("paragraph terminator appends a paragraph");
                };
                paragraph.props.section = Some(self.section.clone());
                self.node(1, span.start)?;
                self.sections.push(Section {
                    properties: self.section.clone(),
                    location: SourceLocation::unknown(),
                });
            }
            "b" => self.state.run.bold = toggle,
            "ab" => self.state.run.bold_cs = toggle,
            "i" => self.state.run.italic = toggle,
            "ai" => self.state.run.italic_cs = toggle,
            "strike" => self.state.run.strike = toggle,
            "striked" => self.state.run.double_strike = toggle,
            "caps" => self.state.run.caps = toggle,
            "scaps" => self.state.run.small_caps = toggle,
            "v" => self.state.run.vanish = toggle,
            "rtlch" => {
                self.state.run.rtl = TriState::On;
                self.state.font_slot = Some(FontSlot::ComplexScript);
            }
            "ltrch" => {
                self.state.run.rtl = TriState::Off;
                self.state.font_slot = None;
            }
            "ul" => {
                self.state.run.underline = Some(if n == Some(0) {
                    Underline::None
                } else {
                    Underline::Single
                });
            }
            "ulnone" => self.state.run.underline = Some(Underline::None),
            "uldb" => self.state.run.underline = Some(Underline::Double),
            "fs" | "afs" => {
                let Some(size) = n.filter(|n| *n > 0) else {
                    return malformed(span.start, "invalid font size");
                };
                if name == "afs" {
                    self.state.run.size_cs = Some(HalfPoints(size));
                } else {
                    self.state.run.size = Some(HalfPoints(size));
                }
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
                    row.merges.clear();
                    row.next_merge = None;
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
                row.merges.push(row.next_merge.take());
            }
            "clmgf" | "clmrg" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                row.next_merge = Some(name == "clmgf");
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
            "itap" | "fldrslt" => (),
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
            "sl" | "slmult" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing line spacing",
                })?;
                if name == "sl" {
                    self.state.line_spacing = value;
                } else {
                    if !matches!(value, 0 | 1) {
                        return malformed(span.start, "invalid line spacing multiplier");
                    }
                    self.state.line_multiple = value == 1;
                }
                let spacing = self.state.line_spacing;
                let props = self
                    .state
                    .paragraph
                    .spacing
                    .get_or_insert_with(Default::default);
                props.line = Some(Twips(if spacing == 0 {
                    240
                } else {
                    spacing.checked_abs().ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "line spacing overflow",
                    })?
                }));
                props.line_rule = Some(if self.state.line_multiple || spacing == 0 {
                    LineSpacingRule::Auto
                } else if spacing < 0 {
                    LineSpacingRule::Exact
                } else {
                    LineSpacingRule::AtLeast
                });
            }
            "sbauto" | "saauto" => {
                let props = self
                    .state
                    .paragraph
                    .spacing
                    .get_or_insert_with(Default::default);
                if name == "sbauto" {
                    props.before_autospacing = n != Some(0);
                } else {
                    props.after_autospacing = n != Some(0);
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
                match name {
                    "paperw" => self.document_page_size.width = Some(Twips(value)),
                    "paperh" => self.document_page_size.height = Some(Twips(value)),
                    _ => (),
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
                match name {
                    "margl" => self.document_page_margins.left = Some(Twips(value)),
                    "margr" => self.document_page_margins.right = Some(Twips(value)),
                    "margt" => self.document_page_margins.top = Some(Twips(value)),
                    "margb" => self.document_page_margins.bottom = Some(Twips(value)),
                    "gutter" => self.document_page_margins.gutter = Some(Twips(value)),
                    _ => (),
                }
            }
            "landscape" | "lndscpsxn" => {
                if name == "landscape" {
                    self.document_page_size.orientation = Some(PageOrientation::Landscape);
                }
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
            _ => {
                self.issue(span, Severity::Loss, format!("control:{name}"))?;
                if self.options.compatibility_profile == CompatibilityProfile::WpsText
                    && !first
                    && ((name == "CocoaLigature" && n == Some(1))
                        || (name == "AppleTypeServices" && n.is_none()))
                    && !self.inlines.is_empty()
                {
                    let literal = n.map_or_else(|| format!("{name} "), |n| format!("{name}{n} "));
                    self.issue(span, Severity::Loss, "wps:literal-unknown-inline-control")?;
                    self.text(literal, span.start)?;
                }
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
fn rtf_lcid_tag(id: i32) -> Option<&'static str> {
    Some(match id {
        9 => "en",
        10 => "es",
        11 => "fi",
        22 => "pt",
        31 => "tr",
        1025 => "ar-SA",
        1029 => "cs-CZ",
        1031 => "de-DE",
        1033 => "en-US",
        1034 | 3082 => "es-ES",
        1035 => "fi-FI",
        1036 => "fr-FR",
        1042 => "ko-KR",
        1045 => "pl-PL",
        1049 => "ru-RU",
        1053 => "sv-SE",
        1055 => "tr-TR",
        1058 => "uk-UA",
        1081 => "hi-IN",
        1093 => "bn-IN",
        2052 => "zh-CN",
        2057 => "en-GB",
        2070 => "pt-PT",
        3073 => "ar-EG",
        4105 => "en-CA",
        4108 => "fr-CH",
        7169 => "ar-TN",
        10241 => "ar-SY",
        16393 => "en-IN",
        _ => return None,
    })
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
    // RTF geometry is not the same as the renderer's OOXML fallback. Preserve
    // these document defaults explicitly, including through section resets.
    let document_page_size = PageSize {
        width: Some(Twips(12240)),
        height: Some(Twips(15840)),
        orientation: None,
        code: None,
    };
    let document_page_margins = PageMargins {
        left: Some(Twips(1800)),
        right: Some(Twips(1800)),
        top: Some(Twips(1440)),
        bottom: Some(Twips(1440)),
        header: Some(Twips(720)),
        footer: Some(Twips(720)),
        gutter: Some(Twips(0)),
    };
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
        paragraph_pending: false,
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
        pending_note_marker: None,
        document_codepage: 1252,
        colors: vec![],
        rgb: [0; 3],
        color_set: false,
        section: SectionProperties {
            page_size: Some(document_page_size),
            page_margins: Some(document_page_margins),
            ..Default::default()
        },
        document_page_size,
        document_page_margins,
        sections: vec![],
        row: None,
        table: None,
        table_rows: 0,
        table_cells: 0,
        row_template: None,
        picture: None,
        media: vec![],
        media_index: MediaIndex::new(),
        media_bytes: 0,
        styles: StyleTable::new(),
        style_draft: None,
        regions: vec![],
        headers_footers: vec![],
        footnotes: NoteTable::new(),
        endnotes: NoteTable::new(),
        numbering: NumberingTable::new(),
        list_draft: None,
        level_draft: None,
        override_draft: None,
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
                p.state.literal_prefix = false;
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
                // WPS ignores leading header whitespace when the first
                // meaningful token is a control word, but preserves it in a
                // literal group (including before control symbols like ~).
                let remaining = &input[lexer.pos..];
                let head = remaining
                    .iter()
                    .position(|b| !b.is_ascii_whitespace())
                    .unwrap_or(remaining.len());
                p.state.literal_prefix = !(remaining.get(head) == Some(&b'\\')
                    && remaining
                        .get(head + 1)
                        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'*'));
                if child_index == Some(1) {
                    p.state.skip = true;
                }
            }
            Token::Close => {
                p.flush()?;
                p.fallback = 0;
                if p.level_draft
                    .as_ref()
                    .is_some_and(|d| d.depth == p.stack.len())
                {
                    p.finish_high()?;
                    p.finish_level(span.start)?;
                }
                if p.list_draft
                    .as_ref()
                    .is_some_and(|d| d.depth == p.stack.len())
                {
                    p.finish_list(span.start)?;
                }
                if p.override_draft
                    .as_ref()
                    .is_some_and(|d| d.depth == p.stack.len())
                {
                    p.finish_override(span.start)?;
                }
                if p.style_draft
                    .as_ref()
                    .is_some_and(|d| d.depth == p.stack.len())
                {
                    p.finish_high()?;
                    p.finish_style(span.start)?;
                }
                let region_inline = if p.regions.last().is_some_and(|r| r.depth == p.stack.len()) {
                    p.end_region(span)?
                } else {
                    None
                };
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
                if let Some(mut inline) = region_inline {
                    if let Inline::Run(run) = &mut inline {
                        run.props = p.state.run.clone();
                    }
                    if p.inlines.is_empty() {
                        p.paragraph_props = p.state.paragraph.clone();
                    }
                    p.paragraph_pending = true;
                    p.inlines.push(inline);
                }
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
                        if p.state.first
                            && !p.state.literal_prefix
                            && byte.is_ascii_whitespace()
                            && p.state.destination == Destination::Body
                            && options.compatibility_profile == CompatibilityProfile::WpsText
                        {
                            if matches!(byte, b' ' | b'\t') {
                                p.issue(
                                    RtfSpan {
                                        start: span.start + index,
                                        end: span.start + index + 1,
                                    },
                                    Severity::Loss,
                                    "wps:header-whitespace:dropped",
                                )?;
                            }
                            continue;
                        }
                        if matches!(byte, b'\r' | b'\n')
                            && p.state.literal_prefix
                            && p.state.destination == Destination::Body
                            && options.compatibility_profile == CompatibilityProfile::WpsText
                        {
                            p.byte(b'\n', span.start + index)?;
                            p.issue(
                                RtfSpan {
                                    start: span.start + index,
                                    end: span.start + index + 1,
                                },
                                Severity::Loss,
                                "wps:leading-group-line-ending",
                            )?;
                        }
                        if !matches!(byte, b'\r' | b'\n') {
                            if !byte.is_ascii_whitespace() {
                                p.state.literal_prefix = false;
                            }
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
                    p.state.literal_prefix = false;
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
                p.state.literal_prefix = false;
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
                    b'-' if options.compatibility_profile == CompatibilityProfile::WpsText => {
                        p.issue(span, Severity::Loss, "wps:soft-hyphen-to-hyphen")?;
                        p.text("-".into(), span.start)?;
                    }
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
    p.clear_note_marker()?;
    p.finish_table();
    if p.paragraph_pending
        || !p.inlines.is_empty()
        || p.blocks.is_empty()
        || matches!(p.blocks.last(), Some(Block::Table(_)))
    {
        p.paragraph(lexer.pos)?;
    }
    p.resolve_style_chains(lexer.pos)?;
    // Overrides may precede their definitions. Validate only after the entire
    // input has been read, and retain a loss even when diagnostic details cap.
    let unresolved_lists: Vec<_> = p
        .numbering
        .nums()
        .filter_map(|num| {
            p.numbering
                .abstract_num(num.abstract_num_id)
                .is_none()
                .then_some(num.abstract_num_id.0)
        })
        .collect();
    for id in unresolved_lists {
        p.issue(
            RtfSpan {
                start: lexer.pos,
                end: lexer.pos,
            },
            Severity::Loss,
            format!("numbering:undefined-abstract:{id}"),
        )?;
    }
    p.sections.push(Section {
        properties: p.section,
        location: SourceLocation::unknown(),
    });
    let default_fonts = p.default_font.and_then(|id| p.fonts.get(&id)).map(|f| {
        let name = Arc::<str>::from(f.name.as_str());
        Fonts {
            ascii: Some(name.clone()),
            h_ansi: Some(name.clone()),
            east_asia: Some(name.clone()),
            complex_script: Some(name),
            ..Default::default()
        }
    });
    p.styles
        .set_defaults(strict_ooxml_wml::model::styles::DocDefaults {
            run: RunProperties {
                size: Some(HalfPoints(24)),
                size_cs: Some(HalfPoints(24)),
                fonts: default_fonts,
                ..Default::default()
            },
            paragraph: Default::default(),
        });
    let font_table = (!p.fonts.is_empty()).then(|| strict_ooxml_wml::model::fonts::FontTable {
        fonts: p
            .fonts
            .values()
            .map(|f| strict_ooxml_wml::model::fonts::FontEntry {
                name: Arc::from(f.name.as_str()),
                hints: Default::default(),
                embeds: Default::default(),
            })
            .collect(),
    });
    let font_table_part = font_table
        .as_ref()
        .map(|_| PartId::new("/word/fontTable.xml"));
    let styles_part = Some(PartId::new("/word/styles.xml"));
    let footnotes_part = (!p.footnotes.is_empty()).then(|| PartId::new("/word/footnotes.xml"));
    let endnotes_part = (!p.endnotes.is_empty()).then(|| PartId::new("/word/endnotes.xml"));
    let numbering_part = (!p.numbering.is_empty()).then(|| PartId::new("/word/numbering.xml"));
    let document = Document {
        body: Body { blocks: p.blocks },
        styles: p.styles,
        numbering: p.numbering,
        footnotes: p.footnotes,
        endnotes: p.endnotes,
        settings: Default::default(),
        theme: None,
        font_table,
        sections: p.sections,
        headers_footers: p.headers_footers,
        media: p.media_index,
        support: Default::default(),
        source: DocumentSource {
            main_document: PartId::new("/word/document.xml"),
            styles: styles_part,
            numbering: numbering_part,
            settings: None,
            font_table: font_table_part,
            footnotes: footnotes_part,
            endnotes: endnotes_part,
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
