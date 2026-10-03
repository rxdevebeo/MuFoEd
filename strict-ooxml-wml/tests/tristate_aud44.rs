//! AUD-44: TriState on/off, vanish override, XOR toggles, settings `w:val="0"`.

#![allow(clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::{StyleType, TriState};

use common::{document_parts, parse_parts, rels, W_NS};

const STYLES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/styles";
const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";

#[test]
fn vanish_off_on_run_overrides_style_vanish() {
    let styles = format!(
        "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"character\" w:styleId=\"Hidden\">\
<w:rPr><w:vanish/></w:rPr></w:style></w:styles>"
    );
    let rels = rels(&[("rIdStyles", STYLES, "styles.xml")]);
    let parts = document_parts(
        "<w:p><w:r><w:rPr><w:rStyle w:val=\"Hidden\"/><w:vanish w:val=\"0\"/></w:rPr>\
         <w:t>visible</w:t></w:r></w:p>",
        &[
            ("word/styles.xml", styles.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let Inline::Run(run) = &document.body.blocks[0].as_paragraph().unwrap().inlines[0] else {
        panic!("run");
    };
    assert_eq!(run.props.vanish, TriState::Off);
    let style = document
        .styles
        .get(&strict_ooxml_wml::model::StyleId::new("Hidden"))
        .unwrap();
    assert_eq!(style.run.vanish, TriState::On);
}

#[test]
fn even_and_odd_headers_can_be_turned_off() {
    let settings =
        format!("<w:settings xmlns:w=\"{W_NS}\"><w:evenAndOddHeaders w:val=\"0\"/></w:settings>");
    let rels = rels(&[("rIdSettings", SETTINGS, "settings.xml")]);
    let parts = document_parts(
        "<w:p/>",
        &[
            ("word/settings.xml", settings.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    assert!(!document.settings.even_and_odd_headers);
}

#[test]
fn bold_xor_across_paragraph_and_character_styles() {
    let styles = format!(
        "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"paragraph\" w:styleId=\"P\">\
<w:rPr><w:b/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"C\">\
<w:rPr><w:b/></w:rPr></w:style></w:styles>"
    );
    let rels = rels(&[("rIdStyles", STYLES, "styles.xml")]);
    let parts = document_parts(
        "<w:p><w:pPr><w:pStyle w:val=\"P\"/></w:pPr>\
         <w:r><w:rPr><w:rStyle w:val=\"C\"/></w:rPr><w:t>x</w:t></w:r></w:p>",
        &[
            ("word/styles.xml", styles.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    assert_eq!(
        document
            .styles
            .get(&strict_ooxml_wml::model::StyleId::new("P"))
            .unwrap()
            .style_type,
        StyleType::Paragraph
    );
    // Effective bold is checked via the renderer (XOR); model keeps both Ons.
    assert_eq!(
        document
            .styles
            .get(&strict_ooxml_wml::model::StyleId::new("P"))
            .unwrap()
            .run
            .bold,
        TriState::On
    );
    assert_eq!(
        document
            .styles
            .get(&strict_ooxml_wml::model::StyleId::new("C"))
            .unwrap()
            .run
            .bold,
        TriState::On
    );
    let _ = SupportStatus::Supported;
}
