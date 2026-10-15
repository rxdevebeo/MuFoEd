//! Page layout: turns the DOM into placed pages (`STAGE-4-TASK.md` §4, §5.4–§5.6).
//!
//! The pipeline is: section geometry → block flow (paragraph/table) →
//! pagination → a list of [`PlacedPage`]s holding paint [`Item`]s. Painting
//! (`crate::paint`) is the only place that knows SVG.

pub mod border;
pub mod decoration;
pub(crate) mod exclusions;
pub(crate) mod floating;
pub(crate) mod headerfooter;
pub(crate) mod pageborders;
pub(crate) mod paginate;
pub(crate) mod paragraph;
pub(crate) mod sectionpage;
pub(crate) mod table;

use std::collections::HashMap;

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::props::{PageMargins, SectionProperties};
use strict_ooxml_wml::model::values::Twips;
use strict_ooxml_wml::model::Document;

use crate::font::FontProvider;
use crate::style::ComputedRun;
use crate::units::twips_to_px;
use crate::{MediaMode, MediaSource, RenderOptions};
pub use decoration::{text_decorations, TextDecorations};
pub(crate) use paragraph::formula_ink;

/// Which advance geometry produced [`TextItem::width`] and must be used to paint.
///
/// Layout, SVG, PDF and caret must agree on one policy per fragment. Guessing
/// from a width comparison re-opened a both/justify divergence for Carlito
/// pair-kerned runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAdvanceKind {
    /// OpenType shaping (kerning, ligatures, complex scripts).
    #[default]
    Shaped,
    /// Sum of `hmtx` advances without GPOS adjustments.
    Metric,
}

/// A painted text fragment.
#[derive(Clone, Debug)]
pub struct TextItem {
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
    /// Advance geometry that produced [`Self::width`].
    pub advance: TextAdvanceKind,
    /// Computed field marker, when this item is a PAGE/NUMPAGES/SECTIONPAGES/
    /// SECTION placeholder resolved at placement time.
    ///
    /// A backend that draws the text needs to know which fragment is a page
    /// number, so the marker is public — but the marker type stays crate-internal
    /// and is matched by behaviour (`is_page_number` and friends), which is what
    /// keeps a second backend from depending on the field machinery's internals.
    pub field: Option<crate::fields::FieldMarker>,
    /// Collapse spaces after Western punctuation (`w:characterSpacingControl`).
    pub compress_punctuation: bool,
    /// First non-space of the next token (for trailing post-punctuation spaces).
    pub following_non_space: Option<char>,
    /// Advance factor for spaces that are not post-punctuation collapses.
    pub plain_space_factor: f64,
}

/// A filled/stroked rectangle.
#[derive(Clone, Debug)]
pub struct RectItem {
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
pub struct LineItem {
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

/// A resolved non-solid fill shared by SVG and PDF.
#[derive(Clone, Debug)]
pub enum PathPaint {
    /// A linear gradient, with stop positions between zero and one.
    LinearGradient {
        /// Resolved RGB stop colours.
        stops: Vec<(f64, String)>,
        /// Endpoints in the shape's local coordinates.
        start: (f64, f64),
        /// End of the gradient axis.
        end: (f64, f64),
    },
    /// A repeating hatch or dot tile.
    Pattern {
        /// Resolved foreground RGB colour.
        foreground: String,
        /// Resolved background RGB colour.
        background: String,
        /// Tile edge in local pixels.
        size: f64,
        /// Foreground line segments, in normalized tile coordinates.
        lines: Vec<(f64, f64, f64, f64)>,
        /// Foreground rectangles, in normalized tile coordinates.
        rectangles: Vec<(f64, f64, f64, f64)>,
    },
}

/// An arbitrary SVG path (a DrawingML shape), in local coordinates.
#[derive(Clone, Debug)]
pub struct PathItem {
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
    /// A gradient or pattern, replacing the solid fill when present.
    pub paint: Option<PathPaint>,
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
pub struct ImageItem {
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
    /// The media part this image was resolved from, when there was one.
    ///
    /// The SVG backend inlines the bytes into `href`, but a backend that emits
    /// the image as its own object — the PDF one, Stage 8B — needs the part to
    /// read the *original* bytes rather than to decode a data URI it would have
    /// to trust. This is the honest answer to "what was placed here".
    pub part: Option<PartId>,
    /// Alternative text.
    pub alt: String,
    /// Optional SVG transform (rotation/flip) applied about the image centre.
    pub transform: Option<String>,
    /// Share of the picture cut away on the left, top, right and bottom
    /// (`a:srcRect`), as fractions of the picture. The rest is stretched over
    /// the item's box. A negative share pads instead of cutting.
    pub crop: Option<[f64; 4]>,
}

impl ImageItem {
    /// The box the *whole* picture is drawn into so that its cropped part
    /// fills `x, y, w, h`: `(x, y, w, h)` of the uncropped picture.
    #[must_use]
    pub fn uncropped_box(&self) -> (f64, f64, f64, f64) {
        let Some([left, top, right, bottom]) = self.crop else {
            return (self.x, self.y, self.w, self.h);
        };
        let visible_w = 1.0 - left - right;
        let visible_h = 1.0 - top - bottom;
        if visible_w <= 0.01 || visible_h <= 0.01 {
            return (self.x, self.y, self.w, self.h);
        }
        let full_w = self.w / visible_w;
        let full_h = self.h / visible_h;
        (
            self.x - left * full_w,
            self.y - top * full_h,
            full_w,
            full_h,
        )
    }
}

/// One paint primitive.
#[derive(Clone, Debug)]
pub struct GroupItem {
    /// SVG affine matrix `[a, b, c, d, e, f]` in page pixels.
    pub matrix: [f64; 6],
    /// Child primitives in the matrix's source coordinate system.
    pub items: Vec<Item>,
    /// Page bounding box `[x, y, width, height]` after transformation.
    pub bounds: [f64; 4],
    /// Optional rectangular clip in child coordinates.
    pub clip: Option<[f64; 4]>,
    /// A contour clip in child coordinates, optionally inverted within clip.
    pub clip_path: Option<PathItem>,
    /// Keep the complement of clip_path within the rectangular clip.
    pub clip_inverse: bool,
    /// Group opacity, composed with ancestor groups.
    pub opacity: f64,
}

impl GroupItem {
    /// Translates the whole group without changing its child coordinates.
    pub fn offset(&mut self, dx: f64, dy: f64) {
        self.matrix[4] += dx;
        self.matrix[5] += dy;
        self.bounds[0] += dx;
        self.bounds[1] += dy;
    }
}

/// One paint primitive.
///
/// The variants are backend-neutral: the SVG backend writes markup and the PDF
/// backend (Stage 8B) writes a content stream from the same values, so a second
/// renderer cannot disagree with the first about what was placed.
#[derive(Clone, Debug)]
pub enum Item {
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
    /// A transformed collection of primitives shared by SVG and PDF.
    Group(GroupItem),
}

impl Item {
    /// Iterates over leaf primitives, including transformed descendants.
    pub fn leaves(&self) -> impl Iterator<Item = &Item> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            while let Some(item) = stack.pop() {
                if let Item::Group(group) = item {
                    stack.extend(group.items.iter().rev());
                } else {
                    return Some(item);
                }
            }
            None
        })
    }
}

/// A fully placed page ready to be painted.
#[derive(Clone, Debug)]
pub struct PlacedPage {
    /// Page width in px.
    pub width_px: f64,
    /// Page height in px.
    pub height_px: f64,
    /// Paint items in deterministic order.
    pub items: Vec<Item>,
    /// Zero-based index into `Document::sections` for this page's geometry
    /// and headers/footers (AUD-74).
    pub section_index: usize,
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
    /// The paragraph keeps this line out of `w:lnNumType` numbering
    /// (`w:suppressLineNumbers`).
    pub suppress_line_number: bool,
}

impl TextLine {
    /// Offsets every item by `(dx, dy)`.
    pub(crate) fn offset(&mut self, dx: f64, dy: f64) {
        for item in &mut self.items {
            item.x += dx;
            item.baseline += dy;
        }
        for item in &mut self.graphics {
            *item = match item.clone() {
                Item::Group(mut group) => {
                    group.offset(dx, dy);
                    Item::Group(group)
                }
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
    /// An explicit column break.
    ColumnBreak,
}

/// A page anchored drawing hosted by text in a table row.
#[derive(Clone, Debug)]
pub(crate) struct RowAnchor {
    pub anchor: strict_ooxml_wml::model::drawing::AnchorDrawing,
    pub x: f64,
    pub y: f64,
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
    /// Page-absolute frame children. The row offset is not applied to these.
    pub page_frames: Vec<Item>,
    /// Page anchored drawings whose paragraph origin is relative to this row.
    pub anchors: Vec<RowAnchor>,
    /// Safe row-relative boundaries between content bands; empty for cantSplit.
    pub split_points: Vec<f64>,
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
    /// Body text columns (`w:cols`).
    pub columns: ColumnLayout,
    /// The column the content being laid out sits in (reading order). Only the
    /// paginator moves it; it picks the `column` box of a floating object.
    pub active_column: usize,
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

    /// Number of body columns.
    #[must_use]
    pub(crate) fn column_count(&self) -> usize {
        self.columns.count()
    }

    /// Width of body column `index` (reading order).
    #[must_use]
    pub(crate) fn column_width(&self, index: usize) -> f64 {
        self.columns.width(index)
    }

    /// Page x of the left edge of body column `index` (reading order).
    #[must_use]
    pub(crate) fn column_left(&self, index: usize) -> f64 {
        self.left + self.columns.offset(index)
    }

    /// Page x and width of the active column, the `column` reference box.
    #[must_use]
    pub(crate) fn active_column_box(&self) -> (f64, f64) {
        (
            self.column_left(self.active_column),
            self.column_width(self.active_column),
        )
    }
}

/// Most text columns a section can define (`CT_Columns/w:col maxOccurs="45"`).
pub(crate) const MAX_COLUMNS: usize = 45;

/// Default `w:cols/@w:space`: Word's half inch.
const DEFAULT_COLUMN_SPACE_TWIPS: i32 = 720;

/// Text columns of a section (ISO/IEC 29500-1 §17.6.3, §17.6.4): each column's
/// offset from the content left edge and its width, in reading order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ColumnLayout {
    count: usize,
    /// `(offset, width)` per column, px. A right-to-left section (`w:bidi`)
    /// already has its columns mirrored: column 0 is the rightmost.
    boxes: [(f64, f64); MAX_COLUMNS],
    /// Draw a line in the middle of each gap (`w:sep`).
    pub separator: bool,
}

impl ColumnLayout {
    /// One column filling the content width.
    #[must_use]
    pub(crate) fn single(content_width: f64) -> Self {
        let mut boxes = [(0.0, 0.0); MAX_COLUMNS];
        if let Some(first) = boxes.first_mut() {
            *first = (0.0, content_width.max(1.0));
        }
        Self {
            count: 1,
            boxes,
            separator: false,
        }
    }

    /// The columns `w:cols` defines within `content_width` px.
    ///
    /// With `equalWidth` the content width is divided by `num`, leaving `space`
    /// after each column but the last. Otherwise every `w:col` is one column of
    /// width `w`, followed by its `space` (0 when absent, as Word reads it); the
    /// last column's space is ignored, and a column without `w` shares what the
    /// others leave. More than [`MAX_COLUMNS`] columns are cut to that many.
    #[must_use]
    pub(crate) fn from_columns(
        columns: Option<&strict_ooxml_wml::model::props::Columns>,
        right_to_left: bool,
        content_width: f64,
        scale: f64,
    ) -> Self {
        let Some(columns) = columns else {
            return Self::single(content_width);
        };
        let content_width = content_width.max(1.0);
        let px = |twips: Option<Twips>, default: i32| {
            twips_to_px(twips.map_or(default, Twips::value).max(0), scale)
        };
        let mut boxes = [(0.0, 0.0); MAX_COLUMNS];
        let count = if columns.equal_width || columns.columns.is_empty() {
            let count = columns.count.map_or(1, usize::from).clamp(1, MAX_COLUMNS);
            let gap = px(columns.space, DEFAULT_COLUMN_SPACE_TWIPS);
            let width = ((content_width - gap * (count - 1) as f64) / count as f64).max(1.0);
            for (index, slot) in boxes.iter_mut().take(count).enumerate() {
                *slot = (index as f64 * (width + gap), width);
            }
            count
        } else {
            let specs: Vec<_> = columns.columns.iter().take(MAX_COLUMNS).collect();
            let count = specs.len();
            let spaces: Vec<f64> = specs
                .iter()
                .enumerate()
                .map(|(index, spec)| {
                    if index + 1 == count {
                        0.0
                    } else {
                        px(spec.space, 0)
                    }
                })
                .collect();
            let declared: f64 = specs
                .iter()
                .filter_map(|spec| spec.width)
                .map(|width| px(Some(width), 0))
                .sum();
            let missing = specs.iter().filter(|spec| spec.width.is_none()).count();
            let share = if missing == 0 {
                0.0
            } else {
                ((content_width - declared - spaces.iter().sum::<f64>()) / missing as f64).max(1.0)
            };
            let mut offset = 0.0;
            for ((slot, spec), space) in boxes.iter_mut().zip(&specs).zip(&spaces) {
                let width = spec
                    .width
                    .map_or(share, |width| px(Some(width), 0))
                    .max(1.0);
                *slot = (offset, width);
                offset += width + space;
            }
            count
        };
        if right_to_left {
            for slot in boxes.iter_mut().take(count) {
                slot.0 = content_width - slot.0 - slot.1;
            }
        }
        Self {
            count,
            boxes,
            separator: columns.separator,
        }
    }

    /// Number of columns (at least one).
    #[must_use]
    pub(crate) fn count(&self) -> usize {
        self.count.max(1)
    }

    /// Offset of column `index` from the content left edge, px.
    #[must_use]
    pub(crate) fn offset(&self, index: usize) -> f64 {
        self.column(index).0
    }

    /// Width of column `index`, px.
    #[must_use]
    pub(crate) fn width(&self, index: usize) -> f64 {
        self.column(index).1.max(1.0)
    }

    /// `(offset, width)` of column `index`; past the last, the last.
    fn column(&self, index: usize) -> (f64, f64) {
        self.boxes
            .get(index.min(self.count() - 1))
            .copied()
            .unwrap_or((0.0, 1.0))
    }

    /// Offsets (from the content left edge) of the separator lines: the middle
    /// of every gap between neighbouring columns, left to right.
    #[must_use]
    pub(crate) fn gap_centres(&self) -> Vec<f64> {
        let mut boxes: Vec<(f64, f64)> = self.boxes.iter().take(self.count()).copied().collect();
        boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
        boxes
            .iter()
            .zip(boxes.iter().skip(1))
            .map(|(left, right)| f64::midpoint(left.0 + left.1, right.0))
            .collect()
    }
}

/// Default page size (US Letter), in twips.
const DEFAULT_PAGE_WIDTH: i32 = 12_240;
/// Default page height (US Letter), in twips.
const DEFAULT_PAGE_HEIGHT: i32 = 15_840;
/// Default margin, in twips (1 inch).
const DEFAULT_MARGIN: i32 = 1_440;
/// Left and right margin of a section that is written without `w:pgMar`:
/// Word's (and WPS's) 1.25 in, not the 1 in of its own template. Pandoc writes
/// `<w:sectPr/>`, and WPS sets those books 18 pt narrower on each side than
/// we did (`CC0_DOCX/050`: 343 pages against our 501). A document with no
/// section properties at all keeps the 1 in default.
const DEFAULT_SIDE_MARGIN: i32 = 1_800;

/// Builds [`Geometry`] from optional section properties.
///
/// AUD-73: missing or non-positive `w:pgSz` dimensions become US Letter; when
/// `ctx` is provided a page warning is recorded.
#[must_use]
pub(crate) fn geometry_for(
    section: Option<&SectionProperties>,
    scale: f64,
    ctx: Option<&LayoutContext<'_>>,
    gutter_at_top: bool,
) -> Geometry {
    let page = section.and_then(|section| section.page_size);
    let side_default = if section.is_some() {
        DEFAULT_SIDE_MARGIN
    } else {
        DEFAULT_MARGIN
    };
    let raw_w = page.and_then(|page| page.width).map(Twips::value);
    let raw_h = page.and_then(|page| page.height).map(Twips::value);
    let replaced = raw_w.is_none_or(|value| value <= 0) || raw_h.is_none_or(|value| value <= 0);
    if replaced {
        if let Some(ctx) = ctx {
            ctx.warn("render.pgSz: non-positive or missing page size; using Letter".to_owned());
        }
    }
    let page_w = raw_w
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_PAGE_WIDTH);
    let page_h = raw_h
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_PAGE_HEIGHT);
    // `w` and `h` are the page; `orient` only tells the printer which way to
    // feed the paper (ISO/IEC 29500-1 §17.6.13). A landscape section written
    // 11905 x 16838 is a portrait page in Word and WPS: swapping it turned
    // four corpus documents sideways (CC0_DOCX/060: 40 pages against 34).
    let width = twips_to_px(page_w, scale);
    let height = twips_to_px(page_h, scale);

    let margins = section
        .and_then(|section| section.page_margins)
        .unwrap_or(PageMargins {
            top: Some(Twips(DEFAULT_MARGIN)),
            right: Some(Twips(side_default)),
            bottom: Some(Twips(DEFAULT_MARGIN)),
            left: Some(Twips(side_default)),
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
    // `w:pgMar/@w:gutter` widens the binding-side margin: the left one, or the
    // right one under `w:rtlGutter`.
    let gutter = margins
        .gutter
        .map_or(0.0, |gutter| twips_to_px(gutter.value().max(0), scale));
    let rtl_gutter = section.is_some_and(|section| section.rtl_gutter);
    let (left_gutter, right_gutter) = if gutter_at_top {
        (0.0, 0.0)
    } else if rtl_gutter {
        (0.0, gutter)
    } else {
        (gutter, 0.0)
    };
    let left = value(margins.left, side_default) + left_gutter;
    let right = value(margins.right, side_default) + right_gutter;
    let columns = ColumnLayout::from_columns(
        section.and_then(|section| section.columns.as_ref()),
        section.is_some_and(|section| section.bidi),
        width - left - right,
        scale,
    );
    Geometry {
        width,
        height,
        left,
        top: value(margins.top, DEFAULT_MARGIN) + if gutter_at_top { gutter } else { 0.0 },
        right,
        bottom: value(margins.bottom, DEFAULT_MARGIN),
        grid_line_pitch,
        columns,
        active_column: 0,
    }
}

/// Geometry in the text-flow coordinate system of a vertical section.
pub(crate) fn body_geometry_for(
    section: Option<&SectionProperties>,
    scale: f64,
    ctx: Option<&LayoutContext<'_>>,
    gutter_at_top: bool,
) -> Geometry {
    use strict_ooxml_wml::model::values::TextDirection;
    let mut geometry = geometry_for(section, scale, ctx, gutter_at_top);
    let direction = section.and_then(|s| s.text_direction);
    let old = geometry;
    match direction {
        Some(TextDirection::TbRl | TextDirection::TbRlV) => {
            geometry.width = old.height;
            geometry.height = old.width;
            geometry.left = old.top;
            geometry.top = old.right;
            geometry.right = old.bottom;
            geometry.bottom = old.left;
        }
        Some(TextDirection::BtLr) => {
            geometry.width = old.height;
            geometry.height = old.width;
            geometry.left = old.bottom;
            geometry.top = old.left;
            geometry.right = old.top;
            geometry.bottom = old.right;
        }
        Some(TextDirection::TbLrV) => {
            geometry.width = old.height;
            geometry.height = old.width;
            geometry.left = old.top;
            geometry.top = old.left;
            geometry.right = old.bottom;
            geometry.bottom = old.right;
        }
        _ => return geometry,
    }
    geometry.columns = ColumnLayout::from_columns(
        section.and_then(|s| s.columns.as_ref()),
        section.is_some_and(|s| s.bidi),
        geometry.content_width(),
        scale,
    );
    geometry
}

/// Heading numbers per outline level (`Heading 1`..`Heading 9`) for one page.
pub(crate) type ChapterNumbers = [Option<(u32, crate::notes::NumberFormat)>; 9];

/// A page-positioned float's wrap area: `(float, page, area)`.
pub(crate) type PageFloat = (u32, usize, crate::layout::exclusions::PageExclusion);

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
    pub(crate) font_programs: std::sync::Arc<crate::font::embedded::FontPrograms>,
    /// Media emission mode.
    pub media_mode: MediaMode,
    /// Footnote/endnote numbering.
    pub note_numbers: crate::notes::NoteNumbering,
    /// List numbering markers by paragraph location.
    pub numbering: crate::numbering::NumberingMarkers,
    /// How many block containers are open where the painter will next look.
    ///
    /// The depth travels as a parameter through `layout_blocks_inline`,
    /// `layout_table` and `layout_cell_content`, and it is *also* published here
    /// because one recursion does not go through them: a text box is laid out from
    /// the paint stage (`paint::graphics::anchor_items`), which is reached from
    /// paragraph layout and would otherwise need the depth threaded through every
    /// inline painter to reach it. One number, written at the one place that owns
    /// the recursion and read where the parameter is not in scope, is better than
    /// a second, drifting copy of it in each painter.
    pub(crate) block_depth: std::cell::Cell<u32>,
    /// Warnings the layout wants the page to carry.
    ///
    /// Laid out rather than returned: the pipeline is thirty functions deep and
    /// only `layout_table` and `layout_paragraph` have anything to say, and both
    /// return a value the callers already use. A channel is the alternative to
    /// threading a `Vec<String>` through every one of them.
    pub(crate) warnings: std::cell::RefCell<Vec<String>>,
    /// Page-dependent field values for header/footer layout (AUD-70).
    ///
    /// Set while a header/footer region with PAGE/NUMPAGES/SECTIONPAGES/SECTION
    /// is being laid out; `None` during body pagination (body fields resolve in
    /// `Paginator::resolve_fields`).
    pub(crate) field_env: std::cell::Cell<Option<crate::fields::FieldEnv>>,
    /// Section whose body is currently being measured, including nested cells.
    pub(crate) current_section: std::cell::Cell<usize>,
    /// Numbered headings active on each page, carried between layout passes.
    pub(crate) chapter_numbers: std::cell::RefCell<Vec<ChapterNumbers>>,
    /// Document-wide paint-item counter for [`ResourceLimits::max_render_items`]
    /// (AUD-72). Reset at the start of each layout pass.
    pub(crate) render_items: std::cell::Cell<u64>,
    /// `(left, top, content width, page width, page height)` in px for frames.
    ///
    /// Body pagination writes the current section here before a table. A page
    /// anchor uses the page box; a margin or text anchor uses the content box.
    pub(crate) frame_anchor: std::cell::Cell<FrameAnchor>,
    /// Height of a static header/footer part at a given content box.
    ///
    /// Key is `(part, left.to_bits, content_width.to_bits)`. Parts that contain
    /// PAGE/NUMPAGES/SECTIONPAGES/SECTION are not stored: their height can
    /// change with the field value. A cache hit must not charge render items.
    pub(crate) region_heights: std::cell::RefCell<HashMap<(PartId, u64, u64), f64>>,
    /// When set, simple-script measurement uses [`TextAdvanceKind::Metric`].
    ///
    /// Non-last justified lines need those advances for the text-class WPS
    /// references. Last lines are remasured to shaped so a both/justify
    /// single-line run matches PDF shaping (D03).
    pub(crate) metric_advances: std::cell::Cell<bool>,
    /// How far each frame signature has already been filled, in px.
    ///
    /// Paragraphs that share one `w:framePr` belong to one frame even when a
    /// table cell separates them. Each one continues where the previous one
    /// stopped. Cleared at the start of a layout pass and at each page break.
    pub(crate) frame_cursors:
        std::cell::RefCell<Vec<(strict_ooxml_wml::model::props::FrameProperties, f64)>>,
    /// Wrap areas of page-positioned floats from the previous layout pass:
    /// `(float, page, area)`, put on their page before any of its text
    /// (WQ-016/WQ-019). A float is its order among the page floats a pass
    /// places.
    pub(crate) page_floats_preload: std::cell::RefCell<Vec<crate::layout::PageFloat>>,
    /// Wrap areas of page-positioned floats placed in this pass.
    pub(crate) page_floats_record: std::cell::RefCell<Vec<crate::layout::PageFloat>>,
    /// Floats whose own preloaded wrap moved them to another page: they are
    /// no longer put on a page ahead of their anchor.
    pub(crate) page_floats_banned: std::cell::RefCell<std::collections::BTreeSet<u32>>,
    /// Floats whose own preloaded wrap carried their anchor to a later page:
    /// placed on that page from then on, as WPS does (WQ-016/WQ-019).
    pub(crate) page_floats_forward: std::cell::RefCell<std::collections::BTreeMap<u32, usize>>,
    /// How often each float changed page between passes.
    pub(crate) page_floats_moves: std::cell::RefCell<std::collections::BTreeMap<u32, u32>>,
    /// The next page float's number in this pass.
    pub(crate) page_floats_next: std::cell::Cell<u32>,
    /// Exact line height of the previous paragraph in the current frame, in px.
    ///
    /// Used to choose the 80% Exact grid after a taller Exact neighbour
    /// (Clio L16055 after `line=216`) while keeping `natural.max(exact*0.8)`
    /// for the first Exact line in a frame (Clio SNP labels).
    pub(crate) frame_prior_exact: std::cell::Cell<Option<f64>>,
    /// Once a taller Exact neighbour forces the 80% grid, keep it for the rest
    /// of the frame so same-size Exact follow-ups (Clio H16142 after L16055)
    /// stay on that pitch.
    pub(crate) frame_force_exact_grid: std::cell::Cell<bool>,
    /// Whether a frame's contents are being laid out (`layout_frame_contents`).
    ///
    /// The WPS first-Exact-line rule (`natural.max(exact*0.8)`) is a frame
    /// behaviour (Clio SNP labels); body Exact lines stay on the 80% grid.
    pub(crate) in_frame: std::cell::Cell<bool>,
}

impl LayoutContext<'_> {
    /// Records a warning for every page of this document.
    pub(crate) fn warn(&self, message: String) {
        let mut warnings = self.warnings.borrow_mut();
        if !warnings.contains(&message) {
            warnings.push(message);
        }
    }

    /// The warnings recorded so far, in the order they first appeared.
    pub(crate) fn take_warnings(&self) -> Vec<String> {
        self.warnings.take()
    }

    /// Records how deep the block recursion is for callers reached from paint.
    pub(crate) fn set_block_depth(&self, depth: u32) {
        self.block_depth.set(depth);
    }

    /// The depth recorded by [`set_block_depth`](Self::set_block_depth).
    pub(crate) fn block_depth(&self) -> u32 {
        self.block_depth.get()
    }

    /// Charges `count` paint items against the document-wide budget (AUD-72).
    pub(crate) fn charge_items(&self, count: usize) -> strict_ooxml_core::error::Result<()> {
        strict_ooxml_core::control::checkpoint()?;
        let limit = self.options.limits.max_render_items;
        let added = u64::try_from(count).unwrap_or(u64::MAX);
        let next = self.render_items.get().saturating_add(added);
        if next > limit {
            return Err(crate::error::RenderError::LimitExceeded {
                what: "render items",
                limit,
                actual: next,
            }
            .into_strict());
        }
        self.render_items.set(next);
        Ok(())
    }

    /// Vertical offset already used by earlier paragraphs of this frame.
    pub(crate) fn frame_resume(
        &self,
        frame: &strict_ooxml_wml::model::props::FrameProperties,
    ) -> f64 {
        self.frame_cursors
            .borrow()
            .iter()
            .find(|(signature, _)| signature == frame)
            .map_or(0.0, |(_, y)| *y)
    }

    /// Records that `height` px of this frame have been filled.
    pub(crate) fn frame_advance(
        &self,
        frame: &strict_ooxml_wml::model::props::FrameProperties,
        height: f64,
    ) {
        let mut cursors = self.frame_cursors.borrow_mut();
        if let Some((_, y)) = cursors.iter_mut().find(|(signature, _)| signature == frame) {
            *y += height;
        } else {
            cursors.push((frame.clone(), height));
        }
    }

    /// Starts frame columns over. A new page, or a new layout pass, does this.
    pub(crate) fn reset_frame_cursors(&self) {
        self.frame_cursors.borrow_mut().clear();
    }
}

impl LayoutContext<'_> {
    /// Pixel-per-point scale shorthand.
    #[must_use]
    pub(crate) fn size_px(&self, size_pt: f64) -> f64 {
        crate::units::pt_to_px(size_pt, self.options.scale)
    }

    /// Measures the advance width of `text` for a run in px.
    ///
    /// [`Self::metric_advances`] selects shaped versus `hmtx` sums. Callers stamp
    /// the matching [`TextAdvanceKind`] on each [`TextItem`].
    #[must_use]
    pub(crate) fn measure(&self, text: &str, run: &ComputedRun) -> f64 {
        self.measure_with_next(text, run, None)
    }

    /// Like [`Self::measure`], with the first non-space of the following token
    /// so trailing post-punctuation spaces can collapse under compressPunctuation.
    #[must_use]
    pub(crate) fn measure_with_next(
        &self,
        text: &str,
        run: &ComputedRun,
        following: Option<char>,
    ) -> f64 {
        self.measure_with_next_factor(text, run, following, 1.0)
    }

    /// [`Self::measure_with_next`] with a plain-space advance factor.
    #[must_use]
    pub(crate) fn measure_with_next_factor(
        &self,
        text: &str,
        run: &ComputedRun,
        following: Option<char>,
        plain_space_factor: f64,
    ) -> f64 {
        let shown = crate::font::present_text(&run.family, text);
        let size_px = self.size_px(run.size_pt);
        let family = crate::style::chosen_family(run, text);
        let requested =
            if run.complex_family.is_some() && crate::style::script_is_complex_only(text) {
                run.complex_family.as_deref().unwrap_or(&run.family)
            } else {
                &run.family
            };
        if run.font_programs.for_resource(&family).is_none()
            && crate::font::family::known_family(requested).is_none()
            && !text.trim().is_empty()
        {
            self.warn(format!("font.substitution: unavailable family {requested:?}; using {family:?}, metrics may differ"));
        }
        if let Some(program) = run.font_programs.for_resource(&family) {
            use crate::font::embedded::Embedding;
            match program.embedding {
                Embedding::Allowed => {}
                Embedding::WholeProgram => self.warn(format!(
                    "font.embedding.no-subset: {requested:?} allows no subset; PDF draws it with a bundled face at its own positions"
                )),
                Embedding::Forbidden => self.warn(format!(
                    "font.embedding.restricted: {requested:?} allows no embedding; it is measured, and drawn with a bundled face"
                )),
            }
        }
        let complex = crate::font::needs_complex_script(&shown);
        let non_space = shown.chars().filter(|ch| !ch.is_whitespace()).count() as f64;
        let extra = crate::style::spacing_px(run, size_px) * non_space;
        let compress = crate::style::compress_punctuation(
            self.document.settings.character_spacing_control.as_deref(),
        );
        let chars: Vec<char> = shown.chars().collect();
        if run.font_programs.for_resource(&family).is_some()
            || complex
            || run.rtl.is_some()
            || run.kerning_pt.is_some()
            || run.language != strict_ooxml_wml::model::props::Language::default()
            || !self.metric_advances.get()
        {
            let shaped = crate::font::shape_run(&shown, &family, run, self.font);
            if shaped.status == crate::font::ShapeStatus::DegradedComplexScript {
                self.warn(crate::font::COMPLEX_SCRIPT_WARNING.to_owned());
            }
            let mut width = shaped.total_advance_em * size_px + extra;
            for &ch in &chars {
                let ordinary = crate::font::embedded::run_advance_em(run, &family, ch, self.font);
                width += (crate::style::space_advance_em(run, &family, ch, ordinary) - ordinary)
                    * size_px;
            }
            if compress {
                let mut prev = None;
                for (i, &ch) in chars.iter().enumerate() {
                    let next = chars.get(i + 1).copied().or(following);
                    let factor = crate::style::compressed_char_factor(
                        prev,
                        ch,
                        next,
                        true,
                        plain_space_factor,
                    );
                    if (factor - 1.0).abs() > f64::EPSILON {
                        width -= crate::style::space_advance_em(
                            run,
                            &family,
                            ch,
                            crate::font::embedded::run_advance_em(run, &family, ch, self.font),
                        ) * size_px
                            * (1.0 - factor);
                    }
                    if !ch.is_whitespace() || factor > 0.0 {
                        prev = Some(ch);
                    }
                }
            }
            return width * run.char_scale;
        }
        let mut total = 0.0;
        let mut prev = None;
        for (i, &ch) in chars.iter().enumerate() {
            let next = chars.get(i + 1).copied().or(following);
            let factor =
                crate::style::compressed_char_factor(prev, ch, next, compress, plain_space_factor);
            total += crate::style::space_advance_em(
                run,
                &family,
                ch,
                crate::font::embedded::run_advance_em(run, &family, ch, self.font),
            ) * factor;
            if !ch.is_whitespace() || factor > 0.0 {
                prev = Some(ch);
            }
        }
        (total * size_px + extra) * run.char_scale
    }

    /// Advance kind [`Self::measure`] currently produces for `text`.
    #[must_use]
    pub(crate) fn advance_kind_for(&self, text: &str, run: &ComputedRun) -> TextAdvanceKind {
        let shown = crate::font::present_text(&run.family, text);
        if run
            .font_programs
            .for_resource(&crate::style::chosen_family(run, &shown))
            .is_some()
            || crate::font::needs_complex_script(&shown)
            || run.rtl.is_some()
            || run.kerning_pt.is_some()
            || run.language != strict_ooxml_wml::model::props::Language::default()
            || !self.metric_advances.get()
        {
            TextAdvanceKind::Shaped
        } else {
            TextAdvanceKind::Metric
        }
    }
}

/// Result of laying out the whole document.
pub(crate) struct Layout {
    /// Pages in order.
    pub pages: Vec<PlacedPage>,
    /// Floating (anchored) objects with the page they belong to.
    pub anchors: Vec<floating::PendingAnchor>,
    /// What the layout had to say about the document, for `Page::warnings`.
    pub warnings: Vec<String>,
    /// Body text lines that `w:lnNumType` numbers.
    pub line_marks: Vec<LineMark>,
    /// Multi-column areas of the body, for the `w:sep` lines between columns.
    pub column_regions: Vec<ColumnRegion>,
}

/// The part of one page a multi-column layout occupied: where the line
/// between its columns (`w:cols/@w:sep`) runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ColumnRegion {
    /// Zero-based page index.
    pub page: usize,
    /// Page x of the content left edge the column offsets start from.
    pub left: f64,
    /// Page y of the top of the columns.
    pub top: f64,
    /// Page y of the bottom of the longest column.
    pub bottom: f64,
    /// The columns.
    pub columns: ColumnLayout,
}

/// Reference boxes of a frame (`w:framePr`) or a floating table (`w:tblpPr`):
/// `page`, `margin` and `text` - the column the frame is anchored in
/// (ISO/IEC 29500-1 §17.3.1.11, §17.4.58).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FrameAnchor {
    /// Page x of the left margin.
    pub margin_x: f64,
    /// Page y of the top margin.
    pub margin_y: f64,
    /// Width between the margins.
    pub content_width: f64,
    /// Height between the margins.
    pub content_height: f64,
    /// Page x of the column the frame's text sits in.
    pub column_x: f64,
    /// Width of that column.
    pub column_width: f64,
    /// Page width.
    pub page_width: f64,
    /// Page height.
    pub page_height: f64,
}

impl FrameAnchor {
    /// The boxes of `geometry`, with its active column as the `text` box.
    #[must_use]
    pub(crate) fn of(geometry: &Geometry) -> Self {
        let (column_x, column_width) = geometry.active_column_box();
        Self {
            margin_x: geometry.left,
            margin_y: geometry.top,
            content_width: geometry.content_width(),
            content_height: geometry.content_height(),
            column_x,
            column_width,
            page_width: geometry.width,
            page_height: geometry.height,
        }
    }
}

/// A body line placed on a page, for line numbering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LineMark {
    /// Zero-based page index.
    pub page: usize,
    /// Section the line belongs to.
    pub section: usize,
    /// Left edge of the line's column, px.
    pub column_left: f64,
    /// Baseline, px from the page top.
    pub baseline: f64,
}
