//! Deterministic vector shapes for stretchy math constructs
//! (`STAGE-5C-TASK.md` §9.1, §12).
//!
//! Delimiters and the radical sign have to stretch to an arbitrary height, and
//! a real font would need one glyph per size. Drawing them as paths keeps the
//! output byte-stable, avoids a hard dependency on a particular face and gives
//! the same structure even for a character the bundled font does not cover.
//!
//! Every path is emitted in the *local* coordinate system of a `w × h` box
//! (`0..w`, `0..h`, `+y` downwards), the contract
//! [`PathItem`](crate::layout::PathItem) uses elsewhere in the renderer.

use std::fmt::Write as _;

/// Builds an SVG path from a list of `(command, points)` in a `w × h` box.
///
/// Points are given in the unit square as alternating `x`/`y` pairs and scaled
/// into the local box, so one path definition serves any delimiter size.
fn path(segments: &[(&str, &[f64])], w: f64, h: f64) -> String {
    let mut out = String::with_capacity(64);
    for (command, points) in segments {
        let _ = out.write_str(command);
        for (index, point) in points.iter().enumerate() {
            let scale = if index % 2 == 0 { w } else { h };
            let _ = write!(
                out,
                " {}",
                crate::units::fmt_num(crate::units::finite(point * scale))
            );
        }
        out.push(' ');
    }
    out.trim_end().to_owned()
}

/// The radical sign in a local `width × height` box whose top edge is the
/// overline.
///
/// The overline occupies the right part of the top edge; the hook rises from
/// the bottom-left corner into the notch, as in a printed radical.
#[must_use]
pub(crate) fn radical_sign(width: f64, height: f64) -> String {
    path(
        &[
            ("M", &[0.0, 1.0, 0.22, 0.42]),
            ("L", &[0.42, 0.0, 1.0, 0.0]),
        ],
        width,
        height,
    )
}

/// The path of a stretchy delimiter in a local `width × height` box, or `None`
/// when the character has no geometric form (it is then drawn as a glyph).
///
/// A stroke-only path: the delimiters are drawn, not filled.
#[must_use]
pub(crate) fn delimiter(character: char, width: f64, height: f64) -> Option<String> {
    let segments: &[(&str, &[f64])] = match character {
        '(' => &[("M", &[1.0, 0.0, 0.15, 0.5]), ("L", &[1.0, 1.0, 0.15, 0.5])],
        ')' => &[("M", &[0.0, 0.0, 0.85, 0.5]), ("L", &[0.0, 1.0, 0.85, 0.5])],
        '[' => &[
            ("M", &[0.9, 0.0, 0.1, 0.0]),
            ("L", &[0.1, 0.0, 0.1, 1.0]),
            ("L", &[0.1, 1.0, 0.9, 1.0]),
        ],
        ']' => &[
            ("M", &[0.1, 0.0, 0.9, 0.0]),
            ("L", &[0.9, 0.0, 0.9, 1.0]),
            ("L", &[0.9, 1.0, 0.1, 1.0]),
        ],
        '{' => &[
            ("M", &[1.0, 0.0, 0.7, 0.0]),
            ("C", &[0.55, 0.02, 0.75, 0.4, 0.2, 0.5]),
            ("C", &[0.75, 0.6, 0.55, 0.98, 1.0, 1.0]),
        ],
        '}' => &[
            ("M", &[0.0, 0.0, 0.3, 0.0]),
            ("C", &[0.45, 0.02, 0.25, 0.4, 0.8, 0.5]),
            ("C", &[0.25, 0.6, 0.45, 0.98, 0.0, 1.0]),
        ],
        '\u{27E8}' => &[("M", &[1.0, 0.0, 0.0, 0.5]), ("L", &[1.0, 1.0, 0.0, 0.5])],
        '\u{27E9}' => &[("M", &[0.0, 0.0, 1.0, 0.5]), ("L", &[0.0, 1.0, 1.0, 0.5])],
        '\u{2308}' => &[("M", &[1.0, 0.0, 0.2, 0.0]), ("L", &[0.2, 0.0, 0.2, 1.0])],
        '\u{2309}' => &[("M", &[0.0, 0.0, 0.8, 0.0]), ("L", &[0.8, 0.0, 0.8, 1.0])],
        '\u{230A}' => &[("M", &[1.0, 1.0, 0.2, 1.0]), ("L", &[0.2, 1.0, 0.2, 0.0])],
        '\u{230B}' => &[("M", &[0.0, 1.0, 0.8, 1.0]), ("L", &[0.8, 1.0, 0.8, 0.0])],
        '\u{2016}' | '\u{2225}' => &[
            ("M", &[0.35, 0.0, 0.35, 1.0]),
            ("L", &[0.65, 0.0, 0.65, 1.0]),
        ],
        // Slash variants grow to fit.
        '\u{2A66}' | '\u{2A67}' | '\u{2A68}' => &[("M", &[0.0, 1.0, 1.0, 0.0])],
        // Mathematical and contour variants of the integral sign.
        '\u{27CC}' | '\u{27CD}' | '\u{27CE}' | '\u{27CF}' => {
            &[("M", &[1.0, 0.0, 0.0, 0.5]), ("L", &[1.0, 1.0, 0.0, 0.5])]
        }
        // Double and triple vertical bars.
        '\u{2AE3}' | '\u{2AE4}' => &[("M", &[0.3, 0.0, 0.3, 1.0]), ("L", &[0.6, 0.0, 0.6, 1.0])],
        _ => return None,
    };
    Some(path(segments, width, height))
}

#[cfg(test)]
mod tests {
    use super::{delimiter, radical_sign};

    /// The numeric tokens of a path (the command letters are skipped).
    fn coordinates(path: &str) -> Vec<f64> {
        path.split_whitespace()
            .filter_map(|token| token.parse::<f64>().ok())
            .collect()
    }

    #[test]
    fn every_geometric_delimiter_has_a_path() {
        for ch in [
            '(', ')', '[', ']', '{', '}', '\u{27E8}', '\u{27E9}', '\u{2308}', '\u{2309}',
            '\u{230A}', '\u{230B}', '\u{2016}',
        ] {
            let d = delimiter(ch, 4.0, 20.0).unwrap_or_else(|| panic!("no path for {ch:?}"));
            assert!(d.starts_with('M'), "path must start with a move: {d}");
            assert!(d.contains('L') || d.contains('C'), "path must draw: {d}");
            assert!(!coordinates(&d).is_empty(), "path must have points: {d}");
        }
    }

    #[test]
    fn characters_without_a_geometric_form_have_no_path() {
        assert!(delimiter('a', 4.0, 20.0).is_none());
        assert!(delimiter('\u{222B}', 4.0, 20.0).is_none());
    }

    #[test]
    fn the_radical_sign_reaches_the_top_edge() {
        let d = radical_sign(4.0, 20.0);
        assert_eq!(d, "M 0 20 0.88 8.4 L 1.68 0 4 0");
    }

    #[test]
    fn coordinates_are_scaled_into_the_local_box() {
        for ch in ['(', ')', '{', '}'] {
            let d = delimiter(ch, 4.0, 20.0).expect("path");
            for value in coordinates(&d) {
                assert!((0.0..=20.0).contains(&value), "{value} out of range in {d}");
            }
        }
        // A wider box widens the horizontal coordinates.
        assert_eq!(
            delimiter('(', 2.0, 20.0).expect("path"),
            "M 2 0 0.3 10 L 2 20 0.3 10"
        );
        assert_eq!(
            delimiter('(', 4.0, 20.0).expect("path"),
            "M 4 0 0.6 10 L 4 20 0.6 10"
        );
    }

    #[test]
    fn a_degenerate_box_still_produces_finite_coordinates() {
        let d = delimiter('(', 0.0, 0.0).expect("path");
        for value in coordinates(&d) {
            assert!(value.is_finite() && value == 0.0, "{value} in {d}");
        }
    }
}
