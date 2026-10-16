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
        let started = std::time::Instant::now();
        let options = OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(TransitionalNormalizer::new()));
        let package = Package::open_path(&path, &options).expect("open");
        let mut document = parse_document(&package, &ParseOptions::default()).expect("parse");
        let parsed = started.elapsed();
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
        if std::env::var("PROBE_TIME").is_ok() {
            // The viewer's path: the first pages, one layout with caret
            // anchors, the visible page painted, the map from that layout.
            let options = RenderOptions::default();
            let t = std::time::Instant::now();
            let preview = strict_ooxml_render_svg::place_document(
                editor.document(),
                &options
                    .clone()
                    .pages(strict_ooxml_render_svg::PageSelection::Range { start: 1, end: 3 }),
                Some(&package),
            );
            let preview_time = t.elapsed();
            let t = std::time::Instant::now();
            let anchored = options.clone().caret_anchors(true);
            let placed = strict_ooxml_render_svg::place_document(
                editor.document(),
                &anchored,
                Some(&package),
            )
            .expect("place");
            let place = t.elapsed();
            let t = std::time::Instant::now();
            let painted = placed.pages.first().map_or(0, |page| {
                strict_ooxml_render_svg::paint_page(page, &options.clone().embed_fonts(false)).len()
            });
            let paint = t.elapsed();
            let t = std::time::Instant::now();
            let map = editor.visual_map_placed(0, &options, Some(&package), &placed.pages);
            let map_time = t.elapsed();
            println!(
                "{path}: pages {} open+parse {} ms preview {} ms ({} pages) place {} ms paint first {} ms ({} KB) map {} ms ({})",
                placed.pages.len(),
                parsed.as_millis(),
                preview_time.as_millis(),
                preview.map_or(0, |p| p.pages.len()),
                place.as_millis(),
                paint.as_millis(),
                painted / 1024,
                map_time.as_millis(),
                if map.is_ok() { "ok" } else { "failed" }
            );
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
