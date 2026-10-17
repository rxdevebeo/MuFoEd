//! Behavioural import contracts for the LaTeX importer.
#![allow(clippy::many_single_char_names, clippy::default_trait_access)]

use strict_ooxml_latex::{
    import, import_with_source, LatexError, LatexOptions, MemorySource, UnsupportedPolicy,
};
use strict_ooxml_wml::model::{Block, Inline, MathNode, RunContent, Twips, VerticalMerge};

fn paragraphs(input: &[u8]) -> Vec<String> {
    let imported = import(input, &LatexOptions::default()).unwrap();
    imported
        .document
        .body
        .blocks
        .iter()
        .map(|block| {
            let Block::Paragraph(paragraph) = block else {
                panic!("paragraph expected");
            };
            paragraph
                .inlines
                .iter()
                .map(|inline| {
                    let Inline::Run(run) = inline else {
                        return String::new();
                    };
                    run.content
                        .iter()
                        .map(|content| match content {
                            RunContent::Text(text) => text.text.clone(),
                            RunContent::Tab => "\t".to_owned(),
                            RunContent::Break(_) => "\n".to_owned(),
                            _ => String::new(),
                        })
                        .collect::<String>()
                })
                .collect()
        })
        .collect()
}

#[test]
fn accents_compose_to_precomposed_letters() {
    assert_eq!(paragraphs(b"caf\\'{e}"), ["caf\u{00E9}"]);
    assert_eq!(paragraphs(b"M\\\"{u}ller"), ["M\u{00FC}ller"]);
    assert_eq!(paragraphs(b"na\\\"{\\i}ve"), ["na\u{00EF}ve"]);
}

#[test]
fn colors_and_font_sizes_apply() {
    let imported = import(
        b"\\textcolor{red}{warm} and {\\Large big} and \\color{blue}tail",
        &LatexOptions::default(),
    )
    .unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    let runs: Vec<_> = paragraph
        .inlines
        .iter()
        .filter_map(Inline::as_run)
        .collect();
    assert!(runs.iter().any(|run| run
        .props
        .color
        .as_ref()
        .is_some_and(|c| c.as_str() == "FF0000")));
    assert!(runs
        .iter()
        .any(|run| run.props.size == Some(strict_ooxml_wml::model::HalfPoints(28))));
    assert!(runs.iter().any(|run| run
        .props
        .color
        .as_ref()
        .is_some_and(|c| c.as_str() == "0000FF")));
}

#[test]
fn definecolor_rgb_and_html_models() {
    let imported = import(
        b"\\definecolor{brand}{HTML}{336699}\\definecolor{grass}{rgb}{0,1,0}\\textcolor{brand}{x}\\textcolor{grass}{y}",
        &LatexOptions::default(),
    )
    .unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    let colors: Vec<_> = paragraph
        .inlines
        .iter()
        .filter_map(Inline::as_run)
        .filter_map(|run| run.props.color.as_ref().map(|c| c.as_str().to_owned()))
        .collect();
    assert!(colors.contains(&"336699".to_owned()));
    assert!(colors.contains(&"00FF00".to_owned()));
}

#[test]
fn verb_and_verbatim_preserve_specials() {
    assert_eq!(paragraphs(b"\\verb|\\foo % bar|"), ["\\foo % bar"]);
    let imported = import(
        b"\\begin{verbatim}\n\\foo % bar\n\\end{verbatim}",
        &LatexOptions::default(),
    )
    .unwrap();
    let text = imported
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .flat_map(|p| p.inlines.iter())
        .filter_map(Inline::as_run)
        .flat_map(|run| run.content.iter())
        .filter_map(|content| match content {
            RunContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(text.contains("\\foo % bar"), "got {text:?}");
}

#[test]
fn plain_text_and_paragraphs() {
    assert_eq!(
        paragraphs(b"First paragraph.\n\nSecond paragraph."),
        ["First paragraph.", "Second paragraph."]
    );
}

#[test]
fn formatting_commands_apply_character_properties() {
    let imported = import(
        b"plain \\textbf{bold} \\emph{italic} \\texttt{mono}",
        &LatexOptions::default(),
    )
    .unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    let runs: Vec<_> = paragraph
        .inlines
        .iter()
        .filter_map(Inline::as_run)
        .collect();
    assert!(runs.iter().any(|run| run.props.bold.is_on()));
    assert!(runs.iter().any(|run| run.props.italic.is_on()));
    assert!(runs
        .iter()
        .any(|run| run.props.fonts.as_ref().is_some_and(|f| f.ascii.is_some())));
    assert_eq!(imported.report.loss_count, 0);
}

#[test]
fn sections_become_outlined_headings() {
    let imported = import(b"\\section{Intro}\n\nBody text.", &LatexOptions::default()).unwrap();
    let Block::Paragraph(heading) = &imported.document.body.blocks[0] else {
        panic!("heading paragraph expected");
    };
    assert_eq!(heading.props.outline_level, Some(0));
    assert_eq!(
        heading
            .props
            .style
            .as_ref()
            .map(strict_ooxml_wml::model::StyleId::as_str),
        Some("Heading1")
    );
    assert_eq!(
        paragraphs(b"\\section{A}\\section{B}"),
        ["1\u{A0}A", "2\u{A0}B"]
    );
}

#[test]
fn starred_sections_are_unnumbered() {
    assert_eq!(paragraphs(b"\\section*{Intro}"), ["Intro"]);
}

#[test]
fn table_of_contents_becomes_a_toc_field() {
    let imported = import(b"\\tableofcontents", &LatexOptions::default()).unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    let instructions: Vec<String> = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Field(field) => field.instruction.as_ref().map(ToString::to_string),
            _ => None,
        })
        .collect();
    assert!(instructions.iter().any(|i| i.contains("TOC")));
}

#[test]
fn page_footer_and_twocolumn_are_applied() {
    let imported = import(b"\\twocolumn\nSome text.", &LatexOptions::default()).unwrap();
    assert_eq!(imported.document.headers_footers.len(), 1);
    assert!(!imported.document.headers_footers[0].is_header);
    let section = &imported.document.sections[0];
    assert_eq!(
        section.properties.columns.as_ref().and_then(|c| c.count),
        Some(2)
    );
    assert_eq!(section.properties.footers.len(), 1);
    let written =
        strict_ooxml_write::write_package(&imported.document, None, &Default::default()).unwrap();
    assert!(written.part_count >= 3);
}

#[test]
fn styles_table_carries_headings() {
    let imported = import(b"\\section{S}", &LatexOptions::default()).unwrap();
    assert!(imported
        .document
        .styles
        .get(&strict_ooxml_wml::model::StyleId::new("Heading1"))
        .is_some());
}

#[test]
fn lists_use_real_numbering_definitions() {
    let text = b"\\begin{itemize}\n\\item one\n\\item two\n\\end{itemize}";
    let imported = import(text, &LatexOptions::default()).unwrap();
    assert!(!imported.document.numbering.is_empty());
    let items: Vec<_> = imported
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .collect();
    assert_eq!(items.len(), 2);
    for item in &items {
        assert!(item.props.numbering.is_some(), "item is numbered");
    }
    assert_eq!(paragraphs(text), ["one", "two"]);

    let nested = import(
        b"\\begin{enumerate}\\item a\\begin{itemize}\\item x\\end{itemize}\\end{enumerate}",
        &LatexOptions::default(),
    )
    .unwrap();
    let levels: Vec<u8> = nested
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .filter_map(|p| p.props.numbering.as_ref())
        .filter_map(|n| n.ilvl.map(|i| i.0))
        .collect();
    assert_eq!(levels, [0, 1]);
}

#[test]
fn description_label_replaces_marker() {
    let imported = import(
        b"\\begin{description}\\item[Term] definition\\end{description}",
        &LatexOptions::default(),
    )
    .unwrap();
    let lines = paragraphs(b"\\begin{description}\\item[Term] definition\\end{description}");
    assert!(lines[0].starts_with("Term"), "got {:?}", lines[0]);
    let _ = imported;
}

#[test]
fn inline_math_becomes_an_omml_expression() {
    let imported = import(b"Area $\\pi r^2$.", &LatexOptions::default()).unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Math(_))));
}

#[test]
fn display_math_becomes_a_math_paragraph() {
    let imported = import(b"\\[\\frac{a}{b}\\]", &LatexOptions::default()).unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    let math = paragraph
        .inlines
        .iter()
        .find_map(Inline::as_math_paragraph)
        .expect("display math");
    let expression = math.expression().expect("equation");
    assert!(matches!(
        expression.nodes.first(),
        Some(MathNode::Fraction(_))
    ));
}

#[test]
fn align_environment_produces_equation_array() {
    let imported = import(
        b"\\begin{align}x &= 1 \\\\ y &= 2\\end{align}",
        &LatexOptions::default(),
    )
    .unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    let math = paragraph
        .inlines
        .iter()
        .find_map(Inline::as_math_paragraph)
        .expect("display math");
    let expression = math.expression().expect("equation");
    assert!(matches!(
        expression.nodes.first(),
        Some(MathNode::EquationArray(_))
    ));
}

#[test]
fn tabular_becomes_a_table() {
    let imported = import(
        b"\\begin{tabular}{ll}a & b \\\\ c & d\\end{tabular}",
        &LatexOptions::default(),
    )
    .unwrap();
    let table = imported
        .document
        .body
        .blocks
        .iter()
        .find_map(Block::as_table)
        .expect("table");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.grid.len(), 2);
}

#[test]
fn multicolumn_sets_grid_span() {
    let imported = import(
        b"\\begin{tabular}{ll}a & b \\\\ \\multicolumn{2}{c}{wide}\\end{tabular}",
        &LatexOptions::default(),
    )
    .unwrap();
    let table = imported
        .document
        .body
        .blocks
        .iter()
        .find_map(Block::as_table)
        .unwrap();
    let spanned = &table.rows[1].cells[0];
    assert_eq!(spanned.props.grid_span, Some(2));
}

#[test]
fn multirow_creates_vertical_merge() {
    let imported = import(
        b"\\begin{tabular}{ll}\\multirow{2}{*}{x} & a \\\\ & b\\end{tabular}",
        &LatexOptions::default(),
    )
    .unwrap();
    let table = imported
        .document
        .body
        .blocks
        .iter()
        .find_map(Block::as_table)
        .unwrap();
    assert_eq!(
        table.rows[0].cells[0].props.vertical_merge,
        Some(VerticalMerge::Restart)
    );
    assert_eq!(
        table.rows[1].cells[0].props.vertical_merge,
        Some(VerticalMerge::Continue)
    );
}

#[test]
fn rules_and_colors_shape_borders_and_shading() {
    let imported = import(
        b"\\begin{tabular}{|l|l|}\\hline\\rowcolor{gray} a & \\cellcolor{red}b \\\\ \\hline c & d \\\\ \\hline\\end{tabular}",
        &LatexOptions::default(),
    )
    .unwrap();
    let table = imported
        .document
        .body
        .blocks
        .iter()
        .find_map(Block::as_table)
        .unwrap();
    let first = &table.rows[0].cells[0];
    assert!(first.props.borders.top.is_some(), "hline -> top border");
    assert!(
        first.props.borders.start.is_some(),
        "spec bar -> left border"
    );
    assert!(first.props.shading.is_some(), "rowcolor shades every cell");
    assert!(table.rows[0].cells[1].props.shading.is_some());
}

#[test]
fn paragraph_columns_carry_widths() {
    let imported = import(
        b"\\begin{tabular}{p{30mm}l}x & y\\end{tabular}",
        &LatexOptions::default(),
    )
    .unwrap();
    let table = imported
        .document
        .body
        .blocks
        .iter()
        .find_map(Block::as_table)
        .unwrap();
    let fixed = table.grid[0].width.map(Twips::value);
    assert_eq!(fixed, Some(1700));
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png
}

fn first_picture(imported: &strict_ooxml_latex::LatexImported) -> strict_ooxml_wml::model::Picture {
    for block in &imported.document.body.blocks {
        let Some(paragraph) = block.as_paragraph() else {
            continue;
        };
        for inline in &paragraph.inlines {
            if let Inline::Drawing(drawing) = inline {
                if let strict_ooxml_wml::model::DrawingKind::Inline(id) = &drawing.kind {
                    if let strict_ooxml_wml::model::Graphic::Picture(picture) = id.graphic.as_ref()
                    {
                        return picture.clone();
                    }
                }
            }
        }
    }
    panic!("no picture found");
}

#[test]
fn includegraphics_width_and_angle_are_applied() {
    let mut source = MemorySource::new();
    source.insert("pic.png", png_bytes(64, 32));
    let imported = import_with_source(
        "main.tex",
        b"\\includegraphics[width=2cm,angle=90]{pic}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    let picture = first_picture(&imported);
    let extent = picture.extent.expect("extent");
    // 2 cm = 720000 EMU; the height follows from the 2:1 aspect ratio.
    assert_eq!(extent.cx.value(), 720_000);
    assert_eq!(extent.cy.value(), 360_000);
    assert_eq!(picture.xfrm.and_then(|xfrm| xfrm.rot), Some(5_400_000));
}

#[test]
fn gif_and_bmp_are_recognized() {
    let mut gif = Vec::new();
    gif.extend_from_slice(b"GIF89a");
    gif.extend_from_slice(&10u16.to_le_bytes());
    gif.extend_from_slice(&20u16.to_le_bytes());
    let mut source = MemorySource::new();
    source.insert("anim.gif", gif);
    let imported = import_with_source(
        "main.tex",
        b"\\includegraphics{anim}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    let picture = first_picture(&imported);
    // 10x20 pixels at 9525 EMU/pixel.
    assert_eq!(picture.extent.unwrap().cx.value(), 95_250);
    assert_eq!(imported.media.len(), 1);
}

#[test]
fn graphicspath_directories_are_searched() {
    let mut source = MemorySource::new();
    source.insert("img/logo.png", png_bytes(16, 16));
    let imported = import_with_source(
        "main.tex",
        b"\\graphicspath{{img/}}\\includegraphics{logo}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(imported.media.len(), 1);
}

fn math_expression(input: &[u8]) -> strict_ooxml_wml::model::MathExpression {
    let imported = import(input, &LatexOptions::default()).unwrap();
    for block in &imported.document.body.blocks {
        let Some(paragraph) = block.as_paragraph() else {
            continue;
        };
        for inline in &paragraph.inlines {
            if let Inline::Math(expression) = inline {
                return expression.clone();
            }
        }
    }
    panic!("no inline math found");
}

fn expression_text(expression: &strict_ooxml_wml::model::MathExpression) -> String {
    expression.text()
}

#[test]
fn binom_and_infix_over_build_fractions() {
    assert!(matches!(
        math_expression(b"$\\binom{n}{k}$").nodes.first(),
        Some(MathNode::Delimiter(_))
    ));
    assert!(matches!(
        math_expression(b"${a \\over b}$").nodes.first(),
        Some(MathNode::Fraction(_))
    ));
}

#[test]
fn not_mathbb_and_styles() {
    assert!(expression_text(&math_expression(b"$\\not=$")).contains('\u{2260}'));
    assert!(expression_text(&math_expression(b"$\\mathbb{R}$")).contains('\u{211D}'));
    assert!(expression_text(&math_expression(b"$\\mathbb{NZ}$")).contains('\u{2115}'));
}

#[test]
fn stackrel_and_text_nesting() {
    assert!(matches!(
        math_expression(b"$x \\stackrel{?}{=} y$").nodes.get(1),
        Some(MathNode::Limit(_))
    ));
    let expression = math_expression(b"$\\text{if } x>0$");
    assert!(expression_text(&expression).starts_with("if "));
    // The `x>0` after the text is ordinary math.
    assert!(
        expression_text(&expression).contains('>')
            || expression_text(&expression).contains('\u{003E}')
    );
}

#[test]
fn align_sets_alignment_points() {
    let imported = import(
        b"\\begin{align}a &= b\\end{align}",
        &LatexOptions::default(),
    )
    .unwrap();
    let mut found = false;
    imported.document.body.blocks.iter().for_each(|block| {
        if let Some(paragraph) = block.as_paragraph() {
            for inline in &paragraph.inlines {
                if let Some(math) = inline.as_math_paragraph() {
                    if let Some(expression) = math.expression() {
                        expression.walk(&mut |node| {
                            if let MathNode::Run(run) = node {
                                if run.properties.alignment_point == Some(true) {
                                    found = true;
                                }
                            }
                        });
                    }
                }
            }
        }
    });
    assert!(found, "an alignment point should be present");
}

#[test]
fn labels_become_bookmarks_and_refs_become_fields() {
    let imported = import(
        b"\\label{sec:intro} see \\ref{sec:intro} and \\pageref{sec:intro}",
        &LatexOptions::default(),
    )
    .unwrap();
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected");
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::BookmarkStart(_))));
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::BookmarkEnd(_))));
    let instructions: Vec<String> = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Field(field) => field.instruction.as_ref().map(ToString::to_string),
            _ => None,
        })
        .collect();
    assert!(instructions.iter().any(|i| i.contains("REF sec_intro")));
    assert!(instructions.iter().any(|i| i.contains("PAGEREF sec_intro")));
}

#[test]
fn footnotemark_and_text_share_a_number() {
    let imported = import(
        b"alpha\\footnotemark beta\\footnotetext{shared note}",
        &LatexOptions::default(),
    )
    .unwrap();
    assert!(imported.document.footnotes.get(1).is_some());
    assert!(imported.document.source.footnotes.is_some());
}

#[test]
fn endnotes_use_the_endnotes_part() {
    let imported = import(b"text\\endnote{end}", &LatexOptions::default()).unwrap();
    assert!(!imported.document.endnotes.is_empty());
    assert!(imported.document.source.endnotes.is_some());
}

#[test]
fn input_is_resolved_through_the_provider() {
    let mut source = MemorySource::new();
    source.insert("part", b"Included body.".as_slice());
    let imported = import_with_source(
        "main.tex",
        b"Before. \\input{part}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(imported.report.included_files, 1);
    let text = imported
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .flat_map(|p| p.inlines.iter())
        .filter_map(Inline::as_run)
        .flat_map(|run| run.content.iter())
        .filter_map(|content| match content {
            RunContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(text.contains("Included body."), "got {text:?}");
}

#[test]
fn missing_include_is_a_loss_not_a_failure() {
    let imported = import(b"\\input{gone}", &LatexOptions::default()).unwrap();
    assert!(imported.report.loss_count >= 1);
    assert_eq!(imported.report.included_files, 0);
}

#[test]
fn footnotes_create_notes_with_separators() {
    let imported = import(b"Text\\footnote{Note body}.", &LatexOptions::default()).unwrap();
    assert!(imported.document.footnotes.len() >= 3);
    assert!(imported.document.footnotes.separator().is_some());
    assert!(imported.document.source.footnotes.is_some());
}

#[test]
fn unsupported_commands_are_reported_and_can_be_rejected() {
    let options = LatexOptions::default();
    let imported = import(b"\\mysterycommand{x}", &options).unwrap();
    assert!(imported
        .report
        .feature_counts
        .keys()
        .any(|key| key.contains("mysterycommand")));

    let strict = LatexOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..LatexOptions::default()
    };
    assert!(matches!(
        import(b"\\mysterycommand{x}", &strict),
        Err(LatexError::UnsupportedRejected { .. })
    ));
}

#[test]
fn rich_document_writes_through_the_writer() {
    let mut source = MemorySource::new();
    source.insert("part", b"included".as_slice());
    let input = b"\\section{S}\nPara with \\textbf{bold}, $x_1^2$, and a list:\n\
\\begin{itemize}\\item a\\item b\\end{itemize}\n\
\\begin{tabular}{ll}x & y\\end{tabular}\n\
\\input{part}\n\\footnote{n}";
    let imported =
        import_with_source("main.tex", input, &source, &LatexOptions::default()).unwrap();
    let written =
        strict_ooxml_write::write_package(&imported.document, None, &Default::default()).unwrap();
    assert!(!written.bytes.is_empty());
    assert!(written.part_count >= 2);
}

#[test]
fn image_is_embedded_and_written() {
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&32u32.to_be_bytes());
    png.extend_from_slice(&16u32.to_be_bytes());
    let mut source = MemorySource::new();
    source.insert("pic.png", png);
    let imported = import_with_source(
        "main.tex",
        b"\\includegraphics{pic}",
        &source,
        &LatexOptions::default(),
    )
    .unwrap();
    assert_eq!(imported.media.len(), 1);

    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &imported.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written =
        strict_ooxml_write::write_package(&imported.document, Some(&bag), &Default::default())
            .unwrap();
    assert!(!written.bytes.is_empty());
}

#[test]
fn unsupported_image_container_is_a_loss_not_a_failure() {
    // A PDF figure is a normal LaTeX `\includegraphics` target the bounded
    // importer cannot embed; it is a recorded loss, not a fatal error.
    let mut source = MemorySource::new();
    source.insert("fig.pdf", b"%PDF-1.7\n%....".as_slice());
    let imported = import_with_source(
        "main.tex",
        b"text \\includegraphics{fig} tail",
        &source,
        &LatexOptions::default(),
    )
    .expect("a PDF figure must not fail the import");
    assert_eq!(imported.media.len(), 0);
    assert!(
        imported
            .report
            .feature_counts
            .keys()
            .any(|feature| feature.contains("unsupported-container")),
        "the skipped figure should be named: {:?}",
        imported.report.feature_counts.keys().collect::<Vec<_>>()
    );

    let strict = LatexOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..LatexOptions::default()
    };
    assert!(matches!(
        import_with_source("main.tex", b"\\includegraphics{fig}", &source, &strict),
        Err(LatexError::UnsupportedRejected { .. })
    ));
}

#[test]
fn a_multibyte_label_does_not_panic() {
    // The bookmark-name cap must never cut a character in half. A key of
    // three-byte characters has no char boundary at byte 40.
    let key = "\u{6F22}".repeat(14);
    let input = format!("\\section{{T}}\\label{{{key}}} see \\ref{{{key}}}");
    let imported =
        import(input.as_bytes(), &LatexOptions::default()).expect("a multi-byte label must import");
    assert!(imported
        .report
        .feature_counts
        .keys()
        .any(|feature| feature.starts_with("label:") || feature.starts_with("cross-reference:")));
}
