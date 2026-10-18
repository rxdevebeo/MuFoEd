//! Derived chemistry of a drawn structure: Hill molecular formula, molar mass
//! and a Kekulé SMILES string. These are conveniences over the model, not a
//! general cheminformatics toolkit.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::element;
use crate::model::{BondOrder, Structure};

impl Structure {
    /// Molecular formula in Hill notation, including implicit hydrogens.
    #[must_use]
    pub fn molecular_formula(&self) -> String {
        let counts = self.element_counts();
        let mut symbols: Vec<&str> = counts.keys().map(String::as_str).collect();
        let has_carbon = counts.contains_key("C");
        symbols.sort_by(|a, b| {
            let rank = |symbol: &str| {
                if has_carbon && symbol == "C" {
                    0
                } else if has_carbon && symbol == "H" {
                    1
                } else {
                    2
                }
            };
            rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
        });
        let mut out = String::new();
        for symbol in symbols {
            out.push_str(symbol);
            let count = counts[symbol];
            if count > 1 {
                out.push_str(&count.to_string());
            }
        }
        out
    }

    /// Total counts of each element symbol, implicit hydrogens included.
    #[must_use]
    pub fn element_counts(&self) -> BTreeMap<String, u32> {
        let mut counts = BTreeMap::new();
        for atom in &self.atoms {
            let symbol = if atom.element == 0 {
                "*".to_owned()
            } else {
                element::symbol(atom.element).to_owned()
            };
            *counts.entry(symbol).or_insert(0) += 1;
            let hydrogens = u32::from(self.hydrogens(atom));
            if hydrogens > 0 {
                *counts.entry("H".to_owned()).or_insert(0) += hydrogens;
            }
        }
        counts
    }

    /// Mean molar mass in g/mol.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn molar_mass(&self) -> f32 {
        let mut mass = 0.0_f32;
        for atom in &self.atoms {
            if let Some(info) = element::element(atom.element) {
                mass += info.mass;
            }
            mass += f32::from(self.hydrogens(atom)) * 1.008;
        }
        mass
    }

    /// Deterministic SMILES for a Kekulé structure.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when any bond is aromatic; the model stores Kekulé
    /// orders only, so an aromatic drawing must be redrawn as alternating
    /// single and double bonds before exporting.
    pub fn smiles(&self) -> Result<String, String> {
        if self
            .bonds
            .iter()
            .any(|bond| bond.order == BondOrder::Aromatic)
        {
            return Err("SMILES export needs Kekulé bonds, not aromatic bonds".into());
        }
        let count = self.atoms.len();
        if count == 0 {
            return Ok(String::new());
        }
        let index_of: BTreeMap<u64, usize> = self
            .atoms
            .iter()
            .enumerate()
            .map(|(index, atom)| (atom.id, index))
            .collect();
        let edges: Vec<(usize, usize, BondOrder)> = self
            .bonds
            .iter()
            .filter_map(|bond| Some((*index_of.get(&bond.a)?, *index_of.get(&bond.b)?, bond.order)))
            .collect();
        let mut adjacency: Vec<Vec<(usize, usize)>> = vec![vec![]; count];
        for (edge, &(a, b, _)) in edges.iter().enumerate() {
            adjacency[a].push((edge, b));
            adjacency[b].push((edge, a));
        }
        let mut visited = vec![false; count];
        let mut tree = vec![false; edges.len()];
        let mut ring = vec![None; edges.len()];
        let mut next_ring = 1_usize;
        visited[0] = true;
        let mut stack = vec![(0_usize, usize::MAX)];
        while let Some((node, parent)) = stack.pop() {
            for &(edge, neighbour) in &adjacency[node] {
                if edge == parent {
                    continue;
                }
                if visited[neighbour] {
                    if ring[edge].is_none() {
                        ring[edge] = Some(next_ring);
                        next_ring += 1;
                    }
                } else {
                    visited[neighbour] = true;
                    tree[edge] = true;
                    stack.push((neighbour, edge));
                }
            }
        }
        let mut out = String::new();
        let mut closure_emitted = vec![0_u8; next_ring + 1];
        let mut emitted = vec![false; count];
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|&index| {
            let degree = adjacency[index].len();
            (degree, self.atoms[index].id)
        });
        for &start in &order {
            if emitted[start] {
                continue;
            }
            emitted[start] = true;
            walk(
                self,
                start,
                usize::MAX,
                &adjacency,
                &tree,
                &ring,
                &mut closure_emitted,
                &mut emitted,
                &mut out,
            );
        }
        Ok(out)
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    structure: &Structure,
    node: usize,
    parent: usize,
    adjacency: &[Vec<(usize, usize)>],
    tree: &[bool],
    ring: &[Option<usize>],
    closure_emitted: &mut [u8],
    emitted: &mut [bool],
    out: &mut String,
) {
    out.push_str(&atom_token(structure, node));
    for &(edge, _) in &adjacency[node] {
        if let Some(number) = ring[edge] {
            let seen = &mut closure_emitted[number];
            if *seen == 1 {
                out.push_str(bond_token(structure, edge));
                out.push_str(&ring_number(number));
            } else {
                out.push_str(&ring_number(number));
            }
            *seen += 1;
        }
    }
    for &(edge, neighbour) in &adjacency[node] {
        if !tree[edge] || neighbour == parent || emitted[neighbour] {
            continue;
        }
        emitted[neighbour] = true;
        out.push_str(bond_token(structure, edge));
        walk(
            structure,
            neighbour,
            node,
            adjacency,
            tree,
            ring,
            closure_emitted,
            emitted,
            out,
        );
    }
}

fn bond_token(structure: &Structure, edge: usize) -> &'static str {
    match structure
        .bonds
        .get(edge)
        .map_or(BondOrder::Single, |bond| bond.order)
    {
        BondOrder::Single | BondOrder::Aromatic => "",
        BondOrder::Double => "=",
        BondOrder::Triple => "#",
    }
}

fn ring_number(number: usize) -> String {
    if number < 10 {
        number.to_string()
    } else {
        format!("%{number}")
    }
}

fn atom_token(structure: &Structure, index: usize) -> String {
    let Some(atom) = structure.atoms.get(index) else {
        return "*".into();
    };
    let symbol = if atom.element == 0 {
        "*"
    } else {
        element::symbol(atom.element)
    };
    let organic = matches!(
        symbol,
        "B" | "C" | "N" | "O" | "P" | "F" | "S" | "Cl" | "Br" | "I"
    );
    let hydrogens = atom.hydrogens.unwrap_or(0);
    if organic && atom.charge == 0 && hydrogens == 0 {
        return symbol.to_owned();
    }
    let mut out = String::from("[");
    out.push_str(symbol);
    if hydrogens > 0 {
        out.push('H');
        if hydrogens > 1 {
            out.push_str(&hydrogens.to_string());
        }
    }
    match atom.charge {
        0 => {}
        charge if charge > 0 => {
            let _ = write!(out, "+{charge}");
        }
        charge => {
            let _ = write!(out, "{charge}");
        }
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Atom, Bond, Point};

    fn atom(id: u64, element: u8, x: f32, y: f32) -> Atom {
        Atom {
            id,
            element,
            position: Point::new(x, y),
            charge: 0,
            hydrogens: None,
        }
    }

    fn bond(id: u64, a: u64, b: u64, order: BondOrder) -> Bond {
        Bond {
            id,
            a,
            b,
            order,
            stereo: crate::model::BondStereo::Plain,
        }
    }

    #[test]
    fn ethanol_formula_and_mass() {
        let structure = Structure {
            id: 1,
            origin: Point::default(),
            atoms: vec![
                atom(1, 6, 0.0, 0.0),
                atom(2, 6, 40.0, 0.0),
                atom(3, 8, 80.0, 0.0),
            ],
            bonds: vec![
                bond(4, 1, 2, BondOrder::Single),
                bond(5, 2, 3, BondOrder::Single),
            ],
        };
        assert_eq!(structure.molecular_formula(), "C2H6O");
        assert!((structure.molar_mass() - 46.07).abs() < 0.2);
        assert_eq!(structure.smiles().unwrap(), "CCO");
    }

    #[test]
    fn benzene_kekule_formula_and_smiles() {
        let atoms: Vec<Atom> = (0..6)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let angle = std::f32::consts::TAU * (i as f32) / 6.0;
                atom(i + 1, 6, angle.cos() * 40.0, angle.sin() * 40.0)
            })
            .collect();
        let bonds: Vec<Bond> = (0..6)
            .map(|i| {
                let order = if i % 2 == 0 {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                };
                bond(i + 100, i + 1, (i + 1) % 6 + 1, order)
            })
            .collect();
        let structure = Structure {
            id: 1,
            origin: Point::default(),
            atoms,
            bonds,
        };
        assert_eq!(structure.molecular_formula(), "C6H6");
        let smiles = structure.smiles().unwrap();
        assert!(smiles.starts_with('C') && smiles.contains('1'), "{smiles}");
        assert_eq!(smiles.matches('1').count(), 2, "{smiles}");
    }

    #[test]
    fn aromatic_bonds_are_rejected() {
        let structure = Structure {
            id: 1,
            origin: Point::default(),
            atoms: vec![atom(1, 6, 0.0, 0.0), atom(2, 6, 40.0, 0.0)],
            bonds: vec![bond(3, 1, 2, BondOrder::Aromatic)],
        };
        assert!(structure.smiles().is_err());
    }
}
