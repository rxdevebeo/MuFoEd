//! Small self-contained visual witnesses for fills and contour wrapping.
use crate::DocxBuilder;
use std::fmt::Write as _;

/// A 192 by 96 pixel inline shape, for colour and clipping oracles.
#[must_use]
pub fn fill_docx(geometry: &str, fill: &str) -> Vec<u8> {
    let body = format!("<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1828800\" cy=\"914400\"/><wp:docPr id=\"1\" name=\"fill witness\"/><a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\"><wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm><a:prstGeom prst=\"{geometry}\"><a:avLst/></a:prstGeom>{fill}<a:ln><a:noFill/></a:ln></wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>");
    DocxBuilder::strict()
        .body(&format!("{body}{SECTION}"))
        .build()
}

/// Three-stop horizontal red/green/blue gradient.
pub const GRADIENT: &str = "<a:gradFill><a:gsLst><a:gs pos=\"0%\"><a:srgbClr val=\"FF0000\"/></a:gs><a:gs pos=\"50%\"><a:srgbClr val=\"00FF00\"/></a:gs><a:gs pos=\"100%\"><a:srgbClr val=\"0000FF\"/></a:gs></a:gsLst><a:lin ang=\"0\" scaled=\"1\"/></a:gradFill>";

const SECTION: &str = "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr>";

/// Red hatch over a blue background.
#[must_use]
pub fn pattern(preset: &str) -> String {
    format!("<a:pattFill prst=\"{preset}\"><a:fgClr><a:srgbClr val=\"FF0000\"/></a:fgClr><a:bgClr><a:srgbClr val=\"0000FF\"/></a:bgClr></a:pattFill>")
}

/// A U-shaped polygon in EMU. Through wrap admits text into the central gap;
/// tight wrap excludes the whole envelope, with the same drawing and words.
#[must_use]
pub fn contour_docx(through: bool) -> Vec<u8> {
    let points = [
        (0, 0),
        (1_828_800, 0),
        (1_828_800, 1_828_800),
        (1_280_160, 1_828_800),
        (1_280_160, 548_640),
        (548_640, 548_640),
        (548_640, 1_828_800),
        (0, 1_828_800),
    ];
    let mut polygon = String::from("<wp:wrapPolygon edited=\"1\"><wp:start x=\"0\" y=\"0\"/>");
    let mut path = String::from("<a:moveTo><a:pt x=\"0\" y=\"0\"/></a:moveTo>");
    for (x, y) in points.iter().skip(1) {
        let _ = write!(polygon, "<wp:lineTo x=\"{x}\" y=\"{y}\"/>");
        let _ = write!(path, "<a:lnTo><a:pt x=\"{x}\" y=\"{y}\"/></a:lnTo>");
    }
    polygon.push_str("</wp:wrapPolygon>");
    path.push_str("<a:close/>");
    let kind = if through { "wrapThrough" } else { "wrapTight" };
    let text = "Small words flow around this U shape. ".repeat(70);
    let body = format!("<w:p><w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"19050\" distR=\"19050\" simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"column\"><wp:posOffset>914400</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx=\"1828800\" cy=\"1828800\"/><wp:{kind} wrapText=\"bothSides\">{polygon}</wp:{kind}><wp:docPr id=\"1\" name=\"U contour\"/><a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\"><wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"1828800\"/></a:xfrm><a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"1828800\" b=\"1828800\"/><a:pathLst><a:path w=\"1828800\" h=\"1828800\">{path}</a:path></a:pathLst></a:custGeom><a:solidFill><a:srgbClr val=\"99CCEE\"/></a:solidFill><a:ln><a:noFill/></a:ln></wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r><w:r><w:t>{text}</w:t></w:r></w:p>");
    DocxBuilder::strict()
        .body(&format!("{body}{SECTION}"))
        .build()
}

/// Named fixtures for a visual inspection directory.
#[must_use]
pub fn gallery() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("01-gradient-rectangle.docx", fill_docx("rect", GRADIENT)),
        ("02-gradient-ellipse.docx", fill_docx("ellipse", GRADIENT)),
        (
            "03-pattern-cross.docx",
            fill_docx("ellipse", &pattern("cross")),
        ),
        (
            "04-pattern-diagonal.docx",
            fill_docx("roundRect", &pattern("dnDiag")),
        ),
        ("05-pattern-dots.docx", fill_docx("rect", &pattern("pct25"))),
        ("06-tight-contour.docx", contour_docx(false)),
        ("07-through-contour.docx", contour_docx(true)),
        (
            "08-unsupported-pattern-warning.docx",
            fill_docx("rect", &pattern("unknownPattern")),
        ),
        (
            "09-path-gradient-warning.docx",
            fill_docx("ellipse", &GRADIENT.replace("<a:lin ang=\"0\" scaled=\"1\"/>", "<a:path path=\"circle\"><a:fillToRect l=\"50%\" t=\"50%\" r=\"50%\" b=\"50%\"/></a:path>")),
        ),
    ]
}
