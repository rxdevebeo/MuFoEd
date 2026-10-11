//! `<text>` emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::coord;
use crate::font::{shape_bundled_run, unicode_x_positions_px, BuiltinFontProvider, FontProvider};
use crate::layout::{TextAdvanceKind, TextItem};

/// Outline edge using the same scalar positions and face as SVG paint.
pub(crate) fn text_ink_right(item: &TextItem) -> Option<f64> {
    thread_local! {
        static FONTS: BuiltinFontProvider = BuiltinFontProvider::new();
    }
    let shown = crate::font::present_text(&item.run.family, &item.text);
    let family = crate::style::chosen_family(&item.run, &item.text);
    let positions = shaped_x_attribute(&shown, &family, item);
    FONTS.with(|fonts| {
        shown
            .chars()
            .zip(positions.split_whitespace())
            .filter_map(|(ch, x)| {
                let x = x.parse::<f64>().ok()?;
                let right = fonts.ink_right_em(&family, ch, item.run.bold, item.run.italic)?;
                Some(item.x + (x - item.x + right * item.size_px) * item.run.char_scale)
            })
            .max_by(f64::total_cmp)
    })
}

/// Writes one text fragment with its shading, highlight, underline and
/// strikethrough.
pub(crate) fn text_svg(out: &mut String, item: &TextItem) {
    if item.run.rotation_deg != 0.0 {
        let _ = writeln!(
            out,
            "<g transform=\"rotate({} {} {})\">",
            coord(item.run.rotation_deg),
            coord(item.x),
            coord(item.baseline)
        );
    }
    // Decorations are geometry shared with the PDF backend, rotated with the
    // text they belong to.
    let decorations = crate::layout::text_decorations(item);
    for decoration in &decorations.behind {
        super::item_svg(out, decoration);
    }
    text_svg_unrotated(out, item);
    for decoration in &decorations.over {
        super::item_svg(out, decoration);
    }
    if item.run.rotation_deg != 0.0 {
        out.push_str("</g>\n");
    }
}

fn text_svg_unrotated(out: &mut String, item: &TextItem) {
    if item.run.char_scale != 1.0 {
        let _ = writeln!(
            out,
            "<g transform=\"translate({} 0) scale({} 1) translate({} 0)\">",
            coord(item.x),
            coord(item.run.char_scale),
            coord(-item.x)
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
    if item.run.outline {
        let _ = write!(
            attributes,
            " stroke=\"{}\" stroke-width=\"{}\"",
            super::escape_attr(&fill),
            coord((item.size_px * 0.035).max(0.5))
        );
    }
    // Explicit per-scalar x from the shared shaper so the browser does not
    // re-shape against a different face (F07). Ligature clusters share one x.
    let x_attr = shaped_x_attribute(&shown, &family, item);
    let _ = writeln!(
        out,
        "  <text x=\"{x_attr}\" y=\"{}\" {} fill=\"{}\" xml:space=\"preserve\">{}</text>",
        coord(item.baseline),
        attributes,
        if item.run.outline {
            "none".to_owned()
        } else {
            super::escape_attr(&fill)
        },
        super::escape_text(&shown),
    );
    if item.run.char_scale != 1.0 {
        out.push_str("</g>\n");
    }
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
                cursor += crate::style::space_advance_em(
                    &item.run,
                    family,
                    ch,
                    provider.advance_em(family, ch, item.run.bold, item.run.italic),
                ) * item.size_px
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
            let Some(shaped) = shape_bundled_run(shown, family, &item.run) else {
                return coord(item.x);
            };
            let mut xs = unicode_x_positions_px(shown, &shaped, item.x, item.size_px);
            let offsets = crate::font::shaped_cluster_offsets_px(item, shown, &shaped);
            let mut cluster_index = 0;
            for (scalar, (byte, _)) in shown.char_indices().enumerate() {
                while shaped
                    .clusters
                    .get(cluster_index)
                    .is_some_and(|cluster| cluster.byte_end <= byte)
                {
                    cluster_index += 1;
                }
                if let (Some(x), Some(offset)) = (xs.get_mut(scalar), offsets.get(cluster_index)) {
                    *x += offset;
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
