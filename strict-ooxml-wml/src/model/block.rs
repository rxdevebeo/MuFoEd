//! Block-level model: paragraphs, tables and structured document tags.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;

use super::ids::{ParaId, TextId};
use super::inline::Inline;
use super::props::{
    CellProperties, ParagraphProperties, RowProperties, RunProperties, TableProperties,
};
use super::revision::Revision;
use super::values::{Rsids, Twips};

/// A paragraph (`w:p`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    /// Paragraph properties.
    pub props: ParagraphProperties,
    /// Inline content.
    pub inlines: Vec<Inline>,
    /// Revision identifiers carried by the paragraph and its runs.
    pub rsids: Rsids,
    /// Tracked change of the paragraph mark (`w:pPr/w:rPr/w:ins|w:del`, ADR-0018).
    pub revision: Option<Revision>,
    /// Unique paragraph id (`w14:paraId`).
    pub para_id: Option<ParaId>,
    /// Unique text id (`w14:textId`).
    pub text_id: Option<TextId>,
    /// Source location of the paragraph.
    pub location: SourceLocation,
}

/// A table grid column (`w:gridCol`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GridCol {
    /// Column width in twips.
    pub width: Option<Twips>,
}

/// The grid a table had before a tracked change (`w:tblGridChange`).
///
/// The live [`Table::grid`] is what layout paints. This copy is the previous
/// `w:tblGrid` Word stored inside the change markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableGridChange {
    /// Revision id (`w:id`).
    pub id: u32,
    /// Column widths before the change.
    pub grid: Vec<GridCol>,
}

/// A table (`w:tbl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// Table properties.
    pub props: TableProperties,
    /// Table grid.
    pub grid: Vec<GridCol>,
    /// Previous grid from `w:tblGrid/w:tblGridChange`, when the source had one.
    pub grid_change: Option<TableGridChange>,
    /// Table rows.
    pub rows: Vec<TableRow>,
    /// Source location.
    pub location: SourceLocation,
}

/// A table row (`w:tr`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    /// Row properties.
    pub props: RowProperties,
    /// Cells.
    pub cells: Vec<TableCell>,
    /// Properties of a row-level `w:sdt` that wrapped this row (AUD-41).
    ///
    /// The parser unwraps `sdtContent/w:tr` into ordinary rows and keeps the
    /// control's `sdtPr` here so the writer can restore the wrapper.
    pub sdt: Option<SdtProperties>,
    /// Source location.
    pub location: SourceLocation,
}

/// A table cell (`w:tc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell {
    /// Cell properties.
    pub props: CellProperties,
    /// Block content (paragraphs, nested tables).
    pub blocks: Vec<Block>,
    /// Properties of a cell-level `w:sdt` that wrapped this cell (AUD-41).
    pub sdt: Option<SdtProperties>,
    /// Source location.
    pub location: SourceLocation,
}

/// Identity fields of a structured document tag (`w:sdtPr`).
///
/// Shared by block/inline [`SdtContainer`] and by row/cell-level unwrapping
/// (AUD-41), where the control is not itself a block in the model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtProperties {
    /// Tag value (`w:tag`).
    pub tag: Option<Arc<str>>,
    /// Friendly alias (`w:alias`).
    pub alias: Option<Arc<str>>,
    /// Numeric id (`w:id`).
    pub id: Option<Arc<str>>,
    /// Placeholder text (`w:placeholder`).
    pub placeholder: Option<Arc<str>>,
    /// Whether the control shows an empty placeholder.
    pub showing_placeholder: bool,
    /// Placeholder run properties (`w:sdtPr/w:rPr`), including `w:sz`.
    pub run_props: Option<Box<RunProperties>>,
    /// Run properties of the control's end marker (`w:sdtEndPr/w:rPr`).
    pub end_run_props: Option<Box<RunProperties>>,
    /// The source had `w:sdtEndPr`, including when it carried no `w:rPr`.
    pub has_end_pr: bool,
    /// Building-block gallery (`w:docPartObj/w:docPartGallery/@w:val`).
    pub doc_part_gallery: Option<Arc<str>>,
    /// Gallery entry is unique (`w:docPartUnique`).
    pub doc_part_unique: bool,
    /// Building-block category (`w:docPartObj/w:docPartCategory/@w:val`).
    pub doc_part_category: Option<Arc<str>>,
    /// Editing lock (`w:lock/@w:val`).
    pub lock: Option<SdtLock>,
    /// The control is removed once its content is edited (`w:temporary`).
    pub temporary: bool,
    /// Custom XML data binding (`w:dataBinding`).
    pub data_binding: Option<SdtDataBinding>,
    /// Display label (`w:label/@w:val`).
    pub label: Option<i64>,
    /// Keyboard tab order (`w:tabIndex/@w:val`).
    pub tab_index: Option<u64>,
    /// Control type: the `CT_SdtPr` type choice other than `w:docPartObj`,
    /// which keeps its own fields above. `None` means no type element was
    /// present (Word treats that as rich text).
    pub control: Option<SdtControl>,
    /// Support-report feature ids of `w:sdtPr` children the model does not
    /// keep. The parser records each one; the writer reports each as a loss.
    pub unmodelled: Vec<Arc<str>>,
    /// Source location of the `w:sdt` element.
    pub location: SourceLocation,
}

/// Editing lock of a content control (`ST_Lock`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SdtLock {
    /// `sdtLocked`: the control cannot be deleted.
    SdtLocked,
    /// `contentLocked`: the contents cannot be edited.
    ContentLocked,
    /// `unlocked`: no locking.
    Unlocked,
    /// `sdtContentLocked`: neither the control nor its contents can change.
    SdtContentLocked,
}

impl SdtLock {
    /// Parses an `ST_Lock` lexical value.
    #[must_use]
    pub fn from_xml(value: &str) -> Option<Self> {
        match value {
            "sdtLocked" => Some(Self::SdtLocked),
            "contentLocked" => Some(Self::ContentLocked),
            "unlocked" => Some(Self::Unlocked),
            "sdtContentLocked" => Some(Self::SdtContentLocked),
            _ => None,
        }
    }

    /// Returns the `ST_Lock` lexical value.
    #[must_use]
    pub fn as_xml(self) -> &'static str {
        match self {
            Self::SdtLocked => "sdtLocked",
            Self::ContentLocked => "contentLocked",
            Self::Unlocked => "unlocked",
            Self::SdtContentLocked => "sdtContentLocked",
        }
    }
}

/// Custom XML data binding of a content control (`CT_DataBinding`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtDataBinding {
    /// Namespace prefix mappings for `xpath` (`w:prefixMappings`).
    pub prefix_mappings: Option<Arc<str>>,
    /// Path expression into the custom XML part (`w:xpath`).
    pub xpath: Arc<str>,
    /// Custom XML part identity (`w:storeItemID`).
    pub store_item_id: Arc<str>,
}

/// One entry of a combo box or drop-down list (`CT_SdtListItem`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtListItem {
    /// Displayed text (`w:displayText`).
    pub display_text: Option<Arc<str>>,
    /// Stored value (`w:value`).
    pub value: Option<Arc<str>>,
}

/// How a date control stores its value in bound XML (`ST_SdtDateMappingType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SdtDateMapping {
    /// `text`.
    Text,
    /// `date`.
    Date,
    /// `dateTime`.
    DateTime,
}

impl SdtDateMapping {
    /// Parses an `ST_SdtDateMappingType` lexical value.
    #[must_use]
    pub fn from_xml(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "date" => Some(Self::Date),
            "dateTime" => Some(Self::DateTime),
            _ => None,
        }
    }

    /// Returns the `ST_SdtDateMappingType` lexical value.
    #[must_use]
    pub fn as_xml(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Date => "date",
            Self::DateTime => "dateTime",
        }
    }
}

/// Date picker settings (`CT_SdtDate`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtDate {
    /// Full date-time value (`w:fullDate`, `ST_DateTime`).
    pub full_date: Option<Arc<str>>,
    /// Display format (`w:dateFormat/@w:val`).
    pub format: Option<Arc<str>>,
    /// Language of the display format (`w:lid/@w:val`).
    pub lid: Option<Arc<str>>,
    /// Storage mapping (`w:storeMappedDataAs/@w:val`).
    pub store_mapped_as: Option<SdtDateMapping>,
    /// Calendar (`w:calendar/@w:val`, `ST_CalendarType` lexical value).
    pub calendar: Option<Arc<str>>,
}

/// Building-block gallery filter (`CT_SdtDocPart`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtDocPart {
    /// Gallery (`w:docPartGallery/@w:val`).
    pub gallery: Option<Arc<str>>,
    /// Category (`w:docPartCategory/@w:val`).
    pub category: Option<Arc<str>>,
    /// Entries are unique (`w:docPartUnique`).
    pub unique: bool,
}

/// One state of a check box control (`w14:checkedState`/`w14:uncheckedState`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdtCheckboxState {
    /// Character code in hexadecimal (`w14:val`).
    pub value: Option<Arc<str>>,
    /// Font of the character (`w14:font`).
    pub font: Option<Arc<str>>,
}

/// Control type of a content control: the type choice of `CT_SdtPr`.
///
/// `w:docPartObj` is kept by [`SdtProperties::doc_part_gallery`] and its
/// siblings rather than here, for compatibility with earlier models.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SdtControl {
    /// `w:richText`.
    RichText,
    /// `w:text`, plain text.
    Text {
        /// `w:multiLine`: soft line breaks are allowed.
        multi_line: bool,
    },
    /// `w:comboBox`.
    ComboBox {
        /// List entries in source order.
        items: Vec<SdtListItem>,
        /// Last selected value (`w:lastValue`).
        last_value: Option<Arc<str>>,
    },
    /// `w:dropDownList`.
    DropDownList {
        /// List entries in source order.
        items: Vec<SdtListItem>,
        /// Last selected value (`w:lastValue`).
        last_value: Option<Arc<str>>,
    },
    /// `w:date`.
    Date(SdtDate),
    /// `w:picture`.
    Picture,
    /// `w:docPartList`.
    DocPartList(SdtDocPart),
    /// `w:citation`.
    Citation,
    /// `w:bibliography`.
    Bibliography,
    /// `w:equation`.
    Equation,
    /// `w:group`.
    Group,
    /// `w14:checkbox`, a Microsoft extension with no ISO/IEC 29500 Strict form.
    Checkbox {
        /// `w14:checked`.
        checked: bool,
        /// `w14:checkedState`.
        checked_state: Option<SdtCheckboxState>,
        /// `w14:uncheckedState`.
        unchecked_state: Option<SdtCheckboxState>,
    },
}

impl SdtControl {
    /// Returns the qualified element name of this control type.
    #[must_use]
    pub fn element_name(&self) -> &'static str {
        match self {
            Self::RichText => "w:richText",
            Self::Text { .. } => "w:text",
            Self::ComboBox { .. } => "w:comboBox",
            Self::DropDownList { .. } => "w:dropDownList",
            Self::Date(_) => "w:date",
            Self::Picture => "w:picture",
            Self::DocPartList(_) => "w:docPartList",
            Self::Citation => "w:citation",
            Self::Bibliography => "w:bibliography",
            Self::Equation => "w:equation",
            Self::Group => "w:group",
            Self::Checkbox { .. } => "w14:checkbox",
        }
    }
}

/// A structured document tag (`w:sdt`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdtContainer {
    /// Tag value (`w:tag`).
    pub tag: Option<Arc<str>>,
    /// Friendly alias (`w:alias`).
    pub alias: Option<Arc<str>>,
    /// Numeric id (`w:id`).
    pub id: Option<Arc<str>>,
    /// Placeholder text (`w:placeholder`).
    pub placeholder: Option<Arc<str>>,
    /// Whether the control shows an empty placeholder.
    pub showing_placeholder: bool,
    /// Placeholder run properties (`w:sdtPr/w:rPr`), including `w:sz`.
    pub run_props: Option<Box<RunProperties>>,
    /// Run properties of the control's end marker (`w:sdtEndPr/w:rPr`).
    pub end_run_props: Option<Box<RunProperties>>,
    /// The source had `w:sdtEndPr`, including when it carried no `w:rPr`.
    pub has_end_pr: bool,
    /// Building-block gallery (`w:docPartObj/w:docPartGallery/@w:val`).
    pub doc_part_gallery: Option<Arc<str>>,
    /// Gallery entry is unique (`w:docPartUnique`).
    pub doc_part_unique: bool,
    /// Building-block category (`w:docPartObj/w:docPartCategory/@w:val`).
    pub doc_part_category: Option<Arc<str>>,
    /// Editing lock (`w:lock/@w:val`).
    pub lock: Option<SdtLock>,
    /// The control is removed once its content is edited (`w:temporary`).
    pub temporary: bool,
    /// Custom XML data binding (`w:dataBinding`).
    pub data_binding: Option<SdtDataBinding>,
    /// Display label (`w:label/@w:val`).
    pub label: Option<i64>,
    /// Keyboard tab order (`w:tabIndex/@w:val`).
    pub tab_index: Option<u64>,
    /// Control type: the `CT_SdtPr` type choice other than `w:docPartObj`,
    /// which keeps its own fields above. `None` means no type element was
    /// present (Word treats that as rich text).
    pub control: Option<SdtControl>,
    /// Support-report feature ids of `w:sdtPr` children the model does not
    /// keep. The parser records each one; the writer reports each as a loss.
    pub unmodelled: Vec<Arc<str>>,
    /// Block content when the tag is block-level.
    pub blocks: Vec<Block>,
    /// Inline content when the tag is inline-level.
    pub inlines: Vec<Inline>,
    /// Source location.
    pub location: SourceLocation,
}

impl SdtContainer {
    /// Returns the identity fields of this control as [`SdtProperties`].
    #[must_use]
    pub fn properties(&self) -> SdtProperties {
        SdtProperties {
            tag: self.tag.clone(),
            alias: self.alias.clone(),
            id: self.id.clone(),
            placeholder: self.placeholder.clone(),
            showing_placeholder: self.showing_placeholder,
            run_props: self.run_props.clone(),
            end_run_props: self.end_run_props.clone(),
            has_end_pr: self.has_end_pr || self.end_run_props.is_some(),
            doc_part_gallery: self.doc_part_gallery.clone(),
            doc_part_unique: self.doc_part_unique,
            doc_part_category: self.doc_part_category.clone(),
            lock: self.lock,
            temporary: self.temporary,
            data_binding: self.data_binding.clone(),
            label: self.label,
            tab_index: self.tab_index,
            control: self.control.clone(),
            unmodelled: self.unmodelled.clone(),
            location: self.location.clone(),
        }
    }
}

/// An unknown block-level element preserved for the support report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueBlock {
    /// Namespace URI (empty when none).
    pub namespace: Arc<str>,
    /// Local name.
    pub local: Arc<str>,
    /// Attributes of the element.
    pub attributes: Vec<(Arc<str>, Arc<str>)>,
    /// Source location.
    pub location: SourceLocation,
}

impl OpaqueBlock {
    /// Returns a stable feature identifier for the element.
    #[must_use]
    pub fn feature_id(&self) -> String {
        if self.namespace.is_empty() {
            self.local.to_string()
        } else {
            format!("w:{}", self.local)
        }
    }
}

/// An alternative-format chunk (`w:altChunk`), never embedded (TZ decision Г.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltChunkInfo {
    /// Relationship id of the embedded chunk (`r:id`).
    pub rel_id: Option<RelId>,
    /// Resolved content type of the chunk part, if known.
    pub content_type: Option<Arc<str>>,
    /// Source location.
    pub location: SourceLocation,
}

/// Block-level content of the document body or a table cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Paragraph.
    Paragraph(Paragraph),
    /// Table.
    Table(Table),
    /// Block-level structured document tag.
    SdtBlock(SdtContainer),
    /// Alternative-format chunk.
    AltChunk(AltChunkInfo),
    /// Unknown block-level element.
    Opaque(OpaqueBlock),
}

impl Block {
    /// Returns the paragraph, if this block is one.
    #[must_use]
    pub fn as_paragraph(&self) -> Option<&Paragraph> {
        match self {
            Self::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        }
    }

    /// Returns the table, if this block is one.
    #[must_use]
    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }
}
