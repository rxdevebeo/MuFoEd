//! Independent acceptance probe for layout and PDF shaping consistency.
mod common;
use strict_ooxml_render_svg::{font::shape_bundled, layout::Item, place_pages};
#[test]
fn justified_layout_matches_pdf_shaping_advance() {
    for alignment in ["left", "both"] {
        let body = format!(r#"<w:p><w:pPr><w:jc w:val="{alignment}"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Carlito" w:hAnsi="Carlito"/><w:sz w:val="64"/></w:rPr><w:t>To</w:t></w:r></w:p>"#);
        let (_, document) = common::open_body(&body);
        let pages = place_pages(&document, &Default::default(), None).unwrap();
        let text = pages.iter().flat_map(|p| &p.items).find_map(|i| match i { Item::Text(t) if t.text == "To" => Some(t), _ => None }).unwrap();
        let shaped = shape_bundled(&text.text, &text.run.family, false, false).unwrap();
        let pdf_width = shaped.total_advance_em * text.size_px;
        println!("alignment={alignment}, layout={}, pdf_shaped={pdf_width}, delta={}", text.width, (text.width-pdf_width).abs());
        assert!((text.width-pdf_width).abs() <= 0.25, "layout and PDF shaping disagree for {alignment}");
    }
}
