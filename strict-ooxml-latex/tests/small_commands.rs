//! Small native commands, text inside OMML, and explicit local groups.
use strict_ooxml_latex::{
    import, import_with_source, LatexError, LatexImported, LatexOptions, MemorySource,
    UnsupportedPolicy,
};
use strict_ooxml_wml::model::{
    Block, Inline, MathExpression, MathNode, MathStyle, Run, RunContent, TriState,
};

fn load(source: &str) -> LatexImported {
    import(source.as_bytes(), &LatexOptions::default()).unwrap()
}

fn runs(doc: &LatexImported) -> Vec<&Run> {
    doc.document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .flat_map(|p| &p.inlines)
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn run_text(run: &Run) -> String {
    run.content
        .iter()
        .filter_map(|c| match c {
            RunContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect()
}

fn prose(doc: &LatexImported) -> String {
    runs(doc).iter().map(|r| run_text(r)).collect()
}

fn formula(doc: &LatexImported) -> &MathExpression {
    doc.document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .flat_map(|p| &p.inlines)
        .find_map(|i| match i {
            Inline::Math(m) => Some(m),
            Inline::MathParagraph(p) => p.expression(),
            _ => None,
        })
        .unwrap()
}

#[test]
fn small_symbols_keep_real_characters_and_script_binding() {
    let doc = load(r"file\textunderscore name $a\longrightarrow b\mid c\top_i$");
    assert_eq!(prose(&doc), "file_name ");
    let m = formula(&doc);
    assert_eq!(m.text(), "a⟶b∣c⊤ i");
    let MathNode::Subscript(s) = m.nodes.last().unwrap() else {
        panic!("script binding");
    };
    assert_eq!(s.base.text(), "⊤");
    assert_eq!(s.subscript.text(), "i");
    assert_eq!(doc.report.loss_count, 0);
}

#[test]
fn em_is_a_local_declaration_and_toggles_inside_emphasis() {
    let doc = load(r"A{\em B{\em C}D}E");
    let r = runs(&doc);
    let pieces: Vec<_> = r
        .iter()
        .map(|r| (run_text(r), r.props.italic == TriState::On))
        .collect();
    assert_eq!(
        pieces,
        vec![
            ("A".into(), false),
            ("B".into(), true),
            ("C".into(), false),
            ("D".into(), true),
            ("E".into(), false)
        ]
    );
    assert_eq!(doc.report.loss_count, 0);
}

#[test]
fn mathematical_text_composes_nested_fonts_and_removes_group_syntax() {
    let doc = load(r"$\text{a \textbf{B\textit{I}\textup{U}} \textit{C\emph{D}} e}$");
    let m = formula(&doc);
    assert_eq!(m.text(), "a BIU CD e");
    let pieces: Vec<_> = m
        .nodes
        .iter()
        .map(|n| {
            let MathNode::Run(r) = n else {
                panic!("text run");
            };
            assert!(r.properties.normal);
            let props = r.run_properties.as_deref().expect("normal text w:rPr");
            let style = r.properties.style.unwrap();
            assert_eq!(
                props.bold.is_on(),
                matches!(style, MathStyle::Bold | MathStyle::BoldItalic)
            );
            assert_eq!(
                props.italic.is_on(),
                matches!(style, MathStyle::Italic | MathStyle::BoldItalic)
            );
            (r.text.as_str(), r.properties.style.unwrap())
        })
        .collect();
    assert_eq!(
        pieces,
        vec![
            ("a ", MathStyle::Plain),
            ("B", MathStyle::Bold),
            ("I", MathStyle::BoldItalic),
            ("U", MathStyle::Bold),
            (" ", MathStyle::Plain),
            ("C", MathStyle::Italic),
            ("D e", MathStyle::Plain)
        ]
    );
    assert_eq!(doc.report.loss_count, 0);
    let written = strict_ooxml_write::write_package(
        &doc.document,
        None,
        &strict_ooxml_write::WriteOptions::default(),
    )
    .unwrap();
    written.report.verify_no_silent_loss().unwrap();
}

#[test]
fn mathematical_text_keeps_spaces_unicode_escapes_and_inner_math() {
    let doc = load(
        r"$\text{Имя: \{a\}\_\%\$\#\&\textunderscore~\textbackslash{} \ensuremath{x_i} и $y^2$}$",
    );
    assert_eq!(formula(&doc).text(), "Имя: {a}_%$#&_\u{00a0}\\ x i и y 2");
    assert_eq!(doc.report.loss_count, 0);
    assert!(formula(&doc)
        .nodes
        .iter()
        .any(|n| matches!(n, MathNode::Subscript(_))));
    assert!(formula(&doc)
        .nodes
        .iter()
        .any(|n| matches!(n, MathNode::Superscript(_))));
}

#[test]
fn text_font_commands_also_work_at_math_top_level_and_on_single_atoms() {
    let doc = load(r"$\textbf{A\textit{B}}+\textit{C}+\textup{D}+\text{\textbf αβ}$");
    assert_eq!(formula(&doc).text(), "AB+C+D+αβ");
    assert_eq!(doc.report.loss_count, 0);
    let m = formula(&doc);
    assert!(m.nodes.iter().any(|n| matches!(n, MathNode::Run(r) if r.text == "α" && r.properties.style == Some(MathStyle::Bold))));
}

#[test]
fn unsupported_text_commands_are_reported_without_printing_control_words() {
    let doc = load(r"$\text{A\unknown{B}C}$");
    assert_eq!(formula(&doc).text(), "ABC");
    assert!(doc
        .report
        .feature_counts
        .contains_key("math.text.command:unknown"));
    let options = LatexOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..LatexOptions::default()
    };
    assert!(matches!(
        import(br"$\text{\unknown{B}}$", &options),
        Err(LatexError::UnsupportedRejected { .. })
    ));
}

#[test]
fn explicit_groups_restore_macros_fonts_and_booleans_across_paragraphs() {
    let doc = load(
        r"\def\a{O}\newif\ifshow\showfalse\begingroup\def\a{I}\showtrue\itshape\a\par\a\ifshow Y\else N\fi\gdef\b{G}\endgroup\a\ifshow Y\else N\fi\b",
    );
    assert_eq!(prose(&doc).replace(' ', ""), "IIYONG");
    let r = runs(&doc);
    assert!(r
        .iter()
        .filter(|r| run_text(r).contains('I'))
        .all(|r| r.props.italic == TriState::On));
    assert!(r
        .iter()
        .filter(|r| run_text(r).contains('O'))
        .all(|r| r.props.italic != TriState::On));
    assert_eq!(doc.report.loss_count, 0);
}

#[test]
fn explicit_groups_interleave_with_braces_and_macro_expansion() {
    let doc = load(
        r"\def\a{O}\def\open{\begingroup}\def\close{\endgroup}\open\def\a{I}{\def\a{B}\a\begingroup\def\a{C}\a\endgroup\a}\a\close\a",
    );
    assert_eq!(prose(&doc), "BCBIO");
    assert_eq!(doc.report.loss_count, 0);
}

#[test]
fn groups_span_included_files_and_preamble_local_definitions_do_not_leak() {
    let mut source = MemorySource::new();
    source.insert("inner.tex", br"\def\a{I}\a\endgroup".as_slice());
    let doc = import_with_source("main.tex", br"\documentclass{article}\def\a{O}\begingroup\def\a{P}\gdef\b{G}\endgroup\begin{document}\a\begingroup\input{inner}\a\b\end{document}", &source, &LatexOptions::default()).unwrap();
    assert_eq!(prose(&doc), "OIOG");
    assert_eq!(doc.report.loss_count, 0);
}

#[test]
fn explicit_groups_restore_named_colors_and_alignment() {
    let doc = load(
        r"\definecolor{shade}{HTML}{112233}\begingroup\definecolor{shade}{HTML}{AABBCC}\centering\color{shade}I\par\endgroup\color{shade}O",
    );
    let r = runs(&doc);
    assert_ne!(r[0].props.color, r[1].props.color);
    let p: Vec<_> = doc
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .collect();
    assert_eq!(
        p[0].props.alignment,
        Some(strict_ooxml_wml::model::Justification::Center)
    );
    assert_eq!(p[1].props.alignment, None);
    assert_eq!(doc.report.loss_count, 0);
}

#[test]
fn malformed_groups_are_losses_and_do_not_steal_an_outer_brace_scope() {
    for (source, feature) in [
        (r"A\endgroup B", "group:unmatched-endgroup"),
        (r"\begingroup A", "group:unterminated-begingroup"),
        (r"{\begingroup A}B", "group:unterminated-begingroup"),
        (
            r"\begingroup{\endgroup A}\endgroup B",
            "group:unmatched-endgroup",
        ),
    ] {
        let doc = load(source);
        assert!(doc.report.feature_counts.contains_key(feature), "{source}");
        assert!(!prose(&doc).contains('\\'));
    }
}

#[test]
fn explicit_groups_and_nested_text_obey_nesting_limits() {
    let mut options = LatexOptions::default();
    options.limits.nesting = 8;
    for source in [
        format!("{}X{}", r"\begingroup".repeat(12), r"\endgroup".repeat(12)),
        format!("$\\text{{{}X{}}}$", r"\textbf{".repeat(12), "}".repeat(12)),
        format!("$\\text{{{}X{}}}$", "{".repeat(12), "}".repeat(12)),
    ] {
        assert!(
            matches!(
                import(source.as_bytes(), &options),
                Err(LatexError::LimitExceeded {
                    limit: "nesting",
                    ..
                })
            ),
            "{source}"
        );
    }
}

#[test]
fn environment_font_switches_do_not_escape_their_scope() {
    let doc = load(r"A\begin{center}\itshape B\par C\end{center}D");
    let r = runs(&doc);
    assert!(r
        .iter()
        .filter(|r| run_text(r).contains('B') || run_text(r).contains('C'))
        .all(|r| r.props.italic == TriState::On));
    assert!(r
        .iter()
        .filter(|r| run_text(r).contains('D'))
        .all(|r| r.props.italic != TriState::On));
    assert_eq!(doc.report.loss_count, 0);
}
