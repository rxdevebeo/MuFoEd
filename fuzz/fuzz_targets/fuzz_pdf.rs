#![no_main]

//! AUD-92: PDF open + page walk must never panic under reduced limits.

use libfuzzer_sys::fuzz_target;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};

fuzz_target!(|data: &[u8]| {
    let limits = PdfLimits {
        max_pages: 32,
        max_content_bytes: 1 << 20,
        max_operations: 50_000,
        max_glyphs: 20_000,
        max_font_glyphs: 4_096,
        max_image_bytes: 1 << 20,
        max_path_points: 10_000,
        max_fonts: 64,
        max_input_bytes: 4 << 20,
        max_form_depth: 4,
        max_cached_image_bytes: 1 << 20,
        max_raster_pixels: 1024 * 1024,
    };
    let Ok(mut doc) = PdfDocument::open(data, limits) else {
        return;
    };
    let pages = doc.page_count().min(32);
    for index in 1..=pages {
        let _ = doc.page(index);
    }
});
