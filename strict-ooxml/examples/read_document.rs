//! Open a Strict `.docx` through the public API and print a summary.
//!
//! ```text
//! cargo run -p strict-ooxml --example open_strict -- path/to/document.docx
//! ```

use std::process::ExitCode;

use strict_ooxml::{OpenOptions, StrictDocument};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: open_strict <file.docx>");
        return ExitCode::from(2);
    };

    match StrictDocument::open_path(&path, &OpenOptions::default()) {
        Ok(document) => {
            println!("blocks: {}", document.document().body.blocks.len());
            println!("{}", document.support_debug());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
