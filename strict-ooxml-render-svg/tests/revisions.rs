//! AUD-43: Final vs Original tracked-change views.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::unreadable_literal
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{render, RenderOptions, RevisionView};

fn svg_text(body: &str, revisions: RevisionView) -> String {
    let entries = [
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ];
    let (_package, parsed) = open_bytes(build_docx(&entries));
    let pages = render(
        &parsed,
        &RenderOptions {
            revisions,
            ..RenderOptions::default()
        },
    )
    .expect("render");
    pages[0].svg.clone()
}

#[test]
fn final_view_hides_deleted_and_keeps_inserted() {
    let body = "\
<w:p><w:ins w:id=\"1\"><w:r><w:t>added</w:t></w:r></w:ins>\
<w:del w:id=\"2\"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>";
    let svg = svg_text(body, RevisionView::Final);
    assert!(svg.contains(">added<"), "{svg}");
    assert!(!svg.contains(">gone<"), "{svg}");
}

#[test]
fn original_view_hides_inserted_and_keeps_deleted() {
    let body = "\
<w:p><w:ins w:id=\"1\"><w:r><w:t>added</w:t></w:r></w:ins>\
<w:del w:id=\"2\"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>";
    let svg = svg_text(body, RevisionView::Original);
    assert!(!svg.contains(">added<"), "{svg}");
    assert!(svg.contains(">gone<"), "{svg}");
}
