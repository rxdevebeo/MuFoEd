#![allow(
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
//! Charts are drawn from their cached data instead of a placeholder.

use std::io::Cursor;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{render_with_media, Page, RenderOptions};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

const C_NS: &str = "http://purl.oclc.org/ooxml/drawingml/chart";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

fn chart_paragraph(rel: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"4572000\" cy=\"2743200\"/><wp:docPr id=\"1\" name=\"Chart 1\"/>\
<a:graphic><a:graphicData uri=\"{C_NS}\"><c:chart xmlns:c=\"{C_NS}\" r:id=\"{rel}\"/>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )
}

fn chart_space(chart: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<c:chartSpace xmlns:c=\"{C_NS}\" xmlns:a=\"{A_NS}\"><c:chart>{chart}</c:chart></c:chartSpace>"
    )
}

fn str_cache(points: &[&str]) -> String {
    let mut xml = format!(
        "<c:strRef><c:strCache><c:ptCount val=\"{}\"/>",
        points.len()
    );
    for (index, point) in points.iter().enumerate() {
        xml.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{point}</c:v></c:pt>"));
    }
    xml.push_str("</c:strCache></c:strRef>");
    xml
}

fn num_cache(count: usize, points: &[(usize, f64)]) -> String {
    let mut xml = format!("<c:numRef><c:numCache><c:ptCount val=\"{count}\"/>");
    for (index, value) in points {
        xml.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{value}</c:v></c:pt>"));
    }
    xml.push_str("</c:numCache></c:numRef>");
    xml
}

/// Renders a document holding one chart whose `c:chart` content is `chart`.
fn render_chart(chart: &str) -> Vec<Page> {
    let bytes = DocxBuilder::strict()
        .body(&chart_paragraph("rIdChart"))
        .rel("rIdChart", "chart", "charts/chart1.xml")
        .content_type("/word/charts/chart1.xml", CHART_CONTENT_TYPE)
        .part("word/charts/chart1.xml", chart_space(chart).into_bytes())
        .build();
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render")
}

/// `(x, y, width, height)` of every `<rect>` filled with `fill`.
fn rects(svg: &str, fill: &str) -> Vec<(f64, f64, f64, f64)> {
    let tree = roxmltree::Document::parse(svg).expect("svg");
    let number = |node: roxmltree::Node<'_, '_>, name: &str| -> f64 {
        node.attribute(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(f64::NAN)
    };
    tree.descendants()
        .filter(|node| node.has_tag_name("rect") && node.attribute("fill") == Some(fill))
        .map(|node| {
            (
                number(node, "x"),
                number(node, "y"),
                number(node, "width"),
                number(node, "height"),
            )
        })
        .collect()
}

fn texts(svg: &str) -> Vec<String> {
    let tree = roxmltree::Document::parse(svg).expect("svg");
    tree.descendants()
        .filter(|node| node.has_tag_name("text"))
        .filter_map(|node| node.text().map(str::to_owned))
        .collect()
}

fn paths(svg: &str, fill: &str) -> Vec<String> {
    let tree = roxmltree::Document::parse(svg).expect("svg");
    tree.descendants()
        .filter(|node| node.has_tag_name("path") && node.attribute("fill") == Some(fill))
        .filter_map(|node| node.attribute("d").map(str::to_owned))
        .collect()
}

fn column_chart() -> String {
    let categories = str_cache(&["A", "B", "C"]);
    format!(
        "<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title>\
<c:autoTitleDeleted val=\"0\"/><c:plotArea><c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>\
<c:ser><c:idx val=\"0\"/><c:tx>{}</c:tx><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr>\
<c:cat>{categories}</c:cat><c:val>{}</c:val></c:ser>\
<c:ser><c:idx val=\"1\"/><c:tx>{}</c:tx><c:cat>{categories}</c:cat><c:val>{}</c:val></c:ser>\
</c:barChart></c:plotArea><c:legend><c:legendPos val=\"r\"/></c:legend>",
        str_cache(&["North"]),
        num_cache(3, &[(0, 10.0), (1, 20.0), (2, 30.0)]),
        str_cache(&["South"]),
        num_cache(3, &[(0, 5.0), (2, 15.0)]),
    )
}

/// The bars of one colour, without the legend swatch (the rightmost rect).
fn bars(svg: &str, fill: &str) -> Vec<(f64, f64, f64, f64)> {
    let mut found = rects(svg, fill);
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.pop();
    found
}

#[test]
fn a_column_chart_draws_bars_in_proportion() {
    let pages = render_chart(&column_chart());
    let svg = &pages[0].svg;
    assert!(rects(svg, "#f2f2f2").is_empty(), "no placeholder");
    let north = bars(svg, "#ff0000");
    assert_eq!(north.len(), 3, "{north:?}");
    let south = bars(svg, "#ed7d31");
    assert_eq!(south.len(), 2, "the missing point has no bar: {south:?}");
    let unit = north[0].3;
    assert!(unit > 1.0);
    assert!((north[1].3 / unit - 2.0).abs() < 0.01, "{north:?}");
    assert!((north[2].3 / unit - 3.0).abs() < 0.01, "{north:?}");
    assert!((south[0].3 / unit - 0.5).abs() < 0.01, "{south:?}");
    assert!((south[1].3 / unit - 1.5).abs() < 0.01, "{south:?}");
    // Bars stand on one baseline and keep to the drawing's 480 x 288 box.
    let base = north[0].1 + north[0].3;
    for bar in north.iter().chain(&south) {
        assert!((bar.1 + bar.3 - base).abs() < 0.01, "{bar:?}");
    }
    // Clustered: within a category the second series sits right of the first.
    assert!(south[0].0 > north[0].0 && south[0].0 < north[1].0);
    let labels = texts(svg);
    for expected in ["Sales", "North", "South", "A", "B", "C", "0", "30"] {
        assert!(
            labels.iter().any(|text| text == expected),
            "{expected}: {labels:?}"
        );
    }
}

#[test]
fn a_pie_chart_draws_sectors_as_paths() {
    let chart = format!(
        "<c:plotArea><c:pieChart><c:varyColors val=\"1\"/><c:ser><c:idx val=\"0\"/>\
<c:tx><c:v>Share</c:v></c:tx><c:cat>{}</c:cat><c:val>{}</c:val></c:ser></c:pieChart></c:plotArea>\
<c:legend><c:legendPos val=\"b\"/></c:legend>",
        str_cache(&["X", "Y"]),
        num_cache(2, &[(0, 1.0), (1, 3.0)]),
    );
    let pages = render_chart(&chart);
    let svg = &pages[0].svg;
    let first = paths(svg, "#4472c4");
    let second = paths(svg, "#ed7d31");
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(second.len(), 1, "{second:?}");
    for d in first.iter().chain(&second) {
        assert!(d.contains('C'), "arcs are cubic Béziers: {d}");
        assert!(!d.contains('A'), "no elliptical arcs: {d}");
    }
    let labels = texts(svg);
    assert!(labels.iter().any(|text| text == "X"));
    assert!(labels.iter().any(|text| text == "Y"));
}

#[test]
fn line_area_and_scatter_charts_draw() {
    let series = format!(
        "<c:ser><c:idx val=\"0\"/><c:xVal>{}</c:xVal><c:yVal>{}</c:yVal><c:val>{}</c:val></c:ser>",
        num_cache(3, &[(0, 1.0), (1, 2.0), (2, 4.0)]),
        num_cache(3, &[(0, 3.0), (1, -1.0), (2, 2.0)]),
        num_cache(3, &[(0, 3.0), (1, -1.0), (2, 2.0)]),
    );
    for kind in ["lineChart", "areaChart", "scatterChart"] {
        let chart = format!("<c:plotArea><c:{kind}>{series}</c:{kind}></c:plotArea>");
        let pages = render_chart(&chart);
        let svg = &pages[0].svg;
        assert!(rects(svg, "#f2f2f2").is_empty(), "{kind}: no placeholder");
        let tree = roxmltree::Document::parse(svg).expect("svg");
        let drawn = tree
            .descendants()
            .filter(|node| {
                node.has_tag_name("path")
                    && (node.attribute("fill") == Some("#4472c4")
                        || node.attribute("stroke") == Some("#4472c4"))
            })
            .count();
        assert!(drawn >= 1, "{kind}: nothing drawn");
    }
}

#[test]
fn a_huge_chart_is_capped_without_panicking() {
    let mut series = String::new();
    let points: Vec<(usize, f64)> = (0..100).map(|index| (index, index as f64)).collect();
    for index in 0..300 {
        series.push_str(&format!(
            "<c:ser><c:idx val=\"{index}\"/><c:val>{}</c:val></c:ser>",
            num_cache(1_000_000, &points)
        ));
    }
    let chart = format!(
        "<c:plotArea><c:barChart><c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>{series}</c:barChart></c:plotArea>"
    );
    let pages = render_chart(&chart);
    let svg = &pages[0].svg;
    let tree = roxmltree::Document::parse(svg).expect("svg");
    let marks = tree
        .descendants()
        .filter(|node| node.has_tag_name("rect") || node.has_tag_name("path"))
        .count();
    assert!(marks <= 20_100, "{marks} marks");
    assert!(
        pages[0]
            .warnings
            .iter()
            .any(|warning| warning.starts_with("render.chart-capped")),
        "{:?}",
        pages[0].warnings
    );
}

#[test]
fn hostile_numbers_stay_finite() {
    let chart = format!(
        "<c:plotArea><c:barChart><c:grouping val=\"percentStacked\"/><c:ser><c:idx val=\"0\"/><c:val>{}</c:val></c:ser>\
<c:ser><c:idx val=\"1\"/><c:val>{}</c:val></c:ser></c:barChart></c:plotArea>",
        num_cache(3, &[(0, 1e308), (1, -1e308), (2, 0.0)]),
        num_cache(3, &[(0, f64::MIN_POSITIVE), (2, 0.0)]),
    );
    let pages = render_chart(&chart);
    let tree = roxmltree::Document::parse(&pages[0].svg).expect("svg");
    for node in tree.descendants().filter(|node| node.has_tag_name("rect")) {
        for name in ["x", "y", "width", "height"] {
            let value: f64 = node.attribute(name).expect(name).parse().expect("number");
            assert!(value.is_finite(), "{name}={value}");
        }
    }
    for node in tree.descendants().filter(|node| node.has_tag_name("path")) {
        let d = node.attribute("d").expect("d");
        assert!(!d.contains("NaN") && !d.contains("inf"), "{d}");
    }
}
