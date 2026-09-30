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
//! the contract and is written down in [`PdfBuilder::allocate`].

use std::collections::BTreeMap;
use std::sync::Arc;

use pdf_writer::types::FontFlags;
use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Str};
use strict_ooxml_core::error::{Result, StrictError};
use strict_ooxml_core::part::PartId;
use strict_ooxml_render_svg::layout::{Item, PathItem, PlacedPage, TextItem};
use strict_ooxml_render_svg::{MediaSource, RenderOptions};

use crate::font::{EmbeddedFont, FaceCollector, FaceKey};
use crate::image::{encode, Encoded};
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
    images: BTreeMap<PartId, String>,
    report: &'a mut PdfReport,
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

    // Pass one: content streams and the images each page needs.
    let mut streams: Vec<(Vec<u8>, BTreeMap<PartId, String>)> = Vec::with_capacity(pages.len());
    for page in pages {
        let mut writer = PageWriter {
            content: Content::new(),
            page_height_pt: px_to_pt(page.height_px, scale),
            scale,
            fonts: &fonts,
            media,
            images: BTreeMap::new(),
            report: &mut report,
        };
        writer.draw_page(page, options.background);
        streams.push((writer.content.finish().into_vec(), writer.images));
    }

    // Pass two: the file, with ids handed out in a fixed order.
    let mut builder = PdfBuilder::new();
    let mut image_refs: BTreeMap<PartId, Ref> = BTreeMap::new();
    let mut page_objects: Vec<PageObjects> = Vec::with_capacity(pages.len());
    for (_, images) in &streams {
        let content_id = builder.allocate();
        let mut image_ids: BTreeMap<PartId, (Ref, Option<Ref>)> = BTreeMap::new();
        for part in images.keys() {
            if image_refs.contains_key(part) {
                continue;
            }
            let Some(bytes) = read_media(media, part)? else {
                continue;
            };
            let encoded = match encode(part.as_str(), &bytes) {
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
            image_refs.insert(part.clone(), id);
            image_ids.insert(part.clone(), (id, mask));
            builder.pending_images.push((id, mask, encoded));
        }
        page_objects.push(PageObjects {
            content: content_id,
            page: builder.allocate(),
            images: image_ids,
        });
    }
    // A CID font needs six objects: the program, its descriptor, the CID font,
    // the ToUnicode CMap, the Identity-H CMap and the Type0 wrapper.
    let font_ids: Vec<FontIds> = fonts.iter().map(|_| builder.allocate_six()).collect();

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
                    xobjects.pair(Name(image_resource_name(part).as_bytes()), *id);
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

/// The six object ids a CID font occupies.
#[derive(Clone, Copy, Debug)]
struct FontIds {
    type0: Ref,
    file: Ref,
    descriptor: Ref,
    cid: Ref,
    to_unicode: Ref,
    identity: Ref,
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
            identity: self.allocate(),
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
            for ch in text.text.chars() {
                match strict_ooxml_render_svg::font::face_source(
                    &text.run.family,
                    text.run.bold,
                    text.run.italic,
                ) {
                    Some(source) => {
                        collector.add(&source, text.run.bold, text.run.italic, ch);
                    }
                    None => report.record_missing_face(&text.run.family),
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
        let matrix = Matrix::chain(
            inner,
            Matrix::translate(self.x(shape.x), self.y(shape.y + shape.h)),
        );
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
        let name = image_resource_name(part);
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

/// The resource name of an image, derived from its part so the same picture on
/// two pages is one object.
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
        } => {
            let mut image = pdf.image_xobject(id, data.as_slice());
            image.filter(Filter::DctDecode);
            image.width(i32::try_from(*width).unwrap_or(i32::MAX));
            image.height(i32::try_from(*height).unwrap_or(i32::MAX));
            image.color_space_name(Name(b"DeviceRGB"));
            image.bits_per_component(8);
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
        let mut cid = pdf.cid_font(cid_id);
        cid.subtype(if font.is_cff {
            pdf_writer::types::CidFontType::Type0
        } else {
            pdf_writer::types::CidFontType::Type2
        });
        cid.base_font(postscript_name);
        cid.font_descriptor(descriptor_id);
        cid.default_width(0.0);
        // The `/W` array is written in runs: a document's glyphs are mostly
        // consecutive, and one range per run keeps it short. The runs are
        // collected before writing because `widths()` borrows the writer, and the
        // widths are integers, so the run test is exact.
        let mut runs: Vec<(u16, u16, u16)> = Vec::new();
        for (gid, width) in &font.widths {
            match runs.last_mut() {
                Some((_, last, last_width))
                    if *gid == last.saturating_add(1) && *last_width == *width =>
                {
                    *last = *gid;
                }
                _ => runs.push((*gid, *gid, *width)),
            }
        }
        {
            let mut widths = cid.widths();
            for (start, last, width) in runs {
                let _ = widths.same(start, last, f32::from(width));
            }
        }
        cid.cid_to_gid_map_predefined(Name(b"Identity"));
    }
    {
        let mut type0 = pdf.type0_font(type0_id);
        type0.base_font(postscript_name);
        type0.encoding_cmap(identity_id);
        type0.descendant_font(cid_id);
        type0.to_unicode(to_unicode_id);
    }
}
