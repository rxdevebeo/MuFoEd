//! AUD-85 / CORE-QUEUE P-7: two-column reading order from a measured gutter.

#![allow(clippy::doc_markdown)]

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::Document;

struct Piece {
    text: &'static str,
    x: f64,
    /// PDF user-space y (from the bottom of the MediaBox).
    y: f64,
}

fn page_pdf(pieces: &[Piece], media: (f64, f64)) -> Vec<u8> {
    let mut content = Vec::new();
    for piece in pieces {
        content.extend_from_slice(
            format!(
                "BT /F1 12 Tf 1 0 0 1 {} {} Tm ({}) Tj ET ",
                piece.x, piece.y, piece.text
            )
            .as_bytes(),
        );
    }
    let (w, h) = media;
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /Font << /F1 \
             5 0 R >> >> /Contents 4 0 R >>"
        )),
        stream(&content),
        font(),
    ])
}

fn font() -> Vec<u8> {
    let mut out = String::from(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 126 \
         /Widths [",
    );
    for _ in 0..95 {
        out.push_str("500 ");
    }
    out.push_str("] >>\n");
    out.into_bytes()
}

fn text(body: &str) -> Vec<u8> {
    body.as_bytes().to_vec()
}

fn stream(body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets: Vec<usize> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len() + body.len());
        body.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        body.extend_from_slice(object);
        body.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(&body);
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in &offsets {
        use std::fmt::Write as _;
        let _ = write!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n0\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn convert_pdf(bytes: &[u8]) -> (Document, String) {
    let mut document = PdfDocument::open(bytes, PdfLimits::default()).expect("open");
    let converted =
        convert(&mut document, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    (converted.document, converted.report.to_string())
}

fn texts_of(document: &Document) -> Vec<String> {
    document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => {
                let mut text = String::new();
                for inline in &paragraph.inlines {
                    if let strict_ooxml_wml::model::inline::Inline::Run(run) = inline {
                        for content in &run.content {
                            if let strict_ooxml_wml::model::inline::RunContent::Text(node) = content
                            {
                                text.push_str(&node.text);
                            }
                        }
                    }
                }
                Some(text)
            }
            _ => None,
        })
        .collect()
}

fn indents_of(document: &Document) -> Vec<i32> {
    document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph
                .props
                .indentation
                .as_ref()
                .and_then(|indent| indent.start)
                .map(|twips| twips.0),
            _ => None,
        })
        .collect()
}

/// Interleaved draw order L1 R1 L2 R2 L3 R3 with a wide measured gutter.
fn two_column_interleaved() -> Vec<u8> {
    page_pdf(
        &[
            Piece {
                text: "LEFTA",
                x: 50.0,
                y: 700.0,
            },
            Piece {
                text: "RIGHTA",
                x: 350.0,
                y: 700.0,
            },
            Piece {
                text: "LEFTB",
                x: 50.0,
                y: 650.0,
            },
            Piece {
                text: "RIGHTB",
                x: 350.0,
                y: 650.0,
            },
            Piece {
                text: "LEFTC",
                x: 50.0,
                y: 600.0,
            },
            Piece {
                text: "RIGHTC",
                x: 350.0,
                y: 600.0,
            },
        ],
        (612.0, 792.0),
    )
}

#[test]
fn two_columns_read_left_then_right() {
    let (document, report) = convert_pdf(&two_column_interleaved());
    let texts = texts_of(&document);
    let joined = texts.join("|");
    assert!(
        report.contains("convert.columns"),
        "expected column inference in report: {report}"
    );
    // Left column top→bottom, then right — not the interleaved draw order.
    let left_pos = joined.find("LEFTA").expect("LEFTA");
    let left_b = joined.find("LEFTB").expect("LEFTB");
    let left_c = joined.find("LEFTC").expect("LEFTC");
    let right_a = joined.find("RIGHTA").expect("RIGHTA");
    let right_b = joined.find("RIGHTB").expect("RIGHTB");
    let right_c = joined.find("RIGHTC").expect("RIGHTC");
    assert!(
        left_pos < left_b
            && left_b < left_c
            && left_c < right_a
            && right_a < right_b
            && right_b < right_c,
        "reading order wrong: {joined}"
    );
}

#[test]
fn two_column_paragraphs_keep_measured_x_indent() {
    let (document, _) = convert_pdf(&two_column_interleaved());
    let indents = indents_of(&document);
    assert!(
        indents.iter().any(|&twips| twips > 0),
        "expected measured indents, got {indents:?}"
    );
    // 50 pt and 350 pt at 20 twips/pt → 1000 and 7000.
    assert!(
        indents.contains(&1000) || indents.iter().any(|&t| (980..1020).contains(&t)),
        "left column x≈50 pt missing: {indents:?}"
    );
    assert!(
        indents.contains(&7000) || indents.iter().any(|&t| (6980..7020).contains(&t)),
        "right column x≈350 pt missing: {indents:?}"
    );
}

#[test]
fn a_single_column_page_is_not_split() {
    let bytes = page_pdf(
        &[
            Piece {
                text: "AAAA",
                x: 72.0,
                y: 700.0,
            },
            Piece {
                text: "BBBB",
                x: 72.0,
                y: 680.0,
            },
            Piece {
                text: "CCCC",
                x: 90.0,
                y: 660.0,
            },
            Piece {
                text: "DDDD",
                x: 72.0,
                y: 640.0,
            },
        ],
        (612.0, 792.0),
    );
    let (document, report) = convert_pdf(&bytes);
    assert!(
        !report.contains("convert.columns"),
        "ordinary spacing must not become columns: {report}"
    );
    let texts = texts_of(&document);
    let joined = texts.join("");
    assert!(joined.contains("AAAA"), "{joined}");
    assert!(joined.contains("DDDD"), "{joined}");
}

#[test]
fn three_columns_are_unsupported_not_reordered() {
    let bytes = page_pdf(
        &[
            Piece {
                text: "A1",
                x: 40.0,
                y: 700.0,
            },
            Piece {
                text: "B1",
                x: 220.0,
                y: 700.0,
            },
            Piece {
                text: "C1",
                x: 400.0,
                y: 700.0,
            },
            Piece {
                text: "A2",
                x: 40.0,
                y: 650.0,
            },
            Piece {
                text: "B2",
                x: 220.0,
                y: 650.0,
            },
            Piece {
                text: "C2",
                x: 400.0,
                y: 650.0,
            },
        ],
        (612.0, 792.0),
    );
    let (_, report) = convert_pdf(&bytes);
    assert!(
        report.contains("unsupported") && report.contains("convert.columns"),
        "three columns must be unsupported: {report}"
    );
}

#[test]
fn uneven_column_widths_are_unsupported() {
    // Left ~200 pt wide text block, right ~80 pt — ratio > 1.55.
    let bytes = page_pdf(
        &[
            Piece {
                text: "LEFTWIDEONE",
                x: 40.0,
                y: 700.0,
            },
            Piece {
                text: "LEFTWIDETWO",
                x: 40.0,
                y: 650.0,
            },
            Piece {
                text: "LEFTWIDETHREE",
                x: 40.0,
                y: 600.0,
            },
            Piece {
                text: "R1",
                x: 480.0,
                y: 700.0,
            },
            Piece {
                text: "R2",
                x: 480.0,
                y: 650.0,
            },
            Piece {
                text: "R3",
                x: 480.0,
                y: 600.0,
            },
        ],
        (612.0, 792.0),
    );
    let (_, report) = convert_pdf(&bytes);
    assert!(
        report.contains("uneven") && report.contains("convert.columns"),
        "uneven columns must be unsupported: {report}"
    );
}
