#![cfg(feature = "visual")]
//! Caret regressions use the actual shared renderer's placement.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    scalar_to_utf16, utf16_to_scalar, Address, EditError, EditLimits, Editor, TextPosition,
};
use strict_ooxml_render_svg::{place_pages, Item, RenderOptions};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};
#[test]
fn caret_matches_renderer_and_hits_exact_unicode_boundaries() {
    let xml="<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body><w:p><w:r><w:t>А🙂Б</w:t></w:r></w:p></w:body></w:document>";
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.as_bytes())
        .build();
    let p = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&p, &ParseOptions::default()).unwrap();
    let opts = RenderOptions::default();
    let placed = place_pages(&d, &opts, Some(&p)).unwrap();
    let text = placed[0]
        .items
        .iter()
        .find_map(|i| if let Item::Text(t) = i { Some(t) } else { None })
        .unwrap();
    let editor = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = editor.visual_map(0, &opts, Some(&p)).unwrap();
    let pos = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 0,
    };
    let rects = map.caret(0, &pos).unwrap();
    assert!(!rects.is_empty());
    assert!((rects[0].x - text.x).abs() < 1e-7);
    let hit = map
        .hit_test(0, 0, rects[0].x, rects[0].y + rects[0].height / 2.0)
        .unwrap()
        .unwrap();
    assert_eq!(hit, pos);
    let selected = map.selection(0, &pos, 3).unwrap();
    assert!(!selected.is_empty());
    assert!((selected.iter().map(|r| r.width).sum::<f64>() - text.width).abs() < 1e-7);
    assert_eq!(map.caret(1, &pos), Err(EditError::StaleRevision));
}
#[test]
fn utf16_conversion_rejects_surrogate_interiors() {
    assert_eq!(utf16_to_scalar("А🙂Б", 3), Ok(2));
    assert_eq!(scalar_to_utf16("А🙂Б", 2), Ok(3));
    assert_eq!(utf16_to_scalar("А🙂Б", 2), Err(EditError::InvalidRange));
    assert_eq!(scalar_to_utf16("a", 2), Err(EditError::InvalidRange));
}
#[test]
fn grapheme_carets_caps_and_duplicate_text_never_guess_source() {
    let xml="<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body><w:p><w:r><w:rPr><w:caps/><w:color w:val=\"000001\"/></w:rPr><w:t>e\u{301}ß</w:t><w:t>same</w:t></w:r></w:p><w:p><w:r><w:t>same</w:t></w:r></w:p></w:body></w:document>";
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.as_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&package, &ParseOptions::default()).unwrap();
    let original = d.body.clone();
    let e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = e
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    let mut pos = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 1,
    };
    assert!(map.caret(0, &pos).unwrap().is_empty());
    pos.offset = 2;
    assert!(!map.caret(0, &pos).unwrap().is_empty());
    pos.content = 1;
    pos.offset = 0;
    let first = map.caret(0, &pos).unwrap();
    assert!(!first.is_empty());
    let later = TextPosition {
        paragraph: Address::body(1),
        inline: vec![0],
        content: 0,
        offset: 0,
    };
    let second = map.caret(0, &later).unwrap();
    assert!(!second.is_empty());
    assert!((first[0].y - second[0].y).abs() > 1e-7);
    let hit = map
        .hit_test(0, 0, second[0].x, second[0].y + second[0].height / 2.0)
        .unwrap()
        .unwrap();
    assert_eq!(hit, later);
    assert_eq!(e.document().body, original);
}
#[test]
fn wrapped_table_text_uses_page_scale_and_rejects_invalid_coordinates() {
    let text = "one two three four five six seven eight nine ten ".repeat(8);
    let body=format!("<w:tbl><w:tblGrid><w:gridCol w:w=\"1800\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>");
    let bytes = DocxBuilder::strict().body(&body).build();
    let p = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&p, &ParseOptions::default()).unwrap();
    let e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = e
        .visual_map(0, &RenderOptions::default().scale(144.0), Some(&p))
        .unwrap();
    let pos = TextPosition {
        paragraph: Address {
            story: strict_ooxml_edit::Story::Body,
            containers: vec![strict_ooxml_edit::Container::Cell {
                table: 0,
                row: 0,
                cell: 0,
            }],
            block: 0,
        },
        inline: vec![0],
        content: 0,
        offset: 0,
    };
    let rectangles = map.selection(0, &pos, text.chars().count()).unwrap();
    assert!(rectangles.len() > 10);
    assert!(rectangles.iter().any(|r| r.y > rectangles[0].y));
    assert_eq!(
        map.hit_test(0, 0, f64::NAN, 0.0),
        Err(EditError::InvalidRange)
    );
}

#[test]
fn selection_crosses_runs_and_paragraphs_in_logical_order() {
    let bytes=DocxBuilder::strict().body("<w:p><w:r><w:t>abc</w:t></w:r><w:r><w:t>def</w:t></w:r></w:p><w:p><w:r><w:t>ghi</w:t></w:r></w:p>").build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&package, &ParseOptions::default()).unwrap();
    let editor = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    let anchor = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 1,
    };
    let focus = TextPosition {
        paragraph: Address::body(1),
        inline: vec![0],
        content: 0,
        offset: 2,
    };
    let rectangles = map.selection_between(0, &anchor, &focus).unwrap();
    assert_eq!(rectangles.len(), 7);
    assert!(rectangles.last().unwrap().y > rectangles[0].y);
    assert_eq!(
        rectangles,
        map.selection_between(0, &focus, &anchor).unwrap()
    );
    let mut invalid = focus.clone();
    invalid.offset = 4;
    assert_eq!(
        map.selection_between(0, &anchor, &invalid),
        Err(EditError::InvalidRange)
    );
    assert_eq!(
        map.selection_between(1, &anchor, &focus),
        Err(EditError::StaleRevision)
    );
}

#[test]
fn empty_paragraph_has_a_caret_without_changing_neighbor_layout() {
    let bytes = DocxBuilder::strict()
        .body("<w:p/><w:p><w:r><w:t>neighbor</w:t></w:r></w:p>")
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&package, &ParseOptions::default()).unwrap();
    let original = d.body.clone();
    let mut editor = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    let pos = TextPosition {
        paragraph: Address::body(0),
        inline: vec![],
        content: 0,
        offset: 0,
    };
    let rects = map.caret(0, &pos).unwrap();
    assert_eq!(rects.len(), 1);
    assert_eq!(
        map.hit_test(0, 0, rects[0].x, rects[0].y + rects[0].height / 2.0)
            .unwrap(),
        Some(pos)
    );
    assert_eq!(editor.document().body, original);
    editor
        .transact(
            0,
            &[strict_ooxml_edit::Edit::Text {
                at: Address::body(0),
                range: 0..0,
                text: "first".into(),
            }],
        )
        .unwrap();
    let map = editor
        .visual_map(1, &RenderOptions::default(), Some(&package))
        .unwrap();
    let pos = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 5,
    };
    assert_eq!(map.caret(1, &pos).unwrap().len(), 1);
}
