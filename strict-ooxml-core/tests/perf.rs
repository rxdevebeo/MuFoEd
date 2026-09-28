//! Regression barrier for XML parsing performance (rework R1/R2).
//!
//! The threshold is deliberately generous so the test does not flake on slow CI
//! hardware; it only catches a return to quadratic behavior (which was ~68 s for
//! this input before R1).

use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

#[test]
fn one_megabyte_xml_parses_within_budget() {
    let mut xml = String::from("<w:document xmlns:w=\"urn:w\">");
    for _ in 0..32_000 {
        xml.push_str("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    }
    xml.push_str("</w:document>");
    let bytes = xml.into_bytes();
    assert!(bytes.len() >= 1_000_000, "fixture must exceed 1 MiB");

    let start = std::time::Instant::now();
    let mut reader = XmlReader::new(
        &bytes,
        PartId::new("/word/document.xml"),
        &ResourceLimits::default(),
    )
    .expect("valid reader");
    let mut events = 0usize;
    loop {
        match reader.next_event().expect("valid xml") {
            XmlEvent::Eof => break,
            _ => events += 1,
        }
    }
    let elapsed = start.elapsed();
    assert!(events > 200_000, "unexpected event count: {events}");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "XML parsing too slow (quadratic regression?): {elapsed:?}"
    );
}
