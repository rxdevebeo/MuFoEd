#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for the document body, paragraphs, runs and inlines.

mod common;

use strict_ooxml_wml::model::block::{Block, OpaqueBlock};
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::{BreakKind, Justification, TriState, Underline};
use strict_ooxml_wml::model::{Paragraph, Table};

use common::{document_parts, parse_parts, R_NS};

fn parse_body(body: &str) -> strict_ooxml_wml::model::Document {
    parse_parts(&document_parts(body, &[])).expect("parse document")
}

fn first_paragraph(document: &strict_ooxml_wml::model::Document) -> &Paragraph {
    match &document.body.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn parses_text_runs_and_space() {
    let document = parse_body(
        "<w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t xml:space=\"preserve\"> world</w:t></w:r></w:p>",
    );
    let paragraph = first_paragraph(&document);
    assert_eq!(paragraph.inlines.len(), 2);
    let Inline::Run(run) = &paragraph.inlines[0] else {
        panic!("expected run");
    };
    match &run.content[0] {
        RunContent::Text(text) => {
            assert_eq!(text.text, "Hello ");
            assert_eq!(text.space, strict_ooxml_wml::model::values::Space::Default);
        }
        other => panic!("unexpected {other:?}"),
    }
    let Inline::Run(run) = &paragraph.inlines[1] else {
        panic!("expected run");
    };
    match &run.content[0] {
        RunContent::Text(text) => {
            assert_eq!(text.text, " world");
            assert_eq!(text.space, strict_ooxml_wml::model::values::Space::Preserve);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_tabs_breaks_and_carriage_returns() {
    let document = parse_body(
        "<w:p><w:r><w:t>a</w:t><w:tab/><w:br w:type=\"page\"/><w:br/><w:cr/><w:noBreakHyphen/><w:softHyphen/></w:r></w:p>",
    );
    let paragraph = first_paragraph(&document);
    let Inline::Run(run) = &paragraph.inlines[0] else {
        panic!("expected run");
    };
    assert!(matches!(run.content[1], RunContent::Tab));
    assert!(matches!(run.content[2], RunContent::Break(BreakKind::Page)));
    assert!(matches!(
        run.content[3],
        RunContent::Break(BreakKind::TextWrapping)
    ));
    assert!(matches!(run.content[4], RunContent::CarriageReturn));
    assert!(matches!(run.content[5], RunContent::NoBreakHyphen));
    assert!(matches!(run.content[6], RunContent::SoftHyphen));
}

#[test]
fn parses_paragraph_properties() {
    let document = parse_body(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/><w:jc w:val=\"center\"/><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"7\"/></w:numPr><w:spacing w:before=\"240\" w:after=\"120\" w:line=\"360\" w:lineRule=\"auto\"/><w:ind w:start=\"720\" w:hanging=\"360\"/><w:keepNext/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>",
    );
    let props = &first_paragraph(&document).props;
    assert_eq!(props.style.as_ref().unwrap().as_str(), "Heading1");
    assert_eq!(props.alignment, Some(Justification::Center));
    let numbering = props.numbering.unwrap();
    assert_eq!(numbering.num_id.unwrap().0, 7);
    assert_eq!(numbering.ilvl.unwrap().0, 1);
    let spacing = props.spacing.unwrap();
    assert_eq!(spacing.before.unwrap().value(), 240);
    assert_eq!(spacing.line.unwrap().value(), 360);
    let indentation = props.indentation.unwrap();
    assert_eq!(indentation.start.unwrap().value(), 720);
    assert_eq!(indentation.hanging.unwrap().value(), 360);
    assert!(props.keep_next);
}

#[test]
fn parses_run_properties() {
    let document = parse_body(
        "<w:p><w:r><w:rPr><w:rStyle w:val=\"Emph\"/><w:b/><w:i w:val=\"false\"/><w:u w:val=\"double\"/><w:color w:val=\"FF0000\"/><w:sz w:val=\"28\"/><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/></w:rPr><w:t>x</w:t></w:r></w:p>",
    );
    let Inline::Run(run) = &first_paragraph(&document).inlines[0] else {
        panic!("expected run");
    };
    assert_eq!(run.props.style.as_ref().unwrap().as_str(), "Emph");
    assert_eq!(run.props.bold, TriState::On);
    assert_eq!(run.props.italic, TriState::Off);
    assert_eq!(run.props.underline, Some(Underline::Double));
    assert_eq!(run.props.color.as_ref().unwrap().as_str(), "FF0000");
    assert_eq!(run.props.size.unwrap().value(), 28);
    let fonts = run.props.fonts.as_ref().unwrap();
    assert_eq!(fonts.ascii.as_deref(), Some("Arial"));
}

#[test]
fn parses_table_grid_rows_and_merges() {
    let document = parse_body(
        "<w:tbl><w:tblPr><w:tblStyle w:val=\"Grid\"/><w:tblW w:w=\"5000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2500\"/><w:gridCol w:w=\"2500\"/></w:tblGrid>\
<w:tr><w:trPr><w:trHeight w:val=\"400\" w:hRule=\"atLeast\"/></w:trPr>\
<w:tc><w:tcPr><w:tcW w:w=\"2500\" w:type=\"dxa\"/><w:shd w:val=\"clear\" w:fill=\"EEEEEE\"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:gridSpan w:val=\"1\"/><w:vMerge w:val=\"restart\"/></w:tcPr><w:p/></w:tc>\
</w:tr></w:tbl>",
    );
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    let table: &Table = table;
    assert_eq!(table.grid.len(), 2);
    assert_eq!(table.grid[0].width.unwrap().value(), 2500);
    assert_eq!(table.rows.len(), 1);
    let row = &table.rows[0];
    assert_eq!(row.cells.len(), 2);
    assert_eq!(row.props.height.unwrap().value.unwrap().value(), 400);
    assert_eq!(row.cells[0].props.width.unwrap().value, Some(2500));
    assert_eq!(
        row.cells[1].props.vertical_merge,
        Some(strict_ooxml_wml::model::values::VerticalMerge::Restart)
    );
    assert_eq!(row.cells[0].blocks.len(), 1);
}

#[test]
fn parses_hyperlinks_bookmarks_and_fields() {
    let body = format!(
        "<w:p>\
<w:bookmarkStart w:id=\"0\" w:name=\"mark\"/>\
<w:hyperlink r:id=\"rId9\" w:anchor=\"target\"><w:r><w:t>link</w:t></w:r></w:hyperlink>\
<w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
<w:bookmarkEnd w:id=\"0\"/>\
<w:r><w:fldChar w:fldCharType=\"begin\"/><w:instrText> PAGE </w:instrText><w:fldChar w:fldCharType=\"end\"/></w:r>\
</w:p>"
    );
    let document = parse_body(&body);
    let inlines = &first_paragraph(&document).inlines;
    assert!(matches!(inlines[0], Inline::BookmarkStart(_)));
    let Inline::Hyperlink(link) = &inlines[1] else {
        panic!("expected hyperlink");
    };
    assert_eq!(link.rel_id.as_ref().unwrap().as_str(), "rId9");
    assert_eq!(link.anchor.as_deref(), Some("target"));
    let Inline::Field(field) = &inlines[2] else {
        panic!("expected field");
    };
    assert_eq!(field.instruction.as_deref(), Some(" PAGE "));
    assert!(matches!(inlines[3], Inline::BookmarkEnd(_)));
    let Inline::Run(run) = &inlines[4] else {
        panic!("expected run");
    };
    assert!(matches!(run.content[0], RunContent::FieldChar(_)));
    assert!(matches!(run.content[1], RunContent::InstrText(_)));
    assert_eq!(
        R_NS,
        "http://purl.oclc.org/ooxml/officeDocument/relationships"
    );
}

#[test]
fn parses_block_and_inline_sdt() {
    let body = "<w:sdt><w:sdtPr><w:tag w:val=\"T\"/><w:alias w:val=\"Alias\"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>inside</w:t></w:r></w:p></w:sdtContent></w:sdt>";
    let document = parse_body(body);
    let Block::SdtBlock(container) = &document.body.blocks[0] else {
        panic!("expected sdt block");
    };
    assert_eq!(container.tag.as_deref(), Some("T"));
    assert_eq!(container.alias.as_deref(), Some("Alias"));
    assert_eq!(container.blocks.len(), 1);

    let body = "<w:p><w:sdt><w:sdtContent><w:r><w:t>x</w:t></w:r></w:sdtContent></w:sdt></w:p>";
    let document = parse_body(body);
    let inlines = &first_paragraph(&document).inlines;
    assert!(matches!(inlines[0], Inline::SdtInline(_)));
}

#[test]
fn unknown_element_becomes_opaque_and_is_recorded() {
    let document = parse_parts(&document_parts(
        "<w:p><w:r><w:t>x</w:t></w:r><w:mystery w:val=\"1\"/></w:p>",
        &[],
    ))
    .expect("parse");
    let inlines = &first_paragraph(&document).inlines;
    assert!(matches!(inlines[1], Inline::Opaque(_)));
    let feature = document.support.get("w:mystery").expect("recorded");
    assert_eq!(feature.status, SupportStatus::Unsupported);
}

#[test]
fn unknown_block_becomes_opaque() {
    let document = parse_parts(&document_parts("<w:weirdBlock/>", &[])).expect("parse document");
    assert!(matches!(document.body.blocks[0], Block::Opaque(_)));
    let Block::Opaque(OpaqueBlock { local, .. }) = &document.body.blocks[0] else {
        unreachable!()
    };
    assert_eq!(local.as_ref(), "weirdBlock");
}

#[test]
fn alt_chunk_is_recorded_but_not_embedded() {
    let parts = document_parts(
        "<w:altChunk r:id=\"rIdChunk\"/>",
        &[(
            "word/_rels/document.xml.rels",
            common::rels(&[(
                "rIdChunk",
                "http://purl.oclc.org/ooxml/officeDocument/relationships/aFChunk",
                "afchunk.html",
            )]),
        )],
    );
    let document = parse_parts(&parts).expect("parse");
    let Block::AltChunk(info) = &document.body.blocks[0] else {
        panic!("expected altChunk");
    };
    assert!(info.rel_id.is_some());
    assert_eq!(
        document.support.get("w:altChunk").unwrap().status,
        SupportStatus::Unsupported
    );
}

#[test]
fn invalid_enum_value_is_recorded_and_defaulted() {
    let document = parse_body("<w:p><w:pPr><w:jc w:val=\"sideways\"/></w:pPr></w:p>");
    let paragraph = first_paragraph(&document);
    assert_eq!(paragraph.props.alignment, None);
    assert_eq!(
        document.support.get("w:jc").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn mce_elements_are_ignored_not_opaque_errors() {
    let body = format!(
        "<w:p><mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice Requires=\"w\"/></mc:AlternateContent></w:p>"
    );
    let document = parse_body(&body);
    assert_eq!(
        document.support.get("mc:AlternateContent").unwrap().status,
        SupportStatus::Ignored
    );
}

#[test]
fn footnote_reference_is_fixed_and_recorded() {
    let document = parse_body("<w:p><w:r><w:footnoteReference w:id=\"3\"/></w:r></w:p>");
    let inlines = &first_paragraph(&document).inlines;
    let Inline::Run(run) = &inlines[0] else {
        panic!("expected run");
    };
    assert!(matches!(run.content[0], RunContent::FootnoteRef(3)));
    assert_eq!(
        document.support.get("w:footnoteReference").unwrap().status,
        SupportStatus::Unsupported
    );
}

#[test]
fn tracks_change_containers_are_flattened() {
    let document = parse_body(
        "<w:p><w:ins w:id=\"1\"><w:r><w:t>added</w:t></w:r></w:ins><w:del w:id=\"2\"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>",
    );
    let paragraph = first_paragraph(&document);
    assert_eq!(paragraph.inlines.len(), 2);
    assert_eq!(
        document.support.get("w:ins").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn paragraph_locations_are_tracked() {
    let body = "<w:p><w:r><w:t>x</w:t></w:r></w:p>\n<w:p><w:r><w:t>y</w:t></w:r></w:p>";
    let document = parse_body(body);
    let Block::Paragraph(first) = &document.body.blocks[0] else {
        panic!("expected paragraph");
    };
    let Block::Paragraph(second) = &document.body.blocks[1] else {
        panic!("expected paragraph");
    };
    assert_eq!(first.location.line, 1);
    assert_eq!(second.location.line, 2);
    assert!(first.location.byte_offset < second.location.byte_offset);
}
