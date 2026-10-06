//! Strict lexical forms for settings, crops, numbering, and math (A09).

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{verify_no_silent_loss, write_package, WriteOptions};

fn rewrite(bytes: &[u8]) -> Vec<u8> {
    let package = Package::open_reader(
        bytes,
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .normalization(TransitionalNormalizer::new()),
    )
    .expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    verify_no_silent_loss(&written.report).expect("silent loss");
    written.bytes
}

fn part_text(bytes: &[u8], name: &str) -> String {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("reopen");
    let raw = package.read_part(&PartId::new(name)).expect(name);
    String::from_utf8(raw).expect("utf-8")
}

fn element_containing<'a>(text: &'a str, start_mark: &str) -> &'a str {
    let start = text.find(start_mark).unwrap_or_else(|| {
        panic!("missing {start_mark} in {text}");
    });
    let rest = &text[start..];
    let end = rest
        .find("/>")
        .or_else(|| rest.find("</"))
        .unwrap_or_else(|| panic!("unclosed {start_mark}"));
    &rest[..end]
}

#[test]
fn f04_settings_bitmap_is_strict() {
    let written = rewrite(&strict_ooxml_testkit::audit::settings_bitmap_docx());
    let settings = part_text(&written, "/word/settings.xml");
    let filter = element_containing(&settings, "<w:stylePaneFormatFilter");
    assert!(
        filter.contains("w:allStyles=\"true\""),
        "0001 must become allStyles: {filter}"
    );
    assert!(
        !filter.contains("w:val"),
        "legacy val must not be copied: {filter}"
    );

    let mixed = settings_docx("<w:stylePaneFormatFilter w:val=\"2002\" w:allStyles=\"false\"/>");
    let mixed_settings = part_text(&rewrite(&mixed), "/word/settings.xml");
    let mixed_filter = element_containing(&mixed_settings, "<w:stylePaneFormatFilter");
    assert!(
        mixed_filter.contains("w:allStyles=\"false\""),
        "named attributes win: {mixed_filter}"
    );
    assert!(
        !mixed_filter.contains("w:val"),
        "legacy val is not overlaid: {mixed_filter}"
    );
    assert!(
        !mixed_filter.contains("customStyles"),
        "the bitmap must not be applied on top of named attributes: {mixed_filter}"
    );

    let reserved = settings_docx("<w:stylePaneFormatFilter w:val=\"0010\"/>");
    let package = Package::open_reader(
        reserved.as_slice(),
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .normalization(TransitionalNormalizer::new()),
    )
    .expect("open reserved");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse reserved");
    let output =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write reserved");
    assert!(
        output
            .report
            .losses()
            .iter()
            .any(|loss| loss.feature_id.contains("stylePaneFormatFilter")),
        "reserved bit 0010 must be reported: {:?}",
        output.report.losses()
    );
    let reserved_settings = part_text(&output.bytes, "/word/settings.xml");
    assert!(
        !reserved_settings.contains("0010"),
        "reserved mask must not be written: {reserved_settings}"
    );
}

#[test]
fn f04_crop_percentage_is_strict() {
    let once = rewrite(&strict_ooxml_testkit::audit::crop_docx());
    let document = part_text(&once, "/word/document.xml");
    assert!(
        document.contains("l=\"1.253%\""),
        "1253 thousandths is 1.253%: {document}"
    );
    assert!(document.contains("t=\"0.010%\""), "{document}");
    assert!(document.contains("r=\"0.020%\""), "{document}");
    assert!(document.contains("b=\"0.030%\""), "{document}");
    let twice = rewrite(&once);
    let again = part_text(&twice, "/word/document.xml");
    assert!(
        again.contains("l=\"1.253%\"") && again.contains("t=\"0.010%\""),
        "a second write must keep the percentage: {again}"
    );
}

#[test]
fn f04_level_suffix_order_is_strict() {
    let written = rewrite(&strict_ooxml_testkit::audit::level_suffix_docx());
    let numbering = part_text(&written, "/word/numbering.xml");
    let suff = numbering.find("<w:suff").expect("suff");
    let text = numbering.find("<w:lvlText").expect("lvlText");
    let justification = numbering.find("<w:lvlJc").expect("lvlJc");
    assert!(
        suff < text && text < justification,
        "suff must precede lvlText and lvlJc: {numbering}"
    );
    assert!(
        numbering.contains("w:val=\"nothing\""),
        "nothing is a legal suffix: {numbering}"
    );
}

#[test]
fn f04_math_onoff_is_strict() {
    let written = rewrite(&strict_ooxml_testkit::audit::math_onoff_docx());
    let settings = part_text(&written, "/word/settings.xml");
    assert!(
        settings.contains("<m:smallFrac m:val=\"false\"/>"),
        "off must become false: {settings}"
    );
    assert!(!settings.contains("m:val=\"off\""), "{settings}");
}

#[test]
fn f04_bitflags_crop_and_rewrite_matrix() {
    let custom = rewrite(&settings_docx("<w:stylePaneFormatFilter w:val=\"0002\"/>"));
    let settings = part_text(&custom, "/word/settings.xml");
    let filter = element_containing(&settings, "<w:stylePaneFormatFilter");
    assert!(
        filter.contains("w:customStyles=\"true\""),
        "0002 is customStyles: {filter}"
    );
    assert!(!filter.contains("w:val"), "{filter}");

    let zero = rewrite(&strict_ooxml_testkit::audit::crop_docx());
    let first = part_text(&zero, "/word/document.xml");
    let second = part_text(&rewrite(&zero), "/word/document.xml");
    assert!(
        first.contains("a:srcRect") && second.contains("l=\"1.253%\""),
        "successive writes keep the crop: {second}"
    );
}

fn settings_docx(inner: &str) -> Vec<u8> {
    strict_ooxml_testkit::DocxBuilder::transitional()
        .body("<w:p><w:r><w:t>SETTINGS PROBE</w:t></w:r></w:p>")
        .rel("rIdSettings", "settings", "settings.xml")
        .content_type(
            "/word/settings.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        )
        .part_xml("word/settings.xml", "w:settings", inner)
        .build()
}
