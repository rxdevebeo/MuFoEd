//! WMF previews of OLE objects (MathType equations) rasterized to their frame.
//!
//! The metafiles are built here the way MathType writes them: a placeable
//! header at ~2306 logical units per inch, `SetTextAlign(TA_BASELINE |
//! TA_UPDATECP)`, and every run placed by a `MoveTo` before an `ExtTextOut`
//! whose own point is (0, 0). Each test pins one defect of the vendored
//! `emfsdk` (see `vendor/README.md`): sizes taken as device pixels, the text
//! alignment ignored, and a one-sample scanline fill that dropped hairlines.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::doc_markdown
)]

use resvg::tiny_skia::Pixmap;
use strict_ooxml_render_svg::metafile::rasterize_metafile_preview;

/// Window of the equation in logical units, as MathType writes it.
const WINDOW: (i16, i16) = (800, 352);
/// Logical units per inch in the placeable header.
const INCH: u16 = 2306;
/// The laid-out frame in px (24.75pt x 11.25pt at 96 dpi); the raster is 2x.
const FRAME: (f64, f64) = (33.0, 15.0);

const TA_BASELINE_UPDATECP: u16 = 0x0019;

#[derive(Default)]
struct Wmf {
    records: Vec<u16>,
    objects: u16,
    max_record: u32,
}

impl Wmf {
    fn record(&mut self, function: u16, params: &[u16]) -> &mut Self {
        let size = 3 + params.len() as u32;
        self.records.push(size as u16);
        self.records.push((size >> 16) as u16);
        self.records.push(function);
        self.records.extend_from_slice(params);
        self.max_record = self.max_record.max(size);
        self
    }

    fn mathtype() -> Self {
        let mut wmf = Self::default();
        wmf.record(0x012E, &[TA_BASELINE_UPDATECP, 0])
            .record(0x020B, &[0, 0])
            .record(0x020C, &[WINDOW.1 as u16, WINDOW.0 as u16]);
        wmf
    }

    /// `CreateFontIndirect` + `SelectObject`; the font becomes object 0.
    fn font(&mut self, height: i16, face: &str) -> &mut Self {
        let mut params = vec![height as u16, 0, 0, 0, 400, 0, 0x0001, 0, 0];
        let mut name = [0u8; 32];
        name[..face.len()].copy_from_slice(face.as_bytes());
        params.extend(
            name.chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
        );
        self.objects = self.objects.max(1);
        self.record(0x02FB, &params).record(0x012D, &[0])
    }

    fn delete_object(&mut self) -> &mut Self {
        self.record(0x01F0, &[0])
    }

    fn move_to(&mut self, x: i16, y: i16) -> &mut Self {
        self.record(0x0214, &[y as u16, x as u16])
    }

    /// `ExtTextOut` at (0, 0): with `TA_UPDATECP` the current position wins.
    fn text(&mut self, bytes: &[u8]) -> &mut Self {
        let mut params = vec![0, 0, bytes.len() as u16, 0];
        let mut padded = bytes.to_vec();
        if padded.len() % 2 == 1 {
            padded.push(0);
        }
        params.extend(
            padded
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
        );
        self.record(0x0A32, &params)
    }

    /// `CreatePenIndirect` (solid black) + `SelectObject`; the pen is object 0.
    fn pen(&mut self, width: i16) -> &mut Self {
        self.objects = self.objects.max(1);
        self.record(0x02FA, &[0, width as u16, 0, 0, 0])
            .record(0x012D, &[0])
    }

    fn line_to(&mut self, x: i16, y: i16) -> &mut Self {
        self.record(0x0213, &[y as u16, x as u16])
    }

    fn bytes(&mut self) -> Vec<u8> {
        self.record(0x0000, &[]);
        let total_words = 9 + self.records.len() as u32;
        let mut words = vec![
            0xCDD7u16,
            0x9AC6,
            0,
            0,
            0,
            WINDOW.0 as u16,
            WINDOW.1 as u16,
            INCH,
            0,
            0,
        ];
        let checksum = words.iter().fold(0u16, |sum, word| sum ^ word);
        words.push(checksum);
        words.extend_from_slice(&[
            1,
            9,
            0x0300,
            total_words as u16,
            (total_words >> 16) as u16,
            self.objects,
            self.max_record as u16,
            (self.max_record >> 16) as u16,
            0,
        ]);
        words.extend_from_slice(&self.records);
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }
}

/// Dark pixels of the raster, as a row-major mask.
struct Ink {
    width: u32,
    height: u32,
    dark: Vec<bool>,
}

impl Ink {
    fn of(wmf: &[u8]) -> Self {
        let png = rasterize_metafile_preview(wmf, FRAME.0, FRAME.1, 64 << 20).expect("a raster");
        let pixmap = Pixmap::decode_png(&png).expect("a PNG");
        let dark = pixmap
            .pixels()
            .iter()
            .map(|pixel| {
                let color = pixel.demultiply();
                let luma =
                    (u32::from(color.red()) + u32::from(color.green()) + u32::from(color.blue()))
                        / 3;
                color.alpha() > 0 && luma < 160
            })
            .collect();
        Self {
            width: pixmap.width(),
            height: pixmap.height(),
            dark,
        }
    }

    fn count(&self) -> usize {
        self.dark.iter().filter(|dark| **dark).count()
    }

    /// `(left, top, right, bottom)`, exclusive, of the dark pixels.
    fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for (index, _) in self.dark.iter().enumerate().filter(|(_, dark)| **dark) {
            let (x, y) = (index as u32 % self.width, index as u32 / self.width);
            bounds = Some(bounds.map_or((x, y, x + 1, y + 1), |(l, t, r, b)| {
                (l.min(x), t.min(y), r.max(x + 1), b.max(y + 1))
            }));
        }
        bounds
    }

    fn rows(&self) -> Vec<u32> {
        (0..self.height)
            .filter(|y| (0..self.width).any(|x| self.dark[(y * self.width + x) as usize]))
            .collect()
    }
}

#[test]
fn the_raster_is_the_frame_at_twice_its_size() {
    let ink = Ink::of(
        &Wmf::mathtype()
            .font(-384, "Times New Roman")
            .move_to(58, 256)
            .text(b"x")
            .bytes(),
    );
    assert_eq!((ink.width, ink.height), (66, 30));
}

/// The font height is logical units: 384 of 2306 per inch is 12pt, about 32px
/// on this raster. It used to be drawn 384px tall, so the 66x30 raster showed
/// a fragment of one glyph, or nothing.
#[test]
fn a_font_height_is_mapped_like_a_coordinate() {
    let ink = Ink::of(
        &Wmf::mathtype()
            .font(-384, "Times New Roman")
            .move_to(58, 256)
            .text(b"x")
            .bytes(),
    );
    let (left, top, right, bottom) = ink.bounds().expect("the glyph is drawn");
    assert!(
        top > 0 && bottom < ink.height,
        "the glyph fits the raster: {top}..{bottom}"
    );
    assert!(
        bottom - top <= 20,
        "an x-height of a 32px font, not a 384px one: {}",
        bottom - top
    );
    assert!(right - left <= 24, "one glyph wide: {}", right - left);
    assert!(
        ink.count() < (ink.width * ink.height / 4) as usize,
        "{} dark pixels",
        ink.count()
    );
}

/// MathType places each run with `MoveTo` and writes (0, 0) in `ExtTextOut`:
/// under `TA_UPDATECP` that point is ignored. Drawn at (0, 0) the baseline was
/// the top edge, so the text sat above the raster.
#[test]
fn update_cp_draws_the_run_at_the_current_position() {
    let ink = Ink::of(
        &Wmf::mathtype()
            .font(-384, "Times New Roman")
            .move_to(58, 256)
            .text(b"x")
            .move_to(560, 256)
            .text(b"x")
            .bytes(),
    );
    let columns = |from: u32, to: u32| {
        (from..to).any(|x| (0..ink.height).any(|y| ink.dark[(y * ink.width + x) as usize]))
    };
    // 560 of 800 logical units is x = 46 of 66.
    assert!(columns(0, 30), "the first run, left");
    assert!(columns(44, 66), "the second run, at its own MoveTo");
    let (_, _, _, bottom) = ink.bounds().expect("ink");
    // The baseline 256 of 352 is y = 22 of 30.
    assert!(
        (20..=24).contains(&bottom),
        "glyphs stand on the baseline: {bottom}"
    );
}

/// A pen width is logical units too: MathType's fraction bar is 19 of them,
/// under 2px here. Taken as pixels it was a 19px block.
#[test]
fn a_pen_width_is_mapped_like_a_length() {
    let ink = Ink::of(
        &Wmf::mathtype()
            .pen(19)
            .move_to(0, 176)
            .line_to(800, 176)
            .bytes(),
    );
    let rows = ink.rows();
    assert!(!rows.is_empty(), "the bar is drawn");
    assert!(
        rows.len() <= 3,
        "a 1.6px bar, not a 19px block: rows {rows:?}"
    );
}

/// A minus is thinner than a pixel. Sampled once at each pixel centre it
/// vanished whenever it fell between two centres; with coverage it is there at
/// every sub-pixel offset.
#[test]
fn a_hairline_minus_survives_every_subpixel_offset() {
    for baseline in (240..=252).step_by(2) {
        let ink = Ink::of(
            &Wmf::mathtype()
                .font(-200, "Symbol")
                .move_to(300, baseline)
                .text(b"-")
                .bytes(),
        );
        assert!(ink.count() > 0, "the minus at baseline {baseline} is drawn");
    }
}

/// The Symbol face holds Adobe Symbol codes: 0xCE is U+2208, not U+00CE.
#[test]
fn symbol_codes_are_mapped_to_unicode() {
    let symbol = Ink::of(
        &Wmf::mathtype()
            .font(-384, "Symbol")
            .move_to(58, 256)
            .text(&[0xCE])
            .bytes(),
    );
    let latin = Ink::of(
        &Wmf::mathtype()
            .font(-384, "Times New Roman")
            .move_to(58, 256)
            .text(&[0xCE])
            .delete_object()
            .bytes(),
    );
    assert!(symbol.count() > 0 && latin.count() > 0);
    let (_, symbol_top, _, _) = symbol.bounds().expect("ink");
    let (_, latin_top, _, _) = latin.bounds().expect("ink");
    // Î carries a circumflex above the cap height; ∈ is no taller than an x.
    assert!(
        symbol_top > latin_top,
        "∈ ({symbol_top}) sits lower than Î ({latin_top})"
    );
}
