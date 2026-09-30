//! Prints what the reader makes of the simplest possible text page.
use std::fmt::Write as _;

use strict_ooxml_pdf::{content::Item, PdfDocument, PdfLimits};

fn main() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nBT /F1 12 Tf 1 0 0 1 20 50 Tm (AB) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 66 /Widths [600 600] >>",
    ];
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets = Vec::new();
    let mut body = String::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len() + body.len());
        let _ = writeln!(body, "{} 0 obj\n{object}\nendobj\n", index + 1);
    }
    out.extend_from_slice(body.as_bytes());
    let mut table = String::new();
    let _ = writeln!(table, "xref\n0 {}\n0000000000 65535 f ", objects.len() + 1);
    for offset in &offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 99 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n");

    let mut pdf = PdfDocument::open(&out, PdfLimits::default()).expect("open");
    let page = pdf.page(1).expect("page");
    println!("geometry {}x{}", page.geometry.width, page.geometry.height);
    for item in page.items() {
        if let Item::Glyph(glyph) = item {
            println!(
                "glyph {:?} x={:.3} y={:.3} size={:.3} ascent={:.3} width={:.3} font={}",
                glyph.text, glyph.x, glyph.y, glyph.size, glyph.ascent, glyph.width, glyph.font
            );
        } else {
            println!("item {item:?}");
        }
    }
    println!("report: {}", pdf.report());
}
