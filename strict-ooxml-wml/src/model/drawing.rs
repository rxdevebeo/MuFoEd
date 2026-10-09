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
    /// Title (`title`), when the producer set one.
    pub title: Option<Arc<str>>,
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
    /// `a:blip/@cstate` (`print`, `screen`, `none`, …).
    pub cstate: Option<Arc<str>>,
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
    /// Non-visual id (`pic:cNvPr/@id`). Absent only for a picture built in code.
    pub nv_id: Option<u32>,
    /// `pic:spPr/@bwMode`, including the explicit value `auto`.
    pub bw_mode: Option<Arc<str>>,
    /// The image reference.
    pub blip: Option<BlipRef>,
    /// Picture geometry extent.
    pub extent: Option<Extent>,
    /// Source-rectangle crop (`a:srcRect`).
    pub src_rect: Option<SrcRect>,
    /// Applied transform (`a:xfrm`) with rotation/flips.
    pub xfrm: Option<Xfrm>,
    /// Attributes the picture carries without a field of their own.
    pub markup: PictureMarkup,
}

/// The attributes of a `pic:pic` that nothing reads but a writer keeps:
/// `pic:cNvPicPr@preferRelativeResize`, `a:picLocks`, `pic:blipFill@rotWithShape`
/// and `@dpi`. Each is the attribute's local name and its value as written.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PictureMarkup {
    /// `pic:cNvPicPr` attributes.
    pub non_visual: Vec<(Arc<str>, Arc<str>)>,
    /// `a:picLocks` attributes; `None` when the element is absent.
    pub locks: Option<Vec<(Arc<str>, Arc<str>)>>,
    /// `pic:blipFill` attributes.
    pub blip_fill: Vec<(Arc<str>, Arc<str>)>,
    /// The children of `a:blip` (colour effects, `a:extLst`) as Strict markup
    /// that declares its own namespaces.
    pub blip_children: Option<Arc<str>>,
    /// The children of `pic:spPr` after `a:xfrm` (geometry, fill, line,
    /// effects, `a:extLst`), as markup in source order.
    pub shape_properties: Option<Arc<str>>,
}

/// A colour that is either an explicit RGB value or a theme reference.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShapeColor {
    /// Explicit colour value.
    pub value: Option<Color>,
    /// Theme colour reference.
    pub theme: Option<ThemeColorRef>,
    /// The colour element as read (`a:srgbClr` with its `a:alpha`, a
    /// `a:schemeClr` with `a:lumMod`/`a:lumOff`, `a:sysClr`, `a:prstClr`), which
    /// the writer puts back instead of [`Self::value`]/[`Self::theme`]. A colour
    /// built in code leaves it `None`.
    pub markup: Option<Arc<str>>,
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
        /// `a:lin/@scaled`, when written.
        scaled: Option<bool>,
        /// `a:gradFill/@rotWithShape`, when written.
        rotate_with_shape: Option<bool>,
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
    /// Image fill (`a:blipFill`).
    ///
    /// The bytes stay the source part. `fill_rect` is `a:fillRect` in the
    /// source spelling (`l`, `t`, `r`, `b`), so a transitional thousandths
    /// value is not rewritten into a percent string.
    Blip {
        /// The image.
        blip: BlipRef,
        /// `a:fillRect` attributes, source lexical form.
        fill_rect: [Option<Arc<str>>; 4],
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

/// One path inside `a:custGeom/a:pathLst` (AUD-49).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GeometryPath {
    /// Path width (`a:path/@w`) in path units.
    pub width: i64,
    /// Path height (`a:path/@h`) in path units.
    pub height: i64,
    /// Fill mode (`a:path/@fill`), when present.
    pub fill: Option<Arc<str>>,
    /// Whether the path is stroked (`a:path/@stroke`).
    pub stroke: Option<bool>,
    /// Path commands.
    pub commands: Vec<PathCommand>,
}

/// A custom geometry (`a:custGeom`), possibly with several paths (AUD-49).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CustomGeometry {
    /// Paths in document order; each keeps its own `w`/`h`.
    pub paths: Vec<GeometryPath>,
    /// The text rectangle (`a:rect` `l`, `t`, `r`, `b`) as written: a guide
    /// name or a coordinate. `None` writes `0 0 r b`.
    pub text_rect: Option<[Arc<str>; 4]>,
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

/// A `wps:style` reference block, carrying theme style indices and scheme colours.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShapeStyle {
    /// Line reference index (`a:lnRef/@idx`), an integer as text.
    pub line_ref: Option<std::sync::Arc<str>>,
    /// Scheme colour under `a:lnRef` (`a:schemeClr/@val`).
    pub line_ref_color: Option<std::sync::Arc<str>>,
    /// Fill reference index (`a:fillRef/@idx`), an integer as text.
    pub fill_ref: Option<std::sync::Arc<str>>,
    /// Scheme colour under `a:fillRef` (`a:schemeClr/@val`).
    pub fill_ref_color: Option<std::sync::Arc<str>>,
    /// Effect reference index (`a:effectRef/@idx`), an integer as text.
    pub effect_ref: Option<std::sync::Arc<str>>,
    /// Scheme colour under `a:effectRef` (`a:schemeClr/@val`).
    pub effect_ref_color: Option<std::sync::Arc<str>>,
    /// Font collection index (`a:fontRef/@idx`: `major` / `minor` / `none`).
    pub font_ref: Option<std::sync::Arc<str>>,
    /// Scheme colour under `a:fontRef` (`a:schemeClr/@val`).
    pub font_ref_color: Option<std::sync::Arc<str>>,
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

/// Text-box body properties (`wps:bodyPr`).
///
/// Optional fields keep an explicit producer value, including lexical defaults
/// such as `wrap="square"` / `anchorCtr="0"`: dropping them is a silent change
/// the census inventory reports (P4).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TextBoxBody {
    /// Vertical anchor (`anchor`).
    pub anchor: Option<TextAnchor>,
    /// Center the text block horizontally (`anchorCtr`).
    pub anchor_centered: Option<bool>,
    /// Left inset (`lIns`) in EMU.
    pub left_inset: Option<Emu>,
    /// Top inset (`tIns`) in EMU.
    pub top_inset: Option<Emu>,
    /// Right inset (`rIns`) in EMU.
    pub right_inset: Option<Emu>,
    /// Bottom inset (`bIns`) in EMU.
    pub bottom_inset: Option<Emu>,
    /// Text wrapping mode inside the box (`wrap`).
    pub wrap: Option<Arc<str>>,
    /// Text vertical direction (`vert`).
    pub vert: Option<Arc<str>>,
    /// Text rotation in 60000ths of a degree (`rot`).
    pub rot: Option<i32>,
    /// Keep text upright under shape rotation (`upright`).
    pub upright: Option<bool>,
    /// Right-to-left columns (`rtlCol`).
    pub rtl_col: Option<bool>,
    /// Compatible line spacing (`compatLnSpc`).
    pub compat_ln_spc: Option<bool>,
    /// Force anti-aliasing (`forceAA`).
    pub force_aa: Option<bool>,
    /// WordArt flag (`fromWordArt`).
    pub from_word_art: Option<bool>,
    /// Horizontal overflow (`horzOverflow`).
    pub horz_overflow: Option<Arc<str>>,
    /// Vertical overflow (`vertOverflow`).
    pub vert_overflow: Option<Arc<str>>,
    /// Number of columns (`numCol`).
    pub num_col: Option<i32>,
    /// Spacing between columns in EMU (`spcCol`).
    pub spc_col: Option<i32>,
    /// Space before/after first/last paragraph (`spcFirstLastPara`).
    pub spc_first_last_para: Option<bool>,
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
    /// Non-visual id (`wps:cNvPr/@id`).
    pub nv_id: Option<u32>,
    /// `wps:spPr/@bwMode`, including the explicit value `auto`.
    pub bw_mode: Option<Arc<str>>,
    /// Whether this shape is a text box (`wps:cNvSpPr/@txBox`).
    pub tx_box: Option<bool>,
    /// `a:spLocks` attributes as written; `None` when the element is absent.
    pub sp_locks: Option<Vec<(Arc<str>, Arc<str>)>>,
    /// The `wps:spPr` children after the line (`a:effectLst`, `a:scene3d`,
    /// `a:sp3d`, `a:extLst`) as markup that declares its own namespaces.
    pub effects: Option<Arc<str>>,
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
/// embedded workbook hangs off — are **not** in the model as markup: they are a
/// producer's own XML. A chart's cached values are read into
/// [`chart`](Self::chart) so it can be drawn, but what the model owes a writer is
/// the reference itself, because a `c:chart` element
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
    /// Cached data of the chart part a `c:chart` points at, when it could be
    /// read (see [`crate::model::chart`]). Always `None` for a diagram, for an
    /// id that resolves to nothing and for a chart part that failed to parse.
    ///
    /// Shared behind an [`Arc`] so the drawing stays small and a chart part
    /// referenced twice is parsed once.
    pub chart: Option<Arc<crate::model::chart::ChartData>>,
    /// The drawing Word cached beside a SmartArt diagram, when it could be read
    /// (see [`DiagramDrawing`]). Always `None` for a chart, for a diagram whose
    /// data part names no drawing part and for a drawing part that failed to
    /// parse.
    ///
    /// Shared behind an [`Arc`] for the same reasons as [`chart`](Self::chart).
    pub diagram: Option<Arc<DiagramDrawing>>,
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

/// Most shapes (`dsp:sp` and `dsp:grpSp`, counted together) kept from one
/// diagram drawing part; the rest are dropped and reported.
pub const MAX_DIAGRAM_SHAPES: usize = 2000;

/// The pre-rendered drawing of a `SmartArt` diagram (`dsp:drawing`).
///
/// A `dgm:relIds` names the diagram's data, layout, style and colour parts;
/// laying the diagram out from those is a layout engine of its own. Word also
/// stores the result of its own layout in a separate drawing part (named from
/// the data part's `dsp:dataModelExt/@relId`), and that drawing is what is kept
/// here: plain DrawingML shapes in the diagram's own coordinate space, where
/// `(0, 0)` is the top-left corner of the graphic frame and one unit is one EMU.
///
/// Shapes reuse the text-box model: a shape's `dsp:txBody` paragraphs become
/// [`Block::Paragraph`]s of its [`TextBox`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramDrawing {
    /// The drawing part the shapes were read from.
    pub part: PartId,
    /// The shape tree's own transform (`dsp:spTree/dsp:grpSpPr/a:xfrm`), when
    /// the producer wrote one. Word writes an empty `dsp:grpSpPr`.
    pub xfrm: Option<GroupTransform>,
    /// The shapes, in document (paint) order: [`Graphic::Shape`] for a
    /// `dsp:sp`, [`Graphic::Group`] for a `dsp:grpSp`.
    pub shapes: Vec<Graphic>,
    /// Whether shapes past [`MAX_DIAGRAM_SHAPES`] were dropped.
    pub truncated: bool,
}

/// A DrawingML locked canvas (`lc:lockedCanvas`) preserved as Strict markup.
///
/// Nested `a:grpSp` / `a:sp` / `a:cxnSp` trees carry the document's `a:off` /
/// `a:ext` / `a:chOff` / `a:chExt` placement. Modelling every connector would be
/// a separate DrawingML product; the markup is rewritten to Strict namespaces
/// at parse time so the writer can emit it into `document.xml` without a
/// Transitional URI (ADR-0007; audit P2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedCanvas {
    /// Strict-form XML of the `lc:lockedCanvas` element, including children.
    pub markup: Arc<str>,
    /// Image relationships the markup still names, as `(source rId, part)`.
    ///
    /// The markup keeps the source ids. The writer allocates new ones, so these
    /// pairs are what retargets `r:embed` at the copied bytes.
    pub images: Vec<(String, PartId)>,
    /// Source location of the canvas root.
    pub location: SourceLocation,
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
    /// A locked canvas (`lc:lockedCanvas`) with Strict markup.
    LockedCanvas(LockedCanvas),
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
    /// Effect extent (`wp:effectExtent`), AUD-50.
    pub effect_extent: Option<EffectExtent>,
    /// Non-visual properties (`wp:docPr`).
    pub doc_pr: Option<DocPr>,
    /// Distance above the object (`distT`), in EMU.
    pub dist_top: Option<u32>,
    /// Distance below the object (`distB`), in EMU.
    pub dist_bottom: Option<u32>,
    /// Distance left of the object (`distL`), in EMU.
    pub dist_left: Option<u32>,
    /// Distance right of the object (`distR`), in EMU.
    pub dist_right: Option<u32>,
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
    /// Percentage offset (`wp14:pctPosHOffset` / `wp14:pctPosVOffset`).
    ///
    /// Word unit where `100000` is 100%. When present this is the Choice branch
    /// of an `mc:AlternateContent` that also carries a `wp:posOffset` Fallback;
    /// either spelling positions the frame, and both are preserved when parsed.
    pub percent_offset: Option<i32>,
}

/// A relative size (`wp14:sizeRelH` / `wp14:sizeRelV`).
///
/// `percent` is in the Word unit where `100000` is 100%. A positive value
/// replaces the fallback `wp:extent` on that axis. Zero is stored so the
/// renderer can report it instead of inventing a size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativeSize {
    /// Reference rectangle (`relativeFrom`).
    pub relative_from: Option<Arc<str>>,
    /// Percentage, with `100000` meaning 100%.
    pub percent: u32,
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
    /// `wp:wrapPolygon/@edited` when the producer set it (including explicit `0`).
    pub polygon_edited: Option<bool>,
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
    /// The `wp:simplePos` point in EMU, `None` for `0, 0`. Word keeps a point
    /// there even when `@simplePos` is off.
    pub simple_pos_point: Option<(i64, i64)>,
    /// `wp:anchor/@hidden`, when the producer wrote it.
    pub hidden: Option<bool>,
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
    /// Relative width (`wp14:sizeRelH`). A positive percent beats `extent`.
    pub size_rel_h: Option<RelativeSize>,
    /// Relative height (`wp14:sizeRelV`). A positive percent beats `extent`.
    pub size_rel_v: Option<RelativeSize>,
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
