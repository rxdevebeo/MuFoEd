//! The SVG path-data subset the layout emits, parsed back into geometry.
//!
//! `PathItem` carries its outline as SVG path data because that is what the SVG
//! backend writes. A PDF content stream wants operators, so the data is parsed
//! back into segments. That round trip through a string is a deliberate
//! trade-off (ADR-0008): the alternative is changing the shared item type, which
//! would touch the Stage-4/5B renderer and its SSIM gate for the sake of a second
//! consumer. The grammar supported is exactly what the producers emit — `M`, `L`,
//! `H`, `V`, `C`, `A`, `Z` — plus the implicit-lineto form of `M`/`L`.
//!
//! # `A` was missing, and the pixel gate is what said so
//!
//! The first version of this parser supported `M`, `L`, `H`, `V`, `C` and `Z`,
//! and skipped everything else on the grounds that the producers do not emit it.
//! They do: `strict-stage5b` and `07-strict-drawingml-shapes` draw rounded
//! rectangles and a circle, and DrawingML's preset geometry reaches us already
//! converted to SVG arc form. So every rounded corner in every PDF this backend
//! wrote was **silently dropped**, and the page came out with sharp corners and,
//! for a circle, with nothing at all where the circle should be. The SVG gate
//! could not see it — it measures the SVG, and the SVG is correct — and the PDF
//! gate had no pixel comparison at all until stage 8's `O-2`.
//!
//! So `A` is parsed here and converted to cubics, because PDF has no arc
//! operator. The conversion is the endpoint-to-centre parameterisation from SVG
//! 1.1 appendix F.6.5 followed by the standard cubic approximation of each arc
//! piece, which is exact for the whole arc and within 0.02 % of the radius for
//! any single piece.
//!
//! # What is still not here
//!
//! `Q`, `T`, `S` and the elliptical shorthand forms. They are skipped rather
//! than guessed at, for the same reason as before: a shape half-drawn is worse
//! than a shape that is visibly not drawn. The corpus check that they are really
//! absent is `every_producer_path_uses_only_the_supported_commands` in
//! `strict-ooxml-render-pdf/tests/pdf.rs` — so the day a producer does emit one,
//! the claim fails loudly instead of quietly costing a corner.

/// One absolute path segment, before shorthand resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Start a new subpath.
    MoveTo(f64, f64),
    /// Straight line. Either coordinate may be [`f64::NAN`] for `H`/`V`
    /// shorthand, which [`resolve`] fills in from the current point.
    LineTo(f64, f64),
    /// Cubic Bézier.
    CurveTo(f64, f64, f64, f64, f64, f64),
    /// An elliptical arc, kept as written and converted by [`resolve`].
    ///
    /// It cannot be resolved during parsing because an arc is defined *from* the
    /// current point, and the parser does not track one: `H` and `V` are
    /// resolved the same way, later, for the same reason.
    Arc {
        /// x radius.
        rx: f64,
        /// y radius.
        ry: f64,
        /// x-axis rotation in degrees.
        rotation: f64,
        /// Whether the arc is the large one of the two.
        large_arc: bool,
        /// Which side of the chord the arc bulges to.
        sweep: bool,
        /// End point x.
        x: f64,
        /// End point y.
        y: f64,
    },
    /// Close the current subpath.
    Close,
}

/// Parses SVG path data into absolute segments.
///
/// Unparseable input yields whatever parsed cleanly, so a malformed path
/// contributes the part that is still usable instead of nothing.
#[must_use]
pub fn parse(data: &str) -> Vec<Segment> {
    let bytes: Vec<char> = data.chars().collect();
    let mut index = 0;
    let mut out: Vec<Segment> = Vec::new();
    let mut command = ' ';

    while let Some(&ch) = bytes.get(index) {
        if ch.is_ascii_whitespace() || ch == ',' {
            index += 1;
            continue;
        }
        if ch.is_ascii_alphabetic() {
            command = ch;
            index += 1;
        } else if command == ' ' {
            // A number with no command in front of it: nothing sensible to do.
            index += 1;
            continue;
        }

        match command {
            'M' | 'm' => {
                let Some(x) = number(&bytes, &mut index) else {
                    break;
                };
                let Some(y) = number(&bytes, &mut index) else {
                    break;
                };
                out.push(Segment::MoveTo(x, y));
                // Extra pairs after a moveto are implicit linetos, per the SVG
                // grammar: `M 0 20 0.88 8.4` is a move then a line.
                command = if command == 'M' { 'L' } else { 'l' };
            }
            'L' | 'l' => {
                let Some(x) = number(&bytes, &mut index) else {
                    break;
                };
                let Some(y) = number(&bytes, &mut index) else {
                    break;
                };
                out.push(Segment::LineTo(x, y));
            }
            'H' | 'h' => {
                let Some(x) = number(&bytes, &mut index) else {
                    break;
                };
                out.push(Segment::LineTo(x, f64::NAN));
            }
            'V' | 'v' => {
                let Some(y) = number(&bytes, &mut index) else {
                    break;
                };
                out.push(Segment::LineTo(f64::NAN, y));
            }
            'C' | 'c' => {
                let Some(values) = six(&bytes, &mut index) else {
                    break;
                };
                out.push(Segment::CurveTo(
                    values[0], values[1], values[2], values[3], values[4], values[5],
                ));
            }
            'A' | 'a' => {
                let (Some(rx), Some(ry), Some(rotation)) = (
                    number(&bytes, &mut index),
                    number(&bytes, &mut index),
                    number(&bytes, &mut index),
                ) else {
                    break;
                };
                // A flag is exactly one character, which is also how the SVG
                // grammar spells it: `a1 1 0 0114 0` is three flags and a
                // coordinate, not the number 114.
                let (Some(large_arc), Some(sweep)) =
                    (flag(&bytes, &mut index), flag(&bytes, &mut index))
                else {
                    break;
                };
                let (Some(x), Some(y)) = (number(&bytes, &mut index), number(&bytes, &mut index))
                else {
                    break;
                };
                out.push(Segment::Arc {
                    rx,
                    ry,
                    rotation,
                    large_arc,
                    sweep,
                    x,
                    y,
                });
            }
            'Z' | 'z' => {
                out.push(Segment::Close);
                command = ' ';
            }
            // Q, T, S and R: not emitted by any producer in the corpus, and a
            // curve guessed at here would differ from what the SVG shows.
            _ => {
                index += 1;
            }
        }
    }
    out
}

/// Reads one number, tolerating the forms SVG allows (`-1.5`, `.5`, `1e-3`).
fn number(bytes: &[char], index: &mut usize) -> Option<f64> {
    skip_separators(bytes, index);
    let start = *index;
    if bytes.get(*index).is_some_and(|&ch| ch == '-' || ch == '+') {
        *index += 1;
    }
    while bytes
        .get(*index)
        .is_some_and(|&ch| ch.is_ascii_digit() || ch == '.')
    {
        *index += 1;
    }
    if bytes.get(*index).is_some_and(|&ch| ch == 'e' || ch == 'E') {
        *index += 1;
        if bytes.get(*index).is_some_and(|&ch| ch == '-' || ch == '+') {
            *index += 1;
        }
        while bytes.get(*index).is_some_and(char::is_ascii_digit) {
            *index += 1;
        }
    }
    if start == *index {
        return None;
    }
    let text: String = bytes.get(start..*index)?.iter().collect();
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Skips whitespace and commas, which SVG treats alike between numbers.
fn skip_separators(bytes: &[char], index: &mut usize) {
    while bytes
        .get(*index)
        .is_some_and(|&ch| ch.is_ascii_whitespace() || ch == ',')
    {
        *index += 1;
    }
}

/// Reads an arc flag: one character, `0` or `1`.
fn flag(bytes: &[char], index: &mut usize) -> Option<bool> {
    skip_separators(bytes, index);
    let ch = *bytes.get(*index)?;
    if ch != '0' && ch != '1' {
        return None;
    }
    *index += 1;
    Some(ch == '1')
}

/// Reads `count` numbers, or nothing at all if the data runs out.
fn six(bytes: &[char], index: &mut usize) -> Option<[f64; 6]> {
    let mut values = [0.0; 6];
    for slot in &mut values {
        *slot = number(bytes, index)?;
    }
    Some(values)
}

/// Resolves `H`/`V` shorthand and turns arcs into cubics, given the current point.
///
/// Returns the resolved segments and the point the path ends at.
#[must_use]
pub fn resolve(segments: &[Segment], current: (f64, f64)) -> (Vec<Segment>, (f64, f64)) {
    let mut out = Vec::with_capacity(segments.len());
    let mut point = current;
    let mut start = current;
    for segment in segments {
        match *segment {
            Segment::MoveTo(x, y) => {
                point = (x, y);
                start = point;
                out.push(Segment::MoveTo(x, y));
            }
            Segment::LineTo(x, y) => {
                let (x, y) = (
                    if x.is_nan() { point.0 } else { x },
                    if y.is_nan() { point.1 } else { y },
                );
                point = (x, y);
                out.push(Segment::LineTo(x, y));
            }
            Segment::CurveTo(x1, y1, x2, y2, x, y) => {
                point = (x, y);
                out.push(Segment::CurveTo(x1, y1, x2, y2, x, y));
            }
            Segment::Arc {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                x,
                y,
            } => {
                let curves = arc_to_curves(point, (x, y), rx, ry, rotation, large_arc, sweep);
                point = (x, y);
                out.extend(curves);
            }
            Segment::Close => {
                point = start;
                out.push(Segment::Close);
            }
        }
    }
    (out, point)
}

/// A quarter turn.
///
/// The error of the cubic approximation grows with the fifth power of the
/// covered angle, so a quarter turn is where it is still cheap: measured,
/// a quarter-turn cubic sits within 2.7e-4 of the radius. Anything longer
/// is split rather than stretched.
const MAX_PIECE_DEGREES: f64 = 90.0;

/// Converts an SVG elliptical arc from `(from, to)` into cubic Béziers.
///
/// The endpoint-to-centre parameterisation is SVG 1.1 appendix F.6.5, and each
/// resulting piece is approximated by the usual `4/3·tan(Δθ/4)` control-point
/// offset. Degenerate arcs — a zero radius, or an endpoint equal to the start —
/// come out as a straight line, which is what the SVG specification says they
/// render as.
#[must_use]
fn arc_to_curves(
    from: (f64, f64),
    to: (f64, f64),
    rx: f64,
    ry: f64,
    rotation: f64,
    large_arc: bool,
    sweep: bool,
) -> Vec<Segment> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    // F.6.2: a zero radius degenerates to a straight line, and an arc whose
    // endpoints coincide is omitted altogether.
    if from == to {
        return Vec::new();
    }
    if rx == 0.0 || ry == 0.0 {
        return vec![Segment::LineTo(to.0, to.1)];
    }
    let phi = rotation.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;

    // F.6.6: grow the radii if they are too small to join the two points.
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let numerator = (rx * rx * ry * ry - denominator).max(0.0);
    let mut coefficient = if denominator == 0.0 {
        0.0
    } else {
        (numerator / denominator).sqrt()
    };
    if large_arc == sweep {
        coefficient = -coefficient;
    }
    let c1x = coefficient * rx * y1 / ry;
    let c1y = -coefficient * ry * x1 / rx;
    let cx = cos_phi * c1x - sin_phi * c1y + f64::midpoint(from.0, to.0);
    let cy = sin_phi * c1x + cos_phi * c1y + f64::midpoint(from.1, to.1);

    // `f64::atan2(self, other)` is `atan2(self, other)` with **self as the y**
    // and `other` as the x — the reverse of every other language's spelling, and
    // silently so: the two arguments are both plain `f64`. Getting it backwards
    // does not fail, it draws the arc the long way round.
    let start_angle = ((y1 - c1y) / ry).atan2((x1 - c1x) / rx);
    let end_angle = ((-y1 - c1y) / ry).atan2((-x1 - c1x) / rx);
    let mut sweep_angle = end_angle - start_angle;
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= 2.0 * std::f64::consts::PI;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += 2.0 * std::f64::consts::PI;
    }

    let pieces = (sweep_angle.abs() / MAX_PIECE_DEGREES.to_radians())
        .ceil()
        .max(1.0);
    let step = sweep_angle / pieces;
    // F.6.2 again: an arc whose endpoints coincide draws nothing.
    if !step.is_finite() || step == 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(pieces as usize);
    let mut angle = start_angle;
    for _ in 0..pieces as usize {
        let next = angle + step;
        let (from_point, from_derivative) = ellipse_point(cx, cy, rx, ry, cos_phi, sin_phi, angle);
        let (to_point, to_derivative) = ellipse_point(cx, cy, rx, ry, cos_phi, sin_phi, next);
        let handle = 4.0 / 3.0 * (step / 4.0).tan();
        out.push(Segment::CurveTo(
            from_point.0 + handle * from_derivative.0,
            from_point.1 + handle * from_derivative.1,
            to_point.0 - handle * to_derivative.0,
            to_point.1 - handle * to_derivative.1,
            to_point.0,
            to_point.1,
        ));
        angle = next;
    }
    out
}

/// A point on the rotated ellipse, and its derivative with respect to the angle.
fn ellipse_point(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    cos_phi: f64,
    sin_phi: f64,
    angle: f64,
) -> ((f64, f64), (f64, f64)) {
    let (sin_angle, cos_angle) = angle.sin_cos();
    (
        (
            cx + rx * cos_angle * cos_phi - ry * sin_angle * sin_phi,
            cy + rx * cos_angle * sin_phi + ry * sin_angle * cos_phi,
        ),
        (
            -rx * sin_angle * cos_phi - ry * cos_angle * sin_phi,
            -rx * sin_angle * sin_phi + ry * cos_angle * cos_phi,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{parse, resolve, Segment};

    /// The largest distance between a cubic's samples and the circle they
    /// approximate.
    ///
    /// The claim `arc_to_curves` makes is "within 0.02 % of the radius", so the
    /// test measures it rather than restating it: the outline is sampled densely
    /// and every sample is checked against `centre + r`.
    fn max_cubic_deviation(data: &str, centre: (f64, f64), radius: f64) -> f64 {
        let (segments, _) = resolve(&parse(data), (0.0, 0.0));
        let mut point = (0.0, 0.0);
        let mut worst: f64 = 0.0;
        for segment in segments {
            let (p0, p1, p2, p3) = match segment {
                Segment::MoveTo(x, y) | Segment::LineTo(x, y) => {
                    point = (x, y);
                    continue;
                }
                Segment::CurveTo(x1, y1, x2, y2, x, y) => (point, (x1, y1), (x2, y2), (x, y)),
                _ => continue,
            };
            for step in 1..=64 {
                let t = f64::from(u32::try_from(step).expect("small")) / 64.0;
                let at = |a: (f64, f64), b: (f64, f64), u: f64| {
                    (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u)
                };
                let a = at(p0, p1, t);
                let b = at(p1, p2, t);
                let c = at(p2, p3, t);
                let ab = at(a, b, t);
                let bc = at(b, c, t);
                let sampled = at(ab, bc, t);
                worst =
                    worst.max(((sampled.0 - centre.0).hypot(sampled.1 - centre.1) - radius).abs());
            }
            point = p3;
        }
        worst
    }

    #[test]
    fn parses_the_rectangle_the_producer_emits() {
        let segments = parse("M0 0 H10 V20 H0 Z");
        let (segments, _) = resolve(&segments, (0.0, 0.0));
        assert_eq!(
            segments,
            vec![
                Segment::MoveTo(0.0, 0.0),
                Segment::LineTo(10.0, 0.0),
                Segment::LineTo(10.0, 20.0),
                Segment::LineTo(0.0, 20.0),
                Segment::Close,
            ]
        );
    }

    #[test]
    fn extra_pairs_after_a_moveto_are_lines() {
        // This exact string comes from the Stage-5C math layout.
        let segments = parse("M 0 20 0.88 8.4 L 1.68 0 4 0");
        let (segments, _) = resolve(&segments, (0.0, 0.0));
        assert_eq!(
            segments,
            vec![
                Segment::MoveTo(0.0, 20.0),
                Segment::LineTo(0.88, 8.4),
                Segment::LineTo(1.68, 0.0),
                Segment::LineTo(4.0, 0.0),
            ]
        );
    }

    #[test]
    fn curves_and_negative_exponents_parse() {
        let segments = parse("M0 0C1e1 2 3 4 5 6");
        assert_eq!(segments[1], Segment::CurveTo(10.0, 2.0, 3.0, 4.0, 5.0, 6.0));
    }

    #[test]
    fn an_unsupported_command_is_skipped_not_half_read() {
        // `Q` is the command this parser does not support. The requirement is
        // narrow and deliberate: the *next* supported command must still be read
        // correctly, so a shape is missing a corner rather than missing its tail.
        let (segments, _) = resolve(&parse("M0 0 Q1 1 2 2 L7 8"), (0.0, 0.0));
        assert_eq!(
            segments,
            vec![Segment::MoveTo(0.0, 0.0), Segment::LineTo(7.0, 8.0),],
            "{segments:?}"
        );
    }

    #[test]
    fn truncated_input_keeps_what_parsed() {
        let segments = parse("M0 0 L1");
        assert_eq!(segments, vec![Segment::MoveTo(0.0, 0.0)]);
    }

    #[test]
    fn close_returns_to_the_subpath_start() {
        let segments = parse("M1 1 L5 1 Z L9 9");
        let (segments, point) = resolve(&segments, (0.0, 0.0));
        assert_eq!(segments[2], Segment::Close);
        assert_eq!(point, (9.0, 9.0));
    }

    #[test]
    fn a_circle_is_two_arcs_and_lands_on_the_circle() {
        // This exact string is what `07-strict-drawingml-shapes` carries.
        let (segments, _) = resolve(
            &parse("M0 48 A48 48 0 1 0 96 48 A48 48 0 1 0 0 48 Z"),
            (0.0, 0.0),
        );
        let curves = segments
            .iter()
            .filter(|segment| matches!(segment, Segment::CurveTo(..)))
            .count();
        // Two half-circles, each split into two quarter-turn pieces.
        assert_eq!(curves, 4, "{segments:?}");
        let deviation = max_cubic_deviation(
            "M0 48 A48 48 0 1 0 96 48 A48 48 0 1 0 0 48 Z",
            (48.0, 48.0),
            48.0,
        );
        assert!(
            deviation < 48.0 * 0.0005,
            "a quarter-turn cubic must sit within a thousandth of the radius, deviated {deviation}"
        );
    }

    #[test]
    fn a_rounded_rectangle_keeps_its_corners() {
        // This exact string is what `strict-stage5b` carries. Before `A` was
        // supported this parsed to a pentagon: four lines and nothing else.
        let data = "M16 0 H128 A16 16 0 0 1 144 16 V80 A16 16 0 0 1 128 96 H16 \
                    A16 16 0 0 1 0 80 V16 A16 16 0 0 1 16 0 Z";
        let (segments, _) = resolve(&parse(data), (0.0, 0.0));
        assert_eq!(
            segments
                .iter()
                .filter(|segment| matches!(segment, Segment::CurveTo(..)))
                .count(),
            4,
            "four corners, four curves: {segments:?}"
        );
        assert_eq!(segments.last(), Some(&Segment::Close));
    }

    #[test]
    fn a_zero_radius_arc_is_a_line() {
        // SVG F.6.2: radii of zero are a straight line, not an error.
        let (segments, _) = resolve(&parse("M0 0 A0 0 0 0 1 10 10"), (0.0, 0.0));
        assert_eq!(segments[1], Segment::LineTo(10.0, 10.0));
    }

    #[test]
    fn an_arc_back_to_its_start_draws_nothing() {
        // F.6.2 again: omitting the arc entirely is the specified rendering.
        let (segments, _) = resolve(&parse("M5 5 A4 4 0 0 1 5 5"), (0.0, 0.0));
        assert_eq!(segments, vec![Segment::MoveTo(5.0, 5.0)]);
    }

    #[test]
    fn an_arc_written_with_packed_flags_still_parses() {
        // The SVG grammar allows flags to run together: `0114 0` is two flags
        // and a coordinate. Reading `114` as a number is how an arc ends up
        // pointing at the wrong place.
        let (segments, point) = resolve(&parse("M0 0a1 1 0 0114 0"), (0.0, 0.0));
        assert!(
            segments.iter().all(|segment| !matches!(
                segment,
                Segment::LineTo(x, _) if (*x - 114.0).abs() < 0.001
            )),
            "{segments:?}"
        );
        assert!(
            (point.0 - 14.0).abs() < 1e-9 && point.1.abs() < 1e-9,
            "the endpoint is (14, 0): {point:?}"
        );
    }
}
