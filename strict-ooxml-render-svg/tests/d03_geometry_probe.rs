//! D03: layout width matches the advance geometry stamped on `TextItem` (SVG/PDF/caret).
mod common;
use strict_ooxml_render_svg::font::{shape_bundled, BuiltinFontProvider, FontProvider};
use strict_ooxml_render_svg::layout::Item;
use strict_ooxml_render_svg::style::chosen_family;
use strict_ooxml_render_svg::{place_pages, RenderOptions, TextAdvanceKind};

#[test]
fn justified_layout_matches_declared_advance_geometry() {
    for (alignment, expected_kind) in [
        ("left", TextAdvanceKind::Shaped),
        ("both", TextAdvanceKind::Metric),
    ] {
        let body = format!(
            r#"<w:p><w:pPr><w:jc w:val="{alignment}"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Carlito" w:hAnsi="Carlito"/><w:sz w:val="64"/></w:rPr><w:t>To</w:t></w:r></w:p>"#
        );
        let (_, document) = common::open_body(&body);
        let pages = place_pages(&document, &RenderOptions::default(), None).unwrap();
        let text = pages
            .iter()
            .flat_map(|p| &p.items)
            .find_map(|i| match i {
                Item::Text(t) if t.text == "To" => Some(t),
                _ => None,
            })
            .unwrap();
        assert_eq!(text.advance, expected_kind, "alignment={alignment}");
        let expected = match text.advance {
            TextAdvanceKind::Shaped => {
                let shaped = shape_bundled(&text.text, &text.run.family, false, false).unwrap();
                shaped.total_advance_em * text.size_px
            }
            TextAdvanceKind::Metric => {
                let provider = BuiltinFontProvider::new();
                let family = chosen_family(&text.run, &text.text);
                text.text
                    .chars()
                    .map(|ch| provider.advance_em(&family, ch, false, false))
                    .sum::<f64>()
                    * text.size_px
            }
        };
        println!(
            "alignment={alignment}, kind={:?}, layout={}, expected={}, delta={}",
            text.advance,
            text.width,
            expected,
            (text.width - expected).abs()
        );
        assert!(
            (text.width - expected).abs() <= 0.25,
            "layout disagrees with declared advance for {alignment}"
        );
    }
}
