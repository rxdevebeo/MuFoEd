//! Rectangle and line emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::{coord, escape_attr};
use crate::layout::{LineItem, PathItem, RectItem};

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

/// Writes an arbitrary shape path.
pub(crate) fn path_svg(out: &mut String, path: &PathItem) {
    let mut attributes = String::new();
    match &path.fill {
        Some(fill) => {
            let _ = write!(attributes, " fill=\"{}\"", escape_attr(fill));
        }
        None => attributes.push_str(" fill=\"none\""),
    }
    if let Some(stroke) = &path.stroke {
        let _ = write!(
            attributes,
            " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"",
            escape_attr(stroke),
            coord(path.stroke_w.max(0.0))
        );
        if let Some(dash) = &path.dash {
            let _ = write!(attributes, " stroke-dasharray=\"{}\"", escape_attr(dash));
        }
    }
    let transform = path_transform(path);
    let _ = write!(attributes, " transform=\"{}\"", escape_attr(&transform));
    let _ = writeln!(
        out,
        "  <path d=\"{}\"{}/>",
        escape_attr(&path.d),
        attributes,
    );
}

/// Builds the SVG transform (translation + rotation/flips) for a path.
fn path_transform(path: &PathItem) -> String {
    let mut parts = vec![format!(
        "translate({} {})",
        crate::units::fmt_num(path.x),
        crate::units::fmt_num(path.y)
    )];
    let cx = crate::units::fmt_num(path.w / 2.0);
    let cy = crate::units::fmt_num(path.h / 2.0);
    if path.rotate_deg.abs() > f64::EPSILON {
        parts.push(format!(
            "rotate({} {cx} {cy})",
            crate::units::fmt_num(path.rotate_deg)
        ));
    }
    if path.flip_h || path.flip_v {
        let sx = if path.flip_h { -1.0 } else { 1.0 };
        let sy = if path.flip_v { -1.0 } else { 1.0 };
        parts.push(format!(
            "translate({cx} {cy}) scale({} {}) translate(-{cx} -{cy})",
            crate::units::fmt_num(sx),
            crate::units::fmt_num(sy)
        ));
    }
    parts.join(" ")
}
