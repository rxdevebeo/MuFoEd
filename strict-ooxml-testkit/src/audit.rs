//! One generated package per confirmed audit mechanism.
//!
//! Expected numbers are stated here, in named units. Nothing in this module
//! calls the production layout, section, or font resolver. `1 px` means 1/96
//! inch. `1 twip` is 1/1440 inch, so `1 px = 15 twips` and `1 pt = 20 twips`.
//! PDF user space in these fixtures is `1 unit = 1 pt`.

use crate::docx::DocxBuilder;
use crate::pdf::PdfBuilder;

/// A 1×1 red PNG. The bytes are a fixture, not a rendered page.
pub const PNG_1X1_RED: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// Whether the scenario is a package or a PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    /// A `.docx` package.
    Docx,
    /// A PDF 1.7 file.
    Pdf,
}

/// A named input plus the independent expectation its tests assert.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    /// Audit id, `A01` … `A21`.
    pub id: &'static str,
    /// Fix card that owns the regression.
    pub card: &'static str,
    /// Stable file stem.
    pub name: &'static str,
    /// Package or PDF.
    pub kind: ScenarioKind,
    /// Builds the bytes. Deterministic.
    pub build: fn() -> Vec<u8>,
    /// Units and expected values. Not a production measurement.
    pub oracle: &'static str,
}

/// Every public scenario, in audit order.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "A01",
        card: "F05",
        name: "two-page-sections",
        kind: ScenarioKind::Pdf,
        build: two_page_mixed_pdf,
        oracle: "Page 1 media box 612×792 pt; page 2 media box 842×595 pt. \
                 After write/reopen both geometries remain. Visual render is exactly 2 pages.",
    },
    Scenario {
        id: "A02",
        card: "F19",
        name: "text-and-vector",
        kind: ScenarioKind::Pdf,
        build: text_and_vector_pdf,
        oracle: "Filled rectangle RGB(1,0,0) at PDF user space x=100 pt, y=100 pt, \
                 200×80 pt, plus the text Hello. The rectangle is either kept or listed \
                 as a vector loss. It is not a silent omission.",
    },
    Scenario {
        id: "A03",
        card: "F18",
        name: "text-baseline",
        kind: ScenarioKind::Pdf,
        build: text_and_vector_pdf,
        oracle: "Text x=72 pt and baseline 92 pt from the top of a 792 pt page. \
                 At 96 dpi that is x=96 px and baseline=122.667 px (92×96/72).",
    },
    Scenario {
        id: "A06",
        card: "F03",
        name: "vml-loss",
        kind: ScenarioKind::Docx,
        build: vml_loss_docx,
        oracle: "Text KEEP THIS TEXT survives. The v:shape is a normalization loss, \
                 so a written result is Degraded (exit 1), not Clean.",
    },
    Scenario {
        id: "A08",
        card: "F20",
        name: "vml-loss-view",
        kind: ScenarioKind::Docx,
        build: vml_loss_docx,
        oracle: "Same package as A06. A successful view still shows the normalization loss.",
    },
    Scenario {
        id: "A09",
        card: "F04",
        name: "settings-bitmap",
        kind: ScenarioKind::Docx,
        build: settings_bitmap_docx,
        oracle: "Input w:val=0001. Strict output has w:allStyles=true and no w:val. \
                 Mask bit 0x0001 is allStyles.",
    },
    Scenario {
        id: "A09",
        card: "F04",
        name: "crop-percentage",
        kind: ScenarioKind::Docx,
        build: crop_docx,
        oracle: "srcRect thousandths of a percent: l=1253 → 1.253%, t=10 → 0.010%, \
                 r=20 → 0.020%, b=30 → 0.030%.",
    },
    Scenario {
        id: "A09",
        card: "F04",
        name: "level-suffix",
        kind: ScenarioKind::Docx,
        build: level_suffix_docx,
        oracle: "suff=nothing stays nothing. CT_Lvl order puts suff before lvlText.",
    },
    Scenario {
        id: "A09",
        card: "F04",
        name: "math-onoff",
        kind: ScenarioKind::Docx,
        build: math_onoff_docx,
        oracle: "m:smallFrac m:val=off becomes the boolean false.",
    },
    Scenario {
        id: "A10",
        card: "F06",
        name: "direct-italic",
        kind: ScenarioKind::Docx,
        build: direct_italic_docx,
        oracle: "Paragraph mark italic and direct run italic both stay on. Output is italic.",
    },
    Scenario {
        id: "A11",
        card: "F07",
        name: "cs-font",
        kind: ScenarioKind::Docx,
        build: cs_font_docx,
        oracle: "cs=Segoe UI does not replace ascii/hAnsi for Latin or Cyrillic.",
    },
    Scenario {
        id: "A11",
        card: "F07",
        name: "unknown-face",
        kind: ScenarioKind::Docx,
        build: unknown_face_docx,
        oracle: "Family NotARealCssFamily is replaced by one bundled face for both measure and delivery.",
    },
    Scenario {
        id: "A11",
        card: "F07",
        name: "theme-heading",
        kind: ScenarioKind::Docx,
        build: theme_heading_docx,
        oracle: "asciiTheme=majorHAnsi is embedded in standalone SVG. The resource hash matches layout and PDF.",
    },
    Scenario {
        id: "A12",
        card: "F11",
        name: "zero-table-width",
        kind: ScenarioKind::Docx,
        build: zero_table_width_docx,
        oracle: "tblW dxa 0 with gridCol 9355 twips is 623.667 px (9355/15) in a wide container, not 1 px.",
    },
    Scenario {
        id: "A13",
        card: "F14",
        name: "relative-width",
        kind: ScenarioKind::Docx,
        build: relative_width_docx,
        oracle: "Page width 9633 twips = 642.2 px. pctWidth 94100 is 94.1% (100% = 100000). \
                 Width = 604.3102 px. Fallback extent 768 px does not win.",
    },
    Scenario {
        id: "A14",
        card: "F12",
        name: "paragraph-shading",
        kind: ScenarioKind::Docx,
        build: paragraph_shading_docx,
        oracle: "Fill F7F7F7 covers the whole paragraph block, including every line.",
    },
    Scenario {
        id: "A15",
        card: "F13",
        name: "tall-header",
        kind: ScenarioKind::Docx,
        build: tall_header_docx,
        oracle: "top and header are 567 twips = 37.8 px. The first body ink starts below the header ink.",
    },
    Scenario {
        id: "A16",
        card: "F10",
        name: "partial-zero-indent",
        kind: ScenarioKind::Docx,
        build: partial_zero_indent_docx,
        oracle: "Numbering left 720 twips and hanging 360 twips (48 px and 24 px). \
                 Direct start=0 is kept. Marker and text do not share an x.",
    },
    Scenario {
        id: "A17",
        card: "F17",
        name: "vmerge-split",
        kind: ScenarioKind::Docx,
        build: vmerge_split_docx,
        oracle: "Three cantSplit rows of 600 twips = 40 px. Page content height 1440 twips = 96 px, \
                 so two rows (80 px) stay on page 1 and one row (40 px) continues on page 2.",
    },
    Scenario {
        id: "A18",
        card: "F16",
        name: "frame-shift",
        kind: ScenarioKind::Docx,
        build: frame_docx,
        oracle: "framePr x=3000 twips y=2000 twips is origin (200 px, 133.333 px). \
                 Both paragraphs share that signature.",
    },
    Scenario {
        id: "A19",
        card: "F09",
        name: "right-tab",
        kind: ScenarioKind::Docx,
        build: right_tab_docx,
        oracle: "Right stop at 4800 twips = 320 px. The text 12 ends at 320±0.25 px and a dot leader is drawn.",
    },
    Scenario {
        id: "A20",
        card: "F08",
        name: "first-line-indent",
        kind: ScenarioKind::Docx,
        build: first_line_indent_docx,
        oracle: "Container width 9000 twips = 600 px. firstLine 2400 twips = 160 px moves the start, not the right edge.",
    },
    Scenario {
        id: "A21",
        card: "F15",
        name: "wrap-square",
        kind: ScenarioKind::Docx,
        build: wrap_square_docx,
        oracle: "Square wrap, 169 px image, side distances 12 px. Text does not ink the expanded image box. \
                 wrapNone on the same picture is a different line break.",
    },
];

/// The scenario table.
pub fn scenarios() -> &'static [Scenario] {
    SCENARIOS
}

/// Two pages, US Letter then landscape A4-sized media boxes, one line each.
pub fn two_page_mixed_pdf() -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let font = helvetica(&mut pdf);
    pdf.set_media_box([0, 0, 612, 792]);
    text_page(&mut pdf, font, "BT /F1 12 Tf 72 700 Td (Page one) Tj ET");
    pdf.set_media_box([0, 0, 842, 595]);
    text_page(&mut pdf, font, "BT /F1 12 Tf 72 500 Td (Page two) Tj ET");
    pdf.build()
}

/// Letter page, red rectangle, and `Hello` at x=72 pt, 92 pt down from the top.
pub fn text_and_vector_pdf() -> Vec<u8> {
    let mut pdf = PdfBuilder::new();
    let font = helvetica(&mut pdf);
    pdf.set_media_box([0, 0, 612, 792]);
    let content = "1 0 0 rg 100 100 200 80 re f BT /F1 12 Tf 72 700 Td (Hello) Tj ET";
    text_page(&mut pdf, font, content);
    pdf.build()
}

/// Transitional paragraph plus a VML shape the normalizer must record.
pub fn vml_loss_docx() -> Vec<u8> {
    let document = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\"><w:body><w:p><w:r><w:t>KEEP THIS TEXT</w:t></w:r></w:p>\
<w:p><w:r><w:pict><v:shape id=\"lost-shape\" style=\"width:100pt;height:60pt\"/></w:pict></w:r></w:p>\
</w:body></w:document>";
    DocxBuilder::transitional()
        .document_bytes(document.as_bytes().to_vec())
        .build()
}

/// `w:stylePaneFormatFilter w:val="0001"`.
pub fn settings_bitmap_docx() -> Vec<u8> {
    settings_part("<w:stylePaneFormatFilter w:val=\"0001\"/>")
}

/// Picture crop in thousandths of a percent, plus a 1×1 PNG.
pub fn crop_docx() -> Vec<u8> {
    let body = "<w:p><w:r><w:drawing>\
<wp:inline><wp:extent cx=\"9525\" cy=\"9525\"/><wp:docPr id=\"1\" name=\"crop\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"crop\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage\"/>\
<a:srcRect l=\"1253\" t=\"10\" r=\"20\" b=\"30\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9525\" cy=\"9525\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>";
    DocxBuilder::transitional()
        .body(body)
        .rel("rIdImage", "image", "media/image1.png")
        .part("word/media/image1.png", PNG_1X1_RED.to_vec())
        .build()
}

/// Numbering level with `lvlText`, `lvlJc`, and `suff=nothing`.
pub fn level_suffix_docx() -> Vec<u8> {
    let numbering = "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\">\
<w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
<w:lvlJc w:val=\"left\"/><w:suff w:val=\"nothing\"/></w:lvl></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    DocxBuilder::transitional()
        .body(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
<w:r><w:t>Item</w:t></w:r></w:p>",
        )
        .rel("rIdNum", "numbering", "numbering.xml")
        .content_type(
            "/word/numbering.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
        )
        .part_xml("word/numbering.xml", "w:numbering", numbering)
        .build()
}

/// Settings math property `m:smallFrac` = `off`.
pub fn math_onoff_docx() -> Vec<u8> {
    settings_part("<m:mathPr><m:smallFrac m:val=\"off\"/></m:mathPr>")
}

/// Direct italic on the paragraph mark and on the visible run.
pub fn direct_italic_docx() -> Vec<u8> {
    paragraph_body(
        "<w:pPr><w:rPr><w:i/></w:rPr></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t>Italic</w:t></w:r>",
    )
}

/// Direct bold on the paragraph mark and on the visible run.
pub fn direct_bold_docx() -> Vec<u8> {
    paragraph_body(
        "<w:pPr><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r>",
    )
}

/// Latin and Cyrillic with a distinct complex-script face.
pub fn cs_font_docx() -> Vec<u8> {
    paragraph_body(
        "<w:r><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Segoe UI\"/></w:rPr>\
<w:t>Hello Привет</w:t></w:r>",
    )
}

/// A family name that is not a bundled face.
pub fn unknown_face_docx() -> Vec<u8> {
    paragraph_body(
        "<w:r><w:rPr><w:rFonts w:ascii=\"NotARealCssFamily\" w:hAnsi=\"NotARealCssFamily\"/></w:rPr>\
<w:t>Fallback</w:t></w:r>",
    )
}

/// A run that asks for the major theme face.
pub fn theme_heading_docx() -> Vec<u8> {
    paragraph_body(
        "<w:r><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/></w:rPr>\
<w:t>Heading</w:t></w:r>",
    )
}

/// `tblW` of dxa 0 and a single grid column of 9355 twips.
pub fn zero_table_width_docx() -> Vec<u8> {
    paragraph_body(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"9355\"/></w:tblGrid>\
<w:tr><w:tc><w:p><w:r><w:t>Column</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    )
}

/// Relative width 94.1% of a 642.2 px page, with a 768 px fallback extent.
pub fn relative_width_docx() -> Vec<u8> {
    let body = "<w:p><w:r><w:drawing \
xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\">\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" \
behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:align>left</wp:align></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:align>top</wp:align></wp:positionV>\
<wp:extent cx=\"7315200\" cy=\"952500\"/>\
<wp14:sizeRelH relativeFrom=\"page\"><wp14:pctWidth>94100</wp14:pctWidth></wp14:sizeRelH>\
<wp:wrapNone/>\
<wp:docPr id=\"1\" name=\"rel\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"rel\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"7315200\" cy=\"952500\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"9633\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    DocxBuilder::transitional()
        .body(body)
        .rel("rIdImage", "image", "media/image1.png")
        .part("word/media/image1.png", PNG_1X1_RED.to_vec())
        .build()
}

/// Grey paragraph shading `F7F7F7`.
pub fn paragraph_shading_docx() -> Vec<u8> {
    paragraph_body(
        "<w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F7F7F7\"/></w:pPr>\
<w:r><w:t>code line</w:t></w:r>",
    )
}

/// Header distance and top margin both 37.8 px, with a one-line header.
pub fn tall_header_docx() -> Vec<u8> {
    let body = "<w:p><w:r><w:t>Body line</w:t></w:r></w:p>\
<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>\
<w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"567\" w:right=\"720\" w:bottom=\"567\" w:left=\"720\" w:header=\"567\" w:footer=\"567\" w:gutter=\"0\"/>\
</w:sectPr>";
    DocxBuilder::transitional()
        .body(body)
        .rel("rIdHeader", "header", "header1.xml")
        .content_type(
            "/word/header1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        )
        .part_xml(
            "word/header1.xml",
            "w:hdr",
            "<w:p><w:r><w:t>HEADER</w:t></w:r></w:p>",
        )
        .build()
}

/// Direct `start=0` beside a numbering level that still has left and hanging.
pub fn partial_zero_indent_docx() -> Vec<u8> {
    let numbering = "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\">\
<w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
<w:lvlJc w:val=\"left\"/><w:suff w:val=\"tab\"/>\
<w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    let body = "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>\
<w:ind w:start=\"0\"/></w:pPr><w:r><w:t>Item</w:t></w:r></w:p>";
    DocxBuilder::transitional()
        .body(body)
        .rel("rIdNum", "numbering", "numbering.xml")
        .content_type(
            "/word/numbering.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
        )
        .part_xml("word/numbering.xml", "w:numbering", numbering)
        .build()
}

/// Three merged rows that do not fit on a 96 px page.
pub fn vmerge_split_docx() -> Vec<u8> {
    let row = |merge: &str, text: &str| {
        format!(
            "<w:tr><w:trPr><w:trHeight w:val=\"600\"/><w:cantSplit/></w:trPr>\
<w:tc><w:tcPr><w:vMerge w:val=\"{merge}\"/>\
<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"CCCCCC\"/>\
<w:tcW w:w=\"2500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:tcW w:w=\"2500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>side</w:t></w:r></w:p></w:tc></w:tr>"
        )
    };
    let body = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2500\"/><w:gridCol w:w=\"2500\"/></w:tblGrid>{}{}{}</w:tbl>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"1440\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>",
        row("restart", "merged"),
        row("continue", ""),
        row("continue", "")
    );
    DocxBuilder::transitional().body(&body).build()
}

/// Two paragraphs with the same frame signature at x=3000, y=2000 twips.
pub fn frame_docx() -> Vec<u8> {
    framed_at(3000)
}

/// Same frame as [`frame_docx`] with a different x, in twips.
pub fn framed_at(x_twips: i32) -> Vec<u8> {
    let body = format!(
        "<w:p><w:pPr><w:framePr w:w=\"4000\" w:h=\"800\" w:x=\"{x_twips}\" w:y=\"2000\"/></w:pPr>\
<w:r><w:t>alpha</w:t></w:r></w:p>\
<w:p><w:pPr><w:framePr w:w=\"4000\" w:h=\"800\" w:x=\"{x_twips}\" w:y=\"2000\"/></w:pPr>\
<w:r><w:t>beta</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    DocxBuilder::transitional().body(&body).build()
}

/// A right tab at 320 px with a dot leader, then the text `12`.
pub fn right_tab_docx() -> Vec<u8> {
    right_tab_at(4800)
}

/// Right tab at `pos_twips` on a 720 px page.
pub fn right_tab_at(pos_twips: i32) -> Vec<u8> {
    let body = format!(
        "<w:p><w:pPr><w:tabs><w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"{pos_twips}\"/></w:tabs></w:pPr>\
<w:r><w:t>Title</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>12</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"10800\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>"
    );
    DocxBuilder::transitional().body(&body).build()
}

/// Positive first-line indent inside a 600 px page.
pub fn first_line_indent_docx() -> Vec<u8> {
    let body = "<w:p><w:pPr><w:ind w:firstLine=\"2400\"/></w:pPr>\
<w:r><w:t>one two three four five six seven eight</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"9000\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    DocxBuilder::transitional().body(body).build()
}

/// Square wrap around a 169 px picture.
pub fn wrap_square_docx() -> Vec<u8> {
    wrap_docx("wrapSquare", "bothSides")
}

/// The same picture with no wrap.
pub fn wrap_none_docx() -> Vec<u8> {
    wrap_docx("wrapNone", "")
}

/// A package whose XML part is not an element.
pub fn damaged_xml_docx() -> Vec<u8> {
    DocxBuilder::transitional()
        .document_bytes(b"<broken".to_vec())
        .build()
}

/// A package whose CRC-32 does not match the bytes.
pub fn damaged_crc_docx() -> Vec<u8> {
    DocxBuilder::transitional().body("<w:p/>").bad_crc().build()
}

/// A truncated archive.
pub fn damaged_truncated_docx() -> Vec<u8> {
    let bytes = DocxBuilder::transitional().body("<w:p/>").build();
    bytes[..20].to_vec()
}

fn paragraph_body(inner: &str) -> Vec<u8> {
    DocxBuilder::transitional()
        .body(&format!("<w:p>{inner}</w:p>"))
        .build()
}

fn settings_part(inner: &str) -> Vec<u8> {
    DocxBuilder::transitional()
        .body("<w:p><w:r><w:t>SETTINGS PROBE</w:t></w:r></w:p>")
        .rel("rIdSettings", "settings", "settings.xml")
        .content_type(
            "/word/settings.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        )
        .part_xml("word/settings.xml", "w:settings", inner)
        .build()
}

fn helvetica(pdf: &mut PdfBuilder) -> u32 {
    pdf.object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
}

fn text_page(pdf: &mut PdfBuilder, font: u32, content: &str) {
    let resources = format!("<< /Font << /F1 {font} 0 R >> >>");
    pdf.page_with(content.as_bytes(), &resources);
}

fn wrap_docx(element: &str, wrap_text: &str) -> Vec<u8> {
    let wrap = if wrap_text.is_empty() {
        format!("<wp:{element}/>")
    } else {
        format!("<wp:{element} wrapText=\"{wrap_text}\"/>")
    };
    let body = format!(
        "<w:p><w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" \
simplePos=\"0\" relativeHeight=\"1\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:posOffset>914400</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
<wp:extent cx=\"1609725\" cy=\"1609725\"/>{wrap}<wp:docPr id=\"1\" name=\"portrait\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"portrait\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1609725\" cy=\"1609725\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>\
<w:r><w:t>Words flow beside the portrait and must leave the wrap rectangle.</w:t></w:r></w:p>"
    );
    DocxBuilder::transitional()
        .body(&body)
        .rel("rIdImage", "image", "media/image1.png")
        .part("word/media/image1.png", PNG_1X1_RED.to_vec())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{inspect_docx, inspect_pdf};

    #[test]
    fn every_scenario_is_a_readable_package() {
        assert!(SCENARIOS.len() >= 21);
        for scenario in SCENARIOS {
            let bytes = (scenario.build)();
            assert!(!bytes.is_empty(), "{}", scenario.name);
            match scenario.kind {
                ScenarioKind::Docx => {
                    inspect_docx(&bytes).unwrap_or_else(|error| {
                        panic!("{}: {} {}", scenario.name, error.kind, error.detail)
                    });
                }
                ScenarioKind::Pdf => {
                    inspect_pdf(&bytes).unwrap_or_else(|error| {
                        panic!("{}: {} {}", scenario.name, error.kind, error.detail)
                    });
                }
            }
        }
    }

    #[test]
    fn damaged_packages_are_rejected() {
        let xml = inspect_docx(&damaged_xml_docx()).expect_err("broken xml");
        assert_eq!(xml.kind, "xml");
        let crc = inspect_docx(&damaged_crc_docx()).expect_err("bad crc");
        assert_eq!(crc.kind, "crc");
        let truncated = inspect_docx(&damaged_truncated_docx()).expect_err("truncated");
        assert_eq!(truncated.kind, "zip");
    }

    #[test]
    fn stated_units_match_the_fixture_constants() {
        assert!((9355.0_f64 / 15.0 - 623.666).abs() < 0.01);
        assert!((94.1_f64 / 100.0 * 642.2 - 604.3102).abs() < 0.001);
        assert!((92.0_f64 * 96.0 / 72.0 - 122.666).abs() < 0.01);
        assert!((3000.0_f64 / 15.0 - 200.0).abs() < 0.001);
        assert!((2000.0_f64 / 15.0 - 133.333).abs() < 0.001);
    }
}
