//! DrawingML serialization (`wp:inline`, `wp:anchor`, `pic:pic`, `wps:wsp`,
//! `wpg:wgp`).
//!
//! The payload of `a:graphicData` is carried by the model, and so is the
//! namespace URI it was parsed from. The writer re-emits the payload inside a
//! single `a:graphic/a:graphicData` pair, taking the URI from the model when it
//! has one and deriving it from the payload otherwise.
//!
//! A chart (`c:chart`) and a SmartArt diagram (`dgm:relIds`) are *references*
//! into parts the model does not carry. The reference is written when the
//! pass-through resolved it (W7: the parts came from the source package and are
//! in this package too), and dropped with a record when it did not — a
//! `c:chart` whose `r:id` points at nothing is unreadable content, which is worse
//! than an absent picture.

use strict_ooxml_wml::model::drawing::{
    AnchorDrawing, CustomGeometry, DocPr, Drawing, DrawingKind, EffectExtent, Extent, ForeignRefs,
    Graphic, GroupShape, GroupTransform, InlineDrawing, PathCommand, Picture, Position, Shape,
    ShapeColor, ShapeFill, ShapeGeometry, ShapeStroke, SrcRect, TextBox, TextBoxBody, Xfrm,
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

/// The relationship attributes of `dgm:relIds`, in the order the element carries
/// them: data, layout, quick style, colours. The order is the schema's, and it is
/// also the order the parser captured them in, so the two halves cannot drift.
const DIAGRAM_RELS: [&str; 4] = ["dm", "lo", "qs", "cs"];

/// Returns `true` when the payload can be written at all, recording a loss
/// otherwise.
///
/// A chart and a SmartArt diagram are writable only when every id they carry
/// resolved to a relationship this write declared. A partial set is refused: a
/// `dgm:relIds` with three of its four attributes is a diagram Word cannot lay
/// out, and a missing one is a reference that dangles.
fn writable(
    ctx: &mut Ctx<'_>,
    graphic: &Graphic,
    location: &strict_ooxml_core::error::SourceLocation,
) -> bool {
    let (feature_id, reason) = match graphic {
        Graphic::Picture(_) | Graphic::Shape(_) | Graphic::Group(_) => return true,
        Graphic::Chart(refs) => match foreign(ctx, refs) {
            Ok(()) => return true,
            Err(reason) => ("c:chart", reason),
        },
        Graphic::Diagram(refs) => match foreign(ctx, refs) {
            Ok(()) => return true,
            Err(reason) => ("dgm:relIds", reason),
        },
        Graphic::None | Graphic::Other => ("a:graphicData", "unrecognised graphic payload"),
    };
    ctx.report_unsupported(feature_id, reason, location);
    false
}

/// Whether a foreign reference can be written, and why not when it cannot.
fn foreign(ctx: &Ctx<'_>, refs: &ForeignRefs) -> Result<(), &'static str> {
    if refs.is_empty() {
        // Recognised from `graphicData/@uri` alone: the element that would carry
        // the id was not in the document, so there is nothing to re-point.
        return Err("the source document carried no relationship id for it");
    }
    for id in refs.ids() {
        if ctx.foreign_rel(id).is_none() {
            return Err("the part behind it is not in this package, so the reference would dangle");
        }
    }
    Ok(())
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
    effect_extent(xml, inline.effect_extent.as_ref());
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
    position_element(
        ctx,
        xml,
        "wp:positionH",
        anchor.position_h.as_ref(),
        "column",
        &anchor.location,
    );
    position_element(
        ctx,
        xml,
        "wp:positionV",
        anchor.position_v.as_ref(),
        "paragraph",
        &anchor.location,
    );
    match &anchor.extent {
        Some(extent) => extent_element(xml, "wp:extent", extent),
        None => {
            // `CT_Anchor` requires `wp:extent`; without it the element is invalid
            // however the rest of the anchor is shaped.
            ctx.report_partial(
                "wp:extent",
                "the floating drawing declared no extent; a zero extent is written",
                &anchor.location,
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
    effect_extent(xml, anchor.effect_extent.as_ref());
    if let Some(wrap) = &anchor.wrap {
        wrap_element(ctx, xml, wrap, anchor.extent.as_ref(), &anchor.location);
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

/// Writes `wp:positionH`/`wp:positionV`.
///
/// `CT_PosH` and `CT_PosV` make `@relativeFrom` `use="required"` and require a
/// `xsd:choice` of exactly one `wp:align` or `wp:posOffset`. The writer wrote the
/// attribute only when the model had one and the child only when the model had
/// one, so an anchor whose position came from a producer that relies on defaults
/// produced `<wp:positionH/>`: two violations each, ten on the corpus, and 20 in
/// total for the pair (`XS-21`).
///
/// The two axes do not share a base vocabulary. `ST_RelFromH` has `column` and
/// `ST_RelFromV` has `paragraph`, and neither has the other's, so the fallback is
/// a parameter: `column` on the horizontal axis is a value the vertical one
/// rejects.
fn position_element(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    name: &'static str,
    position: Option<&Position>,
    default_base: &str,
    location: &strict_ooxml_core::error::SourceLocation,
) {
    xml.start(name);
    if position.and_then(|p| p.relative_from.as_deref()).is_none() {
        ctx.report_partial(
            name,
            &format!(
                "the anchor's position named no base; `{default_base}` is written, which \
                 the element requires and which a floating object means without one"
            ),
            location,
        );
    }
    xml.attr(
        "relativeFrom",
        position
            .and_then(|p| p.relative_from.as_deref())
            .unwrap_or(default_base),
    );
    if let Some(position) = position {
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
    if !xml.has_content() {
        // The choice is required, so an element without one of the two is invalid
        // whatever it says about the base. A zero offset is the position the
        // element would have had anyway, and it is recorded.
        ctx.report_partial(
            name,
            "the anchor's position named neither an alignment nor an offset; a zero \
             offset is written, because the schema requires one of the two",
            location,
        );
        xml.start("wp:posOffset");
        xml.text("0");
        xml.end();
    }
    xml.end();
}

/// Writes `wp:wrap*`.
///
/// The five wrap elements do not share a shape, and the writer gave them all the
/// attributes of the widest one. Two facts the schema makes unmissable:
///
/// - `CT_WrapNone` is `<xsd:complexType name="CT_WrapNone"/>` - **no attributes
///   at all**. The side distances belong to `wp:anchor`, which carries all four.
///   Writing `distL`/`distR` on it is 28 violations on the corpus (`XS-04`).
/// - `CT_WrapTopBottom` has exactly `distT` and `distB`. Its distances are
///   vertical; there is no horizontal distance to write on it.
///
/// `CT_WrapSquare`, `CT_WrapTight` and `CT_WrapThrough` are the ones that take
/// `@wrapText`, and they take it **unqualified** - `wrapText`, not `w:wrapText`,
/// which is what the writer emitted and what the schema rejected (`XS-22`). It is
/// `use="required"`, so a model without one gets `bothSides` and a record.
///
/// `CT_WrapTight` and `CT_WrapThrough` also require a `wp:wrapPolygon` child,
/// which this model does not carry as points. **It is written as the frame's own
/// rectangle**, and the reason is the schema rather than the taste:
///
/// ```text
/// CT_WrapPath:  start    exactly 1
///               lineTo   2 or more
/// ```
///
/// so there is no conformant "no polygon" — an empty one is the same violation one
/// level down, and the census said so (`TZ-21`: "Missing child element(s). Expected
/// is ( wp:start )"). The two points that a producer would put there describe the
/// shape's outline, which this model does not have; the **four** points below
/// describe the frame's own box, which it does. A tight wrap around a rectangle is
/// a tight wrap, and the difference between it and the producer's polygon is
/// recorded rather than invented.
fn wrap_element(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    wrap: &strict_ooxml_wml::model::drawing::Wrap,
    extent: Option<&Extent>,
    location: &strict_ooxml_core::error::SourceLocation,
) {
    use strict_ooxml_wml::model::drawing::WrapKind as K;
    let name = match wrap.kind {
        K::None => "wp:wrapNone",
        K::Square => "wp:wrapSquare",
        K::Tight => "wp:wrapTight",
        K::Through => "wp:wrapThrough",
        K::TopAndBottom => "wp:wrapTopAndBottom",
    };
    xml.start(name);
    match wrap.kind {
        K::None => {
            // No attributes exist on CT_WrapNone. The distances the model carries
            // were written on wp:anchor, where the schema puts them.
        }
        K::TopAndBottom => {
            xml.attr("distT", wrap.dist_top.unwrap_or(0));
            xml.attr("distB", wrap.dist_bottom.unwrap_or(0));
        }
        K::Square | K::Tight | K::Through => {
            if wrap.wrap_text.is_none() {
                ctx.report_partial(
                    name,
                    "the wrap named no side; `bothSides` is written, which the schema \
                     requires and which is what a wrap means without a choice",
                    location,
                );
            }
            xml.attr("wrapText", wrap.wrap_text.as_deref().unwrap_or("bothSides"));
            if wrap.kind != K::Tight && wrap.kind != K::Through {
                xml.attr("distT", wrap.dist_top.unwrap_or(0));
                xml.attr("distB", wrap.dist_bottom.unwrap_or(0));
            }
            xml.attr("distL", wrap.dist_left.unwrap_or(0));
            xml.attr("distR", wrap.dist_right.unwrap_or(0));
            if matches!(wrap.kind, K::Tight | K::Through) {
                // `CT_WrapPath` is `<xsd:sequence><start minOccurs="1"/>`
                // `<lineTo minOccurs="2" maxOccurs="unbounded"/>`, so there is no
                // conformant "no polygon" - an empty `<wp:wrapPolygon/>` is the same
                // violation one level down, and the census said exactly that
                // (`TZ-21`).
                //
                // Two cases, and they are not the same claim. A producer that wrote
                // the contour has it carried on `Wrap::polygon`, so it is written
                // back as the producer drew it and nothing is reported. A shape that
                // came from VML has no contour to carry - `w10:wrap` in this corpus
                // is childless in all sixteen occurrences, so there was never a
                // contour to read - and the frame's own rectangle is written
                // instead, with a Partial record saying so. A tight wrap around a
                // box is a tight wrap; it is not the producer's outline, and the
                // difference is named rather than absorbed.
                if wrap.polygon.len() >= 3 {
                    xml.start("wp:wrapPolygon");
                    let mut points = wrap.polygon.iter();
                    if let Some((x, y)) = points.next() {
                        xml.start("wp:start");
                        xml.attr("x", x);
                        xml.attr("y", y);
                        xml.end();
                    }
                    for (x, y) in points {
                        xml.start("wp:lineTo");
                        xml.attr("x", x);
                        xml.attr("y", y);
                        xml.end();
                    }
                    xml.end();
                } else {
                    if wrap.polygon.is_empty() {
                        ctx.report_partial(
                            name,
                            "a tight or through wrap needs the wrap polygon's own points; the model \
                             carries none for a shape that came from VML - w10:wrap has no contour \
                             in this corpus - so the frame's rectangle is written, which is a tight \
                             wrap around a box rather than around the producer's outline",
                            location,
                        );
                    } else {
                        // One or two points cannot satisfy CT_WrapPath. Writing a
                        // rectangle instead would be conformant AND wrong twice
                        // over: it would hide the producer's contour and invent
                        // one. The rectangle goes out below, and the defect in the
                        // INPUT is named here rather than repaired.
                        ctx.report_partial(
                            name,
                            "a wrap polygon with fewer than three points is not CT_WrapPath; the \
                             producer's points are dropped and the frame's rectangle is written, \
                             and the input's own violation is what the XSD gate should name",
                            location,
                        );
                    }
                    let (cx, cy) = extent.map_or((0, 0), |size| (size.cx.0, size.cy.0));
                    xml.start("wp:wrapPolygon");
                    xml.start("wp:start");
                    xml.attr("x", 0);
                    xml.attr("y", 0);
                    xml.end();
                    for (x, y) in [(cx, 0), (cx, cy), (0, cy)] {
                        xml.start("wp:lineTo");
                        xml.attr("x", x);
                        xml.attr("y", y);
                        xml.end();
                    }
                    xml.end();
                }
            }
        }
    }
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
        // A chart and a SmartArt diagram are references into parts the model does
        // not carry. `writable` has already refused the ones whose parts are not
        // in this package, so by the time we get here every id below resolves to a
        // relationship the written `.rels` declares.
        Graphic::Chart(refs) => {
            xml.attr("uri", strict_graphic_uri(graphic_uri, URI_CHART));
            xml.start("c:chart");
            if let Some(id) = refs.ids().first().and_then(|id| ctx.foreign_rel(id)) {
                xml.attr("r:id", id);
            }
            xml.end();
        }
        Graphic::Diagram(refs) => {
            xml.attr("uri", strict_graphic_uri(graphic_uri, URI_DIAGRAM));
            xml.start("dgm:relIds");
            for (name, id) in DIAGRAM_RELS.iter().zip(refs.ids()) {
                if let Some(id) = ctx.foreign_rel(id) {
                    xml.attr(&format!("r:{name}"), id);
                }
            }
            xml.end();
        }
        Graphic::None | Graphic::Other => {
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
        .and_then(|blip| ctx.media_rel(blip.resolved.as_ref()));
    match embed {
        Some(embed) => xml.attr("r:embed", embed),
        None => ctx.report_unsupported(
            "a:blip/@r:embed",
            "picture has no embedded image relationship",
            &strict_ooxml_core::error::SourceLocation::unknown(),
        ),
    }
    // AUD-38: `a:srcRect` is a sibling of `a:blip` under `pic:blipFill`, not a
    // child. The parser skips unknown children of `a:blip`, so a nested crop was
    // written once and lost on the next read — fixed-point failed.
    xml.end();
    if let Some(slice) = &picture.src_rect {
        source_rect(xml, slice);
    }
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
    xml.start("a:srcRect");
    xml.attr("l", thousandths_percent(slice.left));
    xml.attr("t", thousandths_percent(slice.top));
    xml.attr("r", thousandths_percent(slice.right));
    xml.attr("b", thousandths_percent(slice.bottom));
    xml.end();
}

/// `a:srcRect` stores thousandths of a percent. Strict writes a percentage
/// with three fractional digits, so `1253` is `1.253%` and `10` is `0.010%`.
/// Zero is `0%`. The spelling is decimal text, not a binary float.
fn thousandths_percent(value: i32) -> String {
    if value == 0 {
        return String::from("0%");
    }
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.unsigned_abs();
    let whole = absolute / 1000;
    let fraction = absolute % 1000;
    format!("{sign}{whole}.{fraction:03}%")
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
    // AUD-49: one `a:path` per modelled path, each with its own w/h.
    for path in &custom.paths {
        xml.start("a:path");
        xml.attr("w", path.width.max(1));
        xml.attr("h", path.height.max(1));
        if let Some(fill) = path.fill.as_deref() {
            xml.attr("fill", fill);
        }
        if let Some(stroke) = path.stroke {
            xml.attr("stroke", if stroke { "true" } else { "false" });
        }
        for command in &path.commands {
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
    }
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
                // `a:gs/@pos` is `ST_PositiveFixedPercentage`. Model values are
                // thousandths; Strict wants a `%` form.
                xml.attr("pos", thousandths_percent(stop.position));
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
        xml.attr("val", percentage_attr(tint.as_ref()));
        xml.end();
    }
    if let Some(shade) = &theme.shade {
        xml.start("a:shade");
        xml.attr("val", percentage_attr(shade.as_ref()));
        xml.end();
    }
}

/// Formats a DrawingML percentage attribute for Strict emission.
fn percentage_attr(value: &str) -> String {
    if value.ends_with('%') {
        return value.to_owned();
    }
    if let Ok(number) = value.parse::<i32>() {
        return thousandths_percent(number);
    }
    value.to_owned()
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
        BlipRef, Drawing, DrawingKind, Extent, Graphic, InlineDrawing, Picture, SrcRect,
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
                effect_extent: None,
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
        let mut targets = BTreeMap::new();
        targets.insert(
            "/word/media/image1.png".to_owned(),
            "media/image1.png".to_owned(),
        );
        let mut ctx = Ctx::new(&mut report).with_relationships(
            BTreeMap::new(),
            media,
            targets,
            BTreeMap::new(),
        );
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

    /// AUD-38: `a:srcRect` is a sibling of `a:blip` under `pic:blipFill`.
    #[test]
    fn source_rect_is_a_sibling_of_blip() {
        let drawing = Drawing {
            kind: DrawingKind::Inline(InlineDrawing {
                extent: Some(Extent {
                    cx: Emu(100),
                    cy: Emu(100),
                }),
                effect_extent: None,
                doc_pr: None,
                graphic_uri: None,
                graphic: Box::new(Graphic::Picture(Picture {
                    name: None,
                    descr: None,
                    blip: Some(BlipRef {
                        embed: Some(RelId::new("rId1")),
                        link: None,
                        resolved: Some(PartId::new("/word/media/image1.png")),
                        location: SourceLocation::unknown(),
                    }),
                    extent: None,
                    src_rect: Some(SrcRect {
                        left: 1,
                        top: 2,
                        right: 3,
                        bottom: 4,
                    }),
                    xfrm: None,
                })),
                location: SourceLocation::unknown(),
            }),
            location: SourceLocation::unknown(),
        };
        let mut report = NormalizationReport::new();
        let mut media = BTreeMap::new();
        media.insert("/word/media/image1.png".to_owned(), "rId1".to_owned());
        let mut targets = BTreeMap::new();
        targets.insert(
            "/word/media/image1.png".to_owned(),
            "media/image1.png".to_owned(),
        );
        let mut ctx = Ctx::new(&mut report).with_relationships(
            BTreeMap::new(),
            media,
            targets,
            BTreeMap::new(),
        );
        let mut xml = XmlWriter::new();
        drawing_element(&mut ctx, &mut xml, &drawing);
        let text = xml.finish().expect("balanced");
        assert!(
            text.contains(
                r#"<a:blip r:embed="rId1"/><a:srcRect l="0.001%" t="0.002%" r="0.003%" b="0.004%"/>"#
            ),
            "srcRect must follow the closed blip: {text}"
        );
        assert!(
            !text.contains("<a:blip r:embed=\"rId1\"><a:srcRect"),
            "srcRect must not nest inside blip: {text}"
        );
    }

    #[test]
    fn source_rectangle_percentages_keep_thousandths() {
        assert_eq!(super::thousandths_percent(0), "0%");
        assert_eq!(super::thousandths_percent(10), "0.010%");
        assert_eq!(super::thousandths_percent(1253), "1.253%");
        assert_eq!(super::thousandths_percent(-1253), "-1.253%");
    }
}
