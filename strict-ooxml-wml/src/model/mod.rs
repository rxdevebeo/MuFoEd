//! WordprocessingML Strict document model (DOM).
//!
//! The model is pure data: no parsing logic lives here. Every node carries a
//! [`SourceLocation`](strict_ooxml_core::error::SourceLocation) and unknown
//! constructs are preserved as opaque nodes (ADR-0004, STAGE-2 §6).

pub mod block;
pub mod document;
pub mod drawing;
pub mod ids;
pub mod inline;
pub mod numbering;
pub mod props;
pub mod settings;
pub mod styles;
pub mod support;
pub mod values;

pub use block::{
    AltChunkInfo, Block, GridCol, OpaqueBlock, Paragraph, SdtContainer, Table, TableCell, TableRow,
};
pub use document::{Body, Document, DocumentSource};
pub use drawing::{
    AnchorStub, BlipRef, DocPr, Drawing, DrawingKind, Extent, InlineDrawing, MediaIndex, MediaItem,
    MediaKind, Picture,
};
pub use ids::{AbstractNumId, Ilvl, NumId, ParaId, StyleId, TextId};
pub use inline::{
    BookmarkId, CommentId, Field, FieldChar, Hyperlink, Inline, OpaqueInline, Run, RunContent,
    Symbol, TextNode,
};
pub use numbering::{AbstractNum, Level, LevelOverride, Num, NumberingTable};
pub use props::{
    CellProperties, ColumnSpec, Columns, DocGrid, HeaderFooterKind, HeaderFooterRef, Language,
    LineNumbering, NumPr, PageMargins, PageSize, ParagraphProperties, RowProperties, RunProperties,
    Section, SectionProperties, TableProperties,
};
pub use settings::{DocumentZoom, Settings, Zoom};
pub use styles::{Style, StyleTable};
pub use support::{FeatureUse, SupportModel, SupportStatus};
pub use values::{
    Border, BorderStyle, Borders, BreakKind, CellMargins, Color, DocGridType, EighthsPoint, Emu,
    FieldCharType, Fonts, HalfPoints, HeightRule, Highlight, HighlightOrColor, Indentation,
    Justification, LineNumberRestart, LineSpacingRule, PageOrientation, RowHeight, Rsids,
    SectionType, Shading, Space, Spacing, StyleType, TabAlignment, TabLeader, TabStop, TableLayout,
    TableLook, TextDirection, ThemeColor, TriState, Twips, Underline, UnderlineSpec, VertAlign,
    VerticalJc, VerticalMerge, Width, WidthKind,
};
