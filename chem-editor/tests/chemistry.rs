//! Model, `mhchem`, layout and export behaviour seen through the public API.

use chem_editor::mhchem;
use chem_editor::model::{
    Atom, Bond, BondOrder, BondStereo, ChemDocument, ChemObject, Point, Structure, VERSION,
};

fn methanol() -> Structure {
    Structure {
        id: 1,
        origin: Point::new(20.0, 20.0),
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
                position: Point::new(44.0, 0.0),
                charge: 0,
                hydrogens: None,
            },
        ],
        bonds: vec![Bond {
            id: 4,
            a: 2,
            b: 3,
            order: BondOrder::Single,
            stereo: BondStereo::Plain,
        }],
    }
}

#[test]
fn json_round_trips_a_document() {
    let document = ChemDocument {
        version: VERSION,
        objects: vec![ChemObject::Structure(methanol())],
    };
    let json = document.to_json().unwrap();
    let again = ChemDocument::from_json(&json).unwrap();
    assert_eq!(document, again);
    assert_eq!(again.objects[0].id(), 1);
}

#[test]
fn svg_export_is_self_contained() {
    let mut reaction = mhchem::parse("2H2 + O2 -> 2H2O").unwrap();
    reaction.id = 9;
    let document = ChemDocument {
        version: VERSION,
        objects: vec![
            ChemObject::Structure(methanol()),
            ChemObject::Reaction(reaction),
        ],
    };
    let svg = document.to_svg().unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("<text"));
    assert!(!svg.to_ascii_lowercase().contains("<script"));
}

#[test]
fn formula_mass_and_smiles_are_derived() {
    let structure = methanol();
    assert_eq!(structure.molecular_formula(), "CH4O");
    assert!((structure.molar_mass() - 32.04).abs() < 0.2);
    assert_eq!(structure.smiles().unwrap(), "CO");
}

#[test]
fn mhchem_reactions_normalize() {
    let reaction = mhchem::parse("N2 + 3H2 <=> 2NH3").unwrap();
    let written = mhchem::write(&reaction);
    assert!(written.contains("<=>"), "{written}");
    let again = mhchem::parse(&written).unwrap();
    assert_eq!(mhchem::write(&again), written);
}

#[cfg(feature = "raster")]
#[test]
fn png_export_starts_with_a_png_signature() {
    let document = ChemDocument {
        version: VERSION,
        objects: vec![ChemObject::Structure(methanol())],
    };
    let png = document.to_png(2.0).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
