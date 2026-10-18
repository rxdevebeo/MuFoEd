//! The widths of a CID font, in both shapes `/W` takes (ISO 32000-1 9.7.4.3).
//!
//! `c [w1 w2 ...]` is the shape Word writes for its Type0 fonts; reading it as
//! `c_first c_last w` lost every width, every glyph took `/DW` - a full em -
//! and the words of a justified line ran into each other.

#![allow(missing_docs)]

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::PdfBuilder;

/// The width of each glyph of `<0041 0042 0043>` at 10 pt, with `w` as `/W`.
fn widths(w: &str) -> Vec<f64> {
    let mut pdf = PdfBuilder::new();
    let cmap = pdf.stream(
        "/CMapName /A",
        concat!(
            "/CIDInit /ProcSet findresource begin 12 dict begin begincmap ",
            "1 begincodespacerange <0000> <FFFF> endcodespacerange ",
            "3 beginbfchar <0041> <0041> <0042> <0042> <0043> <0043> endbfchar ",
            "endcmap end end"
        )
        .as_bytes(),
        false,
    );
    let widths = pdf.object(w.to_owned());
    let descendant = pdf.object(format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Test \
         /CIDSystemInfo << /Registry (Test) /Ordering (Test) /Supplement 0 >> \
         /DW 1000 /W {widths} 0 R >>"
    ));
    let font = pdf.object(format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Test /Encoding /Identity-H \
         /DescendantFonts [{descendant} 0 R] /ToUnicode {cmap} 0 R >>"
    ));
    pdf.page_with(
        b"BT /F1 10 Tf 1 0 0 1 10 50 Tm <004100420043> Tj ET",
        &format!("<< /Font << /F1 {font} 0 R >> >>"),
    );
    let mut document = PdfDocument::open(&pdf.build(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    page.items()
        .iter()
        .filter_map(|item| match item {
            Item::Glyph(glyph) => Some((glyph.width * 100.0).round() / 100.0),
            _ => None,
        })
        .collect()
}

#[test]
fn a_run_of_widths_after_one_code_is_read() {
    // Word's shape, behind an indirect reference as Word writes it.
    assert_eq!(
        widths("[0 [597] 65 [480 520] 67 [600]]"),
        vec![4.8, 5.2, 6.0]
    );
}

#[test]
fn a_range_of_one_width_is_read() {
    assert_eq!(widths("[65 67 500]"), vec![5.0, 5.0, 5.0]);
}

#[test]
fn both_shapes_mix() {
    assert_eq!(widths("[65 [480] 66 67 700]"), vec![4.8, 7.0, 7.0]);
}
