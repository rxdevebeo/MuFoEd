//! Figures join the reading order where they stand on a single-column page.
//!
//! A semantic conversion used to append every picture and drawn shape after
//! all of its page's text, so a figure in the middle of a page landed below the
//! page's last paragraph.

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
fn a_figure_in_the_middle_of_a_page_stays_between_its_paragraphs() {
    let bytes = page_with_a_middle_figure();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let order = outline(&converted.document.body.blocks);
    assert_eq!(order, ["Above", "[figure]", "Below"], "{order:?}");
}
