//! Page layout: turns the DOM into placed pages (`STAGE-4-TASK.md` §4, §5.4–§5.6).
//!
//! The pipeline is: section geometry → block flow (paragraph/table) →
//! pagination → a list of [`PlacedPage`]s holding paint [`Item`]s. Painting
//! (`crate::paint`) is the only place that knows SVG.

pub(crate) mod floating;
pub(crate) mod headerfooter;
pub(crate) mod pageborders;
pub(crate) mod paginate;
pub(crate) mod paragraph;
pub(crate) mod table;

use strict_ooxml_wml::model::props::{PageMargins, PageSize, SectionProperties};
use strict_ooxml_wml::model::values::{PageOrientation, Twips};
use strict_ooxml_wml::model::Document;

use crate::font::FontProvider;
use crate::style::ComputedRun;
use crate::units::twips_to_px;
use crate::{MediaMode, MediaSource, RenderOptions};

/// A painted text fragment.
#[derive(Clone, Debug)]
pub(crate) struct TextItem {
    /// Left edge in px.
    pub x: f64,
    /// Baseline y in px.
    pub baseline: f64,
    /// Glyph advance width in px (for highlighting).
    pub width: f64,
    /// Text content.
    pub text: String,
    /// Effective run format.
    pub run: ComputedRun,
    /// Font size in px.
    pub size_px: f64,
    /// Computed field marker, when this item is a PAGE/NUMPAGES/SECTIONPAGES
    /// placeholder resolved at placement time.
    pub field: Option<crate::fields::FieldMarker>,
}

/// A filled/stroked rectangle.
#[derive(Clone, Debug)]
pub(crate) struct RectItem {
    /// X.
    pub x: f64,
    /// Y.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
    /// Fill colour.
    pub fill: Option<String>,
    /// Stroke colour.
    pub stroke: Option<String>,
    /// Stroke width in px.
    pub stroke_w: f64,
}

/// A straight line.
#[derive(Clone, Debug)]
pub(crate) struct LineItem {
    /// Start x.
    pub x1: f64,
    /// Start y.
    pub y1: f64,
    /// End x.
    pub x2: f64,
    /// End y.
    pub y2: f64,
    /// Colour.
    pub color: String,
    /// Width in px.
    pub width: f64,
    /// Dashed stroke.
    pub dashed: bool,
}

/// An arbitrary SVG path (a DrawingML shape), in local coordinates.
#[derive(Clone, Debug)]
pub(crate) struct PathItem {
    /// Placement x.
    pub x: f64,
    /// Placement y.
    pub y: f64,
    /// Local width.
    pub w: f64,
    /// Local height.
    pub h: f64,
    /// SVG path data in local coordinates (`0..w`, `0..h`).
    pub d: String,
    /// Fill colour.
    pub fill: Option<String>,
    /// Stroke colour.
    pub stroke: Option<String>,
    /// Stroke width in px.
    pub stroke_w: f64,
    /// SVG dash array.
    pub dash: Option<String>,
    /// Rotation in degrees.
    pub rotate_deg: f64,
    /// Horizontal flip.
    pub flip_h: bool,
    /// Vertical flip.
    pub flip_v: bool,
}

/// An embedded/placed image.
#[derive(Clone, Debug)]
pub(crate) struct ImageItem {
    /// X.
    pub x: f64,
    /// Y.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
    /// Href (data URI or relative filename), or `None` for a placeholder.
    pub href: Option<String>,
    /// Alternative text.
    pub alt: String,
    /// Optional SVG transform (rotation/flip) applied about the image centre.
    pub transform: Option<String>,
}

/// One paint primitive.
#[derive(Clone, Debug)]
pub(crate) enum Item {
    /// Text.
    Text(TextItem),
    /// Rectangle.
    Rect(RectItem),
    /// Line.
    Line(LineItem),
    /// Image.
    Image(ImageItem),
    /// Arbitrary shape path.
    Path(PathItem),
}

/// A fully placed page ready to be painted.
#[derive(Clone, Debug)]
pub(crate) struct PlacedPage {
    /// Page width in px.
    pub width_px: f64,
    /// Page height in px.
    pub height_px: f64,
    /// Paint items in deterministic order.
    pub items: Vec<Item>,
}

/// A laid-out text line.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextLine {
    /// Paint items.
    pub items: Vec<TextItem>,
    /// Non-text paint items produced by the line's content (math rules, vector
    /// delimiters, rules of an accent or a bar).
    pub graphics: Vec<Item>,
    /// Line height in px.
    pub height: f64,
    /// Baseline offset from the line top in px.
    pub ascent: f64,
    /// Footnote ids referenced on this line, in order.
    pub footnote_refs: Vec<u32>,
}

impl TextLine {
    /// Offsets every item by `(dx, dy)`.
    fn offset(&mut self, dx: f64, dy: f64) {
        for item in &mut self.items {
            item.x += dx;
            item.baseline += dy;
        }
        for item in &mut self.graphics {
            *item = match item.clone() {
                Item::Text(mut text) => {
                    text.x += dx;
                    text.baseline += dy;
                    Item::Text(text)
                }
                Item::Rect(mut rect) => {
                    rect.x += dx;
                    rect.y += dy;
                    Item::Rect(rect)
                }
                Item::Line(mut line) => {
                    line.x1 += dx;
                    line.y1 += dy;
                    line.x2 += dx;
                    line.y2 += dy;
                    Item::Line(line)
                }
                Item::Path(mut path) => {
                    path.x += dx;
                    path.y += dy;
                    Item::Path(path)
                }
                Item::Image(mut image) => {
                    image.x += dx;
                    image.y += dy;
                    Item::Image(image)
                }
            };
        }
    }
}

/// A unit of flow produced by paragraph/table layout.
#[derive(Clone, Debug)]
pub(crate) enum Flow {
    /// A text line.
    Line(TextLine),
    /// An atomic image.
    Image(ImageItem),
    /// An atomic pre-placed block (for example a table row).
    Block {
        /// Items already positioned relative to the block top-left.
        items: Vec<Item>,
        /// Block height in px.
        height: f64,
    },
    /// A table row, carrying repeat/keep metadata.
    TableRow(TableRowFlow),
    /// An explicit page break.
    PageBreak,
}

/// A laid-out table row.
#[derive(Clone, Debug)]
pub(crate) struct TableRowFlow {
    /// Items positioned relative to the row's top-left.
    pub items: Vec<Item>,
    /// Row height in px.
    pub height: f64,
    /// Whether this row repeats as a header on each page (`w:tblHeader`).
    pub header: bool,
}

/// Page geometry derived from a section's `sectPr`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    /// Page width in px.
    pub width: f64,
    /// Page height in px.
    pub height: f64,
    /// Left content margin in px.
    pub left: f64,
    /// Top content margin in px.
    pub top: f64,
    /// Right content margin in px.
    pub right: f64,
    /// Bottom content margin in px.
    pub bottom: f64,
    /// Document-grid line pitch in px (`w:docGrid/@w:linePitch`), if any.
    pub grid_line_pitch: Option<f64>,
}

impl Geometry {
    /// Content width in px.
    #[must_use]
    pub(crate) fn content_width(&self) -> f64 {
        (self.width - self.left - self.right).max(1.0)
    }

    /// Content height in px.
    #[must_use]
    pub(crate) fn content_height(&self) -> f64 {
        (self.height - self.top - self.bottom).max(1.0)
    }
}

/// Default page size (US Letter), in twips.
const DEFAULT_PAGE_WIDTH: i32 = 12_240;
/// Default page height (US Letter), in twips.
const DEFAULT_PAGE_HEIGHT: i32 = 15_840;
/// Default margin, in twips (1 inch).
const DEFAULT_MARGIN: i32 = 1_440;

/// Builds [`Geometry`] from optional section properties.
#[must_use]
pub(crate) fn geometry_for(section: Option<&SectionProperties>, scale: f64) -> Geometry {
    let page = section
        .and_then(|section| section.page_size)
        .unwrap_or(PageSize {
            width: Some(Twips(DEFAULT_PAGE_WIDTH)),
            height: Some(Twips(DEFAULT_PAGE_HEIGHT)),
            orientation: None,
        });
    let mut width = twips_to_px(page.width.map_or(DEFAULT_PAGE_WIDTH, Twips::value), scale);
    let mut height = twips_to_px(page.height.map_or(DEFAULT_PAGE_HEIGHT, Twips::value), scale);
    if matches!(page.orientation, Some(PageOrientation::Landscape)) && width < height {
        std::mem::swap(&mut width, &mut height);
    }

    let margins = section
        .and_then(|section| section.page_margins)
        .unwrap_or(PageMargins {
            top: Some(Twips(DEFAULT_MARGIN)),
            right: Some(Twips(DEFAULT_MARGIN)),
            bottom: Some(Twips(DEFAULT_MARGIN)),
            left: Some(Twips(DEFAULT_MARGIN)),
            header: None,
            footer: None,
            gutter: None,
        });
    let value = |margin: Option<Twips>, default: i32| {
        twips_to_px(margin.map_or(default, Twips::value), scale)
    };
    let grid_line_pitch = section
        .and_then(|section| section.doc_grid.as_ref())
        .filter(|grid| {
            // ISO/IEC 29500-1 §17.6.6: only the snapping grid types make lines
            // occupy whole grid units. `w:type="default"` (also the value used
            // when the attribute is absent) is *no* document grid — `linePitch`
            // then only fixes the number of lines per page, and the line height
            // stays the natural one, as Word/WPS lay it out.
            matches!(
                grid.grid_type,
                Some(
                    strict_ooxml_wml::model::values::DocGridType::Lines
                        | strict_ooxml_wml::model::values::DocGridType::LinesAndChars
                        | strict_ooxml_wml::model::values::DocGridType::SnapToChars
                )
            )
        })
        .and_then(|grid| grid.line_pitch)
        .filter(|pitch| *pitch > 0)
        .map(|pitch| twips_to_px(pitch, scale));
    Geometry {
        width,
        height,
        left: value(margins.left, DEFAULT_MARGIN),
        top: value(margins.top, DEFAULT_MARGIN),
        right: value(margins.right, DEFAULT_MARGIN),
        bottom: value(margins.bottom, DEFAULT_MARGIN),
        grid_line_pitch,
    }
}

/// Shared state for layout and media resolution.
pub(crate) struct LayoutContext<'a> {
    /// The document being laid out.
    pub document: &'a Document,
    /// Render options.
    pub options: &'a RenderOptions,
    /// Font provider.
    pub font: &'a dyn FontProvider,
    /// Optional media source.
    pub media: Option<&'a dyn MediaSource>,
    /// Media emission mode.
    pub media_mode: MediaMode,
    /// Footnote/endnote numbering.
    pub note_numbers: crate::notes::NoteNumbering,
    /// List numbering markers by paragraph location.
    pub numbering: crate::numbering::NumberingMarkers,
}

impl LayoutContext<'_> {
    /// Pixel-per-point scale shorthand.
    #[must_use]
    pub(crate) fn size_px(&self, size_pt: f64) -> f64 {
        crate::units::pt_to_px(size_pt, self.options.scale)
    }

    /// Measures the advance width of `text` for a run in px.
    #[must_use]
    pub(crate) fn measure(&self, text: &str, run: &ComputedRun) -> f64 {
        let size_px = self.size_px(run.size_pt);
        text.chars()
            .map(|ch| self.font.advance_em(&run.family, ch, run.bold, run.italic) * size_px)
            .sum()
    }
}

/// Result of laying out the whole document.
pub(crate) struct Layout {
    /// Pages in order.
    pub pages: Vec<PlacedPage>,
    /// Floating (anchored) objects with the page they belong to.
    pub anchors: Vec<floating::PendingAnchor>,
}
