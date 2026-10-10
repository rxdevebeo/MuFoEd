//! Public import outcomes and package-backed media/structure contracts.
#![cfg(feature = "rtf")]
use strict_ooxml::{
    model::{Block, Inline, RunContent},
    RtfOptions, RtfStatus, StrictDocument,
};

#[test]
fn clean_and_degraded_are_independent_of_docx_support() {
    let clean = StrictDocument::import_rtf(br"{\rtf1 Hello}", &RtfOptions::default()).unwrap();
    assert_eq!(clean.status(), RtfStatus::Clean);
    assert_eq!(clean.document.document().body.blocks.len(), 1);
    assert!(clean.strict_bytes.starts_with(b"PK"));
    let mut options = RtfOptions::default();
    options.limits.issues = 0;
    let degraded = StrictDocument::import_rtf(br"{\rtf1 A\unknowncontrol B}", &options).unwrap();
    assert_eq!(degraded.status(), RtfStatus::Degraded);
    assert!(degraded.import_report.details_truncated);
    assert_eq!(degraded.import_report.loss_count, 1);
    assert!(degraded.write_report.losses().is_empty());
    assert!(StrictDocument::import_rtf(br"{\rtf1 unfinished", &options).is_err());
}

#[cfg(feature = "svg")]
#[test]
fn imported_east_asian_font_reaches_the_svg_face_choice() {
    let source = br"{\rtf1\ansi{\fonttbl{\f0 Arial;}{\f1 Times New Roman;}}\loch\f0 Latin \dbch\af1\u28450?\u23383?}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    assert_eq!(opened.status(), RtfStatus::Clean);
    let pages = opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap();
    assert!(
        pages[0].svg.contains("Arimo"),
        "Latin face was not selected"
    );
    assert!(
        pages[0].svg.contains("Tinos"),
        "East Asian face was not selected"
    );
}

#[cfg(feature = "svg")]
#[test]
fn note_numbering_properties_reach_the_svg_marker() {
    // `\ftnnar`/`\ftnstart` must reach `settings.w:footnotePr` so the rendered
    // footnote marker is the source's number, not the default "1".
    let source = br"{\rtf1\ftnnalc\ftnstart3 A\chftn{\footnote\chftn Note}\par}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    assert_eq!(opened.status(), RtfStatus::Clean);
    let svg = &opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap()[0]
        .svg;
    assert!(
        svg.contains(">c<"),
        "footnote marker did not use \\ftnnalc/\\ftnstart3"
    );
    assert!(!svg.contains(">1<"), "default footnote marker leaked");
}

#[test]
fn public_path_keeps_region_references_and_cached_link_result() {
    let opened = StrictDocument::import_rtf(br#"{\rtf1{\header Header}A\chftn{\footnote\chftn Note}{\field{\*\fldinst HYPERLINK "https://example.test"}{\fldrslt Link}}}"#, &RtfOptions::default()).unwrap();
    let d = opened.document.document();
    assert_eq!(d.headers_footers.len(), 1);
    assert_eq!(d.footnotes.len(), 1);
    let Block::Paragraph(p) = &d.body.blocks[0] else {
        panic!()
    };
    assert!(p
        .inlines
        .iter()
        .any(|i| matches!(i,Inline::Run(r) if r.content.contains(&RunContent::FootnoteRef(1)))));
    assert!(p.inlines.iter().any(|i| matches!(i,Inline::Field(f) if f.instruction.as_deref() == Some("HYPERLINK \"https://example.test\""))));
    #[cfg(feature = "svg")]
    assert!(!opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap()
        .is_empty());
}

#[test]
fn imported_media_outlives_input_and_initial_package_bytes() {
    let hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63606060f80f00010401005fe5c34b0000000049454e44ae426082";
    let png: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    let mut source = format!(
        "{{\\rtf1 before{{\\pict\\pngblip\\picw1\\pich1\\picwgoal15\\pichgoal15\\bin{} ",
        png.len()
    )
    .into_bytes();
    source.extend_from_slice(&png);
    source.extend_from_slice(b"}after}");
    let opened = StrictDocument::import_rtf(&source, &RtfOptions::default()).unwrap();
    assert_eq!(opened.status(), RtfStatus::Clean);
    let document = opened.document;
    drop(opened.strict_bytes);
    drop(source);
    let image = document.document().media.iter().next().unwrap();
    assert_eq!(document.package().read_part(&image.part).unwrap(), png);
    #[cfg(feature = "svg")]
    assert!(document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap()[0]
        .svg
        .contains("data:image/png;base64,"));
}

#[cfg(all(feature = "svg", feature = "pdf"))]
#[test]
fn imported_script_font_slots_accept_explicit_resources_in_both_backends() {
    use std::sync::Arc;
    use strict_ooxml_render_svg::font::{
        resolve_face, FontAsset, FontAssetManifest, FontAssetRegistry, FontEmbeddingPermission,
        FontRegistryLimits,
    };
    let source = br"{\rtf1\ansi{\fonttbl{\f0 Latin Alias;}{\f1 CJK Alias;}}\loch\f0 AV office \dbch\af1\u28450?\u23383?}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    let assets = [("Latin Alias", "Arial"), ("CJK Alias", "Times New Roman")]
        .into_iter()
        .map(|(request, family)| {
            let face = resolve_face(family, false, false).unwrap();
            FontAsset {
                bytes: Arc::from(face.bytes),
                manifest: FontAssetManifest {
                    family: request.into(),
                    resolved_family: face.family.into(),
                    bold: false,
                    italic: false,
                    resource_hash: face.resource_hash,
                    face_index: 0,
                    variations: face.instance_key().variations,
                    source: "bundled test fixture".into(),
                    license_id: "OFL-1.1".into(),
                    license_reference: "bundled font licenses".into(),
                    embedding: FontEmbeddingPermission::EmbedAndSubset,
                },
            }
        })
        .collect();
    let registry =
        Arc::new(FontAssetRegistry::from_assets(assets, FontRegistryLimits::default()).unwrap());
    let options = strict_ooxml::RenderOptions::default().font_assets(registry);
    let svg = opened.document.render_svg(&options).unwrap();
    assert!(svg[0].svg.contains("StrictAsset-"));
    assert!(svg[0]
        .warnings
        .iter()
        .any(|warning| warning.contains("font.glyph.missing: CJK Alias")));
    let pdf = opened.document.render_pdf(&options).unwrap();
    assert_eq!(pdf.embedded_faces.len(), 2);
    assert!(pdf
        .embedded_faces
        .iter()
        .all(|face| face.instance.is_some()));
    assert!(
        !pdf.report.losses().is_empty(),
        "fixture deliberately has no CJK outlines"
    );
}

#[cfg(all(feature = "svg", feature = "pdf"))]
#[test]
fn public_rtf_font_reject_policy_refuses_unknown_request_in_both_backends() {
    use strict_ooxml_render_svg::font::FontFailurePolicy;
    let source = br"{\rtf1{\fonttbl{\f0 Unavailable RTF Face;}}\f0 AV office}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    let options = strict_ooxml::RenderOptions {
        font_report_limit: 0,
        ..strict_ooxml::RenderOptions::default().font_failure_policy(FontFailurePolicy::Reject)
    };
    for error in [
        opened.document.render_svg(&options).unwrap_err(),
        opened.document.render_pdf(&options).unwrap_err(),
    ] {
        assert!(error
            .to_string()
            .contains("font.unresolved: Unavailable RTF Face"));
    }
    assert!(opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .is_ok());
    assert!(opened
        .document
        .render_pdf(&strict_ooxml::RenderOptions::default())
        .is_ok());
}

#[cfg(all(feature = "svg", feature = "pdf"))]
#[test]
fn explicit_continue_policy_reports_bundled_fallback_without_registry() {
    use strict_ooxml_render_svg::font::{FontDiagnosticKind, FontFailurePolicy};
    let opened = StrictDocument::import_rtf(
        br"{\rtf1{\fonttbl{\f0 Unavailable RTF Face;}}\f0 AV office}",
        &RtfOptions::default(),
    )
    .unwrap();
    let options = strict_ooxml::RenderOptions::default()
        .font_failure_policy(FontFailurePolicy::ReportAndContinue);
    let svg = opened.document.render_svg(&options).unwrap();
    let pdf = opened.document.render_pdf(&options).unwrap();
    for report in [&svg[0].font_report, pdf.report.font_report()] {
        let unresolved = report
            .diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.kind == FontDiagnosticKind::Unresolved)
            .unwrap();
        assert_eq!(unresolved.requested_family, "Unavailable RTF Face");
        assert_eq!(unresolved.resolved_family.as_deref(), Some("Carlito"));
        assert!(!report.is_clean());
    }
}

#[cfg(all(feature = "svg", feature = "pdf"))]
#[test]
fn imported_east_asian_slot_delivers_real_cjk_in_strict_mode() {
    use std::sync::Arc;
    use strict_ooxml_render_svg::font::{
        hash_face_bytes, FontAsset, FontAssetManifest, FontAssetRegistry, FontEmbeddingPermission,
        FontFailurePolicy, FontRegistryLimits,
    };
    let bytes = &include_bytes!("../../strict-ooxml-render-svg/fonts/witnesses/cjk.otf")[..];
    let family = "StrictLib Witness CJK SC";
    let source = br"{\rtf1\ansi{\fonttbl{\f0 Arial;}{\f1 StrictLib Witness CJK SC;}}\loch\f0 AV \dbch\af1\u28450?\u23383?}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    let registry = FontAssetRegistry::from_assets(
        vec![FontAsset {
            bytes: Arc::from(bytes),
            manifest: FontAssetManifest {
                family: family.into(),
                resolved_family: family.into(),
                bold: false,
                italic: false,
                resource_hash: hash_face_bytes(bytes),
                face_index: 0,
                variations: vec![],
                source: "pinned Noto CJK derivative; fonts/witnesses/manifest.json".into(),
                license_id: "OFL-1.1".into(),
                license_reference: "fonts/witnesses/CJK-OFL.txt and cjk-COPYRIGHT.txt".into(),
                embedding: FontEmbeddingPermission::EmbedAndSubset,
            },
        }],
        FontRegistryLimits::default(),
    )
    .unwrap();
    let options = strict_ooxml::RenderOptions::default()
        .font_assets(Arc::new(registry))
        .font_failure_policy(FontFailurePolicy::Reject);
    let svg = opened.document.render_svg(&options).unwrap();
    assert!(svg[0].font_report.is_clean());
    assert!(svg[0].svg.contains("漢字"));
    let pdf = opened.document.render_pdf(&options).unwrap();
    assert!(pdf.report.is_clean());
    assert!(pdf
        .embedded_faces
        .iter()
        .any(|face| face.instance.is_some()));
}
