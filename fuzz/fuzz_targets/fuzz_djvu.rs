#![no_main]
use libfuzzer_sys::fuzz_target;
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};
fuzz_target!(|bytes: &[u8]| {
    let limits = DjvuLimits {
        max_file_bytes: 65536,
        max_chunk_bytes: 65536,
        max_chunks: 256,
        max_pages: 8,
        max_component_files: 8,
        max_bzz_block: 65536,
        max_text_bytes: 65536,
        max_zones: 256,
        max_zone_depth: 8,
        max_zone_text_bytes: 262144,
        max_ink_pixels: 65536,
        max_sjbz_symbols: 256,
    };
    if let Ok(doc) = DjvuDocument::open(bytes, limits) {
        for page in doc.pages() {
            let _ = page.image(limits);
        }
    }
});
