//! Charts an editing session adds: the chart part written from the model's
//! chart data, and the drawing that shows it.
//!
//! The model keeps a chart as a reference to a part it does not carry
//! ([`ForeignRefs`]), with the part's cached values beside it for drawing.
//! A new chart therefore needs that part: [`AddedMedia::add_chart`] writes
//! one from the [`ChartData`] (values as literals, no workbook) and keeps it
//! with a relationship from the document part, so [`crate::MediaLayer`]
//! serves both to the writer as if the package had always had them, and the
//! writer copies the part the way it copies any chart.

use std::fmt::Write as _;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::chart::{BarGrouping, ChartData, ChartKind, LegendPosition};
use strict_ooxml_wml::model::drawing::DocPr;
use strict_ooxml_wml::model::{
    Drawing, DrawingKind, Emu, Extent, ForeignRefs, Graphic, InlineDrawing,
};

use crate::media::AddedChart;
use crate::AddedMedia;

/// The content type of a chart part.
pub const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
/// The Strict relationship type of a chart.
pub const CHART_RELATIONSHIP: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/chart";
const NS_CHART: &str = "http://purl.oclc.org/ooxml/drawingml/chart";
const NS_MAIN: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const NS_RELS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

/// Office's default accents, for series without a colour of their own.
const ACCENTS: [&str; 6] = ["4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47"];

impl AddedMedia {
    /// Writes a chart part for `data` and keeps it with a relationship from
    /// the document part; returns the relationship id, for
    /// [`chart_drawing`].
    pub fn add_chart(&mut self, data: &ChartData) -> String {
        let number = self.charts.len() + 1;
        let id = format!("rIdAddedChart{number}");
        let part = PartId::new(format!("/word/charts/added{number}.xml"));
        self.charts.push(AddedChart {
            id: id.clone(),
            part,
            target: format!("charts/added{number}.xml"),
            bytes: Arc::from(chart_part(data).into_bytes()),
        });
        id
    }
}

/// An inline chart drawing for the relationship `rel_id`, drawn from
/// `data`, `width` by `height`.
#[must_use]
pub fn chart_drawing(
    rel_id: &str,
    data: Arc<ChartData>,
    width: Emu,
    height: Emu,
    id: u32,
    name: &str,
) -> Drawing {
    let location = SourceLocation::unknown();
    let extent = Extent {
        cx: width,
        cy: height,
    };
    Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(extent),
            effect_extent: None,
            dist_top: Some(0),
            dist_bottom: Some(0),
            dist_left: Some(0),
            dist_right: Some(0),
            doc_pr: Some(DocPr {
                hidden: None,
                id: Some(id),
                name: Some(Arc::from(name)),
                descr: None,
                title: None,
            }),
            graphic_uri: Some(Arc::from(NS_CHART)),
            graphic: Box::new(Graphic::Chart(ForeignRefs {
                rels: vec![Arc::from(rel_id)],
                chart: Some(data),
                diagram: None,
                location: location.clone(),
            })),
            location: location.clone(),
        }),
        location,
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The chart part (`c:chartSpace`) for `data`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn chart_part(data: &ChartData) -> String {
    let mut xml = String::new();
    let _ = write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <c:chartSpace xmlns:c=\"{NS_CHART}\" xmlns:a=\"{NS_MAIN}\" xmlns:r=\"{NS_RELS}\">\
         <c:roundedCorners val=\"0\"/><c:chart>"
    );
    if let Some(title) = data.title.as_deref().filter(|t| !t.trim().is_empty()) {
        let _ = write!(
            xml,
            "<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx>\
             <c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>",
            escape(title)
        );
    } else {
        xml.push_str("<c:autoTitleDeleted val=\"1\"/>");
    }
    xml.push_str("<c:plotArea><c:layout/>");
    let pie = matches!(data.kind, ChartKind::Pie { .. });
    match &data.kind {
        ChartKind::Bar {
            horizontal,
            grouping,
        } => {
            let direction = if *horizontal { "bar" } else { "col" };
            let grouping_value = match grouping {
                BarGrouping::Clustered => "clustered",
                BarGrouping::Stacked => "stacked",
                BarGrouping::PercentStacked => "percentStacked",
            };
            let _ = write!(
                xml,
                "<c:barChart><c:barDir val=\"{direction}\"/><c:grouping val=\"{grouping_value}\"/>\
                 <c:varyColors val=\"0\"/>"
            );
            series(&mut xml, data, false);
            if *grouping != BarGrouping::Clustered {
                xml.push_str("<c:overlap val=\"100\"/>");
            }
            xml.push_str("<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>");
        }
        ChartKind::Line => {
            xml.push_str("<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>");
            series(&mut xml, data, false);
            xml.push_str(
                "<c:marker val=\"1\"/><c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>",
            );
        }
        ChartKind::Area => {
            xml.push_str("<c:areaChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>");
            series(&mut xml, data, false);
            xml.push_str("<c:axId val=\"1\"/><c:axId val=\"2\"/></c:areaChart>");
        }
        ChartKind::Pie { doughnut } => {
            let element = if *doughnut {
                "doughnutChart"
            } else {
                "pieChart"
            };
            let _ = write!(xml, "<c:{element}><c:varyColors val=\"1\"/>");
            series(&mut xml, data, true);
            xml.push_str("<c:firstSliceAng val=\"0\"/>");
            if *doughnut {
                xml.push_str("<c:holeSize val=\"50\"/>");
            }
            let _ = write!(xml, "</c:{element}>");
        }
        // Anything else is written as columns, the kind every reader draws.
        ChartKind::Scatter | ChartKind::Other(_) => {
            xml.push_str(
                "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>",
            );
            series(&mut xml, data, false);
            xml.push_str("<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>");
        }
    }
    if !pie {
        let horizontal = matches!(
            data.kind,
            ChartKind::Bar {
                horizontal: true,
                ..
            }
        );
        let (category_side, value_side) = if horizontal { ("l", "b") } else { ("b", "l") };
        let _ = write!(
            xml,
            "<c:catAx><c:axId val=\"1\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
             <c:delete val=\"0\"/><c:axPos val=\"{category_side}\"/><c:crossAx val=\"2\"/></c:catAx>\
             <c:valAx><c:axId val=\"2\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling>\
             <c:delete val=\"0\"/><c:axPos val=\"{value_side}\"/><c:majorGridlines/>\
             <c:crossAx val=\"1\"/></c:valAx>"
        );
    }
    xml.push_str("</c:plotArea>");
    if data.legend {
        let position = match data.legend_position {
            LegendPosition::Right => "r",
            LegendPosition::Left => "l",
            LegendPosition::Top => "t",
            LegendPosition::Bottom => "b",
        };
        let _ = write!(
            xml,
            "<c:legend><c:legendPos val=\"{position}\"/><c:overlay val=\"0\"/></c:legend>"
        );
    }
    xml.push_str("<c:plotVisOnly val=\"1\"/></c:chart></c:chartSpace>");
    xml
}

fn fill(xml: &mut String, color: &str, line: bool) {
    if line {
        let _ = write!(
            xml,
            "<c:spPr><a:ln w=\"28575\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln></c:spPr>",
            escape(color)
        );
    } else {
        let _ = write!(
            xml,
            "<c:spPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></c:spPr>",
            escape(color)
        );
    }
}

/// The series, each with its name, colour, categories and values.
fn series(xml: &mut String, data: &ChartData, pie: bool) {
    let count = data.category_count();
    for (index, one) in data.series.iter().enumerate() {
        let _ = write!(
            xml,
            "<c:ser><c:idx val=\"{index}\"/><c:order val=\"{index}\"/>"
        );
        if let Some(name) = &one.name {
            let _ = write!(xml, "<c:tx><c:v>{}</c:v></c:tx>", escape(name));
        }
        let accent = ACCENTS
            .get(index % ACCENTS.len())
            .copied()
            .unwrap_or("4472C4");
        if !pie {
            fill(
                xml,
                one.color.as_deref().unwrap_or(accent),
                matches!(data.kind, ChartKind::Line),
            );
        }
        if pie {
            for point in 0..count {
                let color = one
                    .point_colors
                    .iter()
                    .find(|(at, _)| *at == point)
                    .map_or_else(
                        || {
                            ACCENTS
                                .get(point % ACCENTS.len())
                                .copied()
                                .unwrap_or("4472C4")
                                .to_owned()
                        },
                        |(_, color)| color.clone(),
                    );
                let _ = write!(
                    xml,
                    "<c:dPt><c:idx val=\"{point}\"/><c:bubble3D val=\"0\"/>"
                );
                fill(xml, &color, false);
                xml.push_str("</c:dPt>");
            }
        }
        let _ = write!(xml, "<c:cat><c:strLit><c:ptCount val=\"{count}\"/>");
        for (at, label) in data.categories.iter().enumerate().take(count) {
            let _ = write!(
                xml,
                "<c:pt idx=\"{at}\"><c:v>{}</c:v></c:pt>",
                escape(label)
            );
        }
        let _ = write!(
            xml,
            "</c:strLit></c:cat><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"{count}\"/>"
        );
        for (at, value) in one.values.iter().enumerate() {
            if let Some(value) = value.filter(|v| v.is_finite()) {
                let _ = write!(xml, "<c:pt idx=\"{at}\"><c:v>{value}</c:v></c:pt>");
            }
        }
        xml.push_str("</c:numLit></c:val></c:ser>");
    }
}
