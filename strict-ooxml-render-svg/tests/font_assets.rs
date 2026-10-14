//! Caller-supplied fonts (RTF RC02): measured, drawn and embedded like a
//! document's embedded fonts, pinned and bounded, refused on request.

#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::open_body;
use strict_ooxml_render_svg::font::{
    face_source, FontAsset, FontAssetError, FontAssetLimits, FontAssets,
};
use strict_ooxml_render_svg::{render_with_media, FontPolicy, RenderOptions};

const BODY: &str =
    "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Test Sans\" w:hAnsi=\"Test Sans\"/></w:rPr>\
<w:t>Supplied text</w:t></w:r></w:p>";

/// Arimo's bytes under a family name nothing bundled answers to.
fn arimo(family: &str) -> FontAsset {
    let source = face_source("Arial", false, false).expect("bundled Arimo");
    FontAsset::new(family, false, false, source.data.to_vec())
}

fn render(
    options: &RenderOptions,
) -> strict_ooxml_core::error::Result<Vec<strict_ooxml_render_svg::Page>> {
    let (package, document) = open_body(BODY);
    render_with_media(&document, options, Some(&package))
}

#[test]
fn a_supplied_font_replaces_the_substitute() {
    let substituted = render(&RenderOptions::default()).expect("render");
    assert!(
        substituted[0]
            .warnings
            .iter()
            .any(|warning| warning.starts_with("font.substitution")),
        "{:?}",
        substituted[0].warnings
    );
    // The family is matched without regard to case.
    let assets =
        FontAssets::new(vec![arimo("test SANS")], FontAssetLimits::default()).expect("assets");
    let supplied = render(&RenderOptions::default().font_assets(Arc::new(assets))).expect("render");
    assert!(
        !supplied[0]
            .warnings
            .iter()
            .any(|warning| warning.starts_with("font.substitution")),
        "{:?}",
        supplied[0].warnings
    );
    assert!(
        supplied[0].svg.contains("ASSET-"),
        "the supplied program is the drawn face"
    );
}

#[test]
fn the_reject_policy_refuses_a_substitution() {
    let strict = RenderOptions::default().font_policy(FontPolicy::Reject);
    let error = render(&strict).expect_err("Test Sans is not available");
    assert!(error.to_string().contains("font unavailable"), "{error}");
    let assets =
        FontAssets::new(vec![arimo("Test Sans")], FontAssetLimits::default()).expect("assets");
    render(&strict.font_assets(Arc::new(assets))).expect("supplied, so it renders");
}

#[test]
fn a_set_is_pinned_bounded_and_named_by_its_hash() {
    let wrong_pin = arimo("Test Sans").pinned([0; 32]);
    assert_eq!(
        FontAssets::new(vec![wrong_pin], FontAssetLimits::default()),
        Err(FontAssetError::HashMismatch("Test Sans".to_owned()))
    );
    assert_eq!(
        FontAssets::new(
            vec![arimo("Test Sans"), arimo("TEST SANS")],
            FontAssetLimits::default()
        ),
        Err(FontAssetError::Duplicate("TEST SANS".to_owned()))
    );
    assert_eq!(
        FontAssets::new(
            vec![FontAsset::new("Junk", false, false, vec![0u8; 64])],
            FontAssetLimits::default()
        ),
        Err(FontAssetError::Invalid("Junk".to_owned()))
    );
    let small = FontAssetLimits {
        max_asset_bytes: 1024,
        ..FontAssetLimits::default()
    };
    assert_eq!(
        FontAssets::new(vec![arimo("Test Sans")], small),
        Err(FontAssetError::TooLarge("Test Sans".to_owned()))
    );
    let one = FontAssets::new(vec![arimo("Test Sans")], FontAssetLimits::default()).expect("set");
    let again = FontAssets::new(vec![arimo("Test Sans")], FontAssetLimits::default()).expect("set");
    let other =
        FontAssets::new(vec![arimo("Other Sans")], FontAssetLimits::default()).expect("set");
    assert_eq!(one.set_hash(), again.set_hash());
    assert_ne!(one.set_hash(), other.set_hash());
    assert_eq!(one.families(), ["test sans"]);
}

/// A collection with Arimo as its only face: the font's table records moved
/// past a 16-byte `ttcf` header.
fn collection() -> Vec<u8> {
    let font = face_source("Arial", false, false)
        .expect("bundled Arimo")
        .data;
    let tables = usize::from(u16::from_be_bytes([font[4], font[5]]));
    let mut moved = font.to_vec();
    for table in 0..tables {
        let at = 12 + 16 * table + 8;
        let offset = u32::from_be_bytes([moved[at], moved[at + 1], moved[at + 2], moved[at + 3]]);
        moved[at..at + 4].copy_from_slice(&(offset + 16).to_be_bytes());
    }
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    out.extend_from_slice(&1_u32.to_be_bytes());
    out.extend_from_slice(&16_u32.to_be_bytes());
    out.extend_from_slice(&moved);
    out
}

#[test]
fn a_face_is_taken_out_of_a_collection() {
    let ttc = collection();
    let face = strict_ooxml_render_svg::font::assets::single_face(&ttc, 0).expect("face 0");
    assert_eq!(
        &face[..4],
        &face_source("Arial", false, false).expect("Arimo").data[..4]
    );
    let assets = FontAssets::new(
        vec![FontAsset::new("Test Sans", false, false, ttc.clone()).face_index(0)],
        FontAssetLimits::default(),
    )
    .expect("the face of a collection is a font");
    let supplied = render(&RenderOptions::default().font_assets(Arc::new(assets))).expect("render");
    assert!(supplied[0].svg.contains("ASSET-"));
    assert_eq!(
        FontAssets::new(
            vec![FontAsset::new("Test Sans", false, false, ttc).face_index(1)],
            FontAssetLimits::default()
        ),
        Err(FontAssetError::Invalid("Test Sans".to_owned()))
    );
}
