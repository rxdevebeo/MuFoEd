//! DrawingML serialization (`wp:inline`, `wp:anchor`, `pic:pic`, `wps:wsp`,
//! `wpg:wgp`).
//!
//! The payload of `a:graphicData` is carried by the model, and so is the
//! namespace URI it was parsed from. The writer re-emits the payload inside a
//! single `a:graphic/a:graphicData` pair, taking the URI from the model when it
//! has one and deriving it from the payload otherwise — a chart or a SmartArt
//! reference is written as the reference alone, because the model does not carry
//! the chart part itself and inventing one would produce a package that no
//! reader can open.

use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, CustomGeometry, DocPr, Drawing, DrawingKind, EffectExtent, Extent, Graphic,
    GroupShape, GroupTransform, InlineDrawing, PathCommand, Picture, Position, Shape, ShapeColor,
    ShapeFill, ShapeGeometry, ShapeStroke, SrcRect, TextBox, TextBoxBody, Xfrm,
};
use strict_ooxml_wml::model::values::Emu;

use crate::body::blocks;
use crate::ctx::Ctx;
use crate::xml::{XmlWriter, NS_A, NS_PIC, NS_WPG, NS_WPS};

/// The `graphicData` URI of a picture.
pub const URI_PICTURE: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// The `graphicData` URI of a shape.
pub const URI_SHAPE: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
/// The `graphicData` URI of a group.
pub const URI_GROUP: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";
/// The `graphicData` URI of a chart.
pub const URI_CHART: &str = "http://purl.oclc.org/ooxml/drawingml/chart";
/// The `graphicData` URI of a SmartArt diagram.
pub const URI_DIAGRAM: &str = "http://purl.oclc.org/ooxml/drawingml/diagram";

/// Returns `true` when the payload can be written at all, recording a loss
/// otherwise.
///
/// A chart (`c:chart`) and a SmartArt diagram (`dgm:relIds`) are *references*
/// into parts the model does not carry: writing the element without its
/// `r:id` targets would leave a dangling relationship, which Word reports as
/// unreadable content. Dropping the object and saying so is the honest option.
fn writable(
    ctx: &mut Ctx<'_>,
    graphic: &Graphic,
    location: &strict_ooxml_core::error::SourceLocation,
) -> bool {
    let (feature_id, reason) = match graphic {
        Graphic::Picture(_) | Graphic::Shape(_) | Graphic::Group(_) => return true,
        Graphic::Chart => (
            "c:chart",
            "chart data is not part of the model, so the reference would dangle",
        ),
        Graphic::Diagram => (
            "dgm:relIds",
            "SmartArt data is not part of the model, so the reference would dangle",
        ),
        Graphic::None | Graphic::Other => ("a:graphicData", "unrecognised graphic payload"),
    };
    ctx.report_unsupported(feature_id, reason, location);
    false
}

/// Writes `w:drawing`.
pub fn drawing_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, drawing: &Drawing) {
    match &drawing.kind {
        DrawingKind::Inline(inline) => {
            if !writable(ctx, inline.graphic.as_ref(), &drawing.location) {
                return;
            }
            xml.start("w:drawing");
            inline_element(ctx, xml, inline);
            xml.end();
        }
        DrawingKind::Anchor(anchor) => {
            if !writable(ctx, anchor.graphic.as_ref(), &drawing.location) {
                return;
            }
            xml.start("w:drawing");
            anchor_element(ctx, xml, anchor);
            xml.end();
        }
        DrawingKind::Opaque(opaque) => {
            ctx.report_unsupported(
                &opaque.feature_id(),
                "drawing kept in the model but not serializable",
                &drawing.location,
            );
        }
    }
}

/// Writes `wp:inline`.
pub fn inline_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, inline: &InlineDrawing) {
    xml.start("wp:inline");
    xml.attr("distT", "0");
    xml.attr("distB", "0");
    xml.attr("distL", "0");
    xml.attr("distR", "0");
    match &inline.extent {
        Some(extent) => extent_element(xml, "wp:extent", extent),
        None => {
            ctx.report_partial(
                "wp:extent",
                "inline drawing had no declared extent; a zero extent is written",
                &inline.location,
            );
            extent_element(
                xml,
                "wp:extent",
                &Extent {
                    cx: Emu(0),
                    cy: Emu(0),
                },
            );
        }
    }
    effect_extent(xml, None);
    document_properties(ctx, xml, &inline.doc_pr, "Picture", &inline.location);
    graphic(
        ctx,
        xml,
        inline.graphic_uri.as_deref(),
        inline.graphic.as_ref(),
    );
    xml.end();
}

/// Writes `wp:anchor`.
pub fn anchor_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, anchor: &AnchorDrawing) {
    xml.start("wp:anchor");
    xml.attr("distT", anchor.dist_top.unwrap_or(0));
    xml.attr("distB", anchor.dist_bottom.unwrap_or(0));
    xml.attr("distL", anchor.dist_left.unwrap_or(0));
    xml.attr("distR", anchor.dist_right.unwrap_or(0));
    xml.attr("simplePos", bool_str(anchor.simple_pos));
    xml.attr("relativeHeight", anchor.relative_height.unwrap_or(0));
    xml.attr("behindDoc", bool_str(anchor.behind_doc));
    xml.attr("locked", bool_str(anchor.locked));
    xml.attr("layoutInCell", bool_str(anchor.layout_in_cell));
    xml.attr("allowOverlap", bool_str(anchor.allow_overlap));

    xml.start("wp:simplePos");
    xml.attr("x", "0");
    xml.attr("y", "0");
    xml.end();
    position_element(xml, "wp:positionH", anchor.position_h.as_ref());
    position_element(xml, "wp:positionV", anchor.position_v.as_ref());
    if let Some(extent) = &anchor.extent {
        extent_element(xml, "wp:extent", extent);
    }
    effect_extent(xml, anchor.effect_extent.as_ref());
    if let Some(wrap) = &anchor.wrap {
        wrap_element(xml, wrap);
    }
    document_properties(ctx, xml, &anchor.doc_pr, "Shape", &anchor.location);
    graphic(
        ctx,
        xml,
        anchor.graphic_uri.as_deref(),
        anchor.graphic.as_ref(),
    );
    xml.end();
}

fn position_element(xml: &mut XmlWriter, name: &'static str, position: Option<&Position>) {
    xml.start(name);
    if let Some(position) = position {
        xml.attr(
            "relativeFrom",
            position.relative_from.as_deref().unwrap_or("column"),
        );
        if let Some(align) = &position.align {
            xml.start("wp:align");
            xml.text(align);
            xml.end();
        } else if let Some(offset) = position.offset {
            xml.start("wp:posOffset");
            xml.text(&offset.0.to_string());
            xml.end();
        }
    }
    xml.end();
}

fn wrap_element(xml: &mut XmlWriter, wrap: &strict_ooxml_wml::model::drawing::Wrap) {
    use strict_ooxml_wml::model::drawing::WrapKind as K;
    let name = match wrap.kind {
        K::None => "wp:wrapNone",
        K::Square => "wp:wrapSquare",
        K::Tight => "wp:wrapTight",
        K::Through => "wp:wrapThrough",
        K::TopAndBottom => "wp:wrapTopAndBottom",
    };
    xml.start(name);
    xml.attr_w_opt("wrapText", wrap.wrap_text.as_deref());
    xml.attr("distL", wrap.dist_left.unwrap_or(0));
    xml.attr("distR", wrap.dist_right.unwrap_or(0));
    xml.end();
}

fn extent_element(xml: &mut XmlWriter, name: &str, extent: &Extent) {
    xml.start(name);
    xml.attr("cx", extent.cx.0);
    xml.attr("cy", extent.cy.0);
    xml.end();
}

fn effect_extent(xml: &mut XmlWriter, extent: Option<&EffectExtent>) {
    let (left, top, right, bottom) = match extent {
        Some(extent) => (extent.left.0, extent.top.0, extent.right.0, extent.bottom.0),
        None => (0, 0, 0, 0),
    };
    xml.start("wp:effectExtent");
    xml.attr("l", left);
    xml.attr("t", top);
    xml.attr("r", right);
    xml.attr("b", bottom);
    xml.end();
}

fn document_properties(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    doc_pr: &Option<DocPr>,
    fallback_name: &str,
    _location: &strict_ooxml_core::error::SourceLocation,
) {
    // `wp:docPr/@id` must be unique within the part and Word refuses a file that
    // repeats one, so a fresh id is allocated rather than reusing the parsed
    // value. The id carries no meaning beyond that uniqueness.
    let id = ctx.next_doc_pr_id();
    xml.start("wp:docPr");
    xml.attr("id", id);
    xml.attr(
        "name",
        doc_pr
            .as_ref()
            .and_then(|pr| pr.name.as_deref())
            .unwrap_or(fallback_name),
    );
    xml.attr_opt("descr", doc_pr.as_ref().and_then(|pr| pr.descr.as_deref()));
    xml.end();
}

/// The Strict form of `a:graphicData/@uri`, falling back to the default for
/// this payload when the model carries none.
fn strict_graphic_uri<'a>(uri: Option<&'a str>, fallback: &'static str) -> &'a str {
    match uri {
        Some(value) => strict_ooxml_core::ns::registry::strict_form(value).unwrap_or(value),
        None => fallback,
    }
}

/// Writes `a:graphic` with the payload the model carries.
///
/// `a:graphicData/@uri` names a namespace, so it is written in its Strict form
/// even when the model carries the Transitional one the source document used.
/// It is a value rather than a declaration, which is exactly why it slipped
/// through: the conformance check reads declarations, and the normalizer
/// rewrites declarations, so a picture written with
/// `schemas.openxmlformats.org/drawingml/2006/picture` in `@uri` was reported
/// Strict by both. A URI the registry does not know is written as it came —
/// inventing a Strict name for an unknown namespace would be worse.
fn graphic(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, graphic_uri: Option<&str>, payload: &Graphic) {
    xml.start("a:graphic");
    xml.start("a:graphicData");
    match payload {
        Graphic::Picture(picture) => {
            xml.attr("uri", strict_graphic_uri(graphic_uri, URI_PICTURE));
            picture_element(ctx, xml, picture);
        }
        Graphic::Shape(shape) => {
            xml.attr("uri", strict_graphic_uri(graphic_uri, URI_SHAPE));
            shape_element(ctx, xml, shape);
        }
        Graphic::Group(group) => {
            xml.attr("uri", strict_graphic_uri(graphic_uri, URI_GROUP));
            group_element(ctx, xml, group);
        }
        // A chart or a SmartArt diagram is referenced by relationship ids into
        // parts the model does not carry. Writing the reference alone would
        // produce a dangling :id, which Word reports as unreadable content —
        // worse than not drawing the object at all — so these are dropped by
        // writable before any element is emitted.
        Graphic::Chart | Graphic::Diagram | Graphic::None | Graphic::Other => {
            let _ = graphic_uri;
            xml.end();
            xml.end();
            return;
        }
    }
    xml.end();
    xml.end();
}

/// Writes `pic:pic`.
fn picture_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, picture: &Picture) {
    xml.start("pic:pic");
    xml.start("pic:nvPicPr");
    xml.start("pic:cNvPr");
    xml.attr("id", "0");
    xml.attr("name", picture.name.as_deref().unwrap_or("Picture"));
    xml.attr_opt("descr", picture.descr.as_deref());
    xml.end();
    xml.start("pic:cNvPicPr");
    xml.end();
    xml.end();

    xml.start("pic:blipFill");
    xml.start("a:blip");
    let embed = picture
        .blip
        .as_ref()
        .and_then(|blip| ctx.media_rel(blip.resolved.as_ref()).map(ToOwned::to_owned));
    match embed {
        Some(embed) => xml.attr("r:embed", embed),
        None => ctx.report_unsupported(
            "a:blip/@r:embed",
            "picture has no embedded image relationship",
            &strict_ooxml_core::error::SourceLocation::unknown(),
        ),
    }
    if let Some(slice) = &picture.src_rect {
        source_rect(xml, slice);
    }
    xml.end();
    xml.start("a:stretch");
    xml.empty("a:fillRect");
    xml.end();
    xml.end();

    xml.start("pic:spPr");
    transform(xml, &picture.xfrm, picture.extent.as_ref());
    geometry_element(xml, &ShapeGeometry::None);
    xml.end();
    xml.end();
}

fn source_rect(xml: &mut XmlWriter, slice: &SrcRect) {
    if slice.left == 0 && slice.top == 0 && slice.right == 0 && slice.bottom == 0 {
        return;
    }
    xml.start("a:srcRect");
    xml.attr("l", slice.left);
    xml.attr("t", slice.top);
    xml.attr("r", slice.right);
    xml.attr("b", slice.bottom);
    xml.end();
}

/// Writes `wps:wsp`.
fn shape_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, shape: &Shape) {
    xml.start("wps:wsp");
    xml.start("wps:cNvSpPr");
    xml.empty("a:spLocks");
    xml.end();
    xml.start("wps:spPr");
    transform(xml, &shape.xfrm, shape.extent.as_ref());
    geometry_element(xml, &shape.geometry);
    if let Some(fill) = &shape.fill {
        fill_element(xml, fill);
    }
    if let Some(stroke) = &shape.stroke {
        stroke_element(xml, stroke);
    }
    xml.end();
    if let Some(style) = &shape.style {
        xml.start("wps:style");
        for (name, index, color) in [
            ("a:lnRef", style.line_ref, style.line_ref.map(|_| "phClr")),
            ("a:fillRef", style.fill_ref, style.fill_ref.map(|_| "phClr")),
            (
                "a:effectRef",
                style.effect_ref,
                style.effect_ref.map(|_| "phClr"),
            ),
            ("a:fontRef", style.font_ref, style.font_ref.map(|_| "tx1")),
        ] {
            if let Some(index) = index {
                xml.start(name);
                xml.attr("idx", index);
                if let Some(color) = color {
                    xml.start("a:schemeClr");
                    xml.attr("val", color);
                    xml.end();
                }
                xml.end();
            }
        }
        xml.end();
    }
    if let Some(text) = &shape.text {
        text_box(ctx, xml, text);
    }
    xml.end();
}

fn transform(xml: &mut XmlWriter, xfrm: &Option<Xfrm>, extent: Option<&Extent>) {
    // With no explicit `a:xfrm` the element is still written when the caller
    // knows an extent, because a shape without a transform has no placement at
    // all; without one there is nothing to say and nothing is written.
    if xfrm.is_none() && extent.is_none() {
        return;
    }
    xml.start("a:xfrm");
    if let Some(xfrm) = xfrm {
        if xfrm.flip_h {
            xml.attr("flipH", "true");
        }
        if xfrm.flip_v {
            xml.attr("flipV", "true");
        }
        xml.attr_opt("rot", xfrm.rot);
    }
    match xfrm.and_then(|xfrm| xfrm.offset) {
        Some((x, y)) => {
            xml.start("a:off");
            xml.attr("x", x.0);
            xml.attr("y", y.0);
            xml.end();
        }
        None => {
            xml.start("a:off");
            xml.attr("x", "0");
            xml.attr("y", "0");
            xml.end();
        }
    }
    match extent {
        Some(extent) => {
            xml.start("a:ext");
            xml.attr("cx", extent.cx.0);
            xml.attr("cy", extent.cy.0);
            xml.end();
        }
        None => {
            xml.start("a:ext");
            xml.attr("cx", "0");
            xml.attr("cy", "0");
            xml.end();
        }
    }
    xml.end();
}

fn geometry_element(xml: &mut XmlWriter, geometry: &ShapeGeometry) {
    match geometry {
        ShapeGeometry::None => {}
        ShapeGeometry::Preset(preset) => {
            xml.start("a:prstGeom");
            xml.attr("prst", preset.as_ref());
            xml.empty("a:avLst");
            xml.end();
        }
        ShapeGeometry::Custom(custom) => custom_geometry(xml, custom),
    }
}

fn custom_geometry(xml: &mut XmlWriter, custom: &CustomGeometry) {
    xml.start("a:custGeom");
    xml.start("a:avLst");
    xml.end();
    xml.start("a:gdLst");
    xml.end();
    xml.start("a:ahLst");
    xml.end();
    xml.start("a:cxnLst");
    xml.end();
    xml.start("a:rect");
    xml.attr("l", "0");
    xml.attr("t", "0");
    xml.attr("r", "r");
    xml.attr("b", "b");
    xml.end();
    xml.start("a:pathLst");
    xml.start("a:path");
    xml.attr("w", custom.width.max(1));
    xml.attr("h", custom.height.max(1));
    for command in &custom.commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                xml.start("a:moveTo");
                point(xml, "a:pt", x, y);
                xml.end();
            }
            PathCommand::LineTo { x, y } => {
                xml.start("a:lnTo");
                point(xml, "a:pt", x, y);
                xml.end();
            }
            PathCommand::CubicBezTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                xml.start("a:cubicBezTo");
                point(xml, "a:pt", x1, y1);
                point(xml, "a:pt", x2, y2);
                point(xml, "a:pt", x, y);
                xml.end();
            }
            PathCommand::Close => xml.empty("a:close"),
        }
    }
    xml.end();
    xml.end();
    xml.end();
}

fn point(xml: &mut XmlWriter, name: &'static str, x: i64, y: i64) {
    xml.start(name);
    xml.attr("x", x);
    xml.attr("y", y);
    xml.end();
}

fn fill_element(xml: &mut XmlWriter, fill: &ShapeFill) {
    match fill {
        ShapeFill::None => xml.empty("a:noFill"),
        ShapeFill::Solid { color } => {
            xml.start("a:solidFill");
            shape_color(xml, color);
            xml.end();
        }
        ShapeFill::Gradient { stops, angle } => {
            xml.start("a:gradFill");
            xml.start("a:gsLst");
            for stop in stops {
                xml.start("a:gs");
                xml.attr("pos", stop.position);
                shape_color(xml, &stop.color);
                xml.end();
            }
            xml.end();
            if let Some(angle) = angle {
                xml.start("a:lin");
                xml.attr("ang", angle);
                xml.attr("scaled", "0");
                xml.end();
            }
            xml.end();
        }
        ShapeFill::Pattern {
            preset,
            foreground,
            background,
        } => {
            xml.start("a:pattFill");
            xml.attr("prst", preset.as_deref().unwrap_or("pct5"));
            if let Some(foreground) = foreground {
                xml.start("a:fgClr");
                shape_color(xml, foreground);
                xml.end();
            }
            if let Some(background) = background {
                xml.start("a:bgClr");
                shape_color(xml, background);
                xml.end();
            }
            xml.end();
        }
    }
}

fn shape_color(xml: &mut XmlWriter, color: &ShapeColor) {
    match (&color.value, &color.theme) {
        (Some(value), _) => {
            xml.start("a:srgbClr");
            xml.attr("val", value.as_str());
            xml.end();
        }
        (None, Some(theme)) => {
            xml.start("a:schemeClr");
            xml.attr("val", theme.color.as_str());
            theme_modifiers(xml, theme);
            xml.end();
        }
        (None, None) => xml.empty("a:schemeClr"),
    }
}

fn theme_modifiers(xml: &mut XmlWriter, theme: &strict_ooxml_wml::model::values::ThemeColorRef) {
    if let Some(tint) = &theme.tint {
        xml.start("a:tint");
        xml.attr("val", tint.as_ref());
        xml.end();
    }
    if let Some(shade) = &theme.shade {
        xml.start("a:shade");
        xml.attr("val", shade.as_ref());
        xml.end();
    }
}

fn stroke_element(xml: &mut XmlWriter, stroke: &ShapeStroke) {
    xml.start("a:ln");
    // `w` is an attribute of `a:ln`, so it has to be written before the first
    // child; emitting it afterwards would put it inside the last child's markup,
    // which the reader drops — a silent loss of the line width.
    if let Some(width) = stroke.width {
        xml.attr("w", width.0);
    }
    if stroke.none {
        xml.empty("a:noFill");
    } else if let Some(color) = &stroke.color {
        xml.start("a:solidFill");
        shape_color(xml, color);
        xml.end();
    }
    if let Some(dash) = &stroke.dash {
        xml.start("a:prstDash");
        xml.attr("val", dash.as_ref());
        xml.end();
    }
    if let Some(head) = &stroke.head_end {
        xml.start("a:headEnd");
        xml.attr("type", head.as_ref());
        xml.end();
    }
    if let Some(tail) = &stroke.tail_end {
        xml.start("a:tailEnd");
        xml.attr("type", tail.as_ref());
        xml.end();
    }
    xml.end();
}

fn text_box(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, text: &TextBox) {
    xml.start("wps:txbx");
    xml.start("w:txbxContent");
    blocks(ctx, xml, &text.blocks);
    xml.end();
    xml.end();
    xml.start("wps:bodyPr");
    if let Some(body) = &text.body {
        body_properties(xml, body);
    }
    xml.end();
}

fn body_properties(xml: &mut XmlWriter, body: &TextBoxBody) {
    use strict_ooxml_wml::model::drawing::TextAnchor as A;
    if let Some(anchor) = body.anchor {
        let value = match anchor {
            A::Top => "t",
            A::Center => "ctr",
            A::Bottom => "b",
        };
        xml.attr("anchor", value);
    }
    if body.anchor_centered {
        xml.attr("anchorCtr", "true");
    }
    if let Some(inset) = body.left_inset {
        xml.attr("lIns", inset.0);
    }
    if let Some(inset) = body.top_inset {
        xml.attr("tIns", inset.0);
    }
    if let Some(inset) = body.right_inset {
        xml.attr("rIns", inset.0);
    }
    if let Some(inset) = body.bottom_inset {
        xml.attr("bIns", inset.0);
    }
}

/// Writes `wpg:wgp`.
fn group_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, group: &GroupShape) {
    xml.start("wpg:wgp");
    xml.start("wpg:cNvGrpSpPr");
    xml.end();
    xml.start("wpg:grpSpPr");
    group_transform(xml, &group.xfrm);
    xml.end();
    for child in &group.children {
        match child {
            Graphic::Shape(shape) => shape_element(ctx, xml, shape),
            Graphic::Group(nested) => group_element(ctx, xml, nested),
            Graphic::Picture(picture) => picture_element(ctx, xml, picture),
            _ => ctx.report_unsupported(
                "wpg:wgp/*",
                "only shapes, groups and pictures are valid inside a group",
                &strict_ooxml_core::error::SourceLocation::unknown(),
            ),
        }
    }
    xml.end();
}

fn group_transform(xml: &mut XmlWriter, transform: &Option<GroupTransform>) {
    let Some(transform) = transform else {
        return;
    };
    xml.start("a:xfrm");
    if transform.flip_h {
        xml.attr("flipH", "true");
    }
    if transform.flip_v {
        xml.attr("flipV", "true");
    }
    xml.attr_opt("rot", transform.rot);
    let (offset, extent) = match (transform.offset, transform.extent) {
        (Some(offset), Some(extent)) => (offset, extent),
        _ => {
            xml.start("a:off");
            xml.attr("x", "0");
            xml.attr("y", "0");
            xml.end();
            xml.start("a:ext");
            xml.attr("cx", "0");
            xml.attr("cy", "0");
            xml.end();
            xml.end();
            return;
        }
    };
    xml.start("a:off");
    xml.attr("x", offset.0 .0);
    xml.attr("y", offset.1 .0);
    xml.end();
    xml.start("a:ext");
    xml.attr("cx", extent.cx.0);
    xml.attr("cy", extent.cy.0);
    xml.end();
    let (child_offset, child_extent) = match (transform.child_offset, transform.child_extent) {
        (Some(offset), Some(extent)) => (offset, extent),
        _ => (offset, extent),
    };
    xml.start("a:chOff");
    xml.attr("x", child_offset.0 .0);
    xml.attr("y", child_offset.1 .0);
    xml.end();
    xml.start("a:chExt");
    xml.attr("cx", child_extent.cx.0);
    xml.attr("cy", child_extent.cy.0);
    xml.end();
    xml.end();
}

fn bool_str(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// The namespaces a part using any of the above must declare.
#[must_use]
pub fn namespaces() -> Vec<(&'static str, &'static str)> {
    vec![
        ("a", NS_A),
        ("pic", NS_PIC),
        ("wps", NS_WPS),
        ("wpg", NS_WPG),
        ("c", URI_CHART),
        ("dgm", URI_DIAGRAM),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::normalize::report::NormalizationReport;
    use strict_ooxml_wml::model::drawing::{
        BlipRef, Drawing, DrawingKind, Extent, Graphic, InlineDrawing, Picture,
    };
    use strict_ooxml_wml::model::values::Emu;

    use strict_ooxml_core::opc::rels::RelId;
    use strict_ooxml_core::part::PartId;

    use super::drawing_element;
    use crate::ctx::Ctx;
    use crate::xml::XmlWriter;

    #[test]
    fn an_inline_picture_carries_its_extent_and_relationship() {
        let drawing = Drawing {
            kind: DrawingKind::Inline(InlineDrawing {
                extent: Some(Extent {
                    cx: Emu(914_400),
                    cy: Emu(914_400),
                }),
                doc_pr: None,
                graphic_uri: None,
                graphic: Box::new(Graphic::Picture(Picture {
                    name: Some("Logo".into()),
                    descr: None,
                    blip: Some(BlipRef {
                        embed: Some(RelId::new("rId7")),
                        link: None,
                        resolved: Some(PartId::new("/word/media/image1.png")),
                        location: SourceLocation::unknown(),
                    }),
                    extent: Some(Extent {
                        cx: Emu(914_400),
                        cy: Emu(914_400),
                    }),
                    src_rect: None,
                    xfrm: None,
                })),
                location: SourceLocation::unknown(),
            }),
            location: SourceLocation::unknown(),
        };
        let mut report = NormalizationReport::new();
        let mut media = BTreeMap::new();
        media.insert("/word/media/image1.png".to_owned(), "rId7".to_owned());
        let mut ctx =
            Ctx::new(&mut report).with_relationships(BTreeMap::new(), media, BTreeMap::new());
        let mut xml = XmlWriter::new();
        drawing_element(&mut ctx, &mut xml, &drawing);
        let text = xml.finish().expect("balanced");
        assert!(text.starts_with("<w:drawing><wp:inline"), "{text}");
        assert!(
            text.contains("<wp:extent cx=\"914400\" cy=\"914400\"/>"),
            "{text}"
        );
        assert!(text.contains("<a:blip r:embed=\"rId7\"/>"), "{text}");
        assert!(
            text.contains("uri=\"http://purl.oclc.org/ooxml/drawingml/picture\""),
            "{text}"
        );
        assert!(text.contains("name=\"Logo\""), "{text}");
    }

    #[test]
    fn document_property_ids_stay_unique() {
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = XmlWriter::new();
        let first = ctx.next_doc_pr_id();
        let second = ctx.next_doc_pr_id();
        assert_ne!(first, second);
        assert!(!xml.has_open_elements());
    }
}
