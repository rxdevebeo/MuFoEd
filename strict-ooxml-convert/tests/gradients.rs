//! An axial shading is kept as a linear gradient, not reported and dropped.

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::pdf::PdfBuilder;
use strict_ooxml_wml::model::drawing::{Drawing, DrawingKind, Graphic, ShapeFill};
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::Block;

#[test]
fn an_axial_shading_is_a_linear_gradient() {
    let mut pdf = PdfBuilder::new();
    let shading = pdf.object(
        "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [100 100 200 100] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
         /Extend [true true] >>",
    );
    pdf.set_media_box([0, 0, 612, 792]);
    let resources = format!("<< /Shading << /Sh0 {shading} 0 R >> >>");
    pdf.page_with(b"q 100 100 100 50 re W n /Sh0 sh Q", &resources);
    let bytes = pdf.build();
    let mut reader = PdfDocument::open(&bytes, PdfLimits::default()).expect("open pdf");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Semantic)).expect("convert");
    assert!(
        !converted.report.to_string().contains("vector.gradient"),
        "{}",
        converted.report
    );
    let fills: Vec<&ShapeFill> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .flat_map(|paragraph| paragraph.inlines.iter())
        .filter_map(|inline| match inline {
            Inline::Drawing(Drawing {
                kind: DrawingKind::Anchor(anchor),
                ..
            }) => match anchor.graphic.as_ref() {
                Graphic::Shape(shape) => shape.fill.as_ref(),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let Some(ShapeFill::Gradient { stops, angle, .. }) = fills.first() else {
        panic!("no gradient: {fills:?}");
    };
    assert_eq!(stops.len(), 2);
    assert_eq!(
        stops[0].color.value.as_ref().map(|c| c.as_str().to_owned()),
        Some("FF0000".to_owned())
    );
    // Left to right is 0 degrees.
    assert_eq!(*angle, Some(0));
}
