//! H1: a line repeated in the same band at the top or foot of the pages is the
//! document's header or footer, written as a header or footer part, and the
//! digits that count with the page are a `PAGE` field.

use strict_ooxml_convert::{convert, Mode, PdfOptions, Severity};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::props::HeaderFooterKind;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

/// Three A4 pages: a running head, two body lines, and `Page n of 3` centred
/// at the foot. `first_head` false leaves the head off the first page.
fn three_pages(first_head: bool) -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let widths = vec!["500"; 95].join(" ");
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    ));
    pdf.set_media_box([0, 0, 595, 842]);
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    for page in 1..=3 {
        let head = if first_head || page > 1 {
            "BT /F1 9 Tf 72 800 Td (Quarterly Report) Tj ET\n"
        } else {
            ""
        };
        // The body runs from 72 to 522 (90 characters at 10 pt), so the text
        // is centred on 297; "Page n of 3" is 11 characters, 49.5 pt at 9 pt.
        let filler = "x".repeat(51);
        let stream = format!(
            "{head}\
             BT /F1 10 Tf 72 700 Td (Body text of page number {page} begins here {filler}) Tj ET\n\
             BT /F1 10 Tf 72 686 Td (A second line of the body on this page.) Tj ET\n\
             BT /F1 9 Tf 272.25 40 Td (Page {page} of 3) Tj ET"
        );
        pdf.page_with(stream.as_bytes(), &resources);
    }
    pdf.build()
}

fn convert_pdf(bytes: &[u8]) -> strict_ooxml_convert::Converted {
    let mut reader = PdfDocument::open(bytes, PdfLimits::default()).expect("open pdf");
    convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert")
}

/// Writes the conversion and reads the package back, so the parts are checked
/// as a reader finds them, relationships and references resolved.
fn round_trip(converted: &strict_ooxml_convert::Converted) -> Document {
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written = strict_ooxml_write::write_package(
        &converted.document,
        Some(&bag),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .expect("write");
    let package =
        Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");
    assert!(package.read_part(&PartId::new("/word/header1.xml")).is_ok());
    assert!(package.read_part(&PartId::new("/word/footer1.xml")).is_ok());
    parse_document(&package, &ParseOptions::default()).expect("parse")
}

/// Text of inlines, a field's cached result marked as `{instruction|result}`.
fn inline_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                for content in &run.content {
                    match content {
                        RunContent::Text(node) => out.push_str(&node.text),
                        RunContent::Tab => out.push('\t'),
                        _ => {}
                    }
                }
            }
            Inline::Field(field) => {
                out.push('{');
                out.push_str(field.instruction.as_deref().unwrap_or("").trim());
                out.push('|');
                inline_text(&field.inlines, out);
                out.push('}');
            }
            _ => {}
        }
    }
}

fn blocks_text(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => {
                let mut text = String::new();
                inline_text(&paragraph.inlines, &mut text);
                Some(text)
            }
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn part_text(document: &Document, header: bool) -> Vec<Vec<String>> {
    document
        .headers_footers
        .iter()
        .filter(|part| part.is_header == header)
        .map(|part| blocks_text(&part.blocks))
        .collect()
}

#[test]
fn a_running_head_and_page_number_become_header_and_footer_parts() {
    let converted = convert_pdf(&three_pages(true));
    let document = round_trip(&converted);

    assert_eq!(
        part_text(&document, true),
        [vec!["Quarterly Report".to_owned()]]
    );
    assert_eq!(
        part_text(&document, false),
        [vec!["Page {PAGE|1} of 3".to_owned()]]
    );
    // The body keeps its own text, and only that: no head, no page number.
    let body = blocks_text(&document.body.blocks).join("\n");
    assert!(body.contains("Body text of page number 3"), "{body}");
    assert!(!body.contains("Quarterly"), "{body}");
    assert!(!body.contains("of 3"), "{body}");

    // Every section shows the parts; the first page is like the others.
    for section in &document.sections {
        let kinds = |refs: &[strict_ooxml_wml::model::props::HeaderFooterRef]| {
            refs.iter()
                .map(|reference| (reference.kind, reference.part.is_some()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds(&section.properties.headers),
            [(HeaderFooterKind::Default, true)]
        );
        assert_eq!(
            kinds(&section.properties.footers),
            [(HeaderFooterKind::Default, true)]
        );
        assert!(!section.properties.title_page);
        let margins = section.properties.page_margins.expect("pgMar");
        assert!(margins.header.is_some_and(|header| header.0 > 0));
        assert!(margins.footer.is_some_and(|footer| footer.0 > 0));
    }
    // The footer's line was centred on the page.
    let footer = document
        .headers_footers
        .iter()
        .find(|part| !part.is_header)
        .and_then(|part| part.blocks.first());
    let Some(Block::Paragraph(footer)) = footer else {
        panic!("a footer paragraph");
    };
    assert_eq!(
        footer.props.alignment,
        Some(strict_ooxml_wml::model::values::Justification::Center)
    );

    let inferred: Vec<&str> = converted
        .report
        .losses()
        .iter()
        .filter(|loss| loss.severity == Severity::Inferred)
        .map(|loss| loss.id.as_str())
        .collect();
    for id in [
        "layout.running-header",
        "layout.running-footer",
        "layout.page-number",
    ] {
        assert!(inferred.contains(&id), "{id} in {inferred:?}");
    }
}

#[test]
fn a_first_page_without_the_head_gets_a_first_page_header() {
    let converted = convert_pdf(&three_pages(false));
    let document = round_trip(&converted);
    let first = document.sections.first().expect("a section");
    assert!(first.properties.title_page);
    let first_header = first
        .properties
        .headers
        .iter()
        .find(|reference| reference.kind == HeaderFooterKind::First)
        .and_then(|reference| reference.part.as_ref())
        .and_then(|part| document.header_footer(part))
        .expect("a first-page header");
    assert!(blocks_text(&first_header.blocks).is_empty());
    // The number runs on the first page too: its footer is the others'.
    let first_footer = first
        .properties
        .footers
        .iter()
        .find(|reference| reference.kind == HeaderFooterKind::First)
        .and_then(|reference| reference.part.as_ref())
        .and_then(|part| document.header_footer(part))
        .expect("a first-page footer");
    assert_eq!(
        blocks_text(&first_footer.blocks),
        ["Page {PAGE|1} of 3".to_owned()]
    );
}
