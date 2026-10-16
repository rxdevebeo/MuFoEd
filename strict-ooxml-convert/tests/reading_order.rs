//! Figures stand where the page put them.
//!
//! A semantic conversion used to append every picture and drawn shape after
//! all of its page's text, so a figure in the middle of a page landed below the
//! page's last paragraph. Then they joined the flow; now they are anchored to
//! the page at their place.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::Block;

/// A Letter page: `Above` near the top, a filled rectangle in the middle,
/// `Below` near the bottom (PDF y-up coordinates).
fn page_with_a_middle_figure() -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    let stream = "BT /F1 12 Tf 72 700 Td (Above) Tj ET\n\
                  1 0 0 rg 72 400 200 100 re f\n\
                  BT /F1 12 Tf 72 200 Td (Below) Tj ET";
    pdf.page_with(stream.as_bytes(), &resources);
    pdf.build()
}

/// What each body block is: its text, or `[figure]` for a drawing.
fn outline(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| {
            let Block::Paragraph(paragraph) = block else {
                return None;
            };
            let mut text = String::new();
            let mut figure = false;
            for inline in &paragraph.inlines {
                match inline {
                    Inline::Drawing(_) => figure = true,
                    Inline::Run(run) => {
                        for content in &run.content {
                            match content {
                                RunContent::Text(node) => text.push_str(&node.text),
                                RunContent::Drawing(_) => figure = true,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            if figure {
                Some("[figure]".to_owned())
            } else if text.trim().is_empty() {
                None
            } else {
                Some(text.trim().to_owned())
            }
        })
        .collect()
}

#[test]
fn a_figure_in_the_middle_of_a_page_stands_where_the_page_put_it() {
    // A drawn shape is anchored to its page at its place, held by the page's
    // first paragraph; the text keeps its own order around it.
    let bytes = page_with_a_middle_figure();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let order = outline(&converted.document.body.blocks);
    assert_eq!(order, ["[figure]", "Below"], "{order:?}");
    let Some(Block::Paragraph(first)) = converted.document.body.blocks.first() else {
        panic!("no first paragraph");
    };
    let text: String = first
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => Some(run),
            _ => None,
        })
        .flat_map(|run| run.content.iter())
        .filter_map(|content| match content {
            RunContent::Text(node) => Some(node.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(text.trim(), "Above");
    let anchor = first.inlines.iter().find_map(|inline| match inline {
        Inline::Drawing(strict_ooxml_wml::model::drawing::Drawing {
            kind: strict_ooxml_wml::model::drawing::DrawingKind::Anchor(anchor),
            ..
        }) => Some(anchor),
        _ => None,
    });
    let anchor = anchor.expect("the shape is anchored");
    // 792 - (400 + 100) points from the page's top, behind the text.
    let top = anchor
        .position_v
        .as_ref()
        .and_then(|position| position.offset)
        .expect("a vertical offset");
    assert_eq!(top.0, 292 * 12_700);
    assert!(anchor.behind_doc);
}

#[test]
fn a_pdf_page_that_flows_on_is_marked_where_it_starts() {
    // `w:lastRenderedPageBreak`: Word's note that a page broke here, and how
    // the source's pages are found again in a flow without hard breaks.
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    pdf.page_with(b"BT /F1 12 Tf 72 700 Td (One) Tj ET", &resources);
    pdf.page_with(b"BT /F1 12 Tf 72 700 Td (Two) Tj ET", &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let marked: Vec<String> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .filter(|paragraph| {
            paragraph.inlines.iter().any(|inline| {
                matches!(inline, Inline::Run(run)
                    if run.content.iter().any(|content| matches!(content, RunContent::LastRenderedPageBreak)))
            })
        })
        .map(|paragraph| outline(&[Block::Paragraph(paragraph.clone())]).concat())
        .collect();
    assert_eq!(marked, ["Two"]);
}
