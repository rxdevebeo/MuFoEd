//! Rectangle and line emission (`STAGE-4-TASK.md` §5.8).

use std::fmt::Write as _;

use super::{coord, escape_attr};
use crate::layout::{LineItem, PathItem, PathPaint, RectItem};
use sha2::{Digest, Sha256};

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
    if let Some(paint) = &path.paint {
        let mut id = String::from("fill-");
        for byte in Sha256::digest(format!("{paint:?}").as_bytes()) {
            let _ = write!(id, "{byte:02x}");
        }
        paint_definition(out, &id, paint);
        let _ = write!(attributes, " fill=\"url(#{id})\"");
    } else {
        match &path.fill {
            Some(fill) => {
                let _ = write!(attributes, " fill=\"{}\"", escape_attr(fill));
            }
            None => attributes.push_str(" fill=\"none\""),
        }
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

fn paint_definition(out: &mut String, id: &str, paint: &PathPaint) {
    out.push_str("  <defs>\n");
    match paint {
        PathPaint::LinearGradient { stops, start, end } => {
            let _ = writeln!(out, "    <linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">", coord(start.0), coord(start.1), coord(end.0), coord(end.1));
            for (position, color) in stops {
                let _ = writeln!(
                    out,
                    "      <stop offset=\"{}\" stop-color=\"{}\"/>",
                    coord(*position),
                    escape_attr(color)
                );
            }
            out.push_str("    </linearGradient>\n");
        }
        PathPaint::Pattern {
            foreground,
            background,
            size,
            lines,
            rectangles,
        } => {
            let _ = writeln!(out, "    <pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{}\" height=\"{}\" viewBox=\"0 0 1 1\">", coord(*size), coord(*size));
            let _ = writeln!(
                out,
                "      <rect width=\"1\" height=\"1\" fill=\"{}\"/>",
                escape_attr(background)
            );
            for (x1, y1, x2, y2) in lines {
                let _ = writeln!(out, "      <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.125\"/>", coord(*x1), coord(*y1), coord(*x2), coord(*y2), escape_attr(foreground));
            }
            for (x, y, w, h) in rectangles {
                let _ = writeln!(
                    out,
                    "      <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    coord(*x),
                    coord(*y),
                    coord(*w),
                    coord(*h),
                    escape_attr(foreground)
                );
            }
            out.push_str("    </pattern>\n");
        }
    }
    out.push_str("  </defs>\n");
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
