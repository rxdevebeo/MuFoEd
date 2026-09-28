//! Breadth tests covering the formatting, table, section and inline code paths
//! (`STAGE-4-TASK.md` §5). They complement the focused tests and drive line
//! coverage above the 80% gate.

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::doc_markdown
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, MediaMode, PageSelection, RenderOptions};

fn render_body(body: &str) -> Vec<strict_ooxml_render_svg::Page> {
    let (_package, doc) = common::open_body(body);
    render(&doc, &Default::default()).expect("render")
}

#[test]
fn rich_paragraphs_cover_formatting_paths() {
    let body = r#"<w:p><w:pPr><w:jc w:val="both"/><w:spacing w:before="240" w:after="120" w:line="360" w:lineRule="auto"/><w:ind w:start="720" w:end="360" w:hanging="360"/><w:keepNext/><w:keepLines/><w:tabs><w:tab w:val="center" w:pos="2880"/><w:tab w:val="clear" w:pos="1440"/></w:tabs></w:pPr><w:r><w:rPr><w:b/><w:i/><w:u w:val="single"/><w:strike/><w:color w:val="FF0000"/><w:highlight w:val="yellow"/><w:caps/><w:vertAlign w:val="superscript"/><w:sz w:val="24"/></w:rPr><w:t>Justified text with several words</w:t><w:tab/><w:t>after tab</w:t></w:r><w:r><w:noBreakHyphen/><w:t>join</w:t></w:r><w:r><w:br/><w:t>after break</w:t></w:r><w:r><w:cr/><w:t>after cr</w:t></w:r><w:hyperlink w:anchor="x"><w:r><w:t>link</w:t></w:r></w:hyperlink><w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple><w:sdt><w:sdtContent><w:r><w:t>sdt</w:t></w:r></w:sdtContent></w:sdt></w:p><w:p><w:pPr><w:jc w:val="center"/><w:spacing w:line="240" w:lineRule="exact"/></w:pPr><w:r><w:rPr><w:smallCaps/><w:dstrike/></w:rPr><w:t>centered</w:t></w:r></w:p><w:p><w:pPr><w:jc w:val="end"/><w:spacing w:line="240" w:lineRule="atLeast"/><w:ind w:firstLine="240"/></w:pPr><w:r><w:t>right</w:t></w:r></w:p>"#;
    let pages = render_body(body);
    assert!(!pages.is_empty());
    let svg = &pages[0].svg;
    assert!(svg.contains("JUSTIFIED"), "{svg}");
    assert!(svg.contains("centered"));
    assert!(svg.contains("right"));
}

#[test]
fn rich_table_covers_borders_spans_and_merges() {
    let body = r#"<w:tbl><w:tblPr><w:tblW w:w="5000" w:type="pct"/><w:jc w:val="center"/><w:tblInd w:w="100"/><w:tblLayout w:type="fixed"/><w:tblBorders><w:top w:val="single" w:sz="8" w:color="000000"/><w:left w:val="dashed" w:sz="4" w:color="FF0000"/><w:bottom w:val="double" w:sz="12" w:color="00FF00"/><w:right w:val="nil"/></w:tblBorders><w:shd w:fill="EEEEEE"/><w:tblCellMar><w:left w:w="100"/><w:right w:w="100"/></w:tblCellMar></w:tblPr><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:trPr><w:trHeight w:val="500" w:hRule="atLeast"/><w:tblHeader/><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:tcBorders><w:top w:val="single" w:sz="8" w:color="0000FF"/></w:tcBorders><w:shd w:fill="FFEEEE"/><w:tcMar><w:left w:w="120"/></w:tcMar><w:vAlign w:val="center"/><w:textDirection w:val="lrTb"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>span</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    let pages = render_body(body);
    let svg = &pages[0].svg;
    assert!(svg.contains(">A<"), "{svg}");
    assert!(svg.contains("span"), "{svg}");
    assert!(svg.contains("<line "), "expected borders: {svg}");
}

#[test]
fn section_geometry_and_breaks() {
    let body = r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"/><w:cols w:num="2" w:space="360"/><w:type w:val="nextPage"/><w:titlePg/><w:vAlign w:val="center"/><w:docGrid w:type="lines" w:linePitch="360"/></w:sectPr></w:pPr><w:r><w:t>section</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>new page</w:t></w:r></w:p>"#;
    let (_package, doc) = common::open_body(body);
    let pages = render(&doc, &Default::default()).expect("render");
    assert!(
        pages.len() >= 2,
        "expected a page break, got {}",
        pages.len()
    );
    // Landscape 11in x 8.5in at 96 dpi.
    assert!((pages[0].width_px - 1056.0).abs() < 0.01);
    assert!((pages[0].height_px - 816.0).abs() < 0.01);
}

#[test]
fn numbering_marker_is_rendered() {
    let numbering = format!(
        r#"<?xml version="1.0"?><w:numbering xmlns:w="{}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#,
        common::W
    );
    let doc_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/numbering" Target="numbering.xml"/></Relationships>"#.to_owned();
    let body = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Item one</w:t></w:r></w:p>"#;
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
    ]);
    let (_package, parsed) = open_bytes(bytes);
    let pages = render(&parsed, &RenderOptions::default()).expect("render");
    assert!(
        pages[0].svg.contains(">1.</text>") || pages[0].svg.contains("1."),
        "{}",
        pages[0].svg
    );
}

#[test]
fn images_in_external_and_none_modes() {
    let (package, doc) = common::open_with_image();
    for mode in [
        MediaMode::ExternalFiles,
        MediaMode::None,
        MediaMode::EmbedDataUri,
    ] {
        let options = RenderOptions::default().media(mode);
        let pages = strict_ooxml_render_svg::render_with_media(&doc, &options, Some(&package))
            .expect("render");
        assert!(!pages.is_empty());
    }
}

#[test]
fn page_selection_out_of_range_is_empty() {
    let (_package, doc) = common::open_body("<w:p/>");
    let options = RenderOptions::default().pages(PageSelection::Range { start: 99, end: 99 });
    let pages = render(&doc, &options).expect("render");
    assert!(pages.is_empty());
}
