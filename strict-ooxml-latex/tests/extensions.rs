//! Corpus follow-up: substitution, metadata, columns and resolved field caches.
use strict_ooxml_latex::{import, import_with_source, LatexOptions, MemorySource};
use strict_ooxml_wml::model::{Block, Inline, Justification, RunContent, TriState};

fn inlines(items: &[Inline]) -> String {
    items
        .iter()
        .map(|i| match i {
            Inline::Run(r) => r
                .content
                .iter()
                .filter_map(|c| {
                    if let RunContent::Text(t) = c {
                        Some(t.text.as_str())
                    } else {
                        None
                    }
                })
                .collect(),
            Inline::Field(f) => inlines(&f.inlines),
            Inline::Hyperlink(h) => inlines(&h.inlines),
            Inline::Math(m) => m.text(),
            Inline::MathParagraph(m) => m
                .expression()
                .map_or_else(String::new, strict_ooxml_wml::model::MathExpression::text),
            _ => String::new(),
        })
        .collect()
}

fn text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(p) => inlines(&p.inlines),
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| r.cells.iter())
                .map(|c| text(&c.blocks))
                .collect(),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn macros_are_expanded_in_math_and_text_with_default_and_unicode_atoms() {
    let r = import(r"\newcommand{\twice}[1]{#1+#1}\newcommand\greet[2][Hello]{#1 #2}\def\pair#1#2{#2#1}\greet{World} $\twice{x_i}$ \pair αβ".as_bytes(), &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.contains("Hello World"), "{t}");
    assert!(t.replace(' ', "").contains("xi+xi"), "{t}");
    assert!(t.contains("βα"), "{t}");
    assert!(!t.contains('#'));
}

#[test]
fn macros_respect_local_scope_and_global_definitions() {
    let r = import(
        br"\def\a{outer}{\def\a{inner}\a\gdef\b{global}}\a\b",
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "innerouterglobal");
}

#[test]
fn control_symbol_macros_and_bib_brace_quoted_text_keep_token_boundaries() {
    let r = import(br"\newcommand{\?}[1]{#1!}\?{ok}", &LatexOptions::default()).unwrap();
    assert_eq!(text(&r.document.body.blocks), "ok!");
    let mut source = MemorySource::new();
    source.insert("refs.bib", br#"@article{one, title={A " literal}, year=2026} @article(two, title={B (value)}, year=2025)"#.as_slice());
    let r = import_with_source(
        "main.tex",
        br"\cite{one,two}\bibliography{refs}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.starts_with("[1, 2]"), "{t}");
    assert!(t.contains("A \" literal"), "{t}");
    assert!(t.contains("B (value)"), "{t}");
    assert!(!r
        .report
        .feature_counts
        .contains_key("bibliography:unterminated-entry"));
}

#[test]
fn macro_can_expand_to_a_block_and_cycles_hit_a_budget() {
    let r = import(
        br"\newcommand\head[1]{\section{#1}}\head{Title} Body",
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(
        r.document.body.blocks[0]
            .as_paragraph()
            .unwrap()
            .props
            .outline_level,
        Some(0)
    );
    let mut options = LatexOptions::default();
    options.limits.tokens = 200;
    assert!(import(br"\def\loop{\again}\def\again{\loop}\loop", &options).is_err());
}

#[test]
fn local_packages_and_include_definitions_are_visible_in_source_order() {
    let mut source = MemorySource::new();
    source.insert(
        "own.sty",
        br"\ProvidesPackage{own}\newcommand\hello[1]{Hi #1}\endinput NEVER".as_slice(),
    );
    source.insert("part.tex", br"\def\after{after}".as_slice());
    let r = import_with_source("main.tex", br"\documentclass{article}\usepackage{own}\begin{document}\hello{Ada}\input{part}\after\end{document}", &source, &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.contains("Hi Ada"), "{t}");
    assert!(t.contains("after"), "{t}");
    assert!(!t.contains("NEVER"));
}

#[test]
fn package_dependencies_are_loaded_before_a_later_explicit_list_entry() {
    let mut source = MemorySource::new();
    source.insert("a.sty", br"\RequirePackage{b}\let\saved\B".as_slice());
    source.insert("b.sty", br"\newcommand\B{OK}".as_slice());
    let r = import_with_source(
        "main.tex",
        br"\documentclass{article}\usepackage{a,b}\begin{document}\saved\end{document}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "OK");
    assert_eq!(r.report.included_files, 2);
}

#[test]
fn elsarticle_keeps_title_authors_and_addresses_without_symbolic_keys() {
    let r = import(br"\documentclass{elsarticle}\begin{document}\begin{frontmatter}\title{Title}\author[affKey]{Alice}\address[affKey]{Institute}\begin{abstract}Summary\end{abstract}\end{frontmatter}Body\end{document}", &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    for expected in ["Title", "Alice1", "1 Institute", "Summary", "Body"] {
        assert!(t.contains(expected), "{t}");
    }
    assert!(!t.contains("affKey"));
    assert_eq!(r.report.loss_count, 0, "{:?}", r.report.issues);
}

#[test]
fn parameterized_column_types_apply_alignment_and_font_without_phantom_columns() {
    let r = import(br"\newcolumntype{L}[1]{>{\raggedright\arraybackslash\bfseries}p{#1}}\begin{tabular}{L{2cm}c}one&two\\three&four\end{tabular}", &LatexOptions::default()).unwrap();
    let table = r
        .document
        .body
        .blocks
        .iter()
        .find_map(|b| {
            if let Block::Table(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(table.grid.len(), 2);
    let p = table.rows[0].cells[0].blocks[0].as_paragraph().unwrap();
    assert_eq!(p.props.alignment, Some(Justification::Start));
    assert!(p
        .inlines
        .iter()
        .any(|i| matches!(i, Inline::Run(r) if r.props.bold == TriState::On)));
    assert_eq!(text(&r.document.body.blocks), "onetwothreefour");
}

#[test]
fn forward_section_equation_and_name_refs_are_cached_as_values() {
    let r = import(br"See \ref{sec:k} \nameref{sec:k} and \eqref{eq:k}.\section{Title}\label{sec:k}\begin{equation}x=1\label{eq:k}\end{equation}", &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.starts_with("See 1 Title and (1)."), "{t}");
    assert!(!t.contains("sec:k"));
}

#[test]
fn figure_and_table_references_have_their_own_counters() {
    let r = import(br"\ref{fig:two} / \ref{tab:one}\section{Title}\begin{figure}\caption{First}\end{figure}\begin{figure}\caption{Second}\label{fig:two}\end{figure}\begin{table}\caption{Table}\label{tab:one}\end{table}", &LatexOptions::default()).unwrap();
    assert!(text(&r.document.body.blocks).starts_with("2 / 1"));
}

#[test]
fn math_aliases_keep_original_symbols_and_identity_macros_do_not_add_ast_depth() {
    let mut options = LatexOptions::default();
    options.limits.nesting = 6;
    let nested = format!("${}x{}$", r"\ensuremath{".repeat(30), "}".repeat(30));
    let r = import(nested.as_bytes(), &options).unwrap();
    assert_eq!(text(&r.document.body.blocks), "x");
    let r = import(br"\let\oldalpha\alpha\renewcommand\alpha{\ensuremath{\oldalpha}}$\alpha$ \texorpdfstring{Visible}{PRIVATE_PDF_METADATA}", &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.contains('α'), "{t}");
    assert!(t.contains("Visible"), "{t}");
    assert!(!t.contains("PRIVATE_PDF_METADATA"));
    let r = import(
        br"$\text{value \ensuremath{x_i}}+y$",
        &LatexOptions::default(),
    )
    .unwrap();
    let p = r.document.body.blocks[0].as_paragraph().unwrap();
    let m = p.inlines.iter().find_map(Inline::as_math).unwrap();
    assert!(m
        .nodes
        .iter()
        .any(|n| matches!(n, strict_ooxml_wml::model::MathNode::Subscript(_))));
    assert!(!text(&r.document.body.blocks).contains("ensuremath"));
}

#[test]
fn auxiliary_page_cache_is_data_and_bbl_citations_resolve_forward() {
    let mut source = MemorySource::new();
    source.insert(
        "main.aux",
        br"\newlabel{sec:k}{{3}{12}{Title}{section.3}{}}\write18{NEVER}".as_slice(),
    );
    source.insert("main.bbl", br"\begin{thebibliography}{9}\bibitem{one} Alice. A title.\bibitem{two} Bob. Other.\end{thebibliography}".as_slice());
    let r = import_with_source(
        "main.tex",
        br"Page \pageref{sec:k}; \cite{one,two}.\bibliography{refs}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.contains("Page 12; [1, 2]"), "{t}");
    assert!(t.contains("Alice. A title."), "{t}");
    assert!(!t.contains("NEVER"));
    assert!(!t.contains("__bib"));
}

#[test]
fn unresolved_references_use_question_marks_and_loss_never_raw_keys() {
    let r = import(
        br"\ref{private-key}\pageref{private-key}",
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "????");
    assert_eq!(r.report.loss_count, 2);
}

#[test]
fn bib_database_is_read_as_data_with_an_explicit_normalized_style() {
    let mut source = MemorySource::new();
    source.insert(
        "refs.bib",
        br"@article{one, author={Alice}, title={A {Nested} Title}, year=2026, journal={Journal}}"
            .as_slice(),
    );
    let r = import_with_source(
        "main.tex",
        br"\cite{one}\bibliography{refs}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.starts_with("[1]"), "{t}");
    assert!(t.contains("A Nested Title"), "{t}");
    assert!(t.contains("2026"), "{t}");
    assert!(r
        .report
        .feature_counts
        .contains_key("bibliography:normalized-style"));
}

#[test]
fn opaque_graphics_do_not_print_their_program_as_document_text() {
    let r = import(
        br"before\begin{tikzpicture}\draw (0,0)--(1,1);\end{tikzpicture}\chemfig{H-C-H}after",
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "beforeafter");
    assert_eq!(r.report.loss_count, 2);
}

#[test]
fn aliases_snapshot_meaning_and_boolean_branches_do_not_leak_dead_definitions() {
    let r = import(br"\def\a{old}\let\saved=\a\def\a{new}\newif\ifdraft\ifdraft\def\x{BAD}\else\def\x{good}\fi\saved\a\x\drafttrue\ifdraft yes\else BAD\fi", &LatexOptions::default()).unwrap();
    assert_eq!(text(&r.document.body.blocks), "oldnewgood yes");
    let r = import(
        br"\let\oldbold\textbf\renewcommand\textbf[1]{\oldbold{#1}}\textbf{bold}",
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "bold");
    assert_eq!(r.report.loss_count, 0);
}

#[test]
fn package_internal_names_and_custom_environments_preserve_parameters() {
    let mut source = MemorySource::new();
    source.insert("own.sty", br"\newcommand\own@inner[1]{#1+#1}\newcommand\own[1]{\own@inner{#1}}\newenvironment{greeting}[1]{\textbf{#1}: }{.}".as_slice());
    let r = import_with_source("main.tex", br"\documentclass{article}\usepackage{own}\begin{document}\own{x}\begin{greeting}{Hello}Body\end{greeting}\end{document}", &source, &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.contains("x+x"), "{t}");
    assert!(t.contains("Hello: Body."), "{t}");
    assert!(!t.contains('@'));
}

#[test]
fn correspondence_keys_are_notes_and_contacts_follow_title() {
    let r = import(br"\documentclass{elsarticle}\begin{document}\begin{frontmatter}\title{Title}\author[a]{Alice\corref{private-cor-key}}\ead{alice@example.org}\cortext[private-cor-key]{Corresponding author.}\address[a]{Institute}\begin{abstract}Summary\end{abstract}\end{frontmatter}\end{document}", &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.starts_with("TitleAlice"), "{t}");
    assert!(!t.contains("private-cor-key"));
    assert!(t.contains("alice@example.org"));
    assert!(r
        .document
        .footnotes
        .iter()
        .any(|n| text(&n.blocks).contains("Corresponding author.")));
    let written = strict_ooxml_write::write_package(
        &r.document,
        None,
        &strict_ooxml_write::WriteOptions::default(),
    )
    .unwrap();
    assert!(written.report.verify_no_silent_loss().is_ok());
}

#[test]
fn deep_or_unknown_column_specs_and_primitive_conditions_remain_explicit_losses() {
    let r = import(br"before\ifnum 1=2 BAD\else BAD\fi after\begin{tabular}{>{\bfseries}c@{--}Z}x&y\end{tabular}", &LatexOptions::default()).unwrap();
    assert!(!text(&r.document.body.blocks).contains("BAD"));
    assert!(r
        .report
        .feature_counts
        .contains_key("conditional:requires-tex"));
    assert!(r
        .report
        .feature_counts
        .contains_key("table:column-spec:unknown-Z"));
    let mut options = LatexOptions::default();
    options.limits.nesting = 4;
    assert!(import(
        br"\newcolumntype{Z}{Z}\begin{tabular}{Z}x\end{tabular}",
        &options
    )
    .is_err());
}

#[test]
fn engine_packages_do_not_partially_redefine_native_importer_commands() {
    let mut source = MemorySource::new();
    source.insert(
        "advanced.sty",
        br"\renewcommand\textbf[1]{BROKEN}\def\extra#1{\afterassignment\extra#1}".as_slice(),
    );
    let r = import_with_source(
        "main.tex",
        br"\documentclass{article}\usepackage{advanced}\begin{document}\textbf{Body}\end{document}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "Body");
    assert!(r
        .report
        .feature_counts
        .contains_key("package:requires-tex:advanced"));
}

#[test]
fn float_placement_and_natbib_long_author_metadata_are_not_prose() {
    let r = import(br"\citep{one}\begin{figure}[!htbp]Content\end{figure}\begin{thebibliography}{9}\bibitem[Short(2026)Long, Author, List]{one}Actual bibliography.\end{thebibliography}", &LatexOptions::default()).unwrap();
    let t = text(&r.document.body.blocks);
    assert!(t.starts_with("[Short, 2026]"), "{t}");
    assert!(!t.contains("htbp"), "{t}");
    assert!(!t.contains("Long, Author"), "{t}");
    assert!(t.contains("Actual bibliography."));
}

#[test]
fn tagged_bibliography_fields_choose_content_without_printing_field_names() {
    let r = import(br"\makeatletter\providecommand\bibinfo{\@secondoftwo}\providecommand\bibAnnoteFile{\@gobbletwo}\bibinfo{author}{Alice}\bibinfo{title}{Visible}\bibAnnoteFile{NoStop}{private-key}\makeatother", &LatexOptions::default()).unwrap();
    assert_eq!(text(&r.document.body.blocks), "AliceVisible");
    let r = import(
        br"\makeatletter\def\bad{\bad}\@secondoftwo{\bad}{Good}\makeatother",
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(text(&r.document.body.blocks), "Good");
}

#[test]
#[ignore = "requires LATEX_TEST_TECTONIC and LATEX_TEST_PDFTOPPM; real external toolchain"]
fn real_tikz_and_chemfig_are_embedded_as_rendered_pictures() {
    use strict_ooxml_latex::{FsSource, RenderingSource, TectonicRenderer};
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/latex-extended-probes");
    let source = RenderingSource {
        source: FsSource::new(&output),
        renderer: TectonicRenderer {
            tectonic: std::env::var_os("LATEX_TEST_TECTONIC")
                .expect("Tectonic path")
                .into(),
            pdftoppm: std::env::var_os("LATEX_TEST_PDFTOPPM")
                .expect("Poppler path")
                .into(),
            temporary_root: output.join("jobs"),
            timeout: std::time::Duration::from_secs(45),
            only_cached: true,
        },
    };
    let r = import_with_source("main.tex", br"Before\begin{tikzpicture}\draw[->] (0,0)--(2,1);\node at (1,1) {A};\end{tikzpicture}\chemfig{H-C(-[2]H)(-[6]H)-O-H}After$\chemfig{H-O-H}$", &source, &LatexOptions::default()).unwrap();
    assert_eq!(r.media.len(), 3, "{:?}", r.report.issues);
    assert_eq!(text(&r.document.body.blocks), "BeforeAfter");
    assert_eq!(r.report.feature_counts["graphic:rasterized"].loss_count, 3);
    for (i, (_, bytes)) in r.media.iter().enumerate() {
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(bytes.len() > 100);
        std::fs::write(output.join(format!("graphic-{i}.png")), bytes).unwrap();
    }
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &r.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let package = strict_ooxml_write::write_package(
        &r.document,
        Some(&bag),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .unwrap();
    std::fs::write(output.join("graphics.docx"), package.bytes).unwrap();
    std::fs::write(output.join("resource.pdf"), simple_pdf()).unwrap();
    let r = import_with_source(
        "main.tex",
        br"\includegraphics{resource.pdf}After",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(r.media.len(), 1, "{:?}", r.report.issues);
    assert!(r.report.feature_counts.contains_key("image:rasterized"));
    std::fs::write(output.join("pdf-resource.png"), &r.media[0].1).unwrap();
    let drawing = r
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .flat_map(|p| &p.inlines)
        .find_map(|i| {
            if let Inline::Drawing(d) = i {
                Some(d)
            } else {
                None
            }
        })
        .unwrap();
    if let strict_ooxml_wml::model::DrawingKind::Inline(d) = &drawing.kind {
        // The PDF is 120x60 pt, independent of the bounded raster resolution.
        assert!((d.extent.as_ref().unwrap().cx.0 - 1_524_000).abs() < 1000);
        assert!((d.extent.as_ref().unwrap().cy.0 - 762_000).abs() < 1000);
    } else {
        panic!("an inline picture is required");
    }
}

fn simple_pdf() -> Vec<u8> {
    use std::fmt::Write;
    let stream = "1 0 0 rg 10 10 100 40 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 60] /Contents 4 0 R /Resources << >> >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        writeln!(pdf, "{} 0 obj\n{object}\nendobj", i + 1).unwrap();
    }
    let xref = pdf.len();
    pdf.push_str("xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        writeln!(pdf, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        pdf,
        "trailer\n<< /Root 1 0 R /Size 5 >>\nstartxref\n{xref}\n%%EOF\n"
    )
    .unwrap();
    pdf.into_bytes()
}
