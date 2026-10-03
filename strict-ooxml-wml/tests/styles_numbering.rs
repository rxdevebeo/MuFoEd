#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for `styles.xml`, `numbering.xml` and `settings.xml`.

mod common;

use strict_ooxml_wml::model::ids::{NumId, StyleId};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::StyleType;

use common::{document_parts, parse_parts, rels, W_NS};

const STYLES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/styles";
const NUMBERING: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";
const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";

fn styles_xml() -> Vec<u8> {
    format!(
        "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"paragraph\" w:styleId=\"C\" w:default=\"1\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"B\"><w:basedOn w:val=\"C\"/><w:pPr><w:jc w:val=\"end\"/></w:pPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"A\"><w:basedOn w:val=\"B\"/><w:rPr><w:b/></w:rPr></w:style>\
</w:styles>"
    )
    .into_bytes()
}

fn numbering_xml() -> Vec<u8> {
    format!(
        "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/><w:lvlJc w:val=\"start\"/></w:lvl>\
<w:lvl w:ilvl=\"1\"><w:start w:val=\"1\"/><w:numFmt w:val=\"lowerLetter\"/><w:lvlText w:val=\"%2)\"/></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"5\"><w:abstractNumId w:val=\"0\"/></w:num>\
<w:num w:numId=\"9\"><w:abstractNumId w:val=\"99\"/></w:num>\
</w:numbering>"
    )
    .into_bytes()
}

fn settings_xml() -> Vec<u8> {
    format!(
        "<w:settings xmlns:w=\"{W_NS}\">\
<w:defaultTabStop w:val=\"720\"/><w:zoom w:percent=\"100\" w:val=\"fullPage\"/><w:evenAndOddHeaders/>\
<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:val=\"15\"/></w:compat>\
</w:settings>"
    )
    .into_bytes()
}

fn full_document() -> strict_ooxml_wml::model::Document {
    let rels = rels(&[
        ("rIdStyles", STYLES, "styles.xml"),
        ("rIdNumbering", NUMBERING, "numbering.xml"),
        ("rIdSettings", SETTINGS, "settings.xml"),
    ]);
    let parts = document_parts(
        "<w:p><w:pPr><w:pStyle w:val=\"A\"/><w:numPr><w:numId w:val=\"5\"/></w:numPr></w:pPr></w:p>",
        &[
            ("word/styles.xml", styles_xml()),
            ("word/numbering.xml", numbering_xml()),
            ("word/settings.xml", settings_xml()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    parse_parts(&parts).expect("parse document")
}

#[test]
fn resolves_style_based_on_chain() {
    let document = full_document();
    assert_eq!(document.styles.len(), 3);
    let style = document.styles.get(&StyleId::new("A")).expect("style A");
    assert_eq!(style.style_type, StyleType::Character);
    let chain: Vec<&str> = style.based_on_chain.iter().map(StyleId::as_str).collect();
    assert_eq!(chain, vec!["B", "C"]);
    assert_eq!(style.based_on.as_ref().unwrap().as_str(), "B");
    assert_eq!(
        document
            .styles
            .default_for(StyleType::Paragraph)
            .unwrap()
            .as_str(),
        "C"
    );
}

#[test]
fn resolves_numbering_abstracts() {
    let document = full_document();
    assert!(!document.numbering.is_empty());
    let abstract_num = document
        .numbering
        .resolved_abstract(NumId(5))
        .expect("resolved abstract");
    assert_eq!(abstract_num.levels.len(), 2);
    assert_eq!(abstract_num.levels[0].format.as_deref(), Some("decimal"));
    assert_eq!(
        abstract_num.levels[1].format.as_deref(),
        Some("lowerLetter")
    );
}

#[test]
fn records_missing_abstract_numbering() {
    let document = full_document();
    let feature = document.support.get("w:numId").expect("recorded");
    assert_eq!(feature.status, SupportStatus::Partial);
}

#[test]
fn parses_settings() {
    let document = full_document();
    assert_eq!(document.settings.default_tab_stop.unwrap().value(), 720);
    assert!(document.settings.even_and_odd_headers);
    let zoom = document.settings.zoom.unwrap();
    assert_eq!(zoom.percent, Some(100));
    assert_eq!(document.settings.compatibility.len(), 1);
}

#[test]
fn records_based_on_cycle() {
    let styles = format!(
        "<w:styles xmlns:w=\"{W_NS}\"><w:style w:type=\"paragraph\" w:styleId=\"Loop\"><w:basedOn w:val=\"Loop\"/></w:style></w:styles>"
    )
    .into_bytes();
    let rels = rels(&[("rIdStyles", STYLES, "styles.xml")]);
    let parts = document_parts(
        "<w:p/>",
        &[
            ("word/styles.xml", styles),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    assert_eq!(
        document.support.get("w:basedOn").unwrap().status,
        SupportStatus::Partial
    );
}

/// A `numbering.xml` whose first level starts at `start`.
fn numbering_starting_at(start: &str) -> Vec<u8> {
    format!(
        "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\">\
<w:start w:val=\"{start}\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
</w:lvl></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
</w:numbering>"
    )
    .into_bytes()
}

/// The model and the `w:start` messages of a list that starts at `start`.
fn parse_start(start: &str) -> (strict_ooxml_wml::model::Document, Vec<String>) {
    let mut parts = document_parts("<w:p/>", &[]);
    parts.push((
        "word/numbering.xml".to_owned(),
        numbering_starting_at(start),
    ));
    parts.push((
        "word/_rels/document.xml.rels".to_owned(),
        rels(&[(
            "rIdNum",
            "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering",
            "numbering.xml",
        )]),
    ));
    let document = parse_parts(&parts).expect("parse");
    let messages = document
        .support()
        .iter()
        .filter(|entry| entry.feature_id.as_ref() == "w:start")
        .map(|entry| entry.message.clone().unwrap_or_default())
        .collect();
    (document, messages)
}

/// The `w:start` the model ended up with.
fn start_of(document: &strict_ooxml_wml::model::Document) -> u32 {
    document
        .numbering
        .abstracts()
        .next()
        .and_then(|abstract_num| abstract_num.levels.first())
        .and_then(|level| level.start)
        .expect("a level with a start")
}

#[test]
fn a_start_inside_the_range_is_kept_as_written() {
    let (document, messages) = parse_start("7");
    assert_eq!(start_of(&document), 7);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn a_start_past_the_models_range_is_clamped_and_reported() {
    // `w:start` is `ST_DecimalNumber`, so a producer may write a negative value
    // or one past what the renderer's counter holds. Either way the model takes
    // the nearest value it can and the report says which - the alternative was a
    // silent substitution, or a counter that wraps to zero mid-document.
    let (document, messages) = parse_start("4294967295");
    assert_eq!(
        start_of(&document),
        u32::try_from(i64::from(i32::MAX)).unwrap_or(0)
    );
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("clamped"),
        "the message says the value was clamped: {messages:?}"
    );

    let (document, messages) = parse_start("-5");
    assert_eq!(start_of(&document), 0);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("clamped"), "{messages:?}");
}

#[test]
fn a_start_that_is_not_a_number_is_reported_and_the_level_starts_at_zero() {
    let (document, messages) = parse_start("1.5");
    assert_eq!(start_of(&document), 0);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("not an integer"), "{messages:?}");
}

/// Package whose list instance redirects through a numbering style (`numStyleLink`).
fn numbering_style_link_parts(cycle: bool) -> Vec<(String, Vec<u8>)> {
    let styles = if cycle {
        format!(
            "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"numbering\" w:styleId=\"A\">\
<w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>\
<w:style w:type=\"numbering\" w:styleId=\"B\">\
<w:pPr><w:numPr><w:numId w:val=\"2\"/></w:numPr></w:pPr></w:style>\
</w:styles>"
        )
    } else {
        format!(
            "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"numbering\" w:styleId=\"List\">\
<w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>\
</w:styles>"
        )
    };
    let numbering = if cycle {
        format!(
            "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\"><w:numStyleLink w:val=\"A\"/></w:abstractNum>\
<w:abstractNum w:abstractNumId=\"1\"><w:numStyleLink w:val=\"B\"/></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"1\"/></w:num>\
<w:num w:numId=\"2\"><w:abstractNumId w:val=\"0\"/></w:num>\
</w:numbering>"
        )
    } else {
        format!(
            "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\"><w:styleLink w:val=\"List\"/>\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/>\
<w:lvlText w:val=\"%1.\"/></w:lvl>\
<w:lvl w:ilvl=\"1\"><w:start w:val=\"1\"/><w:numFmt w:val=\"lowerLetter\"/>\
<w:lvlText w:val=\"%2)\"/></w:lvl>\
</w:abstractNum>\
<w:abstractNum w:abstractNumId=\"1\"><w:numStyleLink w:val=\"List\"/></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
<w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>\
</w:numbering>"
        )
    };
    let rels = rels(&[
        ("rIdStyles", STYLES, "styles.xml"),
        ("rIdNumbering", NUMBERING, "numbering.xml"),
    ]);
    document_parts(
        "<w:p><w:pPr><w:numPr><w:numId w:val=\"2\"/><w:ilvl w:val=\"0\"/></w:numPr></w:pPr></w:p>",
        &[
            ("word/styles.xml", styles.into_bytes()),
            ("word/numbering.xml", numbering.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    )
}

#[test]
fn num_style_link_resolves_to_linked_levels() {
    let document = parse_parts(&numbering_style_link_parts(false)).expect("parse");
    let abstract_num = document
        .numbering
        .resolved_abstract(NumId(2))
        .expect("resolved via numStyleLink");
    assert_eq!(abstract_num.id.0, 0);
    assert_eq!(abstract_num.levels.len(), 2);
    assert_eq!(abstract_num.levels[0].format.as_deref(), Some("decimal"));
    assert_eq!(
        abstract_num.levels[1].format.as_deref(),
        Some("lowerLetter")
    );
}

#[test]
fn num_style_link_cycle_is_reported_without_hanging() {
    let document = parse_parts(&numbering_style_link_parts(true)).expect("parse");
    let feature = document
        .support
        .get("w:numStyleLink")
        .expect("cycle recorded");
    assert_eq!(feature.status, SupportStatus::Partial);
    assert_eq!(feature.message.as_deref(), Some("cycle"));
    // The redirect was not applied, so numId 2 still points at abstract 0 (empty).
    let abstract_num = document
        .numbering
        .resolved_abstract(NumId(2))
        .expect("direct abstract");
    assert_eq!(abstract_num.id.0, 0);
    assert!(abstract_num.levels.is_empty());
}

#[test]
fn ilvl_above_eight_is_clamped_and_reported() {
    let numbering = format!(
        "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"12\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/>\
<w:lvlText w:val=\"%1.\"/></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
</w:numbering>"
    );
    let rels = rels(&[("rIdNumbering", NUMBERING, "numbering.xml")]);
    let parts = document_parts(
        "<w:p><w:pPr><w:numPr><w:numId w:val=\"1\"/><w:ilvl w:val=\"12\"/></w:numPr></w:pPr></w:p>",
        &[
            ("word/numbering.xml", numbering.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let level = document
        .numbering
        .resolved_abstract(NumId(1))
        .and_then(|abstract_num| abstract_num.levels.first())
        .expect("level");
    assert_eq!(level.ilvl.0, 8);
    let paragraph_ilvl = document.body.blocks[0]
        .as_paragraph()
        .expect("paragraph")
        .props
        .numbering
        .expect("numPr")
        .ilvl
        .expect("ilvl");
    assert_eq!(paragraph_ilvl.0, 8);
    let feature = document.support.get("w:ilvl").expect("ilvl recorded");
    assert_eq!(feature.status, SupportStatus::Partial);
    assert!(
        feature
            .message
            .as_deref()
            .is_some_and(|message| message.contains("clamped")),
        "{:?}",
        feature.message
    );
}

#[test]
fn duplicate_abstract_and_num_ids_keep_the_first() {
    let numbering = format!(
        "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"first\"/></w:lvl>\
</w:abstractNum>\
<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"second\"/></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"99\"/></w:num>\
</w:numbering>"
    );
    let rels = rels(&[("rIdNumbering", NUMBERING, "numbering.xml")]);
    let parts = document_parts(
        "<w:p/>",
        &[
            ("word/numbering.xml", numbering.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let abstract_num = document
        .numbering
        .resolved_abstract(NumId(1))
        .expect("first abstract");
    assert_eq!(abstract_num.levels[0].format.as_deref(), Some("decimal"));
    assert_eq!(abstract_num.levels[0].text.as_deref(), Some("first"));
    assert_eq!(
        document.support.get("w:abstractNumId").unwrap().status,
        SupportStatus::Partial
    );
    assert_eq!(
        document.support.get("w:numId").unwrap().status,
        SupportStatus::Partial
    );
}
