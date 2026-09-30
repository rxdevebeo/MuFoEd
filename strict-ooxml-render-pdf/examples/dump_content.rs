//! Development aid: dumps the first page's content stream.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dump_content <file.docx>");
    let bytes = std::fs::read(&path).expect("read");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let pages = place_pages(&document, &options, Some(&package)).expect("place");
    println!("pages: {}", pages.len());
    for (index, page) in pages.iter().enumerate() {
        println!("  page {index}: {} items", page.items.len());
    }
    let output = render_with_source(&pages, &options, Some(&package)).expect("render");
    let pdf = lopdf::Document::load_mem(&output.bytes).expect("parse pdf");
    let id = pdf.page_iter().next().expect("a page");
    let content = pdf.get_page_content(id);
    println!("--- {} bytes ---", content.len());
    println!("{}", String::from_utf8_lossy(&content));
    println!("--- report ---\n{}", output.report);
}
