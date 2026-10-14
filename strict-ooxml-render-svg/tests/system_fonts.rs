//! The machine's installed fonts as caller-supplied fonts (feature
//! `system-fonts`), exercised on a directory of our own.
#![cfg(feature = "system-fonts")]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::open_body;
use strict_ooxml_render_svg::font::system::{assets_for, document_families, scan};
use strict_ooxml_render_svg::font::{face_source, FontAssetLimits, FontAssets};
use strict_ooxml_render_svg::{render_with_media, RenderOptions};

#[test]
fn installed_faces_are_found_by_the_family_a_document_names() {
    let dir = std::env::temp_dir().join(format!("strict-system-fonts-{}", std::process::id()));
    let nested = dir.join("nested");
    std::fs::create_dir_all(&nested).expect("dir");
    let arimo = face_source("Arial", false, false)
        .expect("Arimo")
        .data
        .to_vec();
    std::fs::write(nested.join("Arimo.TTF"), &arimo).expect("font");
    std::fs::write(dir.join("readme.txt"), b"not a font").expect("text");
    let faces = scan(&[dir.clone()]);
    assert!(
        faces
            .iter()
            .any(|face| face.family == "Arimo" && !face.bold),
        "{faces:?}"
    );

    let body = "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"arimo\" w:hAnsi=\"arimo\"/></w:rPr>\
<w:t>Installed</w:t></w:r></w:p>";
    let (package, document) = open_body(body);
    // A family the document names only in its runs is found through the
    // caller's list as well.
    let named = vec!["ARIMO".to_owned()];
    let assets = assets_for(&named, &faces, FontAssetLimits::default()).expect("assets");
    assert_eq!(assets.len(), 1);
    let set = FontAssets::new(assets, FontAssetLimits::default()).expect("set");
    let pages = render_with_media(
        &document,
        &RenderOptions::default().font_assets(Arc::new(set)),
        Some(&package),
    )
    .expect("render");
    assert!(pages[0].svg.contains("ASSET-"));
    assert!(
        document_families(&document).is_empty(),
        "the test body has no font table"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
