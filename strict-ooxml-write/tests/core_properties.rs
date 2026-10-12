//! A document built in code can carry `docProps/core.xml` (SC-D9 / L4).
#![allow(clippy::default_trait_access)]

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::document::DocumentSource;
use strict_ooxml_wml::model::Document;
use strict_ooxml_write::{write_package, CoreProperties, WriteOptions};

fn empty_document() -> Document {
    Document {
        body: Default::default(),
        styles: Default::default(),
        numbering: Default::default(),
        footnotes: Default::default(),
        endnotes: Default::default(),
        settings: Default::default(),
        font_table: None,
        theme: None,
        sections: Vec::new(),
        headers_footers: Vec::new(),
        media: Default::default(),
        support: Default::default(),
        source: DocumentSource {
            main_document: PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            font_table: None,
            footnotes: None,
            endnotes: None,
            theme: None,
        },
    }
}

fn read(output: &Package, name: &str) -> String {
    String::from_utf8(
        output
            .read_part(&PartId::new(name))
            .unwrap_or_else(|_| panic!("{name} is present")),
    )
    .expect("UTF-8")
}

#[test]
fn core_properties_are_written_and_related() {
    let options = WriteOptions {
        core_properties: Some(CoreProperties {
            title: Some("A & B <Book>".to_owned()),
            creator: Some("A. Uthor".to_owned()),
            ..CoreProperties::default()
        }),
        ..WriteOptions::default()
    };
    let written = write_package(&empty_document(), None, &options).expect("write");
    let output = Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");

    let core = read(&output, "/docProps/core.xml");
    assert!(
        core.contains("<dc:title>A &amp; B &lt;Book&gt;</dc:title>"),
        "{core}"
    );
    assert!(core.contains("<dc:creator>A. Uthor</dc:creator>"), "{core}");
    assert!(
        core.contains("package/2006/metadata/core-properties"),
        "standard OPC namespace (ADR-0015): {core}"
    );

    let rels = read(&output, "/_rels/.rels");
    assert!(rels.contains("docProps/core.xml"), "{rels}");
    assert!(rels.contains("core-properties"), "{rels}");

    let types = read(&output, "/[Content_Types].xml");
    assert!(types.contains("core-properties+xml"), "{types}");
}
