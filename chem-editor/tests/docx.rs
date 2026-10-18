//! DOCX export and import of the crate's own output.
#![cfg(feature = "docx")]

use chem_editor::mhchem;
use chem_editor::model::{
    Atom, Bond, BondOrder, BondStereo, ChemDocument, ChemObject, Point, Structure, VERSION,
};

fn document() -> ChemDocument {
    let structure = Structure {
        id: 1,
        origin: Point::new(30.0, 30.0),
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
                element: 6,
                position: Point::new(44.0, 0.0),
                charge: 0,
                hydrogens: None,
            },
            Atom {
                id: 4,
                element: 8,
                position: Point::new(88.0, 0.0),
                charge: 0,
                hydrogens: None,
            },
        ],
        bonds: vec![
            Bond {
                id: 5,
                a: 2,
                b: 3,
                order: BondOrder::Single,
                stereo: BondStereo::Wedge,
            },
            Bond {
                id: 6,
                a: 3,
                b: 4,
                order: BondOrder::Double,
                stereo: BondStereo::Plain,
            },
        ],
    };
    let mut reaction = mhchem::parse("2H2 + O2 -> 2H2O").unwrap();
    reaction.id = 100;
    ChemDocument {
        version: VERSION,
        objects: vec![
            ChemObject::Structure(structure),
            ChemObject::Reaction(reaction),
        ],
    }
}

#[test]
fn docx_round_trips_the_editors_own_output() {
    let original = document();
    let export = original.to_docx().unwrap();
    assert!(!export.bytes.is_empty());
    let imported = ChemDocument::from_docx(&export.bytes).unwrap();
    let imported = imported.document;
    imported.validate().unwrap();

    let structures: Vec<&Structure> = imported
        .objects
        .iter()
        .filter_map(|object| match object {
            ChemObject::Structure(structure) => Some(structure),
            ChemObject::Reaction(_) => None,
        })
        .collect();
    assert_eq!(structures.len(), 1);
    let structure = structures[0];
    assert_eq!(structure.atoms.len(), 3);
    assert_eq!(structure.bonds.len(), 2);
    assert_eq!(structure.molecular_formula(), "C2H4O");
    assert!(structure
        .bonds
        .iter()
        .any(|bond| bond.order == BondOrder::Double));
    assert!(structure
        .bonds
        .iter()
        .any(|bond| bond.stereo == BondStereo::Wedge));

    let reactions: Vec<_> = imported
        .objects
        .iter()
        .filter_map(|object| match object {
            ChemObject::Reaction(reaction) => Some(reaction),
            ChemObject::Structure(_) => None,
        })
        .collect();
    assert_eq!(reactions.len(), 1);
    let written = mhchem::write(reactions[0]);
    assert!(written.contains("->"), "{written}");
    assert!(written.contains("2H2O"), "{written}");
}

#[test]
fn oversized_input_is_rejected() {
    let bytes = vec![0_u8; 33 * 1024 * 1024];
    assert!(ChemDocument::from_docx(&bytes).is_err());
}
