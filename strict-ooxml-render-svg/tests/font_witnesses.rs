//! Pinned OFL subsets compared with independent native `HarfBuzz` (see manifest).
mod common;
use std::sync::Arc;
use strict_ooxml_render_svg::font::{
    hash_face_bytes, shape_resource, FontAsset, FontAssetManifest, FontAssetRegistry,
    FontDiagnosticKind, FontEmbeddingPermission, FontFailurePolicy, FontRegistryLimits,
};
use strict_ooxml_render_svg::{render, RenderOptions};

fn asset(family: &str, bytes: &'static [u8]) -> FontAsset {
    FontAsset {
        bytes: Arc::from(bytes),
        manifest: FontAssetManifest {
            family: family.into(),
            resolved_family: family.into(),
            bold: false,
            italic: false,
            resource_hash: hash_face_bytes(bytes),
            face_index: 0,
            variations: vec![],
            source: "pinned Noto-derived finite witness; fonts/witnesses/manifest.json".into(),
            license_id: "OFL-1.1".into(),
            license_reference: "fonts/witnesses licenses and copyright".into(),
            embedding: FontEmbeddingPermission::EmbedAndSubset,
        },
    }
}

fn verify(family: &str, bytes: &'static [u8], text: &str, oracle: &str, degraded: bool) {
    let registry = Arc::new(
        FontAssetRegistry::from_assets(vec![asset(family, bytes)], FontRegistryLimits::default())
            .unwrap(),
    );
    let face = registry.resolve(family, false, false).unwrap();
    for ch in text.chars() {
        assert!(face.resource.glyph_id(ch).is_some(), "{ch}");
    }
    let shaped = shape_resource(text, &face).unwrap();
    let mut lines = oracle.lines();
    let upem: f64 = lines.next().unwrap().parse().unwrap();
    let rows: Vec<Vec<f64>> = lines
        .map(|line| {
            line.split('\t')
                .map(|value| value.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(shaped.glyphs.len(), rows.len());
    let mut origin = 0.0;
    for (glyph, row) in shaped.glyphs.iter().zip(rows) {
        assert_eq!(glyph.glyph_id.to_string(), format!("{:.0}", row[0]));
        assert_eq!(glyph.byte_start.to_string(), format!("{:.0}", row[1]));
        assert!((glyph.x_advance_em - row[2] / upem).abs() < 1e-9);
        assert!((glyph.x_offset_em - row[3] / upem).abs() < 1e-9);
        assert!((glyph.y_offset_em - row[4] / upem).abs() < 1e-9);
        assert!((glyph.x_em - (origin + row[3]) / upem).abs() < 1e-9);
        origin += row[2];
    }
    let (_, doc) = common::open_body(&format!("<w:p><w:r><w:rPr><w:rFonts w:ascii=\"{family}\" w:eastAsia=\"{family}\" w:cs=\"{family}\"/></w:rPr><w:t>{text}</w:t></w:r></w:p>"));
    let options = RenderOptions::default().font_assets(registry);
    let page = render(&doc, &options).unwrap().remove(0);
    assert!(page.svg.contains("data:font/"));
    assert!(!page.font_report.diagnostics().iter().any(|d| matches!(
        d.kind,
        FontDiagnosticKind::MissingGlyph
            | FontDiagnosticKind::ShapingFailed
            | FontDiagnosticKind::UnsupportedDelivery
    )));
    assert_eq!(page.font_report.is_clean(), !degraded);
    let strict = render(
        &doc,
        &options.font_failure_policy(FontFailurePolicy::Reject),
    );
    if degraded {
        assert!(strict
            .unwrap_err()
            .to_string()
            .contains("font.script.degraded"));
    } else {
        assert!(strict.unwrap()[0].font_report.is_clean());
    }
}

#[test]
fn cjk_owned_geometry_matches_native_harfbuzz_and_strict_svg_delivers() {
    verify(
        "StrictLib Witness CJK SC",
        include_bytes!("../fonts/witnesses/cjk.otf"),
        include_str!("../fonts/witnesses/cjk.txt"),
        include_str!("../fonts/witnesses/cjk.tsv"),
        false,
    );
}

#[test]
fn devanagari_owned_geometry_matches_native_harfbuzz_without_claiming_script_acceptance() {
    verify(
        "StrictLib Witness Devanagari",
        include_bytes!("../fonts/witnesses/devanagari.ttf"),
        include_str!("../fonts/witnesses/devanagari.txt"),
        include_str!("../fonts/witnesses/devanagari.tsv"),
        true,
    );
}

/// RC03: every scalar survives in logical order, one x per scalar, and a
/// bidirectional script actually places its logical start away from the visual
/// left edge (proving reordering ran rather than a left-to-right accumulation).
fn assert_logical_order_is_preserved(
    family: &str,
    bytes: &'static [u8],
    text: &str,
    expect_visual_reorder: bool,
) {
    use strict_ooxml_render_svg::font::{
        shape_resource, unicode_cluster_map, unicode_x_positions_px,
    };
    let registry = Arc::new(
        FontAssetRegistry::from_assets(vec![asset(family, bytes)], FontRegistryLimits::default())
            .unwrap(),
    );
    let face = registry.resolve(family, false, false).unwrap();
    let shaped = shape_resource(text, &face).unwrap();
    let map = unicode_cluster_map(text, &shaped);
    let joined: String = map.iter().map(|(ch, _, _)| *ch).collect();
    assert_eq!(joined, text, "logical scalar order must be preserved");
    assert_eq!(
        shaped.clusters.first().map(|cluster| cluster.byte_start),
        Some(0)
    );
    for cluster in &shaped.clusters {
        assert!(cluster.byte_start < cluster.byte_end, "{cluster:?}");
        assert!(text.is_char_boundary(cluster.byte_start));
        assert!(text.is_char_boundary(cluster.byte_end));
    }
    let positions = unicode_x_positions_px(text, &shaped, 0.0, 16.0);
    assert_eq!(positions.len(), text.chars().count());
    if expect_visual_reorder {
        let first_x = positions.first().copied().unwrap();
        let min_x = positions.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            first_x > min_x + 1e-6,
            "RTL logical start must sit right of the visual left edge: {positions:?}"
        );
    }
}

#[test]
fn arabic_rtl_geometry_matches_native_harfbuzz_and_reorders_visually() {
    let family = "StrictLib Witness Arabic";
    let bytes = include_bytes!("../fonts/witnesses/arabic.ttf");
    let text = include_str!("../fonts/witnesses/arabic.txt");
    verify(
        family,
        bytes,
        text,
        include_str!("../fonts/witnesses/arabic.tsv"),
        true,
    );
    assert_logical_order_is_preserved(family, bytes, text, true);
}

#[test]
fn tamil_geometry_matches_native_harfbuzz_and_keeps_logical_order() {
    let family = "StrictLib Witness Tamil";
    let bytes = include_bytes!("../fonts/witnesses/tamil.ttf");
    let text = include_str!("../fonts/witnesses/tamil.txt");
    verify(
        family,
        bytes,
        text,
        include_str!("../fonts/witnesses/tamil.tsv"),
        true,
    );
    assert_logical_order_is_preserved(family, bytes, text, false);
}

#[test]
fn bengali_geometry_matches_native_harfbuzz_and_keeps_logical_order() {
    let family = "StrictLib Witness Bengali";
    let bytes = include_bytes!("../fonts/witnesses/bengali.ttf");
    let text = include_str!("../fonts/witnesses/bengali.txt");
    verify(
        family,
        bytes,
        text,
        include_str!("../fonts/witnesses/bengali.tsv"),
        true,
    );
    assert_logical_order_is_preserved(family, bytes, text, false);
}
