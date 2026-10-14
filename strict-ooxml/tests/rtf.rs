//! Public import outcomes and package-backed media/structure contracts.
#![cfg(feature = "rtf")]
use strict_ooxml::{
    model::{Block, Inline, RunContent},
    RtfOptions, RtfStatus, StrictDocument,
};

#[test]
fn clean_and_degraded_are_independent_of_docx_support() {
    let clean = StrictDocument::import_rtf(br"{\rtf1 Hello}", &RtfOptions::default()).unwrap();
    assert_eq!(clean.status(), RtfStatus::Clean);
    assert_eq!(clean.document.document().body.blocks.len(), 1);
    assert!(clean.strict_bytes.starts_with(b"PK"));
    let mut options = RtfOptions::default();
    options.limits.issues = 0;
    let degraded = StrictDocument::import_rtf(br"{\rtf1 A\unknowncontrol B}", &options).unwrap();
    assert_eq!(degraded.status(), RtfStatus::Degraded);
    assert!(degraded.import_report.details_truncated);
    assert_eq!(degraded.import_report.loss_count, 1);
    assert!(degraded.write_report.losses().is_empty());
    assert!(StrictDocument::import_rtf(br"{\rtf1 unfinished", &options).is_err());
}

#[cfg(feature = "svg")]
#[test]
fn imported_east_asian_font_reaches_the_svg_face_choice() {
    let source = br"{\rtf1\ansi{\fonttbl{\f0 Arial;}{\f1 Times New Roman;}}\loch\f0 Latin \dbch\af1\u28450?\u23383?}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    assert_eq!(opened.status(), RtfStatus::Clean);
    let pages = opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap();
    assert!(
        pages[0].svg.contains("Arimo"),
        "Latin face was not selected"
    );
    assert!(
        pages[0].svg.contains("Tinos"),
        "East Asian face was not selected"
    );
}

#[cfg(feature = "svg")]
#[test]
fn note_numbering_properties_reach_the_svg_marker() {
    // `\ftnnar`/`\ftnstart` must reach `settings.w:footnotePr` so the rendered
    // footnote marker is the source's number, not the default "1".
    let source = br"{\rtf1\ftnnalc\ftnstart3 A\chftn{\footnote\chftn Note}\par}";
    let opened = StrictDocument::import_rtf(source, &RtfOptions::default()).unwrap();
    assert_eq!(opened.status(), RtfStatus::Clean);
    let svg = &opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap()[0]
        .svg;
    assert!(
        svg.contains(">c<"),
        "footnote marker did not use \\ftnnalc/\\ftnstart3"
    );
    assert!(!svg.contains(">1<"), "default footnote marker leaked");
}

#[test]
fn public_path_keeps_region_references_and_cached_link_result() {
    let opened = StrictDocument::import_rtf(br#"{\rtf1{\header Header}A\chftn{\footnote\chftn Note}{\field{\*\fldinst HYPERLINK "https://example.test"}{\fldrslt Link}}}"#, &RtfOptions::default()).unwrap();
    let d = opened.document.document();
    assert_eq!(d.headers_footers.len(), 1);
    assert_eq!(d.footnotes.len(), 1);
    let Block::Paragraph(p) = &d.body.blocks[0] else {
        panic!()
    };
    assert!(p
        .inlines
        .iter()
        .any(|i| matches!(i,Inline::Run(r) if r.content.contains(&RunContent::FootnoteRef(1)))));
    assert!(p.inlines.iter().any(|i| matches!(i,Inline::Field(f) if f.instruction.as_deref() == Some("HYPERLINK \"https://example.test\""))));
    #[cfg(feature = "svg")]
    assert!(!opened
        .document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap()
        .is_empty());
}

#[test]
fn imported_media_outlives_input_and_initial_package_bytes() {
    let hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63606060f80f00010401005fe5c34b0000000049454e44ae426082";
    let png: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    let mut source = format!(
        "{{\\rtf1 before{{\\pict\\pngblip\\picw1\\pich1\\picwgoal15\\pichgoal15\\bin{} ",
        png.len()
    )
    .into_bytes();
    source.extend_from_slice(&png);
    source.extend_from_slice(b"}after}");
    let opened = StrictDocument::import_rtf(&source, &RtfOptions::default()).unwrap();
    assert_eq!(opened.status(), RtfStatus::Clean);
    let document = opened.document;
    drop(opened.strict_bytes);
    drop(source);
    let image = document.document().media.iter().next().unwrap();
    assert_eq!(document.package().read_part(&image.part).unwrap(), png);
    #[cfg(feature = "svg")]
    assert!(document
        .render_svg(&strict_ooxml::RenderOptions::default())
        .unwrap()[0]
        .svg
        .contains("data:image/png;base64,"));
}
