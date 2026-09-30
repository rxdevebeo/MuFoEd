//! Prints the first glyphs the reader recovers from a PDF this project wrote.
use std::path::Path;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_pdf::{content::Item, PdfDocument, PdfLimits};
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn main() {
    let path = std::env::args().nth(1).expect("a .docx");
    let bytes = std::fs::read(&path).expect("read");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let placed = place_pages(&document, &options, Some(&package)).expect("place");
    let pdf = strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("render")
        .bytes;
    let mut read_back = PdfDocument::open(&pdf, PdfLimits::default()).expect("open pdf");
    let page = read_back.page(1).expect("page");
    for item in page.items().iter().take(8) {
        if let Item::Glyph(glyph) = item {
            println!(
                "{:?} x={:.4} y={:.4} w={:.4} size={:.4} font={}",
                glyph.text, glyph.x, glyph.y, glyph.width, glyph.size, glyph.font
            );
        }
    }
    println!("--- layout ---");
    for item in placed[0].items.iter().take(3) {
        if let strict_ooxml_render_svg::Item::Text(text) = item {
            println!(
                "{:?} x={} baseline={} width={} size_px={}",
                text.text, text.x, text.baseline, text.width, text.size_px
            );
        }
    }
    let _ = Path::new(".");
}
