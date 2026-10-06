//! `<text>` emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::coord;
use crate::font::{shape_bundled, unicode_x_positions_px, BuiltinFontProvider, FontProvider};
use crate::layout::{TextAdvanceKind, TextItem};

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
    let family = crate::style::chosen_family(&item.run, &item.text);
    let fill = item
        .run
        .color
        .clone()
        .unwrap_or_else(|| "#000000".to_owned());
    let mut attributes = String::new();
    attributes.push_str("font-family=\"");
    attributes.push_str(&super::escape_attr(&family));
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
    // Explicit per-scalar x from the shared shaper so the browser does not
    // re-shape against a different face (F07). Ligature clusters share one x.
    let x_attr = shaped_x_attribute(&shown, &family, item);
    let _ = writeln!(
        out,
        "  <text x=\"{x_attr}\" y=\"{}\" {} fill=\"{}\" xml:space=\"preserve\">{}</text>",
        coord(item.baseline),
        attributes,
        super::escape_attr(&fill),
        super::escape_text(&shown),
    );
}

fn shaped_x_attribute(shown: &str, family: &str, item: &TextItem) -> String {
    if shown.is_empty() || shown.chars().count() == 1 {
        return coord(item.x);
    }
    let xs = match item.advance {
        TextAdvanceKind::Metric => {
            let provider = BuiltinFontProvider::new();
            let extra = crate::style::spacing_px(&item.run, item.size_px);
            let mut cursor = item.x;
            let mut prev = None;
            let chars: Vec<char> = shown.chars().collect();
            let mut metric = Vec::with_capacity(chars.len());
            for (i, &ch) in chars.iter().enumerate() {
                metric.push(cursor);
                let next = chars.get(i + 1).copied().or(item.following_non_space);
                let factor = crate::style::compressed_char_factor(
                    prev,
                    ch,
                    next,
                    item.compress_punctuation,
                    item.plain_space_factor,
                );
                cursor += provider.advance_em(family, ch, item.run.bold, item.run.italic)
                    * item.size_px
                    * factor;
                if !ch.is_whitespace() {
                    cursor += extra;
                    prev = Some(ch);
                } else if factor > 0.0 {
                    prev = Some(ch);
                }
            }
            metric
        }
        TextAdvanceKind::Shaped => {
            let Some(shaped) = shape_bundled(shown, family, item.run.bold, item.run.italic) else {
                return coord(item.x);
            };
            let mut xs = unicode_x_positions_px(shown, &shaped, item.x, item.size_px);
            let extra = crate::style::spacing_px(&item.run, item.size_px);
            let compress = item.compress_punctuation;
            if extra != 0.0 || compress {
                let chars: Vec<char> = shown.chars().collect();
                let mut prev = None;
                let mut non_space_index = 0usize;
                let mut compress_shift = 0.0;
                let provider = BuiltinFontProvider::new();
                for (logical, &ch) in chars.iter().enumerate() {
                    let next = chars.get(logical + 1).copied().or(item.following_non_space);
                    let factor = crate::style::compressed_char_factor(
                        prev,
                        ch,
                        next,
                        compress,
                        item.plain_space_factor,
                    );
                    if let Some(x) = xs.get_mut(logical) {
                        *x += extra * non_space_index as f64 + compress_shift;
                    }
                    if (factor - 1.0).abs() > f64::EPSILON {
                        compress_shift -= provider.advance_em(
                            family,
                            ch,
                            item.run.bold,
                            item.run.italic,
                        ) * item.size_px
                            * (1.0 - factor);
                    }
                    if !ch.is_whitespace() {
                        non_space_index += 1;
                        prev = Some(ch);
                    } else if factor > 0.0 {
                        prev = Some(ch);
                    }
                }
            }
            xs
        }
    };
    if xs.is_empty() {
        return coord(item.x);
    }
    let mut out = String::new();
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&coord(*x));
    }
    out
}
