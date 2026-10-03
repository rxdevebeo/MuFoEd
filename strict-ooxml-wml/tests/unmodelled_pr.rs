//! AUD-46: unknown `*Pr` children are recorded; modelled properties parse.

#![allow(clippy::doc_markdown)]

mod common;

use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::{Justification, TriState, Twips, WidthKind};

use common::{document_parts, parse_parts, rels, W_NS};

const STYLES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/styles";

#[test]
fn every_default_branch_records() {
    let styles = format!(
        "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"paragraph\" w:styleId=\"S\"><w:zzzStyle/></w:style></w:styles>"
    );
    let rels = rels(&[("rIdStyles", STYLES, "styles.xml")]);
    let parts = document_parts(
        "<w:p><w:pPr><w:zzzP/><w:sectPr><w:zzzSect/></w:sectPr></w:pPr>\
         <w:r><w:rPr><w:bCs/><w:zzzR/></w:rPr><w:t>x</w:t></w:r></w:p>\
         <w:tbl><w:tblPr><w:zzzTbl/></w:tblPr><w:tblGrid><w:gridCol w:w=\"100\"/></w:tblGrid>\
         <w:tr><w:trPr><w:zzzTr/></w:trPr><w:tc><w:tcPr><w:zzzTc/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>",
        &[
            ("word/styles.xml", styles.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    for feature in [
        "w:zzzP",
        "w:zzzR",
        "w:zzzTbl",
        "w:zzzTr",
        "w:zzzTc",
        "w:zzzSect",
        "w:zzzStyle",
    ] {
        let entry = document
            .support
            .get(feature)
            .unwrap_or_else(|| panic!("{feature}"));
        assert_eq!(entry.status, SupportStatus::Unsupported, "{feature}");
        assert_eq!(
            entry.message.as_deref(),
            Some("property not modelled"),
            "{feature}"
        );
    }
    let run = document.body.blocks[0].as_paragraph().unwrap().inlines[0]
        .as_run()
        .unwrap();
    assert_eq!(run.props.bold_cs, TriState::On);
}

#[test]
fn modelled_properties_parse() {
    let body = "\
<w:p><w:pPr><w:framePr w:w=\"200\" w:h=\"100\" w:wrap=\"around\"/></w:pPr>\
 <w:r><w:rPr><w:bCs/><w:iCs w:val=\"0\"/><w:rtl/></w:rPr><w:t>x</w:t></w:r></w:p>\
 <w:tbl><w:tblPr>\
   <w:tblpPr w:leftFromText=\"40\" w:horzAnchor=\"page\" w:tblpX=\"100\"/>\
   <w:tblCellSpacing w:w=\"20\" w:type=\"dxa\"/>\
 </w:tblPr>\
 <w:tblGrid><w:gridCol w:w=\"500\"/><w:gridCol w:w=\"500\"/></w:tblGrid>\
 <w:tr><w:trPr><w:jc w:val=\"center\"/><w:tblCellSpacing w:w=\"10\" w:type=\"dxa\"/></w:trPr>\
 <w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>\
 <w:sectPr><w:pgNumType w:fmt=\"lowerRoman\" w:start=\"3\"/></w:sectPr>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");

    let paragraph = document.body.blocks[0].as_paragraph().unwrap();
    let frame = paragraph.props.frame.as_ref().expect("framePr");
    assert_eq!(frame.width, Some(Twips(200)));
    assert_eq!(frame.height, Some(Twips(100)));
    assert_eq!(frame.wrap.as_deref(), Some("around"));

    let run = paragraph.inlines[0].as_run().unwrap();
    assert_eq!(run.props.bold_cs, TriState::On);
    assert_eq!(run.props.italic_cs, TriState::Off);

    let table = document.body.blocks[1].as_table().unwrap();
    let positioning = table.props.positioning.as_ref().expect("tblpPr");
    assert_eq!(positioning.left_from_text, Some(Twips(40)));
    assert_eq!(positioning.x, Some(100));
    let spacing = table.props.cell_spacing.as_ref().expect("tblCellSpacing");
    assert_eq!(spacing.kind, WidthKind::Dxa);
    assert_eq!(spacing.value, Some(20));
    assert_eq!(table.rows[0].props.alignment, Some(Justification::Center));
    assert_eq!(
        table.rows[0]
            .props
            .cell_spacing
            .as_ref()
            .and_then(|width| width.value),
        Some(10)
    );

    let page_number = document
        .sections
        .last()
        .expect("section")
        .properties
        .page_number
        .as_ref()
        .expect("pgNumType");
    assert_eq!(page_number.format.as_deref(), Some("lowerRoman"));
    assert_eq!(page_number.start, Some(3));

    assert_eq!(
        document.support.get("w:framePr").unwrap().status,
        SupportStatus::Partial
    );
    assert_eq!(
        document.support.get("w:tblpPr").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn property_change_and_tbl_style_pr_are_recorded() {
    let styles = format!(
        "<w:styles xmlns:w=\"{W_NS}\">\
<w:style w:type=\"table\" w:styleId=\"T\">\
<w:tblStylePr w:type=\"firstRow\"><w:tcPr><w:shd w:fill=\"FF0000\"/></w:tcPr></w:tblStylePr>\
</w:style></w:styles>"
    );
    let rels = rels(&[("rIdStyles", STYLES, "styles.xml")]);
    let parts = document_parts(
        "<w:p><w:pPr><w:pPrChange w:id=\"0\" w:author=\"a\"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>",
        &[
            ("word/styles.xml", styles.into_bytes()),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    assert_eq!(
        document.support.get("w:pPrChange").unwrap().status,
        SupportStatus::Partial
    );
    assert_eq!(
        document.support.get("w:tblStylePr").unwrap().status,
        SupportStatus::Partial
    );
}
