#![cfg(feature = "docx")]
//! Real public-model -> Strict writer -> OPC reader -> editor conversions.
use diagram_editor::{Dash, Diagram, DiagramEditor, Point, ShapeKind, TextAlign};

#[test]
fn all_presets_styles_text_and_groups_survive_a_strict_package() {
    let mut editor = DiagramEditor::default();
    let kinds = [
        ShapeKind::Rectangle,
        ShapeKind::Rounded,
        ShapeKind::Ellipse,
        ShapeKind::Diamond,
        ShapeKind::Text,
        ShapeKind::Triangle,
        ShapeKind::Trapezoid,
        ShapeKind::Hexagon,
        ShapeKind::Document,
        ShapeKind::RightArrow,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let col = u16::try_from(i % 3).unwrap();
        let row = u16::try_from(i / 3).unwrap();
        let id = editor
            .add_node(
                kind,
                Point::new(f32::from(col) * 200.0, f32::from(row) * 150.0),
            )
            .unwrap();
        let mut node = editor.diagram().node(id).unwrap().clone();
        node.text = format!("Фигура {}\nстрока 2", i + 1);
        node.style.bold = true;
        node.style.italic = true;
        node.style.align = TextAlign::Right;
        node.style.width = 2.0;
        node.style.padding = 6.0;
        node.style.dash = Dash::Dashed;
        editor.update_node(node).unwrap();
    }
    let ids: Vec<_> = editor.diagram().nodes[..2].iter().map(|n| n.id).collect();
    editor.select_nodes(&ids).unwrap();
    editor.group_selected().unwrap();
    let exported = editor.diagram().to_docx().unwrap();
    assert!(
        exported.issues.iter().all(|i| i.contains("inactive")),
        "{:?}",
        exported.issues
    );
    let package = strict_ooxml::StrictDocument::open_reader(
        &exported.bytes[..],
        &strict_ooxml::OpenOptions::default(),
    )
    .unwrap();
    let pages = package
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap();
    assert!(!pages.is_empty());
    assert!(pages[0].svg.matches("<path").count() >= 8);
    let imported = Diagram::from_docx(&exported.bytes).unwrap();
    assert_eq!(imported.diagram.nodes.len(), kinds.len());
    assert_eq!(imported.diagram.groups, editor.diagram().groups);
    for expected in &editor.diagram().nodes {
        let actual = imported.diagram.node(expected.id).unwrap();
        assert_eq!(actual.kind, expected.kind);
        assert_eq!(actual.text, expected.text);
        assert_eq!(actual.style.bold, expected.style.bold);
        assert_eq!(actual.style.italic, expected.style.italic);
        assert_eq!(actual.style.align, expected.style.align);
        assert_eq!(actual.style.dash, expected.style.dash);
        assert!((actual.position.x - expected.position.x).abs() < 0.001);
        assert!((actual.style.width - expected.style.width).abs() < 0.001);
    }
    if let Ok(dir) = std::env::var("DIAGRAM_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("shapes.docx"),
            &exported.bytes,
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("shapes.json"),
            editor.diagram().to_json().unwrap(),
        )
        .unwrap();
        for (i, page) in pages.iter().enumerate() {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("shapes-page-{i}.svg")),
                &page.svg,
            )
            .unwrap();
        }
    }
}
#[test]
fn connectors_have_named_binding_limits_and_unambiguous_reconstruction() {
    let editor = DiagramEditor::sample();
    let exported = editor.diagram().to_docx().unwrap();
    assert_eq!(exported.issues.len(), 1);
    let imported = Diagram::from_docx(&exported.bytes).unwrap();
    assert_eq!(imported.issues.len(), 1);
    assert_eq!(
        imported.diagram.connections.len(),
        editor.diagram().connections.len()
    );
    for (a, b) in imported
        .diagram
        .connections
        .iter()
        .zip(&editor.diagram().connections)
    {
        assert_eq!(
            (a.id, a.from, a.to, a.from_port, a.to_port),
            (b.id, b.from, b.to, b.from_port, b.to_port)
        );
    }
}
