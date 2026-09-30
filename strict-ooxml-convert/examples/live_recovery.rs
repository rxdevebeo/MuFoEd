//! A live check of the whole recovery chain, against a real model and a real
//! document.
//!
//! ```text
//! cargo run -p strict-ooxml-convert --features raster,ocr-ollama \
//!     --example live_recovery -- <document.pdf> [page]
//! ```
//!
//! # Why this exists and what it is not
//!
//! The tests in `tests/recovery.rs` script the model, so they prove the
//! **pipeline**: which regions the trigger chooses, what happens to the answer,
//! and what the document and the report say afterwards. `strict-ooxml-ocr`'s
//! `live_probe` proves the **transport**: that a real daemon takes the request we
//! build. Neither says whether a real model reads a real page *well*, and that is
//! the question nobody should have to take on faith — a document that says «the
//! scan was recovered» and a document full of misread words look identical from the
//! outside.
//!
//! So this runs the whole thing on a file from the corpus and prints what came
//! back, next to what the reader already had for that page. Read the two together:
//! where the page's own text is the truth and the model's answer is the only
//! source, **the model's answer is the document's text**, and this is the only
//! place in the project where that is visible.
//!
//! Nothing is asserted and nothing is compared to a number: an answer that is
//! wrong here is information, not a failure. A model that is not running prints
//! the reason and exits — the same refusal the converter records in its report.
//!
//! It is opt-in by not being a test. It loads a model, and a test suite that loads
//! a 6 GB model behind someone's back is a test suite people stop running.

#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
fn main() {
    use std::sync::Arc;
    use std::time::Instant;

    use strict_ooxml_convert::{convert, Mode, PdfOptions};
    use strict_ooxml_ocr::ollama::OllamaVision;
    use strict_ooxml_pdf::{PdfDocument, PdfLimits};

    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    // `--save <dir>` writes what the model was shown, one PNG per region. It is
    // the difference between «the model said X» and «the model said X about *this
    // picture*»: an answer you cannot look at is an answer you cannot check.
    let save = match arguments.iter().position(|argument| argument == "--save") {
        Some(at) => {
            let directory = arguments.get(at + 1).cloned();
            arguments.drain(at..at + 2);
            directory
        }
        None => None,
    };
    let path = arguments.first().cloned().expect("a PDF path");
    let only = arguments
        .get(1)
        .and_then(|page| page.parse::<usize>().ok())
        .filter(|page| *page > 0);

    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        eprintln!("cannot read {path}: {error}");
        std::process::exit(2);
    });
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).unwrap_or_else(|error| {
        eprintln!("cannot open {path}: {error}");
        std::process::exit(2);
    });
    println!("file: {}", std::path::Path::new(&path).display());
    println!("pages: {}", document.page_count());

    // What the reader already has, so the model's answer can be read against it.
    let pages = document.pages().unwrap_or_else(|error| {
        eprintln!("cannot read the pages: {error}");
        std::process::exit(2);
    });
    for (index, page) in pages.iter().enumerate() {
        if only.is_some_and(|wanted| wanted != index + 1) {
            continue;
        }
        let text = page.text();
        let (width, height) = page.geometry.displayed();
        println!(
            "\n--- page {}: {width:.0}x{height:.0} pt ({:?}), {} glyphs, {} pictures, {} vectors, \
             {} words of the reader's own text",
            index + 1,
            page.text_layer(),
            page.items()
                .iter()
                .filter(|item| matches!(item, strict_ooxml_pdf::Item::Glyph(_)))
                .count(),
            page.items()
                .iter()
                .filter(|item| matches!(item, strict_ooxml_pdf::Item::Image(_)))
                .count(),
            page.items()
                .iter()
                .filter(|item| matches!(item, strict_ooxml_pdf::Item::Vector(_)))
                .count(),
            text.split_whitespace().count(),
        );
        let head: Vec<&str> = text.split_whitespace().take(24).collect();
        if !head.is_empty() {
            println!("    reader: {}", head.join(" "));
        }
    }
    drop(pages);

    let client = Arc::new(OllamaVision::from_env());
    println!(
        "\nmodel: {}",
        strict_ooxml_ocr::TextRecovery::model_name(&*client)
    );
    match client.model_version() {
        Ok(version) => println!("version: {version}"),
        Err(error) => {
            println!("no model answered ({error}) — is `ollama serve` running?");
            return;
        }
    }

    if let Some(directory) = save.as_ref() {
        if let Err(error) = std::fs::create_dir_all(directory) {
            eprintln!("cannot create {directory}: {error}");
            std::process::exit(2);
        }
        save_regions(&mut document, directory.as_str());
    }

    let started = Instant::now();
    let mut options = PdfOptions {
        mode: Mode::Semantic,
        text_recovery: Some(client),
        ..PdfOptions::default()
    };
    if let Some(page) = only {
        options.pages = Some((page, page));
    }
    let converted = convert(&mut document, &options).unwrap_or_else(|error| {
        eprintln!("the conversion failed: {error}");
        std::process::exit(2);
    });
    println!("\nconverted in {:?}", started.elapsed());
    println!("\nreport:\n{}", converted.report);

    println!("\nwhat the model wrote into the document:");
    print_recovered(&converted.document);
}

/// Writes a PNG of every page the trigger would offer, so that what the model was
/// shown can be looked at rather than believed.
///
/// The regions are chosen by the converter's own rules, not by this probe: it
/// asks the reader for the pictures big enough to hold text, and writes each one at
/// the scale a recovery call would use. A disagreement between what is in the file
/// and what the model wrote is then a fact about the picture, not a mystery.
#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
fn save_regions(document: &mut strict_ooxml_pdf::PdfDocument, directory: &str) {
    use strict_ooxml_pdf::Item;

    let Ok(rasterizer) = document.rasterizer() else {
        eprintln!("no rasterizer for this document");
        return;
    };
    let pages = document.pages().unwrap_or_default();
    let options = strict_ooxml_pdf::raster::RasterOptions {
        scale: 2.0,
        ..strict_ooxml_pdf::raster::RasterOptions::default()
    };
    let mut written = 0usize;
    for (index, page) in pages.iter().enumerate() {
        let (page_width, page_height) = page.geometry.displayed();
        let page_area = page_width * page_height;
        if !page_area.is_finite() || page_area <= 0.0 {
            continue;
        }
        for (number, image) in page
            .items()
            .iter()
            .filter_map(|item| match item {
                Item::Image(image) if image.image.is_some() => Some(image),
                _ => None,
            })
            .enumerate()
        {
            if below_the_trigger_floor(image.width * image.height, page_area) {
                continue;
            }
            let region = strict_ooxml_pdf::raster::Region {
                x: image.x,
                y: image.y,
                width: image.width,
                height: image.height,
            };
            match rasterizer.region_png(index + 1, region, &options) {
                Ok(png) => {
                    let name = format!("{directory}/page-{}-region-{number}.png", index + 1);
                    match std::fs::write(&name, png) {
                        Ok(()) => {
                            println!(
                                "  wrote {name} ({:.0}x{:.0} pt, {:.0}% of the page)",
                                image.width,
                                image.height,
                                100.0 * image.width * image.height / page_area
                            );
                            written += 1;
                        }
                        Err(error) => eprintln!("cannot write {name}: {error}"),
                    }
                }
                Err(error) => eprintln!(
                    "page {} region {number} was not rasterized: {error}",
                    index + 1
                ),
            }
        }
    }
    if written == 0 {
        println!("  no page of this document offers a picture of text");
    }
}

/// Whether a picture is too small for the mixed trigger to offer it.
///
/// **0.25** is the converter's default, and this probe does not read it from
/// anywhere — it is repeated here on purpose rather than duplicated silently: if
/// the default changes, this number is the one thing in this file that has to
/// change with it, and a probe that quietly kept the old value would write
/// pictures the model never saw.
#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
fn below_the_trigger_floor(area: f64, page_area: f64) -> bool {
    !area.is_finite() || area / page_area < 0.25
}

/// Prints the paragraphs a model wrote, which is the only part of this project
/// that a test cannot check: a wrong answer here is the document's text.
#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
fn print_recovered(document: &strict_ooxml_wml::model::Document) {
    use strict_ooxml_wml::model::inline::{Inline, RunContent};

    let mut count = 0usize;
    for block in &document.body.blocks {
        let Some(paragraph) = block.as_paragraph() else {
            continue;
        };
        if paragraph.props.style.is_none() {
            continue;
        }
        count += 1;
        let text: String = paragraph
            .inlines
            .iter()
            .filter_map(|inline| match inline {
                Inline::Run(run) => Some(
                    run.content
                        .iter()
                        .filter_map(|content| match content {
                            RunContent::Text(node) => Some(node.text.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect();
        let head: String = text.trim().chars().take(160).collect();
        println!("  [{count:>3}] {head}");
    }
    if count == 0 {
        println!("  (nothing was recovered)");
    }
}

#[cfg(not(all(feature = "raster", feature = "ocr-ollama")))]
fn main() {
    eprintln!(
        "this probe needs the `raster` and `ocr-ollama` features:\n  \
         cargo run -p strict-ooxml-convert --features raster,ocr-ollama \
         --example live_recovery -- <document.pdf>"
    );
    std::process::exit(2);
}
