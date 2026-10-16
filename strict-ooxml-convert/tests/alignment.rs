//! A line at the right edge, or in the middle, of a page's text is aligned
//! there rather than indented to where it starts.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::values::Justification;
use strict_ooxml_wml::model::Block;

#[test]
fn right_and_centred_lines_are_aligned() {
    let mut pdf = PdfBuilder::new();
    // Every code 500/1000 em wide: at 10 pt a character is 5 pt.
    let widths = vec!["500"; 95].join(" ");
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    ));
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    // The body runs 72..272; RRRR ends at 272; CC is centred on 172.
    let body = "A".repeat(40);
    let stream = format!(
        "BT /F1 10 Tf 72 700 Td ({body}) Tj ET\n\
         BT /F1 10 Tf 252 660 Td (RRRR) Tj ET\n\
         BT /F1 10 Tf 167 620 Td (CC) Tj ET\n\
         BT /F1 10 Tf 72 580 Td ({body}) Tj ET"
    );
    pdf.page_with(stream.as_bytes(), &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let aligned: Vec<(String, Option<Justification>)> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .map(|paragraph| {
            let text: String = paragraph
                .inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Run(run) => Some(run),
                    _ => None,
                })
                .flat_map(|run| run.content.iter())
                .filter_map(|content| match content {
                    RunContent::Text(node) => Some(node.text.as_str()),
                    _ => None,
                })
                .collect();
            (text.trim().to_owned(), paragraph.props.alignment)
        })
        .filter(|(text, _)| !text.is_empty())
        .collect();
    assert_eq!(
        aligned,
        [
            (body.clone(), None),
            ("RRRR".to_owned(), Some(Justification::End)),
            ("CC".to_owned(), Some(Justification::Center)),
            (body, None),
        ],
        "{aligned:?}"
    );
}
