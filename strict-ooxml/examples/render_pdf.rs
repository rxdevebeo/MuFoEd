//! Opens a minimal Strict document and renders it to PDF.
//!
//! ```text
//! cargo run -p strict-ooxml --no-default-features --features pdf --example render_pdf
//! ```

use std::io::Cursor;

use strict_ooxml::{OpenOptions, RenderOptions, StrictDocument};
use strict_ooxml_testkit::DocxBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>Hello</w:t></w:r></w:p>")
        .build();
    let document = StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default())?;
    let pdf = document.render_pdf(&RenderOptions::default())?;
    if !pdf.bytes.starts_with(b"%PDF-") || pdf.page_count == 0 {
        return Err("render_pdf did not produce a PDF".into());
    }
    println!("pages {}", pdf.page_count);
    Ok(())
}
