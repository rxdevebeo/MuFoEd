//! Deterministic SVG writer (`STAGE-4-TASK.md` §5.8).
//!
//! Every number goes through [`crate::units::fmt_num`]; attributes are emitted
//! in a fixed order; text/attribute values are XML-escaped. The output is a
//! single `<svg>` document per page, terminated by `\n`.

pub(crate) mod graphics;
pub(crate) mod image;
pub(crate) mod shapes;
pub(crate) mod text;

use std::fmt::Write as _;

use crate::layout::{Item, PlacedPage};
use crate::units::fmt_num;

/// Renders one placed page to an SVG document string.
#[must_use]
pub(crate) fn render_page(page: &PlacedPage, background: bool) -> String {
    let mut out = String::with_capacity(1024);
    let width = fmt_num(page.width_px);
    let height = fmt_num(page.height_px);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
    );
    if background {
        let _ = writeln!(
            out,
            "  <rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"#ffffff\"/>"
        );
    }
    for item in &page.items {
        match item {
            Item::Rect(rect) => shapes::rect_svg(&mut out, rect),
            Item::Line(line) => shapes::line_svg(&mut out, line),
            Item::Text(item) => text::text_svg(&mut out, item),
            Item::Image(item) => image::image_svg(&mut out, item),
            Item::Path(item) => shapes::path_svg(&mut out, item),
        }
    }
    out.push_str("</svg>\n");
    out
}

pub(crate) use strict_ooxml_core::xml::escape::{escape_attr, escape_text};

/// One warning for `page` if any of its text would lose characters XML cannot
/// carry. The painter removes them while escaping; this is where the removal
/// becomes visible to the caller.
pub(crate) fn invalid_char_warning(page: &PlacedPage) -> Option<String> {
    use strict_ooxml_core::xml::escape::count_invalid;
    let removed: usize = page
        .items
        .iter()
        .map(|item| match item {
            Item::Text(text) => count_invalid(&text.text),
            Item::Image(image) => count_invalid(&image.alt),
            Item::Rect(_) | Item::Line(_) | Item::Path(_) => 0,
        })
        .sum();
    (removed > 0).then(|| {
        format!(
            "render.invalid-xml-char: {removed} character(s) that XML 1.0 cannot carry were \
             removed from the page's text"
        )
    })
}

/// Formats an SVG coordinate.
#[must_use]
pub(crate) fn coord(value: f64) -> String {
    fmt_num(value)
}

#[cfg(test)]
mod tests {
    use super::{escape_attr, escape_text, render_page};
    use crate::layout::{Item, PlacedPage, RectItem};

    #[test]
    fn escaping_is_correct() {
        assert_eq!(escape_text("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(escape_attr("\"q\" 'x'"), "&quot;q&quot; &apos;x&apos;");
        assert_eq!(escape_text("a\u{1}b"), "ab");
    }

    #[test]
    fn page_svg_has_header_viewbox_and_newline() {
        let page = PlacedPage {
            width_px: 816.0,
            height_px: 1056.0,
            items: vec![Item::Rect(RectItem {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                fill: Some("#ff0000".to_owned()),
                stroke: None,
                stroke_w: 0.0,
            })],
        };
        let svg = render_page(&page, true);
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("viewBox=\"0 0 816 1056\""));
        assert!(svg.ends_with("</svg>\n"));
    }
}
