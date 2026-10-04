#![no_main]

//! AUD-92: normalizer must never panic; rewritten (`Cow::Owned`) output must be
//! well-formed XML. Passthrough (`Cow::Borrowed`) includes damaged prefixes the
//! Strict parser reports later — those are left alone on purpose.

use libfuzzer_sys::fuzz_target;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

fuzz_target!(|data: &[u8]| {
    let normalizer = TransitionalNormalizer::new();
    let part = PartId::new("/word/document.xml");
    let Ok(cow) = normalizer.normalize(&part, data) else {
        return;
    };
    if matches!(cow, std::borrow::Cow::Borrowed(_)) {
        return;
    }
    let limits = ResourceLimits {
        max_xml_depth: 64,
        max_xml_attributes_per_elem: 64,
        max_text_len: 1 << 20,
        max_single_uncompressed: 1 << 20,
        max_total_uncompressed: 4 << 20,
        ..ResourceLimits::default()
    };
    let Ok(mut reader) = XmlReader::new(cow.as_ref(), part, &limits) else {
        panic!("normalize rewrote bytes that XmlReader rejects");
    };
    for _ in 0..10_000 {
        match reader.next_event() {
            Ok(XmlEvent::Eof) => break,
            Err(_) => panic!("normalize Owned output is not well-formed XML"),
            Ok(_) => {}
        }
    }
});
