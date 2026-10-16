#![cfg(feature = "save")]
//! Saved bytes are independently reopened and their text/media inspected.
use strict_ooxml_core::{
    opc::{OpenOptions, Package},
    pipeline::{PipelineIssue, PipelineOutcome, PipelineStage, PipelineSummary},
};
use strict_ooxml_edit::{Address, Edit, EditError, EditLimits, Editor, SaveError, SavePolicy};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::WriteOptions;
fn package() -> Package {
    let xml="<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body><w:p><w:r><w:t>abc</w:t></w:r></w:p></w:body></w:document>";
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.as_bytes())
        .build();
    Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap()
}
#[test]
fn save_roundtrip_and_refresh_do_not_destroy_history() {
    let p = package();
    let mut d = parse_document(&p, &ParseOptions::default()).unwrap();
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Text {
            at: Address::body(0),
            range: 1..2,
            text: "🙂 ".into(),
        }],
    )
    .unwrap();
    assert!(e.support_is_stale());
    let saved = e
        .save(1, Some(&p), &WriteOptions::default(), SavePolicy::Lossless)
        .unwrap();
    assert_eq!(saved.pipeline.outcome, PipelineOutcome::Clean);
    let out = Package::open_reader(&saved.bytes[..], &OpenOptions::default()).unwrap();
    let xml = String::from_utf8(
        out.read_part(&strict_ooxml_core::part::PartId::new("/word/document.xml"))
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains("🙂 "));
    assert!(xml.contains("xml:space=\"preserve\""));
    let reopened = parse_document(&out, &ParseOptions::default()).unwrap();
    assert_eq!(reopened.body.blocks.len(), 1);
    e.refresh_support(1, Some(&p), &WriteOptions::default())
        .unwrap();
    assert!(!e.support_is_stale());
    assert_eq!(
        e.document().support.debug_summary(),
        reopened.support.debug_summary()
    );
    e.undo(1).unwrap();
    assert!(e.support_is_stale());
    assert_eq!(e.document().body, original);
}
#[test]
fn prior_losses_are_preserved_and_policy_is_explicit() {
    let p = package();
    let mut d = parse_document(&p, &ParseOptions::default()).unwrap();
    let mut input = PipelineSummary::new();
    input.extend_issues([PipelineIssue {
        stage: PipelineStage::Normalize,
        id: "lost-shape".into(),
        severity: "lossy".into(),
        part: None,
        page: None,
        location: None,
        count: 1,
        detail: "normalized input lost a shape".into(),
    }]);
    let mut e = Editor::new(&mut d, EditLimits::default())
        .unwrap()
        .with_pipeline(input);
    assert!(matches!(
        e.save(0, Some(&p), &WriteOptions::default(), SavePolicy::Lossless),
        Err(SaveError::Rejected(_))
    ));
    let result = e
        .save(
            0,
            Some(&p),
            &WriteOptions::default(),
            SavePolicy::AllowDegraded,
        )
        .unwrap();
    assert_eq!(result.pipeline.outcome, PipelineOutcome::Degraded);
    assert!(result.pipeline.issues.iter().any(|i| i.id == "lost-shape"));
    assert!(e
        .refresh_support(0, Some(&p), &WriteOptions::default())
        .is_err());
    assert!(matches!(
        e.save(
            1,
            Some(&p),
            &WriteOptions::default(),
            SavePolicy::AllowDegraded
        ),
        Err(SaveError::Edit(EditError::StaleRevision))
    ));
}
#[test]
fn writer_losses_do_not_become_clean_after_reopen() {
    let p = package();
    let mut d = parse_document(&p, &ParseOptions::default()).unwrap();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let opaque = strict_ooxml_wml::model::Inline::Opaque(strict_ooxml_wml::model::OpaqueInline {
        namespace: "urn:unsupported".into(),
        local: "payload".into(),
        attributes: vec![],
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    });
    e.transact(
        0,
        &[Edit::InsertInline {
            at: Address::body(0),
            index: 1,
            inline: Box::new(opaque),
        }],
    )
    .unwrap();
    assert!(matches!(
        e.save(1, Some(&p), &WriteOptions::default(), SavePolicy::Lossless),
        Err(SaveError::Rejected(_))
    ));
    let saved = e
        .save(
            1,
            Some(&p),
            &WriteOptions::default(),
            SavePolicy::AllowDegraded,
        )
        .unwrap();
    assert_eq!(saved.pipeline.outcome, PipelineOutcome::Degraded);
    assert!(saved
        .pipeline
        .issues
        .iter()
        .any(|i| i.stage == PipelineStage::Write));
    assert!(e.support_is_stale());
}
#[test]
fn picture_edit_and_save_preserves_original_pixels_and_extent() {
    use strict_ooxml_wml::model::*;
    let (source, mut d, mut drawing, pixels) = picture_fixture();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::InsertInline {
            at: Address::body(0),
            index: 0,
            inline: Box::new(Inline::Drawing(drawing.clone())),
        }],
    )
    .unwrap();
    if let DrawingKind::Inline(v) = &mut drawing.kind {
        v.extent = Some(Extent {
            cx: Emu(1_828_800),
            cy: Emu(914_400),
        });
    }
    e.transact(
        1,
        &[Edit::Drawing {
            at: Address::body(0),
            inline: vec![0],
            content: None,
            drawing: Box::new(drawing),
        }],
    )
    .unwrap();
    #[cfg(feature = "visual")]
    {
        let opts = strict_ooxml_render_svg::RenderOptions::default();
        let map = e.visual_map(2, &opts, Some(&source)).unwrap();
        let target = strict_ooxml_edit::DrawingPosition {
            paragraph: Address::body(0),
            inline: vec![0],
            content: None,
        };
        let rects = map.drawing_rects(2, &target).unwrap();
        assert_eq!(rects.len(), 1);
        assert!((rects[0].width - 192.0).abs() < 1e-7);
        assert_eq!(
            map.hit_drawing(2, rects[0].page, rects[0].x + 1.0, rects[0].y + 1.0)
                .unwrap(),
            Some(target)
        );
    }
    let saved = e
        .save(
            2,
            Some(&source),
            &WriteOptions::default(),
            SavePolicy::Lossless,
        )
        .unwrap();
    let out = Package::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    let reopened = parse_document(&out, &ParseOptions::default()).unwrap();
    let media = reopened.media.iter().next().unwrap();
    assert_eq!(out.read_part(&media.part).unwrap(), pixels);
    let paragraph = reopened.body.blocks[0].as_paragraph().unwrap();
    let Inline::Drawing(drawing) = &paragraph.inlines[0] else {
        panic!()
    };
    let DrawingKind::Inline(inline) = &drawing.kind else {
        panic!()
    };
    assert_eq!(inline.extent.unwrap().cx, Emu(1_828_800));
}

fn picture_fixture() -> (
    Package,
    strict_ooxml_wml::model::Document,
    strict_ooxml_wml::model::Drawing,
    Vec<u8>,
) {
    use strict_ooxml_core::{error::SourceLocation, part::PartId};
    use strict_ooxml_wml::model::*;
    let mut pixels = vec![];
    {
        let mut encoder = png::Encoder::new(&mut pixels, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 0, 0, 255]).unwrap();
    }
    let bytes = DocxBuilder::strict()
        .body("<w:p/>")
        .rel("rIdImage", "image", "media/input.png")
        .part("word/media/input.png", pixels.clone())
        .content_type("/word/media/input.png", "image/png")
        .build();
    let source = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&source, &ParseOptions::default()).unwrap();
    let part = PartId::new("/word/media/input.png");
    d.media.insert(MediaItem {
        part: part.clone(),
        content_type: Some("image/png".into()),
        kind: MediaKind::Png,
    });
    let location = SourceLocation::unknown();
    let extent = Extent {
        cx: Emu(914_400),
        cy: Emu(914_400),
    };
    let drawing = Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(extent),
            effect_extent: None,
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            doc_pr: Some(DocPr {
                hidden: None,
                id: Some(1),
                name: Some("red pixel".into()),
                descr: None,
                title: None,
            }),
            graphic_uri: Some("http://purl.oclc.org/ooxml/drawingml/picture".into()),
            graphic: Box::new(Graphic::Picture(Picture {
                name: Some("red pixel".into()),
                descr: None,
                nv_id: None,
                bw_mode: None,
                blip: Some(BlipRef {
                    embed: None,
                    link: None,
                    resolved: Some(part),
                    cstate: None,
                    location: location.clone(),
                }),
                extent: Some(extent),
                src_rect: None,
                xfrm: None,
                markup: strict_ooxml_wml::model::drawing::PictureMarkup::default(),
            })),
            location: location.clone(),
        }),
        location,
    };
    (source, d, drawing, pixels)
}

/// A picture the session adds: its bytes kept by the editor, its media entry
/// and drawing in one transaction; drawn through the media layer, saved with
/// its bytes, and gone again after one undo.
#[test]
fn an_added_picture_is_drawn_saved_and_undone_in_one_step() {
    use strict_ooxml_edit::{
        picture_drawing, picture_pixels, unused_drawing_id, Definition, MediaLayer, EMU_PER_PX,
    };
    use strict_ooxml_wml::model::*;
    let mut pixels = vec![];
    {
        let mut encoder = png::Encoder::new(&mut pixels, 3, 2);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0u8; 3 * 2 * 4]).unwrap();
    }
    assert_eq!(picture_pixels(&pixels), Some((3, 2)));
    let source = package();
    let mut d = parse_document(&source, &ParseOptions::default()).unwrap();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    assert!(e.add_media(b"not a picture".to_vec()).is_err());
    let item = e.add_media(pixels.clone()).unwrap();
    let drawing = picture_drawing(
        &item,
        Emu(3 * EMU_PER_PX),
        Emu(2 * EMU_PER_PX),
        unused_drawing_id(e.document()),
        "Picture",
    );
    e.define_and_transact(
        0,
        &[Definition::AddMedia(Box::new(item.clone()))],
        &[Edit::InsertInline {
            at: Address::body(0),
            index: 0,
            inline: Box::new(Inline::Drawing(drawing)),
        }],
    )
    .unwrap();
    #[cfg(feature = "visual")]
    {
        let media = MediaLayer::new(e.added_media(), Some(&source));
        let pages = strict_ooxml_render_svg::render_with_media(
            e.document(),
            &strict_ooxml_render_svg::RenderOptions::default(),
            Some(&media),
        )
        .unwrap();
        assert!(
            pages[0].svg.contains("data:image/png;base64,"),
            "{}",
            pages[0].svg
        );
    }
    let media = MediaLayer::new(e.added_media(), Some(&source));
    let saved = e
        .save(
            1,
            Some(&media),
            &WriteOptions::default(),
            SavePolicy::AllowDegraded,
        )
        .unwrap();
    let out = Package::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    let reopened = parse_document(&out, &ParseOptions::default()).unwrap();
    let written = reopened.media.iter().next().expect("the picture's part");
    assert_eq!(out.read_part(&written.part).unwrap(), pixels);

    e.undo(1).unwrap();
    assert!(e.document().media.get(&item.part).is_none());
    let paragraph = e.document().body.blocks[0].as_paragraph().unwrap();
    assert!(!paragraph
        .inlines
        .iter()
        .any(|i| matches!(i, Inline::Drawing(_))));
}

/// A new header: its part and the section's reference to it in one
/// transaction; it takes text and a page number field, is written as a
/// header part, and one undo takes it back.
#[test]
fn a_new_header_is_one_transaction_written_and_undone() {
    use strict_ooxml_edit::{header_footer_reference, new_header_footer, Definition, Story};
    use strict_ooxml_wml::model::props::HeaderFooterKind;
    use strict_ooxml_wml::model::*;
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>abc</w:t></w:r></w:p><w:sectPr/>")
        .build();
    let source = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&source, &ParseOptions::default()).unwrap();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let header = new_header_footer(e.document(), true);
    let part = header.part.clone();
    let mut section = e
        .document()
        .sections
        .last()
        .map(|s| s.properties.clone())
        .unwrap_or_default();
    section
        .headers
        .push(header_footer_reference(&part, HeaderFooterKind::Default));
    e.define_and_transact(
        0,
        &[Definition::AddHeaderFooter(Box::new(header))],
        &[Edit::SectionProperties {
            at: Address::body(0),
            properties: Box::new(section),
        }],
    )
    .unwrap();
    let at = Address {
        story: Story::HeaderFooter(part.clone()),
        containers: vec![],
        block: 0,
    };
    let field = Inline::Field(Field {
        instruction: Some(" PAGE ".into()),
        inlines: vec![],
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    });
    e.transact(
        1,
        &[
            Edit::Text {
                at: at.clone(),
                range: 0..0,
                text: "Page ".into(),
            },
            Edit::InsertInline {
                at: at.clone(),
                index: 1,
                inline: Box::new(field),
            },
        ],
    )
    .unwrap();
    let saved = e
        .save(
            2,
            Some(&source),
            &WriteOptions::default(),
            SavePolicy::AllowDegraded,
        )
        .unwrap();
    let out = Package::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    let reopened = parse_document(&out, &ParseOptions::default()).unwrap();
    let written = reopened
        .headers_footers
        .iter()
        .find(|h| h.is_header)
        .expect("a header part");
    let paragraph = written.blocks[0].as_paragraph().unwrap();
    assert!(paragraph
        .inlines
        .iter()
        .any(|i| matches!(i, Inline::Field(f) if f.instruction.as_deref().is_some_and(|x| x.contains("PAGE")))));
    e.undo(e.revision()).unwrap();
    e.undo(e.revision()).unwrap();
    assert!(e.document().header_footer(&part).is_none());
    assert!(e
        .document()
        .sections
        .iter()
        .all(|s| s.properties.headers.is_empty()));
}
