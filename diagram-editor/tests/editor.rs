//! Editing invariants, persistence and geometry independent of the GUI host.
use diagram_editor::{Diagram, DiagramEditor, Point, Port, ShapeKind};

#[test]
fn move_resize_and_text_each_undo_as_a_whole_and_keep_links_attached() {
    let mut editor = DiagramEditor::sample();
    let before = editor.diagram().clone();
    let id = before.nodes[0].id;
    let connection = &before.connections[0];
    let old_route = before.route(connection);
    let mut node = before.node(id).unwrap().clone();
    node.position.x += 120.0;
    node.size.x += 80.0;
    node.text = "Изменённый блок".into();
    editor.update_node(node).unwrap();
    let after = editor.diagram().clone();
    assert_eq!(after.connections, before.connections);
    assert_ne!(after.route(connection)[0], old_route[0]);
    assert_eq!(
        after.route(connection)[0],
        after.node(id).unwrap().port(Port::Right)
    );
    editor.undo();
    assert_eq!(editor.diagram(), &before);
    editor.redo();
    assert_eq!(editor.diagram(), &after);
    editor.undo();
    editor
        .add_node(ShapeKind::Ellipse, Point::new(600.0, 300.0))
        .unwrap();
    let branched = editor.diagram().clone();
    editor.redo();
    assert_eq!(editor.diagram(), &branched);
}

#[test]
fn json_roundtrip_preserves_ids_links_text_and_styling() {
    let editor = DiagramEditor::sample();
    let source = editor.diagram().to_json().unwrap();
    assert_eq!(Diagram::from_json(&source).unwrap(), *editor.diagram());
    let mut invalid = editor.diagram().clone();
    invalid.nodes[1].id = invalid.nodes[0].id;
    assert!(invalid.to_json().is_err());
    invalid = editor.diagram().clone();
    invalid.connections[0].to = 999;
    assert!(invalid.validate().is_err());
    invalid = editor.diagram().clone();
    invalid.nodes[0].size.x = f32::NAN;
    assert!(invalid.validate().is_err());
    invalid = editor.diagram().clone();
    invalid.version = 9;
    assert!(invalid.validate().is_err());
    assert!(
        Diagram::from_json("{\"version\":1,\"nodes\":[],\"connections\":[],\"unknown\":true}")
            .is_err()
    );
}

#[test]
fn failed_edits_do_not_mutate_the_scene_or_history() {
    let mut editor = DiagramEditor::sample();
    let before = editor.diagram().clone();
    let revision = editor.revision();
    let id = before.nodes[0].id;
    assert!(editor.connect(id, Port::Left, id, Port::Right).is_err());
    let mut node = before.nodes[0].clone();
    node.size.y = -1.0;
    assert!(editor.update_node(node).is_err());
    let mut invalid = before.clone();
    invalid.nodes.clear();
    assert!(editor.replace(invalid).is_err());
    assert_eq!(editor.diagram(), &before);
    assert_eq!(editor.revision(), revision);
    editor.undo();
    assert_eq!(editor.diagram(), &before);
}

#[test]
fn geometric_hits_exclude_empty_corners_and_ports_stay_on_boundaries() {
    let mut editor = DiagramEditor::default();
    let id = editor
        .add_node(ShapeKind::Diamond, Point::new(0.0, 0.0))
        .unwrap();
    let node = editor.diagram().node(id).unwrap();
    assert!(!node.contains(Point::new(1.0, 1.0)));
    assert!(node.contains(node.center()));
    for port in [Port::Left, Port::Right, Port::Top, Port::Bottom] {
        assert!(node.contains(node.port(port)));
    }
}

#[test]
fn every_port_pair_routes_orthogonally_and_svg_escapes_user_text() {
    let mut editor = DiagramEditor::sample();
    let first = editor.diagram().nodes[0].id;
    let second = editor.diagram().nodes[1].id;
    for a in [Port::Left, Port::Right, Port::Top, Port::Bottom] {
        for b in [Port::Left, Port::Right, Port::Top, Port::Bottom] {
            let id = editor.connect(first, a, second, b).unwrap();
            let c = editor
                .diagram()
                .connections
                .iter()
                .find(|c| c.id == id)
                .unwrap();
            let route = editor.diagram().route(c);
            assert!(route
                .windows(2)
                .all(|pair| (pair[0].x - pair[1].x).abs() < 0.001
                    || (pair[0].y - pair[1].y).abs() < 0.001));
        }
    }
    let mut node = editor.diagram().nodes[0].clone();
    node.text = "<script>&\"Привет\"\nмир".into();
    editor.update_node(node).unwrap();
    let svg = editor.diagram().to_svg().unwrap();
    assert!(svg.contains("&lt;script&gt;&amp;&quot;Привет&quot;"));
    assert!(!svg.contains("<script>"));
    assert!(!svg.contains("foreignObject"));
    assert!(svg.contains("<polygon"));
    assert!(svg.contains("<polyline"));
    let mut reader = quick_xml::Reader::from_str(&svg);
    loop {
        if reader.read_event().unwrap() == quick_xml::events::Event::Eof {
            break;
        }
    }
}

#[test]
fn public_selection_and_history_limits_keep_a_valid_scene() {
    let mut editor = DiagramEditor::sample();
    let id = editor.diagram().nodes[0].id;
    assert!(editor
        .select(Some(diagram_editor::Target::Node(999)))
        .is_err());
    editor
        .select(Some(diagram_editor::Target::Node(id)))
        .unwrap();
    editor.delete_selected();
    assert!(editor.diagram().node(id).is_none());
    editor.undo();
    assert!(editor.diagram().node(id).is_some());
    for i in 0..150 {
        let mut node = editor.diagram().node(id).unwrap().clone();
        node.text = format!("Версия {i}");
        editor.update_node(node).unwrap();
    }
    for _ in 0..130 {
        editor.undo();
    }
    assert_eq!(editor.diagram().node(id).unwrap().text, "Версия 21");
    editor.diagram().validate().unwrap();
}

#[test]
fn grouping_duplication_and_reordering_are_atomic_and_keep_internal_links() {
    let mut editor = DiagramEditor::sample();
    let initial = editor.diagram().clone();
    let ids: Vec<_> = initial.nodes[..2].iter().map(|n| n.id).collect();
    editor.select_nodes(&ids).unwrap();
    editor.group_selected().unwrap();
    let grouped = editor.diagram().clone();
    assert_eq!(grouped.groups.len(), 1);
    editor.duplicate_selected().unwrap();
    let copied = editor.diagram().clone();
    assert_eq!(copied.groups.len(), 2);
    assert_eq!(copied.nodes.len(), 5);
    assert_eq!(copied.connections.len(), 3);
    copied.validate().unwrap();
    let copied_ids = editor.selected_nodes();
    assert!(copied
        .connections
        .iter()
        .any(|c| copied_ids.contains(&c.from) && copied_ids.contains(&c.to)));
    editor.undo();
    assert_eq!(editor.diagram(), &grouped);
    editor.undo();
    assert_eq!(editor.diagram(), &initial);
    editor.redo();
    editor.select_nodes(&ids).unwrap();
    editor.ungroup_selected();
    assert!(editor.diagram().groups.is_empty());
    editor.undo();
    assert_eq!(editor.diagram(), &grouped);
}
#[test]
fn v1_json_migrates_and_extended_styles_roundtrip() {
    let old = r#"{"version":1,"nodes":[{"id":1,"kind":"Rectangle","position":{"x":0,"y":0},"size":{"x":160,"y":80},"text":"old","fill":[255,255,255],"stroke":[0,0,0],"font_size":16}],"connections":[]}"#;
    let diagram = Diagram::from_json(old).unwrap();
    assert_eq!(diagram.version, 2);
    assert!(diagram.groups.is_empty());
    assert_eq!(
        Diagram::from_json(&diagram.to_json().unwrap()).unwrap(),
        diagram
    );
}
