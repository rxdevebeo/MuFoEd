//! Localization, rigid object commands, rotation and bounded routing regressions.
#![allow(clippy::many_single_char_names, clippy::float_cmp)]
use diagram_editor::{Alignment, Axis, Diagram, DiagramEditor, Locale, Point, Port, ShapeKind};

fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
        egui::Shape::Vec(v) => {
            for s in v {
                collect(s, out);
            }
        }
        _ => {}
    }
}
#[test]
fn catalogs_are_complete_and_instance_local_without_changing_scene_history() {
    let en: std::collections::BTreeMap<String, String> =
        serde_json::from_str(include_str!("../locales/en.json")).unwrap();
    let ru: std::collections::BTreeMap<String, String> =
        serde_json::from_str(include_str!("../locales/ru.json")).unwrap();
    assert_eq!(en.keys().collect::<Vec<_>>(), ru.keys().collect::<Vec<_>>());
    for (key, value) in &ru {
        assert!(!value.is_empty());
        assert_eq!(
            key.matches('{').count(),
            value.matches('{').count(),
            "{key}"
        );
    }
    let ru = Locale::new("ru-RU");
    assert_eq!(ru.text("Properties"), "Свойства");
    assert_eq!(
        ru.diagnostic("Invalid shape geometry or text"),
        "Недопустимые размеры, координаты или текст фигуры"
    );
    assert_eq!(
        ru.diagnostic("Shape 12 font size was rounded to half a point"),
        "Кегль фигуры 12 округлён до половины пункта"
    );
    let custom = Locale::new("fr")
        .with_catalog(r#"{"Properties":"Propriétés"}"#)
        .unwrap();
    assert_eq!(custom.text("Properties"), "Propriétés");
    assert_eq!(custom.text("Delete"), "Delete");
    assert!(Locale::new("en")
        .with_catalog(r#"{"Properties":12}"#)
        .is_err());
    let mut editor = DiagramEditor::sample();
    let before = editor.diagram().clone();
    let revision = editor.revision();
    editor.set_locale(ru);
    editor.set_locale(custom);
    assert_eq!(editor.diagram(), &before);
    assert_eq!(editor.revision(), revision);
    let ctx = egui::Context::default();
    for code in ["en", "ru"] {
        editor.set_locale(Locale::new(code));
        let mut strings = vec![];
        for _ in 0..3 {
            let out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    editor.show(ui);
                },
            );
            for shape in out.shapes {
                collect(&shape.shape, &mut strings);
            }
        }
        assert!(strings.iter().any(|s| s
            == if code == "ru" {
                "Свойства"
            } else {
                "Properties"
            }));
        if code == "en" {
            assert!(
                !strings
                    .iter()
                    .any(|s| s.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c))),
                "{strings:?}"
            );
        }
    }
}

#[test]
fn alignment_distribution_treat_groups_as_rigid_objects_and_undo_once() {
    let mut e = DiagramEditor::default();
    let mut ids = vec![];
    for p in [
        Point::new(0.0, 10.0),
        Point::new(180.0, 20.0),
        Point::new(420.0, 80.0),
        Point::new(780.0, 150.0),
    ] {
        ids.push(e.add_node(ShapeKind::Rectangle, p).unwrap());
    }
    e.select_nodes(&ids[..2]).unwrap();
    e.group_selected().unwrap();
    e.select_nodes(&ids).unwrap();
    let before = e.diagram().clone();
    let revision = e.revision();
    e.align_selected(Alignment::Top).unwrap();
    assert_eq!(e.revision(), revision + 1);
    let a = e.diagram().node(ids[0]).unwrap();
    let b = e.diagram().node(ids[1]).unwrap();
    assert_eq!(b.position.y - a.position.y, 10.0);
    assert_eq!(e.diagram().node(ids[2]).unwrap().position.y, 10.0);
    e.undo();
    assert_eq!(e.diagram(), &before);
    e.redo();
    e.select_nodes(&ids).unwrap();
    e.distribute_selected(Axis::Horizontal).unwrap();
    let group_right = e.diagram().node(ids[1]).unwrap().position.x + 160.0;
    let middle = e.diagram().node(ids[2]).unwrap();
    let last = e.diagram().node(ids[3]).unwrap();
    assert!(
        (middle.position.x - group_right - (last.position.x - middle.position.x - 160.0)).abs()
            < 0.01
    );
    e.select_nodes(&ids[..2]).unwrap();
    let before = e.diagram().clone();
    assert!(e.align_selected(Alignment::Left).is_err());
    assert_eq!(e.diagram(), &before);
}

#[test]
fn rotation_keeps_ports_hits_and_group_geometry_and_roundtrips_json_svg() {
    let mut e = DiagramEditor::sample();
    let ids: Vec<_> = e.diagram().nodes.iter().map(|n| n.id).collect();
    e.select_nodes(&ids).unwrap();
    e.group_selected().unwrap();
    let before = e.diagram().clone();
    e.rotate_selected(90.0).unwrap();
    assert!(e.diagram().nodes.iter().all(|n| n.style.rotation == 90.0));
    let n = &e.diagram().nodes[0];
    assert!(n.contains(n.center()));
    let left = n.port(Port::Left);
    assert!((left.x - n.center().x).abs() < 0.001);
    assert!(left.y < n.center().y);
    assert_eq!(
        Diagram::from_json(&e.diagram().to_json().unwrap()).unwrap(),
        *e.diagram()
    );
    assert!(e.diagram().to_svg().unwrap().contains("rotate(90"));
    e.undo();
    assert_eq!(e.diagram(), &before);
    e.redo();
    e.select_nodes(&ids).unwrap();
    let changed = e.diagram().clone();
    assert!(e.rotate_selected(f32::NAN).is_err());
    assert_eq!(e.diagram(), &changed);
    for _ in 0..3 {
        e.rotate_selected(90.0).unwrap();
    }
    for (a, b) in e.diagram().nodes.iter().zip(&before.nodes) {
        assert!((a.center().x - b.center().x).abs() < 0.01);
        assert!((a.center().y - b.center().y).abs() < 0.01);
    }
}

#[test]
fn v2_scenes_keep_legacy_routing_and_zero_rotation() {
    let mut old: serde_json::Value =
        serde_json::from_str(&DiagramEditor::sample().diagram().to_json().unwrap()).unwrap();
    old["version"] = 2.into();
    for n in old["nodes"].as_array_mut().unwrap() {
        n["style"].as_object_mut().unwrap().remove("rotation");
    }
    for c in old["connections"].as_array_mut().unwrap() {
        c.as_object_mut().unwrap().remove("routing");
    }
    let loaded = Diagram::from_json(&old.to_string()).unwrap();
    assert_eq!(loaded.version, 3);
    assert!(loaded.nodes.iter().all(|n| n.style.rotation == 0.0));
    assert!(loaded
        .connections
        .iter()
        .all(|c| c.routing == diagram_editor::Routing::Simple));
}

#[test]
fn connectors_avoid_obstacles_and_recompute_after_edits() {
    let mut e = DiagramEditor::default();
    let a = e
        .add_node(ShapeKind::Rectangle, Point::new(0.0, 0.0))
        .unwrap();
    let obstacle = e
        .add_node(ShapeKind::Rectangle, Point::new(240.0, -40.0))
        .unwrap();
    let b = e
        .add_node(ShapeKind::Rectangle, Point::new(560.0, 0.0))
        .unwrap();
    e.connect(a, Port::Right, b, Port::Left).unwrap();
    let c = &e.diagram().connections[0];
    let (path, ok) = e.diagram().route_status(c);
    assert!(ok, "{path:?}");
    assert!(path.iter().any(|p| p.y < -40.0 || p.y > 40.0));
    let node = e.diagram().node(obstacle).unwrap();
    for pair in path.windows(2) {
        let mid = Point::new(pair[0].x.midpoint(pair[1].x), pair[0].y.midpoint(pair[1].y));
        assert!(!node.contains(mid));
    }
    let before = e.diagram().clone();
    let mut n = node.clone();
    n.position.y = 300.0;
    e.update_node(n).unwrap();
    let new = e.diagram().route(&e.diagram().connections[0]);
    assert_ne!(new, path);
    assert!(new.iter().all(|p| (p.y - 40.0).abs() < 0.01));
    e.undo();
    assert_eq!(e.diagram(), &before);
    assert_eq!(e.diagram().route(&e.diagram().connections[0]), path);
    let mut n = e.diagram().node(b).unwrap().clone();
    n.position = Point::new(20.0, 0.0);
    e.update_node(n).unwrap();
    assert!(!e.diagram().route_status(&e.diagram().connections[0]).1);
}

#[cfg(feature = "docx")]
#[test]
fn rotated_group_and_obstacle_routes_survive_docx_and_render() {
    let mut e = DiagramEditor::sample();
    let ids: Vec<_> = e.diagram().nodes.iter().map(|n| n.id).collect();
    e.select_nodes(&ids).unwrap();
    e.group_selected().unwrap();
    e.rotate_selected(30.0).unwrap();
    let output = e.diagram().to_docx().unwrap();
    let restored = Diagram::from_docx(&output.bytes).unwrap();
    for n in &restored.diagram.nodes {
        assert!((n.style.rotation - 30.0).abs() < 0.01);
    }
    let package = strict_ooxml::StrictDocument::open_reader(
        &output.bytes[..],
        &strict_ooxml::OpenOptions::default(),
    )
    .unwrap();
    let pages = package
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap();
    assert!(!pages.is_empty());
    if let Ok(dir) = std::env::var("DIAGRAM_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let dir = std::path::Path::new(&dir);
        std::fs::write(dir.join("rotated.docx"), output.bytes).unwrap();
        std::fs::write(dir.join("rotated.json"), e.diagram().to_json().unwrap()).unwrap();
        std::fs::write(dir.join("rotated.svg"), e.diagram().to_svg().unwrap()).unwrap();
        std::fs::write(dir.join("rotated-docx.svg"), &pages[0].svg).unwrap();
    }
}
