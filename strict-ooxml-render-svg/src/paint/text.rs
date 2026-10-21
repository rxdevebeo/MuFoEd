//! `<text>` emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::coord;
use crate::font::{shape_bundled_run, unicode_x_positions_px, BuiltinFontProvider, FontProvider};
use crate::layout::{TextAdvanceKind, TextItem};

/// Outline edge using the same scalar positions and face as SVG paint.
pub(crate) fn text_ink_right(item: &TextItem) -> Option<f64> {
    thread_local! {
        static FONTS: BuiltinFontProvider = BuiltinFontProvider::new();
    }
    let shown = crate::font::present_text(&item.run.family, &item.text);
    let family = crate::style::chosen_family(&item.run, &item.text);
    let positions = shaped_x_attribute(&shown, &family, item);
    FONTS.with(|fonts| {
        shown
            .chars()
            .zip(positions.split_whitespace())
            .filter_map(|(ch, x)| {
                let x = x.parse::<f64>().ok()?;
                let right = fonts.ink_right_em(&family, ch, item.run.bold, item.run.italic)?;
                Some(item.x + (x - item.x + right * item.size_px) * item.run.char_scale)
            })
            .max_by(f64::total_cmp)
    })
}

/// Writes one text fragment with its shading, highlight, underline and
/// strikethrough.
pub(crate) fn text_svg(out: &mut String, item: &TextItem, mark_off_page: bool) {
    if item.is_caret_anchor() {
        return;
    }
    if item.run.rotation_deg != 0.0 {
        let _ = writeln!(
            out,
            "<g transform=\"rotate({} {} {})\">",
            coord(item.run.rotation_deg),
            coord(item.x),
            coord(item.baseline)
        );
    }
    // Decorations are geometry shared with the PDF backend, rotated with the
    // text they belong to.
    let decorations = crate::layout::text_decorations(item);
    for decoration in &decorations.behind {
        super::item_svg(out, decoration, mark_off_page);
    }
    text_svg_unrotated(out, item, mark_off_page);
    for decoration in &decorations.over {
        super::item_svg(out, decoration, mark_off_page);
    }
    if item.run.rotation_deg != 0.0 {
        out.push_str("</g>\n");
    }
}

fn text_svg_unrotated(out: &mut String, item: &TextItem, mark_off_page: bool) {
    if (item.run.char_scale - 1.0).abs() > 1e-9 {
        let _ = writeln!(
            out,
            "<g transform=\"translate({} 0) scale({} 1) translate({} 0)\">",
            coord(item.x),
            coord(item.run.char_scale),
            coord(-item.x)
        );
    }
    let shown = crate::font::present_text(&item.run.family, &item.text);
    let family = crate::style::chosen_family(&item.run, &item.text);
    // The face that draws; `family` keeps the measured positions.
    let painted = super::svg_family(
        &item.run,
        crate::style::paint_family(&item.run, &item.text, true),
    );
    let fill = item
        .run
        .color
        .clone()
        .unwrap_or_else(|| "#000000".to_owned());
    let mut attributes = String::new();
    attributes.push_str("font-family=\"");
    attributes.push_str(&super::escape_attr(&painted));
    attributes.push('"');
    let _ = write!(attributes, " font-size=\"{}\"", coord(item.size_px));
    if item.run.bold {
        attributes.push_str(" font-weight=\"bold\"");
    }
    if item.run.italic {
        attributes.push_str(" font-style=\"italic\"");
    }
    if item.run.outline {
        let _ = write!(
            attributes,
            " stroke=\"{}\" stroke-width=\"{}\"",
            super::escape_attr(&fill),
            coord((item.size_px * 0.035).max(0.5))
        );
    }
    // A fragment the layout placed outside the page carries its reason so a
    // downstream correction can find it (the mark moves nothing by itself).
    if mark_off_page {
        if let Some(reason) = item.off_page {
            let _ = write!(attributes, " data-off-page=\"{}\"", reason.as_str());
        }
    }
    if Outlines::on() && shown.chars().all(char::is_whitespace) {
        // Nothing to draw: its underline or shading is an item of its own.
        if (item.run.char_scale - 1.0).abs() > 1e-9 {
            out.push_str("</g>\n");
        }
        return;
    }
    if Outlines::on() {
        if let Some(d) = outline_path(&shown, &family, item) {
            let paint = if item.run.outline {
                format!(
                    "fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"",
                    super::escape_attr(&fill),
                    coord((item.size_px * 0.035).max(0.5))
                )
            } else {
                format!("fill=\"{}\"", super::escape_attr(&fill))
            };
            let mark = item
                .off_page
                .filter(|_| mark_off_page)
                .map(|reason| format!(" data-off-page=\"{}\"", reason.as_str()))
                .unwrap_or_default();
            let _ = writeln!(out, "  <g {paint}{mark}>{d}</g>");
            if (item.run.char_scale - 1.0).abs() > 1e-9 {
                out.push_str("</g>\n");
            }
            return;
        }
    }
    // Explicit per-scalar x from the shared shaper so the browser does not
    // re-shape against a different face (F07). Ligature clusters share one x.
    let x_attr = shaped_x_attribute(&shown, &family, item);
    let _ = writeln!(
        out,
        "  <text x=\"{x_attr}\" y=\"{}\" {} fill=\"{}\" xml:space=\"preserve\">{}</text>",
        coord(item.baseline),
        attributes,
        if item.run.outline {
            "none".to_owned()
        } else {
            super::escape_attr(&fill)
        },
        super::escape_text(&shown),
    );
    if (item.run.char_scale - 1.0).abs() > 1e-9 {
        out.push_str("</g>\n");
    }
}

/// Each scalar's x when the layout measured with `hmtx` advances
/// ([`TextAdvanceKind::Metric`]): spacing and compressed punctuation as the
/// layout applied them.
fn metric_positions(shown: &str, family: &str, item: &TextItem) -> Vec<f64> {
    let provider = BuiltinFontProvider::new();
    let extra = crate::style::spacing_px(&item.run, item.size_px);
    let mut cursor = item.x;
    let mut prev = None;
    let chars: Vec<char> = shown.chars().collect();
    let mut metric = Vec::with_capacity(chars.len());
    for (i, &ch) in chars.iter().enumerate() {
        metric.push(cursor);
        let next = chars.get(i + 1).copied().or(item.following_non_space);
        let factor = crate::style::compressed_char_factor(
            prev,
            ch,
            next,
            item.compress_punctuation,
            item.plain_space_factor,
        );
        cursor += crate::style::space_advance_em(
            &item.run,
            family,
            ch,
            provider.advance_em(family, ch, item.run.bold, item.run.italic),
        ) * item.size_px
            * factor;
        if !ch.is_whitespace() {
            cursor += extra;
            prev = Some(ch);
        } else if factor > 0.0 {
            prev = Some(ch);
        }
    }
    metric
}

fn shaped_x_attribute(shown: &str, family: &str, item: &TextItem) -> String {
    if shown.is_empty() || shown.chars().count() == 1 {
        return coord(item.x);
    }
    let xs = match item.advance {
        TextAdvanceKind::Metric => metric_positions(shown, family, item),
        TextAdvanceKind::Shaped => {
            let Some(shaped) = shape_bundled_run(shown, family, &item.run) else {
                return coord(item.x);
            };
            let mut xs = unicode_x_positions_px(shown, &shaped, item.x, item.size_px);
            let offsets = crate::font::shaped_cluster_offsets_px(item, shown, &shaped);
            let mut cluster_index = 0;
            for (scalar, (byte, _)) in shown.char_indices().enumerate() {
                while shaped
                    .clusters
                    .get(cluster_index)
                    .is_some_and(|cluster| cluster.byte_end <= byte)
                {
                    cluster_index += 1;
                }
                if let (Some(x), Some(offset)) = (xs.get_mut(scalar), offsets.get(cluster_index)) {
                    *x += offset;
                }
            }
            xs
        }
    };
    if xs.is_empty() {
        return coord(item.x);
    }
    let mut out = String::new();
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&coord(*x));
    }
    out
}

thread_local! {
    static OUTLINES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Glyph outlines in font units, by face and glyph: a page draws the same
    /// few dozen glyphs hundreds of times.
    static GLYPHS: std::cell::RefCell<GlyphCache> = std::cell::RefCell::new(GlyphCache::new());
    /// The glyphs this page used, defined once in its `<defs>`.
    static USED: std::cell::RefCell<std::collections::BTreeMap<String, std::rc::Rc<str>>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

type GlyphCache = std::collections::HashMap<([u8; 32], u32), std::rc::Rc<Glyph>>;

/// While a page is painted with [`crate::RenderOptions::text_outlines`].
pub(crate) struct Outlines(bool);

impl Outlines {
    pub(crate) fn scope(on: bool) -> Self {
        USED.with(|used| used.borrow_mut().clear());
        Self(OUTLINES.with(|cell| cell.replace(on)))
    }

    fn on() -> bool {
        OUTLINES.with(std::cell::Cell::get)
    }
}

impl Drop for Outlines {
    fn drop(&mut self) {
        OUTLINES.with(|cell| cell.set(self.0));
    }
}

/// The `<defs>` of the glyphs the page drew as outlines, empty when none.
pub(crate) fn outline_defs() -> String {
    USED.with(|used| {
        let used = used.borrow();
        if used.is_empty() {
            return String::new();
        }
        let mut out = String::from("<defs>\n");
        for (id, d) in used.iter() {
            let _ = writeln!(out, "  <path id=\"{id}\" d=\"{d}\"/>");
        }
        out.push_str("</defs>\n");
        out
    })
}

/// One glyph: its outline in font units (y up) as SVG path data, and the
/// face's units per em.
struct Glyph {
    id: String,
    units_per_em: f64,
    d: std::rc::Rc<str>,
}

struct Pen(String);

impl Pen {
    fn point(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "{} {}", coord(f64::from(x)), coord(f64::from(y)));
    }
}

impl skrifa::outline::OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push('M');
        self.point(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push('L');
        self.point(x, y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.push('Q');
        self.point(cx0, cy0);
        self.0.push(' ');
        self.point(x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.push('C');
        self.point(cx0, cy0);
        self.0.push(' ');
        self.point(cx1, cy1);
        self.0.push(' ');
        self.point(x, y);
    }
    fn close(&mut self) {
        self.0.push('Z');
    }
}

/// The outline of glyph `id` in `face`, cached.
fn glyph(face: &crate::font::ResolvedFace, id: u32) -> Option<std::rc::Rc<Glyph>> {
    use skrifa::MetadataProvider as _;
    let key = (face.resource_hash, id);
    if let Some(hit) = GLYPHS.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(hit);
    }
    // Index 0, as the shaper reads the same bytes.
    let font = skrifa::FontRef::from_index(&face.bytes, 0).ok()?;
    let units_per_em = f64::from(
        font.metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .units_per_em,
    );
    if units_per_em <= 0.0 {
        return None;
    }
    let outline = font.outline_glyphs().get(skrifa::GlyphId::new(id))?;
    let mut pen = Pen(String::new());
    outline
        .draw(
            skrifa::outline::DrawSettings::unhinted(
                skrifa::instance::Size::unscaled(),
                skrifa::instance::LocationRef::default(),
            ),
            &mut pen,
        )
        .ok()?;
    let [a, b, c, d] = [
        face.resource_hash[0],
        face.resource_hash[1],
        face.resource_hash[2],
        face.resource_hash[3],
    ];
    let glyph = std::rc::Rc::new(Glyph {
        id: format!("g{a:02x}{b:02x}{c:02x}{d:02x}-{id}"),
        units_per_em,
        d: pen.0.into(),
    });
    GLYPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        // A long session with many faces: start over rather than grow.
        if cache.len() >= 65_536 {
            cache.clear();
        }
        cache.insert(key, std::rc::Rc::clone(&glyph));
    });
    Some(glyph)
}

/// A scale factor with the precision a glyph in font units needs (a few
/// thousandths of a pixel per unit).
fn fine(value: f64) -> String {
    let mut out = format!("{value:.7}");
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out
}

/// A fragment's glyphs as `<use>` references to their outlines, placed as the
/// `<text>` x list would place its clusters; each outline goes into the page's
/// `<defs>` once. `None` when the fragment cannot be drawn faithfully so: not
/// shaped, a glyph the face lacks, a style the face does not have (the
/// viewer's synthetic bold or italic), a variable instance.
fn outline_path(shown: &str, family: &str, item: &TextItem) -> Option<String> {
    if shown.is_empty() {
        return None;
    }
    let (face, placed) = match item.advance {
        TextAdvanceKind::Shaped => shaped_glyphs(shown, family, item)?,
        TextAdvanceKind::Metric => metric_glyphs(shown, family, item)?,
    };
    if face.weight.is_some() || face.bold != item.run.bold || face.italic != item.run.italic {
        return None;
    }
    let mut glyphs = Vec::with_capacity(placed.len());
    for (id, x, y) in placed {
        if id == 0 {
            return None;
        }
        glyphs.push((glyph(&face, id)?, x, y));
    }
    let mut out = String::new();
    for (glyph, x, y) in glyphs {
        if glyph.d.is_empty() {
            continue;
        }
        let scale = item.size_px / glyph.units_per_em;
        let _ = write!(
            out,
            "<use xlink:href=\"#{}\" transform=\"matrix({} 0 0 {} {} {})\"/>",
            glyph.id,
            fine(scale),
            fine(-scale),
            coord(x),
            coord(y)
        );
        USED.with(|used| {
            used.borrow_mut()
                .entry(glyph.id.clone())
                .or_insert_with(|| std::rc::Rc::clone(&glyph.d));
        });
    }
    // A fragment of spaces draws nothing.
    (!out.is_empty()).then_some(out)
}

/// A face, and glyphs to draw with it: glyph id and origin x and y.
type PlacedGlyphs = (crate::font::ResolvedFace, Vec<(u32, f64, f64)>);

/// The face and each glyph with its origin, as the shaper placed the run and
/// the paint offsets (spacing, justification) move its clusters.
fn shaped_glyphs(shown: &str, family: &str, item: &TextItem) -> Option<PlacedGlyphs> {
    let shaped = shape_bundled_run(shown, family, &item.run)?;
    let offsets = crate::font::shaped_cluster_offsets_px(item, shown, &shaped);
    // The cluster of every glyph, for its offset.
    let mut cluster_of = vec![0usize; shaped.glyphs.len()];
    for (index, cluster) in shaped.clusters.iter().enumerate() {
        for slot in cluster_of
            .iter_mut()
            .skip(cluster.glyph_start)
            .take(cluster.glyph_count)
        {
            *slot = index;
        }
    }
    let placed = shaped
        .glyphs
        .iter()
        .enumerate()
        .map(|(index, glyph)| {
            let offset = cluster_of
                .get(index)
                .and_then(|cluster| offsets.get(*cluster))
                .copied()
                .unwrap_or(0.0);
            (
                u32::from(glyph.glyph_id),
                item.x + glyph.x_em * item.size_px + offset,
                item.baseline - glyph.y_offset_em * item.size_px,
            )
        })
        .collect();
    Some((shaped.face, placed))
}

/// The face and each scalar's glyph at the x the layout's `hmtx` advances put
/// it: the bundled face the advances came from, its glyphs by `cmap`.
fn metric_glyphs(shown: &str, family: &str, item: &TextItem) -> Option<PlacedGlyphs> {
    use skrifa::MetadataProvider as _;
    let face = crate::font::resolve_face(
        crate::font::map_family(family),
        item.run.bold,
        item.run.italic,
    )?;
    let font = skrifa::FontRef::from_index(&face.bytes, 0).ok()?;
    let charmap = font.charmap();
    let xs = metric_positions(shown, family, item);
    let placed = shown
        .chars()
        .zip(xs)
        .filter(|(ch, _)| !ch.is_whitespace())
        .map(|(ch, x)| {
            let id = charmap.map(ch).map_or(0, skrifa::GlyphId::to_u32);
            (id, x, item.baseline)
        })
        .collect();
    Some((face, placed))
}
