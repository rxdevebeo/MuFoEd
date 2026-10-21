//! Desired contracts for defects reproduced by the `CC0_TEX` audit.
//!
//! All nine audited defects are active regression tests after remediation.
//! `cargo +1.92.0 test -p strict-ooxml-latex --test cc0_regressions`
//! See docs/history/audit-latex-2026-10-18/REPORT.md. No external corpus is required.
use strict_ooxml_latex::{import, LatexOptions};
use strict_ooxml_wml::model::{Block, Inline, MathExpression, MathNode, RunContent};

fn math(source: &str) -> MathExpression {
    let imported = import(source.as_bytes(), &LatexOptions::default()).unwrap();
    for block in &imported.document.body.blocks {
        if let Block::Paragraph(p) = block {
            for inline in &p.inlines {
                match inline {
                    Inline::Math(m) => return m.clone(),
                    Inline::MathParagraph(p) => return p.expression().unwrap().clone(),
                    _ => {}
                }
            }
        }
    }
    panic!("a mathematical expression is required");
}

fn text(source: &str) -> String {
    let imported = import(source.as_bytes(), &LatexOptions::default()).unwrap();
    let mut text = String::new();
    for block in &imported.document.body.blocks {
        if let Block::Paragraph(p) = block {
            for inline in &p.inlines {
                if let Inline::Run(run) = inline {
                    for c in &run.content {
                        if let RunContent::Text(t) = c {
                            text.push_str(&t.text);
                        }
                    }
                }
            }
        }
    }
    text
}

#[test]
fn control_braced_script_and_plain_radical_are_preserved() {
    let expression = math("$x_{i}+y$");
    let MathNode::Subscript(s) = &expression.nodes[0] else {
        panic!("subscript required")
    };
    assert_eq!(s.subscript.text(), "i");
    assert_eq!(expression.nodes[1].text(), "+y");
    let expression = math(r"$\sqrt{x}$");
    let MathNode::Radical(r) = &expression.nodes[0] else {
        panic!("radical required")
    };
    assert_eq!(r.radicand.text(), "x");
}

#[test]
fn control_parenthesis_in_preamble_does_not_swallow_body() {
    assert_eq!(
        text(
            r"\documentclass{article}\AtBeginDocument{\def\openbracket{(}}\begin{document}BODY\end{document}"
        ),
        "BODY"
    );
}

#[test]
fn aud01_literal_bracket_in_preamble_does_not_swallow_document() {
    assert_eq!(
        text(
            r"\documentclass{article}\AtBeginDocument{\def\openbracket{[}}\begin{document}BODY\end{document}"
        ),
        "BODY"
    );
}

#[test]
fn unmatched_skipped_group_is_a_loss_and_reject_policy_refuses_it() {
    use strict_ooxml_latex::UnsupportedPolicy;
    let source = br"\documentclass[option={]}]{article}\AtBeginDocument{unterminated";
    let result = import(source, &LatexOptions::default()).unwrap();
    assert!(result
        .report
        .feature_counts
        .contains_key("group:unterminated"));
    let options = LatexOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..Default::default()
    };
    assert!(import(source, &options).is_err());
}

#[test]
fn aud02_unbraced_script_consumes_one_tex_atom() {
    let expression = math("$x_i+y$");
    let MathNode::Subscript(s) = &expression.nodes[0] else {
        panic!("subscript required")
    };
    assert_eq!(s.subscript.text(), "i");
    assert_eq!(expression.nodes[1].text(), "+y");
}

#[test]
fn aud03_indexed_radical_keeps_degree_radicand_and_tail_separate() {
    let expression = math(r"$\sqrt[3]{x}+y$");
    let MathNode::Radical(r) = &expression.nodes[0] else {
        panic!("radical required")
    };
    assert_eq!(r.degree.text(), "3");
    assert_eq!(r.radicand.text(), "x");
    assert_eq!(expression.nodes[1].text(), "+y");
}

#[test]
fn single_atom_arguments_support_unicode_spaces_and_unbraced_fractions() {
    let expression = math("$x _ α +y$");
    let MathNode::Subscript(s) = &expression.nodes[0] else {
        panic!("subscript required")
    };
    assert_eq!(s.subscript.text(), "α");
    assert_eq!(expression.nodes[1].text(), "+y");
    let expression = math(r"$\frac12+\sqrt x+y$");
    let MathNode::Fraction(f) = &expression.nodes[0] else {
        panic!("fraction required")
    };
    assert_eq!(f.numerator.text(), "1");
    assert_eq!(f.denominator.text(), "2");
    let MathNode::Radical(r) = &expression.nodes[2] else {
        panic!("radical required")
    };
    assert_eq!(r.radicand.text(), "x");
}

#[test]
fn aud04_label_is_metadata_not_math_ink() {
    assert_eq!(math(r"$x\label{eq:test}$").text(), "x");
}

#[test]
fn aud05_math_spacing_does_not_print_dimension() {
    assert_eq!(math(r"$a\hspace{-9.0cm}b$").text(), "ab");
}

#[test]
fn aud06_array_column_preamble_is_not_cell_content() {
    let expression = math(r"$\begin{array}{cc}a&b\\c&d\end{array}$");
    let MathNode::Matrix(matrix) = &expression.nodes[0] else {
        panic!("matrix required")
    };
    assert_eq!(matrix.rows[0][0].text(), "a");
    assert_eq!(matrix.rows[0][1].text(), "b");
}

#[test]
fn aud07_nested_environment_does_not_drop_outer_tail() {
    let expression = math(r"$\begin{pmatrix}\begin{matrix}a&b\\c&d\end{matrix}&e\end{pmatrix}$");
    assert!(
        expression.text().contains('e'),
        "outer matrix's last cell is missing: {}",
        expression.text()
    );
    let MathNode::Delimiter(d) = &expression.nodes[0] else {
        panic!("outer delimiters")
    };
    let MathNode::Matrix(m) = &d.arguments[0].nodes[0] else {
        panic!("outer matrix")
    };
    assert_eq!(m.rows.len(), 1);
    assert_eq!(m.rows[0].len(), 2);
    assert_eq!(m.rows[0][1].text(), "e");
}

#[test]
fn aud08_layout_configuration_is_not_document_text() {
    assert_eq!(text(r"\setlength{\parindent}{0pt}Real text."), "Real text.");
}

#[test]
fn aud09_macro_definition_parameters_are_not_document_text() {
    assert_eq!(text(r"\def\foo#1{#1}Real text."), "Real text.");
}

#[test]
fn math_labels_become_bookmarks_in_every_math_entry_point() {
    for source in [
        r"$x\label{eq:x}$",
        r"$$x\label{eq:x}$$",
        r"\[x\label{eq:x}\]",
        r"\(x\label{eq:x}\)",
        r"\ensuremath{x\label{eq:x}}",
        r"\begin{align}x&=y\label{eq:x}\end{align}",
    ] {
        let imported = import(source.as_bytes(), &LatexOptions::default()).unwrap();
        let marks: Vec<_> = imported
            .document
            .body
            .blocks
            .iter()
            .filter_map(Block::as_paragraph)
            .flat_map(|p| &p.inlines)
            .filter(|i| matches!(i, Inline::BookmarkStart(_)))
            .collect();
        assert_eq!(marks.len(), 1, "{source}");
        assert_eq!(imported.report.loss_count, 0, "{source}");
        assert!(!math(source).text().contains("eq:x"));
        let written = strict_ooxml_write::write_package(
            &imported.document,
            None,
            &strict_ooxml_write::WriteOptions::default(),
        )
        .unwrap();
        written.report.verify_no_silent_loss().unwrap();
    }
}

#[test]
fn spacing_metadata_and_definitions_leave_adjacent_content_intact() {
    assert_eq!(math(r"$a\kern-2ptb$").text(), "ab");
    let matrix = math(r"$\begin{matrix}a&b\\[2pt]c&d\end{matrix}$");
    assert!(!matrix.text().contains("pt"));
    let MathNode::Matrix(m) = &matrix.nodes[0] else {
        panic!("matrix required")
    };
    assert_eq!(m.rows[1][0].text(), "c");
    assert_eq!(m.rows[1][1].text(), "d");
    assert_eq!(
        text(r"\setlength{\parindent}{0pt}{Real text.}"),
        "Real text."
    );
    assert_eq!(text(r"\newcommand\foo[1][a]{#1}Real text."), "Real text.");
    // An unknown text wrapper's real content must still be retained.
    assert_eq!(text(r"\unknownwrapper{Real text.}"), "Real text.");
}

#[test]
fn math_array_metadata_obeys_column_budget() {
    let mut options = LatexOptions::default();
    options.limits.table_columns = 2;
    assert!(matches!(
        import(br"$\begin{array}{ccc}a&b&c\end{array}$", &options),
        Err(strict_ooxml_latex::LatexError::LimitExceeded {
            limit: "table_columns",
            ..
        })
    ));
}

#[test]
fn array_position_and_column_alignment_are_metadata() {
    use strict_ooxml_wml::model::math::MathVerticalAlign;
    use strict_ooxml_wml::model::MathAlignment;
    let expression = math(r"$\begin{array}[t]{lr}a&b\end{array}$");
    let MathNode::Matrix(m) = &expression.nodes[0] else {
        panic!("matrix required")
    };
    assert_eq!(m.base_justification, Some(MathVerticalAlign::Top));
    assert_eq!(m.columns[0].justification, Some(MathAlignment::Left));
    assert_eq!(m.columns[1].justification, Some(MathAlignment::Right));
    assert_eq!(m.rows[0][0].text(), "a");
}
