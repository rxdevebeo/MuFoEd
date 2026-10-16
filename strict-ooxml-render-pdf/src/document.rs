//! Assembles placed pages into a PDF document.
//!
//! Nothing here measures anything: the geometry comes from
//! [`strict_ooxml_render_svg::place_pages`], so the PDF cannot disagree with the
//! SVG about a line break (ADR-0008). This module translates the placed items
//! into PDF operators and writes the file structure around them.
//!
//! Object numbering is explicit rather than implicit. `pdf-writer` does not
//! allocate ids, and the order in which they are handed out is what makes the
//! output byte-identical between runs (SC-1), so the allocation order is part of
//! the contract and is written down in `PdfBuilder::allocate`.

use std::collections::BTreeMap;
use std::sync::Arc;

use pdf_writer::types::FontFlags;
use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Str};
use strict_ooxml_core::control;
use strict_ooxml_core::error::{Result, StrictError};
use strict_ooxml_core::part::PartId;
use strict_ooxml_render_svg::layout::{
    Item, PathItem, PathPaint, PlacedPage, TextAdvanceKind, TextItem,
};
use strict_ooxml_render_svg::{MediaSource, RenderOptions};

use crate::font::{EmbeddedFont, FaceCollector, FaceKey};
use crate::glyphs::GlyphForms;
use crate::image::{encode_with_limit, Encoded};
use crate::matrix::Matrix;
use crate::path::{self, Segment};
use crate::report::PdfReport;
use crate::units::{dash_array, flate, is_paint, px_to_pt, rgb};

/// What a render produced.
#[derive(Clone, Debug)]
pub struct PdfOutput {
    /// The PDF bytes.
    pub bytes: Vec<u8>,
    /// How many pages the document has.
    pub page_count: usize,
    /// Faces that were embedded, in the order they were written.
    pub embedded_faces: Vec<FaceKey>,
    /// What could not be represented.
    pub report: PdfReport,
}

/// The first reserved object id, after the catalog and the page tree.
const FIRST_ID: u32 = 3;

/// A face that has been subset, with the resource name it is written under.
type PlacedFont = (Arc<EmbeddedFont>, String);

/// Everything one page needs while it is being written.
struct PageWriter<'a> {
    content: Content,
    page_height_pt: f64,
    scale: f64,
    fonts: &'a [PlacedFont],
    media: Option<&'a dyn MediaSource>,
    /// The document's image resource names, fixed before any content is written.
    names: &'a BTreeMap<PartId, String>,
    images: BTreeMap<PartId, String>,
    paints: Vec<(String, PathPaint)>,
    alpha: f64,
    alphas: std::collections::BTreeSet<u8>,
    report: &'a mut PdfReport,
    /// Glyph outlines of the whole document, shared by every page.
    glyph_forms: &'a mut GlyphForms,
    /// The outline forms this page draws.
    glyph_names: std::collections::BTreeSet<String>,
}

/// The bundled faces' metrics, parsed once per process.
///
/// `show_metric` built a fresh provider for every metric text item, which parses
/// all 22 bundled faces each time: a page of justified text paid that cost per
/// word.
fn builtin_provider() -> &'static strict_ooxml_render_svg::font::BuiltinFontProvider {
    static PROVIDER: std::sync::OnceLock<strict_ooxml_render_svg::font::BuiltinFontProvider> =
        std::sync::OnceLock::new();
    PROVIDER.get_or_init(strict_ooxml_render_svg::font::BuiltinFontProvider::new)
}

/// Renders placed pages to PDF without resolving media bytes.
pub fn render(pages: &[PlacedPage], options: &RenderOptions) -> Result<PdfOutput> {
    render_with_source(pages, options, None)
}

/// Renders placed pages to PDF, resolving image bytes through `media`.
///
/// # Errors
///
/// Returns a [`StrictError`] when a part cannot be read or a limit is exceeded.
// The function is the file assembly: collect the faces, build every content
// stream, hand out object ids, then write the catalogue, the page tree and the
// objects. Splitting it would put the id order — which is what makes the output
// reproducible — across two files.
#[allow(clippy::too_many_lines)]
pub fn render_with_source(
    pages: &[PlacedPage],
    options: &RenderOptions,
    media: Option<&dyn MediaSource>,
) -> Result<PdfOutput> {
    let scale = options.scale;
    let mut report = PdfReport::new();
    let fonts = collect_fonts(pages, &mut report);
    // One name per picture, for the whole document, fixed before anything is
    // written: the content stream says `/Im… Do` and the page's `/XObject`
    // dictionary has to answer to that same name, and a name chosen twice is a
    // dictionary with two entries under one key — a malformed page where one of
    // the two pictures is simply not there.
    let image_names = name_the_images(
        pages
            .iter()
            .flat_map(|page| page.items.iter())
            .flat_map(Item::leaves)
            .filter_map(|item| match item {
                Item::Image(image) => image.part.as_ref(),
                _ => None,
            }),
    );
    let image_sizes = image_raster_sizes(pages);

    // Pass one: content streams and the images each page needs.
    let mut streams = Vec::with_capacity(pages.len());
    let mut glyph_forms = GlyphForms::default();
    let _scope = options.control.as_ref().map(control::OpenControl::enter);
    control::stage(control::Stage::Painting, pages.len() as u64);
    for page in pages {
        control::checkpoint_now()?;
        control::advance(streams.len() as u64);
        let mut writer = PageWriter {
            content: Content::new(),
            page_height_pt: px_to_pt(page.height_px, scale),
            scale,
            fonts: &fonts,
            media,
            names: &image_names,
            images: BTreeMap::new(),
            paints: Vec::new(),
            alpha: 1.0,
            alphas: std::collections::BTreeSet::default(),
            report: &mut report,
            glyph_forms: &mut glyph_forms,
            glyph_names: std::collections::BTreeSet::new(),
        };
        writer.draw_page(page, options.background);
        streams.push((
            writer.content.finish().into_vec(),
            writer.images,
            writer.paints,
            writer.alphas,
            writer.glyph_names,
        ));
    }

    // Pass two: the file, with ids handed out in a fixed order.
    //
    // An image XObject is written once per `PartId` (AUD-80). Every page that
    // draws it still lists the same object under `/Resources /XObject`: skipping
    // that entry leaves the content stream's `/Im… Do` unresolved on every page
    // after the first, so a header picture vanishes from page 2 onward.
    let mut builder = PdfBuilder::new();
    let mut image_refs: BTreeMap<PartId, (Ref, Option<Ref>)> = BTreeMap::new();
    let mut page_objects: Vec<PageObjects> = Vec::with_capacity(pages.len());
    for (_, images, paints, _, _) in &streams {
        let content_id = builder.allocate();
        let mut image_ids: BTreeMap<PartId, (Ref, Option<Ref>)> = BTreeMap::new();
        for part in images.keys() {
            if let Some(&(id, mask)) = image_refs.get(part) {
                image_ids.insert(part.clone(), (id, mask));
                continue;
            }
            let Some(bytes) = read_media(media, part)? else {
                continue;
            };
            let rasterized = image_sizes.get(part).and_then(|&(width, height)| {
                strict_ooxml_render_svg::metafile::rasterize_metafile_preview(
                    &bytes,
                    f64::from(width),
                    f64::from(height),
                    options.limits.max_single_uncompressed,
                )
            });
            let bytes = rasterized.as_deref().unwrap_or(&bytes);
            // The uncompressed budget is the package's own part ceiling: a PNG
            // cannot expand past what a part of the same document would be
            // allowed to hold (AUD-14).
            let encoded = match encode_with_limit(
                part.as_str(),
                bytes,
                options.limits.max_single_uncompressed,
            ) {
                Ok(encoded) => encoded,
                Err(reject) => {
                    report.record_image_reject(&reject);
                    continue;
                }
            };
            let id = builder.allocate();
            let mask = if matches!(&encoded, Encoded::Raw { alpha: Some(_), .. }) {
                Some(builder.allocate())
            } else {
                None
            };
            image_refs.insert(part.clone(), (id, mask));
            image_ids.insert(part.clone(), (id, mask));
            builder.pending_images.push((id, mask, encoded));
        }
        page_objects.push(PageObjects {
            content: content_id,
            page: builder.allocate(),
            images: image_ids,
            paints: paints
                .iter()
                .map(|(name, paint)| (name.clone(), builder.allocate(), paint.clone()))
                .collect(),
        });
    }
    // A CID font needs six objects; Identity-H is a predefined encoding name.
    let font_ids: Vec<FontIds> = fonts.iter().map(|_| builder.allocate_six()).collect();
    let glyph_refs: BTreeMap<&str, Ref> = glyph_forms
        .forms()
        .iter()
        .map(|form| (form.name.as_str(), builder.allocate()))
        .collect();

    let mut alpha_ids = BTreeMap::new();
    for alpha in streams
        .iter()
        .flat_map(|(_, _, _, alphas, _)| alphas.iter())
    {
        alpha_ids
            .entry(*alpha)
            .or_insert_with(|| builder.allocate());
    }
    let mut pdf = Pdf::new();
    for (alpha, id) in &alpha_ids {
        pdf.ext_graphics(*id)
            .non_stroking_alpha(f32::from(*alpha) / 255.0)
            .stroking_alpha(f32::from(*alpha) / 255.0);
    }
    pdf.catalog(builder.catalog).pages(builder.pages);
    pdf.pages(builder.pages)
        .kids(page_objects.iter().map(|page| page.page))
        .count(i32::try_from(page_objects.len()).unwrap_or(i32::MAX));
    for (id, mask, encoded) in &builder.pending_images {
        write_image(&mut pdf, *id, *mask, encoded);
    }
    for ((font, _), ids) in fonts.iter().zip(font_ids.iter()) {
        write_font(&mut pdf, *ids, font);
    }
    for form in glyph_forms.forms() {
        let Some(&id) = glyph_refs.get(form.name.as_str()) else {
            continue;
        };
        let (data, compressed) = flate(&form.content);
        let mut xobject = pdf.form_xobject(id, &data);
        if compressed {
            xobject.filter(Filter::FlateDecode);
        }
        let [left, bottom, right, top] = form.bbox;
        xobject.bbox(Rect::new(left, bottom, right, top));
        let unit = 1.0 / form.units_per_em;
        xobject.matrix([unit, 0.0, 0.0, unit, 0.0, 0.0]);
    }
    for objects in &page_objects {
        for (_, id, paint) in &objects.paints {
            crate::fills::write(&mut pdf, *id, paint);
        }
    }
    // Pass one pushed one stream per page and the loop above one set of ids per
    // stream, so the three line up.
    for ((page, objects), (content, _, _, alphas, glyph_names)) in
        pages.iter().zip(&page_objects).zip(&streams)
    {
        let width_pt = px_to_pt(page.width_px, scale) as f32;
        let height_pt = px_to_pt(page.height_px, scale) as f32;
        {
            let (data, compressed) = flate(content);
            let mut stream = pdf.stream(objects.content, &data);
            if compressed {
                stream.filter(Filter::FlateDecode);
            }
        }
        let mut page_writer = pdf.page(objects.page);
        page_writer.parent(builder.pages);
        page_writer.media_box(Rect::new(0.0, 0.0, width_pt, height_pt));
        {
            let mut resources = page_writer.resources();
            {
                let mut states = resources.ext_g_states();
                for alpha in alphas {
                    if let Some(id) = alpha_ids.get(alpha) {
                        states.pair(Name(format!("A{alpha}").as_bytes()), *id);
                    }
                }
            }
            {
                let mut shadings = resources.shadings();
                for (name, id, paint) in &objects.paints {
                    if matches!(paint, PathPaint::LinearGradient { .. }) {
                        shadings.pair(Name(name.as_bytes()), *id);
                    }
                }
            }
            {
                let mut patterns = resources.patterns();
                for (name, id, paint) in &objects.paints {
                    if matches!(paint, PathPaint::Pattern { .. }) {
                        patterns.pair(Name(name.as_bytes()), *id);
                    }
                }
            }
            {
                let mut fonts_dict = resources.fonts();
                for ((_, name), ids) in fonts.iter().zip(font_ids.iter()) {
                    fonts_dict.pair(Name(name.as_bytes()), ids.type0);
                }
            }
            {
                let mut xobjects = resources.x_objects();
                for (part, (id, _)) in &objects.images {
                    // Every drawn image was named first; an unnamed one is
                    // never referenced from a content stream, so skip it.
                    let Some(name) = image_names.get(part) else {
                        continue;
                    };
                    xobjects.pair(Name(name.as_bytes()), *id);
                }
                for name in glyph_names {
                    if let Some(id) = glyph_refs.get(name.as_str()) {
                        xobjects.pair(Name(name.as_bytes()), *id);
                    }
                }
            }
        }
        page_writer.contents(objects.content);
    }

    Ok(PdfOutput {
        bytes: pdf.finish(),
        page_count: pages.len(),
        embedded_faces: fonts.iter().map(|(font, _)| font.key.clone()).collect(),
        report,
    })
}

fn image_raster_sizes(pages: &[PlacedPage]) -> BTreeMap<PartId, (u32, u32)> {
    let mut sizes = BTreeMap::new();
    for page in pages {
        for item in page.items.iter().flat_map(Item::leaves) {
            let Item::Image(image) = item else {
                continue;
            };
            let Some(part) = image.part.as_ref() else {
                continue;
            };
            let size = (image.w.ceil() as u32, image.h.ceil() as u32);
            sizes
                .entry(part.clone())
                .and_modify(|current: &mut (u32, u32)| {
                    current.0 = current.0.max(size.0);
                    current.1 = current.1.max(size.1);
                })
                .or_insert(size);
        }
    }
    sizes
}

/// The ids one page needs.
struct PageObjects {
    /// Its content stream.
    content: Ref,
    /// The page object itself.
    page: Ref,
    /// Image id and optional mask id, by media part.
    images: BTreeMap<PartId, (Ref, Option<Ref>)>,
    paints: Vec<(String, Ref, PathPaint)>,
}

/// The object ids a CID font occupies (six: Type0, program, descriptor, CID
/// font, ToUnicode, and the `/CIDToGIDMap` stream).
#[derive(Clone, Copy, Debug)]
struct FontIds {
    type0: Ref,
    file: Ref,
    descriptor: Ref,
    cid: Ref,
    to_unicode: Ref,
    cid_to_gid: Ref,
}

/// Allocates ids in a fixed order and holds the objects still to be written.
struct PdfBuilder {
    catalog: Ref,
    pages: Ref,
    next: i32,
    pending_images: Vec<(Ref, Option<Ref>, Encoded)>,
}

impl PdfBuilder {
    fn new() -> Self {
        Self {
            catalog: Ref::new(1),
            pages: Ref::new(2),
            next: i32::try_from(FIRST_ID).unwrap_or(i32::MAX),
            pending_images: Vec::new(),
        }
    }

    /// Hands out the next id.
    ///
    /// The order of the calls *is* the file layout, so it must not depend on a
    /// hash map's iteration order or on anything measured at run time.
    fn allocate(&mut self) -> Ref {
        let id = Ref::new(self.next);
        self.next += 1;
        id
    }

    /// Hands out six consecutive ids, in the order `write_font` consumes them.
    fn allocate_six(&mut self) -> FontIds {
        FontIds {
            type0: self.allocate(),
            file: self.allocate(),
            descriptor: self.allocate(),
            cid: self.allocate(),
            to_unicode: self.allocate(),
            cid_to_gid: self.allocate(),
        }
    }
}

/// Reads a media part, reporting a missing source rather than failing.
fn read_media(media: Option<&dyn MediaSource>, part: &PartId) -> Result<Option<Vec<u8>>> {
    match media {
        None => Ok(None),
        Some(source) => match source.read_media(part) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(StrictError::MissingPart(_) | StrictError::MissingReferencedPart { .. }) => {
                Ok(None)
            }
            Err(error) => Err(error),
        },
    }
}

/// Subsets and orders the faces the pages use.
fn collect_fonts(pages: &[PlacedPage], report: &mut PdfReport) -> Vec<PlacedFont> {
    let mut collector = FaceCollector::new();
    let mut programs = BTreeMap::new();
    for page in pages {
        for item in page.items.iter().flat_map(Item::leaves) {
            let Item::Text(text) = item else {
                continue;
            };
            if text.is_caret_anchor() {
                continue;
            }
            let shown = strict_ooxml_render_svg::font::present_text(&text.run.family, &text.text);
            // The face that draws: a font program its licence keeps out of a
            // subset is drawn with its bundled face (`paint_family`).
            let family = strict_ooxml_render_svg::style::paint_family(&text.run, &shown, false);
            let Some(source) =
                strict_ooxml_render_svg::font::embedded::run_face_source(&text.run, &family)
            else {
                report.record_missing_face(&text.run.family);
                continue;
            };
            if let Some(program) = text.run.font_programs.for_resource(&family) {
                programs.insert(family.clone(), Arc::clone(program));
            }
            if text.advance == TextAdvanceKind::Metric {
                // Metric paint addresses scalar CIDs, not shaped clusters. A
                // ligature-only subset cannot serve its individual characters.
                for ch in shown.chars() {
                    collector.add(&source, text.run.bold, text.run.italic, ch);
                }
                continue;
            }
            if let Some(shaped) =
                strict_ooxml_render_svg::font::shape_bundled_run(&shown, &family, &text.run)
            {
                if shaped.glyphs.is_empty() {
                    for ch in shown.chars() {
                        collector.add(&source, text.run.bold, text.run.italic, ch);
                    }
                }
                for cluster in &shaped.clusters {
                    let unicode = shown
                        .get(cluster.byte_start..cluster.byte_end)
                        .unwrap_or("");
                    let end = cluster.glyph_start.saturating_add(cluster.glyph_count);
                    let Some(glyphs) = shaped.glyphs.get(cluster.glyph_start..end) else {
                        continue;
                    };
                    let labels = crate::glyphs::cluster_labels(glyphs, unicode);
                    for (glyph, label) in glyphs.iter().zip(&labels) {
                        collector.add_cluster(
                            &source,
                            text.run.bold,
                            text.run.italic,
                            glyph.glyph_id,
                            label,
                        );
                        // A right-to-left ligature's text layer is written
                        // letter by letter (`glyphs::rtl_ligature`).
                        for ch in crate::glyphs::rtl_ligature(label).unwrap_or_default() {
                            collector.add(&source, text.run.bold, text.run.italic, ch);
                        }
                    }
                }
            } else {
                for ch in shown.chars() {
                    collector.add(&source, text.run.bold, text.run.italic, ch);
                }
            }
        }
    }
    collector.add_notdef();

    let mut out: Vec<PlacedFont> = Vec::new();
    for (index, key) in collector.faces().iter().enumerate() {
        let Some(source) = programs
            .get(&key.family)
            .map(|program| strict_ooxml_render_svg::font::FaceSource {
                family: program.family.as_str(),
                data: &program.bytes,
                weight: None,
                single: false,
            })
            .or_else(|| {
                strict_ooxml_render_svg::font::face_source(&key.family, key.bold, key.italic)
            })
        else {
            report.record_missing_face(&key.family);
            continue;
        };
        match collector.build(key, &source) {
            Ok(font) => out.push((Arc::new(font), key.resource_name(index))),
            Err(error) => report.record_font_error(key, &error),
        }
    }
    out
}

impl PageWriter<'_> {
    /// Draws every item of a page, background first.
    fn draw_page(&mut self, page: &PlacedPage, background: bool) {
        if background {
            self.content.set_fill_rgb(1.0, 1.0, 1.0);
            let width = px_to_pt(page.width_px, self.scale) as f32;
            self.content
                .rect(0.0, 0.0, width, self.page_height_pt as f32);
            self.content.fill_nonzero();
        }
        for item in &page.items {
            self.item(item);
        }
    }

    #[allow(clippy::many_single_char_names)] // PDF `cm` operands a..f
    fn item(&mut self, item: &Item) {
        match item {
            Item::Group(group) => {
                let [a, b, c, d, e, f] = group.matrix;
                let s = f64::from(self.x(1.0));
                let h = self.page_height_pt;
                self.content.save_state();
                let previous_alpha = self.alpha;
                self.alpha *= group.opacity.clamp(0.0, 1.0);
                if group.opacity < 1.0 {
                    let alpha = (self.alpha * 255.0).round() as u8;
                    self.alphas.insert(alpha);
                    self.content
                        .op("gs")
                        .operand(Name(format!("A{alpha}").as_bytes()));
                }
                self.content.transform([
                    a as f32,
                    -b as f32,
                    -c as f32,
                    d as f32,
                    (s * e + c * h) as f32,
                    (h * (1.0 - d) - s * f) as f32,
                ]);
                if let Some([x, y, w, height]) = group.clip {
                    self.content
                        .rect(self.x(x), self.y(y + height), self.x(w), self.x(height));
                    self.content.clip_nonzero().end_path();
                }
                if let Some(shape) = &group.clip_path {
                    if group.clip_inverse {
                        if let Some([x, y, w, h]) = group.clip {
                            self.content
                                .rect(self.x(x), self.y(y + h), self.x(w), self.x(h));
                        }
                    }
                    let segments = path::resolve(&path::parse(&shape.d), (0.0, 0.0)).0;
                    let (sin, cos) = shape.rotate_deg.to_radians().sin_cos();
                    let point = |x: f64, y: f64| {
                        let x = (x - shape.w / 2.0) * if shape.flip_h { -1.0 } else { 1.0 };
                        let y = (y - shape.h / 2.0) * if shape.flip_v { -1.0 } else { 1.0 };
                        (
                            shape.x + shape.w / 2.0 + cos * x - sin * y,
                            shape.y + shape.h / 2.0 + sin * x + cos * y,
                        )
                    };
                    for segment in segments {
                        match segment {
                            Segment::MoveTo(x, y) => {
                                let (x, y) = point(x, y);
                                self.content.move_to(self.x(x), self.y(y));
                            }
                            Segment::LineTo(x, y) => {
                                let (x, y) = point(x, y);
                                self.content.line_to(self.x(x), self.y(y));
                            }
                            Segment::CurveTo(x1, y1, x2, y2, x, y) => {
                                let (x1, y1) = point(x1, y1);
                                let (x2, y2) = point(x2, y2);
                                let (x, y) = point(x, y);
                                self.content.cubic_to(
                                    self.x(x1),
                                    self.y(y1),
                                    self.x(x2),
                                    self.y(y2),
                                    self.x(x),
                                    self.y(y),
                                );
                            }
                            Segment::Close => {
                                self.content.close_path();
                            }
                            Segment::Arc { .. } => {}
                        }
                    }
                    if group.clip_inverse {
                        self.content.clip_even_odd().end_path();
                    } else {
                        self.content.clip_nonzero().end_path();
                    }
                }
                for child in &group.items {
                    self.item(child);
                }
                self.content.restore_state();
                self.alpha = previous_alpha;
            }
            Item::Rect(rect) => self.rect(rect),
            Item::Line(line) => self.line(line),
            Item::Path(path) => self.path(path),
            Item::Image(image) => self.image(image),
            Item::Text(text) => self.text(text),
        }
    }

    /// Converts a px y measured from the top of the page to a PDF y.
    fn y(&self, y_px: f64) -> f32 {
        (self.page_height_pt - px_to_pt(y_px, self.scale)) as f32
    }

    fn x(&self, x_px: f64) -> f32 {
        px_to_pt(x_px, self.scale) as f32
    }

    fn rect(&mut self, rect: &strict_ooxml_render_svg::layout::RectItem) {
        let fill = rect.fill.as_deref().filter(|c| is_paint(c));
        if fill.is_none() && rect.stroke.is_none() {
            return;
        }
        if let Some(fill) = fill {
            let (red, green, blue) = rgb(fill);
            self.content.set_fill_rgb(red, green, blue);
        }
        if let Some(stroke) = rect.stroke.as_deref().filter(|c| is_paint(c)) {
            let (red, green, blue) = rgb(stroke);
            self.content.set_stroke_rgb(red, green, blue);
            self.content
                .set_line_width(px_to_pt(rect.stroke_w, self.scale).max(0.0) as f32);
        }
        self.content.rect(
            self.x(rect.x),
            self.y(rect.y + rect.h),
            self.x(rect.w),
            px_to_pt(rect.h, self.scale) as f32,
        );
        finish(&mut self.content, fill.is_some(), rect.stroke.is_some());
    }

    fn line(&mut self, line: &strict_ooxml_render_svg::layout::LineItem) {
        let (red, green, blue) = rgb(&line.color);
        self.content.set_stroke_rgb(red, green, blue);
        self.content
            .set_line_width(px_to_pt(line.width, self.scale).max(0.0) as f32);
        if line.dashed {
            self.content.set_dash_pattern([4.0, 3.0], 0.0);
        }
        self.content.move_to(self.x(line.x1), self.y(line.y1));
        self.content.line_to(self.x(line.x2), self.y(line.y2));
        self.content.stroke();
        if line.dashed {
            self.content.set_dash_pattern([], 0.0);
        }
    }

    fn path(&mut self, shape: &PathItem) {
        let segments = path::resolve(&path::parse(&shape.d), (0.0, 0.0)).0;
        if segments.is_empty() {
            return;
        }
        let fill = shape.fill.as_deref().filter(|c| is_paint(c));
        if fill.is_none() && shape.paint.is_none() && shape.stroke.is_none() {
            return;
        }
        // The path is in local coordinates with y pointing down; PDF's y points
        // up, so the box is scaled and flipped first and moved second, and the
        // shape's own rotation and mirror are applied about its centre in
        // between.
        let sx = if shape.w > 0.0 {
            px_to_pt(shape.w, self.scale) / shape.w
        } else {
            1.0
        };
        let sy = if shape.h > 0.0 {
            px_to_pt(shape.h, self.scale) / shape.h
        } else {
            1.0
        };
        let mut inner = Matrix::scale(sx as f32, -(sy as f32));
        if shape.rotate_deg.abs() > f64::EPSILON || shape.flip_h || shape.flip_v {
            let mut decoration = Matrix::rotate(shape.rotate_deg as f32);
            if shape.flip_h || shape.flip_v {
                decoration = Matrix::chain(
                    decoration,
                    Matrix::scale(
                        if shape.flip_h { -1.0 } else { 1.0 },
                        if shape.flip_v { -1.0 } else { 1.0 },
                    ),
                );
            }
            let (half_w, half_h) = (shape.w as f32 / 2.0, shape.h as f32 / 2.0);
            let about_centre = Matrix::chain(
                Matrix::translate(-half_w, -half_h),
                Matrix::chain(
                    decoration,
                    Matrix::chain(Matrix::translate(half_w, half_h), inner),
                ),
            );
            inner = about_centre;
        }
        // The path's local box has `y` pointing **down**, so local `y = 0` is the
        // shape's *top* edge, and the y scale is negated to match PDF. That makes
        // the translation the shape's own top edge in PDF coordinates — the same
        // `self.y(shape.y)` every other item uses. Adding `shape.h` here instead
        // drew every DrawingML shape one shape-height too low, which is not a
        // subtle drift: it put a rounded rectangle on the wrong line of the page
        // and no structural bound was looking for it until the PDF pixel gate
        // (`CORE-QUEUE.md` §1) compared the two backends.
        let matrix = Matrix::chain(inner, Matrix::translate(self.x(shape.x), self.y(shape.y)));
        self.content.save_state();
        self.content.transform(matrix.to_array());
        self.outline(&segments);
        if let Some(fill) = fill {
            let (red, green, blue) = rgb(fill);
            self.content.set_fill_rgb(red, green, blue);
        }
        if let Some(paint) = &shape.paint {
            let name = format!("Fill{}", self.paints.len());
            self.paints.push((name.clone(), paint.clone()));
            match paint {
                PathPaint::LinearGradient { .. } => {
                    self.content.save_state().clip_nonzero().end_path();
                    self.content.shading(Name(name.as_bytes())).restore_state();
                    self.outline(&segments);
                }
                PathPaint::Pattern { .. } => {
                    self.content
                        .set_fill_color_space(pdf_writer::types::ColorSpaceOperand::Pattern)
                        .set_fill_pattern([], Name(name.as_bytes()));
                }
            }
        }
        if let Some(stroke) = shape.stroke.as_deref().filter(|c| is_paint(c)) {
            let (red, green, blue) = rgb(stroke);
            self.content.set_stroke_rgb(red, green, blue);
            self.content
                .set_line_width(px_to_pt(shape.stroke_w, self.scale).max(0.0) as f32);
            if let Some(dash) = shape.dash.as_deref() {
                let array = dash_array(dash);
                if !array.is_empty() {
                    self.content.set_dash_pattern(array, 0.0);
                }
            }
        }
        let paint_fill = match &shape.paint {
            Some(PathPaint::LinearGradient { .. }) => false,
            Some(PathPaint::Pattern { .. }) => true,
            None => fill.is_some(),
        };
        finish(&mut self.content, paint_fill, shape.stroke.is_some());
        self.content.restore_state();
    }

    fn outline(&mut self, segments: &[Segment]) {
        for segment in segments {
            match *segment {
                Segment::MoveTo(x, y) => {
                    self.content.move_to(x as f32, y as f32);
                }
                Segment::LineTo(x, y) => {
                    self.content.line_to(x as f32, y as f32);
                }
                Segment::CurveTo(x1, y1, x2, y2, x, y) => {
                    self.content.cubic_to(
                        x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
                    );
                }
                // `resolve` turns every arc into cubics before anything is drawn,
                // so an arc reaching the content stream means the resolver was
                // bypassed — and PDF has no operator for it.
                Segment::Arc { .. } => {
                    self.report.record_unresolved_outline(
                        "a shape's outline still holds an elliptical arc",
                    );
                }
                Segment::Close => {
                    self.content.close_path();
                }
            }
        }
    }

    fn image(&mut self, image: &strict_ooxml_render_svg::layout::ImageItem) {
        let Some(part) = image.part.as_ref() else {
            self.report.record_placeholder(&image.alt);
            return;
        };
        if self.media.is_none() {
            self.report.record_placeholder(&image.alt);
            return;
        }
        // Images are named before drawing; one that was not degrades to a
        // placeholder instead of a dangling resource reference.
        let Some(name) = self.names.get(part).cloned() else {
            self.report.record_placeholder(&image.alt);
            return;
        };
        self.images.insert(part.clone(), name.clone());
        self.content.save_state();
        if image.crop.is_some() {
            // `a:srcRect`: clip to the item's box and draw the whole picture
            // larger behind it, as the SVG backend's viewport does.
            self.content.rect(
                self.x(image.x),
                self.y(image.y + image.h),
                self.x(image.w),
                px_to_pt(image.h, self.scale) as f32,
            );
            self.content.clip_nonzero();
            self.content.end_path();
        }
        let (x, y, w, h) = image.uncropped_box();
        self.content.transform([
            self.x(w),
            0.0,
            0.0,
            px_to_pt(h, self.scale) as f32,
            self.x(x),
            self.y(y + h),
        ]);
        self.content.x_object(Name(name.as_bytes()));
        self.content.restore_state();
    }

    fn text(&mut self, text: &TextItem) {
        if text.is_caret_anchor() {
            return;
        }
        if text.run.rotation_deg == 0.0 {
            self.decorated_text(text);
            return;
        }
        let (sin, cos) = (-text.run.rotation_deg.to_radians()).sin_cos();
        let x = f64::from(self.x(text.x));
        let y = f64::from(self.y(text.baseline));
        self.content.save_state();
        self.content.transform([
            cos as f32,
            sin as f32,
            -sin as f32,
            cos as f32,
            (x - cos * x + sin * y) as f32,
            (y - sin * x - cos * y) as f32,
        ]);
        self.decorated_text(text);
        self.content.restore_state();
    }

    /// Draws a fragment with the shading, highlight, underline and
    /// strikethrough the SVG backend draws for it, from the same geometry.
    fn decorated_text(&mut self, text: &TextItem) {
        let decorations = strict_ooxml_render_svg::layout::text_decorations(text);
        for decoration in &decorations.behind {
            self.item(decoration);
        }
        self.text_unrotated(text);
        for decoration in &decorations.over {
            self.item(decoration);
        }
    }

    fn text_unrotated(&mut self, text: &TextItem) {
        if text.text.is_empty() {
            return;
        }
        self.content.set_text_rendering_mode(if text.run.outline {
            pdf_writer::types::TextRenderingMode::Stroke
        } else {
            pdf_writer::types::TextRenderingMode::Fill
        });
        if text.run.outline {
            let (red, green, blue) = rgb(text.run.color.as_deref().unwrap_or("#000000"));
            self.content.set_stroke_rgb(red, green, blue);
            self.content
                .set_line_width(self.x((text.size_px * 0.035).max(0.5)));
        }
        // The face is matched by the *mapped* family, so the same substitution
        // the layout used decides which embedded face a run refers to, and it is
        // cloned out before anything is drawn: `fonts` is an immutable borrow of
        // `self` and the content stream is a mutable one.
        let mapped = strict_ooxml_render_svg::style::paint_family(&text.run, &text.text, false);
        let found = self
            .fonts
            .iter()
            .find(|(font, _)| {
                font.key.family == mapped
                    && font.key.bold == text.run.bold
                    && font.key.italic == text.run.italic
            })
            .map(|(font, name)| (Arc::clone(font), name.clone()));
        let Some((font, name)) = found else {
            self.report.record_missing_face(&text.run.family);
            return;
        };
        // A character with no glyph in this face is dropped rather than drawn as
        // a box: the SVG shows the fallback, and inventing a different glyph here
        // would make the two backends disagree.
        let shown = strict_ooxml_render_svg::font::present_text(&text.run.family, &text.text);
        match text.advance {
            TextAdvanceKind::Metric => {
                if self.show_metric(text, &font, &name, &shown) {
                    return;
                }
            }
            TextAdvanceKind::Shaped => {
                if let Some(shaped) =
                    strict_ooxml_render_svg::font::shape_bundled_run(&shown, &mapped, &text.run)
                {
                    if self.show_shaped(text, &font, &name, &shown, &shaped) {
                        return;
                    }
                }
            }
        }
        let mut encoded: Vec<u8> = Vec::with_capacity(text.text.len() * 2);
        let mut missing: Vec<char> = Vec::new();
        for ch in text.text.chars() {
            match font.chars.get(&ch) {
                Some(gid) => {
                    if font.notdef_chars.contains(&ch) {
                        missing.push(ch);
                    }
                    encoded.extend_from_slice(&gid.to_be_bytes());
                }
                None => missing.push(ch),
            }
        }
        for ch in missing {
            self.report.record_missing_glyph(&text.run.family, ch);
        }
        if encoded.is_empty() {
            return;
        }
        let (red, green, blue) = rgb(text.run.color.as_deref().unwrap_or("#000000"));
        let size = px_to_pt(text.size_px, self.scale) as f32;
        let x = self.x(text.x);
        let y = self.y(text.baseline);
        self.content.begin_text();
        self.content.set_fill_rgb(red, green, blue);
        self.content.set_font(Name(name.as_bytes()), size);
        self.content
            .set_text_matrix([text.run.char_scale as f32, 0.0, 0.0, 1.0, x, y]);
        self.content.show(Str(&encoded));
        self.content.end_text();
    }

    /// Draws one CID per scalar at cumulative `hmtx` origins ([`TextAdvanceKind::Metric`]).
    fn show_metric(
        &mut self,
        text: &TextItem,
        font: &Arc<EmbeddedFont>,
        name: &str,
        shown: &str,
    ) -> bool {
        if shown.is_empty() {
            return false;
        }
        let provider = builtin_provider();
        let family = strict_ooxml_render_svg::style::chosen_family(&text.run, shown);
        let (red, green, blue) = rgb(text.run.color.as_deref().unwrap_or("#000000"));
        let size = px_to_pt(text.size_px, self.scale) as f32;
        let extra = strict_ooxml_render_svg::style::spacing_px(&text.run, text.size_px);
        let mut cursor = 0.0;
        let mut drew = false;
        self.content.begin_text();
        self.content.set_fill_rgb(red, green, blue);
        self.content.set_font(Name(name.as_bytes()), size);
        let chars: Vec<_> = shown.chars().collect();
        let mut previous = None;
        for (index, &ch) in chars.iter().enumerate() {
            match font.chars.get(&ch) {
                Some(cid) => {
                    if font.notdef_chars.contains(&ch) {
                        self.report.record_missing_glyph(&text.run.family, ch);
                    }
                    let x_px = text.x + cursor * text.run.char_scale;
                    self.content.set_text_matrix([
                        text.run.char_scale as f32,
                        0.0,
                        0.0,
                        1.0,
                        self.x(x_px),
                        self.y(text.baseline),
                    ]);
                    self.content.show(Str(&cid.to_be_bytes()));
                    drew = true;
                }
                None => self.report.record_missing_glyph(&text.run.family, ch),
            }
            let next = chars.get(index + 1).copied().or(text.following_non_space);
            let factor = strict_ooxml_render_svg::style::compressed_char_factor(
                previous,
                ch,
                next,
                text.compress_punctuation,
                text.plain_space_factor,
            );
            cursor += strict_ooxml_render_svg::style::space_advance_em(
                &text.run,
                &family,
                ch,
                strict_ooxml_render_svg::font::embedded::run_advance_em(
                    &text.run, &family, ch, provider,
                ),
            ) * text.size_px
                * factor;
            if !ch.is_whitespace() {
                cursor += extra;
            }
            if !ch.is_whitespace() || factor > 0.0 {
                previous = Some(ch);
            }
        }
        self.content.end_text();
        drew
    }

    /// Draws shaped glyph ids at their visual origins. Returns false when no CID matched.
    fn show_shaped(
        &mut self,
        text: &TextItem,
        font: &Arc<EmbeddedFont>,
        name: &str,
        shown: &str,
        shaped: &strict_ooxml_render_svg::font::ShapedText,
    ) -> bool {
        if shaped.glyphs.is_empty() {
            return false;
        }
        if !text.run.outline
            && (crate::glyphs::needs_outlines(shaped) || crate::glyphs::right_to_left(shown))
        {
            return self.show_outlined(text, font, name, shown, shaped);
        }
        let (red, green, blue) = rgb(text.run.color.as_deref().unwrap_or("#000000"));
        let size = px_to_pt(text.size_px, self.scale) as f32;
        let mut drew = false;
        self.content.begin_text();
        self.content.set_fill_rgb(red, green, blue);
        self.content.set_font(Name(name.as_bytes()), size);
        let offsets = strict_ooxml_render_svg::font::shaped_cluster_offsets_px(text, shown, shaped);
        for (cluster_index, cluster) in shaped.clusters.iter().enumerate() {
            let unicode = shown
                .get(cluster.byte_start..cluster.byte_end)
                .unwrap_or("");
            let cluster_extra = offsets.get(cluster_index).copied().unwrap_or(0.0);
            let end = cluster.glyph_start.saturating_add(cluster.glyph_count);
            let Some(glyphs) = shaped.glyphs.get(cluster.glyph_start..end) else {
                continue;
            };
            let labels = crate::glyphs::cluster_labels(glyphs, unicode);
            for (index, glyph) in glyphs.iter().enumerate() {
                if glyph.glyph_id == 0 {
                    // Drawn as `.notdef` with the text it stands for.
                    for ch in unicode.chars() {
                        self.report.record_missing_glyph(&text.run.family, ch);
                    }
                }
                let label = labels.get(index).map_or("", String::as_str);
                let cid = font
                    .cluster_cids
                    .get(&(glyph.glyph_id, label.to_owned()))
                    .copied()
                    .or_else(|| {
                        unicode
                            .chars()
                            .next()
                            .and_then(|ch| font.chars.get(&ch).copied())
                    });
                let Some(cid) = cid else {
                    continue;
                };
                let x_px =
                    text.x + (glyph.x_em * text.size_px + cluster_extra) * text.run.char_scale;
                let y_px = text.baseline - glyph.y_offset_em * text.size_px;
                self.content.set_text_matrix([
                    text.run.char_scale as f32,
                    0.0,
                    0.0,
                    1.0,
                    self.x(x_px),
                    self.y(y_px),
                ]);
                let bytes = cid.to_be_bytes();
                self.content.show(Str(&bytes));
                drew = true;
            }
        }
        self.content.end_text();
        drew
    }

    /// Draws a shaped run whose glyphs a text layer cannot carry one per
    /// character (`glyphs::needs_outlines`): the glyphs as outline forms, the
    /// text as one invisible `TJ` on the baseline, one glyph per cluster.
    fn show_outlined(
        &mut self,
        text: &TextItem,
        font: &Arc<EmbeddedFont>,
        name: &str,
        shown: &str,
        shaped: &strict_ooxml_render_svg::font::ShapedText,
    ) -> bool {
        let size_pt = px_to_pt(text.size_px, self.scale);
        if size_pt <= 0.0 {
            return false;
        }
        let scale_x = text.run.char_scale;
        let offsets = strict_ooxml_render_svg::font::shaped_cluster_offsets_px(text, shown, shaped);
        let mut placed: Vec<PlacedGlyph> = Vec::new();
        for (cluster_index, cluster) in shaped.clusters.iter().enumerate() {
            let unicode = shown
                .get(cluster.byte_start..cluster.byte_end)
                .unwrap_or("");
            let cluster_extra = offsets.get(cluster_index).copied().unwrap_or(0.0);
            let end = cluster.glyph_start.saturating_add(cluster.glyph_count);
            let Some(glyphs) = shaped.glyphs.get(cluster.glyph_start..end) else {
                continue;
            };
            let labels = crate::glyphs::cluster_labels(glyphs, unicode);
            for (index, glyph) in glyphs.iter().enumerate() {
                if glyph.glyph_id == 0 {
                    // Drawn as `.notdef` with the text it stands for.
                    for ch in unicode.chars() {
                        self.report.record_missing_glyph(&text.run.family, ch);
                    }
                }
                let label = labels.get(index).map_or("", String::as_str);
                let Some(&cid) = font.cluster_cids.get(&(glyph.glyph_id, label.to_owned())) else {
                    continue;
                };
                // The letters of a right-to-left ligature, each with a code of
                // its own, in visual order: the text layer writes them in
                // place of the ligature (`glyphs::rtl_ligature`).
                let letters = crate::glyphs::rtl_ligature(label).and_then(|letters| {
                    letters
                        .iter()
                        .rev()
                        .map(|ch| font.chars.get(ch).copied())
                        .collect::<Option<Vec<u16>>>()
                });
                placed.push(PlacedGlyph {
                    visual: cluster.glyph_start.saturating_add(index),
                    logical: cluster.byte_start,
                    cid,
                    letters,
                    offset_px: glyph.x_em * text.size_px + cluster_extra,
                    rise_px: glyph.y_offset_em * text.size_px,
                    carries_text: !label.is_empty(),
                    spacing: glyph.x_advance_em.abs() > 1e-9,
                });
            }
        }
        if placed.is_empty() {
            return false;
        }
        placed.sort_by_key(|glyph| glyph.visual);

        let (red, green, blue) = rgb(text.run.color.as_deref().unwrap_or("#000000"));
        self.content.set_fill_rgb(red, green, blue);
        for glyph in &placed {
            let Some(form) = self.glyph_forms.name(font, glyph.cid).map(str::to_owned) else {
                continue;
            };
            let x = self.x(text.x + glyph.offset_px * scale_x);
            let y = self.y(text.baseline - glyph.rise_px);
            self.content.save_state();
            self.content
                .transform([(size_pt * scale_x) as f32, 0.0, 0.0, size_pt as f32, x, y]);
            self.content.x_object(Name(form.as_bytes()));
            self.content.restore_state();
            self.glyph_names.insert(form);
        }
        self.text_layer(text, font, name, size_pt, &placed);
        true
    }

    /// The invisible text of an outlined run: the glyphs that carry a
    /// cluster's text, in visual order on the baseline. Pen offsets are in em
    /// of the font size, before the text matrix applies the horizontal scale.
    fn text_layer(
        &mut self,
        text: &TextItem,
        font: &EmbeddedFont,
        name: &str,
        size_pt: f64,
        placed: &[PlacedGlyph],
    ) {
        let scale_x = text.run.char_scale;
        self.content.begin_text();
        self.content
            .set_text_rendering_mode(pdf_writer::types::TextRenderingMode::Invisible);
        self.content.set_font(Name(name.as_bytes()), size_pt as f32);
        self.content.set_text_matrix([
            scale_x as f32,
            0.0,
            0.0,
            1.0,
            self.x(text.x),
            self.y(text.baseline),
        ]);
        {
            let mut shown_text = self.content.show_positioned();
            let mut items = shown_text.items();
            let mut pen = 0.0_f64;
            for glyph in placed.iter().filter(|glyph| glyph.carries_text) {
                let cid = glyph.cid;
                let at = px_to_pt(mark_offset_px(glyph, placed), self.scale) / size_pt;
                let gap = at - pen;
                if gap.abs() > 1e-6 {
                    items.adjust((-gap * 1000.0) as f32);
                }
                let codes = glyph.letters.clone().unwrap_or_else(|| vec![cid]);
                pen = at;
                for code in codes {
                    let bytes = code.to_be_bytes();
                    items.show(Str(&bytes));
                    pen += f64::from(*font.widths.get(&code).unwrap_or(&0)) / 1000.0;
                }
            }
        }
        self.content.end_text();
        self.content
            .set_text_rendering_mode(pdf_writer::types::TextRenderingMode::Fill);
    }
}

/// Where the text layer puts a glyph: a mark (no advance) at the start of
/// its letter — its own cluster's letter when the shaper merged the two,
/// otherwise the logically previous spacing glyph that carries text.
///
/// At its drawn place, over the middle of the letter, it left a gap that
/// pdfium read as a space inside the word (`مَ نْ`).
fn mark_offset_px(glyph: &PlacedGlyph, placed: &[PlacedGlyph]) -> f64 {
    if glyph.spacing {
        return glyph.offset_px;
    }
    placed
        .iter()
        .filter(|base| base.carries_text && base.spacing && base.logical <= glyph.logical)
        .max_by_key(|base| base.logical)
        .map_or(glyph.offset_px, |base| base.offset_px)
}

/// One glyph of an outlined run, as `show_outlined` places it.
struct PlacedGlyph {
    /// Index in the run's visual glyph order.
    visual: usize,
    /// Byte offset of its cluster in the run's text (logical order).
    logical: usize,
    cid: u16,
    /// Pen offset from the run's origin, in px before the horizontal scale.
    offset_px: f64,
    /// Offset above the baseline, in px.
    rise_px: f64,
    /// The codes the text layer writes for a right-to-left ligature, in
    /// visual order; `None` writes [`Self::cid`].
    letters: Option<Vec<u16>>,
    /// Whether this glyph carries text in the text layer.
    carries_text: bool,
    /// Whether the glyph advances the pen (a mark does not).
    spacing: bool,
}

/// Closes a path with the painting operators the flags ask for.
fn finish(content: &mut Content, fill: bool, stroke: bool) {
    match (fill, stroke) {
        (true, true) => {
            content.fill_nonzero_and_stroke();
        }
        (true, false) => {
            content.fill_nonzero();
        }
        (false, true) => {
            content.stroke();
        }
        (false, false) => {
            content.end_path();
        }
    }
}

/// Every picture in the document, named once, before anything is written.
///
/// The base name comes from the part, because it is what makes a content stream
/// readable (`/Im_word_media_image1_png Do` says which picture is drawn) and
/// because the same part on two pages must be one object. Cleaning a part name
/// into a resource name is lossy, though: `/word/media/a-b.png` and
/// `/word/media/a_b.png` are two pictures and one name, and a page's `/XObject`
/// dictionary written with both is a dictionary with two entries under one key —
/// well formed enough to open, and one picture short.
///
/// So a collision gets a counter, in first-seen order. First-seen rather than
/// sorted because the walk that draws the pages is the one that establishes which
/// picture came first, and a name that depends on nothing but the document is
/// what keeps SC-1 (two renders, the same bytes) true.
fn name_the_images<'a>(parts: impl Iterator<Item = &'a PartId>) -> BTreeMap<PartId, String> {
    let mut taken: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut names: BTreeMap<PartId, String> = BTreeMap::new();
    for part in parts {
        if names.contains_key(part) {
            continue;
        }
        let base = image_resource_name(part);
        let mut name = base.clone();
        let mut counter = 1u32;
        while taken.contains(&name) {
            counter += 1;
            name = format!("{base}_{counter}");
        }
        taken.insert(name.clone());
        names.insert(part.clone(), name);
    }
    names
}

/// The resource name an image would have, derived from its part.
///
/// Kept separate from the uniqueness decision above: this is the part, cleaned,
/// and the collision test is about what happens when two parts clean to the same
/// thing.
fn image_resource_name(part: &PartId) -> String {
    let mut out = String::from("Im");
    for ch in part.as_str().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

/// Writes an image XObject and, when the image has one, its soft mask.
fn write_image(pdf: &mut Pdf, id: Ref, mask: Option<Ref>, encoded: &Encoded) {
    match encoded {
        Encoded::Jpeg {
            width,
            height,
            data,
            color_space,
            invert_cmyk,
        } => {
            let mut image = pdf.image_xobject(id, data.as_slice());
            image.filter(Filter::DctDecode);
            image.width(i32::try_from(*width).unwrap_or(i32::MAX));
            image.height(i32::try_from(*height).unwrap_or(i32::MAX));
            image.color_space_name(Name(color_space.pdf_name()));
            image.bits_per_component(8);
            // AUD-81: Adobe YCCK (`transform = 2`) stores inverted CMYK.
            if *invert_cmyk {
                image.decode([1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
            }
        }
        Encoded::Raw {
            width,
            height,
            samples,
            components,
            alpha,
        } => {
            let space = if *components == 1 {
                Name(b"DeviceGray")
            } else {
                Name(b"DeviceRGB")
            };
            let width = i32::try_from(*width).unwrap_or(i32::MAX);
            let height = i32::try_from(*height).unwrap_or(i32::MAX);
            {
                let (data, compressed) = flate(samples);
                let mut image = pdf.image_xobject(id, &data);
                if compressed {
                    image.filter(Filter::FlateDecode);
                }
                image.width(width);
                image.height(height);
                image.color_space_name(space);
                image.bits_per_component(8);
                if let Some(mask) = mask {
                    image.s_mask(mask);
                }
            }
            if let (Some(mask), Some(alpha)) = (mask, alpha.as_ref()) {
                let (data, compressed) = flate(alpha);
                let mut image = pdf.image_xobject(mask, &data);
                if compressed {
                    image.filter(Filter::FlateDecode);
                }
                image.width(width);
                image.height(height);
                image.color_space_name(Name(b"DeviceGray"));
                image.bits_per_component(8);
            }
        }
    }
}

/// Writes a CID font: the program, its descriptor, the Unicode CMap and the
/// `Type0` wrapper that ties them together.
#[allow(clippy::too_many_lines)]
fn write_font(pdf: &mut Pdf, ids: FontIds, font: &EmbeddedFont) {
    let FontIds {
        type0: type0_id,
        file: file_id,
        descriptor: descriptor_id,
        cid: cid_id,
        to_unicode: to_unicode_id,
        cid_to_gid: cid_to_gid_id,
    } = ids;
    // A PDF name, so the base font is a PostScript-style identifier with no
    // spaces; the family goes in the descriptor as a string, where spaces are
    // legal.
    let family = font.key.base_font();
    let mut name_bytes = family.as_bytes().to_vec();
    for ch in &mut name_bytes {
        if !ch.is_ascii_alphanumeric() && *ch != b'-' && *ch != b'+' {
            *ch = b'_';
        }
    }
    let mut owned = name_bytes;
    if owned.first().is_some_and(u8::is_ascii_digit) {
        owned.insert(0, b'_');
    }
    let postscript_name = Name(&owned);

    {
        let (data, compressed) = flate(&font.data);
        let mut stream = pdf.stream(file_id, &data);
        if compressed {
            stream.filter(Filter::FlateDecode);
        }
        if font.is_cff {
            // A CFF program in a TrueType wrapper: the subtype tells a reader to
            // expect OpenType/CFF outlines rather than `glyf`.
            stream.pair(Name(b"Subtype"), Name(b"OpenType"));
        }
    }
    {
        let mut descriptor = pdf.font_descriptor(descriptor_id);
        descriptor.name(postscript_name);
        descriptor.family(Str(family.as_bytes()));
        let [x_min, y_min, x_max, y_max] = font.bounds;
        descriptor.bbox(Rect::new(
            x_min as f32,
            y_min as f32,
            x_max as f32,
            y_max as f32,
        ));
        descriptor.flags(if font.key.italic {
            FontFlags::ITALIC
        } else {
            FontFlags::empty()
        });
        descriptor.italic_angle(if font.key.italic { -12.0 } else { 0.0 });
        descriptor.weight(if font.key.bold { 700 } else { 400 });
        descriptor.stem_v(if font.key.bold { 120.0 } else { 80.0 });
        descriptor.ascent(font.ascent as f32);
        descriptor.descent(font.descent as f32);
        if let Some(cap_height) = font.cap_height {
            descriptor.cap_height(cap_height as f32);
        }
        if font.is_cff {
            descriptor.font_file3(file_id);
        } else {
            descriptor.font_file2(file_id);
        }
    }
    {
        let (data, compressed) = flate(&crate::font::to_unicode_cmap(font));
        let mut stream = pdf.stream(to_unicode_id, &data);
        if compressed {
            stream.filter(Filter::FlateDecode);
        }
    }
    {
        // AUD-82: CID ≠ GID when two characters share an outline.
        let (data, compressed) = flate(&crate::font::cid_to_gid_bytes(font));
        let mut stream = pdf.stream(cid_to_gid_id, &data);
        if compressed {
            stream.filter(Filter::FlateDecode);
        }
    }
    {
        let mut cid = pdf.cid_font(cid_id);
        cid.subtype(if font.is_cff {
            pdf_writer::types::CidFontType::Type0
        } else {
            pdf_writer::types::CidFontType::Type2
        });
        cid.base_font(postscript_name);
        cid.system_info(pdf_writer::types::SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        });
        cid.font_descriptor(descriptor_id);
        cid.default_width(0.0);
        // The `/W` array is written in runs of CIDs: consecutive CIDs with the
        // same width share one range. The runs are collected before writing
        // because `widths()` borrows the writer, and the widths are integers, so
        // the run test is exact.
        let mut runs: Vec<(u16, u16, u16)> = Vec::new();
        for (cid_code, width) in &font.widths {
            match runs.last_mut() {
                Some((_, last, last_width))
                    if *cid_code == last.saturating_add(1) && *last_width == *width =>
                {
                    *last = *cid_code;
                }
                _ => runs.push((*cid_code, *cid_code, *width)),
            }
        }
        {
            let mut widths = cid.widths();
            for (start, last, width) in runs {
                let _ = widths.same(start, last, f32::from(width));
            }
        }
        cid.cid_to_gid_map_stream(cid_to_gid_id);
    }
    {
        let mut type0 = pdf.type0_font(type0_id);
        type0.base_font(postscript_name);
        // The predefined encoding gives readers the two-byte CID code space.
        // A bare CMap stream without its mandatory dictionary entries was
        // interpreted as a single-byte encoding by independent extractors.
        type0.encoding_predefined(Name(b"Identity-H"));
        type0.descendant_font(cid_id);
        type0.to_unicode(to_unicode_id);
    }
}

#[cfg(test)]
mod tests {
    use super::{image_resource_name, name_the_images};
    use std::collections::BTreeMap;
    use strict_ooxml_core::part::PartId;

    fn ids(parts: &[&str]) -> Vec<PartId> {
        parts.iter().map(|part| PartId::new(*part)).collect()
    }

    fn names_for(parts: &[&str]) -> BTreeMap<PartId, String> {
        let parts = ids(parts);
        name_the_images(parts.iter())
    }

    #[test]
    fn a_part_keeps_its_readable_name() {
        let names = names_for(&["/word/media/image1.png"]);
        assert_eq!(
            names[&PartId::new("/word/media/image1.png")],
            "Im_word_media_image1_png"
        );
    }

    #[test]
    fn two_parts_that_clean_alike_get_different_names() {
        // The collision `STAGE-8-OPEN.md` Q-4 predicted: cleaning a part name
        // into a PDF resource name is lossy, and these two files are one name.
        // Written as they were, the page's `/XObject` dictionary holds two
        // entries under one key and one of the pictures is never drawn.
        let names = names_for(&["/word/media/a-b.png", "/word/media/a_b.png"]);
        assert_eq!(names.len(), 2);
        let values: Vec<&String> = names.values().collect();
        assert_ne!(values[0], values[1], "the collision survived: {names:?}");
        assert_eq!(values[0], "Im_word_media_a_b_png");
        assert_eq!(values[1], "Im_word_media_a_b_png_2");
    }

    #[test]
    fn the_first_part_to_arrive_keeps_the_unsuffixed_name() {
        // First-seen order is the contract, because the walk that draws the pages
        // is what fixes it and a name that depends on nothing but the document is
        // what keeps two renders byte-identical (SC-1).
        let names = names_for(&["/word/media/a_b.png", "/word/media/a-b.png"]);
        assert_eq!(
            names[&PartId::new("/word/media/a_b.png")],
            "Im_word_media_a_b_png"
        );
        assert_eq!(
            names[&PartId::new("/word/media/a-b.png")],
            "Im_word_media_a_b_png_2"
        );
    }

    #[test]
    fn the_same_part_repeated_is_one_picture_with_one_name() {
        let names = names_for(&[
            "/word/media/image1.png",
            "/word/media/image1.png",
            "/word/media/image1.png",
        ]);
        assert_eq!(names.len(), 1, "a repeat is the same picture: {names:?}");
    }

    #[test]
    fn three_parts_that_clean_alike_all_get_a_name() {
        let names = names_for(&[
            "/word/media/a b.png",
            "/word/media/a-b.png",
            "/word/media/a.b.png",
        ]);
        let mut values: Vec<&String> = names.values().collect();
        values.sort();
        assert_eq!(values.len(), 3);
        assert_eq!(
            values,
            vec![
                "Im_word_media_a_b_png",
                "Im_word_media_a_b_png_2",
                "Im_word_media_a_b_png_3"
            ]
        );
    }

    #[test]
    fn a_part_name_is_only_cleaned_never_reordered() {
        // The name is derived, not generated: a reader of a content stream has to
        // be able to tell which picture `/Im… Do` draws.
        assert_eq!(
            image_resource_name(&PartId::new("/word/media/i'm a ~pic~.PNG")),
            "Im_word_media_i_m_a__pic__PNG"
        );
    }
}
