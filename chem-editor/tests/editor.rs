//! Editor commands and the egui widget.

use chem_editor::model::{BondOrder, ChemObject, Point};
use chem_editor::{AlignEdge, Axis, ChemEditor, Mode, Target, Tool};

#[test]
fn building_a_ring_is_undoable() {
    let mut editor = ChemEditor::default();
    let structure = editor.add_ring(Point::new(0.0, 0.0), 6).unwrap();
    assert_eq!(editor.structure(structure).unwrap().atoms.len(), 6);
    let revision = editor.revision();
    editor.undo();
    assert!(editor
        .structure(structure)
        .is_some_and(|s| s.atoms.is_empty()));
    editor.redo();
    assert_eq!(editor.structure(structure).unwrap().atoms.len(), 6);
    assert!(editor.revision() >= revision);
}

#[test]
fn atom_and_bond_selection() {
    let mut editor = ChemEditor::default();
    let a = editor.add_atom(6, Point::new(0.0, 0.0)).unwrap();
    let b = editor.add_atom(8, Point::new(44.0, 0.0)).unwrap();
    let bond = editor.connect(a, b).unwrap();
    let structure = editor.active().unwrap();
    editor.select(Some(Target::Atom(structure, a))).unwrap();
    assert!(editor.is_selected(Target::Atom(structure, a)));
    assert_eq!(editor.selection_count(), 1);
    editor
        .set_bond(
            structure,
            bond,
            BondOrder::Double,
            chem_editor::model::BondStereo::Plain,
        )
        .unwrap();
    let bonds = &editor.structure(structure).unwrap().bonds;
    assert_eq!(bonds[0].order, BondOrder::Double);
    editor.select(Some(Target::Bond(structure, bond))).unwrap();
    editor.delete_selected();
    assert!(editor.structure(structure).unwrap().bonds.is_empty());
}

#[test]
fn multi_selection_moves_together() {
    let mut editor = ChemEditor::default();
    let a = editor.add_atom(6, Point::new(0.0, 0.0)).unwrap();
    let b = editor.add_atom(6, Point::new(44.0, 0.0)).unwrap();
    let structure = editor.active().unwrap();
    editor
        .set_selection([Target::Atom(structure, a), Target::Atom(structure, b)])
        .unwrap();
    assert_eq!(editor.selection_count(), 2);
    let before = editor
        .structure(structure)
        .unwrap()
        .atom(a)
        .unwrap()
        .position;
    editor.nudge_selected(Point::new(5.0, 0.0)).unwrap();
    let after = editor
        .structure(structure)
        .unwrap()
        .atom(a)
        .unwrap()
        .position;
    assert!((after.x - before.x - 5.0).abs() < 0.001);
    editor.undo();
    let restored = editor
        .structure(structure)
        .unwrap()
        .atom(a)
        .unwrap()
        .position;
    assert!((restored.x - before.x).abs() < 0.001);
}

#[test]
fn snap_places_atoms_on_the_grid() {
    let mut editor = ChemEditor::default();
    let structure = editor.add_structure(Point::new(0.0, 0.0)).unwrap();
    let atom = editor.add_atom(6, Point::new(13.0, 47.0)).unwrap();
    let position = editor
        .structure(structure)
        .unwrap()
        .atom(atom)
        .unwrap()
        .position;
    assert!((position.x % 22.0).abs() < 0.001, "{position:?}");
    assert!((position.y % 22.0).abs() < 0.001, "{position:?}");
}

#[test]
fn duplicating_a_fragment_adds_a_copy() {
    let mut editor = ChemEditor::default();
    let a = editor.add_atom(6, Point::new(0.0, 0.0)).unwrap();
    let b = editor.add_atom(8, Point::new(44.0, 0.0)).unwrap();
    editor.connect(a, b).unwrap();
    let structure = editor.active().unwrap();
    editor
        .set_selection([Target::Atom(structure, a), Target::Atom(structure, b)])
        .unwrap();
    let before = editor.document().objects.len();
    editor.duplicate_selected().unwrap();
    assert_eq!(editor.document().objects.len(), before + 1);
    assert_eq!(editor.selection_count(), 3);
    editor.undo();
    assert_eq!(editor.document().objects.len(), before);
}

#[test]
fn align_and_distribute_move_atoms() {
    let mut editor = ChemEditor::default();
    editor.set_snap(false);
    let structure = editor.add_structure(Point::new(0.0, 0.0)).unwrap();
    let a = editor.add_atom(6, Point::new(0.0, 0.0)).unwrap();
    let b = editor.add_atom(6, Point::new(40.0, 30.0)).unwrap();
    let c = editor.add_atom(6, Point::new(90.0, 10.0)).unwrap();
    editor
        .set_selection([
            Target::Atom(structure, a),
            Target::Atom(structure, b),
            Target::Atom(structure, c),
        ])
        .unwrap();
    editor
        .align_selection(Axis::Vertical, AlignEdge::Start)
        .unwrap();
    let structure_ref = editor.structure(structure).unwrap();
    let ys: Vec<f32> = [a, b, c]
        .iter()
        .map(|id| structure_ref.atom(*id).unwrap().position.y)
        .collect();
    assert!(ys.iter().all(|y| (y - ys[0]).abs() < 0.001), "{ys:?}");
    editor.distribute_selection(Axis::Horizontal).unwrap();
    let xs: Vec<f32> = [a, b, c]
        .iter()
        .map(|id| {
            editor
                .structure(structure)
                .unwrap()
                .atom(*id)
                .unwrap()
                .position
                .x
        })
        .collect();
    assert!((xs[1] - xs[0] - (xs[2] - xs[1])).abs() < 0.001, "{xs:?}");
}

#[test]
fn clipboard_round_trips_a_fragment() {
    let mut editor = ChemEditor::default();
    let a = editor.add_atom(6, Point::new(0.0, 0.0)).unwrap();
    let b = editor.add_atom(8, Point::new(44.0, 0.0)).unwrap();
    let structure = editor.active().unwrap();
    editor
        .set_selection([Target::Atom(structure, a), Target::Atom(structure, b)])
        .unwrap();
    let json = editor.copy_selection().unwrap();
    let before = editor.document().objects.len();
    editor.paste_clipboard().unwrap();
    assert_eq!(editor.document().objects.len(), before + 1);
    editor.paste_json(&json).unwrap();
    assert_eq!(editor.document().objects.len(), before + 2);
    editor.undo();
    assert_eq!(editor.document().objects.len(), before + 1);
}

#[test]
fn the_widget_draws_without_panicking() {
    let mut editor = ChemEditor::sample();
    assert_eq!(editor.mode(), Mode::Structure);
    editor.set_mode(Mode::Reaction);
    assert_eq!(editor.mode(), Mode::Reaction);
    editor.set_mode(Mode::Structure);
    editor.set_tool(Tool::Atom(8));
    assert_eq!(editor.element(), 8);
    editor.request_fit();
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 760.0),
        )),
        ..Default::default()
    };
    let _ = context.run_ui(input, |ui| {
        editor.show(ui);
    });
    assert!(matches!(
        editor.document().objects.first(),
        Some(ChemObject::Structure(_))
    ));
}
