#![no_main]

//! AUD-92: PDF → Strict convert must never panic under reduced limits.

use libfuzzer_sys::fuzz_target;
use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};

fuzz_target!(|data: &[u8]| {
    let limits = PdfLimits {
        max_pages: 8,
        max_content_bytes: 1 << 20,
        max_operations: 50_000,
        max_glyphs: 20_000,
        max_font_glyphs: 4_096,
        max_image_bytes: 1 << 20,
        max_path_points: 10_000,
        max_fonts: 64,
        max_input_bytes: 2 << 20,
        max_form_depth: 4,
        max_cached_image_bytes: 1 << 20,
        max_cached_form_bytes: 1 << 20,
        max_raster_pixels: 512 * 512,
        max_image_pixels: 512 * 512,
        max_total_content_bytes: 4 << 20,
    };
    let Ok(mut pdf) = PdfDocument::open(data, limits) else {
        return;
    };
    let options = PdfOptions::default()
        .mode(Mode::Semantic)
        .embed_images(false);
    let _ = convert(&mut pdf, &options);
});
