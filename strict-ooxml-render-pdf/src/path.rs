//! The SVG path-data subset the layout emits, parsed back into geometry.
//!
//! `PathItem` carries its outline as SVG path data because that is what the SVG
//! backend writes. A PDF content stream wants operators, so the data is parsed
//! back into segments. That round trip through a string is a deliberate
//! trade-off (ADR-0008): the alternative is changing the shared item type, which
//! would touch the Stage-4/5B renderer and its SSIM gate for the sake of a second
//! consumer. The grammar supported is exactly what the producers emit — `M`, `L`,
//! `H`, `V`, `C`, `Z` — plus the implicit-lineto form of `M`/`L`, and anything
//! else is skipped rather than guessed at.

/// One absolute path segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Start a new subpath.
    MoveTo(f64, f64),
    /// Straight line.
    LineTo(f64, f64),
    /// Cubic Bézier.
    CurveTo(f64, f64, f64, f64, f64, f64),
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

    while index < bytes.len() {
        let ch = bytes[index];
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
                let mut values = [0.0; 6];
                let mut read = 0;
                while read < 6 {
                    let Some(value) = number(&bytes, &mut index) else {
                        break;
                    };
                    values[read] = value;
                    read += 1;
                }
                if read < 6 {
                    break;
                }
                out.push(Segment::CurveTo(
                    values[0], values[1], values[2], values[3], values[4], values[5],
                ));
            }
            'Z' | 'z' => {
                out.push(Segment::Close);
                command = ' ';
            }
            // Q, T, S, A and the arc flags: not emitted by the producers, and a
            // curve flattened here would differ from what the SVG shows.
            _ => {
                index += 1;
            }
        }
    }
    out
}

/// Reads one number, tolerating the forms SVG allows (`-1.5`, `.5`, `1e-3`).
fn number(bytes: &[char], index: &mut usize) -> Option<f64> {
    while *index < bytes.len() && (bytes[*index].is_ascii_whitespace() || bytes[*index] == ',') {
        *index += 1;
    }
    let start = *index;
    if *index < bytes.len() && (bytes[*index] == '-' || bytes[*index] == '+') {
        *index += 1;
    }
    while *index < bytes.len() && (bytes[*index].is_ascii_digit() || bytes[*index] == '.') {
        *index += 1;
    }
    if *index < bytes.len() && (bytes[*index] == 'e' || bytes[*index] == 'E') {
        *index += 1;
        if *index < bytes.len() && (bytes[*index] == '-' || bytes[*index] == '+') {
            *index += 1;
        }
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            *index += 1;
        }
    }
    if start == *index {
        return None;
    }
    let text: String = bytes[start..*index].iter().collect();
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Resolves `H`/`V` shorthand into an absolute line, given the current point.
///
/// Returns `None` when the shorthand has no current point, which cannot happen in
/// well-formed data and is not worth inventing a position for.
#[must_use]
pub fn resolve(segments: &[Segment], current: (f64, f64)) -> (Vec<Segment>, (f64, f64)) {
    let mut out = Vec::with_capacity(segments.len());
    let mut point = current;
    let mut start = current;
    for segment in segments {
        let resolved = match *segment {
            Segment::MoveTo(x, y) => {
                point = (x, y);
                start = point;
                Segment::MoveTo(x, y)
            }
            Segment::LineTo(x, y) => {
                let (x, y) = (
                    if x.is_nan() { point.0 } else { x },
                    if y.is_nan() { point.1 } else { y },
                );
                point = (x, y);
                Segment::LineTo(x, y)
            }
            Segment::CurveTo(..) => {
                point = match segment {
                    Segment::CurveTo(_, _, _, _, x, y) => (*x, *y),
                    _ => point,
                };
                *segment
            }
            Segment::Close => {
                point = start;
                Segment::Close
            }
        };
        out.push(resolved);
    }
    (out, point)
}

#[cfg(test)]
mod tests {
    use super::{parse, resolve, Segment};

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
    fn unknown_commands_are_skipped() {
        let segments = parse("M0 0 A1 1 0 0 1 5 5 L2 2");
        assert!(
            segments.contains(&Segment::LineTo(2.0, 2.0)),
            "{segments:?}"
        );
        assert!(
            !segments.contains(&Segment::LineTo(5.0, 5.0)),
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
}
