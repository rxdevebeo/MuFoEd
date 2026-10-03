//! Image emission tests (`STAGE-4-TASK.md` §5.7).

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::doc_markdown
)]

mod common;

use common::open_with_image;
use strict_ooxml_render_svg::{render, render_with_media, MediaMode, RenderOptions};

#[test]
fn embeds_data_uri_by_default() {
    let (package, document) = open_with_image();
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    let svg = &pages[0].svg;
    assert!(svg.contains("<image "), "{svg}");
    assert!(svg.contains("xlink:href=\"data:image/png;base64,"), "{svg}");
    assert!(svg.contains("a description"), "alt title missing");
}

#[test]
fn external_files_uses_the_part_file_name() {
    let (package, document) = open_with_image();
    let pages = render_with_media(
        &document,
        &RenderOptions::default().media(MediaMode::ExternalFiles),
        Some(&package),
    )
    .expect("render");
    assert!(
        pages[0]
            .svg
            .contains("xlink:href=\"word_media_image1.png\""),
        "{}",
        pages[0].svg
    );
}

#[test]
fn none_mode_draws_a_placeholder() {
    let (package, document) = open_with_image();
    let pages = render_with_media(
        &document,
        &RenderOptions::default().media(MediaMode::None),
        Some(&package),
    )
    .expect("render");
    let svg = &pages[0].svg;
    assert!(!svg.contains("<image "), "{svg}");
    assert!(svg.contains("fill=\"#f2f2f2\""), "{svg}");
}

#[test]
fn missing_media_source_draws_a_placeholder() {
    let (_package, document) = open_with_image();
    let pages = render(&document, &RenderOptions::default()).expect("render");
    assert!(!pages[0].svg.contains("<image "), "{}", pages[0].svg);
    assert!(pages[0].svg.contains("fill=\"#f2f2f2\""));
}
