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
fn supplied_cjk_font_selection_carets_and_hits_match_fullwidth_glyphs() {
    use std::sync::Arc;
    use strict_ooxml_render_svg::font::{FontAsset, FontAssetLimits, FontAssets};
    // Existing OFL witness: these six CJK glyphs each advance exactly 1 em.
    let bytes: Arc<[u8]> = Arc::from(
        include_bytes!("../../strict-ooxml-render-svg/fonts/witnesses/cjk.otf").as_slice(),
    );
    let assets = Arc::new(
        FontAssets::new(
            vec![FontAsset::new("Chinese Witness", false, false, bytes).fallback("zh-CN")],
            FontAssetLimits::default(),
        )
        .unwrap(),
    );
    for scale in [100, 80] {
        let body = format!(
            r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Chinese Witness" w:eastAsia="Chinese Witness"/><w:sz w:val="24"/><w:lang w:eastAsia="zh-CN"/><w:spacing w:val="30"/><w:w w:val="{scale}"/></w:rPr><w:t>漢字中文测试</w:t></w:r></w:p>"#
        );
        let bytes = DocxBuilder::strict().body(&body).build();
        let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
        let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
        let opts = RenderOptions::default().font_assets(assets.clone());
        let placed = place_pages(&document, &opts, Some(&package)).unwrap();
        let text = placed[0]
            .items
            .iter()
            .flat_map(Item::leaves)
            .find_map(|item| {
                if let Item::Text(text) = item {
                    Some(text)
                } else {
                    None
                }
            })
            .unwrap();
        let origin = text.x;
        // 30 twips character spacing = 1.5 pt = 2 px at the default 96 dpi.
        let step = (text.size_px + 2.0) * f64::from(scale) / 100.0;
        let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
        let map = editor.visual_map(0, &opts, Some(&package)).unwrap();
        for offset in 0..=6 {
            let position = TextPosition {
                paragraph: Address::body(0),
                inline: vec![0],
                content: 0,
                offset,
            };
            let carets = map.caret(0, &position).unwrap();
            let expected = origin + f64::from(u32::try_from(offset).unwrap()) * step;
            assert!(
                carets.iter().any(|r| (r.x - expected).abs() < 0.01),
                "{offset}: {carets:?}, expected {expected}"
            );
            if offset < 6 {
                let selected = map.selection(0, &position, offset + 1).unwrap();
                let width: f64 = selected.iter().map(|r| r.width).sum();
                assert!(
                    (width - step).abs() < 0.01,
                    "{offset}: width {width}, expected {step}"
                );
                let rect = selected[0];
                let hit = map
                    .hit_test(0, rect.page, expected + 0.1, rect.y + rect.height / 2.0)
                    .unwrap()
                    .unwrap();
                assert_eq!(hit.offset, offset);
            }
        }
    }
}

#[test]
fn mixed_cjk_and_latin_carets_follow_the_svg_glyph_origins() {
    use std::sync::Arc;
    use strict_ooxml_render_svg::font::{FontAsset, FontAssetLimits, FontAssets};
    let source = "AV漢字中文";
    let font: Arc<[u8]> = Arc::from(
        include_bytes!("../../strict-ooxml-render-svg/fonts/witnesses/cjk.otf").as_slice(),
    );
    let assets = FontAssets::new(
        vec![FontAsset::new("Chinese Witness", false, false, font)],
        FontAssetLimits::default(),
    )
    .unwrap();
    let opts = RenderOptions::default().font_assets(Arc::new(assets));
    let body = format!(
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Chinese Witness" w:eastAsia="Chinese Witness"/><w:sz w:val="24"/><w:spacing w:val="30"/><w:w w:val="80"/><w:kern w:val="2"/></w:rPr><w:t>{source}</w:t></w:r></w:p>"#
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
    let pages = place_pages(&document, &opts, Some(&package)).unwrap();
    let svg = strict_ooxml_render_svg::paint_page(&pages[0], &opts);
    let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
    let map = editor.visual_map(0, &opts, Some(&package)).unwrap();
    for item in pages[0].items.iter().flat_map(Item::leaves) {
        let Item::Text(text) = item else {
            continue;
        };
        if text.text.is_empty() {
            continue;
        }
        let byte = source
            .find(&text.text)
            .expect("fragment belongs to the source");
        let start = source[..byte].chars().count();
        let line = svg
            .lines()
            .find(|line| line.contains(&format!(">{}</text>", text.text)))
            .expect("painted fragment");
        let xs = line
            .split(" x=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        for (index, x) in xs.split_whitespace().enumerate() {
            let x: f64 = x.parse().unwrap();
            let expected = text.x + (x - text.x) * text.run.char_scale;
            let position = TextPosition {
                paragraph: Address::body(0),
                inline: vec![0],
                content: 0,
                offset: start + index,
            };
            let rects = map.caret(0, &position).unwrap();
            assert!(
                rects.iter().any(|rect| (rect.x - expected).abs() < 0.02),
                "{position:?}: {rects:?}, SVG x={expected}"
            );
        }
    }
}
/// A justified line widens the space after a word. That boundary is the end
/// of one fragment and the start of the next, and both used to be drawn.
#[test]
fn a_justified_word_boundary_is_one_caret() {
    let words = "alpha beta gamma delta epsilon zeta eta theta ".repeat(8);
    let body = format!(
        "<w:p><w:pPr><w:jc w:val=\"both\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{words}</w:t></w:r></w:p>"
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
    let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    let paragraph = Address::body(0);
    for offset in 0..=words.chars().count() {
        let pos = TextPosition {
            paragraph: paragraph.clone(),
            inline: vec![0],
            content: 0,
            offset,
        };
        let rects = map.caret(0, &pos).unwrap();
        let mut lines = Vec::new();
        for rect in &rects {
            #[allow(clippy::cast_possible_truncation)] // Bounded page coordinates, quantized to tenths.
            let y = (rect.y * 10.0).round() as i64;
            assert!(
                !lines.contains(&(rect.page, y)),
                "offset {offset} has two carets on one line: {rects:?}"
            );
            lines.push((rect.page, y));
        }
    }
}

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

/// Six empty bordered rows: the attribution copy puts a zero-width space in
/// each, and the layout then sums the row heights along another path. The
/// rules below the rows came out 2.8e-14 px apart, exact comparison refused
/// the map, and an editor lost its caret after inserting a table.
#[test]
fn empty_table_rows_keep_their_carets_despite_float_rounding() {
    let border = r#"w:val="single" w:sz="4" w:space="0" w:color="auto""#;
    // Word's Normal: 1.08 lines and 8 pt after, the fractions that round.
    let empty =
        r#"<w:p><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:p>"#;
    let row = format!("<w:tr><w:tc>{empty}</w:tc></w:tr>").repeat(6);
    let body = format!(
        "<w:p><w:r><w:t>before</w:t></w:r></w:p><w:tbl><w:tblPr><w:tblBorders>\
         <w:top {border}/><w:start {border}/><w:bottom {border}/><w:end {border}/>\
         <w:insideH {border}/><w:insideV {border}/></w:tblBorders></w:tblPr>\
         <w:tblGrid><w:gridCol w:w=\"9355\"/></w:tblGrid>{row}</w:tbl>{empty}"
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&package, &ParseOptions::default()).unwrap();
    let editor = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    for row in 0..6 {
        let pos = TextPosition {
            paragraph: Address {
                story: strict_ooxml_edit::Story::Body,
                containers: vec![strict_ooxml_edit::Container::Cell {
                    table: 1,
                    row,
                    cell: 0,
                }],
                block: 0,
            },
            inline: vec![],
            content: 0,
            offset: 0,
        };
        assert_eq!(map.caret(0, &pos).unwrap().len(), 1, "row {row}");
    }
}

/// A click on an empty cell's line puts the caret in that cell - the
/// bottom right one included, whose only caret stop is at its left edge while
/// the paragraph below the table is nearer the click in a straight line.
#[test]
fn a_click_inside_any_empty_cell_lands_in_that_cell() {
    let border = r#"w:val="single" w:sz="4" w:space="0" w:color="auto""#;
    let empty =
        r#"<w:p><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:p>"#;
    let cells = format!("<w:tc>{empty}</w:tc>").repeat(4);
    let rows = format!("<w:tr>{cells}</w:tr>").repeat(4);
    let body = format!(
        "<w:p><w:r><w:t>before</w:t></w:r></w:p><w:tbl><w:tblPr><w:tblBorders>\
         <w:top {border}/><w:start {border}/><w:bottom {border}/><w:end {border}/>\
         <w:insideH {border}/><w:insideV {border}/></w:tblBorders></w:tblPr>\
         <w:tblGrid>{}</w:tblGrid>{rows}</w:tbl>{empty}",
        "<w:gridCol w:w=\"2338\"/>".repeat(4)
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&package, &ParseOptions::default()).unwrap();
    let editor = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    // A cell is 2338 twips wide: about 156 px at 96 dpi.
    let cell_px = 2338.0 / 15.0;
    for row in 0..4 {
        for cell in 0..4 {
            let pos = TextPosition {
                paragraph: Address {
                    story: strict_ooxml_edit::Story::Body,
                    containers: vec![strict_ooxml_edit::Container::Cell {
                        table: 1,
                        row,
                        cell,
                    }],
                    block: 0,
                },
                inline: vec![],
                content: 0,
                offset: 0,
            };
            let caret = map.caret(0, &pos).unwrap()[0];
            // Three quarters of the way across the cell, on its line; in the
            // last row also below the line, where the paragraph after the
            // table is the nearest thing in a straight line.
            let x = caret.x + cell_px * 0.75;
            let mut ys = vec![caret.y + caret.height / 2.0];
            if row == 3 {
                ys.push(caret.y + caret.height + 6.0);
            }
            for y in ys {
                assert_eq!(
                    map.hit_test(0, caret.page, x, y).unwrap(),
                    Some(pos.clone()),
                    "row {row} cell {cell} y {y}"
                );
            }
        }
    }
}

fn tab_map(body: &str) -> (strict_ooxml_edit::VisualMap, Package) {
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&package, &ParseOptions::default()).unwrap();
    let editor = Editor::new(&mut d, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    (map, package)
}

fn at(inline: Vec<usize>, content: usize, offset: usize) -> TextPosition {
    TextPosition {
        paragraph: Address::body(0),
        inline,
        content,
        offset,
    }
}

/// A tab that ends a paragraph has a caret after it, a default stop to the
/// right of the text, and a click right of it lands there.
#[test]
fn a_trailing_tab_has_a_caret_after_it() {
    let (map, _package) = tab_map("<w:p><w:r><w:t>a</w:t></w:r><w:r><w:tab/></w:r></w:p>");
    let line_start = map.caret(0, &at(vec![0], 0, 0)).unwrap()[0];
    let text_end = map.caret(0, &at(vec![0], 0, 1)).unwrap()[0];
    let after = map.caret(0, &at(vec![1], 0, 1)).unwrap();
    assert_eq!(after.len(), 1, "a caret after the tab");
    // The default stop is 720 twips: 48 px from the line's left edge.
    assert!(
        (after[0].x - line_start.x - 48.0).abs() < 0.5,
        "after the tab at {}, line starts at {}",
        after[0].x,
        line_start.x
    );
    assert!((after[0].y - text_end.y).abs() < 0.5, "on the same line");
    let hit = map
        .hit_test(0, 0, after[0].x + 200.0, after[0].y + after[0].height / 2.0)
        .unwrap();
    assert_eq!(hit, Some(at(vec![1], 0, 1)));
}

/// A line of tabs only: carets before, between and after them, one default
/// stop apart.
#[test]
fn a_line_of_tabs_only_has_a_caret_at_every_stop() {
    let (map, _package) = tab_map("<w:p><w:r><w:tab/><w:tab/></w:r></w:p>");
    let x = |position: TextPosition| {
        let rects = map.caret(0, &position).unwrap();
        assert_eq!(rects.len(), 1, "{position:?}");
        rects[0].x
    };
    let start = x(at(vec![0], 0, 0));
    let middle = x(at(vec![0], 0, 1));
    let between = x(at(vec![0], 1, 0));
    let end = x(at(vec![0], 1, 1));
    assert!((middle - between).abs() < 1e-6, "{middle} {between}");
    assert!((middle - start - 48.0).abs() < 0.5, "{start} {middle}");
    assert!((end - middle - 48.0).abs() < 0.5, "{middle} {end}");
}

/// Spaces a line starts with are laid out as an advance, not drawn; their
/// carets stand in that advance, before the first word's.
#[test]
fn leading_spaces_have_carets_before_the_first_word() {
    let (map, _package) = tab_map("<w:p><w:r><w:t xml:space=\"preserve\">  word</w:t></w:r></w:p>");
    let x = |offset: usize| {
        map.caret(0, &at(vec![0], 0, offset))
            .expect("caret")
            .first()
            .map_or_else(|| panic!("no caret at {offset}"), |rect| rect.x)
    };
    assert!(
        x(0) < x(1) && x(1) < x(2) && x(2) < x(3),
        "{} {} {} {}",
        x(0),
        x(1),
        x(2),
        x(3)
    );
}

#[test]
fn empty_annotated_paragraph_keeps_its_virtual_caret() {
    let (map, _) = tab_map(
        "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"Start\"/><w:bookmarkEnd w:id=\"1\"/></w:p>",
    );
    let pos = at(vec![], 0, 0);
    let rect = map.caret(0, &pos).unwrap()[0];
    assert_eq!(
        map.hit_test(0, rect.page, rect.x, rect.y).unwrap(),
        Some(pos)
    );
}

#[test]
fn formulas_have_independent_hit_boxes_and_two_caret_boundaries() {
    let body = concat!("<w:p xmlns:m=\"http://purl.oclc.org/ooxml/officeDocument/math\">",
        "<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>",
        "<m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath></w:p>");
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
    let original = document.body.clone();
    let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    for index in 0..2 {
        let pos = at(vec![index], 0, 0);
        let rects = map.formula_rects(0, &pos).unwrap();
        assert_eq!(rects.len(), 1);
        let rect = rects[0];
        assert_eq!(
            map.hit_formula(
                0,
                rect.page,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0
            )
            .unwrap(),
            Some(pos.clone())
        );
        let before = map.caret(0, &pos).unwrap()[0];
        let after = map.caret(0, &at(vec![index], 0, 1)).unwrap()[0];
        assert!(after.x > before.x);
        assert_eq!(map.selection(0, &pos, 1).unwrap(), rects);
        assert_eq!(map.formula_rects(1, &pos), Err(EditError::StaleRevision));
    }
    assert_eq!(
        map.hit_formula(0, 0, f64::NAN, 0.0),
        Err(EditError::InvalidRange)
    );
    assert_eq!(
        editor.document().body,
        original,
        "attribution leaves the document intact"
    );
}

#[test]
fn display_formulas_have_hit_boxes_without_changing_the_model_or_placement() {
    let body = concat!("<w:p xmlns:m=\"http://purl.oclc.org/ooxml/officeDocument/math\"><m:oMathPara>",
        "<m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>",
        "<m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>",
        "<m:oMath><m:r><m:t>+c</m:t></m:r></m:oMath></m:oMathPara></w:p>");
    let bytes = DocxBuilder::strict().body(body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
    let before = document.clone();
    let options = RenderOptions::default().caret_anchors(true);
    let placed = place_pages(&document, &options, Some(&package)).unwrap();
    let painted = strict_ooxml_render_svg::paint_page(&placed[0], &options);
    let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
    let map = editor
        .visual_map_placed(0, &options, Some(&package), &placed)
        .unwrap();
    let pos = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 0,
    };
    let rects = map.formula_rects(0, &pos).unwrap();
    assert_eq!(rects.len(), 1);
    let rect = rects[0];
    assert!(rect.width > 15.0 && rect.height > 15.0, "{rect:?}");
    assert_eq!(
        map.hit_formula(
            0,
            rect.page,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0
        )
        .unwrap(),
        Some(pos.clone())
    );
    assert!(!map.caret(0, &pos).unwrap().is_empty());
    assert!(!map
        .caret(0, &TextPosition { offset: 1, ..pos })
        .unwrap()
        .is_empty());
    assert_eq!(editor.document().body, before.body);
    let again = place_pages(editor.document(), &options, Some(&package)).unwrap();
    assert_eq!(
        strict_ooxml_render_svg::paint_page(&again[0], &options),
        painted
    );
}

#[test]
fn every_chart_kind_has_one_full_frame_and_carets_even_without_data() {
    use std::sync::Arc;
    use strict_ooxml_edit::{chart_drawing, DrawingPosition};
    use strict_ooxml_wml::model::chart::*;
    use strict_ooxml_wml::model::{Emu, Inline};
    for kind in [
        ChartKind::Bar {
            horizontal: false,
            grouping: BarGrouping::Clustered,
        },
        ChartKind::Bar {
            horizontal: true,
            grouping: BarGrouping::Stacked,
        },
        ChartKind::Bar {
            horizontal: false,
            grouping: BarGrouping::PercentStacked,
        },
        ChartKind::Line,
        ChartKind::Area,
        ChartKind::Scatter,
        ChartKind::Pie { doughnut: false },
        ChartKind::Pie { doughnut: true },
    ] {
        for values in [
            vec![Some(1.0), Some(2.0)],
            vec![Some(0.0), Some(0.0)],
            vec![None, None],
            vec![],
        ] {
            let chart = ChartData {
                kind: kind.clone(),
                title: Some("Title".into()),
                categories: vec!["A".into(), "B".into()],
                series: vec![ChartSeries {
                    name: Some("Series".into()),
                    values,
                    ..Default::default()
                }],
                legend: true,
                legend_position: LegendPosition::Bottom,
                unsupported: vec![],
            };
            let bytes = DocxBuilder::strict().body("<w:p/>").build();
            let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
            let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
            let strict_ooxml_wml::model::Block::Paragraph(paragraph) = &mut document.body.blocks[0]
            else {
                panic!("paragraph")
            };
            paragraph.inlines = vec![Inline::Drawing(chart_drawing(
                "rIdChart",
                Arc::new(chart),
                Emu(2_857_500),
                Emu(1_714_500),
                1,
                "Chart",
            ))];
            let before = document.body.clone();
            let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
            let options = RenderOptions::default();
            let map = editor.visual_map(0, &options, Some(&package)).unwrap();
            let position = DrawingPosition {
                paragraph: Address::body(0),
                inline: vec![0],
                content: None,
            };
            let rects = map.drawing_rects(0, &position).unwrap();
            assert_eq!(rects.len(), 1, "{kind:?}");
            let r = rects[0];
            assert!(
                (r.width - 300.0).abs() < 1e-6 && (r.height - 180.0).abs() < 1e-6,
                "{kind:?}: {r:?}"
            );
            for (x, y) in [
                (r.x + 1.0, r.y + 1.0),
                (r.x + r.width - 1.0, r.y + r.height - 1.0),
            ] {
                assert_eq!(
                    map.hit_drawing(0, r.page, x, y).unwrap(),
                    Some(position.clone()),
                    "{kind:?}"
                );
            }
            let start = at(vec![0], 0, 0);
            let end = at(vec![0], 0, 1);
            assert!(
                (map.caret(0, &end).unwrap()[0].x - map.caret(0, &start).unwrap()[0].x - r.width)
                    .abs()
                    < 1e-6
            );
            assert_eq!(
                map.hit_test(0, r.page, r.x + r.width + 10.0, r.y + r.height + 10.0)
                    .unwrap(),
                Some(end)
            );
            assert_eq!(editor.document().body, before);
        }
    }
}

/// Gradient, pattern, picture and translucent fills used to be replaced with
/// a flat solid. A picture fill then lost its image and a translucent solid
/// lost its group, and the whole caret map was refused. Recolouring in place
/// keeps every caret, including the text around the shapes.
#[test]
fn shape_fills_keep_the_caret_map() {
    use strict_ooxml_edit::DrawingPosition;
    let shape = |fill: &str| {
        format!(
            "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
             <wp:extent cx=\"457200\" cy=\"228600\"/>\
             <wp:docPr id=\"1\" name=\"s\"/>\
             <a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
             <wps:wsp><wps:cNvPr id=\"1\" name=\"s\"/><wps:spPr>\
             <a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"457200\" cy=\"228600\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"/>{fill}</wps:spPr></wps:wsp>\
             </a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        )
    };
    let body = format!(
        "<w:p><w:r><w:t>Keep</w:t></w:r></w:p>{}{}{}{}{}",
        shape(
            "<a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FF0000\"/></a:gs>\
             <a:gs pos=\"100000\"><a:srgbClr val=\"0000FF\"/></a:gs></a:gsLst><a:lin ang=\"0\"/></a:gradFill>"
        ),
        shape(
            "<a:pattFill prst=\"pct20\"><a:fgClr><a:srgbClr val=\"112233\"/></a:fgClr>\
             <a:bgClr><a:srgbClr val=\"FFFFFF\"/></a:bgClr></a:pattFill>"
        ),
        shape("<a:blipFill><a:blip r:embed=\"rIdMissing\"/></a:blipFill>"),
        shape(
            "<a:solidFill><a:srgbClr val=\"00AA00\"><a:alpha val=\"40000\"/></a:srgbClr></a:solidFill>"
        ),
        shape(
            "<a:noFill/><a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill></a:ln>"
        ),
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
    let before = document.body.clone();
    let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    let text = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 0,
    };
    let rects = map.caret(0, &text).unwrap();
    assert_eq!(rects.len(), 1);
    assert_eq!(
        map.hit_test(
            0,
            rects[0].page,
            rects[0].x,
            rects[0].y + rects[0].height / 2.0
        )
        .unwrap(),
        Some(text)
    );
    for block in 1..=5 {
        let position = DrawingPosition {
            paragraph: Address::body(block),
            inline: vec![0],
            content: Some(0),
        };
        assert!(
            !map.drawing_rects(0, &position).unwrap().is_empty(),
            "shape {block}"
        );
    }
    assert_eq!(editor.document().body, before);
}

/// A tab between runs is laid out from the paragraph mark. The copy's run
/// has to wear that mark, or a larger mark size moves the anchor and the
/// map is refused.
#[test]
fn a_tab_between_runs_keeps_the_mark_size() {
    use strict_ooxml_wml::model::{Block, Inline};
    let bytes = DocxBuilder::strict()
        .body(
            "<w:p><w:pPr><w:rPr><w:sz w:val=\"40\"/><w:szCs w:val=\"40\"/></w:rPr></w:pPr>\
             <w:r><w:t>A</w:t></w:r><w:r><w:t>B</w:t></w:r></w:p>",
        )
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&package, &ParseOptions::default()).unwrap();
    let Block::Paragraph(paragraph) = &mut document.body.blocks[0] else {
        panic!("paragraph");
    };
    paragraph.inlines.insert(1, Inline::Tab);
    let editor = Editor::new(&mut document, EditLimits::default()).unwrap();
    let map = editor
        .visual_map(0, &RenderOptions::default(), Some(&package))
        .unwrap();
    let before = TextPosition {
        paragraph: Address::body(0),
        inline: vec![0],
        content: 0,
        offset: 1,
    };
    let after = TextPosition {
        paragraph: Address::body(0),
        inline: vec![2],
        content: 0,
        offset: 0,
    };
    assert_eq!(map.caret(0, &before).unwrap().len(), 1);
    assert_eq!(map.caret(0, &after).unwrap().len(), 1);
}
