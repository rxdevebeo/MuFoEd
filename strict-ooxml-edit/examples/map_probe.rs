//! Builds the caret map of each document named and says whether it held.
#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]
use std::sync::Arc;
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_edit::{EditLimits, Editor};
use strict_ooxml_render_svg::RenderOptions;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn main() {
    for path in std::env::args().skip(1) {
        let options = OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(TransitionalNormalizer::new()));
        let package = Package::open_path(&path, &options).expect("open");
        let mut document = parse_document(&package, &ParseOptions::default()).expect("parse");
        let editor = match Editor::new(&mut document, EditLimits::default()) {
            Ok(editor) => editor,
            Err(error) => {
                println!("{path}: editor {error:?}");
                continue;
            }
        };
        if std::env::var("PROBE_PARAGRAPHS").is_ok() {
            match editor.paragraphs(&strict_ooxml_edit::Story::Body) {
                Ok(list) => println!(
                    "{path}: paragraphs {} defects {}",
                    list.len(),
                    editor.source_defects().len()
                ),
                Err(error) => println!("{path}: paragraphs error {error:?}"),
            }
            continue;
        }
        let result = editor.visual_map(0, &RenderOptions::default(), Some(&package));
        println!(
            "{path}: {}",
            if result.is_ok() {
                "ok".to_owned()
            } else {
                format!("{:?}", result.err())
            }
        );
    }
}
