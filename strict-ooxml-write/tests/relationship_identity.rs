//! Body allocation and pass-through must share identical relationships.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

#[test]
fn carrying_source_relationships_does_not_duplicate_a_used_hyperlink() {
    for target in ["https://example.com/", "%20https://example.com/"] {
        let rels = format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"link\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/hyperlink\" Target=\"{target}\" TargetMode=\"External\"/></Relationships>");
        let bytes = DocxBuilder::strict()
            .body("<w:p><w:hyperlink r:id=\"link\"><w:r><w:t>first</w:t></w:r></w:hyperlink><w:hyperlink r:id=\"link\"><w:r><w:t>second</w:t></w:r></w:hyperlink></w:p>")
            .part("word/_rels/document.xml.rels", rels.into_bytes())
            .build();
        let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
        let document = parse_document(&package, &ParseOptions::default()).expect("parse");
        let written =
            write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
        let output =
            Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");
        let raw = output
            .read_part(&PartId::new("/word/_rels/document.xml.rels"))
            .expect("rels");
        let text = std::str::from_utf8(&raw).expect("UTF-8");
        let xml = roxmltree::Document::parse(text).expect("independent XML oracle");
        let hyperlinks: Vec<_> = xml
            .descendants()
            .filter(|node| {
                node.attribute("Type")
                    .is_some_and(|value| value.ends_with("/hyperlink"))
            })
            .collect();
        assert_eq!(
            hyperlinks.len(),
            1,
            "{target}: source relationship duplicated"
        );
        assert_eq!(hyperlinks[0].attribute("Target"), Some(target));
        let reparsed = parse_document(&output, &ParseOptions::default()).expect("reparse");
        let again =
            write_package(&reparsed, Some(&output), &WriteOptions::default()).expect("rewrite");
        assert_eq!(
            written.bytes, again.bytes,
            "relationship allocation must be a fixed point"
        );
    }
}
