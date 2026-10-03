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
    parse_document(&package, &ParseOptions { limits })
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
    let blip = inline.picture().unwrap().blip.as_ref().unwrap();
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

/// `depth` tables, each holding the next in its only cell.
fn tables(depth: usize) -> String {
    let mut out = String::new();
    for _ in 0..depth {
        out.push_str("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc>");
    }
    out.push_str("<w:p/>");
    for _ in 0..depth {
        out.push_str("</w:tc></w:tr></w:tbl><w:p/>");
    }
    out
}

#[test]
fn block_nesting_is_bounded_by_its_own_limit_not_by_xml_depth() {
    // Twelve tables are thirty-odd XML elements per level - far inside
    // `max_xml_depth` of 256 - and they used to be accepted at any depth, which
    // is how forty of them overflowed a 1 MiB stack. The bound is its own field
    // because the two answer different questions.
    let twelve = parse_with_limits(&document_parts(&tables(12), &[]), ResourceLimits::default())
        .expect("twelve tables fit the default budget");
    assert!(
        matches!(twelve.body.blocks.first(), Some(Block::Table(_))),
        "the body opens with the outermost table: {:?}",
        twelve.body.blocks.len()
    );

    let limits = ResourceLimits {
        max_block_nesting: 4,
        ..ResourceLimits::default()
    };
    let result = parse_with_limits(&document_parts(&tables(5), &[]), limits);
    match result {
        Err(StrictError::LimitExceeded {
            kind: LimitKind::BlockNesting,
            limit,
            actual,
        }) => assert_eq!((limit, actual), (4, 5)),
        other => panic!("expected BlockNesting, got {other:?}"),
    }
}

#[test]
fn the_block_counter_is_shared_across_container_kinds() {
    // A table and a content control in rotation: six of each is twelve levels
    // and fits. Two independent counters would let twenty-four through.
    let mut body = String::new();
    for _ in 0..6 {
        body.push_str("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:sdt><w:sdtContent>");
    }
    body.push_str("<w:p/>");
    for _ in 0..6 {
        body.push_str("</w:sdtContent></w:sdt></w:tc></w:tr></w:tbl><w:p/>");
    }
    parse_with_limits(&document_parts(&body, &[]), ResourceLimits::default())
        .expect("twelve levels of mixed containers fit");

    let mut too_deep = String::new();
    for _ in 0..7 {
        too_deep.push_str("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:sdt><w:sdtContent>");
    }
    too_deep.push_str("<w:p/>");
    for _ in 0..7 {
        too_deep.push_str("</w:sdtContent></w:sdt></w:tc></w:tr></w:tbl><w:p/>");
    }
    let result = parse_with_limits(&document_parts(&too_deep, &[]), ResourceLimits::default());
    assert!(
        matches!(
            result,
            Err(StrictError::LimitExceeded {
                kind: LimitKind::BlockNesting,
                ..
            })
        ),
        "the thirteenth level must be refused, got {result:?}"
    );
}

/// `depth` text boxes, each holding the next in its `w:txbxContent`.
fn text_boxes(depth: usize) -> String {
    let ns = concat!(
        " xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"",
        " xmlns:r=\"http://purl.oclc.org/ooxml/officeDocument/relationships\"",
        " xmlns:wp=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing\"",
        " xmlns:a=\"http://purl.oclc.org/ooxml/drawingml/main\"",
        " xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\""
    );
    let open = format!(
        "<w:p><w:r><w:drawing><wp:inline{ns}>\
         <wp:extent cx=\"914400\" cy=\"914400\"/><wp:docPr id=\"1\" name=\"box\"/>\
         <a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
         <wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>"
    );
    let close = "</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData>\
         </a:graphic></wp:inline></w:drawing></w:r></w:p>";
    let mut out = open.repeat(depth);
    out.push_str("<w:p/>");
    out.push_str(&close.repeat(depth));
    out
}

#[test]
fn text_box_nesting_has_its_own_budget_and_its_own_kind() {
    // Five text boxes are inside the budget of five and eleven tables would be
    // inside the budget of twelve at the same time - two counters, because a
    // text box is ten frames of parser state and a table is one.
    let five = parse_with_limits(
        &document_parts(&text_boxes(5), &[]),
        ResourceLimits::default(),
    )
    .expect("five text boxes fit the budget of five");
    assert_eq!(five.body.blocks.len(), 1);

    // The sixth box costs its content, not the document.
    let six = parse_with_limits(
        &document_parts(&text_boxes(6), &[]),
        ResourceLimits::default(),
    )
    .expect("a text box past the budget is skipped, not refused");
    assert_eq!(six.body.blocks.len(), 1);
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

/// AUD-51: feature-key budget and namespace-key (not document prefix).
#[test]
fn support_model_caps_distinct_features_and_ignores_spoofed_prefix() {
    // Many unique unknown children → collapse into support.overflow.
    let mut body = String::from("<w:p><w:r><w:t>x</w:t></w:r>");
    for index in 0..200 {
        body.push_str(&format!("<w:zzz{index}/>"));
    }
    body.push_str("</w:p>");
    let limits = ResourceLimits {
        max_support_features: 50,
        ..ResourceLimits::default()
    };
    let document = parse_with_limits(&document_parts(&body, &[]), limits).expect("parse");
    assert!(
        document.support.len() <= 51,
        "len={}",
        document.support.len()
    );
    assert!(document
        .support
        .get(strict_ooxml_wml::model::SUPPORT_OVERFLOW_ID)
        .is_some());

    // A `w:` prefix rebound to a foreign URI must not produce a `w:` feature key.
    let body = "<w:p><w:r><w:t>x</w:t></w:r>\
        <w:mystery xmlns:w=\"http://evil.example/ns\"/>\
        </w:p>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    assert!(document.support.get("w:mystery").is_none());
    assert!(document
        .support
        .iter()
        .any(|f| f.feature_id.as_ref() == "ext:http://evil.example/ns:mystery"));
}
