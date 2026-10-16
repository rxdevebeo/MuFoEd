//! A page laid out as a design keeps its blocks where they stood.
//!
//! Three blocks side by side, read as one flow, stack into three times the
//! page's height. They are text boxes anchored at their places instead.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::drawing::{Drawing, DrawingKind, Graphic};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::Block;

fn page(columns: &[f64]) -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    let mut stream = String::new();
    for (column, x) in columns.iter().enumerate() {
        for row in 0..20 {
            let y = 700 - row * 14;
            stream.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td (Block {column} line {row}) Tj ET\n"
            ));
        }
    }
    pdf.page_with(stream.as_bytes(), &resources);
    pdf.build()
}

/// The text boxes of the document, each with its paragraphs' count.
fn text_boxes(blocks: &[Block]) -> Vec<usize> {
    let mut out = Vec::new();
    for block in blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &paragraph.inlines {
            if let Inline::Drawing(Drawing {
                kind: DrawingKind::Anchor(anchor),
                ..
            }) = inline
            {
                if let Graphic::Shape(shape) = anchor.graphic.as_ref() {
                    if let Some(text) = &shape.text {
                        out.push(text.blocks.len());
                    }
                }
            }
        }
    }
    out
}

fn convert_page(bytes: &[u8]) -> strict_ooxml_wml::model::Document {
    let mut reader = PdfDocument::open(bytes, PdfLimits::default()).expect("open pdf");
    convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic))
        .expect("convert")
        .document
}

#[test]
fn blocks_side_by_side_are_text_boxes_at_their_places() {
    let document = convert_page(&page(&[40.0, 230.0, 420.0]));
    let boxes = text_boxes(&document.body.blocks);
    assert_eq!(boxes, [20, 20, 20], "{boxes:?}");
}

#[test]
fn a_page_of_running_text_stays_a_flow() {
    let document = convert_page(&page(&[72.0]));
    assert!(text_boxes(&document.body.blocks).is_empty());
}
