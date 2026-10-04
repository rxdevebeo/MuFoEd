//! Public facade keeps the source and live model coherent through edits.
#![cfg(feature = "edit")]
use strict_ooxml::{
    edit::{Address, Edit, EditLimits, SavePolicy},
    OpenOptions, RenderOptions, StrictDocument, WriteOptions,
};
use strict_ooxml_testkit::DocxBuilder;
#[test]
fn facade_edit_preview_save_refresh_and_reopen() {
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>abc</w:t></w:r></w:p>")
        .build();
    let mut document = StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let saved = {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        editor
            .transact(
                0,
                &[Edit::Text {
                    at: Address::body(0),
                    range: 1..2,
                    text: "🙂".into(),
                }],
            )
            .unwrap();
        let preview = editor.render_svg(&RenderOptions::default()).unwrap();
        assert!(preview.iter().any(|p| p.svg.contains("🙂")));
        editor.visual_map(1, &RenderOptions::default()).unwrap();
        let saved = editor
            .save(1, &WriteOptions::default(), SavePolicy::Lossless)
            .unwrap();
        editor.refresh_support(1, &WriteOptions::default()).unwrap();
        saved
    };
    assert!(!document.support_is_stale());
    let reopened =
        StrictDocument::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    assert!(reopened.render_svg(&RenderOptions::default()).unwrap()[0]
        .svg
        .contains("🙂"));
    {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        editor
            .transact(
                0,
                &[Edit::Split {
                    at: Address::body(0),
                    offset: 1,
                }],
            )
            .unwrap();
    }
    assert!(document.support_is_stale());
    #[cfg(feature = "report")]
    assert!(document
        .support_report()
        .features
        .iter()
        .any(|f| f.feature_id == "edit.support-stale"));
    {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        assert!(editor.support_is_stale());
        editor.refresh_support(0, &WriteOptions::default()).unwrap();
        assert!(!editor.support_is_stale());
    }
    assert!(!document.support_is_stale());
}
