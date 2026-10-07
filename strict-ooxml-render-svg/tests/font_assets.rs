//! RC02 explicit registry/owned-consumer behavioral tests, without installed fonts.
use std::sync::Arc;

use strict_ooxml_render_svg::font::{
    hash_face_bytes, resolve_face, shape_bundled, shape_resource, unicode_x_positions_px,
    BuiltinFontProvider, FaceMapping, FaceVariation, FontAsset, FontAssetManifest,
    FontAssetRegistry, FontAssetRejection, FontEmbeddingPermission, FontProvider,
    FontRegistryError, FontRegistryLimits, ShapeStatus,
};

fn asset(request: &str, source: &str, bold: bool) -> FontAsset {
    let face = resolve_face(source, bold, false).unwrap();
    FontAsset {
        bytes: Arc::from(face.bytes),
        manifest: FontAssetManifest {
            family: request.into(),
            resolved_family: face.family.into(),
            bold,
            italic: false,
            resource_hash: face.resource_hash,
            face_index: 0,
            variations: face.instance_key().variations,
            source: "compiled bundled font program".into(),
            license_id: "OFL-1.1".into(),
            license_reference: "strict-ooxml-render-svg/fonts licenses".into(),
            embedding: FontEmbeddingPermission::EmbedAndSubset,
        },
    }
}

fn rejection(asset: FontAsset) -> FontAssetRejection {
    match FontAssetRegistry::from_assets(vec![asset], FontRegistryLimits::default()).unwrap_err() {
        FontRegistryError::Asset { index: 0, reason } => reason,
        error => panic!("unexpected failure: {error:?}"),
    }
}

#[test]
fn owned_measurement_shaping_and_lifetime_match_the_pinned_program() {
    let input = asset("Arimo", "Arial", false);
    let original_bytes = Arc::clone(&input.bytes);
    let registry =
        FontAssetRegistry::from_assets(vec![input], FontRegistryLimits::default()).unwrap();
    let face = registry.resolve("  ARIMO ", false, false).unwrap();
    assert_eq!(face.requested_family.as_ref(), "  ARIMO ");
    assert_eq!(face.mapping, FaceMapping::ExactFamily);
    assert_eq!(face.resource.bytes().as_ptr(), original_bytes.as_ptr());
    drop(original_bytes);
    drop(registry);
    let provider = BuiltinFontProvider::new();
    assert_eq!(
        face.resource.metrics().unwrap(),
        provider.metrics("Arial", false, false)
    );
    for ch in "AV office".chars() {
        assert!(face.resource.glyph_id(ch).is_some());
        assert!(
            (face.resource.advance_em(ch).unwrap()
                - provider.advance_em("Arial", ch, false, false))
            .abs()
                < 1e-12
        );
    }
    let text = "AV office e\u{301}";
    let owned = shape_resource(text, &face).unwrap();
    let bundled = shape_bundled(text, "Arial", false, false).unwrap();
    assert_eq!(owned.face.resource.key(), &bundled.face.instance_key());
    assert_eq!(owned.glyphs, bundled.glyphs);
    assert_eq!(owned.clusters, bundled.clusters);
    assert_eq!(
        unicode_x_positions_px(text, &owned, 13.0, 16.0),
        unicode_x_positions_px(text, &bundled, 13.0, 16.0)
    );
    assert_eq!(owned.status, ShapeStatus::Full);
}

#[test]
fn registry_is_order_independent_and_does_not_fabricate_a_style_or_fallback() {
    let first = asset("Alias", "Times New Roman", false);
    let second = asset("Arimo", "Arial", true);
    let a = FontAssetRegistry::from_assets(
        vec![first.clone(), second.clone()],
        FontRegistryLimits::default(),
    )
    .unwrap();
    let b =
        FontAssetRegistry::from_assets(vec![second, first], FontRegistryLimits::default()).unwrap();
    assert_eq!(
        a.manifests().collect::<Vec<_>>(),
        b.manifests().collect::<Vec<_>>()
    );
    assert_eq!(a.len(), 2);
    assert_eq!(
        a.resolve("Alias", false, false).unwrap().mapping,
        FaceMapping::Substitute
    );
    assert!(a.resolve("Alias", true, false).is_none());
    assert!(a.resolve("Arimo", false, false).is_none());
    assert!(a.resolve("missing family", false, false).is_none());
    assert!(a.resolve("Alias\n", false, false).is_none());
    assert!(FontAssetRegistry::default().is_empty());
}

#[test]
fn variable_instance_and_missing_glyphs_remain_explicit() {
    let regular = asset("Arimo", "Arial", false);
    let bold = asset("Arimo", "Arial", true);
    let registry =
        FontAssetRegistry::from_assets(vec![regular, bold], FontRegistryLimits::default()).unwrap();
    let regular = registry.resolve("Arimo", false, false).unwrap();
    let bold = registry.resolve("Arimo", true, false).unwrap();
    assert_eq!(
        regular.resource.key().resource_hash,
        bold.resource.key().resource_hash
    );
    assert_ne!(regular.resource.key(), bold.resource.key());
    let shaped = shape_resource("office", &bold).unwrap();
    assert_eq!(
        shaped.glyphs,
        shape_bundled("office", "Arial", true, false)
            .unwrap()
            .glyphs
    );
    assert!(regular.resource.glyph_id('\u{6F22}').is_none());
    assert!(regular.resource.advance_em('\u{6F22}').is_none());
    let missing = shape_resource("\u{6F22}", &regular).unwrap();
    assert_eq!(missing.status, ShapeStatus::DegradedMissingGlyph);
    assert_eq!(missing.clusters[0].byte_end, "\u{6F22}".len());
    assert!(missing.glyphs.iter().any(|glyph| glyph.glyph_id == 0));
    let empty = shape_resource("", &regular).unwrap();
    assert!(empty.glyphs.is_empty());
    assert!(empty.total_advance_em.abs() <= f64::EPSILON);
}

#[test]
fn all_budgets_are_checked_before_hash_or_parse() {
    let input = asset("Arimo", "Arial", false);
    let limits = FontRegistryLimits {
        max_asset_bytes: input.bytes.len() - 1,
        ..FontRegistryLimits::default()
    };
    assert!(matches!(
        FontAssetRegistry::from_assets(vec![input.clone()], limits),
        Err(FontRegistryError::Asset {
            reason: FontAssetRejection::LimitExceeded,
            ..
        })
    ));
    let limits = FontRegistryLimits {
        max_total_bytes: input.bytes.len(),
        ..FontRegistryLimits::default()
    };
    let other = asset("Other", "Arial", false);
    assert!(matches!(
        FontAssetRegistry::from_assets(vec![input.clone(), other.clone()], limits),
        Err(FontRegistryError::Asset {
            index: 1,
            reason: FontAssetRejection::LimitExceeded
        })
    ));
    let limits = FontRegistryLimits {
        max_assets: 1,
        ..FontRegistryLimits::default()
    };
    assert_eq!(
        FontAssetRegistry::from_assets(vec![input.clone(), other], limits).unwrap_err(),
        FontRegistryError::TooManyAssets
    );
    let limits = FontRegistryLimits {
        max_metadata_bytes: 1,
        ..FontRegistryLimits::default()
    };
    assert!(matches!(
        FontAssetRegistry::from_assets(vec![input.clone()], limits),
        Err(FontRegistryError::Asset {
            reason: FontAssetRejection::InvalidMetadata,
            ..
        })
    ));
    let limits = FontRegistryLimits {
        max_variations: 1,
        ..FontRegistryLimits::default()
    };
    let mut excess = input.clone();
    excess
        .manifest
        .variations
        .push(FaceVariation::new(*b"xxxx", 0.0).unwrap());
    assert!(matches!(
        FontAssetRegistry::from_assets(vec![excess], limits),
        Err(FontRegistryError::Asset {
            reason: FontAssetRejection::LimitExceeded,
            ..
        })
    ));
    let limits = FontRegistryLimits {
        max_assets: usize::MAX,
        ..FontRegistryLimits::default()
    };
    assert_eq!(
        FontAssetRegistry::from_assets(vec![input.clone()], limits).unwrap_err(),
        FontRegistryError::InvalidLimits
    );
    let mut bad_hash = input.clone();
    bad_hash.manifest.resource_hash = [0; 32];
    let mut missing_permission = input;
    missing_permission.manifest.family = "Other".into();
    missing_permission.manifest.embedding = FontEmbeddingPermission::Forbidden;
    // Complete preflight rejects entry 1 before the corrupt hash in entry 0 is processed.
    assert_eq!(
        FontAssetRegistry::from_assets(
            vec![bad_hash, missing_permission],
            FontRegistryLimits::default()
        )
        .unwrap_err(),
        FontRegistryError::Asset {
            index: 1,
            reason: FontAssetRejection::PermissionMissing
        }
    );
}

#[test]
fn pinned_hash_metadata_index_variations_and_duplicate_bindings_are_validated() {
    let input = asset("Arimo", "Arial", false);
    let mut bad = input.clone();
    bad.manifest.resource_hash = [0; 32];
    assert_eq!(rejection(bad), FontAssetRejection::HashMismatch);
    let mut bad = input.clone();
    bad.manifest.face_index = 1;
    assert!(matches!(
        rejection(bad),
        FontAssetRejection::InvalidResource(_)
    ));
    let mut bad = input.clone();
    bad.manifest
        .variations
        .push(FaceVariation::new(*b"xxxx", 0.0).unwrap());
    assert!(matches!(
        rejection(bad),
        FontAssetRejection::InvalidResource(_)
    ));
    let mut bad = input.clone();
    bad.manifest.license_reference.clear();
    assert_eq!(rejection(bad), FontAssetRejection::InvalidMetadata);
    let mut bad = input.clone();
    bad.manifest.family = "\n".into();
    assert_eq!(rejection(bad), FontAssetRejection::InvalidMetadata);
    let mut duplicate = input.clone();
    duplicate.manifest.family = " ARIMO ".into();
    assert_eq!(
        FontAssetRegistry::from_assets(vec![input, duplicate], FontRegistryLimits::default())
            .unwrap_err(),
        FontRegistryError::Asset {
            index: 1,
            reason: FontAssetRejection::DuplicateBinding
        }
    );
}

fn with_embedding_flags(mut input: FontAsset, flags: u16) -> FontAsset {
    let mut bytes = input.bytes.to_vec();
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    let record = (0..count)
        .map(|index| 12 + index * 16)
        .find(|offset| &bytes[*offset..*offset + 4] == b"OS/2")
        .unwrap();
    let offset = usize::try_from(u32::from_be_bytes(
        bytes[record + 8..record + 12].try_into().unwrap(),
    ))
    .unwrap();
    bytes[offset + 8..offset + 10].copy_from_slice(&flags.to_be_bytes());
    input.manifest.resource_hash = hash_face_bytes(&bytes);
    input.bytes = Arc::from(bytes);
    input
}

#[test]
fn embedding_flags_override_a_permissive_attestation() {
    for flags in [0x0002, 0x0100, 0x0200, 0x8000] {
        assert_eq!(
            rejection(with_embedding_flags(asset("Arimo", "Arial", false), flags)),
            FontAssetRejection::EmbeddingRestricted
        );
    }
    // Even a permissive font flag cannot replace absent caller permission evidence.
    let mut input = with_embedding_flags(asset("Arimo", "Arial", false), 0);
    input.manifest.embedding = FontEmbeddingPermission::Forbidden;
    assert_eq!(rejection(input), FontAssetRejection::PermissionMissing);
}
