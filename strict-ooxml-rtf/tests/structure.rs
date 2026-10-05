//! Structural import contracts, independent of the importer's implementation.
#![allow(clippy::default_trait_access)]
use strict_ooxml_rtf::{import, RtfOptions};
use strict_ooxml_wml::model::{
    AbstractNumId, Block, Ilvl, Inline, NumId, RunContent, StyleId, TriState,
};

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
        assert!(written.report.losses().is_empty());
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
fn multi_paragraph_field_keeps_cached_boundaries_and_reports_instruction_loss() {
    let source = br"{\rtf1 A{\field{\*\fldinst TOC}{\fldrslt B\par C\par}}\par D}";
    let imported = import(source, &RtfOptions::default()).unwrap();
    assert!(imported
        .report
        .issues
        .iter()
        .any(|i| i.feature == "field:multi-paragraph-instruction-dropped"));
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
