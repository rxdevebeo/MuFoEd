//! Rectangle and line emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::{coord, escape_attr};
use crate::layout::{LineItem, RectItem};

/// Writes a rectangle.
pub(crate) fn rect_svg(out: &mut String, rect: &RectItem) {
    if rect.fill.is_none() && rect.stroke.is_none() {
        return;
    }
    let mut attributes = String::new();
    if let Some(fill) = &rect.fill {
        let _ = write!(attributes, " fill=\"{}\"", escape_attr(fill));
    } else {
        attributes.push_str(" fill=\"none\"");
    }
    if let Some(stroke) = &rect.stroke {
        let _ = write!(
            attributes,
            " stroke=\"{}\" stroke-width=\"{}\"",
            escape_attr(stroke),
            coord(rect.stroke_w.max(0.0))
        );
    }
    let _ = writeln!(
        out,
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
        coord(rect.x),
        coord(rect.y),
        coord(rect.w.max(0.0)),
        coord(rect.h.max(0.0)),
        attributes,
    );
}

/// Writes a line.
pub(crate) fn line_svg(out: &mut String, line: &LineItem) {
    let dash = if line.dashed {
        " stroke-dasharray=\"4 3\""
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}/>",
        coord(line.x1),
        coord(line.y1),
        coord(line.x2),
        coord(line.y2),
        escape_attr(&line.color),
        coord(line.width.max(0.0)),
        dash,
    );
}
