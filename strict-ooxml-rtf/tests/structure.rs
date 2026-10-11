//! Structural import contracts, independent of the importer's implementation.
#![allow(clippy::default_trait_access)]
use strict_ooxml_rtf::{import, RtfOptions};
use strict_ooxml_wml::model::{
    AbstractNumId, Block, BorderStyle, Color, EighthsPoint, HalfPoints, Ilvl, Inline, NoteKind,
    NumId, RunContent, StyleId, TabLeader, TriState, Twips, WidthKind,
};

/// The complex-field markers (`w:fldChar`/`w:instrText`) of one paragraph, in order.
fn field_marker_marks(inlines: &[Inline]) -> Vec<String> {
    inlines
        .iter()
        .filter_map(|i| {
            let Inline::Run(r) = i else { return None };
            r.content.iter().find_map(|c| match c {
                RunContent::FieldChar(f) => Some(format!("fieldchar:{}", f.kind.as_str())),
                RunContent::InstrText(t) => Some(format!("instr:{t}")),
                _ => None,
            })
        })
        .collect()
}

#[test]
fn unresolved_abstract_list_reference_is_never_clean() {
    let source =
        br"{\rtf1{\*\listoverridetable{\listoverride\listid42\listoverridecount0\ls7}}\ls7 item}";
    let result = import(source, &RtfOptions::default()).unwrap();
    assert!(result
        .report
        .issues
        .iter()
        .any(|i| i.feature == "numbering:undefined-abstract:42"));
    let options = RtfOptions {
        unsupported_policy: strict_ooxml_rtf::UnsupportedPolicy::Reject,
        ..Default::default()
    };
    assert!(import(source, &options).is_err());
}

#[test]
fn list_definition_budget_is_enforced_before_insertion() {
    let source = br"{\rtf1{\*\listtable{\list\listid1}{\list\listid2}}body}";
    let mut options = RtfOptions::default();
    options.limits.definitions = 1;
    assert!(matches!(
        import(source, &options),
        Err(strict_ooxml_rtf::RtfError::LimitExceeded {
            limit: "list_definitions",
            ..
        })
    ));
}

#[test]
fn abstract_list_definitions_may_follow_overrides() {
    let source=br"{\rtf1{\*\listoverridetable{\listoverride\listid42\listoverridecount0\ls7}}{\*\listtable{\list{\listlevel\levelnfc0{\leveltext\'02\'00.;}}\listid42}}\ls7 item}";
    let result = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(result.report.loss_count, 0);
    assert!(result
        .document
        .numbering
        .resolved_abstract(NumId(7))
        .is_some());
}

#[test]
fn rtf_page_defaults_do_not_fall_through_to_renderer_defaults() {
    use strict_ooxml_wml::model::Twips;
    let d = reopened(br"{\rtf1 Default}");
    let section = &d.sections[0].properties;
    let size = section
        .page_size
        .expect("RTF default geometry must be explicit");
    assert_eq!(size.width, Some(Twips(12240)));
    assert_eq!(size.height, Some(Twips(15840)));
    let margins = section.page_margins.unwrap();
    assert_eq!(margins.left, Some(Twips(1800)));
    assert_eq!(margins.right, Some(Twips(1800)));
    assert_eq!(margins.top, Some(Twips(1440)));
    assert_eq!(margins.bottom, Some(Twips(1440)));
}

#[test]
fn section_reset_restores_document_geometry_not_ooxml_defaults() {
    use strict_ooxml_wml::model::Twips;
    let d=reopened(br"{\rtf1\paperw16000\paperh20000\margl900\margr1000\margt1100\margb1200\sectd\pgwsxn18000 A\sect\sectd B}");
    assert_eq!(d.sections.len(), 2);
    assert_eq!(
        d.sections[0].properties.page_size.unwrap().width,
        Some(Twips(18000))
    );
    assert_eq!(
        d.sections[1].properties.page_size.unwrap().width,
        Some(Twips(16000))
    );
    for section in &d.sections {
        assert_eq!(
            section.properties.page_size.unwrap().height,
            Some(Twips(20000))
        );
        assert_eq!(
            section.properties.page_margins.unwrap().left,
            Some(Twips(900))
        );
    }
}

#[test]
fn metafile_payloads_and_extents_survive_package_roundtrip() {
    use strict_ooxml_wml::model::drawing::MediaKind;
    let wmf: Vec<u8> = "0100090000030c0000000000030000000000030000000000"
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    let mut emf = vec![0u8; 108];
    for (offset, value) in [
        (0, 1u32),
        (4, 88),
        (40, 0x464d_4520),
        (44, 0x10000),
        (48, 108),
        (52, 2),
        (72, 100),
        (76, 100),
        (80, 100),
        (84, 100),
        (88, 14),
        (92, 20),
        (104, 20),
    ] {
        emf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (format, bytes, kind) in [
        ("wmetafile8", wmf, MediaKind::Wmf),
        ("emfblip", emf, MediaKind::Emf),
    ] {
        let mut source = format!(
            "{{\\rtf1{{\\pict\\{format}\\picwgoal1440\\pichgoal720\\bin{} ",
            bytes.len()
        )
        .into_bytes();
        source.extend_from_slice(&bytes);
        source.extend_from_slice(b"}}");
        let imported = import(&source, &RtfOptions::default()).unwrap();
        assert_eq!(imported.media.len(), 1, "metafile must not be discarded");
        assert_eq!(imported.media[0].1, bytes);
        assert_eq!(imported.document.media.iter().next().unwrap().kind, kind);
        let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
            panic!()
        };
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!()
        };
        let RunContent::Drawing(drawing) = &run.content[0] else {
            panic!()
        };
        let strict_ooxml_wml::model::drawing::DrawingKind::Inline(inline) = &drawing.kind else {
            panic!()
        };
        let extent = inline.extent.unwrap();
        assert_eq!(extent.cx, strict_ooxml_wml::model::Emu(914_400));
        assert_eq!(extent.cy, strict_ooxml_wml::model::Emu(457_200));
        assert_eq!(
            imported.report.loss_count, 0,
            "retention must not claim rendering fidelity"
        );
        let mut bag = strict_ooxml_write::package::MediaBag::new();
        for (part, data) in imported.media {
            bag.insert(part, data);
        }
        let written =
            strict_ooxml_write::write_package(&imported.document, Some(&bag), &Default::default())
                .unwrap();
        assert_eq!(
            written
                .report
                .losses()
                .iter()
                .map(|loss| loss.feature_id.as_str())
                .collect::<Vec<_>>(),
            vec!["pic:cNvPr@id"],
            "the writer must report its generated picture id while preserving media"
        );
        let package = strict_ooxml_core::opc::Package::open_reader(
            written.bytes.as_slice(),
            &Default::default(),
        )
        .unwrap();
        let document = strict_ooxml_wml::parse_document(&package, &Default::default()).unwrap();
        let media = document.media.iter().next().unwrap();
        assert_eq!(media.kind, kind);
        assert_eq!(package.read_part(&media.part).unwrap(), bytes);
    }
}

#[test]
fn malformed_metafile_signatures_and_budget_overflow_are_rejected() {
    assert!(matches!(
        import(
            br"{\rtf1{\pict\wmetafile8\picwgoal1440\pichgoal720 00000000}}",
            &RtfOptions::default()
        ),
        Err(strict_ooxml_rtf::RtfError::Malformed { .. })
    ));
    assert!(import(
        br"{\rtf1{\pict\emfblip\picwgoal1440\pichgoal720 00000000}}",
        &RtfOptions::default()
    )
    .is_err());
    let mut options = RtfOptions::default();
    options.limits.media_part_bytes = 2;
    assert!(matches!(import(br"{\rtf1{\pict\wmetafile8\picwgoal1440\pichgoal720 0100090000030c0000000000030000000000030000000000}}",&options),Err(strict_ooxml_rtf::RtfError::LimitExceeded {limit:"media_part_bytes",..})));
}

#[test]
fn orphan_and_assumed_note_markers_are_explicit_losses() {
    let orphan = import(br"{\rtf1 A\chftn B}", &RtfOptions::default()).unwrap();
    assert!(orphan
        .report
        .issues
        .iter()
        .any(|i| i.feature == "note:unpaired-reference-marker"));
    let assumed = import(br"{\rtf1 X{\footnote\ftnalt End}}", &RtfOptions::default()).unwrap();
    assert!(assumed
        .report
        .issues
        .iter()
        .any(|i| i.feature == "note:implicit-reference-marker-assumed"));
    let automatic = import(
        br"{\rtf1 A\chftn{\footnote\chftn Note}}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(automatic.report.loss_count, 0);
    assert_eq!(automatic.document.footnotes.len(), 1);
}

#[test]
fn region_geometry_cannot_change_body_geometry_silently() {
    let result = import(
        br"{\rtf1{\header\paperw100\margl0 Header}Body}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(
        result.document.sections[0]
            .properties
            .page_size
            .unwrap()
            .width,
        Some(strict_ooxml_wml::model::Twips(12240))
    );
    assert_eq!(
        result.document.sections[0]
            .properties
            .page_margins
            .unwrap()
            .left,
        Some(strict_ooxml_wml::model::Twips(1800))
    );
    assert_eq!(result.report.loss_count, 2);
}

#[test]
fn document_defaults_survive_without_named_styles() {
    let d = reopened(br"{\rtf1\deff0{\fonttbl{\f0 Arial;}}Text}");
    let defaults = d
        .styles
        .defaults()
        .expect("docDefaults must be serialized even without named styles");
    assert_eq!(
        defaults.run.size,
        Some(strict_ooxml_wml::model::HalfPoints(24))
    );
    assert_eq!(
        defaults.run.fonts.as_ref().unwrap().ascii.as_deref(),
        Some("Arial")
    );
    assert!(d
        .font_table
        .as_ref()
        .unwrap()
        .fonts
        .iter()
        .any(|f| f.name.as_ref() == "Arial"));
}

#[test]
fn absent_deff_defaults_to_the_first_font_in_the_table() {
    // RTF 1.9.1 `\deff`: when the control is absent the default font is font 0.
    // The default must reach `w:docDefaults` so runs with no `\f` (empty
    // paragraphs, paragraph marks) inherit the real face metrics instead of the
    // renderer's fallback.
    let d = reopened(br"{\rtf1{\fonttbl{\f0 Helvetica;}{\f1 Times;}}Text}");
    let defaults = d.styles.defaults().expect("docDefaults are always emitted");
    assert_eq!(
        defaults.run.fonts.as_ref().unwrap().ascii.as_deref(),
        Some("Helvetica")
    );
}

#[test]
fn explicit_deff_wins_over_the_first_font_in_the_table() {
    let d = reopened(br"{\rtf1\deff1{\fonttbl{\f0 Helvetica;}{\f1 Times;}}Text}");
    let defaults = d.styles.defaults().expect("docDefaults are always emitted");
    assert_eq!(
        defaults.run.fonts.as_ref().unwrap().ascii.as_deref(),
        Some("Times")
    );
}

#[test]
fn empty_paragraph_mark_carries_the_ambient_run_size() {
    // An empty paragraph is a bare paragraph mark; its height comes from
    // `w:pPr/w:rPr`. Without the ambient run formatting the mark falls back to
    // the 12pt document default and every following line shifts by the leading
    // difference (054 Hotyn2 body is 10pt).
    let d = reopened(br"{\rtf1{\fonttbl{\f0 Arial;}}\f0\fs20 A\par\par B}");
    let empty = d
        .body
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.inlines.is_empty() => Some(paragraph),
            _ => None,
        })
        .expect("the source has an empty paragraph");
    let run = empty
        .props
        .run_props
        .as_ref()
        .expect("the paragraph mark carries run properties");
    assert_eq!(run.size, Some(HalfPoints(20)));
    assert_eq!(run.fonts.as_ref().unwrap().ascii.as_deref(), Some("Arial"));
}

#[test]
fn rtf10_indent_controls_map_like_their_modern_aliases() {
    // RTF 1.0 `\lin`/`\rin`/`\fin` are the same indents as `\li`/`\ri`/`\fi`.
    // They are common in the corpus (32 documents use `\lin`), and dropping
    // them loses the paragraph indentation.
    let d = reopened(br"{\rtf1\lin720\rin360\fin180 line}");
    let Block::Paragraph(paragraph) = &d.body.blocks[0] else {
        panic!()
    };
    let indentation = paragraph.props.indentation.expect("indentation");
    assert_eq!(indentation.start, Some(Twips(720)));
    assert_eq!(indentation.end, Some(Twips(360)));
    assert_eq!(indentation.first_line, Some(Twips(180)));
}

#[test]
fn numbering_suffix_precedes_level_text_in_the_written_package() {
    let source=br"{\rtf1{\*\listtable{\list{\listlevel\levelnfc0{\leveltext\'02\'00.;}}\listid42}}{\*\listoverridetable{\listoverride\listid42\listoverridecount0\ls7}}\ls7 item}";
    let d = import(source, &RtfOptions::default()).unwrap();
    let output = strict_ooxml_write::write_package(&d.document, None, &Default::default()).unwrap();
    let p =
        strict_ooxml_core::opc::Package::open_reader(output.bytes.as_slice(), &Default::default())
            .unwrap();
    let xml = String::from_utf8(
        p.read_part(&strict_ooxml_core::part::PartId::new("/word/numbering.xml"))
            .unwrap(),
    )
    .unwrap();
    assert!(
        xml.find("<w:suff").unwrap() < xml.find("<w:lvlText").unwrap(),
        "CT_Lvl requires suff before lvlText"
    );
}

#[test]
fn multi_paragraph_field_becomes_the_complex_marker_form() {
    let source = br"{\rtf1 A{\field{\*\fldinst TOC}{\fldrslt B\par C\par}}\par D}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert!(
        !imported
            .report
            .issues
            .iter()
            .any(|i| i.feature.starts_with("field:")),
        "a block result is preserved as the complex marker form"
    );
    let d = reopened(source);
    let text: Vec<String> = d
        .body
        .blocks
        .iter()
        .map(|b| {
            let Block::Paragraph(p) = b else { panic!() };
            p.inlines
                .iter()
                .filter_map(|i| {
                    if let Inline::Run(r) = i {
                        Some(
                            r.content
                                .iter()
                                .filter_map(|c| {
                                    if let RunContent::Text(t) = c {
                                        Some(t.text.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<String>(),
                        )
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect();
    assert_eq!(text, ["AB", "C", "", "D"]);
    // The field scope survives as begin / instrText / separate / end, split
    // across the cached result paragraphs.
    let Block::Paragraph(first) = &d.body.blocks[0] else {
        panic!()
    };
    let Block::Paragraph(second) = &d.body.blocks[1] else {
        panic!()
    };
    assert_eq!(
        field_marker_marks(&first.inlines),
        ["fieldchar:begin", "instr:TOC", "fieldchar:separate"]
    );
    assert_eq!(field_marker_marks(&second.inlines), ["fieldchar:end"]);
}

#[test]
fn dirty_field_marker_and_compat_controls_are_not_losses() {
    // `\flddirty` marks the complex-field begin marker dirty; `\fldedit` and the
    // legacy `{\*\datafield}` cache are compatibility info, not losses.
    let source = br"{\rtf1 X{\field\flddirty\fldedit{\*\fldinst PAGEREF _Toc1}{\fldrslt 7}}{\*\datafield 08}\par D}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "every field-compat control is Info: {:?}",
        imported.report.issues
    );
    assert!(
        imported
            .report
            .issues
            .iter()
            .any(|i| i.feature == "field-compat:fldedit"),
        "fldedit is recorded as Info: {:?}",
        imported.report.issues
    );
    assert!(
        imported
            .report
            .issues
            .iter()
            .any(|i| i.feature == "destination:datafield"),
        "the legacy cache is recorded as Info: {:?}",
        imported.report.issues
    );
    let d = reopened(source);
    let Block::Paragraph(first) = &d.body.blocks[0] else {
        panic!()
    };
    assert!(
        first.inlines.iter().any(|i| matches!(i,
            Inline::Run(r) if r.content.iter().any(|c| matches!(c,
                RunContent::FieldChar(f) if f.kind.as_str() == "begin" && f.dirty)))),
        "the begin marker carries w:dirty"
    );
}

#[test]
fn negative_list_ids_and_symbol_style_names_never_become_body_text() {
    let source=br"{\rtf1{\fonttbl{\f0\fcharset2 Symbol;}}{\stylesheet{\s0\f0 Normal;}}{\*\listtable{\list{\listlevel\levelnfc0{\leveltext\'02\'00.;}}\listid-42}}{\*\listoverridetable{\listoverride\listid-42\listoverridecount0\ls7}}\pard\ls7 X}";
    let d = reopened(source);
    assert_eq!(
        d.numbering.num(NumId(7)).unwrap().abstract_num_id,
        AbstractNumId((-42i32).cast_unsigned())
    );
    assert_eq!(
        d.styles
            .get(&StyleId::new("rtf-s0"))
            .unwrap()
            .name
            .as_deref(),
        Some("Normal")
    );
    let Block::Paragraph(p) = &d.body.blocks[0] else {
        panic!()
    };
    assert_eq!(p.inlines.len(), 1);
}

#[test]
fn spacing_rules_and_tab_reset_are_explicit() {
    use strict_ooxml_wml::model::{LineSpacingRule, Twips};
    let source = br"{\rtf1\tqr\tldot\tx1440\sl360\slmult1 A\par\pard\sl-300\slmult0\tx720 B}";
    let d = reopened(source);
    let Block::Paragraph(a) = &d.body.blocks[0] else {
        panic!()
    };
    let Block::Paragraph(b) = &d.body.blocks[1] else {
        panic!()
    };
    assert_eq!(a.props.spacing.unwrap().line, Some(Twips(360)));
    assert_eq!(
        a.props.spacing.unwrap().line_rule,
        Some(LineSpacingRule::Auto)
    );
    assert_eq!(
        b.props.spacing.unwrap().line_rule,
        Some(LineSpacingRule::Exact)
    );
    assert_eq!(b.props.tabs.len(), 1);
    assert_eq!(b.props.tabs[0].leader, None);
    assert_eq!(
        b.props.tabs[0].alignment,
        strict_ooxml_wml::model::TabAlignment::Start
    );
}

#[test]
fn associated_bold_italic_and_size_survive_wml_roundtrip() {
    let source = br"{\rtf1\ab\ai\afs28 Associated}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let run = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Run(run) => Some(run),
            _ => None,
        })
        .expect("associated-formatting run");
    assert_eq!(run.props.bold_cs, TriState::On);
    assert_eq!(run.props.italic_cs, TriState::On);
    assert_eq!(
        run.props.size_cs,
        Some(strict_ooxml_wml::model::HalfPoints(28))
    );
    assert_eq!(run.props.bold, TriState::Absent);
    assert_eq!(run.props.italic, TriState::Absent);
}

#[test]
fn associated_font_survives_font_switch_and_wml_roundtrip() {
    let source = br"{\rtf1{\fonttbl{\f0 Arial;}{\f1 SimSun;}}\f0 Latin {\af1 Han \f0 After}}";
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let runs = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => Some(run),
            _ => None,
        })
        .collect::<Vec<_>>();
    let associated = runs
        .iter()
        .find(|run| {
            run.content.iter().any(
                |content| matches!(content, RunContent::Text(text) if text.text.contains("Han")),
            )
        })
        .expect("associated-font run");
    assert_eq!(
        associated.props.fonts.as_ref().unwrap().ascii.as_deref(),
        Some("Arial")
    );
    assert_eq!(
        associated
            .props
            .fonts
            .as_ref()
            .unwrap()
            .east_asia
            .as_deref(),
        Some("SimSun")
    );
    let after_switch = runs
        .iter()
        .find(|run| {
            run.content.iter().any(
                |content| matches!(content, RunContent::Text(text) if text.text.contains("After")),
            )
        })
        .expect("run after ordinary font switch");
    assert_eq!(
        after_switch
            .props
            .fonts
            .as_ref()
            .unwrap()
            .east_asia
            .as_deref(),
        Some("SimSun")
    );
}

#[test]
fn script_font_selectors_map_f_and_af_to_their_wml_slots() {
    let source = br"{\rtf1{\fonttbl{\f0 Base;}{\f1 Low;}{\f2 High;}{\f3 Double;}{\f4 Complex;}}\f0\loch\f1 Low \hich\af2 High \dbch\af3 Double \rtlch\af4 Complex {\loch\f1 Scoped} After {\ltrch\fcs1\af4 SouthAsian}}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let fonts_for = |needle: &str| {
        paragraph.inlines.iter().find_map(|inline| {
            match inline {
            Inline::Run(run)
                if run.content.iter().any(|content| {
                    matches!(content, RunContent::Text(text) if text.text.contains(needle))
                }) => run.props.fonts.as_ref(),
            _ => None,
        }
        })
    };
    let low = fonts_for("Low").expect("low-ANSI font properties");
    assert_eq!(low.ascii.as_deref(), Some("Low"));
    assert_eq!(low.h_ansi.as_deref(), Some("Base"));
    let high = fonts_for("High").expect("high-ANSI font properties");
    assert_eq!(high.h_ansi.as_deref(), Some("High"));
    assert_eq!(high.east_asia.as_deref(), Some("Base"));
    let double = fonts_for("Double").expect("double-byte font properties");
    assert_eq!(double.east_asia.as_deref(), Some("Double"));
    let complex = fonts_for("Complex").expect("complex-script font properties");
    assert_eq!(complex.complex_script.as_deref(), Some("Complex"));
    let after_group = fonts_for("After").expect("font properties after nested group");
    assert_eq!(after_group.complex_script.as_deref(), Some("Complex"));
    let south_asian = fonts_for("SouthAsian").expect("South Asian font properties");
    assert_eq!(south_asian.complex_script.as_deref(), Some("Complex"));
}

#[test]
fn font_table_theme_selectors_round_trip_to_wml_theme_slots() {
    let source = br"{\rtf1{\fonttbl{\flomajor\f0 MajorAscii;}{\fhimajor\f1 MajorHAnsi;}{\fdbmajor\f2 MajorEastAsia;}{\fbimajor\f3 MajorBidi;}{\flominor\f4 MinorAscii;}{\fhiminor\f5 MinorHAnsi;}{\fdbminor\f6 MinorEastAsia;}{\fbiminor\f7 MinorBidi;}}\f0 A \f1 H \f2 E \f3 B \f4 a \f5 h \f6 e \f7 b}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let expected = [
        ("A", "MajorAscii", "majorAscii"),
        ("H", "MajorHAnsi", "majorHAnsi"),
        ("E", "MajorEastAsia", "majorEastAsia"),
        ("B", "MajorBidi", "majorBidi"),
        ("a", "MinorAscii", "minorAscii"),
        ("h", "MinorHAnsi", "minorHAnsi"),
        ("e", "MinorEastAsia", "minorEastAsia"),
        ("b", "MinorBidi", "minorBidi"),
    ];
    for (text, face, theme) in expected {
        let fonts = paragraph
            .inlines
            .iter()
            .find_map(|inline| {
                match inline {
                Inline::Run(run)
                    if run.content.iter().any(|content| {
                        matches!(content, RunContent::Text(value) if value.text.contains(text))
                    }) => run.props.fonts.as_ref(),
                _ => None,
            }
            })
            .unwrap_or_else(|| panic!("font properties for {text}"));
        let (actual_face, actual_theme) = match theme {
            "majorAscii" | "minorAscii" => (fonts.ascii.as_deref(), fonts.ascii_theme.as_deref()),
            "majorHAnsi" | "minorHAnsi" => (fonts.h_ansi.as_deref(), fonts.h_ansi_theme.as_deref()),
            "majorEastAsia" | "minorEastAsia" => {
                (fonts.east_asia.as_deref(), fonts.east_asia_theme.as_deref())
            }
            "majorBidi" | "minorBidi" => {
                (fonts.complex_script.as_deref(), fonts.cs_theme.as_deref())
            }
            _ => unreachable!(),
        };
        assert_eq!(actual_face, Some(face));
        assert_eq!(actual_theme, Some(theme));
    }
}

#[test]
fn font_table_fbidis_hint_does_not_change_the_declared_face() {
    let source = br"{\rtf1{\fonttbl{\f0\fbidis\froman\fcharset0 Times New Roman;}}\f0 text}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let fonts = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Run(run) => run.props.fonts.as_ref(),
            _ => None,
        })
        .expect("font properties");
    assert_eq!(fonts.ascii.as_deref(), Some("Times New Roman"));
}

#[test]
fn document_fbidis_keeps_one_font_active_instead_of_associated_font() {
    let source = br"{\rtf1\fbidis{\fonttbl{\f0 Base;}{\f1 Associated;}}\f0 text\af1 still-base}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let fonts = paragraph
        .inlines
        .iter()
        .find_map(|inline| {
            match inline {
            Inline::Run(run)
                if run.content.iter().any(|content| {
                    matches!(content, RunContent::Text(value) if value.text.contains("still-base"))
                }) => run.props.fonts.as_ref(),
            _ => None,
        }
        })
        .expect("font properties");
    assert_eq!(fonts.ascii.as_deref(), Some("Base"));
    assert_eq!(fonts.east_asia.as_deref(), Some("Base"));
}

#[test]
fn unsupported_font_script_class_remains_an_explicit_loss() {
    let imported = import(
        br"{\rtf1{\fonttbl{\f0 Base;}}\fcs2\af0 text}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(imported.report.loss_count, 1);
    assert!(imported
        .report
        .issues
        .iter()
        .any(|issue| issue.feature == "font-script-class:2"));
}

#[test]
fn lang_and_langnp_map_known_lcids_and_restore_group_state() {
    let source = br"{\rtf1\lang1033 English {\langnp2057 British}\lang9 Neutral}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let runs = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => Some(run),
            _ => None,
        })
        .collect::<Vec<_>>();
    let tag_for = |needle: &str| {
        runs.iter()
            .find(|run| {
                run.content.iter().any(|content| {
                matches!(content, RunContent::Text(text) if text.text.contains(needle))
            })
            })
            .and_then(|run| run.props.language.as_ref())
            .and_then(|language| language.val.as_deref())
    };
    assert_eq!(tag_for("English"), Some("en-US"));
    assert_eq!(tag_for("British"), Some("en-GB"));
    assert_eq!(tag_for("Neutral"), Some("en"));
}

#[test]
fn unknown_language_lcid_remains_an_explicit_loss() {
    let imported = import(br"{\rtf1\lang1024 text}", &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 1);
    assert!(imported
        .report
        .issues
        .iter()
        .any(|issue| issue.feature == "language:unknown-lcid:1024"));
}

#[test]
fn langfe_and_langfenp_map_to_east_asia_language_slot() {
    let source = br"{\rtf1\lang1033 English \langfe1042 Korean {\langfenp2052 Chinese}}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let language_for = |needle: &str| {
        paragraph.inlines.iter().find_map(|inline| {
            match inline {
            Inline::Run(run)
                if run.content.iter().any(|content| {
                    matches!(content, RunContent::Text(text) if text.text.contains(needle))
                }) => run.props.language.as_ref(),
            _ => None,
        }
        })
    };
    let korean = language_for("Korean").expect("Korean language properties");
    assert_eq!(korean.val.as_deref(), Some("en-US"));
    assert_eq!(korean.east_asia.as_deref(), Some("ko-KR"));
    let chinese = language_for("Chinese").expect("Chinese language properties");
    assert_eq!(chinese.val.as_deref(), Some("en-US"));
    assert_eq!(chinese.east_asia.as_deref(), Some("zh-CN"));
}

#[test]
fn alang_maps_lcid_to_bidi_language_and_restores_group_state() {
    let source = br"{\rtf1\lang1033 English \langfe2052 Chinese \alang1025 Arabic {\alang1093 Bengali} Restored}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 0);
    let document = reopened(source);
    let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("paragraph")
    };
    let language_for = |needle: &str| {
        paragraph.inlines.iter().find_map(|inline| {
            match inline {
            Inline::Run(run)
                if run.content.iter().any(|content| {
                    matches!(content, RunContent::Text(text) if text.text.contains(needle))
                }) => run.props.language.as_ref(),
            _ => None,
        }
        })
    };
    let chinese = language_for("Chinese").expect("Chinese language properties");
    assert_eq!(chinese.val.as_deref(), Some("en-US"));
    assert_eq!(chinese.east_asia.as_deref(), Some("zh-CN"));
    let arabic = language_for("Arabic").expect("Arabic language properties");
    assert_eq!(arabic.bidi.as_deref(), Some("ar-SA"));
    let bengali = language_for("Bengali").expect("Bengali language properties");
    assert_eq!(bengali.bidi.as_deref(), Some("bn-IN"));
    let restored = language_for("Restored").expect("restored language properties");
    assert_eq!(restored.bidi.as_deref(), Some("ar-SA"));
}

#[test]
fn unknown_alang_lcid_remains_an_explicit_loss() {
    let imported = import(br"{\rtf1\alang1024 text}", &RtfOptions::default()).unwrap();
    assert_eq!(imported.report.loss_count, 1);
    assert!(imported
        .report
        .issues
        .iter()
        .any(|issue| issue.feature == "language:unknown-lcid:1024"));
}

fn reopened(input: &[u8]) -> strict_ooxml_wml::model::Document {
    let imported = import(input, &RtfOptions::default()).unwrap();
    let mut media = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in imported.media {
        media.insert(part, bytes);
    }
    let written =
        strict_ooxml_write::write_package(&imported.document, Some(&media), &Default::default())
            .unwrap();
    assert!(
        written.report.losses().is_empty(),
        "{:?}",
        written.report.losses()
    );
    let package =
        strict_ooxml_core::opc::Package::open_reader(&written.bytes[..], &Default::default())
            .unwrap();
    strict_ooxml_wml::parse_document(&package, &Default::default()).unwrap()
}

#[test]
fn paragraph_and_character_styles_inherit_and_roundtrip() {
    let input = br"{\rtf1{\stylesheet{\s0\fs20 Normal;}{\s1\sbasedon0\fs32\b Title;}{\*\cs2\i Emphasis;}}\pard\s1 Title {\cs2 emphasized}}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        assert_eq!(document.styles.len(), 3);
        let title = document.styles.get(&StyleId::new("rtf-s1")).unwrap();
        assert_eq!(title.name.as_deref(), Some("Title"));
        assert_eq!(title.based_on_chain, [StyleId::new("rtf-s0")]);
        assert_eq!(title.run.bold, TriState::On);
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!()
        };
        assert_eq!(p.props.style, Some(StyleId::new("rtf-s1")));
        let Inline::Run(run) = &p.inlines[1] else {
            panic!()
        };
        assert_eq!(run.props.style, Some(StyleId::new("rtf-cs2")));
    }
}

#[test]
fn character_scale_spacing_and_tab_leaders_roundtrip() {
    // `\charscalex150` -> `w:w` 150; `\expnd4` is 4 quarter-points -> 20 twips;
    // `\expndtw12` is 12 twips; `\plain` resets both; `\tlth` is a heavy leader.
    let input =
        br"{\rtf1\pard\charscalex150\expnd4 scaled\plain reset\par\tlth\tx1000\tab thick\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let Block::Paragraph(scaled) = &document.body.blocks[0] else {
            panic!()
        };
        let Inline::Run(scaled_run) = &scaled.inlines[0] else {
            panic!()
        };
        assert_eq!(scaled_run.props.scale, Some(150));
        assert_eq!(scaled_run.props.spacing, Some(Twips(20)));
        let Inline::Run(reset_run) = &scaled.inlines[1] else {
            panic!()
        };
        assert_eq!(reset_run.props.scale, None);
        assert_eq!(reset_run.props.spacing, None);

        let Block::Paragraph(tabbed) = &document.body.blocks[1] else {
            panic!()
        };
        let stop = tabbed
            .props
            .tabs
            .iter()
            .find(|stop| stop.position == Twips(1000))
            .expect("tab stop");
        assert_eq!(stop.leader, Some(TabLeader::Heavy));
    }

    let escaped = import(br"{\rtf1\expndtw12 twelve\par}", &RtfOptions::default())
        .unwrap()
        .document;
    let Block::Paragraph(paragraph) = &escaped.body.blocks[0] else {
        panic!()
    };
    let Inline::Run(run) = &paragraph.inlines[0] else {
        panic!()
    };
    assert_eq!(run.props.spacing, Some(Twips(12)));
}

#[test]
fn run_effects_kerning_and_position_roundtrip() {
    let input = br"{\rtf1\pard\outl\embo\impr\noproof\snaptogrid flagged\par\shad\kern24 kerned\par\up6 raised\par\dn4 lowered\par\shad0 plain\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let run_props = |index: usize| {
            let Block::Paragraph(paragraph) = &document.body.blocks[index] else {
                panic!()
            };
            let Inline::Run(run) = &paragraph.inlines[0] else {
                panic!()
            };
            run.props.clone()
        };
        let flagged = run_props(0);
        assert_eq!(flagged.outline, TriState::On);
        assert_eq!(flagged.emboss, TriState::On);
        assert_eq!(flagged.imprint, TriState::On);
        assert_eq!(flagged.no_proof, TriState::On);
        assert_eq!(flagged.snap_to_grid, TriState::On);
        let kerned = run_props(1);
        assert_eq!(kerned.shadow, TriState::On);
        assert_eq!(kerned.kerning, Some(HalfPoints(24)));
        assert_eq!(run_props(2).position, Some(HalfPoints(6)));
        assert_eq!(run_props(3).position, Some(HalfPoints(-4)));
        assert_eq!(run_props(4).shadow, TriState::Off);
    }
}

#[test]
fn keep_widow_and_page_break_rules_roundtrip() {
    let input = br"{\rtf1\pard\keep\keepn\widctlpar\pagebb a\par\pard\nowidctlpar b\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let Block::Paragraph(first) = &document.body.blocks[0] else {
            panic!()
        };
        assert_eq!(first.props.keep_lines, TriState::On);
        assert_eq!(first.props.keep_next, TriState::On);
        assert_eq!(first.props.widow_control, TriState::On);
        assert_eq!(first.props.page_break_before, TriState::On);
        let Block::Paragraph(second) = &document.body.blocks[1] else {
            panic!()
        };
        assert_eq!(second.props.widow_control, TriState::Off);
        assert_eq!(second.props.keep_lines, TriState::Absent);
    }
}

#[test]
fn mixed_direction_run_is_split_into_directional_runs() {
    // "abc" + Arabic "السلام" + "123", written with no explicit `\rtlch`/`\ltrch`.
    let input = br"{\rtf1\uc0\pard abc \u1575\u1604\u1587\u1604\u1575\u1605  123\par}";
    let expected = "abc \u{627}\u{644}\u{633}\u{644}\u{627}\u{645} 123";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let Block::Paragraph(paragraph) = &document.body.blocks[0] else {
            panic!()
        };
        let mut text = String::new();
        let mut rtl_on = false;
        for inline in &paragraph.inlines {
            let Inline::Run(run) = inline else {
                continue;
            };
            rtl_on |= run.props.rtl == TriState::On;
            for content in &run.content {
                if let RunContent::Text(node) = content {
                    text.push_str(&node.text);
                }
            }
        }
        assert_eq!(text, expected, "mixed-run split must preserve logical text");
        assert!(rtl_on, "the RTL segment must be recorded as w:rtl");
    }
}

#[test]
fn headers_footers_and_both_note_kinds_are_separate_parts() {
    let input = br"{\rtf1{\header Header\par}{\footer Footer}Body\chftn{\footnote\chftn Note\par}X{\footnote\ftnalt End}\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        assert_eq!(document.headers_footers.len(), 2);
        assert_eq!(document.footnotes.len(), 1);
        assert_eq!(document.endnotes.len(), 1);
        assert_eq!(document.sections[0].properties.headers.len(), 1);
        assert_eq!(document.sections[0].properties.footers.len(), 1);
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!()
        };
        assert!(p.inlines.iter().any(
            |i| matches!(i, Inline::Run(r) if r.content.contains(&RunContent::FootnoteRef(1)))
        ));
        assert!(p.inlines.iter().any(
            |i| matches!(i, Inline::Run(r) if r.content.contains(&RunContent::EndnoteRef(1)))
        ));
    }
}

#[test]
fn wps_sectd_before_sect_keeps_each_section_header() {
    // WPS writes `\sectd\sect` — the reverse of Word's `\sect\sectd`. Deferring
    // the reset past the following `\sect` must leave each section with the
    // header it accumulated (072 lost every header but the last).
    let input = br"{\rtf1{\fonttbl{\f0 Arial;}}intro\par\sectd\sect{\header First}{\pard bodyA\par}\sectd\sect{\header Second}{\pard bodyB\par}}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        assert_eq!(document.headers_footers.len(), 2);
        // intro, bodyA, bodyB: the header written after a break belongs to the
        // section that follows it, so the intro section carries none.
        assert_eq!(document.sections.len(), 3);
        assert_eq!(document.sections[0].properties.headers.len(), 0);
        assert_eq!(document.sections[1].properties.headers.len(), 1);
        assert_eq!(document.sections[2].properties.headers.len(), 1);
    }
}

#[test]
fn section_bidi_controls_map() {
    let rtl = reopened(br"{\rtf1\rtlsect Body}");
    assert!(rtl.sections[0].properties.bidi);
    let ltr = reopened(br"{\rtf1\rtlsect\ltrsect Body}");
    assert!(
        !ltr.sections[0].properties.bidi,
        "\\ltrsect clears section bidi"
    );
}

#[test]
fn section_columns_map_to_w_cols() {
    let d = reopened(br"{\rtf1\cols2\colno1\colw4820\colsr425\colno2\colw4727 Body}");
    let columns = d.sections[0]
        .properties
        .columns
        .as_ref()
        .expect("column properties");
    assert_eq!(columns.count, Some(2));
    assert!(
        !columns.equal_width,
        "explicit \\colw makes the columns unequal"
    );
    assert_eq!(columns.columns[0].width, Some(Twips(4820)));
    assert_eq!(columns.columns[0].space, Some(Twips(425)));
    assert_eq!(columns.columns[1].width, Some(Twips(4727)));
}

#[test]
fn titlepg_and_even_headers_are_recorded() {
    // `\titlepg` needs `w:titlePg`, and an even-page header only renders when
    // settings carry `w:evenAndOddHeaders`.
    let d = reopened(br"{\rtf1\titlepg{\headerl Even}{\headerr Odd}Body\par}");
    assert!(
        d.settings.even_and_odd_headers,
        "an even header implies evenAndOddHeaders"
    );
    let section = &d.sections[0].properties;
    assert!(section.title_page, "\\titlepg sets w:titlePg");
    assert_eq!(
        section.headers.len(),
        2,
        "even and default header references"
    );
}

#[test]
fn bookmarks_become_paired_start_and_end_inlines() {
    let d = import(
        br"{\rtf1 Before{\*\bkmkstart mark}Text{\*\bkmkend mark}After}",
        &RtfOptions::default(),
    )
    .unwrap()
    .document;
    let mut start = None;
    let mut end = None;
    for block in &d.body.blocks {
        if let Block::Paragraph(paragraph) = block {
            for inline in &paragraph.inlines {
                match inline {
                    Inline::BookmarkStart(bookmark) => {
                        assert_eq!(bookmark.name.as_ref(), "mark");
                        start = Some(bookmark.id.clone());
                    }
                    Inline::BookmarkEnd(id) => end = Some(id.clone()),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(start, end, "start and end must share a w:id");
}

#[test]
fn chapter_page_number_becomes_a_page_field() {
    // `\chpgn`/`\sectnum` render numbers; keep them as dynamic fields so a
    // renderer computes the value instead of dropping them.
    let d = import(br"{\rtf1 Page \chpgn\par}", &RtfOptions::default())
        .unwrap()
        .document;
    let Block::Paragraph(paragraph) = &d.body.blocks[0] else {
        panic!()
    };
    assert!(paragraph.inlines.iter().any(
        |i| matches!(i, Inline::Field(f) if f.instruction.as_deref().map(str::trim) == Some("PAGE"))
    ));
}

#[test]
fn field_instruction_and_formatted_cached_result_survive() {
    let input =
        br#"{\rtf1 A{\field{\*\fldinst HYPERLINK "https://example.test"}{\fldrslt{\b Link}}}Z}"#;
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!()
        };
        let Inline::Field(field) = &p.inlines[1] else {
            panic!()
        };
        assert_eq!(
            field.instruction.as_deref(),
            Some("HYPERLINK \"https://example.test\"")
        );
        let Inline::Run(run) = &field.inlines[0] else {
            panic!()
        };
        assert_eq!(run.props.bold, TriState::On);
        assert!(matches!(&run.content[0], RunContent::Text(t) if t.text == "Link"));
    }
}

#[test]
fn multilevel_list_definition_and_instance_roundtrip() {
    let input = br"{\rtf1{\*\listtable{\list{\listlevel\levelnfc0\levelstartat3{\leveltext\'02\'00.;}{\levelnumbers\'01;}\fi-360\li720}\listid42}}{\*\listoverridetable{\listoverride\listid42\listoverridecount0\ls7}}\pard\ls7\ilvl0 first\par second\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        assert_eq!(
            document.numbering.num(NumId(7)).unwrap().abstract_num_id,
            AbstractNumId(42)
        );
        let level = document
            .numbering
            .abstract_num(AbstractNumId(42))
            .unwrap()
            .level(Ilvl(0))
            .unwrap();
        assert_eq!(level.text.as_deref(), Some("%1."));
        assert_eq!(level.start, Some(3));
        assert_eq!(level.format.as_deref(), Some("decimal"));
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!()
        };
        assert_eq!(p.props.numbering.unwrap().num_id, Some(NumId(7)));
    }
}

#[test]
fn list_override_level_replaces_the_level() {
    // RTF 1.5 "List Override Level": `{\lfolevel\listoverrideformat{\listlevel ...}}`
    // replaces one level of the referenced abstract list (corpus 021, whose
    // override swaps the `*` marker for a bullet glyph). The model carries it as
    // `w:lvlOverride/w:lvl`, so no `list-override-control:*` loss may remain.
    let input = br"{\rtf1{\*\listtable{\list{\listlevel\levelnfc23{\leveltext\'01*;}{\levelnumbers;}}\listid2}}{\*\listoverridetable{\listoverride\listid2\listoverridecount1{\lfolevel\listoverrideformat{\listlevel\levelnfc23\leveljc0\levelfollow0\levelstartat0{\leveltext\'01\u183 ?;}{\levelnumbers;}}}\ls1}}\pard\ls1\ilvl0 item\par}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    assert!(
        !imported
            .report
            .issues
            .iter()
            .any(|i| i.feature.starts_with("list-override-control:")),
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        // The abstract list keeps the literal `*`; the override replaces it.
        let abstract_level = document
            .numbering
            .abstract_num(AbstractNumId(2))
            .unwrap()
            .level(Ilvl(0))
            .unwrap();
        assert_eq!(abstract_level.text.as_deref(), Some("*"));
        let overrides = &document.numbering.num(NumId(1)).unwrap().overrides;
        assert_eq!(overrides.len(), 1);
        let over = &overrides[0];
        assert_eq!(over.ilvl, Ilvl(0));
        assert_eq!(over.start_override, None);
        let level = over
            .level
            .as_ref()
            .expect("format override carries a level");
        assert_eq!(level.format.as_deref(), Some("bullet"));
        assert_eq!(level.text.as_deref(), Some("\u{b7}"));
    }
}

#[test]
fn list_override_start_sets_the_start_override() {
    // The start-only form (`\lfolevel\listoverridestart\levelstartatN`) overrides
    // only the level's start value: `w:lvlOverride/w:startOverride` with no `w:lvl`.
    let input = br"{\rtf1{\*\listtable{\list{\listlevel\levelnfc0{\leveltext\'02\'00.;}{\levelnumbers;}}\listid2}}{\*\listoverridetable{\listoverride\listid2\listoverridecount1{\lfolevel\listoverridestart\levelstartat5}\ls1}}\pard\ls1\ilvl0 item\par}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let overrides = &document.numbering.num(NumId(1)).unwrap().overrides;
        assert_eq!(overrides.len(), 1);
        let over = &overrides[0];
        assert_eq!(over.ilvl, Ilvl(0));
        assert_eq!(over.start_override, Some(5));
        assert!(over.level.is_none());
    }
}

#[test]
fn document_note_numbering_and_placement_map_to_settings() {
    // `\ftn*`/`\aftn*` are document-wide and land in `settings.xml` as
    // `w:footnotePr`/`w:endnotePr` (RTF 1.5 "Footnotes and Endnotes").
    let input =
        br"{\rtf1\ftnnalc\ftnstart3\ftnrestart\ftnbj\aftnnruc\aftnstart2\aftnrstcont\aenddoc Body}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let footnote = &document.settings.footnote_properties;
        assert_eq!(footnote.num_format.as_deref(), Some("lowerLetter"));
        assert_eq!(footnote.num_start, Some(3));
        assert_eq!(footnote.num_restart.as_deref(), Some("eachSect"));
        assert_eq!(footnote.position.as_deref(), Some("pageBottom"));
        let endnote = &document.settings.endnote_properties;
        assert_eq!(endnote.num_format.as_deref(), Some("upperRoman"));
        assert_eq!(endnote.num_start, Some(2));
        assert_eq!(endnote.num_restart.as_deref(), Some("continuous"));
        assert_eq!(endnote.position.as_deref(), Some("docEnd"));
    }
}

#[test]
fn section_note_properties_map_to_the_running_section() {
    // WPS writes the section-scoped `\s*` note controls inside `\sectd`; they
    // must reach the committed `w:sectPr`, not the document settings.
    let d = reopened(br"{\rtf1\ftnbj\sectd\sftnnar\sftnstart7\sftnrestart\saftnnruc Body\par}");
    assert_eq!(
        d.settings.footnote_properties.position.as_deref(),
        Some("pageBottom")
    );
    assert!(
        d.settings.footnote_properties.num_format.is_none(),
        "the section form must not leak into the document settings"
    );
    let section = &d.sections[0].properties;
    assert_eq!(
        section.footnote_properties.num_format.as_deref(),
        Some("decimal")
    );
    assert_eq!(section.footnote_properties.num_start, Some(7));
    assert_eq!(
        section.footnote_properties.num_restart.as_deref(),
        Some("eachSect")
    );
    assert_eq!(
        section.endnote_properties.num_format.as_deref(),
        Some("upperRoman")
    );
}

#[test]
fn recognized_note_controls_are_not_reported_as_loss() {
    let imported = import(
        br"{\rtf1\ftnnar\ftnbj\sftnbj\aenddoc Body}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
}

#[test]
fn unsupported_note_controls_stay_explicit_losses() {
    // `\aftnbj` has no `ST_EdnPos` value and `\ftnlytwnine` is a layout
    // compatibility hint; both must stay honest Loss, not be renamed away.
    for source in [
        &br"{\rtf1\aftnbj Body}"[..],
        &br"{\rtf1\ftnlytwnine Body}"[..],
    ] {
        let imported = import(source, &RtfOptions::default()).unwrap();
        assert_eq!(
            imported.report.loss_count, 1,
            "{:?}",
            imported.report.issues
        );
        assert!(imported
            .report
            .issues
            .iter()
            .any(|issue| issue.feature.starts_with("control:")));
    }
}

#[test]
fn note_separator_definitions_become_reserved_notes() {
    // `{\*\ftnsep ...}`/`{\*\ftnsepc ...}` define the separator rule content;
    // they must become the reserved ids -1/0 instead of a dropped destination.
    let input = br"{\rtf1\ftnbj{\*\ftnsep\chftnsep}{\*\ftnsepc\chftnsepc}{\*\aftnsep\chftnsep}A\chftn{\footnote\chftn Note}\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let separator = document.footnotes.separator().expect("separator note");
        assert_eq!((separator.id, separator.kind), (-1, NoteKind::Separator));
        let continuation = document
            .footnotes
            .continuation_separator()
            .expect("continuation separator note");
        assert_eq!(
            (continuation.id, continuation.kind),
            (0, NoteKind::ContinuationSeparator)
        );
        assert_eq!(
            document.settings.footnote_properties.separator_ids,
            vec![-1, 0]
        );
        assert!(document.endnotes.separator().is_some());
        assert_eq!(document.settings.endnote_properties.separator_ids, vec![-1]);
        let Block::Paragraph(p) = &separator.blocks[0] else {
            panic!("separator note body");
        };
        assert!(p
            .inlines
            .iter()
            .any(|i| matches!(i, Inline::Run(r) if r.content.contains(&RunContent::Separator))));
    }

    let imported = import(input, &RtfOptions::default()).unwrap();
    assert!(
        !imported
            .report
            .issues
            .iter()
            .any(|i| i.feature.starts_with("destination:ftn")),
        "{:?}",
        imported.report.issues
    );
}

#[test]
fn field_instruction_formatting_is_info_not_loss() {
    // Run formatting embedded in `\fldinst` styles the field-code display, not
    // the instruction text: each known formatting control is Info, not Loss.
    let imported = import(
        br#"{\rtf1 A{\field{\*\fldinst\b\i\insrsid42\lang1033 HYPERLINK "https://example.test"}{\fldrslt Link}}Z}"#,
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    assert!(imported
        .report
        .issues
        .iter()
        .any(|i| i.feature == "field-instruction-formatting:b"));
    let Block::Paragraph(paragraph) = &imported.document.body.blocks[0] else {
        panic!()
    };
    let Inline::Field(field) = &paragraph.inlines[1] else {
        panic!()
    };
    assert_eq!(
        field.instruction.as_deref(),
        Some("HYPERLINK \"https://example.test\"")
    );
}

#[test]
fn unknown_field_instruction_control_stays_a_loss() {
    let imported = import(
        br"{\rtf1 A{\field{\*\fldinst\bogus}{\fldrslt Link}}Z}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert!(
        imported
            .report
            .issues
            .iter()
            .any(|i| i.feature == "field-instruction-control:bogus"),
        "{:?}",
        imported.report.issues
    );
}

#[test]
fn contextual_space_maps_to_wml_contextual_spacing() {
    // `\contextualspace` == "don't add space between paragraphs of the same
    // style" -> `w:contextualSpacing`.
    let input = br"{\rtf1\contextualspace Same\par\contextualspace0 Off\par}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let Block::Paragraph(first) = &document.body.blocks[0] else {
            panic!()
        };
        assert_eq!(first.props.contextual_spacing, TriState::On);
        let Block::Paragraph(second) = &document.body.blocks[1] else {
            panic!()
        };
        assert_eq!(second.props.contextual_spacing, TriState::Off);
    }
}

#[test]
fn legacy_list_constructs_are_info_not_loss() {
    // `\listtext` (cached list text), `\jclisttab`/`\levelspace`/`\levelindent`/
    // `\leveltemplateid` (compat markers) and `{\*\levelmarker ...}` have no WML
    // equivalent and must not be losses (dropping `\listtext` avoids a double
    // marker).
    let input = br"{\rtf1{\*\listtable{\list\listtemplateid1{\listlevel\levelnfc0\leveljc0\jclisttab\tx360\levelspace0\levelindent0\leveltemplateid9{\leveltext\'02\'00.;}{\levelnumbers;}{\*\levelmarker \{hyphen\}}}\listid1}}{\*\listoverridetable{\listoverride\listid1\listoverridecount0\ls1}}\pard\ls1\ilvl0{\listtext\t1.\t}item\par}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    assert!(imported.report.info_count >= 3);
    for feature in ["destination:listtext", "destination:levelmarker"] {
        assert!(
            imported.report.issues.iter().any(|i| i.feature == feature),
            "missing {feature}"
        );
    }
}

#[test]
fn table_look_and_bidi_map_to_table_properties() {
    // `\tbllk*` are `w:tblLook` conditional-formatting flags and `\rtlrow` is
    // `w:bidiVisual`; both are table-level in the Strict model.
    let input = br"{\rtf1\trowd\tbllkhdrrows\tbllklastrow\tbllkhdrcols\tbllklastcol\tbllknocolband\rtlrow\cellx2000\cellx4000\intbl A\cell B\cell\row}";
    for document in [
        import(input, &RtfOptions::default()).unwrap().document,
        reopened(input),
    ] {
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("table")
        };
        let look = table.props.look.expect("w:tblLook");
        assert!(look.first_row, "\\tbllkhdrrows -> first_row");
        assert!(look.last_row, "\\tbllklastrow -> last_row");
        assert!(look.first_column, "\\tbllkhdrcols -> first_column");
        assert!(look.last_column, "\\tbllklastcol -> last_column");
        assert!(look.no_v_band, "\\tbllknocolband -> no_v_band");
        assert!(table.props.bidi_visual, "\\rtlrow -> w:bidiVisual");
    }
}

#[test]
fn cell_borders_map_to_tc_borders() {
    // RTF cell borders are `<edge> <style> \brdrw? \brdrcf?` inside a row
    // definition; the four edges map to `w:tcBorders` (`w:sz` is eighths of a
    // point, so `\brdrw20` = 1pt = 8).
    let input = br"{\rtf1{\colortbl;\red255\green0\blue0;}\trowd\clbrdrt\brdrs\brdrw20\brdrcf1\clbrdrb\brdrnil\clbrdrl\brdrdash\clbrdrr\brdrdot\cellx2000\intbl A\cell\row}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("table")
        };
        let borders = &table.rows[0].cells[0].props.borders;
        let top = borders.top.as_ref().expect("top border");
        assert_eq!(top.style, Some(BorderStyle::Single));
        assert_eq!(top.size, Some(EighthsPoint(8)));
        assert_eq!(top.color.as_ref().map(Color::as_str), Some("FF0000"));
        assert_eq!(
            borders.bottom.as_ref().unwrap().style,
            Some(BorderStyle::Nil)
        );
        assert_eq!(
            borders.start.as_ref().unwrap().style,
            Some(BorderStyle::Dashed)
        );
        assert_eq!(
            borders.end.as_ref().unwrap().style,
            Some(BorderStyle::Dotted)
        );
    }
}

#[test]
fn paragraph_borders_map_to_pbdr() {
    // Paragraph edges `\brdrt\brdrs\brdrw10` map to `w:pBdr`; `\brdrbar` and
    // `\brdrbtw` are the paragraph-only `bar`/`between` edges.
    let input =
        br"{\rtf1\pard\brdrt\brdrs\brdrw10\brdrl\brdrdb\brdrbar\brdrs\brdrbtw\brdrdot body\par}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!("paragraph")
        };
        let borders = &p.props.borders;
        let top = borders.top.as_ref().expect("w:top");
        assert_eq!(top.style, Some(BorderStyle::Single));
        assert_eq!(top.size, Some(EighthsPoint(4)));
        assert_eq!(
            borders.start.as_ref().unwrap().style,
            Some(BorderStyle::Double)
        );
        assert_eq!(
            borders.bar.as_ref().unwrap().style,
            Some(BorderStyle::Single)
        );
        assert_eq!(
            borders.between.as_ref().unwrap().style,
            Some(BorderStyle::Dotted)
        );
    }
}

#[test]
fn row_exception_borders_map_to_tbl_prex() {
    // Row edges `\trbrdr*` map to the row exception borders (`w:tblPrEx`).
    let input = br"{\rtf1\trowd\trbrdrt\brdrs\trbrdrb\brdrdb\cellx2000\intbl A\cell\row}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("table")
        };
        let borders = &table.rows[0].props.exception_borders;
        assert_eq!(
            borders.top.as_ref().unwrap().style,
            Some(BorderStyle::Single)
        );
        assert_eq!(
            borders.bottom.as_ref().unwrap().style,
            Some(BorderStyle::Double)
        );
    }
}

#[test]
fn cell_shading_margins_width_and_diagonals() {
    // Cell-level property controls: `\clhidemark`, `\clpad{t,b,l,r}`,
    // `\clftsWidth`/`\clwWidth`, `\clcbpat`/`\clshdng` and `\cldgll`.
    let input = br"{\rtf1{\colortbl;\red255\green0\blue0;}\trowd\clhidemark\clpadt50\clpadl100\clftsWidth3\clwWidth2000\clcbpat1\clshdng10000\cldgll\brdrs\cellx2000\intbl A\cell\row}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("table")
        };
        let cell = &table.rows[0].cells[0].props;
        assert!(cell.hide_mark, "\\clhidemark -> w:hideMark");
        assert_eq!(cell.margins.top, Some(Twips(50)));
        assert_eq!(cell.margins.start, Some(Twips(100)));
        let width = cell.width.expect("w:tcW");
        assert_eq!(width.kind, WidthKind::Dxa);
        assert_eq!(width.value, Some(2000));
        let shading = cell.shading.as_ref().expect("w:shd");
        assert_eq!(shading.pattern.as_deref(), Some("solid"));
        assert_eq!(shading.fill.as_ref().map(Color::as_str), Some("FF0000"));
        assert_eq!(
            cell.borders.tl2br.as_ref().unwrap().style,
            Some(BorderStyle::Single)
        );
    }
}

#[test]
fn brdrtbl_makes_the_cell_edge_inherit() {
    // `\clbrdrt\brdrtbl` means "use the table border": the cell edge override is
    // dropped (inherit), and the token is not a loss.
    let input = br"{\rtf1\trowd\clbrdrt\brdrtbl\clbrdrl\brdrnil\cellx2000\intbl A\cell\row}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("table")
        };
        let borders = &table.rows[0].cells[0].props.borders;
        assert!(
            borders.top.is_none(),
            "\\brdrtbl inherits, no w:top override"
        );
        assert_eq!(
            borders.start.as_ref().unwrap().style,
            Some(BorderStyle::Nil)
        );
    }
}

#[test]
fn table_gap_and_row_shading() {
    // `\trgaph` is the table's half cell gap (`w:tblCellMar`), and
    // `\trcbpat` shades every cell of the row.
    let input = br"{\rtf1{\colortbl;\red255\green0\blue0;}\trowd\trgaph108\trcbpat1\cellx2000\intbl A\cell\row}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Table(table) = &document.body.blocks[0] else {
            panic!("table")
        };
        assert_eq!(table.props.cell_margins.start, Some(Twips(108)));
        assert_eq!(table.props.cell_margins.end, Some(Twips(108)));
        let shading = table.rows[0].cells[0]
            .props
            .shading
            .as_ref()
            .expect("row shading lands on the cell");
        assert_eq!(shading.fill.as_ref().map(Color::as_str), Some("FF0000"));
    }
}

#[test]
fn character_shading_maps_to_run_shading() {
    let input = br"{\rtf1{\colortbl;\red255\green0\blue0;}\chshdng10000\chcbpat1\chcfpat1 shaded}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!("paragraph")
        };
        let Inline::Run(run) = &p.inlines[0] else {
            panic!("run")
        };
        let shading = run.props.shading.as_ref().expect("w:rPr/w:shd");
        assert_eq!(shading.pattern.as_deref(), Some("solid"));
        assert_eq!(shading.fill.as_ref().map(Color::as_str), Some("FF0000"));
    }
}

#[test]
fn character_border_maps_to_run_border() {
    // `\chbrdr` is one box around the run -> `w:rPr/w:bdr` (single edge).
    let input = br"{\rtf1\chbrdr\brdrs\brdrw10 boxed}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let Block::Paragraph(p) = &document.body.blocks[0] else {
            panic!("paragraph")
        };
        let Inline::Run(run) = &p.inlines[0] else {
            panic!("run")
        };
        let border = run.props.borders.top.as_ref().expect("w:rPr/w:bdr");
        assert_eq!(border.style, Some(BorderStyle::Single));
        assert_eq!(border.size, Some(EighthsPoint(4)));
    }
}

#[test]
fn page_borders_map_to_pg_borders() {
    // Page-border edges `\pgbrdr{t,b,l,r}` map to the section's `w:pgBorders`.
    let input = br"{\rtf1{\colortbl;\red255\green0\blue0;}\sectd\pgbrdrt\brdrs\brdrw20\brdrcf1\pgbrdrb\brdrdb body\par}";
    let imported = import(input, &RtfOptions::default()).unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    for document in [imported.document, reopened(input)] {
        let page = document.sections[0]
            .properties
            .page_borders
            .as_ref()
            .expect("w:pgBorders");
        let top = page.top.as_ref().expect("w:pgBorders/w:top");
        assert_eq!(top.style, Some(BorderStyle::Single));
        assert_eq!(top.size, Some(EighthsPoint(8)));
        assert_eq!(top.color.as_ref().map(Color::as_str), Some("FF0000"));
        assert_eq!(
            page.bottom.as_ref().unwrap().style,
            Some(BorderStyle::Double)
        );
    }
}

#[test]
fn nested_tables_are_an_explicit_bounded_reject() {
    // Nested tables (`\itap > 1`) are not flattened: the import rejects with a
    // named feature so the loss is bounded and visible.
    let input = br"{\rtf1\trowd\cellx2000\intbl A\itap2\cell\row}";
    let error = import(input, &RtfOptions::default()).unwrap_err();
    assert!(
        matches!(
            error,
            strict_ooxml_rtf::RtfError::UnsupportedRejected { ref feature, .. }
                if feature == "nested-table-structure"
        ),
        "{error:?}"
    );
}

#[test]
fn table_compat_controls_are_info_not_loss() {
    let imported = import(
        br"{\rtf1\trowd\cgrid\irow0\irowband1\lastrow\snaptogridincell\cellx2000\intbl A\cell\row}",
        &RtfOptions::default(),
    )
    .unwrap();
    assert_eq!(
        imported.report.loss_count, 0,
        "{:?}",
        imported.report.issues
    );
    assert!(imported
        .report
        .issues
        .iter()
        .any(|i| i.feature == "table-compat:cgrid"));
}
