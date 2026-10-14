//! Behavioral import contracts independent of lexical implementation.
#![allow(clippy::many_single_char_names, clippy::default_trait_access)]
use strict_ooxml_rtf::*;
use strict_ooxml_wml::model::{Block, Inline, RunContent, TriState};

fn paragraphs(input: &[u8]) -> Vec<String> {
    paragraphs_with_options(input, &RtfOptions::default())
}
fn paragraphs_with_options(input: &[u8], options: &RtfOptions) -> Vec<String> {
    let imported = import(input, options).unwrap();
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
fn nul_drop_is_explicit_counted_and_does_not_weaken_other_encoding_checks() {
    let input = br"{\rtf1 A\par\'00\'00}";
    assert!(import(input, &RtfOptions::default()).is_err());
    let options = RtfOptions {
        nul_policy: NulPolicy::DropAndReport,
        ..Default::default()
    };
    let imported = import(input, &options).unwrap();
    assert_eq!(imported.report.discarded_nuls, 2);
    assert_eq!(imported.report.loss_count, 1);
    assert_eq!(paragraphs_with_options(input, &options), ["A", ""]);
    assert_eq!(
        paragraphs_with_options(br"{\rtf1 A\'00B}", &options),
        ["AB"]
    );
    for invalid in [
        br"{\rtf1\'01}".as_slice(),
        br"{\rtf1\uc0\u-10179}".as_slice(),
        br"{\rtf1\ansicpg65001\'ff}".as_slice(),
    ] {
        assert!(import(invalid, &options).is_err());
    }
    let reject = RtfOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..options.clone()
    };
    assert!(matches!(
        import(input, &reject),
        Err(RtfError::UnsupportedRejected { .. })
    ));
    let capped = RtfOptions {
        limits: RtfLimits {
            issues: 0,
            ..Default::default()
        },
        ..options
    };
    let result = import(input, &capped).unwrap();
    assert_eq!(result.report.discarded_nuls, 2);
    assert_eq!(result.report.loss_count, 1);
    assert!(result.report.details_truncated);
}

#[test]
fn wps_group_prefixes_match_independent_minimal_probes() {
    let options = RtfOptions {
        compatibility_profile: CompatibilityProfile::WpsText,
        ..Default::default()
    };
    for (input, expected) in [
        (b"{\\rtf1 A{\t\nB}}".as_slice(), "A\t\nB"),
        (b"{\\rtf1 A{\n    }B}".as_slice(), "A\n    B"),
        (b"{\\rtf1 A{\r\nB}}".as_slice(), "A\n\nB"),
        (b"{\\rtf1 A{B\nC}}".as_slice(), "ABC"),
        (b"{\\rtf1 A{\n\\b B}}".as_slice(), "AB"),
        (b"{\\rtf1 A{\t\n\\u66?}}".as_slice(), "AB"),
        (b"{\\rtf1 A{\r\n\\*\\private X}B}".as_slice(), "AB"),
        (b"{\\rtf1 A{\n\\~B}}".as_slice(), "A\n\u{a0}B"),
        (b"{\\rtf1 A{\n{\\b B}}}".as_slice(), "A\nB"),
    ] {
        assert_eq!(paragraphs_with_options(input, &options), [expected]);
    }
    assert_eq!(paragraphs(b"{\\rtf1 A{\r\nB}}"), ["AB"]);
    let mut bounded = options;
    bounded.limits.text_scalars = 1;
    assert!(matches!(
        import(b"{\\rtf1{\r\n}}", &bounded),
        Err(RtfError::LimitExceeded {
            limit: "text_scalars",
            ..
        })
    ));
}

#[test]
fn wps_leading_group_line_endings_are_info_not_losses() {
    let options = RtfOptions {
        compatibility_profile: CompatibilityProfile::WpsText,
        ..Default::default()
    };
    let imported = import(b"{\\rtf1 A{\r\nB}}", &options).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "the preserved line feed matches WPS: {:?}",
        imported.report.issues
    );
    assert_eq!(
        imported.report.info_count, 2,
        "one Info per raw line-end byte"
    );
    assert!(
        imported
            .report
            .issues
            .iter()
            .all(|i| i.feature == "wps:leading-group-line-ending" && i.severity == Severity::Info),
        "{:?}",
        imported.report.issues
    );
    assert_eq!(
        paragraphs_with_options(b"{\\rtf1 A{\r\nB}}", &options),
        ["A\n\nB"]
    );
}

#[test]
fn wps_inline_words_and_soft_hyphens_are_explicit_losses() {
    let options = RtfOptions {
        compatibility_profile: CompatibilityProfile::WpsText,
        ..Default::default()
    };
    for (input, expected) in [
        (
            br"{\rtf1 A\CocoaLigature1 B}".as_slice(),
            "ACocoaLigature1 B",
        ),
        (
            br"{\rtf1 A\AppleTypeServices B}".as_slice(),
            "AAppleTypeServices B",
        ),
        (br"{\rtf1\CocoaLigature1 A}".as_slice(), "A"),
        (br"{\rtf1 A{\CocoaLigature1 B}}".as_slice(), "AB"),
        (br"{\rtf1 A\-B}".as_slice(), "A-B"),
        (br"{\rtf1\uc0 A\u173 B}".as_slice(), "A\u{ad}B"),
    ] {
        assert_eq!(paragraphs_with_options(input, &options), [expected]);
    }
    assert_eq!(paragraphs(br"{\rtf1 A\CocoaLigature1 B}"), ["AB"]);
    assert!(
        import(br"{\rtf1 A\-B}", &options)
            .unwrap()
            .report
            .loss_count
            > 0
    );
    let reject = RtfOptions {
        unsupported_policy: UnsupportedPolicy::Reject,
        ..options
    };
    assert!(matches!(
        import(br"{\rtf1 A\-B}", &reject),
        Err(RtfError::UnsupportedRejected { .. })
    ));
}

#[test]
fn horizontal_merges_and_final_cell_paragraphs_preserve_structure() {
    let imported = import(
        br"{\rtf1\trowd\clmgf\cellx1000\clmrg\cellx2000\intbl A\cell B\cell\row}",
        &RtfOptions::default(),
    )
    .unwrap();
    let Block::Table(table) = &imported.document.body.blocks[0] else {
        panic!()
    };
    assert_eq!(table.rows[0].cells.len(), 1);
    assert_eq!(table.rows[0].cells[0].props.grid_span, Some(2));
    assert_eq!(
        table.rows[0].cells[0].props.width.as_ref().unwrap().value,
        Some(2000)
    );
    let Block::Paragraph(p) = &table.rows[0].cells[0].blocks[0] else {
        panic!()
    };
    let text: String = p
        .inlines
        .iter()
        .flat_map(|i| match i {
            Inline::Run(r) => r.content.iter(),
            _ => panic!(),
        })
        .filter_map(|c| match c {
            RunContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "AB");
    let written =
        strict_ooxml_write::write_package(&imported.document, None, &Default::default()).unwrap();
    let package =
        strict_ooxml_core::opc::Package::open_reader(&written.bytes[..], &Default::default())
            .unwrap();
    let reopened = strict_ooxml_wml::parse_document(&package, &Default::default()).unwrap();
    let Block::Table(reopened) = &reopened.body.blocks[0] else {
        panic!()
    };
    assert_eq!(reopened.rows[0].cells[0].props.grid_span, Some(2));
    assert_eq!(
        reopened.rows[0].cells[0].props.width,
        table.rows[0].cells[0].props.width
    );
    let imported = import(
        br"{\rtf1\trowd\cellx1000\cellx2000\intbl A\par\cell B\cell\row}",
        &RtfOptions::default(),
    )
    .unwrap();
    let Block::Table(t) = &imported.document.body.blocks[0] else {
        panic!()
    };
    assert_eq!(t.rows[0].cells[0].blocks.len(), 2);
    assert!(matches!(&t.rows[0].cells[0].blocks[1], Block::Paragraph(p) if p.inlines.is_empty()));
    assert!(import(
        br"{\rtf1\trowd\clmrg\cellx1000\intbl A\cell\row}",
        &RtfOptions::default()
    )
    .is_err());
    assert!(import(br"{\rtf1\trowd\trleft-2000000000\clmgf\cellx0\clmrg\cellx2000000000\intbl A\cell B\cell\row}", &RtfOptions::default()).is_err());
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
fn section_boundaries_keep_empty_paragraphs_and_geometry() {
    use strict_ooxml_wml::model::SectionType;
    assert_eq!(paragraphs(br"{\rtf1 A\par\sect B\par}"), ["A", "", "B"]);
    assert_eq!(paragraphs(br"{\rtf1 A\sect B}"), ["A", "B"]);
    let imported = import(
        br"{\rtf1\paperw10000\sbknone A\sect\sectd\paperw12000\sbkodd B}",
        &RtfOptions::default(),
    )
    .unwrap();
    let sections = &imported.document.sections;
    assert_eq!(sections.len(), 2);
    assert_eq!(
        sections[0].properties.section_type,
        Some(SectionType::Continuous)
    );
    assert_eq!(
        sections[1].properties.section_type,
        Some(SectionType::OddPage)
    );
    assert_ne!(
        sections[0].properties.page_size,
        sections[1].properties.page_size
    );
    let written =
        strict_ooxml_write::write_package(&imported.document, None, &Default::default()).unwrap();
    let package =
        strict_ooxml_core::opc::Package::open_reader(&written.bytes[..], &Default::default())
            .unwrap();
    let reopened = strict_ooxml_wml::parse_document(&package, &Default::default()).unwrap();
    assert_eq!(reopened.sections.len(), 2);
    assert_eq!(
        reopened.sections[0].properties.section_type,
        Some(SectionType::Continuous)
    );
    assert_eq!(
        reopened.sections[1].properties.section_type,
        Some(SectionType::OddPage)
    );
    assert!(import(br"{\rtf1\trowd\cellx1000 A\sect}", &RtfOptions::default()).is_err());
}
#[test]
fn unspecified_codepage_only_requires_encoding_for_nonascii_bytes() {
    assert_eq!(
        paragraphs(
            br"{\rtf1\ansi\ansicpg0\deff0{\fonttbl{\f0\fcharset1 Sanskrit 2003;}}ASCII \u1046?}"
        ),
        ["ASCII Ж"]
    );
    assert!(matches!(
        import(br"{\rtf1\ansi\ansicpg0 \'cf}", &RtfOptions::default()),
        Err(RtfError::InvalidEncoding { .. })
    ));
}
#[test]
fn unsigned_unicode_is_explicit_bounded_compatibility() {
    let source = br"{\rtf1\uc0 A\u55357 \u56457 B}";
    assert!(import(source, &RtfOptions::default()).is_err());
    let options = RtfOptions {
        unicode_parameter_policy: UnicodeParameterPolicy::AllowUnsignedAndReport,
        ..Default::default()
    };
    let imported = import(source, &options).unwrap();
    assert_eq!(imported.report.info_count, 2);
    assert_eq!(imported.report.loss_count, 0);
    let Block::Paragraph(p) = &imported.document.body.blocks[0] else {
        panic!()
    };
    let text: String = p
        .inlines
        .iter()
        .flat_map(|i| match i {
            Inline::Run(r) => r.content.iter(),
            _ => panic!(),
        })
        .filter_map(|c| match c {
            RunContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "A💉B");
    for source in [
        br"{\rtf1\u65536?}".as_slice(),
        br"{\rtf1\u-32769?}".as_slice(),
    ] {
        assert!(import(source, &options).is_err());
    }
}
#[test]
fn symbol_font_bytes_preserve_font_and_glyph_in_roundtrip() {
    let input = br"{\rtf1\deff0{\fonttbl{\f0\fcharset0 Arial;}{\f1\fcharset2 Wingdings;}}\f0 A\f1\'fc\f0 B}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    assert_eq!(imported.report.text_scalars, 3);
    let symbols = |document: &strict_ooxml_wml::model::Document| {
        document
            .body
            .blocks
            .iter()
            .flat_map(|b| match b {
                Block::Paragraph(p) => p.inlines.iter(),
                _ => panic!(),
            })
            .flat_map(|i| match i {
                Inline::Run(r) => r.content.iter(),
                _ => panic!(),
            })
            .filter_map(|c| match c {
                RunContent::Symbol(s) => Some((s.font.to_string(), s.character)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        symbols(&imported.document),
        [("Wingdings".into(), '\u{fc}')]
    );
    let written =
        strict_ooxml_write::write_package(&imported.document, None, &Default::default()).unwrap();
    let package =
        strict_ooxml_core::opc::Package::open_reader(&written.bytes[..], &Default::default())
            .unwrap();
    let reopened = strict_ooxml_wml::parse_document(&package, &Default::default()).unwrap();
    assert_eq!(symbols(&reopened), symbols(&imported.document));
    let mut options = RtfOptions::default();
    options.limits.text_scalars = 2;
    assert!(matches!(
        import(input, &options),
        Err(RtfError::LimitExceeded {
            limit: "text_scalars",
            ..
        })
    ));
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
    assert_eq!(x.report.feature_counts["control:unknown"].loss_count, 1);
    assert_eq!(x.report.unattributed_loss_count, 0);
}
#[test]
fn feature_aggregate_cap_is_bounded_and_totals_reconcile() {
    let mut input = String::from("{\\rtf1 ");
    for mut value in 0..4097_usize {
        let mut name = ['a'; 6];
        for ch in name.iter_mut().rev() {
            *ch = char::from(b'a' + u8::try_from(value % 26).unwrap());
            value /= 26;
        }
        input.push('\\');
        input.push('q');
        input.extend(name);
        input.push(' ');
    }
    input.push('}');
    let result = import(input.as_bytes(), &RtfOptions::default()).unwrap();
    let tracked_losses: usize = result
        .report
        .feature_counts
        .values()
        .map(|c| c.loss_count)
        .sum();
    let tracked_info: usize = result
        .report
        .feature_counts
        .values()
        .map(|c| c.info_count)
        .sum();
    assert_eq!(result.report.feature_counts.len(), 4096);
    assert!(result.report.feature_counts_truncated);
    assert_eq!(
        tracked_losses + result.report.unattributed_loss_count,
        result.report.loss_count
    );
    assert_eq!(
        tracked_info + result.report.unattributed_info_count,
        result.report.info_count
    );
    assert_eq!(result.report.unattributed_loss_count, 1);
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

#[test]
fn a_header_without_a_version_is_version_one() {
    // As Word and WPS read it; some real documents start `{\rtf \ansi`.
    assert_eq!(paragraphs(br"{\rtf \ansi \deff0 Hello}"), ["Hello"]);
    assert!(matches!(
        import(br"{\rtf2 Hello}", &RtfOptions::default()),
        Err(RtfError::InvalidHeader)
    ));
}

#[test]
fn an_untagged_font_name_wins_over_the_tagged_table_name() {
    // Msftedit writes `{\*\fname Arial;}Arial (Hebrew);`: the tagged name is
    // no installed font, the `\fname` one is what Word and WPS use.
    let imported = import(
        br"{\rtf1\ansi{\fonttbl{\f0\fnil\fcharset177{\*\fname Arial;}Arial (Hebrew);}{\f1\fnil Calibri;}}\f0 x\f1 y\par}",
        &RtfOptions::default(),
    )
    .unwrap();
    let Block::Paragraph(p) = &imported.document.body.blocks[0] else {
        panic!("paragraph expected")
    };
    let names: Vec<String> = p
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(r) => r
                .props
                .fonts
                .as_ref()
                .and_then(|f| f.ascii.as_deref())
                .map(str::to_owned),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Arial", "Calibri"]);
}

#[test]
fn physical_alignment_follows_the_paragraph_direction() {
    use strict_ooxml_wml::model::Justification;
    // `\ql`/`\qr` are the left and right edges whatever comes first;
    // Strict's start/end are the paragraph's own.
    let imported = import(
        br"{\rtf1\ansi\pard\rtlpar\qr a\par\pard\qr\rtlpar b\par\pard\rtlpar\ql c\par\pard\qr d\par\pard\qr\rtlpar\ltrpar e\par}",
        &RtfOptions::default(),
    )
    .unwrap();
    let sides: Vec<(Option<Justification>, TriState)> = imported
        .document
        .body
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(p) => (p.props.alignment, p.props.bidi),
            _ => panic!("paragraph expected"),
        })
        .collect();
    assert_eq!(
        sides,
        [
            (Some(Justification::Start), TriState::On),
            (Some(Justification::Start), TriState::On),
            (Some(Justification::End), TriState::On),
            (Some(Justification::End), TriState::Absent),
            (Some(Justification::End), TriState::Off),
        ]
    );
}
