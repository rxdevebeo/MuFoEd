//! Prints what the reader makes of one page of a PDF: its text lines (runs
//! of glyphs that follow each other exactly) and, with `--glyphs`, every
//! glyph - the first thing to look at when a PDF converts wrong.
//!
//! ```text
//! cargo run -p strict-ooxml-pdf --example dump_lines -- <file.pdf> [page] [--glyphs]
//! ```

use strict_ooxml_pdf::{content::Item, PdfDocument, PdfLimits};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let glyphs = arguments.iter().any(|argument| argument == "--glyphs");
    let positional: Vec<&String> = arguments.iter().filter(|a| !a.starts_with("--")).collect();
    let Some(path) = positional.first() else {
        eprintln!("usage: dump_lines <file.pdf> [page] [--glyphs]");
        std::process::exit(2);
    };
    let number: usize = positional
        .get(1)
        .and_then(|page| page.parse().ok())
        .unwrap_or(1);
    let bytes = std::fs::read(path.as_str()).unwrap_or_else(|error| {
        eprintln!("cannot read {path}: {error}");
        std::process::exit(2);
    });
    let mut pdf = PdfDocument::open(&bytes, PdfLimits::default()).unwrap_or_else(|error| {
        eprintln!("cannot open {path}: {error}");
        std::process::exit(2);
    });
    let page = pdf.page(number).unwrap_or_else(|error| {
        eprintln!("cannot read page {number}: {error}");
        std::process::exit(2);
    });
    println!(
        "page {number}: {}x{} pt, rotation {}",
        page.geometry.width, page.geometry.height, page.geometry.rotation
    );
    if glyphs {
        for item in page.items() {
            if let Item::Glyph(glyph) = item {
                println!(
                    "glyph {:?} x={:.2} y={:.2} w={:.2} size={:.2} font={} mapped={}",
                    glyph.text, glyph.x, glyph.y, glyph.width, glyph.size, glyph.font, glyph.mapped
                );
            }
        }
    }
    for (index, line) in strict_ooxml_pdf::text_lines(page.items())
        .iter()
        .enumerate()
    {
        let font = line.glyphs.first().map_or("", |glyph| glyph.font.as_str());
        println!(
            "line {index:4} x={:7.2} base={:7.2} w={:7.2} size={:5.2} font={font} {:?}",
            line.x,
            line.baseline,
            line.width,
            line.size,
            line.text()
        );
    }
    println!("report: {}", pdf.report());
}
