#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Coverage tests for parser edge branches, robustness and model sizes.

mod common;

use std::io::Cursor;
use std::mem::size_of;

use strict_ooxml_core::error::{LimitKind, StrictError};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::drawing::DrawingKind;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

use common::{document_parts, package_entries, parse_parts, W_NS};

fn parse_with_limits(
    parts: &[(String, Vec<u8>)],
    limits: ResourceLimits,
) -> strict_ooxml_core::error::Result<Document> {
    let bytes = package_entries(parts);
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default().limits(limits))?;
    parse_document(
        &package,
        &ParseOptions {
            conformance: strict_ooxml_core::opc::ConformancePolicy::StrictOnly,
            limits,
        },
    )
}

#[test]
fn parses_inline_comment_and_note_references() {
    let body = "<w:p>\
<w:commentRangeStart w:id=\"1\"/><w:commentRangeEnd w:id=\"1\"/><w:commentReference w:id=\"1\"/>\
<w:footnoteReference w:id=\"2\"/><w:endnoteReference w:id=\"3\"/>\
<w:drawing/>\
<w:hyperlink w:anchor=\"anchor\" w:tooltip=\"tip\"><w:r><w:t>t</w:t></w:r></w:hyperlink>\
</w:p>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    let inlines = &document.body.blocks[0].as_paragraph().unwrap().inlines;
    assert!(matches!(inlines[0], Inline::CommentRangeStart(_)));
    assert!(matches!(inlines[1], Inline::CommentRangeEnd(_)));
    assert!(matches!(inlines[2], Inline::CommentReference(_)));
    assert!(matches!(inlines[3], Inline::FootnoteRef(2)));
    assert!(matches!(inlines[4], Inline::EndnoteRef(3)));
    let Inline::Drawing(drawing) = &inlines[5] else {
        panic!("expected drawing");
    };
    assert!(matches!(drawing.kind, DrawingKind::Opaque(_)));
    let Inline::Hyperlink(link) = &inlines[6] else {
        panic!("expected hyperlink");
    };
    assert_eq!(link.anchor.as_deref(), Some("anchor"));
    assert_eq!(link.tooltip.as_deref(), Some("tip"));
    assert!(link.rel_id.is_none());
    for feature in [
        "w:commentReference",
        "w:footnoteReference",
        "w:endnoteReference",
    ] {
        assert!(document.support.get(feature).is_some(), "missing {feature}");
    }
}

#[test]
fn parses_body_background_and_foreign_elements() {
    let body = format!(
        "<w:background w:color=\"FFFFFF\"/>\
<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice/></mc:AlternateContent>\
<w:p/>"
    );
    let document = parse_parts(&document_parts(&body, &[])).expect("parse");
    assert!(document.support.get("w:background").is_some());
    assert_eq!(
        document.support.get("mc:AlternateContent").unwrap().status,
        SupportStatus::Ignored
    );
    assert!(document
        .body
        .blocks
        .iter()
        .any(|block| block.as_paragraph().is_some()));
}

#[test]
fn parses_sdt_with_placeholder_and_end_pr() {
    let body = "<w:sdt><w:sdtPr><w:tag w:val=\"T\"/><w:showingPlcHdr/><w:placeholder/></w:sdtPr>\
<w:sdtEndPr/><w:sdtContent><w:p><w:r><w:t>x</w:t></w:r></w:p></w:sdtContent></w:sdt>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    let Block::SdtBlock(container) = &document.body.blocks[0] else {
        panic!("expected sdt");
    };
    assert!(container.showing_placeholder);
    assert!(container.placeholder.is_some());
    assert_eq!(container.blocks.len(), 1);
}

#[test]
fn parses_table_cell_change_containers() {
    let body = "<w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc>\
<w:ins w:id=\"1\"><w:p><w:r><w:t>a</w:t></w:r></w:p></w:ins>\
<w:del w:id=\"2\"><w:p/></w:del>\
<w:customXml><w:p/></w:customXml>\
<w:altChunk r:id=\"rId1\"/>\
</w:tc></w:tr></w:tbl>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    assert!(!table.rows[0].cells[0].blocks.is_empty());
}

#[test]
fn parses_drawing_link_without_embed() {
    let body = "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1\" cy=\"2\"/>\
<a:graphic><a:graphicData uri=\"urn:pic\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"n\"/></pic:nvPicPr>\
<pic:blipFill><a:blip r:link=\"rIdLink\"/></pic:blipFill><pic:spPr><a:ext cx=\"3\" cy=\"4\"/></pic:spPr></pic:pic></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    let Inline::Run(run) = &document.body.blocks[0].as_paragraph().unwrap().inlines[0] else {
        panic!("expected run");
    };
    let RunContent::Drawing(drawing) = &run.content[0] else {
        panic!("expected drawing");
    };
    let DrawingKind::Inline(inline) = &drawing.kind else {
        panic!("expected inline");
    };
    let blip = inline.picture.as_ref().unwrap().blip.as_ref().unwrap();
    assert!(blip.embed.is_none());
    assert_eq!(blip.link.as_ref().unwrap().as_str(), "rIdLink");
    assert!(document.media.is_empty());
}

#[test]
fn parses_cdata_text() {
    let body = "<w:p><w:r><w:t><![CDATA[<raw>]]></w:t></w:r></w:p>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    let Inline::Run(run) = &document.body.blocks[0].as_paragraph().unwrap().inlines[0] else {
        panic!("expected run");
    };
    let RunContent::Text(text) = &run.content[0] else {
        panic!("expected text");
    };
    assert_eq!(text.text, "<raw>");
}

#[test]
fn depth_limit_is_enforced_without_panic() {
    let depth = 40usize;
    let body = format!(
        "{}{}{}",
        "<w:ins w:id=\"1\">".repeat(depth),
        "<w:p/>",
        "</w:ins>".repeat(depth)
    );
    let limits = ResourceLimits {
        max_xml_depth: 16,
        ..ResourceLimits::default()
    };
    let result = parse_with_limits(&document_parts(&body, &[]), limits);
    assert!(
        matches!(
            result,
            Err(StrictError::LimitExceeded {
                kind: LimitKind::XmlDepth,
                ..
            })
        ),
        "expected XmlDepth limit, got {result:?}"
    );
}

#[test]
fn truncated_document_is_an_error_not_a_panic() {
    let document = format!("<w:document xmlns:w=\"{W_NS}\"><w:body><w:p><w:r><w:t>x").into_bytes();
    let parts = vec![("word/document.xml".to_owned(), document)];
    assert!(parse_parts(&parts).is_err());
}

#[test]
fn wrong_root_element_is_an_error() {
    let body = format!("<w:unexpected xmlns:w=\"{W_NS}\"/>").into_bytes();
    let parts = vec![("word/document.xml".to_owned(), body)];
    assert!(parse_parts(&parts).is_err());
}

#[test]
fn wrong_styles_root_is_an_error() {
    let styles = format!("<w:notstyles xmlns:w=\"{W_NS}\"/>").into_bytes();
    let parts = document_parts(
        "<w:p/>",
        &[
            ("word/styles.xml", styles),
            (
                "word/_rels/document.xml.rels",
                common::rels(&[(
                    "rIdStyles",
                    "http://purl.oclc.org/ooxml/officeDocument/relationships/styles",
                    "styles.xml",
                )]),
            ),
        ],
    );
    assert!(parse_parts(&parts).is_err());
}

#[test]
fn model_variant_sizes_are_bounded() {
    // Justifies the `large_enum_variant` allowance (ADR-0004 / REWORK M9).
    assert!(
        size_of::<Block>() < 2048,
        "Block is {} bytes",
        size_of::<Block>()
    );
    assert!(
        size_of::<Inline>() < 1024,
        "Inline is {} bytes",
        size_of::<Inline>()
    );
}
