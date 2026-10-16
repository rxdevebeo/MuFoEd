//! A hyperlink a converter generated (no source package) gets an external
//! relationship of its own (SC-D9).

// Spelling the fields out is the point: this is a whole `Document`, and the
// lint that objects would have me hide which parts of it are default.
#![allow(clippy::default_trait_access)]

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::document::DocumentSource;
use strict_ooxml_wml::model::inline::{Hyperlink, Inline, Run, RunContent, TextNode};
use strict_ooxml_wml::model::props::{ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::values::{Rsids, Space};
use strict_ooxml_wml::model::Document;
use strict_ooxml_write::{write_package, WriteOptions};

const URL: &str = "https://example.invalid/djvu-link";

fn document_with_target() -> Document {
    Document {
        body: Default::default(),
        styles: Default::default(),
        numbering: Default::default(),
        footnotes: Default::default(),
        endnotes: Default::default(),
        comments: Vec::new(),
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
            comments: None,
            theme: None,
        },
    }
}

fn paragraph_with_target() -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Hyperlink(Hyperlink {
            rel_id: None,
            target: Some(Arc::from(URL)),
            anchor: None,
            tooltip: None,
            history: false,
            tgt_frame: None,
            inlines: vec![Inline::Run(Run {
                props: RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: "link".to_owned(),
                    space: Space::Preserve,
                })],
                revision: None,
                location: SourceLocation::unknown(),
            })],
            location: SourceLocation::unknown(),
        })],
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: SourceLocation::unknown(),
    }
}

fn part(output: &Package, name: &str) -> String {
    let raw = output
        .read_part(&PartId::new(name))
        .expect("part is present");
    String::from_utf8(raw).expect("UTF-8")
}

#[test]
fn a_generated_hyperlink_gets_its_own_external_relationship() {
    let mut document = document_with_target();
    document
        .body
        .blocks
        .push(Block::Paragraph(paragraph_with_target()));
    let written = write_package(&document, None, &WriteOptions::default()).expect("write");
    let output = Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");

    let document_xml = part(&output, "/word/document.xml");
    let rels = part(&output, "/word/_rels/document.xml.rels");

    let rel_doc = roxmltree::Document::parse(&rels).expect("rels XML");
    let links: Vec<_> = rel_doc
        .descendants()
        .filter(|node| {
            node.attribute("Type")
                .is_some_and(|value| value.ends_with("/hyperlink"))
        })
        .collect();
    assert_eq!(links.len(), 1, "one external hyperlink relationship");
    assert_eq!(links[0].attribute("Target"), Some(URL));
    assert_eq!(links[0].attribute("TargetMode"), Some("External"));
    let id = links[0].attribute("Id").expect("id");

    assert!(
        document_xml.contains(&format!("r:id=\"{id}\"")),
        "the hyperlink references its relationship: {document_xml}"
    );
    assert_eq!(document_xml.matches("<w:hyperlink").count(), 1);
}
