//! Independent acceptance probe for DrawingML angle preservation.
use strict_ooxml_core::{normalize::{RawNormalizer, TransitionalNormalizer}, part::PartId};

#[test]
fn acceptance_hue_angle_keeps_integer_units() {
    let input = br#"<a:srgbClr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" val="FF0000"><a:hue val="60000"/><a:hueOff val="60000"/></a:srgbClr>"#;
    let normalizer = TransitionalNormalizer::new();
    let output = normalizer.normalize_part(&PartId::new("/word/theme/theme1.xml"), input).expect("normalize");
    let xml = std::str::from_utf8(&output).expect("utf8");
    println!("{xml}");
    assert!(!xml.contains("60%"), "angle must retain 60000ths of a degree, not percent");
}
