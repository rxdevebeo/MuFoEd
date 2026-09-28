//! Paragraph, run, table and section property types.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;

use super::ids::{Ilvl, NumId, StyleId};
use super::values::{
    Borders, CellMargins, Color, DocGridType, Fonts, Indentation, Justification, LineNumberRestart,
    PageOrientation, SectionType, Shading, Spacing, TabStop, TableLayout, TableLook, TextDirection,
    TriState, Twips, Underline, VertAlign, VerticalJc, Width,
};

/// Numbering reference (`w:numPr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NumPr {
    /// Numbering instance id (`w:numId`).
    pub num_id: Option<NumId>,
    /// List level (`w:ilvl`).
    pub ilvl: Option<Ilvl>,
}

/// Language tags (`w:lang`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Language {
    /// Primary language.
    pub val: Option<Arc<str>>,
    /// East-Asian language.
    pub east_asia: Option<Arc<str>>,
    /// Bidirectional language.
    pub bidi: Option<Arc<str>>,
}

/// Paragraph properties (`w:pPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ParagraphProperties {
    /// Paragraph style reference (`w:pStyle`).
    pub style: Option<StyleId>,
    /// Alignment (`w:jc`).
    pub alignment: Option<Justification>,
    /// Numbering reference (`w:numPr`).
    pub numbering: Option<NumPr>,
    /// Spacing (`w:spacing`).
    pub spacing: Option<Spacing>,
    /// Indentation (`w:ind`).
    pub indentation: Option<Indentation>,
    /// Borders (`w:pBdr`).
    pub borders: Borders,
    /// Shading (`w:shd`).
    pub shading: Option<Shading>,
    /// Custom tab stops (`w:tabs`).
    pub tabs: Vec<TabStop>,
    /// Keep with next paragraph (`w:keepNext`).
    pub keep_next: bool,
    /// Keep lines together (`w:keepLines`).
    pub keep_lines: bool,
    /// Start on a new page (`w:pageBreakBefore`).
    pub page_break_before: bool,
    /// Widow/orphan control (`w:widowControl`), tri-state.
    pub widow_control: TriState,
    /// Outline level 0..=9 (`w:outlineLvl`).
    pub outline_level: Option<u8>,
    /// Bidirectional paragraph (`w:bidi`).
    pub bidi: bool,
    /// Marker run properties (`w:rPr` inside `w:pPr`).
    pub run_props: Option<RunProperties>,
    /// Section properties (`w:sectPr` inside `w:pPr`).
    pub section: Option<SectionProperties>,
    /// Text flow direction (`w:textDirection`).
    pub text_direction: Option<TextDirection>,
    /// Suppress line numbers (`w:suppressLineNumbers`).
    pub suppress_line_numbers: bool,
    /// Contextual spacing (`w:contextualSpacing`).
    pub contextual_spacing: bool,
    /// Word wrap (`w:wordWrap`).
    pub word_wrap: TriState,
    /// Snap to grid (`w:snapToGrid`).
    pub snap_to_grid: TriState,
    /// Source location of `w:pPr`.
    pub location: Option<SourceLocation>,
}

/// The set of formatting toggles shared by `w:rPr` runs.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RunProperties {
    /// Character style reference (`w:rStyle`).
    pub style: Option<StyleId>,
    /// Fonts (`w:rFonts`).
    pub fonts: Option<Fonts>,
    /// Bold (`w:b`).
    pub bold: TriState,
    /// Italic (`w:i`).
    pub italic: TriState,
    /// Underline style (`w:u`).
    pub underline: Option<Underline>,
    /// Underline colour (`w:u/@w:color`).
    pub underline_color: Option<Color>,
    /// Strikethrough (`w:strike`).
    pub strike: TriState,
    /// Double strikethrough (`w:dstrike`).
    pub double_strike: TriState,
    /// Text colour (`w:color`).
    pub color: Option<Color>,
    /// Highlight (`w:highlight`).
    pub highlight: Option<super::values::Highlight>,
    /// Font size in half-points (`w:sz`).
    pub size: Option<super::values::HalfPoints>,
    /// Complex-script font size in half-points (`w:szCs`).
    pub size_cs: Option<super::values::HalfPoints>,
    /// Vertical alignment (`w:vertAlign`).
    pub vert_align: Option<VertAlign>,
    /// Character spacing in twips (`w:spacing`).
    pub spacing: Option<Twips>,
    /// Vertical position in half-points (`w:position`).
    pub position: Option<super::values::HalfPoints>,
    /// All capitals (`w:caps`).
    pub caps: bool,
    /// Small capitals (`w:smallCaps`).
    pub small_caps: bool,
    /// Right-to-left run (`w:rtl`).
    pub rtl: bool,
    /// Hidden text (`w:vanish`).
    pub vanish: bool,
    /// Emboss (`w:emboss`).
    pub emboss: bool,
    /// Imprint (`w:imprint`).
    pub imprint: bool,
    /// Outline (`w:outline`).
    pub outline: bool,
    /// Shadow (`w:shadow`).
    pub shadow: bool,
    /// Do not proof (`w:noProof`).
    pub no_proof: bool,
    /// Snap to grid (`w:snapToGrid`).
    pub snap_to_grid: TriState,
    /// Character scale percentage (`w:w`).
    pub scale: Option<u16>,
    /// Kerning threshold in half-points (`w:kern`).
    pub kerning: Option<super::values::HalfPoints>,
    /// Emphasis mark (`w:em`).
    pub emphasis: Option<Arc<str>>,
    /// Language (`w:lang`).
    pub language: Option<Language>,
    /// Borders (`w:bdr`).
    pub borders: Borders,
    /// Shading (`w:shd`).
    pub shading: Option<Shading>,
    /// Source location of `w:rPr`.
    pub location: Option<SourceLocation>,
}

/// Table properties (`w:tblPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TableProperties {
    /// Table style reference (`w:tblStyle`).
    pub style: Option<StyleId>,
    /// Preferred width (`w:tblW`).
    pub width: Option<Width>,
    /// Table alignment (`w:jc`).
    pub alignment: Option<Justification>,
    /// Layout algorithm (`w:tblLayout`).
    pub layout: Option<TableLayout>,
    /// Default cell margins (`w:tblCellMar`).
    pub cell_margins: CellMargins,
    /// Table borders (`w:tblBorders`).
    pub borders: Borders,
    /// Shading (`w:shd`).
    pub shading: Option<Shading>,
    /// Conditional-formatting lookup (`w:tblLook`).
    pub look: Option<TableLook>,
    /// Table indent (`w:tblInd`).
    pub indent: Option<Twips>,
    /// Visual right-to-left table (`w:bidiVisual`).
    pub bidi_visual: bool,
    /// Source location of `w:tblPr`.
    pub location: Option<SourceLocation>,
}

/// Table row properties (`w:trPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RowProperties {
    /// Row height (`w:trHeight`).
    pub height: Option<super::values::RowHeight>,
    /// Repeat as a header row on each page (`w:tblHeader`).
    pub header: bool,
    /// Do not split the row across pages (`w:cantSplit`).
    pub cant_split: bool,
    /// Row cell margins (`w:tblCellMar`).
    pub cell_margins: CellMargins,
    /// Grid columns before the row (`w:gridBefore`).
    pub grid_before: Option<i32>,
    /// Grid columns after the row (`w:gridAfter`).
    pub grid_after: Option<i32>,
    /// Preferred width before (`w:wBefore`).
    pub width_before: Option<Width>,
    /// Preferred width after (`w:wAfter`).
    pub width_after: Option<Width>,
    /// Revision id (`w:rsid`).
    pub rsid: Option<Arc<str>>,
    /// Source location of `w:trPr`.
    pub location: Option<SourceLocation>,
}

/// Table cell properties (`w:tcPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CellProperties {
    /// Preferred width (`w:tcW`).
    pub width: Option<Width>,
    /// Number of grid columns spanned (`w:gridSpan`).
    pub grid_span: Option<u16>,
    /// Vertical merge state (`w:vMerge`).
    pub vertical_merge: Option<super::values::VerticalMerge>,
    /// Vertical alignment (`w:vAlign`).
    pub vertical_align: Option<VerticalJc>,
    /// Text direction (`w:textDirection`).
    pub text_direction: Option<TextDirection>,
    /// Borders (`w:tcBorders`).
    pub borders: Borders,
    /// Shading (`w:shd`).
    pub shading: Option<Shading>,
    /// Cell margins (`w:tcMar`).
    pub margins: CellMargins,
    /// Hide the cell mark (`w:hideMark`).
    pub hide_mark: bool,
    /// Fit text (`w:tcFitText`).
    pub fit_text: bool,
    /// Do not wrap text (`w:noWrap`).
    pub no_wrap: bool,
    /// Source location of `w:tcPr`.
    pub location: Option<SourceLocation>,
}

/// Page size (`w:pgSz`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PageSize {
    /// Page width in twips.
    pub width: Option<Twips>,
    /// Page height in twips.
    pub height: Option<Twips>,
    /// Orientation.
    pub orientation: Option<PageOrientation>,
}

/// Page margins (`w:pgMar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PageMargins {
    /// Top margin in twips.
    pub top: Option<Twips>,
    /// Right margin in twips.
    pub right: Option<Twips>,
    /// Bottom margin in twips.
    pub bottom: Option<Twips>,
    /// Left margin in twips.
    pub left: Option<Twips>,
    /// Header margin in twips.
    pub header: Option<Twips>,
    /// Footer margin in twips.
    pub footer: Option<Twips>,
    /// Gutter margin in twips.
    pub gutter: Option<Twips>,
}

/// A single column definition inside `w:cols`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ColumnSpec {
    /// Column width in twips.
    pub width: Option<Twips>,
    /// Space after the column in twips.
    pub space: Option<Twips>,
}

/// Column configuration (`w:cols`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Columns {
    /// Number of columns.
    pub count: Option<u16>,
    /// Default space between columns in twips.
    pub space: Option<Twips>,
    /// Equal column widths.
    pub equal_width: bool,
    /// Draw a separator between columns.
    pub separator: bool,
    /// Explicit column definitions.
    pub columns: Vec<ColumnSpec>,
}

/// Document grid (`w:docGrid`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DocGrid {
    /// Grid type.
    pub grid_type: Option<DocGridType>,
    /// Line pitch.
    pub line_pitch: Option<i32>,
    /// Character space.
    pub character_space: Option<i32>,
}

/// Line numbering (`w:lnNumType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LineNumbering {
    /// Count by this many lines.
    pub count_by: Option<u16>,
    /// Starting line number.
    pub start: Option<u16>,
    /// Restart mode.
    pub restart: Option<LineNumberRestart>,
    /// Distance from text.
    pub distance: Option<Twips>,
}

/// Which header/footer a reference points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeaderFooterKind {
    /// Default header/footer.
    Default,
    /// First-page header/footer.
    First,
    /// Even-page header/footer.
    Even,
}

/// A header or footer reference in a section (`w:headerReference`/`w:footerReference`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderFooterRef {
    /// Which header/footer this is.
    pub kind: HeaderFooterKind,
    /// Relationship id of the referenced part.
    pub rel_id: RelId,
}

/// Section properties (`w:sectPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SectionProperties {
    /// Page size (`w:pgSz`).
    pub page_size: Option<PageSize>,
    /// Page margins (`w:pgMar`).
    pub page_margins: Option<PageMargins>,
    /// Columns (`w:cols`).
    pub columns: Option<Columns>,
    /// Section break type (`w:type`).
    pub section_type: Option<SectionType>,
    /// Different first page (`w:titlePg`).
    pub title_page: bool,
    /// Header references.
    pub headers: Vec<HeaderFooterRef>,
    /// Footer references.
    pub footers: Vec<HeaderFooterRef>,
    /// Document grid (`w:docGrid`).
    pub doc_grid: Option<DocGrid>,
    /// Vertical alignment of the section (`w:vAlign`).
    pub vertical_align: Option<VerticalJc>,
    /// Bidirectional section (`w:bidi`).
    pub bidi: bool,
    /// Right-to-left gutter (`w:rtlGutter`).
    pub rtl_gutter: bool,
    /// Gutter at the top (`w:gutterAtTop`).
    pub gutter_at_top: bool,
    /// Text direction.
    pub text_direction: Option<TextDirection>,
    /// Line numbering.
    pub line_numbering: Option<LineNumbering>,
    /// Source location of `w:sectPr`.
    pub location: Option<SourceLocation>,
}

/// A resolved section of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Section properties.
    pub properties: SectionProperties,
    /// Source location of the section.
    pub location: SourceLocation,
}
