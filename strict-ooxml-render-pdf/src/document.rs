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
use strict_ooxml_core::error::{Result, StrictError};
use strict_ooxml_core::part::PartId;
use strict_ooxml_render_svg::layout::{Item, PathItem, PlacedPage, TextAdvanceKind, TextItem};
use strict_ooxml_render_svg::{FontProvider, MediaSource, RenderOptions};

use crate::font::{EmbeddedFont, FaceCollector, FaceKey};
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
    report: &'a mut PdfReport,
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
    let image_names = name_the_images(pages.iter().flat_map(|page| page.items.iter()).filter_map(
        |item| match item {
            Item::Image(image) => image.part.as_ref(),
            _ => None,
        },
    ));

    // Pass one: content streams and the images each page needs.
    let mut streams: Vec<(Vec<u8>, BTreeMap<PartId, String>)> = Vec::with_capacity(pages.len());
    for page in pages {
        let mut writer = PageWriter {
            content: Content::new(),
            page_height_pt: px_to_pt(page.height_px, scale),
            scale,
            fonts: &fonts,
            media,
            names: &image_names,
            images: BTreeMap::new(),
            report: &mut report,
        };
        writer.draw_page(page, options.background);
        streams.push((writer.content.finish().into_vec(), writer.images));
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
    for (_, images) in &streams {
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
            // The uncompressed budget is the package's own part ceiling: a PNG
            // cannot expand past what a part of the same document would be
            // allowed to hold (AUD-14).
            let encoded = match encode_with_limit(
                part.as_str(),
                &bytes,
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
        });
    }
    // A CID font needs seven objects: the program, its descriptor, the CID font,
    // the ToUnicode CMap, the Identity-H CMap, the CIDToGIDMap stream and the
    // Type0 wrapper.
    let font_ids: Vec<FontIds> = fonts.iter().map(|_| builder.allocate_seven()).collect();

    let mut pdf = Pdf::new();
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
    for (index, page) in pages.iter().enumerate() {
        let objects = &page_objects[index];
        let width_pt = px_to_pt(page.width_px, scale) as f32;
        let height_pt = px_to_pt(page.height_px, scale) as f32;
        {
            let (data, compressed) = flate(&streams[index].0);
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
                let mut fonts_dict = resources.fonts();
                for ((_, name), ids) in fonts.iter().zip(font_ids.iter()) {
                    fonts_dict.pair(Name(name.as_bytes()), ids.type0);
                }
            }
            {
                let mut xobjects = resources.x_objects();
                for (part, (id, _)) in &objects.images {
                    let name = image_names.get(part).unwrap_or_else(|| {
                        panic!("{} was named before it was drawn", part.as_str())
                    });
                    xobjects.pair(Name(name.as_bytes()), *id);
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

/// The ids one page needs.
struct PageObjects {
    /// Its content stream.
    content: Ref,
    /// The page object itself.
    page: Ref,
    /// Image id and optional mask id, by media part.
    images: BTreeMap<PartId, (Ref, Option<Ref>)>,
}

/// The object ids a CID font occupies (seven: Type0, program, descriptor, CID
/// font, ToUnicode, Identity-H, and the `/CIDToGIDMap` stream).
#[derive(Clone, Copy, Debug)]
struct FontIds {
    type0: Ref,
    file: Ref,
    descriptor: Ref,
    cid: Ref,
    to_unicode: Ref,
    identity: Ref,
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

    /// Hands out seven consecutive ids, in the order `write_font` consumes them.
    fn allocate_seven(&mut self) -> FontIds {
        FontIds {
            type0: self.allocate(),
            file: self.allocate(),
            descriptor: self.allocate(),
            cid: self.allocate(),
            to_unicode: self.allocate(),
            identity: self.allocate(),
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
    for page in pages {
        for item in &page.items {
            let Item::Text(text) = item else {
                continue;
            };
            let shown = strict_ooxml_render_svg::font::present_text(&text.run.family, &text.text);
            let Some(source) = strict_ooxml_render_svg::font::face_source(
                &text.run.family,
                text.run.bold,
                text.run.italic,
            ) else {
                report.record_missing_face(&text.run.family);
                continue;
            };
            if text.advance == TextAdvanceKind::Metric {
                // Metric paint addresses scalar CIDs, not shaped clusters. A
                // ligature-only subset cannot serve its individual characters.
                for ch in shown.chars() {
                    collector.add(&source, text.run.bold, text.run.italic, ch);
                }
                continue;
            }
            if let Some(shaped) = strict_ooxml_render_svg::font::shape_bundled(
                &shown,
                &text.run.family,
                text.run.bold,
                text.run.italic,
            ) {
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
                    for (index, glyph) in glyphs.iter().enumerate() {
                        let label = if index == 0 { unicode } else { "" };
                        collector.add_cluster(
                            &source,
                            text.run.bold,
                            text.run.italic,
                            glyph.glyph_id,
                            label,
                        );
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
        let Some(source) =
            strict_ooxml_render_svg::font::face_source(key.family, key.bold, key.italic)
        else {
            report.record_missing_face(key.family);
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
            match item {
                Item::Rect(rect) => self.rect(rect),
                Item::Line(line) => self.line(line),
                Item::Path(path) => self.path(path),
                Item::Image(image) => self.image(image),
                Item::Text(text) => self.text(text),
            }
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
        if fill.is_none() && shape.stroke.is_none() {
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
        for segment in &segments {
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
        if let Some(fill) = fill {
            let (red, green, blue) = rgb(fill);
            self.content.set_fill_rgb(red, green, blue);
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
        finish(&mut self.content, fill.is_some(), shape.stroke.is_some());
        self.content.restore_state();
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
        let name = self
            .names
            .get(part)
            .unwrap_or_else(|| panic!("{} was named before it was drawn", part.as_str()))
            .clone();
        self.images.insert(part.clone(), name.clone());
        self.content.save_state();
        self.content.transform([
            self.x(image.w),
            0.0,
            0.0,
            px_to_pt(image.h, self.scale) as f32,
            self.x(image.x),
            self.y(image.y + image.h),
        ]);
        self.content.x_object(Name(name.as_bytes()));
        self.content.restore_state();
    }

    fn text(&mut self, text: &TextItem) {
        if text.text.is_empty() {
            return;
        }
        // The face is matched by the *mapped* family, so the same substitution
        // the layout used decides which embedded face a run refers to, and it is
        // cloned out before anything is drawn: `fonts` is an immutable borrow of
        // `self` and the content stream is a mutable one.
        let mapped = strict_ooxml_render_svg::font::map_family(&text.run.family);
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
                if let Some(shaped) = strict_ooxml_render_svg::font::shape_bundled(
                    &shown,
                    &text.run.family,
                    text.run.bold,
                    text.run.italic,
                ) {
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
                Some(gid) => encoded.extend_from_slice(&gid.to_be_bytes()),
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
        self.content.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, y]);
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
        for ch in shown.chars() {
            match font.chars.get(&ch) {
                Some(cid) => {
                    let x_px = text.x + cursor;
                    self.content.set_text_matrix([
                        1.0,
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
            cursor +=
                provider.advance_em(&family, ch, text.run.bold, text.run.italic) * text.size_px;
            if !ch.is_whitespace() {
                cursor += extra;
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
        let (red, green, blue) = rgb(text.run.color.as_deref().unwrap_or("#000000"));
        let size = px_to_pt(text.size_px, self.scale) as f32;
        let mut drew = false;
        self.content.begin_text();
        self.content.set_fill_rgb(red, green, blue);
        self.content.set_font(Name(name.as_bytes()), size);
        let extra = strict_ooxml_render_svg::style::spacing_px(&text.run, text.size_px);
        let mut non_space_before = 0usize;
        for cluster in &shaped.clusters {
            let unicode = shown
                .get(cluster.byte_start..cluster.byte_end)
                .unwrap_or("");
            let cluster_extra = extra * non_space_before as f64;
            non_space_before += unicode.chars().filter(|ch| !ch.is_whitespace()).count();
            let end = cluster.glyph_start.saturating_add(cluster.glyph_count);
            let Some(glyphs) = shaped.glyphs.get(cluster.glyph_start..end) else {
                continue;
            };
            for (index, glyph) in glyphs.iter().enumerate() {
                if glyph.glyph_id == 0 {
                    if let Some(ch) = unicode.chars().next() {
                        self.report.record_missing_glyph(&text.run.family, ch);
                    }
                    continue;
                }
                let label = if index == 0 { unicode } else { "" };
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
                let x_px = text.x + glyph.x_em * text.size_px + cluster_extra;
                let y_px = text.baseline - glyph.y_offset_em * text.size_px;
                self.content
                    .set_text_matrix([1.0, 0.0, 0.0, 1.0, self.x(x_px), self.y(y_px)]);
                let bytes = cid.to_be_bytes();
                self.content.show(Str(&bytes));
                drew = true;
            }
        }
        self.content.end_text();
        drew
    }
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

/// Writes a CID font: the program, its descriptor, the two CMaps and the
/// `Type0` wrapper that ties them together.
#[allow(clippy::too_many_lines)]
fn write_font(pdf: &mut Pdf, ids: FontIds, font: &EmbeddedFont) {
    let FontIds {
        type0: type0_id,
        file: file_id,
        descriptor: descriptor_id,
        cid: cid_id,
        to_unicode: to_unicode_id,
        identity: identity_id,
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
        let (data, compressed) = flate(&crate::font::identity_h_cmap());
        let mut stream = pdf.stream(identity_id, &data);
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
        type0.encoding_cmap(identity_id);
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
