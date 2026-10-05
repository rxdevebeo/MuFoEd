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

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::props::{PageMargins, SectionProperties};
use strict_ooxml_wml::model::values::{PageOrientation, Twips};
use strict_ooxml_wml::model::Document;

use crate::font::FontProvider;
use crate::style::ComputedRun;
use crate::units::twips_to_px;
use crate::{MediaMode, MediaSource, RenderOptions};

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
    /// Computed field marker, when this item is a PAGE/NUMPAGES/SECTIONPAGES/
    /// SECTION placeholder resolved at placement time.
    ///
    /// A backend that draws the text needs to know which fragment is a page
    /// number, so the marker is public — but the marker type stays crate-internal
    /// and is matched by behaviour (`is_page_number` and friends), which is what
    /// keeps a second backend from depending on the field machinery's internals.
    pub field: Option<crate::fields::FieldMarker>,
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
    /// Page-absolute frame children. The row offset is not applied to these.
    pub page_frames: Vec<Item>,
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
///
/// AUD-73: missing or non-positive `w:pgSz` dimensions become US Letter; when
/// `ctx` is provided a page warning is recorded.
#[must_use]
pub(crate) fn geometry_for(
    section: Option<&SectionProperties>,
    scale: f64,
    ctx: Option<&LayoutContext<'_>>,
) -> Geometry {
    let page = section.and_then(|section| section.page_size);
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
    let mut width = twips_to_px(page_w, scale);
    let mut height = twips_to_px(page_h, scale);
    if matches!(
        page.and_then(|page| page.orientation),
        Some(PageOrientation::Landscape)
    ) && width < height
    {
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
    let mut geometry = Geometry {
        width,
        height,
        left: value(margins.left, DEFAULT_MARGIN),
        top: value(margins.top, DEFAULT_MARGIN),
        right: value(margins.right, DEFAULT_MARGIN),
        bottom: value(margins.bottom, DEFAULT_MARGIN),
        grid_line_pitch,
    };
    reserve_header_footer(ctx, section, &mut geometry);
    geometry
}

/// Grows the top and bottom margins so a header or footer that does not fit in
/// the margin is not painted over the body (A15).
fn reserve_header_footer(
    ctx: Option<&LayoutContext<'_>>,
    section: Option<&SectionProperties>,
    geometry: &mut Geometry,
) {
    let (Some(ctx), Some(section)) = (ctx, section) else {
        return;
    };
    let (extra_top, extra_bottom) =
        crate::layout::headerfooter::body_reserve(ctx, section, geometry);
    geometry.top = (geometry.top + extra_top).min(geometry.height * 0.75);
    geometry.bottom = (geometry.bottom + extra_bottom).min(geometry.height - geometry.top);
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
    /// Document-wide paint-item counter for [`ResourceLimits::max_render_items`]
    /// (AUD-72). Reset at the start of each layout pass.
    pub(crate) render_items: std::cell::Cell<u64>,
    /// `(left margin, top margin, content width)` in px for a frame inside a cell.
    ///
    /// Body pagination writes the current section here before a table. A page
    /// anchor ignores the margins; a margin or text anchor adds them. A frame
    /// without `w:w` uses the content width.
    pub(crate) frame_anchor: std::cell::Cell<(f64, f64, f64)>,
    /// How far each frame signature has already been filled, in px.
    ///
    /// Paragraphs that share one `w:framePr` belong to one frame even when a
    /// table cell separates them. Each one continues where the previous one
    /// stopped. Cleared at the start of a layout pass and at each page break.
    pub(crate) frame_cursors:
        std::cell::RefCell<Vec<(strict_ooxml_wml::model::props::FrameProperties, f64)>>,
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
    #[must_use]
    pub(crate) fn measure(&self, text: &str, run: &ComputedRun) -> f64 {
        let size_px = self.size_px(run.size_pt);
        let family = crate::style::chosen_family(run, text);
        text.chars()
            .map(|ch| self.font.advance_em(&family, ch, run.bold, run.italic) * size_px)
            .sum()
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
}
