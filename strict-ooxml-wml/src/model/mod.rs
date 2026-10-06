//! WordprocessingML Strict document model (DOM).
//!
//! The model is pure data: no parsing logic lives here. Every node carries a
//! [`SourceLocation`](strict_ooxml_core::error::SourceLocation) and unknown
//! constructs are preserved as opaque nodes (ADR-0004, STAGE-2 §6).

pub mod block;
pub mod document;
pub mod drawing;
pub mod fonts;
pub mod ids;
pub mod inline;
pub mod math;
pub mod notes;
pub mod numbering;
pub mod props;
pub mod revision;
pub mod settings;
pub mod styles;
pub mod support;
pub mod theme;
pub mod values;

pub use block::{
    AltChunkInfo, Block, GridCol, OpaqueBlock, Paragraph, SdtContainer, SdtProperties, Table,
    TableCell, TableRow,
};
pub use document::{Body, Document, DocumentSource, HeaderFooter};
pub use drawing::{
    AnchorDrawing, BlipRef, CustomGeometry, DocPr, Drawing, DrawingKind, EffectExtent, Extent,
    ForeignRefs, GeometryPath, GradientStop, Graphic, GroupShape, GroupTransform, InlineDrawing,
    LockedCanvas, MediaIndex, MediaItem, MediaKind, PathCommand, Picture, Position, Shape,
    ShapeColor, ShapeFill, ShapeGeometry, ShapeStroke, ShapeStyle, SrcRect, TextAnchor, TextBox,
    TextBoxBody, Wrap, WrapKind, Xfrm,
};
pub use fonts::{EmbedKind, EmbeddedFont, FontEntry, FontHints, FontSig, FontTable};
pub use ids::{AbstractNumId, Ilvl, NumId, ParaId, StyleId, TextId};
pub use inline::{
    Bookmark, BookmarkId, CommentId, Directional, DirectionalKind, DirectionalVal, Field,
    FieldChar, Hyperlink, Inline, OpaqueInline, Run, RunContent, Symbol, TextNode,
};
pub use math::{
    Accent, ArgumentProperties, Bar, BorderBox, Boxed, Delimiter, EquationArray, Fraction,
    Function, GroupCharacter, LimitLocation, MathAlignment, MathArgument, MathExpression,
    MathLimit, MathNode, MathParagraph, MathParagraphProperties, MathPosition, MathRun,
    MathRunProperties, MathScript, MathStyle, MathVerticalJc, Matrix, MatrixColumn, NaryOperator,
    Phantom, PreScript, Radical, SubSuperscript, Subscript, Superscript, UnknownMathNode,
};
pub use notes::{Note, NoteKind, NoteProperties, NoteTable};
pub use numbering::{AbstractNum, Level, LevelOverride, Num, NumberingTable};
pub use props::{
    BorderOffsetFrom, BorderZOrder, CellProperties, ColumnSpec, Columns, DocGrid, FrameProperties,
    HeaderFooterKind, HeaderFooterRef, Language, LineNumbering, NumPr, PageBorder, PageBorders,
    PageMargins, PageNumberType, PageSize, ParagraphProperties, RowProperties, RunProperties,
    Section, SectionProperties, TablePositioning, TableProperties,
};
pub use revision::{Revision, RevisionKind};
pub use settings::{DocumentZoom, Settings, Zoom};
pub use styles::{Style, StyleTable};
pub use support::{FeatureUse, SupportModel, SupportStatus, SUPPORT_OVERFLOW_ID};
pub use theme::{FontSet, Theme, ThemeColors, ThemeFonts};
pub use values::{
    Border, BorderStyle, Borders, BreakKind, CellMargins, Color, DocGridType, EighthsPoint, Emu,
    FieldCharType, Fonts, HalfPoints, HeightRule, Highlight, HighlightOrColor, Indentation,
    Justification, LineNumberRestart, LineSpacingRule, PageOrientation, RowHeight, Rsids,
    SectionType, Shading, Space, Spacing, StyleType, TabAlignment, TabLeader, TabStop, TableLayout,
    TableLook, TextDirection, ThemeColor, TriState, Twips, Underline, UnderlineSpec, VertAlign,
    VerticalJc, VerticalMerge, Width, WidthKind,
};
