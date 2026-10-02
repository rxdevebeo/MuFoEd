//! DrawingML graphics model (drawings, anchors, shapes, groups, text boxes)
//! and the media index (`STAGE-5B-TASK.md` §3.1, §4).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::part::PartId;

use super::block::Block;
use super::inline::OpaqueInline;
use super::values::{Color, Emu, ThemeColorRef};

/// A DrawingML extent (`wp:extent`, `a:ext`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Extent {
    /// Width in EMU.
    pub cx: Emu,
    /// Height in EMU.
    pub cy: Emu,
}

/// An effect extent (`wp:effectExtent`), in EMU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct EffectExtent {
    /// Left extent.
    pub left: Emu,
    /// Top extent.
    pub top: Emu,
    /// Right extent.
    pub right: Emu,
    /// Bottom extent.
    pub bottom: Emu,
}

/// Non-visual drawing properties (`wp:docPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DocPr {
    /// Drawing object id.
    pub id: Option<u32>,
    /// Name.
    pub name: Option<Arc<str>>,
    /// Description.
    pub descr: Option<Arc<str>>,
}

/// A reference to an image part (`a:blip`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlipRef {
    /// `r:embed` relationship id.
    pub embed: Option<RelId>,
    /// `r:link` relationship id (external; not fetched).
    pub link: Option<RelId>,
    /// Resolved media part for `embed`.
    pub resolved: Option<PartId>,
    /// Source location.
    pub location: SourceLocation,
}

/// A source-rectangle crop (`a:srcRect`), in thousandths of a percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SrcRect {
    /// Left inset (`l`).
    pub left: i32,
    /// Top inset (`t`).
    pub top: i32,
    /// Right inset (`r`).
    pub right: i32,
    /// Bottom inset (`b`).
    pub bottom: i32,
}

/// A 2-D transform (`a:xfrm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Xfrm {
    /// Offset (`a:off`) in EMU.
    pub offset: Option<(Emu, Emu)>,
    /// Rotation in 60000ths of a degree (`rot`).
    pub rot: Option<i32>,
    /// Horizontal flip (`flipH`).
    pub flip_h: bool,
    /// Vertical flip (`flipV`).
    pub flip_v: bool,
}

/// A `pic:pic` picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Picture name (`pic:cNvPr/@name`).
    pub name: Option<Arc<str>>,
    /// Picture description (`pic:cNvPr/@descr`).
    pub descr: Option<Arc<str>>,
    /// The image reference.
    pub blip: Option<BlipRef>,
    /// Picture geometry extent.
    pub extent: Option<Extent>,
    /// Source-rectangle crop (`a:srcRect`).
    pub src_rect: Option<SrcRect>,
    /// Applied transform (`a:xfrm`) with rotation/flips.
    pub xfrm: Option<Xfrm>,
}

/// A colour that is either an explicit RGB value or a theme reference.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShapeColor {
    /// Explicit colour value.
    pub value: Option<Color>,
    /// Theme colour reference.
    pub theme: Option<ThemeColorRef>,
}

/// A gradient colour stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GradientStop {
    /// Position, in thousandths of a percent (`0..=100000`).
    pub position: i32,
    /// Stop colour.
    pub color: ShapeColor,
}

/// A shape fill (`a:solidFill`, `a:gradFill`, `a:pattFill`, `a:noFill`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeFill {
    /// No fill (`a:noFill`).
    None,
    /// Solid fill.
    Solid {
        /// Fill colour.
        color: ShapeColor,
    },
    /// Linear gradient fill.
    Gradient {
        /// Colour stops, in document order.
        stops: Vec<GradientStop>,
        /// Gradient angle in 60000ths of a degree.
        angle: Option<i32>,
    },
    /// Pattern fill (rendered as a flat foreground colour).
    Pattern {
        /// Pattern preset (`prst`).
        preset: Option<Arc<str>>,
        /// Foreground colour.
        foreground: Option<ShapeColor>,
        /// Background colour.
        background: Option<ShapeColor>,
    },
}

/// A shape outline (`a:ln`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShapeStroke {
    /// Explicit `a:noFill` / no line.
    pub none: bool,
    /// Line colour.
    pub color: Option<ShapeColor>,
    /// Line width in EMU.
    pub width: Option<Emu>,
    /// Preset dash (`a:prstDash/@val`).
    pub dash: Option<Arc<str>>,
    /// Head end decoration (`a:headEnd/@type`).
    pub head_end: Option<Arc<str>>,
    /// Tail end decoration (`a:tailEnd/@type`).
    pub tail_end: Option<Arc<str>>,
}

/// One command of a custom-geometry path (`a:custGeom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathCommand {
    /// `a:moveTo`.
    MoveTo {
        /// X in path units.
        x: i64,
        /// Y in path units.
        y: i64,
    },
    /// `a:lnTo`.
    LineTo {
        /// X in path units.
        x: i64,
        /// Y in path units.
        y: i64,
    },
    /// `a:cubicBezTo`.
    CubicBezTo {
        /// First control point X.
        x1: i64,
        /// First control point Y.
        y1: i64,
        /// Second control point X.
        x2: i64,
        /// Second control point Y.
        y2: i64,
        /// End point X.
        x: i64,
        /// End point Y.
        y: i64,
    },
    /// `a:close`.
    Close,
}

/// A custom geometry path (`a:custGeom/a:pathLst/a:path`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CustomGeometry {
    /// Path width (`a:path/@w`) in path units.
    pub width: i64,
    /// Path height (`a:path/@h`) in path units.
    pub height: i64,
    /// Flattened commands.
    pub commands: Vec<PathCommand>,
}

/// A shape's geometry (`a:prstGeom`/`a:custGeom`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeGeometry {
    /// No explicit geometry.
    None,
    /// Preset geometry name (`a:prstGeom/@prst`).
    Preset(Arc<str>),
    /// Custom geometry (subset of commands).
    Custom(CustomGeometry),
}

/// A `wps:style` reference block, carrying theme style indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ShapeStyle {
    /// Line reference index (`a:lnRef/@idx`).
    pub line_ref: Option<i32>,
    /// Fill reference index (`a:fillRef/@idx`).
    pub fill_ref: Option<i32>,
    /// Effect reference index (`a:effectRef/@idx`).
    pub effect_ref: Option<i32>,
    /// Font reference index (`a:fontRef/@idx`).
    pub font_ref: Option<i32>,
}

/// Vertical anchoring of text within a text box (`w:bodyPr/@anchor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAnchor {
    /// Top (`t`).
    Top,
    /// Center (`ctr`).
    Center,
    /// Bottom (`b`).
    Bottom,
}

/// Text-box body properties (`w:bodyPr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextBoxBody {
    /// Vertical anchor.
    pub anchor: Option<TextAnchor>,
    /// Center the text block horizontally (`anchorCtr`).
    pub anchor_centered: bool,
    /// Left inset (`lIns`) in EMU.
    pub left_inset: Option<Emu>,
    /// Top inset (`tIns`) in EMU.
    pub top_inset: Option<Emu>,
    /// Right inset (`rIns`) in EMU.
    pub right_inset: Option<Emu>,
    /// Bottom inset (`bIns`) in EMU.
    pub bottom_inset: Option<Emu>,
}

/// A shape's text body (`wps:txbx`/`w:txbxContent`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBox {
    /// Body properties.
    pub body: Option<TextBoxBody>,
    /// Block content (paragraphs, tables).
    pub blocks: Vec<Block>,
    /// Source location.
    pub location: SourceLocation,
}

/// A `wps:wsp` shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    /// Shape name (`wps:cNvPr/@name`).
    pub name: Option<Arc<str>>,
    /// Shape description (`wps:cNvPr/@descr`).
    pub descr: Option<Arc<str>>,
    /// Geometry.
    pub geometry: ShapeGeometry,
    /// Transform (`a:xfrm`).
    pub xfrm: Option<Xfrm>,
    /// Transform offset/extent (`a:off`/`a:ext`).
    pub offset: Option<(Emu, Emu)>,
    /// Transform extent (`a:ext`).
    pub extent: Option<Extent>,
    /// Fill.
    pub fill: Option<ShapeFill>,
    /// Outline.
    pub stroke: Option<ShapeStroke>,
    /// Text body.
    pub text: Option<TextBox>,
    /// Style reference block.
    pub style: Option<ShapeStyle>,
    /// Source location.
    pub location: SourceLocation,
}

/// A group's transform (`wpg:grpSpPr/a:xfrm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GroupTransform {
    /// Group offset (`a:off`).
    pub offset: Option<(Emu, Emu)>,
    /// Group extent (`a:ext`).
    pub extent: Option<Extent>,
    /// Child coordinate offset (`a:chOff`).
    pub child_offset: Option<(Emu, Emu)>,
    /// Child coordinate extent (`a:chExt`).
    pub child_extent: Option<Extent>,
    /// Rotation in 60000ths of a degree.
    pub rot: Option<i32>,
    /// Horizontal flip.
    pub flip_h: bool,
    /// Vertical flip.
    pub flip_v: bool,
}

/// A `wpg:wgp` group of graphics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupShape {
    /// Group name (`wpg:cNvPr/@name`).
    pub name: Option<Arc<str>>,
    /// Group description (`wpg:cNvPr/@descr`).
    pub descr: Option<Arc<str>>,
    /// Group transform.
    pub xfrm: Option<GroupTransform>,
    /// Child graphics.
    pub children: Vec<Graphic>,
    /// Source location.
    pub location: SourceLocation,
}

/// Relationship ids a `c:chart` or a `dgm:relIds` element carries, in document
/// order.
///
/// The parts behind those ids — the chart, the four SmartArt parts, whatever an
/// embedded workbook hangs off — are **not** in the model: they are a producer's
/// own XML, and modelling them would be modelling DrawingML charts. What the
/// model owes a consumer is the reference itself, because a `c:chart` element
/// whose `r:id` points at nothing is unreadable content, and one that was
/// silently rewritten into a picture is a lie.
///
/// A writer holding the source package copies the parts and re-points the
/// reference (ADR-0007, W7). A writer without one records the loss, which is
/// what happened before this type existed.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ForeignRefs {
    /// The ids, in the order the element carries them: one for `c:chart`
    /// (`r:id`), four for `dgm:relIds` (`r:dm`, `r:lo`, `r:qs`, `r:cs`).
    pub rels: Vec<Arc<str>>,
    /// Source location of the element that carried them.
    pub location: SourceLocation,
}

impl ForeignRefs {
    /// The ids as strings, which is what a writer looks up.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.rels.iter().map(std::convert::AsRef::as_ref).collect()
    }

    /// Whether anything was captured at all.
    ///
    /// A chart recognised only from its `graphicData/@uri` has no ids, and a
    /// writer must treat it as unwritable rather than invent one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rels.is_empty()
    }
}

/// The graphic payload of a drawing (`a:graphicData` content).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Graphic {
    /// No recognised graphic content.
    None,
    /// A picture (`pic:pic`).
    Picture(Picture),
    /// A shape (`wps:wsp`).
    Shape(Shape),
    /// A group (`wpg:wgp`).
    Group(GroupShape),
    /// A chart reference (`c:chart`) and the part ids it points at.
    Chart(ForeignRefs),
    /// A SmartArt diagram reference (`dgm:relIds`) and the part ids it points at.
    Diagram(ForeignRefs),
    /// Another unrecognised graphic.
    Other,
}

/// An inline drawing (`wp:inline`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineDrawing {
    /// Drawing extent (`wp:extent`).
    pub extent: Option<Extent>,
    /// Non-visual properties (`wp:docPr`).
    pub doc_pr: Option<DocPr>,
    /// `a:graphicData/@uri`.
    pub graphic_uri: Option<Arc<str>>,
    /// Graphic payload.
    pub graphic: Box<Graphic>,
    /// Source location.
    pub location: SourceLocation,
}

impl InlineDrawing {
    /// Returns the embedded picture, when the graphic is one.
    #[must_use]
    pub fn picture(&self) -> Option<&Picture> {
        match self.graphic.as_ref() {
            Graphic::Picture(picture) => Some(picture),
            _ => None,
        }
    }
}

/// Anchor positioning (`wp:positionH`/`wp:positionV`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Position {
    /// Base (`relativeFrom`).
    pub relative_from: Option<Arc<str>>,
    /// Alignment (`wp:align`).
    pub align: Option<Arc<str>>,
    /// Offset in EMU (`wp:posOffset`).
    pub offset: Option<Emu>,
}

/// Text-wrapping kind (`wp:wrap*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapKind {
    /// `wp:wrapNone`.
    None,
    /// `wp:wrapSquare`.
    Square,
    /// `wp:wrapTight`.
    Tight,
    /// `wp:wrapThrough`.
    Through,
    /// `wp:wrapTopAndBottom`.
    TopAndBottom,
}

/// Text wrapping (`wp:wrap*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrap {
    /// Wrapping kind.
    pub kind: WrapKind,
    /// Side selection (`wrapText`), normally `bothSides`.
    pub wrap_text: Option<Arc<str>>,
    /// Left distance (`distL`), in EMU.
    pub dist_left: Option<u32>,
    /// Right distance (`distR`), in EMU.
    pub dist_right: Option<u32>,
    /// Top distance (`distT`), in EMU.
    pub dist_top: Option<u32>,
    /// Bottom distance (`distB`), in EMU.
    pub dist_bottom: Option<u32>,
    /// The wrap contour (`wp:wrapPolygon`): one `wp:start` followed by two or more
    /// `wp:lineTo`, as `(x, y)` in EMU.
    ///
    /// `CT_WrapPath` is a sequence of exactly one `start` and at least two
    /// `lineTo`, so an EMPTY polygon is non-conformant and there is no "no contour"
    /// spelling to fall back to. When this is empty the writer draws the frame's own
    /// rectangle, which is a tight wrap around a box - honest, and not the contour
    /// the producer chose.
    ///
    /// The corpus has exactly one document that carries a contour at all, and its
    /// points are in VML shape space (0..21626, the classic `coordsize`), so they
    /// are scaled on the way in rather than copied. That scaling is the reason this
    /// field is points and not a path string: a path would have to be re-parsed to
    /// be written, and re-parsing is where the unit goes wrong.
    pub polygon: Vec<(i64, i64)>,
}

/// A floating drawing (`wp:anchor`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorDrawing {
    /// Drawing extent (`wp:extent`).
    pub extent: Option<Extent>,
    /// Effect extent (`wp:effectExtent`).
    pub effect_extent: Option<EffectExtent>,
    /// Non-visual properties (`wp:docPr`).
    pub doc_pr: Option<DocPr>,
    /// Simple positioning (`wp:simplePos`).
    pub simple_pos: bool,
    /// Horizontal position.
    pub position_h: Option<Position>,
    /// Vertical position.
    pub position_v: Option<Position>,
    /// Text wrapping.
    pub wrap: Option<Wrap>,
    /// Draw behind the document text (`behindDoc`).
    pub behind_doc: bool,
    /// Relative z-order (`relativeHeight`).
    pub relative_height: Option<u32>,
    /// Distance above the object (`distT`), in EMU.
    pub dist_top: Option<u32>,
    /// Distance below the object (`distB`), in EMU.
    pub dist_bottom: Option<u32>,
    /// Distance left of the object (`distL`), in EMU.
    pub dist_left: Option<u32>,
    /// Distance right of the object (`distR`), in EMU.
    pub dist_right: Option<u32>,
    /// Allow overlap with other floating objects (`allowOverlap`).
    pub allow_overlap: bool,
    /// Position in a table cell (`layoutInCell`).
    pub layout_in_cell: bool,
    /// Locked anchor (`locked`).
    pub locked: bool,
    /// `a:graphicData/@uri`.
    pub graphic_uri: Option<Arc<str>>,
    /// Graphic payload.
    pub graphic: Box<Graphic>,
    /// Source location.
    pub location: SourceLocation,
}

impl AnchorDrawing {
    /// Returns the embedded picture, when the graphic is one.
    #[must_use]
    pub fn picture(&self) -> Option<&Picture> {
        match self.graphic.as_ref() {
            Graphic::Picture(picture) => Some(picture),
            _ => None,
        }
    }
}

/// A DrawingML drawing (`w:drawing`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawing {
    /// Drawing payload.
    pub kind: DrawingKind,
    /// Source location.
    pub location: SourceLocation,
}

/// Discriminates the payload of a [`Drawing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawingKind {
    /// Inline drawing (`wp:inline`), fully parsed.
    Inline(InlineDrawing),
    /// Floating drawing (`wp:anchor`), fully parsed (Stage 5B).
    Anchor(AnchorDrawing),
    /// Unrecognised drawing content.
    Opaque(OpaqueInline),
}

/// Media kind inferred from a part's content type or extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// PNG image.
    Png,
    /// JPEG image.
    Jpeg,
    /// GIF image.
    Gif,
    /// BMP image.
    Bmp,
    /// TIFF image.
    Tiff,
    /// EMF metafile.
    Emf,
    /// WMF metafile.
    Wmf,
    /// SVG image.
    Svg,
    /// Any other media.
    Other,
}

impl MediaKind {
    /// Infers the media kind from a content type or file extension string.
    #[must_use]
    pub fn from_content_type(content_type: &str) -> Self {
        let normalized = content_type.to_ascii_lowercase();
        if normalized.contains("png") {
            return Self::Png;
        }
        if normalized.contains("jpeg") || normalized.contains("jpg") {
            return Self::Jpeg;
        }
        if normalized.contains("gif") {
            return Self::Gif;
        }
        if normalized.contains("bmp") {
            return Self::Bmp;
        }
        if normalized.contains("tiff") || normalized.contains("tif") {
            return Self::Tiff;
        }
        if normalized.contains("emf") {
            return Self::Emf;
        }
        if normalized.contains("wmf") {
            return Self::Wmf;
        }
        if normalized.contains("svg") {
            return Self::Svg;
        }
        Self::Other
    }

    /// Infers the media kind from a part path's extension.
    #[must_use]
    pub fn from_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "bmp" => Self::Bmp,
            "tif" | "tiff" => Self::Tiff,
            "emf" => Self::Emf,
            "wmf" => Self::Wmf,
            "svg" => Self::Svg,
            _ => Self::Other,
        }
    }
}

/// Metadata for one media part (image bytes are not decoded in Stage 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaItem {
    /// Media part id.
    pub part: PartId,
    /// Resolved content type.
    pub content_type: Option<Arc<str>>,
    /// Inferred media kind.
    pub kind: MediaKind,
}

/// Index of media parts referenced by the document.
#[derive(Clone, Debug, Default)]
pub struct MediaIndex {
    items: Vec<MediaItem>,
    by_part: HashMap<PartId, usize>,
}

impl MediaIndex {
    /// Creates an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a media item, returning its index (deduplicating by part).
    pub fn insert(&mut self, item: MediaItem) -> usize {
        if let Some(&index) = self.by_part.get(&item.part) {
            return index;
        }
        let index = self.items.len();
        self.by_part.insert(item.part.clone(), index);
        self.items.push(item);
        index
    }

    /// Returns the item for a part, if indexed.
    #[must_use]
    pub fn get(&self, part: &PartId) -> Option<&MediaItem> {
        self.by_part
            .get(part)
            .and_then(|&index| self.items.get(index))
    }

    /// Iterates over indexed media items.
    pub fn iter(&self) -> impl Iterator<Item = &MediaItem> {
        self.items.iter()
    }

    /// Returns the number of media items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if no media was indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
