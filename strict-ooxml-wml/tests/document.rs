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

/// AUD-41: repeating-section `w:sdt` around three table rows unwraps into the model.
#[test]
fn row_level_sdt_unwraps_into_table_rows() {
    let body = "\
<w:tbl><w:tblGrid><w:gridCol w:w=\"1000\"/></w:tblGrid>\
<w:sdt><w:sdtPr><w:tag w:val=\"repeat\"/><w:alias w:val=\"Items\"/></w:sdtPr>\
<w:sdtContent>\
<w:tr><w:tc><w:p><w:r><w:t>one</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>three</w:t></w:r></w:p></w:tc></w:tr>\
</w:sdtContent></w:sdt></w:tbl>";
    let document = parse_body(body);
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    assert_eq!(table.rows.len(), 3);
    for (index, expected) in ["one", "two", "three"].iter().enumerate() {
        let row = &table.rows[index];
        let sdt = row.sdt.as_ref().expect("row keeps sdtPr");
        assert_eq!(sdt.tag.as_deref(), Some("repeat"));
        assert_eq!(sdt.alias.as_deref(), Some("Items"));
        let text = cell_text(&row.cells[0]);
        assert_eq!(text, *expected);
    }
    let entry = document.support.get("w:sdt").expect("partial record");
    assert_eq!(entry.status, SupportStatus::Partial);
    assert!(
        entry.message.as_deref().unwrap_or("").contains("unwrapped"),
        "message={:?}",
        entry.message
    );
}

/// AUD-41: cell-level `w:sdt` unwraps into ordinary cells with properties kept.
#[test]
fn cell_level_sdt_unwraps_into_table_cells() {
    let body = "\
<w:tbl><w:tblGrid><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"1000\"/></w:tblGrid>\
<w:tr>\
<w:sdt><w:sdtPr><w:tag w:val=\"c\"/></w:sdtPr>\
<w:sdtContent>\
<w:tc><w:p><w:r><w:t>left</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>right</w:t></w:r></w:p></w:tc>\
</w:sdtContent></w:sdt>\
</w:tr></w:tbl>";
    let document = parse_body(body);
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(cell_text(&table.rows[0].cells[0]), "left");
    assert_eq!(cell_text(&table.rows[0].cells[1]), "right");
    assert_eq!(
        table.rows[0].cells[0]
            .sdt
            .as_ref()
            .and_then(|s| s.tag.as_deref()),
        Some("c")
    );
}

fn cell_text(cell: &strict_ooxml_wml::model::block::TableCell) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        if let Block::Paragraph(paragraph) = block {
            for inline in &paragraph.inlines {
                if let Inline::Run(run) = inline {
                    for content in &run.content {
                        if let RunContent::Text(text) = content {
                            out.push_str(&text.text);
                        }
                    }
                }
            }
        }
    }
    out
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
        SupportStatus::Supported
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

/// A `w:commentReference` is a child of `w:r`, and that is the whole point.
///
/// It used to be dispatched through the INLINE table only, where a producer
/// never puts it, so the element became an Opaque and was dropped with a report
/// line - leaving `w:commentRangeStart`/`End` written around a comment that no
/// run pointed at. This asserts it lands in the RUN, which is where the markup
/// is and therefore the only place a writer arm could ever fire.
#[test]
fn a_comment_reference_is_parsed_as_run_content_not_an_inline() {
    let document = parse_body(
        "<w:p><w:commentRangeStart w:id=\"1\"/>\
         <w:r><w:t>text</w:t><w:commentReference w:id=\"1\"/></w:r>\
         <w:commentRangeEnd w:id=\"1\"/></w:p>",
    );
    let paragraph = first_paragraph(&document);

    let Inline::Run(run) = &paragraph.inlines[1] else {
        panic!(
            "expected the run between the range markers, got {:?}",
            paragraph.inlines[1]
        );
    };
    assert!(
        run.content
            .iter()
            .any(|content| matches!(content, RunContent::CommentReference(1))),
        "w:commentReference must survive as run content, not be reported and dropped: {:?}",
        run.content
    );
    // The element is Supported, not Ignored: the body is carried in
    // comments.xml, so the anchor is the only half this project owes.
    assert_eq!(
        document
            .support
            .get("w:commentReference")
            .expect("w:commentReference is recorded")
            .status,
        SupportStatus::Supported
    );
}
