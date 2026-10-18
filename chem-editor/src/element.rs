//! The periodic table: symbol, English name, category, mean atomic mass and a
//! CPK-style colour for every element. Data-only, no chemistry is computed
//! here beyond lookups.

/// Broad periodic-table family, used to colour the palette and drawings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Group 1 (Li, Na, ...).
    AlkaliMetal,
    /// Group 2 (Be, Mg, ...).
    AlkalineEarth,
    /// d-block metals.
    TransitionMetal,
    /// p-block metals as usually taught.
    PostTransitionMetal,
    /// Metalloids.
    Metalloid,
    /// Reactive nonmetals.
    Nonmetal,
    /// Group 17 (F, Cl, ...).
    Halogen,
    /// Group 18 (He, Ne, ...).
    NobleGas,
    /// f-block lanthanoids.
    Lanthanide,
    /// f-block actinoids.
    Actinide,
}

impl Category {
    /// Default ink colour when an element has no dedicated CPK colour.
    #[must_use]
    pub const fn color(self) -> [u8; 3] {
        match self {
            Self::AlkaliMetal => [0xAB, 0x5C, 0xF2],
            Self::AlkalineEarth => [0x3D, 0xFF, 0x00],
            Self::TransitionMetal => [0xC0, 0xC0, 0xC0],
            Self::PostTransitionMetal => [0xA0, 0xA0, 0xA8],
            Self::Metalloid => [0xCC, 0xCC, 0xA0],
            Self::Nonmetal => [0x30, 0x30, 0x30],
            Self::Halogen => [0x30, 0xA0, 0x30],
            Self::NobleGas => [0x70, 0xC8, 0xF0],
            Self::Lanthanide => [0xB0, 0x90, 0xC0],
            Self::Actinide => [0xD0, 0xA0, 0xB0],
        }
    }
}

/// One element of the periodic table.
#[derive(Clone, Copy, Debug)]
pub struct ElementInfo {
    /// Atomic number, 1..=118.
    pub z: u8,
    /// Chemical symbol as written (`H`, `He`, `Na`).
    pub symbol: &'static str,
    /// English name.
    pub name: &'static str,
    /// Periodic-table family.
    pub category: Category,
    /// Standard atomic weight (mean or best mass number for unstable elements).
    pub mass: f32,
}

impl ElementInfo {
    /// Drawing colour: a CPK value for common atoms, category colour otherwise.
    #[must_use]
    pub const fn color(&self) -> [u8; 3] {
        match self.z {
            1 | 6 => [0x22, 0x22, 0x22],
            7 => [0x30, 0x50, 0xF8],
            8 => [0xFF, 0x0D, 0x0D],
            9 => [0x90, 0xE0, 0x50],
            15 => [0xFF, 0x80, 0x00],
            16 => [0xE8, 0xC0, 0x00],
            17 => [0x1F, 0xF0, 0x1F],
            26 => [0xE0, 0x66, 0x33],
            29 => [0xC8, 0x80, 0x33],
            35 => [0xA6, 0x29, 0x29],
            53 => [0x94, 0x00, 0x94],
            _ => self.category.color(),
        }
    }

    /// Implicit valence used to count missing hydrogens in a structure.
    ///
    /// This is a small teaching-table, not a full valence model; uncommon
    /// oxidation states and multi-valence atoms are approximated.
    #[must_use]
    pub fn valence(&self) -> u8 {
        match self.symbol {
            "H" | "F" | "Cl" | "Br" | "I" | "Li" | "Na" | "K" | "Rb" | "Cs" | "Fr" => 1,
            "Be" | "Mg" | "Ca" | "Sr" | "Ba" | "Ra" | "Zn" | "Cd" | "Hg" | "O" | "S" | "Se" => 2,
            "B" | "Al" | "Ga" | "In" | "Tl" | "N" | "P" | "As" => 3,
            "C" | "Si" | "Ge" | "Sn" | "Pb" => 4,
            _ => 0,
        }
    }
}

const fn e(
    z: u8,
    symbol: &'static str,
    name: &'static str,
    category: Category,
    mass: f32,
) -> ElementInfo {
    ElementInfo {
        z,
        symbol,
        name,
        category,
        mass,
    }
}

use Category::{
    Actinide, AlkaliMetal, AlkalineEarth, Halogen, Lanthanide, Metalloid, NobleGas, Nonmetal,
    PostTransitionMetal, TransitionMetal,
};

/// The whole periodic table, ordered by atomic number.
pub const ELEMENTS: [ElementInfo; 118] = [
    e(1, "H", "Hydrogen", Nonmetal, 1.008),
    e(2, "He", "Helium", NobleGas, 4.003),
    e(3, "Li", "Lithium", AlkaliMetal, 6.94),
    e(4, "Be", "Beryllium", AlkalineEarth, 9.012),
    e(5, "B", "Boron", Metalloid, 10.81),
    e(6, "C", "Carbon", Nonmetal, 12.011),
    e(7, "N", "Nitrogen", Nonmetal, 14.007),
    e(8, "O", "Oxygen", Nonmetal, 15.999),
    e(9, "F", "Fluorine", Halogen, 18.998),
    e(10, "Ne", "Neon", NobleGas, 20.180),
    e(11, "Na", "Sodium", AlkaliMetal, 22.990),
    e(12, "Mg", "Magnesium", AlkalineEarth, 24.305),
    e(13, "Al", "Aluminium", PostTransitionMetal, 26.982),
    e(14, "Si", "Silicon", Metalloid, 28.085),
    e(15, "P", "Phosphorus", Nonmetal, 30.974),
    e(16, "S", "Sulfur", Nonmetal, 32.06),
    e(17, "Cl", "Chlorine", Halogen, 35.45),
    e(18, "Ar", "Argon", NobleGas, 39.95),
    e(19, "K", "Potassium", AlkaliMetal, 39.098),
    e(20, "Ca", "Calcium", AlkalineEarth, 40.078),
    e(21, "Sc", "Scandium", TransitionMetal, 44.956),
    e(22, "Ti", "Titanium", TransitionMetal, 47.867),
    e(23, "V", "Vanadium", TransitionMetal, 50.942),
    e(24, "Cr", "Chromium", TransitionMetal, 51.996),
    e(25, "Mn", "Manganese", TransitionMetal, 54.938),
    e(26, "Fe", "Iron", TransitionMetal, 55.845),
    e(27, "Co", "Cobalt", TransitionMetal, 58.933),
    e(28, "Ni", "Nickel", TransitionMetal, 58.693),
    e(29, "Cu", "Copper", TransitionMetal, 63.546),
    e(30, "Zn", "Zinc", TransitionMetal, 65.38),
    e(31, "Ga", "Gallium", PostTransitionMetal, 69.723),
    e(32, "Ge", "Germanium", Metalloid, 72.630),
    e(33, "As", "Arsenic", Metalloid, 74.922),
    e(34, "Se", "Selenium", Nonmetal, 78.971),
    e(35, "Br", "Bromine", Halogen, 79.904),
    e(36, "Kr", "Krypton", NobleGas, 83.798),
    e(37, "Rb", "Rubidium", AlkaliMetal, 85.468),
    e(38, "Sr", "Strontium", AlkalineEarth, 87.62),
    e(39, "Y", "Yttrium", TransitionMetal, 88.906),
    e(40, "Zr", "Zirconium", TransitionMetal, 91.224),
    e(41, "Nb", "Niobium", TransitionMetal, 92.906),
    e(42, "Mo", "Molybdenum", TransitionMetal, 95.95),
    e(43, "Tc", "Technetium", TransitionMetal, 98.0),
    e(44, "Ru", "Ruthenium", TransitionMetal, 101.07),
    e(45, "Rh", "Rhodium", TransitionMetal, 102.91),
    e(46, "Pd", "Palladium", TransitionMetal, 106.42),
    e(47, "Ag", "Silver", TransitionMetal, 107.87),
    e(48, "Cd", "Cadmium", TransitionMetal, 112.41),
    e(49, "In", "Indium", PostTransitionMetal, 114.82),
    e(50, "Sn", "Tin", PostTransitionMetal, 118.71),
    e(51, "Sb", "Antimony", Metalloid, 121.76),
    e(52, "Te", "Tellurium", Metalloid, 127.60),
    e(53, "I", "Iodine", Halogen, 126.90),
    e(54, "Xe", "Xenon", NobleGas, 131.29),
    e(55, "Cs", "Caesium", AlkaliMetal, 132.91),
    e(56, "Ba", "Barium", AlkalineEarth, 137.33),
    e(57, "La", "Lanthanum", Lanthanide, 138.91),
    e(58, "Ce", "Cerium", Lanthanide, 140.12),
    e(59, "Pr", "Praseodymium", Lanthanide, 140.91),
    e(60, "Nd", "Neodymium", Lanthanide, 144.24),
    e(61, "Pm", "Promethium", Lanthanide, 145.0),
    e(62, "Sm", "Samarium", Lanthanide, 150.36),
    e(63, "Eu", "Europium", Lanthanide, 151.96),
    e(64, "Gd", "Gadolinium", Lanthanide, 157.25),
    e(65, "Tb", "Terbium", Lanthanide, 158.93),
    e(66, "Dy", "Dysprosium", Lanthanide, 162.50),
    e(67, "Ho", "Holmium", Lanthanide, 164.93),
    e(68, "Er", "Erbium", Lanthanide, 167.26),
    e(69, "Tm", "Thulium", Lanthanide, 168.93),
    e(70, "Yb", "Ytterbium", Lanthanide, 173.05),
    e(71, "Lu", "Lutetium", Lanthanide, 174.97),
    e(72, "Hf", "Hafnium", TransitionMetal, 178.49),
    e(73, "Ta", "Tantalum", TransitionMetal, 180.95),
    e(74, "W", "Tungsten", TransitionMetal, 183.84),
    e(75, "Re", "Rhenium", TransitionMetal, 186.21),
    e(76, "Os", "Osmium", TransitionMetal, 190.23),
    e(77, "Ir", "Iridium", TransitionMetal, 192.22),
    e(78, "Pt", "Platinum", TransitionMetal, 195.08),
    e(79, "Au", "Gold", TransitionMetal, 196.97),
    e(80, "Hg", "Mercury", TransitionMetal, 200.59),
    e(81, "Tl", "Thallium", PostTransitionMetal, 204.38),
    e(82, "Pb", "Lead", PostTransitionMetal, 207.2),
    e(83, "Bi", "Bismuth", PostTransitionMetal, 208.98),
    e(84, "Po", "Polonium", PostTransitionMetal, 209.0),
    e(85, "At", "Astatine", Halogen, 210.0),
    e(86, "Rn", "Radon", NobleGas, 222.0),
    e(87, "Fr", "Francium", AlkaliMetal, 223.0),
    e(88, "Ra", "Radium", AlkalineEarth, 226.0),
    e(89, "Ac", "Actinium", Actinide, 227.0),
    e(90, "Th", "Thorium", Actinide, 232.04),
    e(91, "Pa", "Protactinium", Actinide, 231.04),
    e(92, "U", "Uranium", Actinide, 238.03),
    e(93, "Np", "Neptunium", Actinide, 237.0),
    e(94, "Pu", "Plutonium", Actinide, 244.0),
    e(95, "Am", "Americium", Actinide, 243.0),
    e(96, "Cm", "Curium", Actinide, 247.0),
    e(97, "Bk", "Berkelium", Actinide, 247.0),
    e(98, "Cf", "Californium", Actinide, 251.0),
    e(99, "Es", "Einsteinium", Actinide, 252.0),
    e(100, "Fm", "Fermium", Actinide, 257.0),
    e(101, "Md", "Mendelevium", Actinide, 258.0),
    e(102, "No", "Nobelium", Actinide, 259.0),
    e(103, "Lr", "Lawrencium", Actinide, 266.0),
    e(104, "Rf", "Rutherfordium", TransitionMetal, 267.0),
    e(105, "Db", "Dubnium", TransitionMetal, 268.0),
    e(106, "Sg", "Seaborgium", TransitionMetal, 269.0),
    e(107, "Bh", "Bohrium", TransitionMetal, 270.0),
    e(108, "Hs", "Hassium", TransitionMetal, 269.0),
    e(109, "Mt", "Meitnerium", TransitionMetal, 278.0),
    e(110, "Ds", "Darmstadtium", TransitionMetal, 281.0),
    e(111, "Rg", "Roentgenium", TransitionMetal, 282.0),
    e(112, "Cn", "Copernicium", TransitionMetal, 285.0),
    e(113, "Nh", "Nihonium", PostTransitionMetal, 286.0),
    e(114, "Fl", "Flerovium", PostTransitionMetal, 289.0),
    e(115, "Mc", "Moscovium", PostTransitionMetal, 290.0),
    e(116, "Lv", "Livermorium", PostTransitionMetal, 293.0),
    e(117, "Ts", "Tennessine", Halogen, 294.0),
    e(118, "Og", "Oganesson", NobleGas, 294.0),
];

/// Element by atomic number (1-based).
#[must_use]
pub fn element(z: u8) -> Option<&'static ElementInfo> {
    ELEMENTS.get(usize::from(z).checked_sub(1)?)
}

/// Element by exact symbol, case-sensitive (`C` and `Cl` are distinct).
#[must_use]
pub fn by_symbol(symbol: &str) -> Option<&'static ElementInfo> {
    ELEMENTS.iter().find(|info| info.symbol == symbol)
}

/// Symbol for an atomic number, or `"*"` for the dummy/pseudo atom `0`.
#[must_use]
pub fn symbol(z: u8) -> &'static str {
    if z == 0 {
        "*"
    } else {
        element(z).map_or("*", |info| info.symbol)
    }
}

/// Grid cell of an element in an 18-column periodic table.
///
/// Returns `(column, row)` with columns `0..=17` and rows `0..=8`: the seven
/// main periods occupy rows `0..=6`, lanthanoids row 7 and actinoids row 8,
/// both starting at column 3. Elements are never placed on the same cell.
#[must_use]
pub const fn periodic_cell(z: u8) -> (u8, u8) {
    match z {
        2 => (17, 0),
        3..=4 => (z - 3, 1),
        5..=10 => (z - 5 + 12, 1),
        11..=12 => (z - 11, 2),
        13..=18 => (z - 13 + 12, 2),
        19..=36 => (z - 19, 3),
        37..=54 => (z - 37, 4),
        55..=56 => (z - 55, 5),
        57..=71 => (z - 57 + 3, 7),
        72..=86 => (z - 72 + 3, 5),
        87..=88 => (z - 87, 6),
        89..=103 => (z - 89 + 3, 8),
        104..=118 => (z - 104 + 3, 6),
        _ => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_consistent() {
        assert_eq!(ELEMENTS.len(), 118);
        for (index, info) in ELEMENTS.iter().enumerate() {
            assert_eq!(usize::from(info.z), index + 1, "{}", info.symbol);
            assert_eq!(by_symbol(info.symbol).map(|i| i.z), Some(info.z));
            assert_eq!(element(info.z).map(|i| i.symbol), Some(info.symbol));
            assert!(info.mass > 0.0);
        }
        assert_eq!(symbol(0), "*");
        assert_eq!(by_symbol("Cl").map(|i| i.z), Some(17));
        assert!(by_symbol("cl").is_none());
    }

    #[test]
    fn periodic_cells_are_inside_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for info in &ELEMENTS {
            let (column, row) = periodic_cell(info.z);
            assert!(column < 18 && row < 9, "{} at {column},{row}", info.symbol);
            assert!(seen.insert((column, row)), "{} overlaps", info.symbol);
        }
        assert_eq!(periodic_cell(1), (0, 0));
        assert_eq!(periodic_cell(57), (3, 7));
    }
}
