#![no_main]

use libfuzzer_sys::fuzz_target;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

fuzz_target!(|data: &[u8]| {
    let limits = ResourceLimits {
        max_xml_depth: 64,
        max_xml_attributes_per_elem: 64,
        max_text_len: 1 << 20,
        ..ResourceLimits::default()
    };
    if let Ok(mut reader) = XmlReader::new(data, PartId::new("/f.xml"), &limits) {
        for _ in 0..10_000 {
            match reader.next_event() {
                Ok(XmlEvent::Eof) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }
});
