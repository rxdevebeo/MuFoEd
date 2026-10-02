//! Dumps a rendered page's content stream, so a wrong coordinate can be read
//! rather than guessed at.
//!
//! ```text
//! cargo run -p strict-ooxml-render-pdf --example dump_content -- <file.docx>
//! ```
//!
//! # This is a development instrument, not an example for a user
//!
//! It writes to stdout, it reads one path, it has no options and it exists to
//! answer one question: what operators did the writer actually emit for this
//! document. A caller of this crate wants [`render_with_source`], and the CLI
//! wants `strict-ooxml to-pdf`. The one public example of the crate is therefore
//! this one, and it is declared in the crate documentation as a debugging aid so
//! that it does not read as a supported entry point — which is what
//! `STAGE-8-OPEN.md` Q-10 asked for.
//!
//! The other half of the same job lives next door: `strict-ooxml-pdf`'s
//! `dump_page` reads a PDF back and prints what a *reader* sees of it.

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
