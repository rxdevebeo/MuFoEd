//! Regression for the blank second page of corpus document 035.
mod common;

use strict_ooxml_core::normalize::{
    ExpansionLimit, NormalizerOptions, RawNormalizer, TransitionalNormalizer,
};
use strict_ooxml_core::part::PartId;

#[test]
fn normalized_vml_diagram_reserves_space_before_its_caption() {
    let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:v="urn:schemas-microsoft-com:vml" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:body><w:p><w:r><w:t>Before diagram</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:pict>
<v:group coordsize="31027,23630" style="width:244.3pt;height:186.05pt;mso-position-horizontal-relative:char;mso-position-vertical-relative:line">
<v:shape style="left:0;top:0;width:29589;height:23630"><v:imagedata r:id="rIdImage"/></v:shape>
<v:group coordsize="10000,6000" style="position:absolute;left:1000;top:1000;width:10000;height:6000">
<v:rect style="left:0;top:0;width:10000;height:6000" filled="f" stroked="f"><v:textbox><w:txbxContent><w:p><w:r><w:t>Diagram label</w:t></w:r></w:p></w:txbxContent></v:textbox></v:rect>
</v:group>
</v:group></w:pict></w:r></w:p>
<w:p><w:r><w:t>Figure caption</w:t></w:r></w:p>
<w:p><w:r><w:t>After diagram</w:t></w:r></w:p></w:body></w:document>"#;
    let normalizer = TransitionalNormalizer::with_options(NormalizerOptions {
        max_expansion: ExpansionLimit::Bytes(32768),
        ..Default::default()
    });
    let xml = normalizer
        .normalize_part(&PartId::new("/word/document.xml"), source.as_bytes())
        .expect("normalize")
        .into_owned();
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="{}" Target="media/image.png"/></Relationships>"#,
        common::IMAGE_REL
    );
    let (package, document) = common::open_bytes(common::build_docx(&[
        ("[Content_Types].xml", common::content_types().into_bytes()),
        ("_rels/.rels", common::root_rels().into_bytes()),
        ("word/document.xml", xml),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/media/image.png", common::tiny_png()),
    ]));
    let pages = strict_ooxml_render_svg::render_with_media(
        &document,
        &strict_ooxml_render_svg::RenderOptions::default(),
        Some(&package),
    )
    .expect("render");
    assert_eq!(pages.len(), 1);
    let svg = roxmltree::Document::parse(&pages.first().expect("page").svg).expect("svg");
    let images: Vec<_> = svg
        .descendants()
        .filter(|n| n.has_tag_name("image"))
        .collect();
    assert_eq!(images.len(), 1);
    let image = images.first().expect("image");
    let value = |name| {
        image
            .attribute(name)
            .expect("image coordinate")
            .parse::<f64>()
            .expect("number")
    };
    assert!((value("width") - 310.64).abs() < 0.1);
    assert!((value("height") - 248.067).abs() < 0.1);
    assert!(
        value("x") > 96.0,
        "diagram follows centred paragraph placement"
    );
    let bottom = value("y") + value("height");
    for text in ["Figure", "After"] {
        let node = svg
            .descendants()
            .find(|n| {
                n.has_tag_name("text") && n.text().is_some_and(|value| value.starts_with(text))
            })
            .expect("following text");
        let baseline: f64 = node
            .attribute("y")
            .expect("baseline")
            .parse()
            .expect("number");
        assert!(baseline > bottom, "{text} overlaps diagram");
    }
    assert!(svg
        .descendants()
        .any(|n| n.has_tag_name("text")
            && n.text().is_some_and(|value| value.starts_with("Diagram"))));
}
