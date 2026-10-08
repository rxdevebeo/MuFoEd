#![allow(
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::float_cmp
)]
//! A `c:chart r:id` is followed to its chart part, and the part's cached
//! values become the drawing's [`ChartData`].

use std::io::Cursor;
use std::sync::Arc;

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::chart::{
    BarGrouping, ChartData, ChartKind, LegendPosition, MAX_CHART_POINTS, MAX_CHART_SERIES,
};
use strict_ooxml_wml::model::{
    Document, DrawingKind, ForeignRefs, Graphic, Inline, RunContent, SupportStatus,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

const C_NS: &str = "http://purl.oclc.org/ooxml/drawingml/chart";
const C_TRANSITIONAL_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

/// A paragraph holding one inline chart (480 × 288 px at 96 DPI).
fn chart_paragraph(id: usize, rel: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"4572000\" cy=\"2743200\"/><wp:docPr id=\"{id}\" name=\"Chart {id}\"/>\
<a:graphic><a:graphicData uri=\"{C_NS}\"><c:chart xmlns:c=\"{C_NS}\" r:id=\"{rel}\"/>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )
}

/// A chart part around `chart` (the content of `c:chart`), in namespace `ns`.
fn chart_space(ns: &str, chart: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<c:chartSpace xmlns:c=\"{ns}\" xmlns:a=\"{A_NS}\"><c:chart>{chart}</c:chart></c:chartSpace>"
    )
}

fn str_cache(points: &[&str]) -> String {
    let mut xml = format!(
        "<c:strRef><c:f>S!A1</c:f><c:strCache><c:ptCount val=\"{}\"/>",
        points.len()
    );
    for (index, point) in points.iter().enumerate() {
        xml.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{point}</c:v></c:pt>"));
    }
    xml.push_str("</c:strCache></c:strRef>");
    xml
}

/// A number cache of `count` points with `(idx, value)` entries.
fn num_cache(count: usize, points: &[(usize, &str)]) -> String {
    let mut xml = format!(
        "<c:numRef><c:f>S!B1</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{count}\"/>"
    );
    for (index, value) in points {
        xml.push_str(&format!("<c:pt idx=\"{index}\"><c:v>{value}</c:v></c:pt>"));
    }
    xml.push_str("</c:numCache></c:numRef>");
    xml
}

/// The clustered column chart: two series over three categories, the second
/// missing its middle point; the first has an explicit colour.
fn column_chart() -> String {
    let categories = str_cache(&["A", "B", "C"]);
    let plot = format!(
        "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>\
<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx>{}</c:tx>\
<c:spPr><a:solidFill><a:srgbClr val=\"ff0000\"/></a:solidFill></c:spPr>\
<c:cat>{categories}</c:cat><c:val>{}</c:val></c:ser>\
<c:ser><c:idx val=\"1\"/><c:order val=\"1\"/><c:tx>{}</c:tx>\
<c:cat>{categories}</c:cat><c:val>{}</c:val></c:ser>\
<c:gapWidth val=\"150\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
<c:catAx><c:axId val=\"1\"/></c:catAx><c:valAx><c:axId val=\"2\"/></c:valAx>",
        str_cache(&["North"]),
        num_cache(3, &[(0, "10"), (1, "20"), (2, "30")]),
        str_cache(&["South"]),
        num_cache(3, &[(0, "5"), (2, "15")]),
    );
    format!(
        "<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Sal</a:t></a:r><a:r><a:t>es</a:t></a:r></a:p></c:rich></c:tx></c:title>\
<c:autoTitleDeleted val=\"0\"/><c:plotArea><c:layout/>{plot}</c:plotArea>\
<c:legend><c:legendPos val=\"r\"/></c:legend><c:plotVisOnly val=\"1\"/>"
    )
}

/// A pie chart with a literal series name and no legend or title.
fn pie_chart() -> String {
    format!(
        "<c:autoTitleDeleted val=\"1\"/><c:plotArea><c:pieChart><c:varyColors val=\"1\"/>\
<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx><c:v>Share</c:v></c:tx>\
<c:cat>{}</c:cat><c:val>{}</c:val></c:ser><c:firstSliceAng val=\"0\"/></c:pieChart></c:plotArea>",
        str_cache(&["X", "Y"]),
        num_cache(2, &[(0, "1"), (1, "3")]),
    )
}

/// A package with one inline chart per `(rel id, part name, part bytes)`.
fn package(charts: &[(&str, &str, Vec<u8>)]) -> Package {
    let body: String = charts
        .iter()
        .enumerate()
        .map(|(index, (rel, _, _))| chart_paragraph(index + 1, rel))
        .collect();
    let mut builder = DocxBuilder::strict().body(&body);
    for (rel, name, bytes) in charts {
        builder = builder
            .rel(rel, "chart", &format!("charts/{name}"))
            .content_type(&format!("/word/charts/{name}"), CHART_CONTENT_TYPE)
            .part(&format!("word/charts/{name}"), bytes.clone());
    }
    Package::open_reader(Cursor::new(builder.build()), &OpenOptions::default()).expect("open")
}

fn parse(charts: &[(&str, &str, Vec<u8>)]) -> Document {
    parse_document(&package(charts), &ParseOptions::default()).expect("parse")
}

/// Every chart reference in the body, in order.
fn chart_refs(document: &Document) -> Vec<ForeignRefs> {
    let mut refs = Vec::new();
    for block in &document.body.blocks {
        let Some(paragraph) = block.as_paragraph() else {
            continue;
        };
        for inline in &paragraph.inlines {
            let Inline::Run(run) = inline else { continue };
            for content in &run.content {
                let RunContent::Drawing(drawing) = content else {
                    continue;
                };
                let DrawingKind::Inline(inline) = &drawing.kind else {
                    continue;
                };
                if let Graphic::Chart(found) = inline.graphic.as_ref() {
                    refs.push(found.clone());
                }
            }
        }
    }
    refs
}

fn only_chart(document: &Document) -> Arc<ChartData> {
    let refs = chart_refs(document);
    assert_eq!(refs.len(), 1);
    refs[0].chart.clone().expect("chart data")
}

#[test]
fn a_clustered_column_chart_is_read_from_its_cache() {
    let part = chart_space(C_NS, &column_chart());
    let document = parse(&[("rIdChart1", "chart1.xml", part.into_bytes())]);
    let chart = only_chart(&document);
    assert_eq!(
        chart.kind,
        ChartKind::Bar {
            horizontal: false,
            grouping: BarGrouping::Clustered
        }
    );
    assert_eq!(chart.title.as_deref(), Some("Sales"));
    assert_eq!(chart.categories, vec!["A", "B", "C"]);
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[0].name.as_deref(), Some("North"));
    assert_eq!(
        chart.series[0].values,
        vec![Some(10.0), Some(20.0), Some(30.0)]
    );
    assert_eq!(chart.series[0].color.as_deref(), Some("FF0000"));
    assert_eq!(chart.series[1].name.as_deref(), Some("South"));
    assert_eq!(chart.series[1].values, vec![Some(5.0), None, Some(15.0)]);
    assert_eq!(chart.series[1].color, None);
    assert!(chart.legend);
    assert_eq!(chart.legend_position, LegendPosition::Right);
    assert!(chart.unsupported.is_empty(), "{:?}", chart.unsupported);
    assert_eq!(chart.category_count(), 3);
    assert_eq!(
        document.support.get("c:chartSpace").map(|use_| use_.status),
        Some(SupportStatus::Supported)
    );
}

#[test]
fn a_pie_chart_is_read_and_a_deleted_title_stays_deleted() {
    let part = chart_space(C_NS, &pie_chart());
    let document = parse(&[("rIdPie", "chart2.xml", part.into_bytes())]);
    let chart = only_chart(&document);
    assert_eq!(chart.kind, ChartKind::Pie { doughnut: false });
    assert_eq!(chart.title, None);
    assert!(!chart.legend);
    assert_eq!(chart.categories, vec!["X", "Y"]);
    assert_eq!(chart.series[0].name.as_deref(), Some("Share"));
    assert_eq!(chart.series[0].values, vec![Some(1.0), Some(3.0)]);
}

#[test]
fn a_malformed_chart_part_leaves_no_data_and_a_record() {
    let broken = format!("<c:chartSpace xmlns:c=\"{C_NS}\"><c:chart><c:plotArea>");
    let good = chart_space(C_NS, &pie_chart());
    let document = parse(&[
        ("rIdBad", "chart1.xml", broken.into_bytes()),
        ("rIdGood", "chart2.xml", good.into_bytes()),
    ]);
    let refs = chart_refs(&document);
    assert_eq!(
        refs.len(),
        2,
        "the document still parses, with both drawings"
    );
    assert_eq!(refs[0].ids(), vec!["rIdBad"]);
    assert!(refs[0].chart.is_none());
    assert!(
        refs[1].chart.is_some(),
        "one bad part does not spoil the next"
    );
    let record = document.support.get("c:chartSpace").expect("record");
    assert_eq!(record.status, SupportStatus::Partial);
}

#[test]
fn a_part_that_is_not_a_chart_leaves_no_data() {
    let not_chart = b"<?xml version=\"1.0\"?><root/>".to_vec();
    let document = parse(&[("rIdOdd", "chart1.xml", not_chart)]);
    assert!(chart_refs(&document)[0].chart.is_none());
    assert_eq!(
        document.support.get("c:chartSpace").map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
}

#[test]
fn an_unresolved_id_is_recorded_and_not_drawn() {
    let body = chart_paragraph(1, "rIdNowhere");
    let bytes = DocxBuilder::strict().body(&body).build();
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let refs = chart_refs(&document);
    assert_eq!(refs[0].ids(), vec!["rIdNowhere"]);
    assert!(refs[0].chart.is_none());
    assert_eq!(
        document.support.get("c:chart").map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
}

#[test]
fn one_part_referenced_twice_is_parsed_once() {
    let part = chart_space(C_NS, &pie_chart()).into_bytes();
    let body = format!(
        "{}{}",
        chart_paragraph(1, "rIdOne"),
        chart_paragraph(2, "rIdTwo")
    );
    let bytes = DocxBuilder::strict()
        .body(&body)
        .rel("rIdOne", "chart", "charts/chart1.xml")
        .rel("rIdTwo", "chart", "charts/chart1.xml")
        .content_type("/word/charts/chart1.xml", CHART_CONTENT_TYPE)
        .part("word/charts/chart1.xml", part)
        .build();
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let refs = chart_refs(&document);
    let first = refs[0].chart.as_ref().expect("first");
    let second = refs[1].chart.as_ref().expect("second");
    assert!(Arc::ptr_eq(first, second));
}

#[test]
fn a_transitional_chart_namespace_is_read_too() {
    let part = chart_space(C_TRANSITIONAL_NS, &pie_chart());
    let document = parse(&[("rIdT", "chart1.xml", part.into_bytes())]);
    assert_eq!(
        only_chart(&document).series[0].values,
        vec![Some(1.0), Some(3.0)]
    );
}

#[test]
fn a_horizontal_stacked_bar_and_a_combination_are_reported() {
    let chart = format!(
        "<c:plotArea><c:bar3DChart><c:barDir val=\"bar\"/><c:grouping val=\"percentStacked\"/>\
<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
<c:spPr><a:solidFill><a:schemeClr val=\"accent2\"/></a:solidFill></c:spPr>\
<c:val>{}</c:val></c:ser></c:bar3DChart><c:surfaceChart/></c:plotArea>",
        num_cache(2, &[(0, "1"), (1, "NaN")])
    );
    let part = chart_space(C_NS, &chart);
    let document = parse(&[("rIdH", "chart1.xml", part.into_bytes())]);
    let chart = only_chart(&document);
    assert_eq!(
        chart.kind,
        ChartKind::Bar {
            horizontal: true,
            grouping: BarGrouping::PercentStacked
        }
    );
    assert_eq!(
        chart.series[0].values,
        vec![Some(1.0), None],
        "NaN is a missing point"
    );
    assert_eq!(chart.series[0].color.as_deref(), Some("ED7D31"));
    assert!(chart.unsupported.contains(&"c:bar3DChart".to_owned()));
    assert!(chart.unsupported.contains(&"c:surfaceChart".to_owned()));
    assert_eq!(
        document
            .support
            .get("c:surfaceChart")
            .map(|use_| use_.status),
        Some(SupportStatus::Partial)
    );
}

#[test]
fn series_and_points_are_capped() {
    let mut series = String::new();
    for index in 0..(MAX_CHART_SERIES + 4) {
        series.push_str(&format!(
            "<c:ser><c:idx val=\"{index}\"/><c:order val=\"{index}\"/><c:val>{}</c:val></c:ser>",
            num_cache(
                usize::MAX,
                &[
                    (0, "1"),
                    (MAX_CHART_POINTS + 10, "2"),
                    (usize::MAX - 1, "3")
                ]
            )
        ));
    }
    let chart = format!("<c:plotArea><c:lineChart>{series}</c:lineChart></c:plotArea>");
    let part = chart_space(C_NS, &chart);
    let document = parse(&[("rIdBig", "chart1.xml", part.into_bytes())]);
    let chart = only_chart(&document);
    assert_eq!(chart.kind, ChartKind::Line);
    assert_eq!(chart.series.len(), MAX_CHART_SERIES);
    assert!(chart
        .series
        .iter()
        .all(|series| series.values == vec![Some(1.0)]));
    assert!(chart.unsupported.contains(&"limit.chart_series".to_owned()));
    assert!(chart.unsupported.contains(&"limit.chart_points".to_owned()));
}

#[test]
fn a_sparse_index_does_not_allocate_past_the_budget() {
    // One point at the last allowed index in each of many series: the budget is
    // sixteen slots per cached point plus one series' worth, so most series
    // come out truncated rather than 4096 slots each.
    let mut series = String::new();
    for index in 0..64 {
        series.push_str(&format!(
            "<c:ser><c:idx val=\"{index}\"/><c:order val=\"{index}\"/><c:val>{}</c:val></c:ser>",
            num_cache(MAX_CHART_POINTS, &[(MAX_CHART_POINTS - 1, "7")])
        ));
    }
    let chart = format!("<c:plotArea><c:barChart>{series}</c:barChart></c:plotArea>");
    let part = chart_space(C_NS, &chart);
    let document = parse(&[("rIdSparse", "chart1.xml", part.into_bytes())]);
    let chart = only_chart(&document);
    let slots: usize = chart.series.iter().map(|series| series.values.len()).sum();
    assert!(slots <= 64 * 16 + MAX_CHART_POINTS, "{slots} slots");
    assert!(chart.unsupported.contains(&"limit.chart_points".to_owned()));
}
