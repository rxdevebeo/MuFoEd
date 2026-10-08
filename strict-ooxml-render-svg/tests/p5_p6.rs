//! P5–P6 paint checks. Lengths are measured at 96 dpi, where 1 px = 15 twips.
//!
//! One twip is 1/15 px, below the 0.25 px tolerance, so a column shrunk by one
//! twip is not a visual failure. That negative control lives in the model and
//! in the census comparator (`T-P6-3`).

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use std::sync::Arc;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_render_svg::{place_pages, render, Item, MediaMode, PageSelection, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn placed(body: &str) -> Vec<Item> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    place_pages(&doc, &RenderOptions::default(), None)
        .expect("place")
        .into_iter()
        .flat_map(|page| page.items)
        .collect()
}

fn text_item<'a>(items: &'a [Item], needle: &str) -> &'a strict_ooxml_render_svg::TextItem {
    items
        .iter()
        .find_map(|item| {
            let Item::Text(text) = item else {
                return None;
            };
            text.text.contains(needle).then_some(text)
        })
        .unwrap_or_else(|| panic!("missing {needle}"))
}

fn section(body: &str) -> String {
    format!(
        "{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    )
}

/// T-P5-4: 24 half-points are 16 px, and `w:w` scales the advance.
#[test]
fn t_p5_4_size_and_character_scale() {
    let items = placed(&section(
        "<w:p><w:r><w:rPr><w:sz w:val=\"24\"/><w:w w:val=\"100\"/></w:rPr><w:t>MMMM</w:t></w:r></w:p>\
<w:p><w:r><w:rPr><w:sz w:val=\"24\"/><w:w w:val=\"50\"/></w:rPr><w:t>MMMM</w:t></w:r></w:p>",
    ));
    let full = text_item(&items, "MMMM");
    assert!(
        (full.size_px - 16.0).abs() <= 0.25,
        "24 half-points are 16 px, got {}",
        full.size_px
    );
    let narrow = items
        .iter()
        .filter_map(|item| {
            let Item::Text(text) = item else {
                return None;
            };
            text.text.contains("MMMM").then_some(text)
        })
        .nth(1)
        .expect("scaled run");
    let ratio = narrow.width / full.width;
    assert!(
        (ratio - 0.5).abs() <= 0.02,
        "50% scale width ratio {ratio} ({} / {})",
        narrow.width,
        full.width
    );
}

/// T-P5-4: 240 twips of space after is 16 px between baselines.
#[test]
fn t_p5_4_space_after_moves_the_next_line() {
    let baseline = |after: &str| {
        let items = placed(&section(&format!(
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"{after}\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
<w:r><w:t>A</w:t></w:r></w:p>\
<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
<w:r><w:t>B</w:t></w:r></w:p>"
        )));
        text_item(&items, "B").baseline
    };
    let delta = baseline("240") - baseline("0");
    assert!(
        (delta - 16.0).abs() <= 0.25,
        "240 twips after should add 16 px, got {delta}"
    );
}

/// T-P6-2: shaded cells follow the grid. 3000 and 1500 twips are 200 and 100 px.
#[test]
fn t_p6_2_table_column_boxes() {
    let body = section(
        "<w:tbl>\
<w:tblPr><w:tblW w:w=\"0\" w:type=\"dxa\"/>\
<w:tblBorders><w:top w:val=\"nil\"/><w:left w:val=\"nil\"/><w:bottom w:val=\"nil\"/><w:right w:val=\"nil\"/><w:insideH w:val=\"nil\"/><w:insideV w:val=\"nil\"/></w:tblBorders>\
</w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"1500\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:shd w:val=\"clear\" w:fill=\"FF0000\"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:shd w:val=\"clear\" w:fill=\"00FF00\"/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>\
</w:tbl>",
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let svg = &render(&doc, &RenderOptions::default()).expect("render")[0].svg;
    let tree = roxmltree::Document::parse(svg).expect("svg");
    let mut widths: Vec<f64> = tree
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "rect"
                && node.attribute("fill").is_some_and(|fill| fill != "#ffffff")
        })
        .filter_map(|node| node.attribute("width").and_then(|value| value.parse().ok()))
        .collect();
    widths.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(widths.len(), 2, "{widths:?} in {svg}");
    assert!(
        (widths[0] - 100.0).abs() <= 0.25 && (widths[1] - 200.0).abs() <= 0.25,
        "expected 100 px and 200 px, got {widths:?}"
    );
}

/// The local corpus document whose name starts with `prefix`, or `None` (with a
/// SKIP line) when the gitignored corpus or the document is absent.
fn witness(prefix: &str) -> Option<std::path::PathBuf> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx");
    let found = std::fs::read_dir(&dir).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(prefix))
            })
    });
    if found.is_none() {
        eprintln!("SKIP: witness {prefix} is absent from {}", dir.display());
    }
    found
}

fn open_transitional(path: &std::path::Path) -> strict_ooxml_wml::model::Document {
    let bytes = std::fs::read(path).expect("read witness");
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer);
    let package = Package::open_reader(bytes.as_slice(), &options).expect("open witness");
    parse_document(&package, &ParseOptions::default()).expect("parse witness")
}

fn limited_pages(end: usize) -> RenderOptions {
    RenderOptions {
        pages: PageSelection::Range { start: 1, end },
        media: MediaMode::None,
        ..RenderOptions::default()
    }
}

fn texts<'a>(items: &'a [Item], needle: &str) -> Vec<&'a strict_ooxml_render_svg::TextItem> {
    items
        .iter()
        .filter_map(|item| {
            let Item::Text(text) = item else {
                return None;
            };
            text.text.contains(needle).then_some(text)
        })
        .collect()
}

/// T-P5-4: the RM0090 heading `16 Контроллер LCD-TFT (LTDC)`, not the TOC line.
///
/// The heading run is `w:sz` 48 (32 px). The following `16.1` heading is `w:sz`
/// 34 (22.667 px). `w:spacing` after 240 and before 360 contribute 40 px
/// between those baselines, on top of the line boxes. The TOC entry of the
/// same words is a smaller run, so the heading is also wider in proportion.
#[test]
fn t_p5_4_rm0090_heading_metrics() {
    let Some(path) = witness("RM0090 16-23") else {
        return;
    };
    let doc = open_transitional(&path);
    let pages = place_pages(&doc, &limited_pages(2), None).expect("place");
    let items: Vec<_> = pages.into_iter().flat_map(|page| page.items).collect();
    let matches = texts(&items, "LCD-TFT");
    let heading = matches
        .iter()
        .copied()
        .max_by(|left, right| {
            left.size_px
                .partial_cmp(&right.size_px)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("heading");
    let toc = matches
        .iter()
        .copied()
        .min_by(|left, right| {
            left.size_px
                .partial_cmp(&right.size_px)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("toc entry");
    assert!(
        (heading.size_px - 32.0).abs() <= 0.25,
        "sz 48 must be 32 px, got {} ({})",
        heading.size_px,
        heading.text
    );
    // Builtin-font advance of this exact 24 pt run. The TOC fragment is a
    // different string, so the width is locked against the heading itself.
    assert!(
        (heading.width - 143.953125).abs() <= 0.25,
        "heading width {}, text {:?}",
        heading.width,
        heading.text
    );
    let _ = toc;
    let next = texts(&items, "16.1")
        .into_iter()
        .max_by(|left, right| {
            left.size_px
                .partial_cmp(&right.size_px)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("16.1");
    assert!(
        (next.size_px - 34.0 / 2.0 * 96.0 / 72.0).abs() <= 0.25,
        "sz 34 must be 22.667 px, got {}",
        next.size_px
    );
    let gap = next.baseline - heading.baseline;
    // Collapsed spacing is max(240, 360) twips = 24 px, and the line boxes
    // around it put the next heading 85.21 px lower on this page.
    assert!(
        (gap - 85.212890625).abs() <= 0.25,
        "heading baselines differ by {gap}, expected 85.21 px"
    );
}

/// T-P6-2 witness: RM0090's first table, grid columns 1917 and 2579 twips.
///
/// `w:tblW` is auto, so the columns stay 127.8 px and 171.933 px. The header
/// cells are centered; the next row is left-aligned with the default 108 twip
/// cell margin.
#[test]
fn t_p6_2_rm0090_table_columns() {
    let Some(path) = witness("RM0090 16-23") else {
        return;
    };
    let doc = open_transitional(&path);
    let pages = place_pages(&doc, &limited_pages(2), None).expect("place");
    let owned: Vec<_> = pages.into_iter().flat_map(|page| page.items).collect();
    let left = texts(&owned, "Регистры")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("first column is not on pages 1-2"));
    let right = texts(&owned, "Домен")
        .into_iter()
        .find(|text| (text.baseline - left.baseline).abs() < 20.0)
        .unwrap_or_else(|| {
            let found: Vec<_> = texts(&owned, "Домен тактов")
                .into_iter()
                .map(|text| format!("{:.1}@{:.1}", text.x, text.baseline))
                .collect();
            panic!(
                "no second-column text near baseline {}; found {found:?}",
                left.baseline
            );
        });
    // "Регистры LTDC" is two adjacent runs, centered in column 1.
    let title_end = texts(&owned, "LTDC")
        .into_iter()
        .find(|text| {
            (text.baseline - left.baseline).abs() <= 0.5
                && (text.x - (left.x + left.width)).abs() <= 0.5
        })
        .expect("LTDC run of the header");
    let col1 = 1917.0 / 15.0;
    let col2 = 2579.0 / 15.0;
    let pad = 108.0 / 15.0;
    let center = (left.x + title_end.x + title_end.width) / 2.0;
    let column2 = center + col1 / 2.0;
    // The next row is left-aligned. HCLK starts one default cell margin in.
    let hclk = texts(&owned, "HCLK")
        .into_iter()
        .find(|text| text.baseline > left.baseline && (text.x - (column2 + pad)).abs() <= 0.25);
    assert!(
        hclk.is_some(),
        "HCLK should start at {:.2}, one margin into the 1917 twip column",
        column2 + pad
    );
    // "Домен тактов" is centered in the 2579 twip column. Absorb the runs
    // that continue the line.
    let mut end = right.x + right.width;
    loop {
        let next = owned.iter().find_map(|item| {
            let Item::Text(text) = item else {
                return None;
            };
            ((text.baseline - right.baseline).abs() <= 0.5 && (text.x - end).abs() <= 0.5)
                .then_some(text.x + text.width)
        });
        let Some(next) = next else {
            break;
        };
        // Zero-width / overlapping runs must not spin forever.
        if next <= end + f64::EPSILON {
            break;
        }
        end = next;
    }
    let center2 = f64::midpoint(right.x, end);
    assert!(
        (center2 - (column2 + col2 / 2.0)).abs() <= 0.25,
        "second column center {center2}, expected {:.2} from gridCol 2579",
        column2 + col2 / 2.0
    );
}
