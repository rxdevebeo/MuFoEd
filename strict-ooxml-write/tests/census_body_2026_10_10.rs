//! Census rows of 2026-10-10 in the body and property writers.
//!
//! Each test is one row the census gate listed as a silent change between an
//! input part and the part this writer produced:
//!
//! - `w:framePr@anchorLock was=0`: an explicit off was dropped.
//! - `w:tblInd@type was=nil`: the indent was rewritten as `dxa`.
//! - `pgBorders` edges `@val was=palmsColor` / `twistedLines1`: art borders were
//!   not in `ST_Border` as the model knew it, and were written as `none`.
//! - `w:tcBorders` removed: a container holding only `w:tl2br` came back empty.
//! - `w:ffData` children (`enabled`, `calcOnExit`, `checkBox`, `textInput`, ...):
//!   legacy form-field data was skipped with its `w:fldChar`.
//! - `w:dataBinding@prefixMappings`: the Transitional URI inside the mappings is
//!   written in its Strict form, and the write now cites it.
//! - `w:br@clear`: kept on a text wrapping break (`RunContent::ClearingBreak`).
//! - `w:footnoteReference@customMarkFollows`: an explicit off is the default and
//!   is not written back; the parser cites it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown,
    clippy::format_push_string,
    missing_docs
)]

use std::sync::Arc;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::{FormFieldElement, Inline, RunContent};
use strict_ooxml_wml::model::values::{BorderStyle, WidthKind};
use strict_ooxml_wml::model::{Document, SupportStatus};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

/// What one write produced.
struct Written {
    document: Document,
    package: Package,
    bytes: Vec<u8>,
    report: String,
}

fn open(bytes: &[u8], normalize: bool) -> Package {
    let options = if normalize {
        OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(
                strict_ooxml_core::normalize::transitional::TransitionalNormalizer::new(),
            ))
    } else {
        OpenOptions::default()
    };
    Package::open_reader(bytes, &options).expect("open")
}

fn write(bytes: &[u8], normalize: bool) -> Written {
    let source = open(bytes, normalize);
    let document = parse_document(&source, &ParseOptions::default()).expect("parse");
    let written = write_package(&document, Some(&source), &WriteOptions::default()).expect("write");
    let package = open(&written.bytes, false);
    Written {
        document,
        package,
        bytes: written.bytes,
        report: written.report.to_string(),
    }
}

fn write_transitional_body(body: &str) -> Written {
    write(&DocxBuilder::transitional().body(body).build(), true)
}

fn part_text(package: &Package, part: &str) -> String {
    String::from_utf8_lossy(&package.read_part(&PartId::new(part)).expect(part)).into_owned()
}

fn document_xml(written: &Written) -> String {
    part_text(&written.package, "/word/document.xml")
}

/// The written package's second write is byte-identical to its first.
fn assert_fixed_point(first: &Written) {
    let again = write(&first.bytes, false);
    assert!(again.bytes == first.bytes, "not a fixed point");
}

/// The first start tag named `tag` in `xml`, through its `>`.
fn first_tag<'a>(xml: &'a str, tag: &str) -> &'a str {
    let start = xml
        .find(&format!("<{tag} "))
        .or_else(|| xml.find(&format!("<{tag}>")))
        .unwrap_or_else(|| panic!("no {tag} in {xml}"));
    let rest = &xml[start..];
    let end = rest.find('>').expect("closed tag");
    &rest[..=end]
}

fn cited(document: &Document, feature: &str) -> bool {
    document
        .support()
        .get(feature)
        .is_some_and(|feature| feature.status != SupportStatus::Supported)
}

fn simple_table(tbl_pr: &str, tc_pr: &str) -> String {
    format!(
        "<w:tbl><w:tblPr>{tbl_pr}</w:tblPr><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/>{tc_pr}</w:tcPr>\
<w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>"
    )
}

#[test]
fn a_frame_keeps_an_explicit_anchor_lock_off() {
    let written = write_transitional_body(
        "<w:p><w:pPr><w:framePr w:w=\"2000\" w:hAnchor=\"page\" w:anchorLock=\"0\"/></w:pPr>\
<w:r><w:t>framed</w:t></w:r></w:p>",
    );
    let xml = document_xml(&written);
    let frame = first_tag(&xml, "w:framePr");
    assert!(frame.contains(r#"w:anchorLock="false""#), "{frame}");
    let reparsed = write(&written.bytes, false);
    let Some(Block::Paragraph(paragraph)) = reparsed.document.body.blocks.first() else {
        panic!("paragraph");
    };
    let frame = paragraph.props.frame.as_ref().expect("frame");
    assert_eq!(frame.anchor_lock, Some(false));
    assert_fixed_point(&written);
}

#[test]
fn a_frame_keeps_an_anchor_lock_on_and_omits_an_absent_one() {
    let written = write_transitional_body(
        "<w:p><w:pPr><w:framePr w:w=\"2000\" w:anchorLock=\"1\"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>\
<w:p><w:pPr><w:framePr w:w=\"2000\"/></w:pPr><w:r><w:t>b</w:t></w:r></w:p>",
    );
    let xml = document_xml(&written);
    assert_eq!(xml.matches(r#"w:anchorLock="true""#).count(), 1, "{xml}");
    assert_eq!(xml.matches("w:anchorLock=").count(), 1, "{xml}");
    assert_fixed_point(&written);
}

#[test]
fn a_nil_table_indent_keeps_its_type() {
    let written =
        write_transitional_body(&simple_table("<w:tblInd w:w=\"0\" w:type=\"nil\"/>", ""));
    let xml = document_xml(&written);
    let indent = first_tag(&xml, "w:tblInd");
    assert!(indent.contains(r#"w:type="nil""#), "{indent}");
    let Some(Block::Table(table)) = written.document.body.blocks.first() else {
        panic!("table");
    };
    assert_eq!(table.props.indent_kind, Some(WidthKind::Nil));
    assert_fixed_point(&written);
}

#[test]
fn a_dxa_table_indent_is_still_dxa() {
    let written =
        write_transitional_body(&simple_table("<w:tblInd w:w=\"120\" w:type=\"dxa\"/>", ""));
    let xml = document_xml(&written);
    let indent = first_tag(&xml, "w:tblInd");
    assert!(indent.contains(r#"w:type="dxa""#), "{indent}");
    assert!(indent.contains(r#"w:w="6pt""#), "{indent}");
    assert_fixed_point(&written);
}

#[test]
fn art_page_borders_keep_their_style() {
    let edge = |side: &str, art: &str| {
        format!("<w:{side} w:val=\"{art}\" w:sz=\"20\" w:space=\"24\" w:color=\"auto\"/>")
    };
    let body = format!(
        "<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr><w:pgBorders w:offsetFrom=\"page\">{}{}{}{}\
</w:pgBorders><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>",
        edge("top", "palmsColor"),
        edge("left", "palmsColor"),
        edge("bottom", "twistedLines1"),
        edge("right", "twistedLines1"),
    );
    let written = write_transitional_body(&body);
    let xml = document_xml(&written);
    assert_eq!(xml.matches(r#"w:val="palmsColor""#).count(), 2, "{xml}");
    assert_eq!(xml.matches(r#"w:val="twistedLines1""#).count(), 2, "{xml}");
    assert_fixed_point(&written);
}

#[test]
fn every_art_border_name_round_trips_through_the_enum() {
    for name in [
        "palmsColor",
        "twistedLines1",
        "balloons3Colors",
        "stars3d",
        "custom",
    ] {
        let style = BorderStyle::from_strict(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(style.as_str(), name);
    }
}

#[test]
fn a_cell_border_holding_only_a_diagonal_is_kept() {
    let written = write_transitional_body(&simple_table(
        "",
        "<w:tcBorders><w:tl2br w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
</w:tcBorders>",
    ));
    let xml = document_xml(&written);
    assert!(xml.contains("<w:tcBorders><w:tl2br "), "{xml}");
    assert!(
        first_tag(&xml, "w:tl2br").contains(r#"w:val="single""#),
        "{xml}"
    );
    assert_fixed_point(&written);
}

#[test]
fn cell_diagonals_follow_the_edges_in_schema_order() {
    let written = write_transitional_body(&simple_table(
        "",
        "<w:tcBorders><w:tr2bl w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
<w:left w:val=\"nil\"/>\
<w:tl2br w:val=\"double\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tcBorders>",
    ));
    let xml = document_xml(&written);
    let start = xml.find("<w:start ").expect("start edge");
    let down = xml.find("<w:tl2br ").expect("tl2br");
    let up = xml.find("<w:tr2bl ").expect("tr2bl");
    assert!(start < down && down < up, "{xml}");
    assert_fixed_point(&written);
}

#[test]
fn a_paragraph_border_diagonal_is_still_not_modelled() {
    // `CT_PBdr` has no diagonal; only `w:tcBorders` keeps one.
    let written = write_transitional_body(
        "<w:p><w:pPr><w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"auto\"/>\
</w:pBdr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>",
    );
    let xml = document_xml(&written);
    assert!(!xml.contains("tl2br"), "{xml}");
    assert_fixed_point(&written);
}

const CHECKBOX_FIELD: &str = "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"><w:ffData>\
<w:name w:val=\"Check1\"/><w:enabled/><w:calcOnExit w:val=\"0\"/>\
<w:checkBox><w:sizeAuto/><w:default w:val=\"0\"/><w:checked w:val=\"0\"/></w:checkBox>\
</w:ffData></w:fldChar></w:r>\
<w:r><w:instrText xml:space=\"preserve\"> FORMCHECKBOX </w:instrText></w:r>\
<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>";

const TEXT_FIELD: &str = "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"><w:ffData>\
<w:name w:val=\"Text1\"/><w:enabled/><w:calcOnExit w:val=\"0\"/>\
<w:textInput><w:default w:val=\"abc\"/><w:maxLength w:val=\"10\"/></w:textInput>\
</w:ffData></w:fldChar></w:r>\
<w:r><w:instrText xml:space=\"preserve\"> FORMTEXT </w:instrText></w:r>\
<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>abc</w:t></w:r>\
<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>";

/// The first kept `w:ffData` content among `inlines`, fields included.
fn form_data(inlines: &[Inline]) -> Option<Vec<FormFieldElement>> {
    inlines.iter().find_map(|inline| match inline {
        Inline::Run(run) => run.content.iter().find_map(|content| match content {
            RunContent::FieldChar(field) => field.form_data.clone(),
            _ => None,
        }),
        Inline::Field(field) => form_data(&field.inlines),
        _ => None,
    })
}

#[test]
fn a_checkbox_form_field_keeps_its_field_data() {
    let written = write_transitional_body(CHECKBOX_FIELD);
    let xml = document_xml(&written);
    for piece in [
        "<w:fldChar w:fldCharType=\"begin\"><w:ffData><w:name w:val=\"Check1\"/>",
        "<w:enabled/>",
        "<w:calcOnExit ",
        "<w:checkBox><w:sizeAuto/><w:default ",
        "<w:checked ",
        "</w:checkBox></w:ffData></w:fldChar>",
    ] {
        assert!(xml.contains(piece), "{piece} in {xml}");
    }
    assert_eq!(xml.matches("<w:ffData>").count(), 1, "{xml}");
    assert_fixed_point(&written);
}

#[test]
fn a_text_form_field_keeps_its_field_data_in_order() {
    let written = write_transitional_body(TEXT_FIELD);
    let Some(Block::Paragraph(paragraph)) = written.document.body.blocks.first() else {
        panic!("paragraph");
    };
    let data = form_data(&paragraph.inlines).expect("form data");
    let names: Vec<&str> = data.iter().map(|element| element.local.as_ref()).collect();
    assert_eq!(names, ["name", "enabled", "calcOnExit", "textInput"]);
    let xml = document_xml(&written);
    assert!(
        xml.contains(
            "<w:textInput><w:default w:val=\"abc\"/><w:maxLength w:val=\"10\"/></w:textInput>"
        ),
        "{xml}"
    );
    assert_fixed_point(&written);
}

#[test]
fn a_data_binding_prefix_mapping_rewrite_is_cited() {
    let body = "<w:sdt><w:sdtPr><w:dataBinding w:prefixMappings=\"xmlns:ns0='http://schemas.openxmlformats.org/officeDocument/2006/extended-properties'\" \
w:xpath=\"/ns0:Properties[1]/ns0:Company[1]\" w:storeItemID=\"{6668398D-A668-4E3E-A5EB-62B293D839F1}\"/>\
<w:text/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>x</w:t></w:r></w:p></w:sdtContent></w:sdt>";
    let written = write_transitional_body(body);
    let xml = document_xml(&written);
    assert!(
        xml.contains("http://purl.oclc.org/ooxml/officeDocument/extendedProperties"),
        "{xml}"
    );
    assert!(
        written.report.contains("w:dataBinding@prefixMappings"),
        "{}",
        written.report
    );
    assert_fixed_point(&written);
}

#[test]
fn a_clearing_break_keeps_its_clear() {
    let written = write_transitional_body(
        "<w:p><w:r><w:t>a</w:t><w:br w:clear=\"all\"/><w:t>b</w:t></w:r></w:p>\
<w:p><w:r><w:br w:clear=\"none\"/></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\" w:clear=\"left\"/></w:r></w:p>",
    );
    let xml = document_xml(&written);
    assert!(
        xml.contains(r#"<w:br w:type="textWrapping" w:clear="all"/>"#),
        "{xml}"
    );
    // `none` is the default; on a page break `clear` means nothing and is cited.
    assert_eq!(xml.matches("w:clear=").count(), 1, "{xml}");
    let feature = written.document.support().get("w:br@clear").expect("cited");
    assert_eq!(feature.count, 1, "only the page break's clear is a loss");
    assert_fixed_point(&written);
}

#[test]
fn a_custom_mark_follows_attribute_is_cited() {
    let written = write_transitional_body(
        "<w:p><w:r><w:footnoteReference w:customMarkFollows=\"0\" w:id=\"1\"/></w:r></w:p>",
    );
    assert!(cited(
        &written.document,
        "w:footnoteReference@customMarkFollows"
    ));
}
