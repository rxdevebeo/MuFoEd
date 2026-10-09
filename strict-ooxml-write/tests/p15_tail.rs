//! P15 — small facts the writer used to drop.
//!
//! Each one was a census row: `w:suppressOverlap`, `w:specVanish`, a column
//! bookmark's `w:colFirst`/`w:colLast`, and `w:tblHeader w:val="0"` (which
//! overrides a table style that repeats the row).

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

fn written_document(body: &str) -> String {
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(bytes.as_slice(), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let reopened =
        Package::open_reader(written.bytes.as_slice(), &OpenOptions::default()).expect("reopen");
    String::from_utf8_lossy(
        &reopened
            .read_part(&PartId::new("/word/document.xml"))
            .expect("document"),
    )
    .into_owned()
}

#[test]
fn t_p15_paragraph_and_run_toggles_survive() {
    let xml = written_document(
        "<w:p><w:pPr><w:suppressOverlap/></w:pPr>\
<w:r><w:rPr><w:specVanish/></w:rPr><w:t>x</w:t></w:r></w:p>",
    );
    assert!(
        xml.contains(r#"<w:suppressOverlap w:val="true"/>"#),
        "{xml}"
    );
    assert!(xml.contains(r#"<w:specVanish w:val="true"/>"#), "{xml}");
}

#[test]
fn t_p15_column_bookmark_and_header_off_survive() {
    let xml = written_document(
        "<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"100\"/></w:tblGrid>\
<w:tr><w:trPr><w:tblHeader w:val=\"0\"/></w:trPr><w:tc><w:p>\
<w:bookmarkStart w:id=\"1\" w:name=\"c\" w:colFirst=\"0\" w:colLast=\"2\"/>\
<w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p></w:tc></w:tr></w:tbl><w:p/>",
    );
    assert!(xml.contains(r#"w:colFirst="0""#), "{xml}");
    assert!(xml.contains(r#"w:colLast="2""#), "{xml}");
    assert!(xml.contains(r#"<w:tblHeader w:val="false"/>"#), "{xml}");
}

#[test]
fn t_p15_unsigned_char_space_is_written_signed() {
    let xml = written_document(
        "<w:p/><w:sectPr><w:docGrid w:linePitch=\"360\" w:charSpace=\"4294961151\"/></w:sectPr>",
    );
    assert!(xml.contains(r#"w:charSpace="-6145""#), "{xml}");
}

#[test]
fn t_p15_picture_locks_and_anchor_point_survive() {
    let body = "<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" \
behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" hidden=\"0\" allowOverlap=\"1\">\
<wp:simplePos x=\"635\" y=\"914400\"/>\
<wp:positionH relativeFrom=\"column\"><wp:posOffset>0</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
<wp:extent cx=\"100\" cy=\"100\"/><wp:wrapNone/><wp:docPr id=\"1\" name=\"p\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"p\"/>\
<pic:cNvPicPr preferRelativeResize=\"0\"><a:picLocks noChangeAspect=\"1\" noChangeArrowheads=\"1\"/>\
</pic:cNvPicPr></pic:nvPicPr><pic:blipFill rotWithShape=\"1\"><a:blip r:embed=\"rId5\">\
<a:lum bright=\"10000\"/><a:extLst><a:ext uri=\"{28A0092B-C50C-407E-A947-70E740481C1C}\">\
<a14:useLocalDpi xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" val=\"0\"/>\
</a:ext></a:extLst></a:blip></pic:blipFill><pic:spPr><a:xfrm><a:ext cx=\"100\" cy=\"100\"/>\
</a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:ln><a:noFill/></a:ln></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>";
    let bytes = DocxBuilder::strict()
        .body(body)
        .rel("rId5", "image", "media/image1.png")
        .content_type("/word/media/image1.png", "image/png")
        .part("word/media/image1.png", b"\x89PNG\r\n\x1a\nimage".to_vec())
        .build();
    let package = Package::open_reader(bytes.as_slice(), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
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
    assert!(xml.contains(r#"<wp:simplePos x="635" y="914400"/>"#), "{xml}");
    assert!(xml.contains(r#"hidden="false""#), "{xml}");
    assert!(xml.contains(r#"<pic:cNvPicPr preferRelativeResize="0">"#), "{xml}");
    assert!(
        xml.contains(r#"<a:picLocks noChangeAspect="1" noChangeArrowheads="1"/>"#),
        "{xml}"
    );
    assert!(xml.contains(r#"<pic:blipFill rotWithShape="1">"#), "{xml}");
    assert!(xml.contains(r#"bright="10000""#), "{xml}");
    assert!(xml.contains("useLocalDpi"), "{xml}");
    assert!(xml.contains(r#"prst="rect""#), "{xml}");
    assert!(xml.contains("<a:noFill"), "{xml}");
    // The kept markup parses again and comes back the same.
    let again = parse_document(&reopened, &ParseOptions::default()).expect("reparse");
    let rewritten =
        write_package(&again, Some(&reopened), &WriteOptions::default()).expect("rewrite");
    assert_eq!(written.bytes, rewritten.bytes, "not a fixed point");
}

#[test]
fn t_p15_shape_colours_keep_their_element() {
    let shape = |fill: &str| {
        format!(
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/>\
<wp:docPr id=\"1\" name=\"s\"/><a:graphic>\
<a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/>\
</a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:solidFill>{fill}</a:solidFill>\
</wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        )
    };
    let xml = written_document(&shape(r#"<a:sysClr val="windowText" lastClr="000000"/>"#));
    assert!(xml.contains("<a:sysClr "), "{xml}");
    assert!(xml.contains(r#"lastClr="000000""#), "{xml}");
    let xml = written_document(&shape(r#"<a:prstClr val="black"/>"#));
    assert!(xml.contains("<a:prstClr "), "{xml}");
    assert!(xml.contains(r#"val="black""#), "{xml}");
    let xml = written_document(&shape(
        r#"<a:schemeClr val="accent1"><a:lumMod val="60%"/><a:lumOff val="40%"/></a:schemeClr>"#,
    ));
    assert!(xml.contains("<a:lumMod ") && xml.contains(r#"val="60%""#), "{xml}");
    assert!(xml.contains("<a:lumOff ") && xml.contains(r#"val="40%""#), "{xml}");
}
