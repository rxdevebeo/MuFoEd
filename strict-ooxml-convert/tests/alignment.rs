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
    // A one-line paragraph the width of the text may run 15% past its edge:
    // 200 pt wide, so 30 pt (600 twips) into the right margin.
    let first_end = converted
        .document
        .body
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.props.indentation.and_then(|i| i.end),
            _ => None,
        })
        .map(|end| end.0);
    assert_eq!(first_end, Some(-600));
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

#[test]
fn a_page_number_on_its_entrys_line_is_reached_by_a_tab() {
    let mut pdf = PdfBuilder::new();
    let widths = vec!["500"; 95].join(" ");
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    ));
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    // Text runs 72..272; the entry's number "9" ends at 272.
    let body = "A".repeat(40);
    let stream = format!(
        "BT /F1 10 Tf 72 700 Td ({body}) Tj ET\n\
         BT /F1 10 Tf 72 680 Td (Intro) Tj ET\n\
         BT /F1 10 Tf 267 680 Td (9) Tj ET"
    );
    pdf.page_with(stream.as_bytes(), &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let entry = converted
        .document
        .body
        .blocks
        .iter()
        .find_map(|block| {
            match block {
            Block::Paragraph(paragraph)
                if paragraph.inlines.iter().any(|inline| {
                    matches!(inline, Inline::Run(run) if run.content.contains(&RunContent::Tab))
                }) =>
            {
                Some(paragraph)
            }
            _ => None,
        }
        })
        .expect("the entry and its number are one paragraph");
    let stop = entry.props.tabs.first().expect("a tab stop");
    assert_eq!(
        stop.alignment,
        strict_ooxml_wml::model::values::TabAlignment::End
    );
    let margin = converted
        .document
        .sections
        .first()
        .and_then(|section| section.properties.page_margins)
        .and_then(|margins| margins.left)
        .map_or(0, |left| left.0);
    assert_eq!(stop.position.0 + margin, 272 * 20);
}

#[test]
fn lines_that_run_on_are_a_word_apart() {
    // No space at the first line's end, as LaTeX and browsers write it.
    let mut pdf = PdfBuilder::new();
    let widths = vec!["500"; 95].join(" ");
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    ));
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    let stream = "BT /F1 10 Tf 72 700 Td (the first line ends) Tj ET\n\
                  BT /F1 10 Tf 72 688 Td (and the second runs on) Tj ET\n\
                  BT /F1 10 Tf 72 676 Td (with a hyphen- ated word) Tj ET";
    pdf.page_with(stream.as_bytes(), &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let text: String = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .flat_map(|paragraph| paragraph.inlines.iter())
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
    assert_eq!(
        text,
        "the first line ends and the second runs on with a hyphen- ated word"
    );
}

/// groff sets two words in one string and the space between them as character
/// spacing equal to the space's width (`(yn)Tj` at `Tc` = space + `Tw`). Read
/// as tracking, the gap was part of each letter: `any number` came out as
/// `anyn` and `umber`, the halves of a line broken mid-word (L1).
#[test]
fn character_spacing_the_width_of_a_space_is_a_space() {
    let mut pdf = PdfBuilder::new();
    let widths = vec!["500"; 95].join(" ");
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    ));
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    // A character is 5 pt and so is the space; `Tw 1` makes a word space 6 pt.
    // "collects an" is 55 pt, "y", a 6 pt space and "n" 16 pt more.
    let stream = "BT /F1 10 Tf 1 Tw 72 700 Td (collects an)Tj \
                  6 Tc 55 0 Td (yn)Tj 0 Tc 16 0 Td (umber of files)Tj ET";
    pdf.page_with(stream.as_bytes(), &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let paragraphs: Vec<String> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .map(|paragraph| {
            paragraph
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
                .collect()
        })
        .collect();
    assert_eq!(paragraphs, ["collects any number of files"]);
}

/// A contents line's dots drawn as a dotted path under the baseline are the
/// leader of the tab to its page number, not a shape anchored to the page that
/// stays put when the text moves.
#[test]
fn a_dotted_path_to_a_page_number_is_the_tabs_leader() {
    use strict_ooxml_wml::model::values::TabLeader;
    let mut pdf = PdfBuilder::new();
    let widths = vec!["500"; 95].join(" ");
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    ));
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    // "Introduction" ends at 132; the number stands at 500; the dots run
    // between them, 1.8 pt under the baseline, as Google Docs draws them.
    let stream = "BT /F1 10 Tf 72 700 Td (Introduction) Tj ET\n\
                  BT /F1 10 Tf 500 700 Td (6) Tj ET\n\
                  BT /F1 10 Tf 72 600 Td (After the contents) Tj ET\n\
                  0.5 w 1 J [0 1.5] 0 d 135 698.2 m 498 698.2 l S";
    pdf.page_with(stream.as_bytes(), &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    let paragraphs: Vec<_> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect();
    let leaders: Vec<_> = paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.props.tabs.iter())
        .filter_map(|tab| tab.leader)
        .collect();
    assert_eq!(leaders, [TabLeader::Dot]);
    let shapes = paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.inlines.iter())
        .filter(|inline| matches!(inline, Inline::Drawing(_)))
        .count();
    assert_eq!(shapes, 0, "the dots are not drawn a second time");
}
