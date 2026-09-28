//! Prints the Stage-3 Feature Report for a Strict `.docx`.
//!
//! ```text
//! cargo run -p strict-ooxml --example support_report -- document.docx
//! ```

use strict_ooxml::{OpenOptions, StrictDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: support_report <file.docx>")?;
    let document = StrictDocument::open_path(&path, &OpenOptions::default())?;
    print!("{}", document.report_text());
    Ok(())
}
