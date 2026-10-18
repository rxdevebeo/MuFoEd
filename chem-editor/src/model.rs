//! The editable chemistry document: 2D structures and reaction equations.
//!
//! The document is the portable, human-readable source of truth. JSON is the
//! editable format; SVG and DOCX are derived exports. Coordinates are logical
//! pixels at 96 DPI, stable IDs are `u64`, and every reference is by ID.

#![allow(clippy::many_single_char_names)]

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::element;

/// Document schema version.
pub const VERSION: u32 = 1;
/// Maximum number of top-level objects.
pub const MAX_OBJECTS: usize = 1000;
/// Maximum atoms across all structures.
pub const MAX_ATOMS: usize = 5000;
/// Maximum bonds across all structures.
pub const MAX_BONDS: usize = 5000;
/// Maximum species across all reactions.
pub const MAX_SPECIES: usize = 2000;
/// Largest accepted JSON scene, in bytes.
pub const MAX_JSON: usize = 8 * 1024 * 1024;

/// Logical pixel coordinates, independent of zoom and pan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl Point {
    /// Constructs a point.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Component-wise sum.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }

    /// Component-wise difference.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }

    /// Uniform scale.
    #[must_use]
    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }

    /// Euclidean length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    /// Distance to another point.
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        self.sub(other).length()
    }
}

/// Bond order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondOrder {
    /// One line.
    #[default]
    Single,
    /// Two parallel lines.
    Double,
    /// Three parallel lines.
    Triple,
    /// Aromatic ring bond.
    Aromatic,
}

impl BondOrder {
    /// Number of parallel lines this order is drawn with.
    #[must_use]
    pub const fn lines(self) -> u8 {
        match self {
            Self::Single | Self::Aromatic => 1,
            Self::Double => 2,
            Self::Triple => 3,
        }
    }
}

/// Bond stereo decoration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondStereo {
    /// Plain line.
    #[default]
    Plain,
    /// Solid wedge (towards the viewer).
    Wedge,
    /// Hashed wedge (away from the viewer).
    Hash,
}

/// One atom of a structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Atom {
    /// Stable document ID.
    pub id: u64,
    /// Atomic number 1..=118; `0` is the dummy atom `*`.
    pub element: u8,
    /// Position relative to the structure origin.
    pub position: Point,
    /// Formal charge.
    #[serde(default)]
    pub charge: i8,
    /// Explicit hydrogen count; `None` means "use the implicit count".
    #[serde(default)]
    pub hydrogens: Option<u8>,
}

impl Atom {
    /// Symbol shown at the atom.
    #[must_use]
    pub fn symbol(&self) -> &'static str {
        element::symbol(self.element)
    }
}

/// One bond between two atoms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bond {
    /// Stable document ID.
    pub id: u64,
    /// First endpoint atom ID.
    pub a: u64,
    /// Second endpoint atom ID.
    pub b: u64,
    /// Bond order.
    #[serde(default)]
    pub order: BondOrder,
    /// Stereo decoration.
    #[serde(default)]
    pub stereo: BondStereo,
}

/// A 2D structural formula: atoms and the bonds between them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    /// Stable document ID.
    pub id: u64,
    /// Scene position of the structure's local origin.
    #[serde(default)]
    pub origin: Point,
    /// Atoms of the structure.
    #[serde(default)]
    pub atoms: Vec<Atom>,
    /// Bonds of the structure.
    #[serde(default)]
    pub bonds: Vec<Bond>,
}

impl Structure {
    /// Atom by stable ID.
    #[must_use]
    pub fn atom(&self, id: u64) -> Option<&Atom> {
        self.atoms.iter().find(|atom| atom.id == id)
    }

    /// All bonds attached to `id`.
    pub fn bonds_at(&self, id: u64) -> impl Iterator<Item = &Bond> {
        self.bonds
            .iter()
            .filter(move |bond| bond.a == id || bond.b == id)
    }

    /// Axis-aligned bounds of the atoms, in scene coordinates.
    #[must_use]
    pub fn bounds(&self) -> (Point, Point) {
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for atom in &self.atoms {
            let p = atom.position.add(self.origin);
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        if self.atoms.is_empty() {
            (self.origin, self.origin)
        } else {
            (min, max)
        }
    }

    /// Implicit hydrogen count for an atom, honouring the explicit override.
    #[must_use]
    pub fn hydrogens(&self, atom: &Atom) -> u8 {
        if let Some(count) = atom.hydrogens {
            return count;
        }
        let Some(info) = element::element(atom.element) else {
            return 0;
        };
        let used: u8 = self.bonds_at(atom.id).map(|bond| bond.order.lines()).sum();
        info.valence().saturating_sub(used)
    }
}

/// One drawn fragment of a species.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    /// An element symbol with a subscript count.
    Element {
        /// Chemical symbol, or `e` for an electron.
        symbol: String,
        /// Subscript count; `1` is not drawn.
        #[serde(default = "one")]
        count: u32,
        /// Mass number drawn as a prescript superscript (isotope), if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        isotope: Option<u16>,
    },
    /// A parenthesised group with a subscript count.
    Group {
        /// Contents of the group.
        parts: Vec<Part>,
        /// Subscript count; `1` is not drawn.
        #[serde(default = "one")]
        count: u32,
    },
    /// A hydrate dot, joining two fragments inside one species.
    Dot,
}

const fn one() -> u32 {
    1
}

/// Aqueous state annotation after a species.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// No annotation.
    #[default]
    None,
    /// `(s)`.
    Solid,
    /// `(l)`.
    Liquid,
    /// `(g)`.
    Gas,
    /// `(aq)`.
    Aqueous,
}

impl State {
    /// The text drawn after the species, without the parentheses.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Solid => "s",
            Self::Liquid => "l",
            Self::Gas => "g",
            Self::Aqueous => "aq",
        }
    }
}

/// Gas / precipitate marker drawn after a species.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marker {
    /// None.
    #[default]
    None,
    /// Upward arrow, a gas.
    Gas,
    /// Downward arrow, a precipitate.
    Precipitate,
}

/// One species in a reaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Species {
    /// Stoichiometric coefficient; `1` is not drawn.
    #[serde(default = "one")]
    pub coefficient: u32,
    /// Drawn fragments, in order.
    pub parts: Vec<Part>,
    /// Formal charge.
    #[serde(default)]
    pub charge: i8,
    /// Phase annotation.
    #[serde(default)]
    pub state: State,
    /// Gas / precipitate marker.
    #[serde(default)]
    pub marker: Marker,
}

/// Reaction arrow style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowKind {
    /// `->`
    #[default]
    Right,
    /// `<-`
    Left,
    /// `<->`
    LeftRight,
    /// `<=>` (equilibrium)
    Equilibrium,
    /// `=>`
    Double,
    /// `<=` (reverse equilibrium)
    ReverseEquilibrium,
}

impl ArrowKind {
    /// Whether the arrow has an arrowhead at the right end.
    #[must_use]
    pub const fn right_head(self) -> bool {
        matches!(
            self,
            Self::Right
                | Self::LeftRight
                | Self::Equilibrium
                | Self::Double
                | Self::ReverseEquilibrium
        )
    }

    /// Whether the arrow has an arrowhead at the left end.
    #[must_use]
    pub const fn left_head(self) -> bool {
        matches!(
            self,
            Self::Left | Self::LeftRight | Self::Equilibrium | Self::ReverseEquilibrium
        )
    }

    /// Number of parallel shafts (2 for equilibrium / double arrows).
    #[must_use]
    pub const fn shafts(self) -> u8 {
        match self {
            Self::Equilibrium | Self::ReverseEquilibrium | Self::Double => 2,
            _ => 1,
        }
    }
}

/// A reaction arrow with conditions above and below the shaft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arrow {
    /// Arrow style.
    #[serde(default)]
    pub kind: ArrowKind,
    /// Conditions drawn above the arrow.
    #[serde(default)]
    pub above: Vec<String>,
    /// Conditions drawn below the arrow.
    #[serde(default)]
    pub below: Vec<String>,
}

impl Default for Arrow {
    fn default() -> Self {
        Self {
            kind: ArrowKind::Right,
            above: vec![],
            below: vec![],
        }
    }
}

/// A reaction equation: reactants, an arrow and products.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reaction {
    /// Stable document ID.
    pub id: u64,
    /// Scene position of the reaction's local origin.
    #[serde(default)]
    pub origin: Point,
    /// Species on the left of the arrow.
    pub reactants: Vec<Species>,
    /// The arrow and its conditions.
    #[serde(default)]
    pub arrow: Arrow,
    /// Species on the right of the arrow.
    pub products: Vec<Species>,
}

/// A top-level document object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChemObject {
    /// A structural formula.
    Structure(Structure),
    /// A reaction equation.
    Reaction(Reaction),
}

impl ChemObject {
    /// Stable object ID.
    #[must_use]
    pub const fn id(&self) -> u64 {
        match self {
            Self::Structure(s) => s.id,
            Self::Reaction(r) => r.id,
        }
    }

    /// Scene origin.
    #[must_use]
    pub const fn origin(&self) -> Point {
        match self {
            Self::Structure(s) => s.origin,
            Self::Reaction(r) => r.origin,
        }
    }

    /// Mutable scene origin.
    pub fn origin_mut(&mut self) -> &mut Point {
        match self {
            Self::Structure(s) => &mut s.origin,
            Self::Reaction(r) => &mut r.origin,
        }
    }
}

/// Portable editable chemistry document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChemDocument {
    /// Schema version, currently 1.
    pub version: u32,
    /// Top-level objects in paint order.
    #[serde(default)]
    pub objects: Vec<ChemObject>,
}

impl Default for ChemDocument {
    fn default() -> Self {
        Self {
            version: VERSION,
            objects: vec![],
        }
    }
}

impl ChemDocument {
    /// Object by stable ID.
    #[must_use]
    pub fn object(&self, id: u64) -> Option<&ChemObject> {
        self.objects.iter().find(|object| object.id() == id)
    }

    /// Validates versions, IDs, references and resource/coordinate limits.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic for an unsupported version, a duplicate ID, a
    /// dangling bond or an exceeded limit.
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("Unsupported document version".into());
        }
        if self.objects.len() > MAX_OBJECTS {
            return Err("Document object limit exceeded".into());
        }
        let mut ids: HashSet<u64> = HashSet::new();
        let mut atoms = 0_usize;
        let mut bonds = 0_usize;
        let mut species = 0_usize;
        for object in &self.objects {
            if object.id() == 0 || object.id() > 1_000_000_000 || !ids.insert(object.id()) {
                return Err("Invalid or duplicate object ID".into());
            }
            if !finite(object.origin()) {
                return Err("Object origin is out of range".into());
            }
            match object {
                ChemObject::Structure(structure) => {
                    atoms += structure.atoms.len();
                    bonds += structure.bonds.len();
                    validate_structure(structure, &mut ids)?;
                }
                ChemObject::Reaction(reaction) => {
                    species += reaction.reactants.len() + reaction.products.len();
                    validate_reaction(reaction)?;
                }
            }
        }
        if atoms > MAX_ATOMS || bonds > MAX_BONDS || species > MAX_SPECIES {
            return Err("Document content limit exceeded".into());
        }
        Ok(())
    }

    /// Loads bounded JSON and validates the whole document before returning it.
    ///
    /// # Errors
    ///
    /// Returns an encoding, schema, reference or resource-limit diagnostic.
    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > MAX_JSON {
            return Err("Chemistry file is too large".into());
        }
        let document: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        document.validate()?;
        Ok(document)
    }

    /// Serializes the editable source after validation.
    ///
    /// # Errors
    ///
    /// Returns a validation or serialization error.
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|error| error.to_string())
    }
}

fn validate_structure(structure: &Structure, ids: &mut HashSet<u64>) -> Result<(), String> {
    for atom in &structure.atoms {
        if atom.id == 0
            || atom.id > 1_000_000_000
            || !ids.insert(atom.id)
            || atom.element > 118
            || !finite(atom.position)
            || !(-20..=20).contains(&atom.charge)
            || atom.hydrogens.is_some_and(|h| h > 12)
        {
            return Err("Invalid atom".into());
        }
    }
    let atom_ids: HashSet<u64> = structure.atoms.iter().map(|atom| atom.id).collect();
    for bond in &structure.bonds {
        if bond.id == 0
            || bond.id > 1_000_000_000
            || !ids.insert(bond.id)
            || bond.a == bond.b
            || !atom_ids.contains(&bond.a)
            || !atom_ids.contains(&bond.b)
        {
            return Err("Invalid bond".into());
        }
    }
    for (index, first) in structure.bonds.iter().enumerate() {
        for second in &structure.bonds[index + 1..] {
            let same = (first.a == second.a && first.b == second.b)
                || (first.a == second.b && first.b == second.a);
            if same {
                return Err("Duplicate bond".into());
            }
        }
    }
    Ok(())
}

fn validate_reaction(reaction: &Reaction) -> Result<(), String> {
    if reaction.reactants.is_empty() || reaction.products.is_empty() {
        return Err("A reaction needs reactants and products".into());
    }
    for species in reaction.reactants.iter().chain(&reaction.products) {
        if species.coefficient == 0
            || species.coefficient > 100_000
            || species.parts.is_empty()
            || species.parts.len() > 128
            || !(-20..=20).contains(&species.charge)
        {
            return Err("Invalid species".into());
        }
        validate_parts(&species.parts, 0)?;
    }
    for text in reaction.arrow.above.iter().chain(&reaction.arrow.below) {
        if text.len() > 256 || text.chars().any(char::is_control) {
            return Err("Invalid arrow condition".into());
        }
    }
    Ok(())
}

fn validate_parts(parts: &[Part], depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err("Species groups are nested too deeply".into());
    }
    for part in parts {
        match part {
            Part::Element {
                symbol,
                count,
                isotope,
            } => {
                if symbol.is_empty()
                    || symbol.len() > 8
                    || *count == 0
                    || *count > 100_000
                    || isotope.is_some_and(|mass| mass == 0 || mass > 300)
                {
                    return Err("Invalid element fragment".into());
                }
            }
            Part::Group { parts, count } => {
                if parts.is_empty() || *count == 0 || *count > 100_000 {
                    return Err("Invalid group fragment".into());
                }
                validate_parts(parts, depth + 1)?;
            }
            Part::Dot => {}
        }
    }
    Ok(())
}

fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 100_000.0 && p.y.abs() <= 100_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ChemDocument {
        ChemDocument {
            version: VERSION,
            objects: vec![ChemObject::Structure(Structure {
                id: 1,
                origin: Point::new(10.0, 20.0),
                atoms: vec![
                    Atom {
                        id: 2,
                        element: 6,
                        position: Point::new(0.0, 0.0),
                        charge: 0,
                        hydrogens: None,
                    },
                    Atom {
                        id: 3,
                        element: 8,
                        position: Point::new(42.0, 0.0),
                        charge: 0,
                        hydrogens: None,
                    },
                ],
                bonds: vec![Bond {
                    id: 4,
                    a: 2,
                    b: 3,
                    order: BondOrder::Double,
                    stereo: BondStereo::Plain,
                }],
            })],
        }
    }

    #[test]
    fn json_round_trips() {
        let document = sample();
        let json = document.to_json().unwrap();
        let again = ChemDocument::from_json(&json).unwrap();
        assert_eq!(document, again);
    }

    #[test]
    fn invalid_documents_are_rejected() {
        let mut dangling = sample();
        if let ChemObject::Structure(structure) = &mut dangling.objects[0] {
            structure.bonds[0].b = 999;
        }
        assert!(dangling.validate().is_err());
        let mut duplicate = sample();
        if let ChemObject::Structure(structure) = &mut duplicate.objects[0] {
            let clone = structure.atoms[0].clone();
            structure.atoms.push(clone);
        }
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn hydrogens_follow_valence() {
        let document = sample();
        let ChemObject::Structure(structure) = &document.objects[0] else {
            panic!("sample is a structure");
        };
        let carbon = structure.atom(2).unwrap();
        assert_eq!(structure.hydrogens(carbon), 2, "carbon with a double bond");
    }
}
