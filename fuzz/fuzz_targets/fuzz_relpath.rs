#![no_main]

use libfuzzer_sys::fuzz_target;
use strict_ooxml_core::opc::path::{canonicalize_part_name, resolve_target};
use strict_ooxml_core::part::PartId;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = canonicalize_part_name(text);
        let base = PartId::new("/word/document.xml");
        let _ = resolve_target(&base, text, false);
        let _ = resolve_target(&base, text, true);
    }
});
