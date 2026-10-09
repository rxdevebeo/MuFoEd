//! P12 — properties a header carries that the writer used to drop.
//!
//! The census compared each source header with its Strict copy and found
//! `w:cs` on Telugu runs and `w:autoSpaceDE`/`w:autoSpaceDN`/`w:adjustRightInd`
//! switched off in a footer. Neither was in the model, so neither was written.

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::values::TriState;
use strict_ooxml_wml::model::Block;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

#[test]
fn t_p12_cs_and_auto_space_survive_a_write() {
    let body = "<w:p><w:pPr><w:autoSpaceDE w:val=\"0\"/><w:autoSpaceDN w:val=\"false\"/>\
<w:adjustRightInd w:val=\"off\"/></w:pPr>\
<w:r><w:rPr><w:cs/><w:lang w:bidi=\"te-IN\"/></w:rPr><w:t>x</w:t></w:r></w:p>";
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(bytes.as_slice(), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let Some(Block::Paragraph(paragraph)) = document.body.blocks.first() else {
        panic!("a paragraph");
    };
    assert_eq!(paragraph.props.auto_space_de, TriState::Off);
    assert_eq!(paragraph.props.auto_space_dn, TriState::Off);
    assert_eq!(paragraph.props.adjust_right_ind, TriState::Off);

    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let reopened =
        Package::open_reader(written.bytes.as_slice(), &OpenOptions::default()).expect("reopen");
    let xml = String::from_utf8_lossy(
        &reopened
            .read_part(&PartId::new("/word/document.xml"))
            .expect("document"),
    )
    .into_owned();
    assert!(xml.contains(r#"<w:autoSpaceDE w:val="false"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:autoSpaceDN w:val="false"/>"#), "{xml}");
    assert!(
        xml.contains(r#"<w:adjustRightInd w:val="false"/>"#),
        "{xml}"
    );
    assert!(xml.contains(r#"<w:cs w:val="true"/>"#), "{xml}");
    // `EG_RPrBase` puts `w:cs` before `w:lang`.
    assert!(xml.find("<w:cs ") < xml.find("<w:lang "), "{xml}");
}
