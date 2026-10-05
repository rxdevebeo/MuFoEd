//! Behavioral import contracts independent of lexical implementation.
#![allow(clippy::many_single_char_names, clippy::default_trait_access)]
use strict_ooxml_rtf::*;
use strict_ooxml_wml::model::{Block, Inline, RunContent, TriState};

fn paragraphs(input: &[u8]) -> Vec<String> {
    let imported = import(input, &RtfOptions::default()).unwrap();
    imported
        .document
        .body
        .blocks
        .iter()
        .map(|block| {
            let Block::Paragraph(p) = block else {
                panic!("paragraph expected")
            };
            p.inlines
                .iter()
                .map(|inline| {
                    let Inline::Run(r) = inline else {
                        panic!("run expected")
                    };
                    r.content
                        .iter()
                        .map(|c| match c {
                            RunContent::Text(t) => t.text.clone(),
                            RunContent::Tab => "\t".into(),
                            RunContent::Break(_) => "\n".into(),
                            _ => panic!("unexpected content"),
                        })
                        .collect::<String>()
                })
                .collect()
        })
        .collect()
}

#[test]
fn text_paragraphs_and_escapes() {
    assert_eq!(
        paragraphs(br"{\rtf1 one\par two\tab \{x\}\\\par}"),
        ["one", "two\t{x}\\"]
    );
}
#[test]
fn scoped_bold() {
    let x = import(br"{\rtf1 A{\b B}C}", &RtfOptions::default()).unwrap();
    let Block::Paragraph(p) = &x.document.body.blocks[0] else {
        panic!()
    };
    let flags: Vec<_> = p
        .inlines
        .iter()
        .map(|i| match i {
            Inline::Run(r) => r.props.bold,
            _ => panic!(),
        })
        .collect();
    assert_eq!(flags, [TriState::Absent, TriState::On, TriState::Absent]);
}
#[test]
fn unicode_fallback_and_surrogates() {
    assert_eq!(
        paragraphs(br"{\rtf1\uc1 \u1046? \u-10179?\u-8704?}"),
        ["Ж 😀"]
    );
}
#[test]
fn cyrillic_codepage() {
    assert_eq!(
        paragraphs(br"{\rtf1\ansi\ansicpg1251 \'cf\'f0\'e8\'e2\'e5\'f2}"),
        ["Привет"]
    );
}
#[test]
fn ignored_destination_and_binary_are_atomic() {
    assert_eq!(paragraphs(br"{\rtf1 a{\*\private \bin4 }{\x}b}"), ["ab"]);
}
#[test]
fn rejects_truncation() {
    assert!(matches!(
        import(br"{\rtf1 text", &RtfOptions::default()),
        Err(RtfError::Malformed { .. })
    ));
}
#[test]
fn loss_details_cap_does_not_clear_losses() {
    let mut opts = RtfOptions::default();
    opts.limits.issues = 0;
    let x = import(br"{\rtf1\unknown a}", &opts).unwrap();
    assert_eq!(x.report.loss_count, 1);
    assert!(x.report.details_truncated);
}
#[test]
fn depth_limit() {
    let mut opts = RtfOptions::default();
    opts.limits.depth = 1;
    assert!(matches!(
        import(br"{\rtf1 {x}}", &opts),
        Err(RtfError::LimitExceeded { limit: "depth", .. })
    ));
}

#[test]
fn font_charset_and_alternate_names() {
    let x=import(br"{\rtf1\ansi\deff0{\fonttbl{\f0\fcharset204{\*\falt Arial}Times New Roman;}}\f0 \'cf\'f0}",&RtfOptions::default()).unwrap();
    let Block::Paragraph(p) = &x.document.body.blocks[0] else {
        panic!()
    };
    let Inline::Run(r) = &p.inlines[0] else {
        panic!()
    };
    assert_eq!(
        r.props.fonts.as_ref().unwrap().ascii.as_deref(),
        Some("Times New Roman")
    );
    assert_eq!(
        paragraphs(br"{\rtf1\ansi\deff0{\fonttbl{\f0\fcharset204 Times New Roman;}}\f0 \'cf\'f0}"),
        ["Пр"]
    );
}
#[test]
fn colors_geometry_and_indentation() {
    let x=import(br"{\rtf1{\colortbl;\red255\green128\blue0;}\paperw12240\paperh15840\margl1440\li720\cf1 orange}",&RtfOptions::default()).unwrap();
    let Block::Paragraph(p) = &x.document.body.blocks[0] else {
        panic!()
    };
    assert_eq!(p.props.indentation.unwrap().start.unwrap().0, 720);
    let Inline::Run(r) = &p.inlines[0] else {
        panic!()
    };
    assert_eq!(r.props.color.as_ref().unwrap().as_str(), "FF8000");
    assert_eq!(
        x.document.sections[0]
            .properties
            .page_size
            .unwrap()
            .width
            .unwrap()
            .0,
        12240
    );
}
#[test]
fn writer_roundtrip() {
    use strict_ooxml_core::opc::{OpenOptions, Package};
    use strict_ooxml_wml::{parse_document, ParseOptions};
    let x = import(br"{\rtf1 A{\b B}\par\u1046?}", &RtfOptions::default()).unwrap();
    let w = strict_ooxml_write::write_package(&x.document, None, &Default::default()).unwrap();
    strict_ooxml_write::verify_no_silent_loss(&w.report).unwrap();
    let package = Package::open_reader(&w.bytes[..], &OpenOptions::default()).unwrap();
    let d = parse_document(&package, &ParseOptions::default()).unwrap();
    assert_eq!(d.body.blocks.len(), 2);
    let Block::Paragraph(p) = &d.body.blocks[0] else {
        panic!()
    };
    let Inline::Run(r) = &p.inlines[1] else {
        panic!()
    };
    assert_eq!(r.props.bold, TriState::On);
    assert!(matches!(&r.content[0],RunContent::Text(t) if t.text=="B"));
    let Block::Paragraph(p) = &d.body.blocks[1] else {
        panic!()
    };
    let Inline::Run(r) = &p.inlines[0] else {
        panic!()
    };
    assert!(matches!(&r.content[0],RunContent::Text(t) if t.text=="Ж"));
}

#[test]
fn malformed_inputs_and_encoding_errors() {
    for bytes in [
        br"{\rtf1\bin-1 x}".as_slice(),
        br"{\rtf1\bin50 x}",
        br"{\rtf1\'xz}",
        br"{\rtf1\fs-2 x}",
        br"{\rtf1 x}junk",
        br"{\rtf1\uc-1 x}",
    ] {
        assert!(
            matches!(
                import(bytes, &RtfOptions::default()),
                Err(RtfError::Malformed { .. })
            ),
            "{bytes:?}"
        );
    }
    assert!(matches!(
        import(br"{\rtf1\u-10179?}", &RtfOptions::default()),
        Err(RtfError::InvalidEncoding { .. })
    ));
    assert!(matches!(
        import(br"{\rtf1\ansicpg932 \'82}", &RtfOptions::default()),
        Err(RtfError::InvalidEncoding { .. })
    ));
}
#[test]
fn replacement_is_a_loss() {
    let options = RtfOptions {
        invalid_text_policy: InvalidTextPolicy::ReplaceAndReport,
        ..Default::default()
    };
    let x = import(br"{\rtf1\u-10179?}", &options).unwrap();
    assert_eq!(x.report.loss_count, 1);
}
#[test]
fn reject_policy_and_trailing_nul() {
    let options = RtfOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..Default::default()
    };
    assert!(matches!(
        import(br"{\rtf1{\pict 00}x}", &options),
        Err(RtfError::UnsupportedRejected { .. })
    ));
    let x = import(b"{\\rtf1 x}\r\n\0", &options).unwrap();
    assert_eq!(x.report.loss_count, 0);
    assert_eq!(x.report.info_count, 1);
}
#[test]
fn every_resource_budget_is_enforced() {
    for budget in ["input_bytes", "tokens", "text_scalars", "nodes"] {
        let mut options = RtfOptions::default();
        match budget {
            "input_bytes" => options.limits.input_bytes = 0,
            "tokens" => options.limits.tokens = 2,
            "text_scalars" => options.limits.text_scalars = 1,
            _ => options.limits.nodes = 0,
        }
        assert!(
            matches!(import(br"{\rtf1 AB}",&options),Err(RtfError::LimitExceeded{limit,..}) if limit==budget)
        );
    }
}
#[test]
fn double_byte_hex_sequence_and_plain_reset() {
    assert_eq!(paragraphs(br"{\rtf1\ansicpg932 \'82\'a0}"), ["あ"]);
    let x = import(br"{\rtf1\b bold\plain normal}", &RtfOptions::default()).unwrap();
    let Block::Paragraph(p) = &x.document.body.blocks[0] else {
        panic!()
    };
    let Inline::Run(r) = &p.inlines[1] else {
        panic!()
    };
    assert_eq!(r.props.bold, TriState::Absent);
}
#[test]
fn bounded_adversarial_bytes_do_not_panic() {
    let mut seed = 1_234_567_u32;
    for len in 0..256 {
        let mut bytes = br"{\rtf1 ".to_vec();
        for _ in 0..len {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            bytes.push((seed >> 24) as u8);
        }
        let _ = import(&bytes, &RtfOptions::default());
    }
}

#[test]
fn table_cell_boundaries_and_empty_cells() {
    let x = import(
        br"{\rtf1 before\par\trowd\cellx2000\cellx4000\intbl A\cell\cell\row\pard after}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(x.document.body.blocks.len(), 3);
    let Block::Table(t) = &x.document.body.blocks[1] else {
        panic!()
    };
    assert_eq!(t.grid.len(), 2);
    assert_eq!(t.rows.len(), 1);
    assert_eq!(t.rows[0].cells.len(), 2);
    let Block::Paragraph(p) = &t.rows[0].cells[1].blocks[0] else {
        panic!()
    };
    assert!(p.inlines.is_empty());
    let w = strict_ooxml_write::write_package(&x.document, None, &Default::default()).unwrap();
    strict_ooxml_write::verify_no_silent_loss(&w.report).unwrap();
}
#[test]
fn table_errors_and_budgets() {
    assert!(matches!(
        import(br"{\rtf1\trowd\cellx100 A\row}", &RtfOptions::default()),
        Err(RtfError::Malformed { .. })
    ));
    let mut o = RtfOptions::default();
    o.limits.table_rows = 0;
    assert!(matches!(
        import(br"{\rtf1\trowd\cellx100 A\cell\row}", &o),
        Err(RtfError::LimitExceeded {
            limit: "table_rows",
            ..
        })
    ));
    assert!(matches!(
        import(br"{\rtf1\itap2}", &RtfOptions::default()),
        Err(RtfError::UnsupportedRejected { .. })
    ));
}

#[test]
fn unicode_alternate_destination_does_not_duplicate_text() {
    assert_eq!(
        paragraphs(br"{\rtf1{\upr{ANSI}{\*\ud Unicode\u1046?}}}"),
        ["UnicodeЖ"]
    );
    assert!(matches!(
        import(br"{\rtf1{\upr{ANSI}}}", &RtfOptions::default()),
        Err(RtfError::Malformed { .. })
    ));
}

#[test]
fn repeated_row_definition_keeps_cell_content() {
    let x = import(
        br"{\rtf1\trowd\cellx100 A\cell\trowd\cellx100\row}",
        &RtfOptions::default(),
    )
    .unwrap();
    let Block::Table(t) = &x.document.body.blocks[0] else {
        panic!()
    };
    assert_eq!(t.rows[0].cells.len(), 1);
    let Block::Paragraph(p) = &t.rows[0].cells[0].blocks[0] else {
        panic!()
    };
    let Inline::Run(r) = &p.inlines[0] else {
        panic!()
    };
    assert!(matches!(&r.content[0],RunContent::Text(t) if t.text=="A"));
}

#[test]
fn reused_row_definition_retains_table_structure() {
    let x = import(
        br"{\rtf1\trowd\cellx100\intbl A\cell\row\pard\intbl B\cell\row}",
        &RtfOptions::default(),
    )
    .unwrap();
    let Block::Table(t) = &x.document.body.blocks[0] else {
        panic!()
    };
    assert_eq!(t.rows.len(), 2);
}

#[test]
fn escaped_line_endings_are_paragraphs_and_raw_line_endings_are_ignored() {
    assert_eq!(paragraphs(b"{\\rtf1 a\\\nb\r\nc\\\rd}"), ["a", "bc", "d"]);
}

#[test]
fn binary_png_reaches_docx_without_byte_changes() {
    use strict_ooxml_core::opc::{OpenOptions, Package};
    let png = include_bytes!("fixtures/pixel.png");
    let mut input = format!(
        "{{\\rtf1 before{{\\pict\\pngblip\\picw1\\pich1\\picwgoal15\\pichgoal15\\bin{} ",
        png.len()
    )
    .into_bytes();
    input.extend_from_slice(png);
    input.extend_from_slice(b"}after}");
    let imported = import(&input, &RtfOptions::default()).unwrap();
    assert_eq!(imported.media.len(), 1);
    assert_eq!(imported.media[0].1, png);
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    bag.insert(imported.media[0].0.clone(), imported.media[0].1.clone());
    let written =
        strict_ooxml_write::write_package(&imported.document, Some(&bag), &Default::default())
            .unwrap();
    strict_ooxml_write::verify_no_silent_loss(&written.report).unwrap();
    let package = Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap();
    let reopened = strict_ooxml_wml::parse_document(&package, &Default::default()).unwrap();
    let image = reopened.media.iter().next().unwrap();
    assert_eq!(package.read_part(&image.part).unwrap(), png);
    let mut options = RtfOptions::default();
    options.limits.media_part_bytes = 1;
    assert!(matches!(
        import(&input, &options),
        Err(RtfError::LimitExceeded {
            limit: "media_part_bytes",
            ..
        })
    ));
}
#[test]
fn hex_png_and_malformed_picture_data() {
    use std::fmt::Write;
    let mut source = String::from("{\\rtf1{\\pict\\pngblip\\picwgoal15\\pichgoal15 ");
    for byte in include_bytes!("fixtures/pixel.png") {
        write!(&mut source, "{byte:02x}").unwrap();
    }
    source.push_str("}}");
    assert_eq!(
        import(source.as_bytes(), &RtfOptions::default())
            .unwrap()
            .media[0]
            .1,
        include_bytes!("fixtures/pixel.png")
    );
    assert!(matches!(
        import(
            br"{\rtf1{\pict\pngblip\picwgoal15\pichgoal15 123}}",
            &RtfOptions::default()
        ),
        Err(RtfError::Malformed { .. })
    ));
}

#[test]
fn paragraph_properties_after_text_apply_to_whole_paragraph() {
    let x = import(br"{\rtf1 text\qc\par}", &RtfOptions::default()).unwrap();
    let Block::Paragraph(p) = &x.document.body.blocks[0] else {
        panic!()
    };
    assert_eq!(
        p.props.alignment,
        Some(strict_ooxml_wml::model::Justification::Center)
    );
}

#[test]
fn unicode_alternate_requires_actual_unicode_destination() {
    assert!(matches!(
        import(br"{\rtf1{\upr{ansi}{other}}}", &RtfOptions::default()),
        Err(RtfError::Malformed { .. })
    ));
    assert_eq!(paragraphs(br"{\rtf1\uc1\u65\bin4 }{\x B}"), ["A B"]);
}
