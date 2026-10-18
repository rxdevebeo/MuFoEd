//! Backend-independent drawing primitives for structures and reactions.
//!
//! The scene is turned into a flat list of lines, filled polygons and text
//! runs once, then painted by either the egui widget or the SVG exporter. Text
//! metrics are approximate (an average advance per character), so layout is
//! stable across backends without needing a font.

#![allow(clippy::too_many_lines, clippy::many_single_char_names)]

use crate::element;
use crate::model::{Arrow, ChemObject, Marker, Part, Point, Reaction, Species, State, Structure};

/// Ink and text options for a rendering pass.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Default ink for bonds and text.
    pub ink: [u8; 3],
    /// Base font size, in logical pixels.
    pub font: f32,
    /// Draw element symbols in their CPK colour.
    pub colored_atoms: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            ink: [0x1e, 0x1e, 0x1e],
            font: 18.0,
            colored_atoms: true,
        }
    }
}

/// Horizontal text alignment; text is always vertically centred on its point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// The point is the left edge.
    Left,
    /// The point is the horizontal centre.
    Center,
    /// The point is the right edge.
    Right,
}

/// One drawing primitive.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    /// A stroked line.
    Line {
        /// Start point.
        from: Point,
        /// End point.
        to: Point,
        /// Stroke width.
        width: f32,
        /// sRGB colour.
        color: [u8; 3],
    },
    /// A filled polygon.
    Polygon {
        /// Outline points.
        points: Vec<Point>,
        /// sRGB fill.
        fill: [u8; 3],
    },
    /// A text run anchored at `pos`.
    Text {
        /// Anchor position.
        pos: Point,
        /// Text to draw.
        text: String,
        /// Font size.
        size: f32,
        /// sRGB colour.
        color: [u8; 3],
        /// Horizontal placement of `pos`.
        align: Align,
        /// Draw in italics.
        italic: bool,
    },
}

/// Approximate advance width of `text` at `size`.
#[must_use]
pub fn text_width(text: &str, size: f32) -> f32 {
    text.chars().map(|c| char_factor(c) * size).sum()
}

fn char_factor(c: char) -> f32 {
    match c {
        ' ' | '.' | ',' | ':' | '\'' | '!' | 'i' | 'l' | 'I' | '1' => 0.30,
        '(' | ')' | '[' | ']' | '-' | 'f' | 't' | 'r' => 0.36,
        'm' | 'w' | 'M' | 'W' => 0.80,
        c if c.is_ascii_uppercase() => 0.62,
        c if c.is_ascii_digit() => 0.58,
        c if c.is_ascii_lowercase() => 0.54,
        _ => 0.60,
    }
}

/// Renders one top-level object.
#[must_use]
pub fn object_prims(object: &ChemObject, theme: &Theme, show_carbon: bool) -> Vec<Prim> {
    match object {
        ChemObject::Structure(structure) => structure_prims(structure, theme, show_carbon),
        ChemObject::Reaction(reaction) => reaction_prims(reaction, theme),
    }
}

/// Axis-aligned bounds of a primitive list, or `None` when empty.
#[must_use]
pub fn prims_bounds(prims: &[Prim]) -> Option<(Point, Point)> {
    let mut min = Point::new(f32::INFINITY, f32::INFINITY);
    let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    let mut touch = |p: Point| {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    };
    for prim in prims {
        any = true;
        match prim {
            Prim::Line { from, to, .. } => {
                touch(*from);
                touch(*to);
            }
            Prim::Polygon { points, .. } => points.iter().for_each(|p| touch(*p)),
            Prim::Text {
                pos,
                text,
                size,
                align,
                ..
            } => {
                let width = text_width(text, *size);
                let left = match align {
                    Align::Left => pos.x,
                    Align::Center => pos.x - width / 2.0,
                    Align::Right => pos.x - width,
                };
                touch(Point::new(left, pos.y - size / 2.0));
                touch(Point::new(left + width, pos.y + size / 2.0));
            }
        }
    }
    any.then_some((min, max))
}

struct Label {
    text: String,
    charge: String,
    width: f32,
}

impl Label {
    fn radius(&self) -> f32 {
        self.width.midpoint(text_width(&self.charge, 12.0)) + 3.0
    }
}

/// Renders a structural formula.
#[must_use]
pub fn structure_prims(structure: &Structure, theme: &Theme, show_carbon: bool) -> Vec<Prim> {
    let mut prims = vec![];
    let labels: Vec<Option<Label>> = structure
        .atoms
        .iter()
        .map(|atom| atom_label(structure, atom, show_carbon, theme.font))
        .collect();
    // A labelled atom pushes its text outward, along the bonds' bisector, so
    // the ring lines do not cross the letter. Bonds are then trimmed less.
    let offsets: Vec<Point> = structure
        .atoms
        .iter()
        .map(|atom| structure.label_outward(atom).scale(theme.font * 0.6))
        .collect();
    let trim_at = |index: usize| {
        labels
            .get(index)
            .and_then(Option::as_ref)
            .map_or(0.0, |label| {
                (label.radius() - offsets[index].length()).max(1.5)
            })
    };
    let position = |id: u64| {
        structure
            .atom(id)
            .map(|atom| atom.position.add(structure.origin))
    };
    for bond in &structure.bonds {
        let (Some(a), Some(b)) = (position(bond.a), position(bond.b)) else {
            continue;
        };
        let from = trim_towards(a, b, trim_at(index_of(structure, bond.a)));
        let to = trim_towards(b, a, trim_at(index_of(structure, bond.b)));
        bond_prims(from, to, bond.order, bond.stereo, theme.ink, &mut prims);
    }
    for (index, atom) in structure.atoms.iter().enumerate() {
        let Some(label) = &labels[index] else {
            continue;
        };
        let p = atom.position.add(structure.origin).add(offsets[index]);
        let color = atom_color(atom.element, theme);
        let total = label.width + text_width(&label.charge, label_charge_size(theme));
        let left = p.x - total / 2.0;
        prims.push(Prim::Text {
            pos: Point::new(left, p.y),
            text: label.text.clone(),
            size: theme.font,
            color,
            align: Align::Left,
            italic: false,
        });
        if !label.charge.is_empty() {
            prims.push(Prim::Text {
                pos: Point::new(left + label.width, p.y - theme.font * 0.34),
                text: label.charge.clone(),
                size: label_charge_size(theme),
                color,
                align: Align::Left,
                italic: false,
            });
        }
    }
    prims
}

fn label_charge_size(theme: &Theme) -> f32 {
    theme.font * 0.7
}

fn index_of(structure: &Structure, id: u64) -> usize {
    structure
        .atoms
        .iter()
        .position(|atom| atom.id == id)
        .unwrap_or(usize::MAX)
}

fn atom_label(
    structure: &Structure,
    atom: &crate::model::Atom,
    show_carbon: bool,
    size: f32,
) -> Option<Label> {
    let symbol = atom.symbol();
    let hydrogens = structure.hydrogens(atom);
    let plain_carbon = atom.element == 6 && atom.charge == 0 && !show_carbon;
    if plain_carbon {
        return None;
    }
    let mut text = symbol.to_owned();
    if atom.element != 6 && hydrogens > 0 {
        text.push('H');
        if hydrogens > 1 {
            text.push_str(&hydrogens.to_string());
        }
    }
    let charge = charge_text(atom.charge);
    Some(Label {
        width: text_width(&text, size),
        text,
        charge,
    })
}

fn charge_text(charge: i8) -> String {
    match charge {
        0 => String::new(),
        1 => "+".into(),
        -1 => "-".into(),
        c if c > 0 => format!("{c}+"),
        c => format!("{}-", c.unsigned_abs()),
    }
}

fn atom_color(element_z: u8, theme: &Theme) -> [u8; 3] {
    if !theme.colored_atoms {
        return theme.ink;
    }
    let Some(info) = element::element(element_z) else {
        return theme.ink;
    };
    let color = info.color();
    if color.iter().all(|channel| *channel > 210) {
        theme.ink
    } else {
        color
    }
}

fn trim_towards(from: Point, towards: Point, radius: f32) -> Point {
    let delta = towards.sub(from);
    let length = delta.length();
    if radius <= 0.0 || length <= radius + 1.0 {
        return from;
    }
    Point::new(
        from.x + delta.x / length * radius,
        from.y + delta.y / length * radius,
    )
}

fn bond_prims(
    from: Point,
    to: Point,
    order: crate::model::BondOrder,
    stereo: crate::model::BondStereo,
    color: [u8; 3],
    out: &mut Vec<Prim>,
) {
    use crate::geometry::perpendicular;
    use crate::model::{BondOrder, BondStereo};
    let delta = to.sub(from);
    let length = delta.length();
    if length < 0.5 {
        return;
    }
    let perp = perpendicular(from, to);
    match stereo {
        BondStereo::Wedge => {
            let half = 5.0_f32.min(length * 0.35);
            out.push(Prim::Polygon {
                points: vec![
                    from,
                    Point::new(to.x + perp.x * half, to.y + perp.y * half),
                    Point::new(to.x - perp.x * half, to.y - perp.y * half),
                ],
                fill: color,
            });
            return;
        }
        BondStereo::Hash => {
            let half = 5.0_f32.min(length * 0.35);
            let steps = 6_u16;
            for step in 1..=steps {
                let t = f32::from(step) / f32::from(steps);
                let center = Point::new(from.x + delta.x * t, from.y + delta.y * t);
                let widen = half * t;
                out.push(Prim::Line {
                    from: Point::new(center.x + perp.x * widen, center.y + perp.y * widen),
                    to: Point::new(center.x - perp.x * widen, center.y - perp.y * widen),
                    width: 1.3,
                    color,
                });
            }
            return;
        }
        BondStereo::Plain => {}
    }
    let offsets: &[f32] = match order {
        BondOrder::Single | BondOrder::Aromatic => &[0.0],
        BondOrder::Double => &[-3.5, 3.5],
        BondOrder::Triple => &[-5.0, 0.0, 5.0],
    };
    for offset in offsets {
        out.push(Prim::Line {
            from: Point::new(from.x + perp.x * offset, from.y + perp.y * offset),
            to: Point::new(to.x + perp.x * offset, to.y + perp.y * offset),
            width: 1.8,
            color,
        });
    }
}

/// Renders a reaction equation.
#[must_use]
pub fn reaction_prims(reaction: &Reaction, theme: &Theme) -> Vec<Prim> {
    let mut prims = vec![];
    let size = theme.font;
    let baseline = reaction.origin.y;
    let mut x = reaction.origin.x;
    let mut pending_plus = false;
    for species in &reaction.reactants {
        if pending_plus {
            x = plus_prims(&mut prims, x, baseline, size, theme);
        }
        x = species_prims(species, x, baseline, size, theme, &mut prims);
        pending_plus = true;
    }
    x += size * 0.5;
    x = arrow_prims(&reaction.arrow, x, baseline, size, theme, &mut prims);
    x += size * 0.5;
    pending_plus = false;
    for species in &reaction.products {
        if pending_plus {
            x = plus_prims(&mut prims, x, baseline, size, theme);
        }
        x = species_prims(species, x, baseline, size, theme, &mut prims);
        pending_plus = true;
    }
    prims
}

fn plus_prims(out: &mut Vec<Prim>, x: f32, baseline: f32, size: f32, theme: &Theme) -> f32 {
    let gap = size * 0.45;
    let text = "+";
    out.push(Prim::Text {
        pos: Point::new(x + gap, baseline),
        text: text.into(),
        size,
        color: theme.ink,
        align: Align::Left,
        italic: false,
    });
    x + gap * 2.0 + text_width(text, size)
}

fn species_prims(
    species: &Species,
    mut x: f32,
    baseline: f32,
    size: f32,
    theme: &Theme,
    out: &mut Vec<Prim>,
) -> f32 {
    if species.coefficient != 1 {
        let text = species.coefficient.to_string();
        out.push(Prim::Text {
            pos: Point::new(x, baseline),
            text: text.clone(),
            size,
            color: theme.ink,
            align: Align::Left,
            italic: false,
        });
        x += text_width(&text, size) + size * 0.08;
    }
    x = parts_prims(&species.parts, x, baseline, size, theme, out);
    if species.charge != 0 {
        let text = charge_text(species.charge);
        out.push(Prim::Text {
            pos: Point::new(x, baseline - size * 0.36),
            text: text.clone(),
            size: size * 0.7,
            color: theme.ink,
            align: Align::Left,
            italic: false,
        });
        x += text_width(&text, size * 0.7);
    }
    if species.state != State::None {
        let text = format!("({})", species.state.code());
        out.push(Prim::Text {
            pos: Point::new(x, baseline),
            text: text.clone(),
            size,
            color: theme.ink,
            align: Align::Left,
            italic: false,
        });
        x += text_width(&text, size);
    }
    if species.marker != Marker::None {
        let text = if species.marker == Marker::Gas {
            "↑"
        } else {
            "↓"
        };
        out.push(Prim::Text {
            pos: Point::new(x + size * 0.1, baseline),
            text: text.into(),
            size,
            color: theme.ink,
            align: Align::Left,
            italic: false,
        });
        x += text_width("↑", size) + size * 0.1;
    }
    x
}

fn parts_prims(
    parts: &[Part],
    mut x: f32,
    baseline: f32,
    size: f32,
    theme: &Theme,
    out: &mut Vec<Prim>,
) -> f32 {
    for part in parts {
        match part {
            Part::Element {
                symbol,
                count,
                isotope,
            } => {
                let color = element_color(symbol, theme);
                if let Some(mass) = isotope {
                    let text = mass.to_string();
                    out.push(Prim::Text {
                        pos: Point::new(x, baseline - size * 0.36),
                        text: text.clone(),
                        size: size * 0.7,
                        color,
                        align: Align::Left,
                        italic: false,
                    });
                    x += text_width(&text, size * 0.7);
                }
                out.push(Prim::Text {
                    pos: Point::new(x, baseline),
                    text: symbol.clone(),
                    size,
                    color,
                    align: Align::Left,
                    italic: false,
                });
                x += text_width(symbol, size);
                if *count > 1 {
                    let text = count.to_string();
                    out.push(Prim::Text {
                        pos: Point::new(x, baseline + size * 0.30),
                        text: text.clone(),
                        size: size * 0.7,
                        color,
                        align: Align::Left,
                        italic: false,
                    });
                    x += text_width(&text, size * 0.7);
                }
            }
            Part::Group { parts, count } => {
                out.push(Prim::Text {
                    pos: Point::new(x, baseline),
                    text: "(".into(),
                    size,
                    color: theme.ink,
                    align: Align::Left,
                    italic: false,
                });
                x += text_width("(", size);
                x = parts_prims(parts, x, baseline, size, theme, out);
                out.push(Prim::Text {
                    pos: Point::new(x, baseline),
                    text: ")".into(),
                    size,
                    color: theme.ink,
                    align: Align::Left,
                    italic: false,
                });
                x += text_width(")", size);
                if *count > 1 {
                    let text = count.to_string();
                    out.push(Prim::Text {
                        pos: Point::new(x, baseline + size * 0.30),
                        text: text.clone(),
                        size: size * 0.7,
                        color: theme.ink,
                        align: Align::Left,
                        italic: false,
                    });
                    x += text_width(&text, size * 0.7);
                }
            }
            Part::Dot => {
                out.push(Prim::Text {
                    pos: Point::new(x, baseline),
                    text: "·".into(),
                    size,
                    color: theme.ink,
                    align: Align::Left,
                    italic: false,
                });
                x += text_width("·", size);
            }
        }
    }
    x
}

fn element_color(symbol: &str, theme: &Theme) -> [u8; 3] {
    if theme.colored_atoms {
        if let Some(info) = element::by_symbol(symbol) {
            let color = info.color();
            if color.iter().any(|channel| *channel < 210) {
                return color;
            }
        }
    }
    theme.ink
}

fn arrow_prims(
    arrow: &Arrow,
    x: f32,
    baseline: f32,
    size: f32,
    theme: &Theme,
    out: &mut Vec<Prim>,
) -> f32 {
    let above = arrow.above.join(", ");
    let below = arrow.below.join(", ");
    let condition = text_width(&above, size)
        .max(text_width(&below, size))
        .max(0.0);
    let length = (size * 2.4).max(condition + size * 0.8);
    let shaft_gap = if arrow.kind.shafts() == 2 { 2.5 } else { 0.0 };
    let offset = arrow_head_size(size);
    let left = if arrow.kind.left_head() {
        x + offset
    } else {
        x
    };
    let right = if arrow.kind.right_head() {
        x + length - offset
    } else {
        x + length
    };
    for side in if arrow.kind.shafts() == 2 {
        vec![-shaft_gap, shaft_gap]
    } else {
        vec![0.0]
    } {
        out.push(Prim::Line {
            from: Point::new(left, baseline + side),
            to: Point::new(right, baseline + side),
            width: 1.8,
            color: theme.ink,
        });
    }
    let head = arrow_head_size(size);
    if arrow.kind.right_head() {
        out.push(arrowhead(
            Point::new(x + length, baseline),
            Point::new(-1.0, 0.0),
            head,
            theme.ink,
        ));
    }
    if arrow.kind.left_head() {
        out.push(arrowhead(
            Point::new(x, baseline),
            Point::new(1.0, 0.0),
            head,
            theme.ink,
        ));
    }
    let center = x + length / 2.0;
    if !above.is_empty() {
        out.push(Prim::Text {
            pos: Point::new(center, baseline - size * 0.95),
            text: above,
            size,
            color: theme.ink,
            align: Align::Center,
            italic: false,
        });
    }
    if !below.is_empty() {
        out.push(Prim::Text {
            pos: Point::new(center, baseline + size * 0.95),
            text: below,
            size,
            color: theme.ink,
            align: Align::Center,
            italic: false,
        });
    }
    x + length
}

fn arrow_head_size(size: f32) -> f32 {
    size * 0.55
}

fn arrowhead(tip: Point, direction: Point, size: f32, color: [u8; 3]) -> Prim {
    let perp = Point::new(-direction.y, direction.x);
    Prim::Polygon {
        points: vec![
            tip,
            Point::new(
                tip.x + direction.x * size + perp.x * size * 0.5,
                tip.y + direction.y * size + perp.y * size * 0.5,
            ),
            Point::new(
                tip.x + direction.x * size - perp.x * size * 0.5,
                tip.y + direction.y * size - perp.y * size * 0.5,
            ),
        ],
        fill: color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Atom, Bond, BondOrder, BondStereo};

    fn ethanol() -> Structure {
        Structure {
            id: 1,
            origin: Point::default(),
            atoms: vec![
                Atom {
                    id: 1,
                    element: 6,
                    position: Point::new(0.0, 0.0),
                    charge: 0,
                    hydrogens: None,
                },
                Atom {
                    id: 2,
                    element: 8,
                    position: Point::new(44.0, 0.0),
                    charge: 0,
                    hydrogens: None,
                },
            ],
            bonds: vec![Bond {
                id: 3,
                a: 1,
                b: 2,
                order: BondOrder::Single,
                stereo: BondStereo::Plain,
            }],
        }
    }

    #[test]
    fn a_structure_becomes_lines_and_a_label() {
        let theme = Theme::default();
        let prims = structure_prims(&ethanol(), &theme, false);
        assert!(prims.iter().any(|p| matches!(p, Prim::Line { .. })));
        assert!(prims
            .iter()
            .any(|p| matches!(p, Prim::Text { text, .. } if text == "OH")));
        assert!(prims_bounds(&prims).is_some());
    }

    #[test]
    fn a_reaction_becomes_an_arrow() {
        let reaction = crate::mhchem::parse("2H2 + O2 ->[heat] 2H2O").unwrap();
        let prims = reaction_prims(&reaction, &Theme::default());
        assert!(prims.iter().any(|p| matches!(p, Prim::Polygon { .. })));
        assert!(prims
            .iter()
            .any(|p| matches!(p, Prim::Text { text, .. } if text == "heat")));
    }
}
