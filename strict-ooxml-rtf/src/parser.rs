#![allow(
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::default_trait_access
)]
use crate::lists::{number_format, LevelDraft, ListDraft, OverrideDraft, OverrideLevelDraft};
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
    AbstractNum, AbstractNumId, Block, Bookmark, BookmarkId, Border, BorderStyle, BreakKind,
    CellMargins, CellProperties, Color, ColumnSpec, EighthsPoint, Field, FieldChar, FieldCharType,
    Fonts, GridCol, HalfPoints, HeaderFooter, HeaderFooterKind, HeaderFooterRef, HeightRule, Ilvl,
    Inline, Justification, Language, Level, LevelOverride, LineSpacingRule, Note, NoteKind,
    NoteProperties, NoteTable, Num, NumId, NumPr, NumberingTable, PageBorder, PageBorders,
    PageMargins, PageOrientation, PageSize, Paragraph, ParagraphProperties, RowHeight,
    RowProperties, Rsids, Run, RunContent, RunProperties, Section, SectionProperties, SectionType,
    Shading, Space, Style, StyleId, StyleTable, StyleType, Symbol, TabAlignment, TabLeader,
    TabStop, Table, TableCell, TableLayout, TableLook, TableProperties, TableRow, TextDirection,
    TextNode, TriState, Twips, Underline, VertAlign, VerticalJc, VerticalMerge, Width, WidthKind,
};
use unicode_bidi::BidiInfo;

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
/// Which border collection a running RTF border edge writes into.
#[derive(Clone, Copy)]
enum BorderScope {
    Paragraph,
    Cell,
    Row,
    Run,
    Page,
}
/// One edge of a `Borders` collection.
#[derive(Clone, Copy)]
enum BorderSide {
    Top,
    Bottom,
    Start,
    End,
    InsideHorizontal,
    InsideVertical,
    Bar,
    Between,
    TopLeftToBottomRight,
    BottomLeftToTopRight,
}
/// The edge a trailing border style/width/colour/space token applies to.
#[derive(Clone, Copy)]
struct BorderTarget {
    scope: BorderScope,
    side: BorderSide,
}
/// Where an RTF shading control (`\ch*`/`\cl*`/`\tr*`/paragraph) lands.
#[derive(Clone, Copy)]
enum ShadingScope {
    Paragraph,
    Run,
    Cell,
    Row,
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
    NoteSeparator {
        endnote: bool,
        kind: NoteKind,
    },
    Field {
        instruction: String,
        /// `\flddirty`: the cached result must be recomputed.
        dirty: bool,
    },
    Bookmark {
        is_start: bool,
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
    theme: Option<FontTheme>,
}
#[derive(Clone, Copy)]
struct FontTheme {
    slot: FontSlot,
    reference: &'static str,
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
    /// Explicit cell width from `\clftsWidth`/`\clwWidth` for the next `\cellx`.
    next_width_kind: Option<WidthKind>,
    next_width_value: Option<i32>,
    /// Row background shading (`\trcbpat`/`\trcfpat`/`\trshdng`).
    shading: Option<Shading>,
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
    /// Edge that trailing `<style> \brdrw? \brsp? \brdrcf?` tokens configure.
    current_border: Option<BorderTarget>,
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
            current_border: None,
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

/// Which note role a `\ftn*`/`\aftn*` control configures.
#[derive(Clone, Copy)]
enum NoteRole {
    Footnote,
    Endnote,
}

/// The `NoteProperties` field an RTF note control sets.
#[derive(Clone, Copy)]
enum NoteProperty {
    /// Placement (`w:pos`).
    Position(&'static str),
    /// Number format (`w:numFmt`).
    Format(&'static str),
    /// Restart mode (`w:numRestart`).
    Restart(&'static str),
    /// Starting number (`w:numStart`); the value is the control word parameter.
    Start,
}

struct NoteControl {
    role: NoteRole,
    property: NoteProperty,
}

/// Classifies a footnote/endnote control word without its section `s` prefix.
///
/// Follows the RTF 1.5 "Footnotes and Endnotes" table. Position values are the
/// Strict `ST_FtnPos`/`ST_EdnPos` lexical forms; an endnote has no
/// `pageBottom`/`beneathText` position, so `\aftnbj`/`\aftntj` are intentionally
/// left unrecognized.
fn note_control_base(name: &str) -> Option<NoteControl> {
    use NoteProperty::{Format, Position, Restart, Start};
    use NoteRole::{Endnote, Footnote};
    let (role, property) = match name {
        "ftnnar" => (Footnote, Format("decimal")),
        "ftnnalc" => (Footnote, Format("lowerLetter")),
        "ftnnauc" => (Footnote, Format("upperLetter")),
        "ftnnrlc" => (Footnote, Format("lowerRoman")),
        "ftnnruc" => (Footnote, Format("upperRoman")),
        "ftnnchi" => (Footnote, Format("chicago")),
        "aftnnar" => (Endnote, Format("decimal")),
        "aftnnalc" => (Endnote, Format("lowerLetter")),
        "aftnnauc" => (Endnote, Format("upperLetter")),
        "aftnnrlc" => (Endnote, Format("lowerRoman")),
        "aftnnruc" => (Endnote, Format("upperRoman")),
        "aftnnchi" => (Endnote, Format("chicago")),
        "ftnbj" => (Footnote, Position("pageBottom")),
        "ftntj" => (Footnote, Position("beneathText")),
        "endnotes" => (Footnote, Position("sectEnd")),
        "enddoc" => (Footnote, Position("docEnd")),
        "aendnotes" => (Endnote, Position("sectEnd")),
        "aenddoc" => (Endnote, Position("docEnd")),
        "ftnstart" => (Footnote, Start),
        "aftnstart" => (Endnote, Start),
        "ftnrstcont" => (Footnote, Restart("continuous")),
        "ftnrestart" => (Footnote, Restart("eachSect")),
        "ftnrstpg" => (Footnote, Restart("eachPage")),
        "aftnrstcont" => (Endnote, Restart("continuous")),
        "aftnrestart" => (Endnote, Restart("eachSect")),
        _ => return None,
    };
    Some(NoteControl { role, property })
}

/// Character-formatting control words the writer embeds inside `\fldinst`.
///
/// They format the field-code display, not the instruction text, so the
/// importer drops them. This set is deliberately curated: a control word that is
/// not a known run/paragraph formatting property stays an explicit `Loss`
/// instead of being renamed away wholesale.
fn is_field_instruction_formatting(name: &str) -> bool {
    matches!(
        name,
        // Fonts, script slots and languages.
        "f" | "af" | "fs" | "afs" | "fcs" | "loch" | "hich" | "dbch"
            | "ltrch" | "rtlch" | "lang" | "langnp" | "langfe" | "langfenp" | "alang"
        // Colour and highlight.
            | "cf" | "cb" | "chcbpat" | "highlight"
        // Bold/italic/underline/strike and other character toggles.
            | "b" | "ab" | "i" | "ai" | "ul" | "ulnone" | "uld" | "uldb" | "ulth"
            | "ulw" | "ulwave" | "strike" | "striked" | "caps" | "scaps" | "v"
            | "outl" | "shad" | "embo" | "impr" | "noproof" | "snaptogrid" | "cs"
            | "webhidden" | "super" | "sub" | "nosupersub"
        // Size, position, spacing and scaling.
            | "expnd" | "expndtw" | "charscalex" | "kern" | "position" | "up" | "dn"
        // Revision save ids.
            | "insrsid" | "charrsid"
    )
}

/// A run holding one complex-field marker (`w:fldChar`).
fn field_char_marker(kind: FieldCharType, dirty: bool) -> Inline {
    Inline::Run(Run {
        props: RunProperties::default(),
        content: vec![RunContent::FieldChar(FieldChar { kind, dirty })],
        revision: None,
        location: SourceLocation::unknown(),
    })
}

fn assign_font(fonts: &mut Fonts, slot: FontSlot, face: Arc<str>) {
    match slot {
        FontSlot::Ascii => fonts.ascii = Some(face),
        FontSlot::HighAnsi => fonts.h_ansi = Some(face),
        FontSlot::EastAsia => fonts.east_asia = Some(face),
        FontSlot::ComplexScript => fonts.complex_script = Some(face),
    }
}
fn assign_font_theme(fonts: &mut Fonts, slot: FontSlot, reference: &'static str) {
    let reference: Arc<str> = Arc::from(reference);
    match slot {
        FontSlot::Ascii => fonts.ascii_theme = Some(reference),
        FontSlot::HighAnsi => fonts.h_ansi_theme = Some(reference),
        FontSlot::EastAsia => fonts.east_asia_theme = Some(reference),
        FontSlot::ComplexScript => fonts.cs_theme = Some(reference),
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
    font_theme: Option<FontTheme>,
    pending_font_theme: Option<FontTheme>,
    single_font_active: bool,
    default_font: Option<i32>,
    pending_note_marker: Option<RtfSpan>,
    document_codepage: i32,
    colors: Vec<Option<Color>>,
    rgb: [u8; 3],
    color_set: bool,
    section: SectionProperties,
    /// A `\sectd` reset whose effect is deferred to the section boundary.
    ///
    /// `\sectd` resets the running section properties. Word writes it *after*
    /// the `\sect` that ends a section (`\sect\sectd`), but WPS writes it
    /// *before* (`\sectd\sect`). In the latter order an immediate reset would
    /// wipe the headers/footers and geometry a section accumulated before that
    /// section is committed at the following `\sect` (072 lost every header but
    /// the last). Deferring the reset past the next `\sect` makes both orders
    /// commit the section that actually preceded the break.
    section_reset_pending: bool,
    document_page_size: PageSize,
    document_page_margins: PageMargins,
    sections: Vec<Section>,
    row: Option<RowBuilder>,
    table: Option<Table>,
    /// `w:tblLook` flags accumulated from `\tbllk*` (applied at table creation).
    table_look: TableLook,
    /// `w:bidiVisual` from `\rtlrow`/`\ltrrow`.
    table_rtl: bool,
    table_rows: usize,
    table_cells: usize,
    /// Table default cell half-gap from `\trgaph` (left/right cell margins).
    table_gap: Option<Twips>,
    /// Table default cell padding from `\trpadd{t,b,l,r}` (`w:tblCellMar`).
    table_padding: CellMargins,
    /// Table layout from `\trautofit` (`w:tblLayout`), applied at creation.
    table_layout: Option<TableLayout>,
    /// Document default tab stop from `\pardeftab` (`w:defaultTabStop`).
    default_tab_stop: Option<Twips>,
    row_template: Option<RowBuilder>,
    picture: Option<PictureBuilder>,
    media: Vec<(PartId, Vec<u8>)>,
    media_index: MediaIndex,
    media_bytes: usize,
    styles: StyleTable,
    style_draft: Option<StyleDraft>,
    regions: Vec<RegionFrame>,
    headers_footers: Vec<HeaderFooter>,
    /// Bookmark name to `w:id`, so a `\bkmkstart`/`\bkmkend` pair shares an id.
    bookmarks: std::collections::HashMap<String, u32>,
    bookmark_seq: u32,
    /// An even-page (`\headerl`/`\footerl`) region was seen: `w:evenAndOddHeaders`.
    even_and_odd_headers: bool,
    /// Column index (0-based) that `\colw`/`\colsr` describe.
    column_index: usize,
    footnotes: NoteTable,
    endnotes: NoteTable,
    /// Document-wide footnote numbering/placement (`\ftn*`/`\enddoc`).
    document_footnote: NoteProperties,
    /// Document-wide endnote numbering/placement (`\aftn*`/`\aenddoc`).
    document_endnote: NoteProperties,
    numbering: NumberingTable,
    list_draft: Option<ListDraft>,
    level_draft: Option<LevelDraft>,
    override_draft: Option<OverrideDraft>,
    override_level_draft: Option<OverrideLevelDraft>,
}
impl Parser<'_> {
    /// Builds a [`Level`] from the active level draft, consuming it.
    fn take_level(&mut self, offset: usize) -> Result<Level, RtfError> {
        let draft = self.level_draft.take().expect("checked level depth");
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
        Ok(Level {
            ilvl: Ilvl(0),
            start: Some(draft.start),
            format: Some(Arc::from(draft.format)),
            text: Some(Arc::from(text)),
            justification: Some(draft.justification),
            suffix: Some(Arc::from(draft.suffix)),
            paragraph: self.state.paragraph.clone(),
            run: self.state.run.clone(),
            ..Default::default()
        })
    }
    fn finish_level(&mut self, offset: usize) -> Result<(), RtfError> {
        let mut level = self.take_level(offset)?;
        if let Some(over) = self.override_level_draft.as_mut() {
            if over.level.is_some() {
                return malformed(offset, "duplicate list override level");
            }
            over.level = Some(level);
        } else {
            let list = self.list_draft.as_mut().ok_or(RtfError::Malformed {
                offset,
                reason: "list level outside list",
            })?;
            if list.levels.len() >= 9 {
                return malformed(offset, "list has more than nine levels");
            }
            level.ilvl = Ilvl(u8::try_from(list.levels.len()).expect("at most nine"));
            list.levels.push(level);
        }
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
            overrides: draft.levels,
            location: SourceLocation::unknown(),
        }) {
            return malformed(offset, "duplicate list instance");
        }
        self.node(1, offset)
    }
    /// Finalizes the `\lfolevel` group into a [`LevelOverride`] on the override.
    fn finish_override_level(&mut self, offset: usize) -> Result<(), RtfError> {
        let draft = self
            .override_level_draft
            .take()
            .expect("checked override level depth");
        let over = self.override_draft.as_mut().ok_or(RtfError::Malformed {
            offset,
            reason: "list override level outside declaration",
        })?;
        if over.levels.len() >= 9 {
            return malformed(offset, "list override has more than nine levels");
        }
        let ilvl = Ilvl(u8::try_from(over.levels.len()).expect("at most nine"));
        let level = draft.level.map(|mut level| {
            level.ilvl = ilvl;
            level
        });
        over.levels.push(LevelOverride {
            ilvl,
            start_override: draft.start_override,
            level,
        });
        self.node(1, offset)
    }
    /// Applies a list-level control word to the active [`LevelDraft`], if any.
    ///
    /// Shared by the `\listtable` levels and the `\lfolevel` override levels.
    /// Returns `true` when `name` is a recognized level control.
    fn level_control(
        &mut self,
        name: &str,
        n: Option<i32>,
        span: RtfSpan,
    ) -> Result<bool, RtfError> {
        let Some(level) = &mut self.level_draft else {
            return Ok(false);
        };
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
            }
            "levelstartat" => {
                level.start = n
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid list start",
                    })?;
            }
            "leveljc" | "leveljcn" => {
                level.justification = match n {
                    Some(0) => Justification::Start,
                    Some(1) => Justification::Center,
                    Some(2) => Justification::End,
                    _ => return malformed(span.start, "invalid list alignment"),
                };
            }
            "levelfollow" => {
                level.suffix = match n {
                    Some(0) => "tab",
                    Some(1) => "space",
                    Some(2) => "nothing",
                    _ => return malformed(span.start, "invalid list suffix"),
                };
            }
            "leveltemplateid" => {}
            _ => return Ok(false),
        }
        Ok(true)
    }
    /// Applies an RTF border control (`<edge> <style> \brdrw? \brsp? \brdrcf?`).
    ///
    /// Paragraph edges map to `w:pBdr`, cell edges to `w:tcBorders`, and row
    /// edges to `w:tblPrEx`. `\brdrtbl` (a reference to the table borders) and
    /// the page-border family only clear the running edge, so a trailing style
    /// token is never misapplied to a stale edge; those edge tokens still fall
    /// through to the explicit Loss accounting.
    fn border_control(
        &mut self,
        name: &str,
        n: Option<i32>,
        span: RtfSpan,
    ) -> Result<bool, RtfError> {
        match name {
            "brdrt" => return self.begin_border(BorderScope::Paragraph, BorderSide::Top, span),
            "brdrb" => return self.begin_border(BorderScope::Paragraph, BorderSide::Bottom, span),
            "brdrl" => return self.begin_border(BorderScope::Paragraph, BorderSide::Start, span),
            "brdrr" => return self.begin_border(BorderScope::Paragraph, BorderSide::End, span),
            "brdrbar" => return self.begin_border(BorderScope::Paragraph, BorderSide::Bar, span),
            "brdrbtw" => {
                return self.begin_border(BorderScope::Paragraph, BorderSide::Between, span)
            }
            "clbrdrt" => return self.begin_border(BorderScope::Cell, BorderSide::Top, span),
            "clbrdrb" => return self.begin_border(BorderScope::Cell, BorderSide::Bottom, span),
            "clbrdrl" => return self.begin_border(BorderScope::Cell, BorderSide::Start, span),
            "clbrdrr" => return self.begin_border(BorderScope::Cell, BorderSide::End, span),
            "trbrdrt" => return self.begin_border(BorderScope::Row, BorderSide::Top, span),
            "trbrdrb" => return self.begin_border(BorderScope::Row, BorderSide::Bottom, span),
            "trbrdrl" => return self.begin_border(BorderScope::Row, BorderSide::Start, span),
            "trbrdrr" => return self.begin_border(BorderScope::Row, BorderSide::End, span),
            "trbrdrh" => {
                return self.begin_border(BorderScope::Row, BorderSide::InsideHorizontal, span)
            }
            "trbrdrv" => {
                return self.begin_border(BorderScope::Row, BorderSide::InsideVertical, span)
            }
            "cldgll" => {
                return self.begin_border(BorderScope::Cell, BorderSide::TopLeftToBottomRight, span)
            }
            "cldglu" => {
                return self.begin_border(BorderScope::Cell, BorderSide::BottomLeftToTopRight, span)
            }
            // A character border (`\chbrdr`) is one `w:rPr/w:bdr` box; the single
            // edge slot the writer/reader use is `top`.
            "chbrdr" => return self.begin_border(BorderScope::Run, BorderSide::Top, span),
            "brdrtbl" => {
                // `\clbrdX\brdrtbl` makes the cell edge inherit the table border:
                // drop the override we just started and consume the token.
                if let Some(target) = self.state.current_border {
                    if matches!(target.scope, BorderScope::Cell) {
                        self.clear_border_edge(target);
                        self.state.current_border = None;
                        return Ok(true);
                    }
                }
                self.state.current_border = None;
                return Ok(false);
            }
            "pgbrdrt" => return self.begin_border(BorderScope::Page, BorderSide::Top, span),
            "pgbrdrb" => return self.begin_border(BorderScope::Page, BorderSide::Bottom, span),
            "pgbrdrl" => return self.begin_border(BorderScope::Page, BorderSide::Start, span),
            "pgbrdrr" => return self.begin_border(BorderScope::Page, BorderSide::End, span),
            "brdrart" => {
                // A page-border art style (`\brdrartN`) has no Strict `ST_Border`
                // form: drop the edge we started and let the art (and its trailing
                // width/space) fall through to Loss rather than misattribute them.
                if let Some(target) = self.state.current_border {
                    if matches!(target.scope, BorderScope::Page) {
                        self.clear_border_edge(target);
                    }
                }
                self.state.current_border = None;
                return Ok(false);
            }
            _ => {}
        }
        let Some(target) = self.state.current_border else {
            return Ok(false);
        };
        if matches!(target.scope, BorderScope::Page) {
            return self.page_border_token(target, name, n, span);
        }
        if let Some(style) = border_style(name) {
            self.border_slot(target).style = Some(style);
            return Ok(true);
        }
        match name {
            "brdrw" => {
                let twips = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing border width",
                })?;
                self.border_slot(target).size = Some(EighthsPoint(border_eighths(twips)));
            }
            "brdrcf" => {
                let index = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "invalid border color index",
                })?;
                let Some(color) = self.palette_color(index, span)? else {
                    return Ok(true);
                };
                self.border_slot(target).color = Some(color);
            }
            "brsp" => {
                let twips = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing border space",
                })?;
                self.border_slot(target).space = Some(border_space_points(twips));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    /// Starts a fresh border edge and remembers it for the trailing tokens.
    fn begin_border(
        &mut self,
        scope: BorderScope,
        side: BorderSide,
        span: RtfSpan,
    ) -> Result<bool, RtfError> {
        if matches!(scope, BorderScope::Cell | BorderScope::Row) && self.row.is_none() {
            return malformed(span.start, "border outside table row");
        }
        let target = BorderTarget { scope, side };
        self.state.current_border = Some(target);
        if matches!(scope, BorderScope::Page) {
            *self.page_slot(side) = PageBorder::default();
        } else {
            *self.border_slot(target) = Border::default();
        }
        Ok(true)
    }
    /// Returns the mutable edge slot a running border targets.
    fn border_slot(&mut self, target: BorderTarget) -> &mut Border {
        let borders = match target.scope {
            BorderScope::Paragraph => &mut self.state.paragraph.borders,
            BorderScope::Cell => {
                &mut self
                    .row
                    .as_mut()
                    .expect("cell border requires a row")
                    .next_props
                    .borders
            }
            BorderScope::Row => {
                &mut self
                    .row
                    .as_mut()
                    .expect("row border requires a row")
                    .props
                    .exception_borders
            }
            BorderScope::Run => &mut self.state.run.borders,
            BorderScope::Page => unreachable!("page edges use page_slot, not border_slot"),
        };
        match target.side {
            BorderSide::Top => borders.top.get_or_insert_with(Border::default),
            BorderSide::Bottom => borders.bottom.get_or_insert_with(Border::default),
            BorderSide::Start => borders.start.get_or_insert_with(Border::default),
            BorderSide::End => borders.end.get_or_insert_with(Border::default),
            BorderSide::InsideHorizontal => borders
                .inside_horizontal
                .get_or_insert_with(Border::default),
            BorderSide::InsideVertical => {
                borders.inside_vertical.get_or_insert_with(Border::default)
            }
            BorderSide::Bar => borders.bar.get_or_insert_with(Border::default),
            BorderSide::Between => borders.between.get_or_insert_with(Border::default),
            BorderSide::TopLeftToBottomRight => borders.tl2br.get_or_insert_with(Border::default),
            BorderSide::BottomLeftToTopRight => borders.tr2bl.get_or_insert_with(Border::default),
        }
    }
    /// Removes the edge a running border targets (used by `\brdrtbl` inherit).
    fn clear_border_edge(&mut self, target: BorderTarget) {
        if matches!(target.scope, BorderScope::Page) {
            if let Some(page) = self.section.page_borders.as_mut() {
                match target.side {
                    BorderSide::Top => page.top = None,
                    BorderSide::Bottom => page.bottom = None,
                    BorderSide::Start => page.left = None,
                    _ => page.right = None,
                }
            }
            return;
        }
        let borders = match target.scope {
            BorderScope::Paragraph => &mut self.state.paragraph.borders,
            BorderScope::Cell => {
                &mut self
                    .row
                    .as_mut()
                    .expect("cell border requires a row")
                    .next_props
                    .borders
            }
            BorderScope::Row => {
                &mut self
                    .row
                    .as_mut()
                    .expect("row border requires a row")
                    .props
                    .exception_borders
            }
            BorderScope::Run => &mut self.state.run.borders,
            BorderScope::Page => {
                unreachable!("page edges must be cleared with the page collection")
            }
        };
        match target.side {
            BorderSide::Top => borders.top = None,
            BorderSide::Bottom => borders.bottom = None,
            BorderSide::Start => borders.start = None,
            BorderSide::End => borders.end = None,
            BorderSide::InsideHorizontal => borders.inside_horizontal = None,
            BorderSide::InsideVertical => borders.inside_vertical = None,
            BorderSide::Bar => borders.bar = None,
            BorderSide::Between => borders.between = None,
            BorderSide::TopLeftToBottomRight => borders.tl2br = None,
            BorderSide::BottomLeftToTopRight => borders.tr2bl = None,
        }
    }
    /// Applies a border style/width/colour/space token to the running page edge.
    fn page_border_token(
        &mut self,
        target: BorderTarget,
        name: &str,
        n: Option<i32>,
        span: RtfSpan,
    ) -> Result<bool, RtfError> {
        if let Some(style) = border_style(name) {
            self.page_slot(target.side).style = Some(style);
            return Ok(true);
        }
        match name {
            "brdrw" => {
                let twips = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing border width",
                })?;
                self.page_slot(target.side).size = Some(EighthsPoint(border_eighths(twips)));
            }
            "brdrcf" => {
                let index = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "invalid border color index",
                })?;
                let Some(color) = self.palette_color(index, span)? else {
                    return Ok(true);
                };
                self.page_slot(target.side).color = Some(color);
            }
            "brsp" => {
                let twips = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing border space",
                })?;
                self.page_slot(target.side).space = Some(border_space_points(twips));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    /// Returns the mutable page-border edge (`w:pgBorders/{top,left,bottom,right}`).
    fn page_slot(&mut self, side: BorderSide) -> &mut PageBorder {
        let page = self
            .section
            .page_borders
            .get_or_insert_with(PageBorders::default);
        match side {
            BorderSide::Top => page.top.get_or_insert_with(PageBorder::default),
            BorderSide::Bottom => page.bottom.get_or_insert_with(PageBorder::default),
            BorderSide::Start => page.left.get_or_insert_with(PageBorder::default),
            _ => page.right.get_or_insert_with(PageBorder::default),
        }
    }
    /// Resolves a colour-table index (`Ok(Some(color))`, with an absent entry
    /// mapped to `auto`; `Ok(None)` = undefined and already reported as a Loss).
    fn palette_color(&mut self, index: i32, span: RtfSpan) -> Result<Option<Color>, RtfError> {
        let Ok(index) = usize::try_from(index) else {
            return malformed(span.start, "invalid colour index");
        };
        let Some(color) = self.colors.get(index).cloned() else {
            self.issue(span, Severity::Loss, format!("undefined-color:{index}"))?;
            return Ok(None);
        };
        Ok(Some(color.unwrap_or_else(|| Color::new("auto"))))
    }
    /// Applies one RTF shading control to its target scope.
    fn apply_shading(
        &mut self,
        scope: ShadingScope,
        name: &str,
        value: i32,
        span: RtfSpan,
    ) -> Result<(), RtfError> {
        if name.ends_with("shdng") || name == "shading" {
            let pattern: Arc<str> = Arc::from(shading_pattern(value));
            self.shading_mut(scope, span)?.pattern = Some(pattern);
        } else {
            let Some(color) = self.palette_color(value, span)? else {
                return Ok(());
            };
            let shading = self.shading_mut(scope, span)?;
            if name.ends_with("cbpat") {
                shading.fill = Some(color);
            } else {
                shading.color = Some(color);
            }
        }
        Ok(())
    }
    /// Returns the mutable shading of a scope (`\ch*`/`\cl*`/`\tr*`/paragraph).
    fn shading_mut(
        &mut self,
        scope: ShadingScope,
        span: RtfSpan,
    ) -> Result<&mut Shading, RtfError> {
        Ok(match scope {
            ShadingScope::Paragraph => self
                .state
                .paragraph
                .shading
                .get_or_insert_with(Shading::default),
            ShadingScope::Run => self.state.run.shading.get_or_insert_with(Shading::default),
            ShadingScope::Cell => self
                .row
                .as_mut()
                .ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell shading outside row",
                })?
                .next_props
                .shading
                .get_or_insert_with(Shading::default),
            ShadingScope::Row => self
                .row
                .as_mut()
                .ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "row shading outside row",
                })?
                .shading
                .get_or_insert_with(Shading::default),
        })
    }
    fn begin_region(&mut self, kind: RegionKind, offset: usize) -> Result<(), RtfError> {
        if matches!(kind, RegionKind::Header { .. }) && !self.regions.is_empty()
            || matches!(
                kind,
                RegionKind::Note { .. } | RegionKind::NoteSeparator { .. }
            ) && self.regions.iter().any(|r| {
                matches!(
                    r.kind,
                    RegionKind::Note { .. }
                        | RegionKind::NoteSeparator { .. }
                        | RegionKind::Header { .. }
                )
            })
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
                if kind == HeaderFooterKind::Even {
                    self.even_and_odd_headers = true;
                }
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
            RegionKind::NoteSeparator { endnote, kind } => {
                // The reserved separator (`-1`) and continuation-separator
                // (`0`) definitions, plus the `w:footnotePr`/`w:endnotePr`
                // references that make the rule appear above the note block.
                let id = match kind {
                    NoteKind::Separator => -1,
                    _ => 0,
                };
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
                table.insert(Note {
                    id,
                    kind,
                    blocks,
                    location: SourceLocation::unknown(),
                });
                let props = if endnote {
                    &mut self.document_endnote
                } else {
                    &mut self.document_footnote
                };
                if !props.separator_ids.contains(&id) {
                    props.separator_ids.push(id);
                    props.separator_ids.sort_unstable();
                }
                Ok(None)
            }
            RegionKind::Field { instruction, dirty } => {
                // A single-paragraph, up-to-date result can use the simple-field
                // form. A block result (the shared model has no block-result
                // simple field) or a dirty field (whose `w:dirty` flag only the
                // complex-field begin marker carries) is emitted as the complex
                // marker form: begin / instrText / separate / result / end, so
                // the result paragraphs and the field scope survive.
                if !dirty && blocks.len() <= 1 && !ended_paragraph {
                    if blocks.is_empty() {
                        return malformed(span.start, "empty field builder");
                    }
                    let mut inlines = vec![];
                    for block in blocks {
                        let Block::Paragraph(p) = block else {
                            return malformed(span.start, "table in inline field result");
                        };
                        inlines.extend(p.inlines);
                    }
                    return Ok(Some(Inline::Field(Field {
                        instruction: Some(Arc::from(instruction.trim())),
                        inlines,
                        location: SourceLocation::unknown(),
                    })));
                }
                self.inlines
                    .push(field_char_marker(FieldCharType::Begin, dirty));
                self.inlines.push(Inline::Run(Run {
                    props: RunProperties::default(),
                    content: vec![RunContent::InstrText(instruction.trim().to_owned())],
                    revision: None,
                    location: SourceLocation::unknown(),
                }));
                self.inlines
                    .push(field_char_marker(FieldCharType::Separate, false));
                let preceded = self.inlines.len() > 3;
                for (index, block) in blocks.into_iter().enumerate() {
                    let Block::Paragraph(p) = block else {
                        return malformed(span.start, "table in field result");
                    };
                    if index == 0 {
                        if !preceded {
                            self.paragraph_props = p.props;
                        }
                    } else {
                        self.paragraph(span.start)?;
                        self.paragraph_props = p.props;
                    }
                    self.inlines.extend(p.inlines);
                    self.paragraph_pending = true;
                }
                self.inlines
                    .push(field_char_marker(FieldCharType::End, false));
                self.paragraph(span.start)?;
                Ok(None)
            }
            RegionKind::Bookmark { is_start } => {
                let mut name = String::new();
                for block in &blocks {
                    if let Block::Paragraph(paragraph) = block {
                        for inline in &paragraph.inlines {
                            if let Inline::Run(run) = inline {
                                for content in &run.content {
                                    if let RunContent::Text(text) = content {
                                        name.push_str(&text.text);
                                    }
                                }
                            }
                        }
                    }
                }
                let name = name.trim().to_owned();
                if name.is_empty() {
                    self.issue(span, Severity::Loss, "bookmark:empty-name")?;
                    return Ok(None);
                }
                // `w:id` is a decimal number and pairs start with end; `w:name`
                // is the bookmark's identity for hyperlinks and REF fields.
                let id = if let Some(id) = self.bookmarks.get(&name) {
                    *id
                } else {
                    let id = self.bookmark_seq;
                    self.bookmark_seq = self.bookmark_seq.saturating_add(1);
                    self.bookmarks.insert(name.clone(), id);
                    id
                };
                if is_start {
                    Ok(Some(Inline::BookmarkStart(Bookmark::new(
                        id.to_string(),
                        name,
                    ))))
                } else {
                    Ok(Some(Inline::BookmarkEnd(BookmarkId::new(id.to_string()))))
                }
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
            custom_style: false,
            auto_redefine: false,
            semi_hidden: draft.semi_hidden,
            hidden: draft.hidden,
            q_format: draft.quick,
            locked: false,
            unhide_when_used: false,
            ui_priority: draft.priority,
            table: TableProperties::default(),
            row: RowProperties::default(),
            cell: CellProperties::default(),
            paragraph: self.state.paragraph.clone(),
            run: self.state.run.clone(),
            conditions: vec![],
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
        self.table_look = TableLook::default();
        self.table_rtl = false;
        self.table_gap = None;
        self.table_padding = CellMargins::default();
        self.table_layout = None;
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
            let look = (self.table_look != TableLook::default()).then_some(self.table_look);
            // `\trgaph` is half the inter-cell gap (both sides); an explicit
            // `\trpadd*` overrides the half-gap on each side it names, so the
            // table keeps the padding the source actually declared.
            let mut cell_margins =
                self.table_gap
                    .map_or_else(CellMargins::default, |gap| CellMargins {
                        start: Some(gap),
                        end: Some(gap),
                        ..CellMargins::default()
                    });
            if self.table_padding.top.is_some() {
                cell_margins.top = self.table_padding.top;
            }
            if self.table_padding.bottom.is_some() {
                cell_margins.bottom = self.table_padding.bottom;
            }
            if self.table_padding.start.is_some() {
                cell_margins.start = self.table_padding.start;
            }
            if self.table_padding.end.is_some() {
                cell_margins.end = self.table_padding.end;
            }
            self.node(1, span.start)?;
            self.table = Some(Table {
                props: TableProperties {
                    indent: Some(Twips(row.left)),
                    cell_margins,
                    layout: self.table_layout,
                    look,
                    bidi_visual: self.table_rtl,
                    ..Default::default()
                },
                grid,
                grid_change: None,
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
        // A row background (`\trcbpat`/`\trcfpat`/`\trshdng`) shades the whole
        // row; WML has no row-level shading, so it lands on every cell that does
        // not carry its own `\cl*` shading.
        if let Some(shading) = row.shading.clone() {
            for cell in &mut cells {
                if cell.props.shading.is_none() {
                    cell.props.shading = Some(shading.clone());
                }
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
                    theme: self.font_theme.take(),
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
        if let Some(slot) = self
            .state
            .font_slot
            .or_else(|| font.theme.map(|theme| theme.slot))
        {
            assign_font(
                self.state.run.fonts.get_or_insert_with(Fonts::default),
                slot,
                face.clone(),
            );
            if let Some(theme) = font.theme {
                assign_font_theme(
                    self.state.run.fonts.get_or_insert_with(Fonts::default),
                    slot,
                    theme.reference,
                );
            }
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
        if let Some(theme) = font.theme {
            let fonts = self.state.run.fonts.get_or_insert_with(Fonts::default);
            assign_font_theme(fonts, theme.slot, theme.reference);
        }
        self.state.codepage = cp;
        Ok(())
    }
    fn select_associated_font(&mut self, id: i32, span: RtfSpan) -> Result<(), RtfError> {
        if self.single_font_active {
            return Ok(());
        }
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
            if let Some(theme) = font.theme {
                assign_font_theme(
                    self.state.run.fonts.get_or_insert_with(Fonts::default),
                    slot,
                    theme.reference,
                );
            }
            return Ok(());
        }
        self.state.associated_font = Some(id);
        self.state
            .run
            .fonts
            .get_or_insert_with(Default::default)
            .east_asia = Some(face);
        if let Some(theme) = font.theme {
            assign_font_theme(
                self.state.run.fonts.get_or_insert_with(Fonts::default),
                theme.slot,
                theme.reference,
            );
        }
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
        if self.section_reset_pending {
            self.reset_section();
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
                kind: RegionKind::Field { instruction, .. },
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
        // A run with no explicit direction that mixes LTR and RTL strong text is
        // split into direction-homogeneous runs at paragraph end (RC03).
        self.push_text(text, offset)
    }

    /// Coalesces adjacent text in equal runs, so source escape density cannot
    /// multiply model nodes. Never called across explicit paragraph breaks.
    fn push_text(&mut self, text: String, offset: usize) -> Result<(), RtfError> {
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
            // An empty paragraph is only a paragraph mark, so its height comes
            // from `w:pPr/w:rPr`. Carry the ambient run formatting (font and
            // half-point size) so the mark matches the surrounding text instead
            // of falling back to the document defaults; WPS renders the empty
            // line at the body size, and a size mismatch shifts every following
            // line by one line's worth of leading.
            let mut props = self.state.paragraph.clone();
            if self.state.run != RunProperties::default() {
                props.run_props = Some(self.state.run.clone());
            }
            props
        } else {
            self.paragraph_props.clone()
        };
        let mut inlines = std::mem::take(&mut self.inlines);
        split_mixed_directional_runs(&mut inlines);
        let block = Block::Paragraph(Paragraph {
            props,
            inlines,
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
    /// Resets the running section properties to the document defaults.
    fn reset_section(&mut self) {
        self.section = SectionProperties {
            page_size: Some(self.document_page_size),
            page_margins: Some(self.document_page_margins),
            ..Default::default()
        };
        self.section_reset_pending = false;
    }
    /// Applies a footnote/endnote control word to [`NoteProperties`].
    ///
    /// `\ftn*`/`\aftn*` (and `\enddoc`/`\aenddoc`) are document-wide and land in
    /// `settings.xml`; a leading `s` (`\sftnbj`, `\saftnnrlc`, ...) marks the
    /// section-scoped variant and lands in the running `w:sectPr`. Returns
    /// `true` when `name` is a recognized note property.
    fn note_control(
        &mut self,
        name: &str,
        n: Option<i32>,
        span: RtfSpan,
    ) -> Result<bool, RtfError> {
        let (base, section) = match name.strip_prefix('s') {
            Some(rest) if note_control_base(rest).is_some() => (rest, true),
            _ => (name, false),
        };
        let Some(control) = note_control_base(base) else {
            return Ok(false);
        };
        let props = match (section, control.role) {
            (true, NoteRole::Footnote) => &mut self.section.footnote_properties,
            (true, NoteRole::Endnote) => &mut self.section.endnote_properties,
            (false, NoteRole::Footnote) => &mut self.document_footnote,
            (false, NoteRole::Endnote) => &mut self.document_endnote,
        };
        match control.property {
            NoteProperty::Position(value) => props.position = Some(Arc::from(value)),
            NoteProperty::Format(value) => props.num_format = Some(Arc::from(value)),
            NoteProperty::Restart(value) => props.num_restart = Some(Arc::from(value)),
            NoteProperty::Start => {
                let value = n
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid note start number",
                    })?;
                props.num_start = Some(value);
            }
        }
        Ok(true)
    }
    fn word(&mut self, name: &str, n: Option<i32>, span: RtfSpan) -> Result<(), RtfError> {
        let first = self.state.first;
        self.state.first = false;
        // Resolve a deferred `\sectd` before the next control word takes effect,
        // unless that word is the `\sect` that must commit the section first.
        if self.section_reset_pending && name != "sect" {
            self.reset_section();
        }
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
            if self.level_control(name, n, span)? {
                return Ok(());
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
                        levels: vec![],
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
                // The override level count is redundant with the number of
                // `\lfolevel` groups actually present, so accept any value;
                // `\listoverrideformat`/`\listoverridestart` only select which
                // form follows (the nested controls carry the actual level or
                // start); `\listtemplateid`/`\listhybrid`/`\listsimple` are
                // top-level list hints with no `w:num` analogue.
                "listoverridecount" | "listoverrideformat" | "listoverridestart"
                | "listtemplateid" | "listhybrid" | "listsimple" => return Ok(()),
                "lfolevel" => {
                    if self.override_level_draft.is_some() {
                        return malformed(span.start, "nested list override level");
                    }
                    if self.override_draft.is_none() {
                        return malformed(span.start, "list override level outside declaration");
                    }
                    self.override_level_draft = Some(OverrideLevelDraft {
                        depth: self.stack.len(),
                        start_override: None,
                        level: None,
                    });
                    return Ok(());
                }
                "listlevel" => {
                    if self.override_level_draft.is_none() {
                        return malformed(span.start, "list level outside list override level");
                    }
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
                _ => (),
            }
            if self.level_control(name, n, span)? {
                return Ok(());
            }
            // A `\levelstartat` without a nested `\listlevel` is a start-only
            // override (`\listoverridestart`).
            if name == "levelstartat" {
                let start = n
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid list start",
                    })?;
                let over = self
                    .override_level_draft
                    .as_mut()
                    .ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "level start outside list override level",
                    })?;
                over.start_override = Some(start);
                return Ok(());
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
                        dirty: false,
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
            if name == "bkmkstart" || name == "bkmkend" {
                // `{\*\bkmkstart Name}` / `{\*\bkmkend Name}`: the name is the
                // destination's text.
                self.state.ignorable = false;
                return self.begin_region(
                    RegionKind::Bookmark {
                        is_start: name == "bkmkstart",
                    },
                    span.start,
                );
            }
        }
        if first && matches!(name, "ftnsep" | "ftnsepc" | "aftnsep" | "aftnsepc") {
            // `{\*\ftnsep ...}`/`{\*\ftnsepc ...}` and the endnote twins define
            // the separator content. Capture them as the reserved separator
            // definitions instead of dropping the destination.
            self.state.ignorable = false;
            let endnote = name.starts_with('a');
            let kind = if name.ends_with("sepc") {
                NoteKind::ContinuationSeparator
            } else {
                NoteKind::Separator
            };
            return self.begin_region(RegionKind::NoteSeparator { endnote, kind }, span.start);
        }
        if first && self.state.ignorable {
            self.state.skip = true;
            // `{\*\levelmarker ...}` is a legacy alternative list marker
            // superseded by `\leveltext`, and `{\*\datafield ...}` is a legacy
            // cached binary field payload whose human-readable result Word takes
            // from `\fldrslt`; both are ignored, not a loss.
            let severity = if matches!(name, "levelmarker" | "datafield") {
                Severity::Info
            } else {
                Severity::Loss
            };
            return self.issue(span, severity, format!("destination:{name}"));
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
                    self.font_theme = self.pending_font_theme.take();
                    self.font_id = Some(n.filter(|id| *id >= 0).ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "invalid font id",
                    })?);
                    self.font_charset = 0;
                    self.font_codepage = None;
                    self.font_name.clear();
                }
                "fbimajor" | "fbiminor" | "fdbmajor" | "fdbminor" | "fhimajor" | "fhiminor"
                | "flomajor" | "flominor" => {
                    let (slot, reference) = match name {
                        "fbimajor" => (FontSlot::ComplexScript, "majorBidi"),
                        "fbiminor" => (FontSlot::ComplexScript, "minorBidi"),
                        "fdbmajor" => (FontSlot::EastAsia, "majorEastAsia"),
                        "fdbminor" => (FontSlot::EastAsia, "minorEastAsia"),
                        "fhimajor" => (FontSlot::HighAnsi, "majorHAnsi"),
                        "fhiminor" => (FontSlot::HighAnsi, "minorHAnsi"),
                        "flomajor" => (FontSlot::Ascii, "majorAscii"),
                        "flominor" => (FontSlot::Ascii, "minorAscii"),
                        _ => unreachable!(),
                    };
                    let theme = FontTheme { slot, reference };
                    if self.font_id.is_some() {
                        self.font_theme = Some(theme);
                    } else {
                        self.pending_font_theme = Some(theme);
                    }
                }
                "fcharset" => {
                    self.font_charset = n.ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "missing font charset",
                    })?;
                }
                "cpg" => self.font_codepage = n,
                "fnil" | "froman" | "fswiss" | "fmodern" | "fscript" | "fdecor" | "ftech"
                | "fbidi" | "fbidis" | "fprq" => (),
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
                _ if is_field_instruction_formatting(name) => {
                    // The writer embeds the run formatting in `\fldinst`; it
                    // styles the field-code display, not the instruction text,
                    // so it is dropped as ignored formatting rather than a loss.
                    return self.issue(
                        span,
                        Severity::Info,
                        format!("field-instruction-formatting:{name}"),
                    );
                }
                _ => {
                    // Anything else inside an instruction is not recognized;
                    // a blanket rename of every instruction control to Info is
                    // forbidden, so an unknown one stays a Loss.
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
                    | "levelmarker"
                    | "datafield"
            )
        {
            self.state.skip = true;
            let severity = if matches!(
                name,
                "info"
                    | "generator"
                    // Legacy cached list text/marker: regenerated from the list
                    // table, so keeping it would double the marker.
                    | "listtext"
                    | "pntext"
                    | "levelmarker"
            ) {
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
        if self.note_control(name, n, span)? {
            return Ok(());
        }
        if self.border_control(name, n, span)? {
            return Ok(());
        }
        match name {
            "upr" => self.state.upr_children = Some(0),
            "rtf" if n == Some(1) && self.stack.len() == 1 => (),
            // RichEdit writes this when the document uses one CHP font rather
            // than associated script fonts; plain `\f` already seeds all WML slots.
            "fbidis" => self.single_font_active = n != Some(0),
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
                self.state.current_border = None;
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
            "chftnsep" => self.content(RunContent::Separator, span.start)?,
            "chftnsepc" => self.content(RunContent::ContinuationSeparator, span.start)?,
            "tqc" => self.state.tab_alignment = TabAlignment::Center,
            "tqr" => self.state.tab_alignment = TabAlignment::End,
            "tqdec" => self.state.tab_alignment = TabAlignment::Decimal,
            "tb" => self.state.tab_alignment = TabAlignment::Bar,
            "tldot" => self.state.tab_leader = Some(TabLeader::Dot),
            "tlhyph" => self.state.tab_leader = Some(TabLeader::Hyphen),
            "tlul" => self.state.tab_leader = Some(TabLeader::Underscore),
            "tlth" => self.state.tab_leader = Some(TabLeader::Heavy),
            "tleq" => {
                self.issue(span, Severity::Loss, "tab:leader-equal-unsupported")?;
            }
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
            // Default tab stop (`w:defaultTabStop` in settings.xml): `\deftab`
            // is the document default and `\pardeftab` the WPS spelling, both in
            // twips.
            "deftab" | "pardeftab" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing default tab stop",
                })?;
                self.default_tab_stop = Some(Twips(value));
            }
            "widctlpar" => self.state.paragraph.widow_control = toggle,
            "nowidctlpar" => self.state.paragraph.widow_control = TriState::Off,
            "contextualspace" => self.state.paragraph.contextual_spacing = toggle,
            "outlinelevel" => {
                self.state.paragraph.outline_level =
                    n.and_then(|n| u8::try_from(n).ok()).filter(|n| *n <= 9);
            }
            "sectd" => {
                self.section_reset_pending = true;
            }
            "titlepg" => self.section.title_page = true,
            "cols" | "colsx" | "colno" | "colw" | "colsr" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing column value",
                })?;
                match name {
                    "cols" => {
                        let count = u16::try_from(value).map_err(|_| RtfError::Malformed {
                            offset: span.start,
                            reason: "invalid column count",
                        })?;
                        let columns = self.section.columns.get_or_insert_with(Default::default);
                        columns.count = Some(count);
                        if columns.columns.is_empty() {
                            columns.equal_width = true;
                        }
                        self.column_index = 0;
                    }
                    "colsx" => {
                        self.section
                            .columns
                            .get_or_insert_with(Default::default)
                            .space = Some(Twips(value));
                    }
                    "colno" => {
                        self.column_index = usize::try_from(value.max(1) - 1).unwrap_or(0);
                    }
                    "colw" | "colsr" => {
                        let columns = self.section.columns.get_or_insert_with(Default::default);
                        // An explicit per-column width makes the columns unequal.
                        columns.equal_width = false;
                        while columns.columns.len() <= self.column_index {
                            columns.columns.push(ColumnSpec::default());
                        }
                        let column = &mut columns.columns[self.column_index];
                        if name == "colw" {
                            column.width = Some(Twips(value));
                        } else {
                            column.space = Some(Twips(value));
                        }
                    }
                    _ => unreachable!("matched column control"),
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
                if self.section_reset_pending {
                    self.reset_section();
                }
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
            "outl" => self.state.run.outline = toggle,
            "shad" => self.state.run.shadow = toggle,
            "embo" => self.state.run.emboss = toggle,
            "impr" => self.state.run.imprint = toggle,
            "noproof" => self.state.run.no_proof = toggle,
            "snaptogrid" => self.state.run.snap_to_grid = toggle,
            "kern" | "kerning" => {
                let size = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing kerning size",
                })?;
                if size < 0 {
                    self.issue(span, Severity::Loss, "run:kerning-out-of-range")?;
                } else {
                    self.state.run.kerning = Some(HalfPoints(size));
                }
            }
            "position" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing vertical position",
                })?;
                self.state.run.position = Some(HalfPoints(value));
            }
            "up" | "dn" => {
                let magnitude = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing position offset",
                })?;
                let signed = if name == "dn" {
                    magnitude.checked_neg().ok_or(RtfError::Malformed {
                        offset: span.start,
                        reason: "position offset overflow",
                    })?
                } else {
                    magnitude
                };
                self.state.run.position = Some(HalfPoints(signed));
            }
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
            "ulc" => {
                // Underline colour index (`w:u/@w:color`); palette index 0 is the
                // `auto` slot, which `palette_color` maps to `w:val="auto"`.
                let index = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing underline colour",
                })?;
                if let Some(color) = self.palette_color(index, span)? {
                    self.state.run.underline_color = Some(color);
                }
            }
            "cb" => {
                // Character background colour (`w:shd` on the run), the WPS
                // spelling of the character background; `\chcbpat` is the Word
                // spelling of the same property.
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing character background",
                })?;
                self.apply_shading(ShadingScope::Run, "chcbpat", value, span)?;
            }
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
            "charscalex" => {
                let percent = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing character scale",
                })?;
                match u16::try_from(percent) {
                    Ok(scale) if scale <= 600 => self.state.run.scale = Some(scale),
                    _ => self.issue(span, Severity::Loss, "run:char-scale-out-of-range")?,
                }
            }
            "expnd" | "expndtw" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing character spacing",
                })?;
                // `\expnd` is in quarter-points, `\expndtw` in twips (five per
                // quarter-point); `w:spacing` is twips.
                let twips = if name == "expnd" {
                    value.checked_mul(5)
                } else {
                    Some(value)
                };
                match twips {
                    Some(twips) => self.state.run.spacing = Some(Twips(twips)),
                    None => self.issue(span, Severity::Loss, "run:char-spacing-overflow")?,
                }
            }
            "aexpnd" => {
                // Associated (complex-script) expansion has no WML run slot.
                self.issue(span, Severity::Loss, "run:associated-spacing-unsupported")?;
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
            "rtlsect" => self.section.bidi = true,
            "ltrsect" => self.section.bidi = false,
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
                self.state.current_border = None;
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
                self.state.current_border = None;
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
                // `\clftsWidth`/`\clwWidth` give the explicit cell width; without
                // them the width is the `\cellx` delta (both agree in the corpus).
                props.width = Some(Width {
                    kind: row.next_width_kind.take().unwrap_or(WidthKind::Dxa),
                    value: row.next_width_value.take().or(Some(width)),
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
            "clhidemark" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                row.next_props.hide_mark = n != Some(0);
            }
            "clpadt" | "clpadb" | "clpadl" | "clpadr" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing cell margin",
                })?;
                let margin = Twips(value);
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                let margins = &mut row.next_props.margins;
                match name {
                    "clpadt" => margins.top = Some(margin),
                    "clpadb" => margins.bottom = Some(margin),
                    "clpadl" => margins.start = Some(margin),
                    _ => margins.end = Some(margin),
                }
            }
            // The `\clpadf*` family only selects the padding unit (twips); the
            // value itself comes from `\clpad*`.
            "clpadfl" | "clpadfr" | "clpadft" | "clpadfb" => {
                return self.issue(
                    span,
                    Severity::Info,
                    format!("table-cell-padding-unit:{name}"),
                );
            }
            "clftsWidth" => {
                let kind = match n {
                    Some(0) => WidthKind::Auto,
                    Some(1) => WidthKind::Nil,
                    Some(2) => WidthKind::Pct,
                    Some(3) => WidthKind::Dxa,
                    _ => return malformed(span.start, "invalid cell width type"),
                };
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                row.next_width_kind = Some(kind);
            }
            "clwWidth" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing cell width",
                })?;
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                row.next_width_value = Some(value);
            }
            "clcbpat" | "clcfpat" | "clshdng" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing cell shading value",
                })?;
                self.apply_shading(ShadingScope::Cell, name, value, span)?;
            }
            "clshdrawnil" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                row.next_props.shading = None;
            }
            // Cell text direction (`w:textDirection`). `\cltxlrtb` is the Strict
            // default (`tb`, top-to-bottom, left-to-right), so WPS writes no
            // element for it; only a non-default direction is carried.
            "cltxlrtb" => {
                return self.issue(span, Severity::Info, "table-cell-compat:cltxlrtb");
            }
            "cltxtbrl" | "cltxbtlr" | "cltxlrtbv" | "cltxtbrlv" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "cell property outside row",
                })?;
                row.next_props.text_direction = Some(match name {
                    "cltxtbrl" => TextDirection::TbRl,
                    "cltxbtlr" => TextDirection::BtLr,
                    "cltxlrtbv" => TextDirection::LrTbV,
                    _ => TextDirection::TbRlV,
                });
            }
            "trcbpat" | "trcfpat" | "trshdng" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing row shading value",
                })?;
                self.apply_shading(ShadingScope::Row, name, value, span)?;
            }
            "chcbpat" | "chcfpat" | "chshdng" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing character shading value",
                })?;
                self.apply_shading(ShadingScope::Run, name, value, span)?;
            }
            "cbpat" | "cfpat" | "shading" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing paragraph shading value",
                })?;
                self.apply_shading(ShadingScope::Paragraph, name, value, span)?;
            }
            "trgaph" => self.table_gap = n.map(Twips),
            // Table default cell padding (`w:tblCellMar`). The value comes from
            // `\trpadd{t,b,l,r}`; the parallel `\trpaddf{t,b,l,r}` selects the
            // unit, where 3 is twips and 0 means "no padding" for that side.
            "trpaddt" | "trpaddb" | "trpaddl" | "trpaddr" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing table cell padding",
                })?;
                match name {
                    "trpaddt" => self.table_padding.top = Some(Twips(value)),
                    "trpaddb" => self.table_padding.bottom = Some(Twips(value)),
                    "trpaddl" => self.table_padding.start = Some(Twips(value)),
                    _ => self.table_padding.end = Some(Twips(value)),
                }
            }
            "trpaddft" | "trpaddfb" | "trpaddfl" | "trpaddfr" => {
                let unit = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing table cell padding unit",
                })?;
                match unit {
                    // A null unit clears that side's padding to zero.
                    0 => match name {
                        "trpaddft" => self.table_padding.top = Some(Twips(0)),
                        "trpaddfb" => self.table_padding.bottom = Some(Twips(0)),
                        "trpaddfl" => self.table_padding.start = Some(Twips(0)),
                        _ => self.table_padding.end = Some(Twips(0)),
                    },
                    // Twips is the declared padding unit; no conversion needed.
                    3 => (),
                    _ => {
                        return self.issue(
                            span,
                            Severity::Loss,
                            format!("table-cell-padding-unit:{name}:{unit}"),
                        )
                    }
                }
            }
            // Row height (`w:trHeight`): a positive value is a minimum
            // (`atLeast`), a negative value an exact height (`exact`).
            "trrh" => {
                let value = n.ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "missing row height",
                })?;
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "row property outside row",
                })?;
                row.props.height = Some(if value == 0 {
                    RowHeight {
                        value: Some(Twips(0)),
                        rule: Some(HeightRule::Auto),
                    }
                } else {
                    RowHeight {
                        value: Some(Twips(value.checked_abs().ok_or(RtfError::Malformed {
                            offset: span.start,
                            reason: "row height overflow",
                        })?)),
                        rule: Some(if value < 0 {
                            HeightRule::Exact
                        } else {
                            HeightRule::AtLeast
                        }),
                    }
                });
            }
            // Row alignment (`w:trPr/w:jc`).
            "trql" | "trqc" | "trqr" => {
                let row = self.row.as_mut().ok_or(RtfError::Malformed {
                    offset: span.start,
                    reason: "row property outside row",
                })?;
                row.props.alignment = Some(match name {
                    "trqr" => Justification::End,
                    "trqc" => Justification::Center,
                    _ => Justification::Start,
                });
            }
            // Table layout algorithm (`w:tblLayout`), applied when the table is
            // created at its first row.
            "trautofit" => {
                self.table_layout = Some(if n == Some(0) {
                    TableLayout::Fixed
                } else {
                    TableLayout::Autofit
                });
            }
            // Preferred row/table widths are carried by the `\cellx` grid (the
            // Strict `w:tblW`/`w:trW` are derived), so these only name the unit
            // and value; there is nothing extra to write back.
            "trftsWidth" | "trftsWidthA" | "trftsWidthB" | "trwWidth" | "trwWidthA"
            | "trwWidthB" => {
                return self.issue(span, Severity::Info, format!("table-row-compat:{name}"));
            }
            // Table-indent controls with no independent WML form: the indent is
            // carried by the row position (`\trleft`), and WPS writes no
            // `w:tblInd` for the corpus (`\tblind0`).
            "tblind" | "tblindtype" | "alntblind" => {
                return self.issue(span, Severity::Info, format!("table-indent-compat:{name}"));
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
            // `w:tblLook` conditional-formatting flags, repeated on every row.
            "tbllkhdrrows" => self.table_look.first_row = n != Some(0),
            "tbllkhdrcols" => self.table_look.first_column = n != Some(0),
            "tbllklastrow" => self.table_look.last_row = n != Some(0),
            "tbllklastcol" => self.table_look.last_column = n != Some(0),
            "tbllknocolband" => self.table_look.no_v_band = n != Some(0),
            "tbllknorowband" => self.table_look.no_h_band = n != Some(0),
            "rtlrow" => self.table_rtl = n != Some(0),
            "ltrrow" => self.table_rtl = false,
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
            "li" | "lin" | "ri" | "rin" | "fi" | "fin" => {
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
                    "li" | "lin" => props.start = Some(Twips(value)),
                    "ri" | "rin" => props.end = Some(Twips(value)),
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
                    props.before_autospacing = Some(n != Some(0));
                } else {
                    props.after_autospacing = Some(n != Some(0));
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
            "flddirty" => {
                // The field's cached result is stale. `w:dirty` lives on the
                // complex-field begin marker, which the field's `end_region`
                // emits when this flag is set.
                if let Some(RegionFrame {
                    kind: RegionKind::Field { dirty, .. },
                    ..
                }) = self.regions.last_mut()
                {
                    *dirty = true;
                    return Ok(());
                }
                return self.issue(span, Severity::Info, "field-compat:flddirty");
            }
            "fldedit" => {
                // "Field was edited": a Word/RTF compatibility hint with no WML
                // analogue on either field form.
                return self.issue(span, Severity::Info, "field-compat:fldedit");
            }
            "chpgn" | "sectnum" => {
                // RTF special characters that render a page/section number.
                // Keep them as dynamic fields (a renderer computes the value)
                // instead of dropping them: 072's headers read "Ruth (Page )".
                self.finish_high()?;
                if self.row.is_none() {
                    self.finish_table();
                }
                self.node(1, span.start)?;
                if self.inlines.is_empty() {
                    self.paragraph_props = self.state.paragraph.clone();
                }
                let instruction: Arc<str> = Arc::from(if name == "chpgn" {
                    " PAGE "
                } else {
                    " SECTION "
                });
                self.inlines.push(Inline::Field(Field {
                    instruction: Some(instruction),
                    inlines: vec![],
                    location: SourceLocation::unknown(),
                }));
            }
            // Word 7 / list-table compatibility markers with no WML equivalent:
            // `\jclisttab` only flags the `\tx` tab as a list tab (the tab stop
            // is already captured), `\levelspace`/`\levelindent` are superseded
            // by the level's paragraph indents, `\leveltemplateid` is a cosmetic
            // template id, `\levelold` marks a level converted from Word 6.0 or
            // 7.0, and `\fbias` is the East-Asian font-bias hint Word emits only
            // inside a `\listlevel` (no direct `w:rFonts` analogue).
            "jclisttab" | "levelspace" | "levelindent" | "leveltemplateid" | "levelold"
            | "fbias" => {
                return self.issue(span, Severity::Info, format!("list-compat:{name}"));
            }
            // Table compatibility controls with no WML equivalent: the character
            // grid (`\cgrid`/`\snaptogridincell`), the table-growth/format hints
            // (`\nogrowautofit`/`\nocxsptable`/`\usenormstyforlist`) and the
            // conditional-formatting row band ids (`\irow`/`\irowband`/
            // `\lastrow`) are hints the Strict model does not carry.
            "cgrid" | "snaptogridincell" | "nocxsptable" | "nogrowautofit"
            | "usenormstyforlist" | "irow" | "irowband" | "lastrow" | "lytcalctblwd" => {
                return self.issue(span, Severity::Info, format!("table-compat:{name}"));
            }
            // Page-border placement hints (draw in header/footer, offset) with no
            // Strict `w:pgBorders` analogue; the page edges themselves stay Loss
            // because the corpus uses unrepresentable art borders (`\brdrart`).
            "pgbrdrhead" | "pgbrdrfoot" | "pgbrdropt" => {
                return self.issue(span, Severity::Info, format!("page-border-compat:{name}"));
            }
            // Revision save identifiers (`\insrsid`/`\charrsid`/`\pararsid`):
            // non-visual merge metadata. The model carries optional `w:rsid*`
            // slots the renderer never reads, and the RTF stream's decimal ids
            // and their per-run/per-paragraph placement are not reconstructed
            // here, so they are named Info rather than dropped as a Loss.
            "insrsid" | "charrsid" | "pararsid" => {
                return self.issue(span, Severity::Info, format!("rsid-compat:{name}"));
            }
            // Word 6/7 auto-spacing and font-alignment hints whose bare form is
            // the Strict default, so a mapping would write nothing back:
            // `\faauto` is `w:textAlignment="auto"`, `\adjustright` is
            // `w:adjustRightInd` on, and `\aspalpha`/`\aspnum` are
            // `w:autoSpaceDE`/`w:autoSpaceDN` on. A non-default value (`...0`)
            // has no slot in the paragraph model and stays a Loss.
            "faauto" | "adjustright" | "aspalpha" | "aspnum" if n.is_none() => {
                return self.issue(span, Severity::Info, format!("word6-compat:{name}"));
            }
            // Word 5/6/7 and Asian text-layout compatibility hints with no
            // Strict paragraph slot: `\hyphpar` enables paragraph auto-hyphenation
            // (the WML default) and `\wrapdefault`/`\sectdefaultcl`/`\linex`/
            // `\lnbrkrule`/`\partightenfactor` only assert default layout
            // behaviour. `\hyphpar0` (suppress hyphenation) is non-default and
            // stays a Loss.
            "hyphpar" if n.is_none() => {
                return self.issue(span, Severity::Info, "paragraph-compat:hyphpar");
            }
            // Word 5/6/7 and Asian text-layout hints whose effect is default
            // layout: `\wrapdefault`/`\sectdefaultcl`/`\linex`/`\lnbrkrule`/
            // `\partightenfactor` assert default line breaking, and the Arabic
            // kashida hints `\qk`/`\qnatural` get no kashida `w:jc` from WPS.
            "wrapdefault" | "sectdefaultcl" | "linex" | "lnbrkrule" | "partightenfactor" | "qk"
            | "qnatural" => {
                return self.issue(span, Severity::Info, format!("paragraph-compat:{name}"));
            }
            // WPS writes `\hyphpar0` (and `\noline`) as paragraph resets whose
            // effect is the default layout; its DOCX carries no
            // `w:suppressAutoHyphens`, so the honest reading is a named Info.
            "hyphpar" if n == Some(0) => {
                return self.issue(span, Severity::Info, "paragraph-compat:hyphpar0");
            }
            "noline" => {
                return self.issue(span, Severity::Info, "paragraph-compat:noline");
            }
            // `\nooverflow`/`\nocwrap` are WPS cell-text controls; WPS writes no
            // `w:noWrap` for them, so they assert the default wrapping.
            "nooverflow" | "nocwrap" => {
                return self.issue(span, Severity::Info, format!("table-cell-compat:{name}"));
            }
            // Picture resolution hint (`\hres`/`\chhres`, in dots per inch) set to
            // 0 in the corpus: no Strict slot and no visual effect.
            "hres" | "chhres" => {
                return self.issue(span, Severity::Info, format!("picture-resolution:{name}"));
            }
            // Word 6 animated-text effect with no Strict representation; modern
            // renderers (and WPS) draw the text unanimated.
            "animtext" => {
                return self.issue(span, Severity::Info, format!("character-compat:{name}"));
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
/// Maps an RTF border line-style control word to a WML [`BorderStyle`].
fn border_style(name: &str) -> Option<BorderStyle> {
    Some(match name {
        "brdrs" => BorderStyle::Single,
        "brdrth" => BorderStyle::Thick,
        "brdrdb" => BorderStyle::Double,
        "brdrdot" => BorderStyle::Dotted,
        "brdrdash" => BorderStyle::Dashed,
        "brdrinset" => BorderStyle::Inset,
        "brdroutset" => BorderStyle::Outset,
        "brdrtriple" => BorderStyle::Triple,
        "brdrdashsm" => BorderStyle::DashSmallGap,
        "brdrdashd" => BorderStyle::DotDash,
        "brdrdashdd" => BorderStyle::DotDotDash,
        "brdrdashdotstr" => BorderStyle::DashDotStroked,
        "brdrtnthsg" => BorderStyle::ThinThickSmallGap,
        "brdrthtnsg" => BorderStyle::ThickThinSmallGap,
        "brdrtnthtnsg" => BorderStyle::ThinThickThinSmallGap,
        "brdrtnthmg" => BorderStyle::ThinThickMediumGap,
        "brdrthtnmg" => BorderStyle::ThickThinMediumGap,
        "brdrtnthtnmg" => BorderStyle::ThinThickThinMediumGap,
        "brdrtnthlg" => BorderStyle::ThinThickLargeGap,
        "brdrthtnlg" => BorderStyle::ThickThinLargeGap,
        "brdrtnthtnlg" => BorderStyle::ThinThickThinLargeGap,
        "brdrwavy" => BorderStyle::Wave,
        "brdrwavydb" => BorderStyle::DoubleWave,
        "brdrnil" => BorderStyle::Nil,
        "brdrnone" => BorderStyle::None,
        _ => return None,
    })
}
/// Converts an RTF `\brdrwN` (twips) to `w:sz` (eighths of a point), clamped to
/// the `ST_EighthsPoint` range `0..=96`.
fn border_eighths(twips: i32) -> u16 {
    let eighths = if twips <= 0 {
        0
    } else {
        (i64::from(twips) * 8 + 10) / 20
    };
    u16::try_from(eighths.clamp(0, 96)).expect("clamped to ST_EighthsPoint range")
}
/// Converts an RTF `\brspN` (twips) to `w:space` (points), clamped to `0..=31`.
fn border_space_points(twips: i32) -> u16 {
    let points = if twips <= 0 {
        0
    } else {
        (i64::from(twips) + 10) / 20
    };
    u16::try_from(points.clamp(0, 31)).expect("clamped to point range")
}
/// Maps an RTF shading intensity (`\shdng`, `\clshdng`, ... 0..=10000) to an
/// `ST_Shd` pattern: `clear`, `solid` or the nearest `pctNN`.
fn shading_pattern(value: i32) -> String {
    if value <= 0 {
        "clear".to_owned()
    } else if value >= 10_000 {
        "solid".to_owned()
    } else {
        let pct = ((i64::from(value) + 250) / 500) * 5;
        format!("pct{}", pct.clamp(5, 95))
    }
}
/// Replaces each direction-mixed, text-only run with direction-homogeneous runs.
/// Runs that already carry `w:rtl` or hold non-text content are left untouched.
fn split_mixed_directional_runs(inlines: &mut Vec<Inline>) {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in std::mem::take(inlines) {
        let Inline::Run(run) = inline else {
            out.push(inline);
            continue;
        };
        let text_only = run
            .content
            .iter()
            .all(|content| matches!(content, RunContent::Text(_)));
        if run.props.rtl != TriState::Absent || !text_only {
            out.push(Inline::Run(run));
            continue;
        }
        let text: String = run
            .content
            .iter()
            .filter_map(|content| match content {
                RunContent::Text(node) => Some(node.text.as_str()),
                _ => None,
            })
            .collect();
        let segments = bidi_direction_segments(&text);
        if segments.len() <= 1 {
            out.push(Inline::Run(run));
            continue;
        }
        for (segment, rtl) in segments {
            let mut props = run.props.clone();
            props.rtl = if rtl { TriState::On } else { TriState::Absent };
            out.push(Inline::Run(Run {
                props,
                content: vec![RunContent::Text(TextNode {
                    text: segment,
                    space: Space::Preserve,
                })],
                revision: run.revision.clone(),
                location: run.location.clone(),
            }));
        }
    }
    *inlines = out;
}

/// Splits `text` into logical-order segments of one direction each using the
/// Unicode Bidirectional Algorithm. A single-direction string returns one
/// segment, so the caller only acts on genuinely mixed text.
fn bidi_direction_segments(text: &str) -> Vec<(String, bool)> {
    let bidi = BidiInfo::new(text, None);
    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (index, _) in text.char_indices() {
        let rtl = bidi
            .levels
            .get(index)
            .is_some_and(unicode_bidi::Level::is_rtl);
        match current {
            Some(previous) if previous == rtl => {}
            Some(previous) => {
                segments.push((text[start..index].to_owned(), previous));
                start = index;
                current = Some(rtl);
            }
            None => current = Some(rtl),
        }
    }
    if let Some(previous) = current {
        segments.push((text[start..].to_owned(), previous));
    }
    if segments.is_empty() {
        segments.push((text.to_owned(), false));
    }
    segments
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
        font_theme: None,
        pending_font_theme: None,
        single_font_active: false,
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
        section_reset_pending: false,
        document_page_size,
        document_page_margins,
        sections: vec![],
        row: None,
        table: None,
        table_look: TableLook::default(),
        table_rtl: false,
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
        bookmarks: std::collections::HashMap::new(),
        bookmark_seq: 0,
        even_and_odd_headers: false,
        column_index: 0,
        footnotes: NoteTable::new(),
        endnotes: NoteTable::new(),
        document_footnote: NoteProperties::default(),
        document_endnote: NoteProperties::default(),
        numbering: NumberingTable::new(),
        list_draft: None,
        level_draft: None,
        override_draft: None,
        override_level_draft: None,
        table_gap: None,
        table_padding: CellMargins::default(),
        table_layout: None,
        default_tab_stop: None,
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
                if p.override_level_draft
                    .as_ref()
                    .is_some_and(|d| d.depth == p.stack.len())
                {
                    p.finish_override_level(span.start)?;
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
    let default_fonts = p
        .default_font
        .or_else(|| p.fonts.keys().next().copied())
        .and_then(|id| p.fonts.get(&id))
        .map(|f| {
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
        settings: strict_ooxml_wml::model::settings::Settings {
            default_tab_stop: p.default_tab_stop,
            even_and_odd_headers: p.even_and_odd_headers,
            footnote_properties: p.document_footnote,
            endnote_properties: p.document_endnote,
            ..Default::default()
        },
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
