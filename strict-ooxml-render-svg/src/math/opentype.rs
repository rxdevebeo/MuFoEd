//! The OpenType `MATH` table of the bundled math face, STIX Two Math.
//!
//! The table carries what a formula needs beyond a glyph's outline: the
//! constants a radical's gap and rule are set with, where an accent attaches
//! to a letter, and for the signs that grow (`√`, `∫`, `∑`, wide accents) a
//! ladder of larger cuts plus, for some, the parts to assemble any height
//! from. The vector shapes in [`super::shapes`] stand in for these only when
//! the table cannot be read.
//!
//! The table is read directly from the face's bytes (OpenType 1.9, "MATH —
//! The mathematical typesetting table"); outlines come from `skrifa`. Every
//! measure is returned in em, `y` up, origin on the baseline.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider as _, Tag};

/// The face whose `MATH` table and outlines are used.
const FAMILY: &str = "STIX Two Math";

/// Most repetitions of an assembly's extenders: far beyond any formula.
const MAX_REPEATS: usize = 64;

/// The positioning constants this renderer uses, in em.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Constants {
    /// `DisplayOperatorMinHeight`: the least height of a display-style large
    /// operator.
    pub(crate) display_operator_min_height: f64,
    /// `AccentBaseHeight`: an accent is designed to sit on a base this tall
    /// and is raised only by what a base has above it.
    pub(crate) accent_base_height: f64,
    /// `SubscriptBaselineDropMin`: a subscript's baseline lies at least this
    /// far below the bottom of a tall base.
    pub(crate) subscript_baseline_drop_min: f64,
    /// `SuperscriptBaselineDropMax`: a superscript's baseline lies at most
    /// this far below the top of a tall base.
    pub(crate) superscript_baseline_drop_max: f64,
    /// `RadicalVerticalGap`: white between the radicand and the rule.
    pub(crate) radical_vertical_gap: f64,
    /// `RadicalDisplayStyleVerticalGap`: the same in display style.
    pub(crate) radical_display_style_vertical_gap: f64,
    /// `RadicalRuleThickness`.
    pub(crate) radical_rule_thickness: f64,
    /// `RadicalExtraAscender`: white kept above the rule.
    pub(crate) radical_extra_ascender: f64,
    /// `RadicalKernBeforeDegree`.
    pub(crate) radical_kern_before_degree: f64,
    /// `RadicalKernAfterDegree` (negative: the degree overlaps the sign).
    pub(crate) radical_kern_after_degree: f64,
    /// `RadicalDegreeBottomRaisePercent`, as a fraction of the sign's height.
    pub(crate) radical_degree_bottom_raise: f64,
}

/// One glyph, or an assembly of glyphs, as a filled outline.
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    /// The glyph whose accent attachment applies (the first part of an
    /// assembly).
    pub(crate) glyph: u32,
    commands: Vec<Command>,
    /// Advance width, in em.
    pub(crate) advance: f64,
    /// Ink bounds `[x_min, y_min, x_max, y_max]`, in em, `y` up.
    pub(crate) bounds: [f64; 4],
    /// Italic correction, in em: how far the top of the shape leans past its
    /// advance.
    pub(crate) italic: f64,
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Close,
}

impl Shape {
    /// Height of the ink, in em.
    pub(crate) fn height(&self) -> f64 {
        self.bounds[3] - self.bounds[1]
    }

    /// The outline as SVG path data at `size_px` per em, in the local box of
    /// its ink (`0..w`, `0..h`, `y` down), with that box's width and height.
    pub(crate) fn local_path(&self, size_px: f64) -> (String, f64, f64) {
        use std::fmt::Write as _;
        let [x_min, y_min, x_max, y_max] = self.bounds;
        let x = |value: f64| crate::units::fmt_num(crate::units::finite((value - x_min) * size_px));
        let y = |value: f64| crate::units::fmt_num(crate::units::finite((y_max - value) * size_px));
        let mut d = String::with_capacity(self.commands.len() * 24);
        for command in &self.commands {
            let _ = match *command {
                Command::Move(px, py) => write!(d, "M{} {}", x(px), y(py)),
                Command::Line(px, py) => write!(d, "L{} {}", x(px), y(py)),
                Command::Curve(ax, ay, bx, by, px, py) => write!(
                    d,
                    "C{} {} {} {} {} {}",
                    x(ax),
                    y(ay),
                    x(bx),
                    y(by),
                    x(px),
                    y(py)
                ),
                Command::Close => write!(d, "Z"),
            };
        }
        (d, (x_max - x_min) * size_px, (y_max - y_min) * size_px)
    }

    /// This shape scaled by `factor` about its origin.
    pub(crate) fn scaled(&self, factor: f64) -> Self {
        let commands = self
            .commands
            .iter()
            .map(|command| match *command {
                Command::Move(x, y) => Command::Move(x * factor, y * factor),
                Command::Line(x, y) => Command::Line(x * factor, y * factor),
                Command::Curve(ax, ay, bx, by, x, y) => Command::Curve(
                    ax * factor,
                    ay * factor,
                    bx * factor,
                    by * factor,
                    x * factor,
                    y * factor,
                ),
                Command::Close => Command::Close,
            })
            .collect();
        Self {
            glyph: self.glyph,
            commands,
            advance: self.advance * factor,
            bounds: self.bounds.map(|value| value * factor),
            italic: self.italic * factor,
        }
    }

    /// This shape moved up by `dy` em.
    fn raised(&self, dy: f64) -> Vec<Command> {
        self.commands
            .iter()
            .map(|command| match *command {
                Command::Move(x, y) => Command::Move(x, y + dy),
                Command::Line(x, y) => Command::Line(x, y + dy),
                Command::Curve(ax, ay, bx, by, x, y) => {
                    Command::Curve(ax, ay + dy, bx, by + dy, x, y + dy)
                }
                Command::Close => Command::Close,
            })
            .collect()
    }
}

/// The parsed face: the `MATH` table's bytes and the units per em.
struct MathFace {
    font: FontRef<'static>,
    math: &'static [u8],
    units: f64,
    constants: Option<Constants>,
}

fn face() -> Option<&'static MathFace> {
    static FACE: OnceLock<Option<MathFace>> = OnceLock::new();
    FACE.get_or_init(|| {
        let source = crate::font::builtin::face_source(FAMILY, false, false)?;
        let font = FontRef::new(source.data).ok()?;
        let math = font.table_data(Tag::new(b"MATH"))?.as_bytes();
        let units = f64::from(
            font.metrics(Size::unscaled(), LocationRef::default())
                .units_per_em,
        );
        if units <= 0.0 || u16_at(math, 0)? != 1 {
            return None;
        }
        let mut face = MathFace {
            font,
            math,
            units,
            constants: None,
        };
        face.constants = face.read_constants();
        Some(face)
    })
    .as_ref()
}

/// The positioning constants, when the table has them.
pub(crate) fn constants() -> Option<Constants> {
    face()?.constants
}

/// The smallest cut of `ch` at least `min_height` em tall (ink). When no cut
/// is tall enough and `assemble` is set, the sign is assembled from its parts
/// to that height; otherwise the largest cut is returned.
pub(crate) fn vertical(ch: char, min_height: f64, assemble: bool) -> Option<Shape> {
    let face = face()?;
    let glyph = face.font.charmap().map(ch)?.to_u32();
    face.stretched(glyph, min_height, assemble)
}

/// The widest cut of the accent `ch` no wider than `max_width` em, or the
/// accent itself.
pub(crate) fn accent(ch: char, max_width: f64) -> Option<Shape> {
    let face = face()?;
    let glyph = face.font.charmap().map(ch)?.to_u32();
    let mut chosen = glyph;
    if let Some(construction) = face.construction(glyph, false) {
        for (variant, advance) in face.variants(construction) {
            if advance > max_width {
                break;
            }
            chosen = variant;
        }
    }
    face.shape(chosen)
}

/// Where an accent attaches over `ch` drawn in this face, in em from the
/// glyph's origin; `italic` takes the mathematical italic letter.
pub(crate) fn top_accent_attachment_of_char(ch: char, italic: bool) -> Option<f64> {
    let face = face()?;
    let ch = if italic { math_italic(ch) } else { ch };
    let glyph = face.font.charmap().map(ch)?.to_u32();
    face.top_accent_attachment(glyph)
}

/// Where an accent attaches over `shape`, in em from its origin.
pub(crate) fn top_accent_attachment(shape: &Shape) -> f64 {
    face()
        .and_then(|face| face.top_accent_attachment(shape.glyph))
        .unwrap_or(f64::midpoint(shape.bounds[0], shape.bounds[2]))
}

/// The Mathematical Italic letter for a Latin or Greek letter (ISO/IEC
/// 10646, Mathematical Alphanumeric Symbols); `ch` itself otherwise.
fn math_italic(ch: char) -> char {
    let offset = |base: u32, from: char| {
        char::from_u32(base + (u32::from(ch) - u32::from(from))).unwrap_or(ch)
    };
    match ch {
        // The italic h is the letterlike Planck constant.
        'h' => '\u{210E}',
        'A'..='Z' => offset(0x1D434, 'A'),
        'a'..='z' => offset(0x1D44E, 'a'),
        '\u{0391}'..='\u{03A9}' => offset(0x1D6E2, '\u{0391}'),
        '\u{03B1}'..='\u{03C9}' => offset(0x1D6FC, '\u{03B1}'),
        _ => ch,
    }
}

impl MathFace {
    fn em(&self, units: f64) -> f64 {
        units / self.units
    }

    fn read_constants(&self) -> Option<Constants> {
        let table = Self::offset(self.math, 4)?;
        // Each MathValueRecord is a 2-byte value and a 2-byte device offset,
        // from byte 8 on.
        let record = |index: usize| -> Option<f64> {
            Some(self.em(f64::from(i16_at(table, 8 + index * 4)?)))
        };
        Some(Constants {
            display_operator_min_height: self.em(f64::from(u16_at(table, 6)?)),
            accent_base_height: record(2)?,
            subscript_baseline_drop_min: record(6)?,
            superscript_baseline_drop_max: record(10)?,
            radical_vertical_gap: record(45)?,
            radical_display_style_vertical_gap: record(46)?,
            radical_rule_thickness: record(47)?,
            radical_extra_ascender: record(48)?,
            radical_kern_before_degree: record(49)?,
            radical_kern_after_degree: record(50)?,
            radical_degree_bottom_raise: f64::from(i16_at(table, 212)?) / 100.0,
        })
    }

    /// The subtable an `Offset16` at `at` in `data` points to.
    fn offset(data: &'static [u8], at: usize) -> Option<&'static [u8]> {
        let offset = usize::from(u16_at(data, at)?);
        if offset == 0 {
            return None;
        }
        data.get(offset..)
    }

    /// The `MathValueRecord` for `glyph` in a coverage-indexed list
    /// (`MathItalicsCorrectionInfo`, `MathTopAccentAttachment`), in em.
    fn glyph_value(&self, which: usize, glyph: u32) -> Option<f64> {
        let info = Self::offset(self.math, 6)?;
        let list = Self::offset(info, which)?;
        let coverage = Self::offset(list, 0)?;
        let index = coverage_index(coverage, glyph)?;
        let count = usize::from(u16_at(list, 2)?);
        if index >= count {
            return None;
        }
        Some(self.em(f64::from(i16_at(list, 4 + index * 4)?)))
    }

    fn italic_correction(&self, glyph: u32) -> Option<f64> {
        self.glyph_value(0, glyph)
    }

    fn top_accent_attachment(&self, glyph: u32) -> Option<f64> {
        self.glyph_value(2, glyph)
    }

    /// The `MathGlyphConstruction` of `glyph`, vertical or horizontal.
    fn construction(&self, glyph: u32, vertical: bool) -> Option<&'static [u8]> {
        let variants = Self::offset(self.math, 8)?;
        let coverage = Self::offset(variants, if vertical { 2 } else { 4 })?;
        let index = coverage_index(coverage, glyph)?;
        let vertical_count = usize::from(u16_at(variants, 6)?);
        let horizontal_count = usize::from(u16_at(variants, 8)?);
        let slot = if vertical {
            (index < vertical_count).then_some(10 + index * 2)?
        } else {
            (index < horizontal_count).then_some(10 + (vertical_count + index) * 2)?
        };
        Self::offset(variants, slot)
    }

    /// The cuts of a construction, smallest first: glyph and its measure in em.
    fn variants(&self, construction: &'static [u8]) -> Vec<(u32, f64)> {
        let count = u16_at(construction, 2).map_or(0, usize::from);
        (0..count)
            .filter_map(|index| {
                let glyph = u16_at(construction, 4 + index * 4)?;
                let advance = u16_at(construction, 6 + index * 4)?;
                Some((u32::from(glyph), self.em(f64::from(advance))))
            })
            .collect()
    }

    fn stretched(&self, glyph: u32, min_height: f64, assemble: bool) -> Option<Shape> {
        let Some(construction) = self.construction(glyph, true) else {
            return self.shape(glyph);
        };
        let variants = self.variants(construction);
        if let Some((variant, _)) = variants.iter().find(|(_, advance)| *advance >= min_height) {
            return self.shape(*variant);
        }
        if assemble {
            if let Some(shape) = Self::offset(construction, 0)
                .and_then(|assembly| self.assembly(assembly, min_height))
            {
                return Some(shape);
            }
        }
        let largest = variants.last().map_or(glyph, |(variant, _)| *variant);
        self.shape(largest)
    }

    /// A vertical `GlyphAssembly` built to `height` em: the extenders are
    /// repeated as often as the height needs, then the overlaps between
    /// connectors are spread evenly (never below `MinConnectorOverlap`).
    fn assembly(&self, assembly: &'static [u8], height: f64) -> Option<Shape> {
        struct Part {
            glyph: u32,
            start: f64,
            end: f64,
            advance: f64,
            extender: bool,
        }
        let italic = self.em(f64::from(i16_at(assembly, 0)?));
        let count = usize::from(u16_at(assembly, 4)?);
        let parts: Vec<Part> = (0..count)
            .filter_map(|index| {
                let at = 6 + index * 10;
                Some(Part {
                    glyph: u32::from(u16_at(assembly, at)?),
                    start: self.em(f64::from(u16_at(assembly, at + 2)?)),
                    end: self.em(f64::from(u16_at(assembly, at + 4)?)),
                    advance: self.em(f64::from(u16_at(assembly, at + 6)?)),
                    extender: u16_at(assembly, at + 8)? & 1 == 1,
                })
            })
            .collect();
        if parts.is_empty() {
            return None;
        }
        let overlap_min = self.em(f64::from(u16_at(Self::offset(self.math, 8)?, 0)?));
        // Bottom to top, extenders repeated `repeats` times.
        let expand = |repeats: usize| -> Vec<&Part> {
            parts
                .iter()
                .flat_map(|part| {
                    let times = if part.extender { repeats } else { 1 };
                    std::iter::repeat_n(part, times)
                })
                .collect()
        };
        let reach = |list: &[&Part]| -> f64 {
            list.iter().map(|part| part.advance).sum::<f64>()
                - overlap_min * (list.len().saturating_sub(1) as f64)
        };
        let mut repeats = 1;
        let mut list = expand(repeats);
        while reach(&list) < height && repeats < MAX_REPEATS && parts.iter().any(|p| p.extender) {
            repeats += 1;
            list = expand(repeats);
        }
        let joints = list.len().saturating_sub(1);
        let overlap = if joints == 0 {
            0.0
        } else {
            let most = list
                .windows(2)
                .filter_map(|pair| match pair {
                    [below, above] => Some(below.end.min(above.start)),
                    _ => None,
                })
                .fold(f64::INFINITY, f64::min)
                .max(overlap_min);
            let total: f64 = list.iter().map(|part| part.advance).sum();
            ((total - height) / joints as f64).clamp(overlap_min, most)
        };
        let mut commands = Vec::new();
        let mut advance: f64 = 0.0;
        let mut y = 0.0;
        for part in &list {
            let shape = self.shape(part.glyph)?;
            // Each part stands on the previous one: its ink bottom at `y`.
            commands.extend(shape.raised(y - shape.bounds[1]));
            advance = advance.max(shape.advance);
            y += part.advance - overlap;
        }
        let first = list.first().map_or(0, |part| part.glyph);
        Some(Shape::from_commands(first, commands, advance, italic))
    }

    /// The outline of one glyph, cached.
    fn shape(&self, glyph: u32) -> Option<Shape> {
        static CACHE: OnceLock<Mutex<HashMap<u32, Arc<Shape>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(hit) = cache.lock().ok().and_then(|map| map.get(&glyph).cloned()) {
            return Some((*hit).clone());
        }
        let id = GlyphId::new(glyph);
        let outline = self.font.outline_glyphs().get(id)?;
        let mut pen = Pen {
            scale: 1.0 / self.units,
            commands: Vec::new(),
            current: (0.0, 0.0),
        };
        outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                &mut pen,
            )
            .ok()?;
        let advance = self
            .font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(id)
            .map_or(0.0, |advance| self.em(f64::from(advance)));
        let italic = self.italic_correction(glyph).unwrap_or(0.0);
        let shape = Shape::from_commands(glyph, pen.commands, advance, italic);
        if let Ok(mut map) = cache.lock() {
            map.insert(glyph, Arc::new(shape.clone()));
        }
        Some(shape)
    }
}

impl Shape {
    fn from_commands(glyph: u32, commands: Vec<Command>, advance: f64, italic: f64) -> Self {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut grow = |x: f64, y: f64| {
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        };
        for command in &commands {
            match *command {
                Command::Move(x, y) | Command::Line(x, y) => grow(x, y),
                Command::Curve(ax, ay, bx, by, x, y) => {
                    // The control points bound a cubic: close enough for a
                    // glyph, whose extremes are on-curve points by design.
                    grow(ax, ay);
                    grow(bx, by);
                    grow(x, y);
                }
                Command::Close => {}
            }
        }
        if !bounds.iter().all(|value| value.is_finite()) {
            bounds = [0.0; 4];
        }
        Self {
            glyph,
            commands,
            advance,
            bounds,
            italic,
        }
    }
}

/// Collects an outline in em, quadratic segments raised to cubic ones (the
/// PDF backend draws cubic curves only).
struct Pen {
    scale: f64,
    commands: Vec<Command>,
    current: (f64, f64),
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = (f64::from(x) * self.scale, f64::from(y) * self.scale);
        self.commands.push(Command::Move(point.0, point.1));
        self.current = point;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = (f64::from(x) * self.scale, f64::from(y) * self.scale);
        self.commands.push(Command::Line(point.0, point.1));
        self.current = point;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (sx, sy) = self.current;
        let (cx, cy) = (f64::from(cx0) * self.scale, f64::from(cy0) * self.scale);
        let (ex, ey) = (f64::from(x) * self.scale, f64::from(y) * self.scale);
        self.commands.push(Command::Curve(
            sx + 2.0 / 3.0 * (cx - sx),
            sy + 2.0 / 3.0 * (cy - sy),
            ex + 2.0 / 3.0 * (cx - ex),
            ey + 2.0 / 3.0 * (cy - ey),
            ex,
            ey,
        ));
        self.current = (ex, ey);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let s = self.scale;
        let point = (f64::from(x) * s, f64::from(y) * s);
        self.commands.push(Command::Curve(
            f64::from(cx0) * s,
            f64::from(cy0) * s,
            f64::from(cx1) * s,
            f64::from(cy1) * s,
            point.0,
            point.1,
        ));
        self.current = point;
    }

    fn close(&mut self) {
        self.commands.push(Command::Close);
    }
}

/// The index of `glyph` in an OpenType coverage table.
fn coverage_index(coverage: &[u8], glyph: u32) -> Option<usize> {
    let glyph = u16::try_from(glyph).ok()?;
    let count = usize::from(u16_at(coverage, 2)?);
    match u16_at(coverage, 0)? {
        1 => (0..count).find(|index| u16_at(coverage, 4 + index * 2) == Some(glyph)),
        2 => (0..count).find_map(|index| {
            let at = 4 + index * 6;
            let start = u16_at(coverage, at)?;
            let end = u16_at(coverage, at + 2)?;
            let first = u16_at(coverage, at + 4)?;
            (start..=end)
                .contains(&glyph)
                .then(|| usize::from(first) + usize::from(glyph - start))
        }),
        _ => None,
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_face_has_a_math_table() {
        let constants = constants().expect("constants");
        assert!(constants.display_operator_min_height > 1.0);
        assert!(constants.accent_base_height > 0.3 && constants.accent_base_height < 0.7);
        assert!(constants.radical_rule_thickness > 0.0);
        assert!(constants.radical_kern_after_degree < 0.0);
        assert!(constants.radical_degree_bottom_raise > 0.0);
    }

    #[test]
    fn signs_grow_by_cuts_and_then_by_assembly() {
        let small = vertical('\u{221A}', 0.0, false).expect("sqrt");
        let tall = vertical('\u{221A}', 3.0, false).expect("sqrt");
        assert!(tall.height() > small.height());
        let assembled = vertical('\u{221A}', 12.0, true).expect("assembled sqrt");
        assert!(assembled.height() >= 11.0, "{}", assembled.height());
        let integral = vertical('\u{222B}', 12.0, true).expect("assembled integral");
        assert!(integral.height() >= 11.0, "{}", integral.height());
        let display = vertical(
            '\u{2211}',
            constants().expect("c").display_operator_min_height,
            false,
        )
        .expect("sum");
        assert!(display.height() > 1.0);
    }

    #[test]
    fn accents_attach_and_widen() {
        let hat = accent('\u{0302}', 0.0).expect("hat");
        let wide = accent('\u{0302}', 3.0).expect("wide hat");
        assert!(wide.bounds[2] - wide.bounds[0] > hat.bounds[2] - hat.bounds[0]);
        let upright = top_accent_attachment_of_char('x', false).expect("x");
        let italic = top_accent_attachment_of_char('x', true).expect("italic x");
        assert!(upright > 0.0 && italic > 0.0);
        assert_eq!(math_italic('h'), '\u{210E}');
        assert_eq!(math_italic('b'), '\u{1D44F}');
        assert_eq!(math_italic('+'), '+');
    }

    #[test]
    fn a_path_lies_in_its_box() {
        let shape = vertical('\u{222B}', 2.0, false).expect("integral");
        let (d, w, h) = shape.local_path(20.0);
        assert!(d.starts_with('M') && d.contains('Z'), "{d}");
        assert!(w > 0.0 && h > 0.0);
        for value in d
            .split(|c: char| c.is_ascii_alphabetic() || c == ' ')
            .filter_map(|token| token.parse::<f64>().ok())
        {
            assert!((-0.01..=w.max(h) + 0.01).contains(&value), "{value}");
        }
    }
}
