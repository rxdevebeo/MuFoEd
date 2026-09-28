//! Parse a Strict `.docx` and print a summary of the document model.
//!
//! ```text
//! cargo run -p strict-ooxml-wml --example parse_document -- path/to/document.docx
//! ```

use std::process::ExitCode;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: parse_document <file.docx>");
        return ExitCode::from(2);
    };

    let package = match Package::open_path(&path, &OpenOptions::default()) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    let document = match parse_document(&package, &ParseOptions::default()) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };

    println!("blocks: {}", document.body.blocks.len());
    println!("sections: {}", document.sections.len());
    println!("styles: {}", document.styles.len());
    println!("numbering instances: {}", document.numbering.len());
    println!("media: {}", document.media.len());
    println!("{}", document.support_debug());
    ExitCode::SUCCESS
}
