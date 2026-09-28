//! Minimal example: open a package and print its conformance and part map.
//!
//! Run with:
//!
//! ```text
//! cargo run -p strict-ooxml-core --example open_strict -- path/to/document.docx
//! ```

use std::process::ExitCode;

use strict_ooxml_core::opc::{OpenOptions, Package};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: open_strict <file.docx>");
        return ExitCode::from(2);
    };

    match Package::open_path(&path, &OpenOptions::default()) {
        Ok(package) => {
            println!("conformance: {:?}", package.conformance());
            if let Ok(main) = package.main_document_part() {
                println!("main document: {main}");
            }
            println!("parts:");
            for part in package.parts() {
                println!(
                    "  {} [{}]",
                    part.id,
                    part.content_type.as_deref().unwrap_or("<unknown>")
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}
