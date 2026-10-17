//! A semantic conversion puts a page's pictures where the page put them.
//!
//! A letter's two signatures and stamp stand in one row; placed as paragraphs
//! of their own they became a column, and the letter's last lines went to a
//! second page.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::drawing::{AnchorDrawing, Drawing, DrawingKind, WrapKind};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::Block;

/// A Letter page: a heading, a row of two pictures with nothing beside them,
/// a picture with a line of text beside it, one with text set over it and
/// one across an empty line.
fn letter() -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let font = pdf.object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    let samples = vec![0x80; 2 * 2 * 3];
    let picture = pdf.stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8",
        &samples,
        true,
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> /XObject << /Im1 {picture} 0 R >> >>");
    let stream = "BT /F1 12 Tf 72 720 Td (Heading) Tj ET\n\
                  q 100 0 0 50 72 600 cm /Im1 Do Q\n\
                  q 100 0 0 50 300 600 cm /Im1 Do Q\n\
                  BT /F1 12 Tf 72 560 Td (Names) Tj ET\n\
                  q 100 0 0 50 300 400 cm /Im1 Do Q\n\
                  BT /F1 12 Tf 72 420 Td (Beside) Tj ET\n\
                  q 200 0 0 50 72 200 cm /Im1 Do Q\n\
                  BT /F1 12 Tf 80 220 Td (Over) Tj ET\n\
                  q 100 0 0 50 300 100 cm /Im1 Do Q\n\
                  BT /F1 12 Tf 72 120 Td (   ) Tj ET\n\
                  BT /F1 12 Tf 72 60 Td (End) Tj ET";
    pdf.page_with(stream.as_bytes(), &resources);
    pdf.build()
}

fn anchors(blocks: &[Block]) -> Vec<(usize, &AnchorDrawing)> {
    let mut out = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &paragraph.inlines {
            if let Inline::Drawing(Drawing {
                kind: DrawingKind::Anchor(anchor),
                ..
            }) = inline
            {
                // Pictures only: the page's head and foot are text boxes.
                if matches!(
                    anchor.graphic.as_ref(),
                    strict_ooxml_wml::model::drawing::Graphic::Picture(_)
                ) {
                    out.push((index, anchor));
                }
            }
        }
    }
    out
}

#[allow(clippy::cast_precision_loss)]
fn points(anchor: &AnchorDrawing) -> (f64, f64) {
    let at = |position: &Option<strict_ooxml_wml::model::drawing::Position>| {
        position
            .as_ref()
            .and_then(|position| position.offset)
            .map_or(f64::NAN, |emu| emu.0 as f64 / 12_700.0)
    };
    (at(&anchor.position_h), at(&anchor.position_v))
}

#[test]
fn pictures_are_anchored_where_the_page_put_them() {
    let bytes = letter();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let blocks = &converted.document.body.blocks;
    let found = anchors(blocks);
    assert_eq!(found.len(), 5, "every picture is anchored");
    // All held by the page's first paragraph, which carries its text too:
    // no paragraph exists only to hold a picture.
    assert!(found.iter().all(|(index, _)| *index == found[0].0));
    let Block::Paragraph(first) = &blocks[found[0].0] else {
        unreachable!()
    };
    assert!(first
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Run(_))));

    let mut placed: Vec<((f64, f64), WrapKind, bool)> = found
        .iter()
        .map(|(_, anchor)| {
            let wrap = anchor.wrap.as_ref().map(|wrap| wrap.kind);
            (points(anchor), wrap.expect("a wrap"), anchor.behind_doc)
        })
        .collect();
    placed.sort_by(|a, b| a.0 .1.total_cmp(&b.0 .1).then(a.0 .0.total_cmp(&b.0 .0)));
    let near =
        |(x, y): (f64, f64), (ex, ey): (f64, f64)| (x - ex).abs() < 0.5 && (y - ey).abs() < 0.5;
    // Tops from the page's top: 792 - (y + height).
    assert!(near(placed[0].0, (72.0, 142.0)), "{placed:?}");
    assert!(near(placed[1].0, (300.0, 142.0)), "{placed:?}");
    assert_eq!((placed[0].1, placed[0].2), (WrapKind::None, false));
    assert_eq!((placed[1].1, placed[1].2), (WrapKind::None, false));
    assert!(near(placed[2].0, (300.0, 342.0)), "{placed:?}");
    assert_eq!((placed[2].1, placed[2].2), (WrapKind::Square, false));
    assert!(near(placed[3].0, (72.0, 542.0)), "{placed:?}");
    assert_eq!((placed[3].1, placed[3].2), (WrapKind::None, true));
    // An empty line across a picture is no text across it.
    assert!(near(placed[4].0, (300.0, 642.0)), "{placed:?}");
    assert_eq!((placed[4].1, placed[4].2), (WrapKind::None, false));
}

#[test]
fn without_images_nothing_is_anchored() {
    let bytes = letter();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let options = PdfOptions::default()
        .mode(Mode::Semantic)
        .embed_images(false);
    let converted = convert(&mut reader, &options).expect("convert");
    assert!(anchors(&converted.document.body.blocks).is_empty());
}

#[test]
fn a_turned_picture_keeps_its_turn() {
    // 50 by 100 pt, turned a quarter anticlockwise (PDF): its box on the page
    // is 100 wide and 50 tall, and DrawingML turns it 270 degrees clockwise.
    let mut pdf = PdfBuilder::new();
    let picture = pdf.stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8",
        &[0x80; 12],
        true,
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /XObject << /Im1 {picture} 0 R >> >>");
    pdf.page_with(b"q 0 50 -100 0 300 400 cm /Im1 Do Q", &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let found = anchors(&converted.document.body.blocks);
    let (_, anchor) = found.first().expect("the picture is anchored");
    let strict_ooxml_wml::model::drawing::Graphic::Picture(picture) = anchor.graphic.as_ref()
    else {
        panic!("a picture")
    };
    let xfrm = picture.xfrm.expect("a turn");
    assert_eq!(xfrm.rot, Some(270 * 60_000));
    let extent = anchor.extent.expect("an extent");
    assert_eq!((extent.cx.0, extent.cy.0), (50 * 12_700, 100 * 12_700));
    // Centred on its box: x 200..300, so the 50 pt picture starts at 225.
    assert_eq!(points(anchor).0.round(), 225.0);
}
