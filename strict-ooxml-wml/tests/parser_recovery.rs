//! Invalid optional properties and foreign formula children must not swallow content.
#![allow(clippy::doc_markdown)]
mod common;
use common::{document_parts, parse_parts};
use std::fmt::Write;
use strict_ooxml_wml::model::{Block, Inline, RunContent};

#[test]
fn settings_containers_ignore_foreign_and_incomplete_entries() {
    let settings = format!("<w:settings xmlns:w=\"{}\" xmlns:m=\"{}\" xmlns:f=\"urn:foreign\"><f:ignored/><w:docVars><f:docVar w:name=\"bad\" w:val=\"bad\"/><w:unknown/><w:docVar w:name=\"incomplete\"/><w:docVar w:name=\"kept\" w:val=\"value\"/></w:docVars><w:rsids><f:rsid w:val=\"bad\"/><w:rsid/><w:rsid w:val=\"12345678\"/></w:rsids><w:compat><f:compatSetting w:name=\"bad\" w:val=\"bad\"/><w:compatSetting w:name=\"incomplete\"/><w:compatSetting w:name=\"kept\" w:val=\"value\"/><w:spaceForUL w:val=\"off\"/></w:compat><m:mathPr><f:mathFont m:val=\"bad\"/><m:unknown/><m:mathFont m:val=\"Cambria Math\"/></m:mathPr><w:noLineBreaksAfter/><w:noLineBreaksBefore/></w:settings>", common::W_NS, common::M_NS);
    let relationships = common::rels(&[(
        "settings",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/settings",
        "settings.xml",
    )]);
    let doc = parse_parts(&document_parts(
        "<w:p/>",
        &[
            ("word/settings.xml", settings.into_bytes()),
            ("word/_rels/document.xml.rels", relationships),
        ],
    ))
    .expect("settings recovery");
    assert_eq!(doc.settings.document_variables.len(), 1);
    assert_eq!(&*doc.settings.document_variables[0].0, "kept");
    assert_eq!(doc.settings.compatibility.len(), 1);
    assert!(!doc.settings.compat_flags.space_for_underline);
    assert_eq!(
        doc.settings
            .revision_save_ids
            .as_ref()
            .expect("rsids")
            .entries
            .len(),
        1
    );
    assert_eq!(
        doc.settings
            .math_properties
            .as_ref()
            .expect("math properties")
            .math_font
            .as_deref(),
        Some("Cambria Math")
    );
    assert!(!doc.support.is_empty());
}

#[test]
fn nested_table_controls_keep_the_innermost_identity() {
    let cell = "<w:tc><w:p><w:r><w:t>kept</w:t></w:r></w:p></w:tc>";
    let cell_control = format!("<w:sdt><w:sdtPr><w:tag w:val=\"inner-cell\"/></w:sdtPr><w:sdtContent>{cell}</w:sdtContent></w:sdt>");
    let row = format!("<w:tr><w:sdt><w:sdtContent>{cell_control}</w:sdtContent></w:sdt></w:tr>");
    let inner = format!("<w:sdt><w:sdtPr><w:tag w:val=\"inner-row\"/></w:sdtPr><w:sdtContent>{row}</w:sdtContent></w:sdt>");
    let body = format!("<w:tbl><w:sdt><w:sdtContent>{inner}</w:sdtContent></w:sdt></w:tbl>");
    let doc = parse_parts(&document_parts(&body, &[])).expect("nested controls");
    let Block::Table(table) = &doc.body.blocks[0] else {
        panic!("table disappeared")
    };
    assert_eq!(table.rows.len(), 1);
    let row = &table.rows[0];
    assert_eq!(
        row.sdt.as_ref().expect("row control").tag.as_deref(),
        Some("inner-row")
    );
    assert_eq!(row.cells.len(), 1);
    assert_eq!(
        row.cells[0]
            .sdt
            .as_ref()
            .expect("cell control")
            .tag
            .as_deref(),
        Some("inner-cell")
    );
}

#[test]
fn table_controls_and_invalid_properties_preserve_cells() {
    let foreign = "<foreign xmlns=\"urn:foreign\"><p>discarded</p></foreign>";
    for value in ["nonsense", "", "-1"] {
        let cell = format!("<w:tc>{foreign}<w:tcPr>{foreign}<w:vAlign w:val=\"{value}\"/><w:textDirection w:val=\"{value}\"/><w:tcW w:type=\"{value}\" w:w=\"{value}\"/><w:vMerge w:val=\"{value}\"/><w:shd w:val=\"{value}\"/></w:tcPr><w:p><w:r><w:t>kept</w:t></w:r></w:p></w:tc>");
        let row = format!("<w:tr w:rsidR=\"12345678\">{foreign}<w:trPr>{foreign}<w:jc w:val=\"{value}\"/><w:trHeight w:hRule=\"{value}\" w:val=\"{value}\"/></w:trPr><w:sdt>{foreign}<w:sdtPr><w:tag w:val=\"cell\"/></w:sdtPr><w:sdtContent>{foreign}{cell}</w:sdtContent></w:sdt></w:tr>");
        let body = format!("<w:tbl>{foreign}<w:tblPr>{foreign}<w:jc w:val=\"{value}\"/><w:tblLayout w:type=\"{value}\"/><w:tblW w:type=\"{value}\" w:w=\"{value}\"/><w:tblBorders><w:top w:val=\"{value}\"/></w:tblBorders></w:tblPr><w:tblGrid>{foreign}<w:unknown/><w:gridCol w:w=\"2400\"/></w:tblGrid><w:sdt>{foreign}<w:sdtPr><w:tag w:val=\"row\"/></w:sdtPr><w:sdtContent>{foreign}{row}</w:sdtContent></w:sdt></w:tbl>");
        let doc = parse_parts(&document_parts(&body, &[])).expect("table recovery");
        let Block::Table(table) = &doc.body.blocks[0] else {
            panic!("table disappeared")
        };
        assert_eq!(table.grid.len(), 1);
        assert_eq!(table.rows.len(), 1);
        assert!(table.rows[0].sdt.is_some());
        assert_eq!(table.rows[0].cells.len(), 1);
        assert!(table.rows[0].cells[0].sdt.is_some());
        let Block::Paragraph(p) = &table.rows[0].cells[0].blocks[0] else {
            panic!("cell paragraph disappeared")
        };
        assert!(p.inlines.iter().any(|inline| matches!(inline, Inline::Run(run) if run.content.iter().any(|c| matches!(c, RunContent::Text(t) if t.text == "kept")))));
        assert!(
            !doc.support.is_empty(),
            "skipped and invalid properties must be reported"
        );
    }
}

#[test]
fn invalid_optional_properties_preserve_the_paragraph() {
    let paragraph_properties = ["jc", "textDirection", "outlineLvl"];
    let run_properties = [
        "u",
        "vertAlign",
        "highlight",
        "sz",
        "szCs",
        "position",
        "spacing",
        "kern",
        "w",
        "effect",
        "em",
        "b",
        "i",
        "strike",
        "caps",
        "smallCaps",
    ];
    for value in ["nonsense", "-1", "4294967296", "", "600%"] {
        let mut ppr = String::new();
        for name in paragraph_properties {
            write!(&mut ppr, "<w:{name} w:val=\"{value}\"/>").expect("write string");
        }
        let mut rpr = String::new();
        for name in run_properties {
            write!(&mut rpr, "<w:{name} w:val=\"{value}\"/>").expect("write string");
        }
        rpr.push_str("<w:rFonts w:hint=\"invalid\"/><foreign xmlns=\"urn:foreign\"/>");
        ppr.push_str("<w:framePr w:hAnchor=\"invalid\" w:vAnchor=\"invalid\" w:xAlign=\"invalid\" w:yAlign=\"invalid\" w:wrap=\"invalid\" w:hRule=\"invalid\"/><foreign xmlns=\"urn:foreign\"/>");
        let body = format!(
            "<w:p><w:pPr>{ppr}</w:pPr><w:r><w:rPr>{rpr}</w:rPr><w:t>kept</w:t></w:r></w:p>"
        );
        let doc = parse_parts(&document_parts(&body, &[])).expect("optional properties recover");
        let Block::Paragraph(p) = &doc.body.blocks[0] else {
            panic!("paragraph disappeared")
        };
        assert!(p.inlines.iter().any(|inline| matches!(inline, Inline::Run(run) if run.content.iter().any(|c| matches!(c, RunContent::Text(t) if t.text == "kept")))));
        assert!(!doc.support.is_empty(), "invalid values must be reported");
    }
}

#[test]
fn every_formula_container_skips_foreign_children_without_losing_its_argument() {
    let x = "<m:r><m:t>x</m:t></m:r>";
    for (name, children) in [
        ("f", format!("<m:num>{x}</m:num><m:den/>")),
        ("rad", format!("<m:deg/><m:e>{x}</m:e>")),
        ("sSup", format!("<m:e>{x}</m:e><m:sup/>")),
        ("sSub", format!("<m:e>{x}</m:e><m:sub/>")),
        ("sSubSup", format!("<m:e>{x}</m:e><m:sub/><m:sup/>")),
        ("sPre", format!("<m:sub/><m:sup/><m:e>{x}</m:e>")),
        ("nary", format!("<m:sub/><m:sup/><m:e>{x}</m:e>")),
        ("d", format!("<m:e>{x}</m:e>")),
        ("acc", format!("<m:e>{x}</m:e>")),
        ("bar", format!("<m:e>{x}</m:e>")),
        ("groupChr", format!("<m:e>{x}</m:e>")),
        ("limLow", format!("<m:e>{x}</m:e><m:lim/>")),
        ("limUpp", format!("<m:e>{x}</m:e><m:lim/>")),
        ("func", format!("<m:fName/><m:e>{x}</m:e>")),
        ("eqArr", format!("<m:e>{x}</m:e>")),
        ("m", format!("<m:mr><m:e>{x}</m:e></m:mr>")),
        ("box", format!("<m:e>{x}</m:e>")),
        ("borderBox", format!("<m:e>{x}</m:e>")),
        ("phant", format!("<m:e>{x}</m:e>")),
    ] {
        let body = format!("<w:p><m:oMath><m:{name}><foreign xmlns=\"urn:foreign\">discarded</foreign><m:unknown/><m:{name}Pr><m:unknown/><m:ctrlPr><w:rPr><w:b/></w:rPr></m:ctrlPr></m:{name}Pr>{children}</m:{name}></m:oMath><w:r><w:t>tail</w:t></w:r></w:p>");
        let doc = parse_parts(&document_parts(&body, &[])).expect("foreign children recover");
        let Block::Paragraph(p) = &doc.body.blocks[0] else {
            panic!("paragraph disappeared")
        };
        let expression = p
            .inlines
            .iter()
            .find_map(Inline::as_math)
            .expect("formula retained");
        assert!(
            expression.text().contains('x'),
            "{name}: argument disappeared"
        );
        assert!(
            !expression.text().contains("discarded"),
            "{name}: foreign text leaked"
        );
        assert!(p.inlines.iter().any(|inline| matches!(inline, Inline::Run(run) if run.content.iter().any(|c| matches!(c, RunContent::Text(t) if t.text == "tail")))));
        assert!(
            !doc.support.is_empty(),
            "{name}: skipped children must be reported"
        );
    }
}
