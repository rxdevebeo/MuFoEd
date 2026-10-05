//! `<text>` emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::coord;
use crate::layout::TextItem;

/// Writes one text fragment, optionally with a highlight rectangle behind it.
pub(crate) fn text_svg(out: &mut String, item: &TextItem) {
    if let Some(highlight) = &item.run.highlight {
        // Highlight: an approximate box around the glyphs.
        let height = item.size_px * 1.2;
        let top = item.baseline - item.size_px * 0.9;
        let _ = writeln!(
            out,
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            coord(item.x),
            coord(top),
            coord(item.width),
            coord(height),
            super::escape_attr(highlight),
        );
    }
    let shown = crate::font::present_text(&item.run.family, &item.text);
    let fill = item
        .run
        .color
        .clone()
        .unwrap_or_else(|| "#000000".to_owned());
    let mut attributes = String::new();
    attributes.push_str("font-family=\"");
    attributes.push_str(&super::escape_attr(&crate::style::chosen_family(
        &item.run, &item.text,
    )));
    attributes.push('"');
    let _ = write!(attributes, " font-size=\"{}\"", coord(item.size_px));
    if item.run.bold {
        attributes.push_str(" font-weight=\"bold\"");
    }
    if item.run.italic {
        attributes.push_str(" font-style=\"italic\"");
    }
    let decoration = match (item.run.underline, item.run.strike) {
        (true, true) => Some("underline line-through"),
        (true, false) => Some("underline"),
        (false, true) => Some("line-through"),
        (false, false) => None,
    };
    if let Some(decoration) = decoration {
        let _ = write!(attributes, " text-decoration=\"{decoration}\"");
    }
    let _ = writeln!(
        out,
        "  <text x=\"{}\" y=\"{}\" {} fill=\"{}\" xml:space=\"preserve\">{}</text>",
        coord(item.x),
        coord(item.baseline),
        attributes,
        super::escape_attr(&fill),
        super::escape_text(&shown),
    );
}
