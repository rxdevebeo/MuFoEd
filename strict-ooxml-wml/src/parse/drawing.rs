//! Parsing of `w:drawing`: inline pictures, anchored drawings, shapes, groups
//! and text boxes (`STAGE-2 §8`, `STAGE-5B-TASK.md` §5.1–§5.4).

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::drawing::{
    AnchorDrawing, BlipRef, CustomGeometry, DocPr, Drawing, DrawingKind, EffectExtent, Extent,
    ForeignRefs, GeometryPath, GradientStop, Graphic, GroupShape, GroupTransform, InlineDrawing,
    LockedCanvas, MediaItem, MediaKind, PathCommand, Picture, Position, RelativeSize, Shape,
    ShapeColor, ShapeFill, ShapeGeometry, ShapeStroke, ShapeStyle, SrcRect, TextAnchor, TextBox,
    TextBoxBody, Wrap, WrapKind, Xfrm,
};
use crate::model::support::SupportStatus;
use crate::model::values::{Color, Emu, ThemeColor, ThemeColorRef};
use crate::{
    CHART_STRICT_NS, DIAGRAM_STRICT_NS, DRAWINGML_STRICT_NS, LOCKED_CANVAS_STRICT_NS,
    LOCKED_CANVAS_TRANSITIONAL_NS, MS_WORD_2006_WML_NS, MS_WORD_PROCESSING_DRAWING_NS,
    MS_WORD_PROCESSING_GROUP_NS, MS_WORD_PROCESSING_SHAPE_NS, PICTURE_STRICT_NS, RELS_STRICT_NS,
    WORDPROCESSING_DRAWING_STRICT_NS, WORD_PROCESSING_GROUP_STRICT_NS,
    WORD_PROCESSING_SHAPE_STRICT_NS,
};

use super::{attr_in_ns, plain_attr, PartParser};

/// Whether a name is in the wordprocessingDrawing namespace.
fn is_wordprocessing_drawing(name: &QName) -> bool {
    name.ns
        .as_ref()
        .is_some_and(|ns| ns == crate::WORDPROCESSING_DRAWING_STRICT_NS)
}

/// `r:id` on a `c:chart`.
const R_ID: &str = "id";
/// `r:dm` on a `dgm:relIds` - the diagram data part.
const REL_DM: &str = "dm";
/// `r:lo` on a `dgm:relIds` - the diagram layout part.
const REL_LO: &str = "lo";
/// `wp14:anchorId` / `wp14:editId` are editor bookmarks. Strict does not
/// declare them, and dropping them without a feature id hides the change.
fn record_editor_ids(parser: &mut PartParser<'_>, element: &str, attrs: &[Attr]) {
    for attr in attrs {
        let local = attr.name.local();
        if local == "anchorId" || local == "editId" {
            parser.record(
                &format!("{element}@{local}"),
                SupportStatus::Ignored,
                Some("editor id is not part of Strict".to_owned()),
                Some(parser.location()),
            );
        }
    }
}

/// `r:qs` on a `dgm:relIds` - the diagram quick-style part.
const REL_QS: &str = "qs";
/// `r:cs` on a `dgm:relIds` - the diagram colour part.
const REL_CS: &str = "cs";

impl PartParser<'_> {
    /// Parses a `w:drawing` element; its start element has been consumed.
    pub(crate) fn parse_drawing(&mut self) -> Result<Drawing> {
        let location = self.location();
        self.nested(|parser| {
            let mut kind = DrawingKind::Opaque(crate::model::inline::OpaqueInline {
                namespace: parser.intern(crate::WML_STRICT_NS),
                local: parser.intern("drawing"),
                attributes: Vec::new(),
                location: location.clone(),
            });
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                            && name.local() == "inline"
                        {
                            record_editor_ids(parser, "wp:inline", &attrs);
                            let inline = parser.parse_inline_drawing(&attrs)?;
                            kind = DrawingKind::Inline(inline);
                        } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                            && name.local() == "anchor"
                        {
                            record_editor_ids(parser, "wp:anchor", &attrs);
                            let anchor = parser.parse_anchor(&attrs)?;
                            kind = DrawingKind::Anchor(anchor);
                        } else {
                            parser.record(
                                &super::feature_id_for(&name),
                                SupportStatus::Unsupported,
                                Some("drawing content is not supported".to_owned()),
                                Some(parser.location()),
                            );
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of drawing")),
                }
            }
            Ok(Drawing { kind, location })
        })
    }

    /// Parses `wp:inline`.
    fn parse_inline_drawing(&mut self, attrs: &[Attr]) -> Result<InlineDrawing> {
        let location = self.location();
        self.nested(|parser| {
            let mut inline = InlineDrawing {
                extent: None,
                effect_extent: None,
                doc_pr: None,
                dist_top: parse_u32_attr(attrs, "distT"),
                dist_bottom: parse_u32_attr(attrs, "distB"),
                dist_left: parse_u32_attr(attrs, "distL"),
                dist_right: parse_u32_attr(attrs, "distR"),
                graphic_uri: None,
                graphic: Box::new(Graphic::None),
                location,
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                            && name.local() == "extent"
                        {
                            inline.extent = Some(parser.parse_extent(&attrs));
                            parser.skip_element()?;
                        } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                            && name.local() == "docPr"
                        {
                            inline.doc_pr = Some(parser.parse_doc_pr(&attrs));
                            parser.skip_element()?;
                        } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                            && name.local() == "effectExtent"
                        {
                            inline.effect_extent = Some(parser.parse_effect_extent(&attrs));
                            parser.skip_element()?;
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphic" {
                            let (uri, graphic) = parser.parse_graphic()?;
                            inline.graphic_uri = uri;
                            inline.graphic = Box::new(graphic);
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of inline drawing")),
                }
            }
            Ok(inline)
        })
    }

    /// Parses `wp:anchor`.
    fn parse_anchor(&mut self, attrs: &[Attr]) -> Result<AnchorDrawing> {
        let location = self.location();
        self.nested(|parser| {
            let mut anchor = AnchorDrawing {
                extent: None,
                effect_extent: None,
                doc_pr: None,
                simple_pos: bool_attr(attrs, "simplePos"),
                position_h: None,
                position_v: None,
                wrap: None,
                behind_doc: bool_attr(attrs, "behindDoc"),
                relative_height: parse_u32_attr(attrs, "relativeHeight"),
                dist_top: parse_u32_attr(attrs, "distT"),
                dist_bottom: parse_u32_attr(attrs, "distB"),
                dist_left: parse_u32_attr(attrs, "distL"),
                dist_right: parse_u32_attr(attrs, "distR"),
                allow_overlap: !attr_is_false(attrs, "allowOverlap"),
                layout_in_cell: !attr_is_false(attrs, "layoutInCell"),
                locked: bool_attr(attrs, "locked"),
                graphic_uri: None,
                graphic: Box::new(Graphic::None),
                size_rel_h: None,
                size_rel_v: None,
                location,
            };
            parser.parse_anchor_children_into(&mut anchor)?;
            parser.record(
                "wp:anchor",
                SupportStatus::Supported,
                None,
                Some(anchor.location.clone()),
            );
            Ok(anchor)
        })
    }

    /// Fills an [`AnchorDrawing`] from its open element's children.
    ///
    /// `mc:AlternateContent` around `wp:positionV` (wp14 percent vs EMU Fallback)
    /// is resolved here — skipping the block used to drop both branches and leave
    /// the writer inventing `relativeFrom=paragraph` with a zero offset.
    fn parse_anchor_children_into(&mut self, anchor: &mut AnchorDrawing) -> Result<()> {
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) {
                        match name.local() {
                            "positionH" => {
                                anchor.position_h = Some(self.parse_position(&attrs)?);
                            }
                            "positionV" => {
                                anchor.position_v = Some(self.parse_position(&attrs)?);
                            }
                            "extent" => {
                                anchor.extent = Some(self.parse_extent(&attrs));
                                self.skip_element()?;
                            }
                            "effectExtent" => {
                                anchor.effect_extent = Some(self.parse_effect_extent(&attrs));
                                self.skip_element()?;
                            }
                            "docPr" => {
                                anchor.doc_pr = Some(self.parse_doc_pr(&attrs));
                                self.skip_element()?;
                            }
                            "wrapNone" => {
                                anchor.wrap = Some(self.parse_wrap(WrapKind::None, &attrs)?);
                            }
                            "wrapSquare" => {
                                anchor.wrap = Some(self.parse_wrap(WrapKind::Square, &attrs)?);
                            }
                            "wrapTight" => {
                                anchor.wrap = Some(self.parse_wrap(WrapKind::Tight, &attrs)?);
                            }
                            "wrapThrough" => {
                                anchor.wrap = Some(self.parse_wrap(WrapKind::Through, &attrs)?);
                            }
                            "wrapTopAndBottom" => {
                                anchor.wrap =
                                    Some(self.parse_wrap(WrapKind::TopAndBottom, &attrs)?);
                            }
                            _ => self.skip_element()?,
                        }
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphic" {
                        let (uri, graphic) = self.parse_graphic()?;
                        anchor.graphic_uri = uri;
                        anchor.graphic = Box::new(graphic);
                    } else if is_ns(&name, MS_WORD_PROCESSING_DRAWING_NS) {
                        match name.local() {
                            "sizeRelH" => {
                                anchor.size_rel_h = self.parse_relative_size(&attrs)?;
                            }
                            "sizeRelV" => {
                                anchor.size_rel_v = self.parse_relative_size(&attrs)?;
                            }
                            _ => self.skip_element()?,
                        }
                    } else if name
                        .ns
                        .as_ref()
                        .is_some_and(|ns| ns.as_str() == super::MCE_NS)
                        && name.local() == "AlternateContent"
                    {
                        self.parse_mce_alternate_content(&attrs, |parser, _| {
                            parser.parse_anchor_children_into(anchor)
                        })?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of anchor")),
            }
        }
        Ok(())
    }

    /// Parses `wp14:sizeRelH` / `wp14:sizeRelV`.
    fn parse_relative_size(&mut self, attrs: &[Attr]) -> Result<Option<RelativeSize>> {
        let relative_from = plain_attr(attrs, "relativeFrom").map(|value| self.intern(value));
        let mut percent = None;
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if name.local() == "pctWidth" || name.local() == "pctHeight" {
                            let text = parser.read_element_text()?;
                            percent = text.trim().parse::<u32>().ok();
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of relative size"));
                    }
                }
            }
            Ok(percent.map(|percent| RelativeSize {
                relative_from,
                percent,
            }))
        })
    }

    /// Parses `wp:positionH`/`wp:positionV`.
    fn parse_position(&mut self, attrs: &[Attr]) -> Result<Position> {
        let mut position = Position {
            relative_from: plain_attr(attrs, "relativeFrom").map(|value| self.intern(value)),
            align: None,
            offset: None,
            percent_offset: None,
        };
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) {
                            if name.local() == "posOffset" {
                                let text = parser.read_element_text()?;
                                position.offset = text.trim().parse::<i64>().ok().map(Emu);
                            } else if name.local() == "align" {
                                let text = parser.read_element_text()?;
                                let value = text.trim().to_owned();
                                if !value.is_empty() {
                                    position.align = Some(parser.intern(&value));
                                }
                            } else {
                                parser.skip_element()?;
                            }
                        } else if is_ns(&name, MS_WORD_PROCESSING_DRAWING_NS)
                            && (name.local() == "pctPosHOffset" || name.local() == "pctPosVOffset")
                        {
                            let text = parser.read_element_text()?;
                            position.percent_offset = text.trim().parse::<i32>().ok();
                        } else if name
                            .ns
                            .as_ref()
                            .is_some_and(|ns| ns.as_str() == super::MCE_NS)
                            && name.local() == "AlternateContent"
                        {
                            // Rare: AC wrapping only the offset child. Resolve so
                            // the Fallback `wp:posOffset` is not discarded.
                            parser.parse_mce_alternate_content(&[], |parser, _| {
                                loop {
                                    match parser.next_event()? {
                                        XmlEvent::StartElement { name, .. } => {
                                            if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                                                && name.local() == "posOffset"
                                            {
                                                let text = parser.read_element_text()?;
                                                position.offset =
                                                    text.trim().parse::<i64>().ok().map(Emu);
                                            } else if is_ns(&name, MS_WORD_PROCESSING_DRAWING_NS)
                                                && (name.local() == "pctPosHOffset"
                                                    || name.local() == "pctPosVOffset")
                                            {
                                                let text = parser.read_element_text()?;
                                                position.percent_offset =
                                                    text.trim().parse::<i32>().ok();
                                            } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                                                && name.local() == "align"
                                            {
                                                let text = parser.read_element_text()?;
                                                let value = text.trim().to_owned();
                                                if !value.is_empty() {
                                                    position.align = Some(parser.intern(&value));
                                                }
                                            } else {
                                                parser.skip_element()?;
                                            }
                                        }
                                        XmlEvent::EndElement { .. } => break,
                                        XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                                        XmlEvent::Eof => {
                                            return Err(parser.invalid(
                                                "unexpected end of position AlternateContent",
                                            ));
                                        }
                                    }
                                }
                                Ok(())
                            })?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of position")),
                }
            }
            Ok(position)
        })
    }

    /// Parses a `wp:wrap*` element.
    fn parse_wrap(&mut self, kind: WrapKind, attrs: &[Attr]) -> Result<Wrap> {
        // The polygon is read BEFORE the attribute struct is built and consumes
        // this element's own `End`, so there is no `skip_element` afterwards.
        // Reading it afterwards instead consumed the `wp:wrap*` closing tag and
        // desynchronised the anchor loop, which then read the rest of the part as
        // children of nothing - the document came out truncated and the writer
        // refused it.
        let (polygon, polygon_edited) = self.parse_wrap_polygon()?;
        let wrap = Wrap {
            kind,
            wrap_text: plain_attr(attrs, "wrapText").map(|value| self.intern(value)),
            dist_left: parse_u32_attr(attrs, "distL"),
            dist_right: parse_u32_attr(attrs, "distR"),
            dist_top: parse_u32_attr(attrs, "distT"),
            dist_bottom: parse_u32_attr(attrs, "distB"),
            polygon,
            polygon_edited,
        };
        Ok(wrap)
    }

    /// Reads `wp:wrapPolygon` inside a `wp:wrap*`, in document order.
    ///
    /// The contour is already EMU on this side of the pipeline: the producer that
    /// wrote it wrote DrawingML, not VML, and `wp:start`/`wp:lineTo` are both
    /// `ST_PositiveCoordinate`. Nothing is scaled here and that is the point - a
    /// contour that arrived in VML shape space is converted where the shape is, and
    /// a converter that scaled a second time would be the unit bug this comment is
    /// standing in front of.
    ///
    /// A polygon that does not satisfy `CT_WrapPath` (one `start`, at least two
    /// `lineTo`) is kept as read and left for the XSD gate to name. Dropping it
    /// would produce a conforming document and a silent loss, which is the one
    /// outcome this project treats as the defect.
    fn parse_wrap_polygon(&mut self) -> Result<(Vec<(i64, i64)>, Option<bool>)> {
        let mut points = Vec::new();
        let mut edited = None;
        let mut depth = 0usize;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wordprocessing_drawing(&name) {
                        depth += 1;
                        let local = name.local();
                        if local == "wrapPolygon" {
                            edited = optional_bool_attr(&attrs, "edited");
                        } else if local == "start" || local == "lineTo" {
                            let x = plain_attr(&attrs, "x").and_then(|v| v.trim().parse().ok());
                            let y = plain_attr(&attrs, "y").and_then(|v| v.trim().parse().ok());
                            if let (Some(x), Some(y)) = (x, y) {
                                points.push((x, y));
                            }
                        }
                    } else {
                        // Not wordprocessingDrawing: not part of a wrap contour,
                        // and the whole point of the depth counter is that the
                        // first `End` seen at depth 0 is THIS element's own.
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of wrap")),
            }
        }
        Ok((points, edited))
    }

    /// Parses `a:graphic`, returning `(uri, graphic)`.
    fn parse_graphic(&mut self) -> Result<(Option<std::sync::Arc<str>>, Graphic)> {
        self.nested(|parser| {
            let mut graphic_uri = None;
            let mut graphic = Graphic::None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphicData" {
                            let (uri, parsed) = parser.parse_graphic_data(&attrs)?;
                            graphic_uri = uri;
                            graphic = parsed;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of graphic")),
                }
            }
            Ok((graphic_uri, graphic))
        })
    }

    /// Parses `a:graphicData`, returning `(uri, graphic)`.
    fn parse_graphic_data(
        &mut self,
        attrs: &[Attr],
    ) -> Result<(Option<std::sync::Arc<str>>, Graphic)> {
        let uri = plain_attr(attrs, "uri").map(|value| self.intern(value));
        self.nested(|parser| {
            let mut graphic = Graphic::None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement {
                        name,
                        attrs: element,
                    } => {
                        if let Some(parsed) = parser.parse_graphic_payload(&name, &element)? {
                            graphic = parsed;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of graphic data")),
                }
            }
            if matches!(graphic, Graphic::None) {
                if let Some(uri) = uri.as_deref() {
                    if uri.contains("/chart") {
                        graphic = Graphic::Chart(ForeignRefs::default());
                    } else if uri.contains("/diagram") {
                        graphic = Graphic::Diagram(ForeignRefs::default());
                    }
                }
            }
            Ok((uri, graphic))
        })
    }

    /// One child of `a:graphicData`.
    ///
    /// `None` means the element was skipped and the graphic already parsed stays.
    /// A wordprocessing shape is handled here; pictures, groups, charts and
    /// diagrams are [`parse_graphic_payload_rest`](Self::parse_graphic_payload_rest)
    /// so their locals are not on the stack while a text box inside the shape is
    /// parsed.
    fn parse_graphic_payload(&mut self, name: &QName, attrs: &[Attr]) -> Result<Option<Graphic>> {
        if name.local() == "wsp" && is_shape_ns(name) {
            if !is_ns(name, WORD_PROCESSING_SHAPE_STRICT_NS) {
                self.record(
                    "wps:wsp",
                    SupportStatus::Partial,
                    Some("Microsoft/legacy shape namespace compatibility".to_owned()),
                    Some(self.location()),
                );
            }
            return Ok(Some(Graphic::Shape(self.parse_shape()?)));
        }
        self.parse_graphic_payload_rest(name, attrs)
    }

    /// Graphic-data children other than a wordprocessing shape.
    fn parse_graphic_payload_rest(
        &mut self,
        name: &QName,
        attrs: &[Attr],
    ) -> Result<Option<Graphic>> {
        if is_locked_canvas_ns(name) && name.local() == "lockedCanvas" {
            return Ok(Some(Graphic::LockedCanvas(
                self.capture_locked_canvas(name, attrs)?,
            )));
        }
        if is_ns(name, PICTURE_STRICT_NS) && name.local() == "pic" {
            return Ok(Some(Graphic::Picture(self.parse_picture()?)));
        }
        if matches!(name.local(), "wgp" | "grpSp") && is_group_ns(name) {
            if !is_ns(name, WORD_PROCESSING_GROUP_STRICT_NS) {
                self.record(
                    "wpg:wgp",
                    SupportStatus::Partial,
                    Some("Microsoft/legacy group namespace compatibility".to_owned()),
                    Some(self.location()),
                );
            }
            return Ok(Some(Graphic::Group(self.parse_group()?)));
        }
        if name.local() == "chart" {
            // The attributes of *this* element, not of the `a:graphicData` that
            // carries it: `r:id` is where the chart part is named.
            let graphic = Graphic::Chart(self.foreign_refs(attrs, &[R_ID]));
            self.skip_element()?;
            return Ok(Some(graphic));
        }
        if name.local() == "relIds" {
            // `dgm:relIds` carries four ids in a fixed order, and the order is
            // the only thing that says which is which.
            let graphic =
                Graphic::Diagram(self.foreign_refs(attrs, &[REL_DM, REL_LO, REL_QS, REL_CS]));
            self.skip_element()?;
            return Ok(Some(graphic));
        }
        if is_ns(name, CHART_STRICT_NS) || is_ns(name, DIAGRAM_STRICT_NS) {
            self.skip_element()?;
            return Ok(Some(Graphic::Other));
        }
        self.skip_element()?;
        Ok(None)
    }

    /// Captures `lc:lockedCanvas` as Strict markup so `a:off`/`a:ext`/`chOff`
    /// round-trip (audit P2). The start event is already consumed.
    fn capture_locked_canvas(&mut self, name: &QName, attrs: &[Attr]) -> Result<LockedCanvas> {
        let location = self.location();
        let mut markup = String::new();
        write_start_markup(&mut markup, name, attrs);
        let mut depth = 1u32;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    depth = depth.saturating_add(1);
                    write_start_markup(&mut markup, &name, &attrs);
                }
                XmlEvent::EndElement { name } => {
                    write_end_markup(&mut markup, &name);
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                XmlEvent::Text(text) => {
                    escape_text_into_markup(&mut markup, &text);
                }
                XmlEvent::CData(text) => {
                    markup.push_str("<![CDATA[");
                    markup.push_str(&text);
                    markup.push_str("]]>");
                }
                XmlEvent::Eof => {
                    return Err(self.invalid("unexpected end of locked canvas"));
                }
            }
        }
        self.record(
            "lc:lockedCanvas",
            SupportStatus::Supported,
            None,
            Some(location.clone()),
        );
        Ok(LockedCanvas {
            markup: std::sync::Arc::<str>::from(markup),
            location,
        })
    }

    /// Captures the relationship ids a foreign graphic element carries.
    ///
    /// `names` is the fixed attribute order the element uses, so the ids come
    /// out in the order a writer has to write them back in. Only the Strict
    /// relationships namespace is read, exactly as `a:blip/@r:embed` is read: the
    /// normalizer has already rewritten a Transitional `r:` by the time a part is
    /// parsed. A missing attribute is skipped rather than defaulted — `dgm:relIds`
    /// without `r:lo` is a producer's bug, and a writer that invented an id
    /// would point at nothing.
    fn foreign_refs(&mut self, attrs: &[Attr], names: &[&str]) -> ForeignRefs {
        let mut rels = Vec::new();
        for name in names {
            if let Some(value) = attr_in_ns(attrs, RELS_STRICT_NS, name) {
                let value = value.to_owned();
                rels.push(self.intern(&value));
            }
        }
        ForeignRefs {
            rels,
            location: self.location(),
        }
    }

    /// Parses `pic:pic`.
    fn parse_picture(&mut self) -> Result<Picture> {
        self.nested(|parser| {
            let mut picture = Picture {
                name: None,
                descr: None,
                blip: None,
                extent: None,
                src_rect: None,
                xfrm: None,
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "nvPicPr" {
                            let (name, descr) = parser.parse_nv_pic_pr()?;
                            picture.name = name;
                            picture.descr = descr;
                        } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "blipFill" {
                            let (blip, src_rect) = parser.parse_blip_fill()?;
                            picture.blip = blip;
                            picture.src_rect = src_rect;
                        } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "spPr" {
                            let (extent, xfrm) = parser.parse_sp_pr()?;
                            picture.extent = extent;
                            picture.xfrm = xfrm;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of picture")),
                }
            }
            Ok(picture)
        })
    }

    /// Parses `pic:nvPicPr`.
    fn parse_nv_pic_pr(
        &mut self,
    ) -> Result<(Option<std::sync::Arc<str>>, Option<std::sync::Arc<str>>)> {
        self.nested(|parser| {
            let mut name = None;
            let mut descr = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement {
                        name: element,
                        attrs,
                    } => {
                        if is_ns(&element, PICTURE_STRICT_NS) && element.local() == "cNvPr" {
                            name = plain_attr(&attrs, "name").map(|value| parser.intern(value));
                            descr = plain_attr(&attrs, "descr").map(|value| parser.intern(value));
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of nvPicPr")),
                }
            }
            Ok((name, descr))
        })
    }

    /// Parses `pic:blipFill`, resolving the image reference and crop.
    fn parse_blip_fill(&mut self) -> Result<(Option<BlipRef>, Option<SrcRect>)> {
        self.nested(|parser| {
            let mut blip = None;
            let mut src_rect = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "blip" {
                            let embed = attr_in_ns(&attrs, RELS_STRICT_NS, "embed").map(RelId::new);
                            let link = attr_in_ns(&attrs, RELS_STRICT_NS, "link").map(RelId::new);
                            let resolved = embed
                                .as_ref()
                                .and_then(|id| parser.resolve_relationship_target(id.as_str()));
                            if let Some(part) = &resolved {
                                let content_type = parser.content_type(part);
                                let kind = content_type.as_deref().map_or_else(
                                    || {
                                        let extension = part
                                            .as_str()
                                            .rsplit_once('.')
                                            .map_or("", |(_, ext)| ext);
                                        MediaKind::from_extension(extension)
                                    },
                                    MediaKind::from_content_type,
                                );
                                parser.media.insert(MediaItem {
                                    part: part.clone(),
                                    content_type,
                                    kind,
                                });
                            }
                            blip = Some(BlipRef {
                                embed,
                                link,
                                resolved,
                                location: parser.location(),
                            });
                            parser.skip_element()?;
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "srcRect" {
                            src_rect = Some(parse_src_rect(&attrs));
                            parser.skip_element()?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of blipFill")),
                }
            }
            Ok((blip, src_rect))
        })
    }

    /// Parses `pic:spPr`, extracting the extent and transform.
    fn parse_sp_pr(&mut self) -> Result<(Option<Extent>, Option<Xfrm>)> {
        self.nested(|parser| {
            let mut extent = None;
            let mut xfrm = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "ext" {
                            extent = Some(parser.parse_extent(&attrs));
                            parser.skip_element()?;
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "xfrm" {
                            let parts = parser.parse_xfrm_parts(&attrs)?;
                            if parts.extent.is_some() {
                                extent = parts.extent;
                            }
                            xfrm = Some(Xfrm {
                                offset: parts.offset,
                                rot: parts.rot,
                                flip_h: parts.flip_h,
                                flip_v: parts.flip_v,
                            });
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of shape properties"))
                    }
                }
            }
            Ok((extent, xfrm))
        })
    }

    /// Parses a `wps:wsp` shape.
    fn parse_shape(&mut self) -> Result<Shape> {
        let location = self.location();
        self.nested(|parser| {
            let mut shape = Shape {
                name: None,
                descr: None,
                tx_box: None,
                geometry: ShapeGeometry::None,
                xfrm: None,
                offset: None,
                extent: None,
                fill: None,
                stroke: None,
                text: None,
                style: None,
                location,
            };
            let mut body = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_shape_ns(&name) {
                            match name.local() {
                                "cNvPr" => {
                                    shape.name =
                                        plain_attr(&attrs, "name").map(|v| parser.intern(v));
                                    shape.descr =
                                        plain_attr(&attrs, "descr").map(|v| parser.intern(v));
                                    parser.skip_element()?;
                                }
                                "cNvSpPr" => {
                                    shape.tx_box = optional_bool_attr(&attrs, "txBox");
                                    parser.skip_element()?;
                                }
                                "spPr" => parser.parse_shape_sp_pr(&mut shape)?,
                                "style" => shape.style = Some(parser.parse_shape_style()?),
                                "txbx" => shape.text = Some(parser.parse_text_box()?),
                                "bodyPr" => body = Some(parser.parse_text_box_body(&attrs)?),
                                _ => parser.skip_element()?,
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of shape")),
                }
            }
            if let Some(body) = body {
                if let Some(text) = &mut shape.text {
                    text.body = Some(body);
                } else {
                    shape.text = Some(TextBox {
                        body: Some(body),
                        blocks: Vec::new(),
                        location: shape.location.clone(),
                    });
                }
            }
            parser.record(
                "wps:wsp",
                SupportStatus::Supported,
                None,
                Some(shape.location.clone()),
            );
            Ok(shape)
        })
    }

    /// Parses `wps:spPr` into `shape`.
    fn parse_shape_sp_pr(&mut self, shape: &mut Shape) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) {
                            match name.local() {
                                "xfrm" => {
                                    let parts = parser.parse_xfrm_parts(&attrs)?;
                                    shape.offset = parts.offset;
                                    shape.extent = parts.extent;
                                    shape.xfrm = Some(Xfrm {
                                        offset: parts.offset,
                                        rot: parts.rot,
                                        flip_h: parts.flip_h,
                                        flip_v: parts.flip_v,
                                    });
                                }
                                "prstGeom" => {
                                    let preset = plain_attr(&attrs, "prst")
                                        .map_or_else(|| "rect".to_owned(), str::to_owned);
                                    shape.geometry = ShapeGeometry::Preset(parser.intern(&preset));
                                    parser.skip_element()?;
                                }
                                "custGeom" => {
                                    shape.geometry =
                                        ShapeGeometry::Custom(parser.parse_cust_geom()?);
                                }
                                "solidFill" | "gradFill" | "pattFill" | "noFill" | "blipFill"
                                | "grpFill" => {
                                    if shape.fill.is_none() {
                                        shape.fill = parser.parse_shape_fill(&name, &attrs)?;
                                    } else {
                                        parser.skip_element()?;
                                    }
                                }
                                "ln" => shape.stroke = Some(parser.parse_shape_stroke(&attrs)?),
                                _ => parser.skip_element()?,
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of shape properties"))
                    }
                }
            }
            Ok(())
        })
    }

    /// Parses `a:xfrm` attributes and children.
    fn parse_xfrm_parts(&mut self, attrs: &[Attr]) -> Result<XfrmParts> {
        let mut parts = XfrmParts {
            rot: parse_i32_attr(attrs, "rot"),
            flip_h: bool_attr(attrs, "flipH"),
            flip_v: bool_attr(attrs, "flipV"),
            ..XfrmParts::default()
        };
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "off" {
                            parts.offset = Some(parser.parse_offset(&attrs));
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "ext" {
                            parts.extent = Some(parser.parse_extent(&attrs));
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "chOff" {
                            parts.child_offset = Some(parser.parse_offset(&attrs));
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "chExt" {
                            parts.child_extent = Some(parser.parse_extent(&attrs));
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of transform")),
                }
            }
            Ok(parts)
        })
    }

    /// Parses `a:custGeom` (a subset: moveTo/lnTo/cubicBezTo/close).
    fn parse_cust_geom(&mut self) -> Result<CustomGeometry> {
        self.nested(|parser| {
            let mut geometry = CustomGeometry::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "pathLst" {
                            parser.parse_path_list(&mut geometry)?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of custom geometry"))
                    }
                }
            }
            parser.record(
                "a:custGeom",
                SupportStatus::Partial,
                Some("custom geometry supports moveTo/lnTo/cubicBezTo/close only".to_owned()),
                Some(parser.location()),
            );
            Ok(geometry)
        })
    }

    /// Parses `a:pathLst` into one [`GeometryPath`] per `a:path` (AUD-49).
    fn parse_path_list(&mut self, geometry: &mut CustomGeometry) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "path" {
                            let mut path = GeometryPath {
                                width: plain_attr(&attrs, "w")
                                    .and_then(|v| v.trim().parse().ok())
                                    .unwrap_or(0),
                                height: plain_attr(&attrs, "h")
                                    .and_then(|v| v.trim().parse().ok())
                                    .unwrap_or(0),
                                fill: plain_attr(&attrs, "fill").map(|v| parser.intern(v)),
                                stroke: plain_attr(&attrs, "stroke")
                                    .map(|v| matches!(v, "true" | "1" | "on" | "t")),
                                commands: Vec::new(),
                            };
                            parser.parse_path(&mut path)?;
                            geometry.paths.push(path);
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of path list")),
                }
            }
            Ok(())
        })
    }

    /// Parses one `a:path` into `path.commands`.
    fn parse_path(&mut self, path: &mut GeometryPath) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if !is_ns(&name, DRAWINGML_STRICT_NS) {
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "moveTo" => {
                                let point = parser.parse_first_point()?;
                                path.commands.push(PathCommand::MoveTo {
                                    x: point.0,
                                    y: point.1,
                                });
                            }
                            "lnTo" => {
                                let point = parser.parse_first_point()?;
                                path.commands.push(PathCommand::LineTo {
                                    x: point.0,
                                    y: point.1,
                                });
                            }
                            "cubicBezTo" => {
                                let points = parser.parse_points()?;
                                if points.len() >= 3 {
                                    path.commands.push(PathCommand::CubicBezTo {
                                        x1: points[0].0,
                                        y1: points[0].1,
                                        x2: points[1].0,
                                        y2: points[1].1,
                                        x: points[2].0,
                                        y: points[2].1,
                                    });
                                }
                            }
                            "close" => {
                                path.commands.push(PathCommand::Close);
                                parser.skip_element()?;
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of path")),
                }
            }
            Ok(())
        })
    }

    /// Parses the first `a:pt` of the current element.
    fn parse_first_point(&mut self) -> Result<(i64, i64)> {
        let points = self.parse_points()?;
        Ok(points.first().copied().unwrap_or((0, 0)))
    }

    /// Parses all `a:pt` children of the current element.
    fn parse_points(&mut self) -> Result<Vec<(i64, i64)>> {
        self.nested(|parser| {
            let mut points = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "pt" {
                            let x = plain_attr(&attrs, "x").and_then(|v| v.trim().parse().ok());
                            let y = plain_attr(&attrs, "y").and_then(|v| v.trim().parse().ok());
                            if let (Some(x), Some(y)) = (x, y) {
                                points.push((x, y));
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of point list")),
                }
            }
            Ok(points)
        })
    }

    /// Parses a fill element.
    fn parse_shape_fill(&mut self, name: &QName, attrs: &[Attr]) -> Result<Option<ShapeFill>> {
        match name.local() {
            "noFill" => {
                self.skip_element()?;
                Ok(Some(ShapeFill::None))
            }
            "solidFill" => {
                let color = self.parse_fill_color()?;
                Ok(Some(ShapeFill::Solid {
                    color: color.unwrap_or_default(),
                }))
            }
            "gradFill" => Ok(Some(self.parse_grad_fill()?)),
            "pattFill" => {
                let preset = plain_attr(attrs, "prst").map(|v| self.intern(v));
                self.nested(|parser| {
                    let mut foreground = None;
                    let mut background = None;
                    loop {
                        match parser.next_event()? {
                            XmlEvent::StartElement { name, attrs: _ } => {
                                if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "fgClr" {
                                    foreground = parser.parse_child_color()?;
                                } else if is_ns(&name, DRAWINGML_STRICT_NS)
                                    && name.local() == "bgClr"
                                {
                                    background = parser.parse_child_color()?;
                                } else {
                                    parser.skip_element()?;
                                }
                            }
                            XmlEvent::EndElement { .. } => break,
                            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                            XmlEvent::Eof => {
                                return Err(parser.invalid("unexpected end of pattern fill"));
                            }
                        }
                    }
                    Ok(Some(ShapeFill::Pattern {
                        preset,
                        foreground,
                        background,
                    }))
                })
            }
            _ => {
                self.skip_element()?;
                self.record(
                    "a:fill",
                    SupportStatus::Partial,
                    Some("unsupported fill kind preserved as no fill".to_owned()),
                    Some(self.location()),
                );
                Ok(Some(ShapeFill::None))
            }
        }
    }

    /// Parses the first colour child of `a:solidFill`-like containers.
    fn parse_fill_color(&mut self) -> Result<Option<ShapeColor>> {
        self.nested(|parser| {
            let mut color = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if color.is_none() && is_ns(&name, DRAWINGML_STRICT_NS) {
                            color = parser.parse_shape_color(&name, &attrs)?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of fill")),
                }
            }
            Ok(color)
        })
    }

    /// Parses one colour element child of a fill wrapper.
    fn parse_child_color(&mut self) -> Result<Option<ShapeColor>> {
        self.nested(|parser| {
            let mut color = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if color.is_none() && is_ns(&name, DRAWINGML_STRICT_NS) {
                            color = parser.parse_shape_color(&name, &attrs)?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of colour")),
                }
            }
            Ok(color)
        })
    }

    /// Parses an `a:srgbClr`/`a:schemeClr` colour element.
    fn parse_shape_color(&mut self, name: &QName, attrs: &[Attr]) -> Result<Option<ShapeColor>> {
        match name.local() {
            "srgbClr" => {
                let value = plain_attr(attrs, "val").map(Color::new);
                self.skip_element()?;
                Ok(value.map(|value| ShapeColor {
                    value: Some(value),
                    theme: None,
                }))
            }
            "schemeClr" => {
                let slot = plain_attr(attrs, "val").map(str::to_owned);
                self.nested(|parser| {
                    let mut tint = None;
                    let mut shade = None;
                    loop {
                        match parser.next_event()? {
                            XmlEvent::StartElement { name, attrs } => {
                                if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "tint" {
                                    tint = plain_attr(&attrs, "val").map(|v| parser.intern(v));
                                } else if is_ns(&name, DRAWINGML_STRICT_NS)
                                    && name.local() == "shade"
                                {
                                    shade = plain_attr(&attrs, "val").map(|v| parser.intern(v));
                                }
                                parser.skip_element()?;
                            }
                            XmlEvent::EndElement { .. } => break,
                            XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                            XmlEvent::Eof => {
                                return Err(parser.invalid("unexpected end of scheme colour"));
                            }
                        }
                    }
                    Ok(slot.map(|slot| ShapeColor {
                        value: None,
                        theme: Some(ThemeColorRef {
                            color: ThemeColor::new(slot),
                            tint,
                            shade,
                        }),
                    }))
                })
            }
            "prstClr" | "sysClr" | "scrgbClr" | "hslClr" => {
                self.skip_element()?;
                self.record(
                    "a:color",
                    SupportStatus::Partial,
                    Some("colour space not resolved".to_owned()),
                    Some(self.location()),
                );
                Ok(None)
            }
            _ => {
                self.skip_element()?;
                Ok(None)
            }
        }
    }

    /// Parses `a:gradFill`.
    fn parse_grad_fill(&mut self) -> Result<ShapeFill> {
        let mut stops = Vec::new();
        let mut angle = None;
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "gsLst" {
                            parser.parse_gradient_stops(&mut stops)?;
                        } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "lin" {
                            angle = parse_i32_attr(&attrs, "ang");
                            parser.skip_element()?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of gradient fill")),
                }
            }
            Ok(ShapeFill::Gradient { stops, angle })
        })
    }

    /// Parses `a:gsLst`.
    fn parse_gradient_stops(&mut self, stops: &mut Vec<GradientStop>) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "gs" {
                            let position = parse_i32_attr(&attrs, "pos").unwrap_or(0);
                            let color = parser.parse_fill_color()?.unwrap_or_default();
                            stops.push(GradientStop { position, color });
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of gradient stops")),
                }
            }
            Ok(())
        })
    }

    /// Parses `a:ln`.
    fn parse_shape_stroke(&mut self, attrs: &[Attr]) -> Result<ShapeStroke> {
        let mut stroke = ShapeStroke {
            width: plain_attr(attrs, "w")
                .and_then(|v| v.trim().parse::<i64>().ok())
                .map(Emu),
            ..ShapeStroke::default()
        };
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) {
                            match name.local() {
                                "noFill" => {
                                    stroke.none = true;
                                    parser.skip_element()?;
                                }
                                "solidFill" | "gradFill" | "pattFill" => {
                                    if let Some(fill) = parser.parse_shape_fill(&name, &attrs)? {
                                        stroke.color = fill_color(&fill);
                                    }
                                }
                                "prstDash" => {
                                    stroke.dash =
                                        plain_attr(&attrs, "val").map(|v| parser.intern(v));
                                    parser.skip_element()?;
                                }
                                "headEnd" => {
                                    stroke.head_end =
                                        plain_attr(&attrs, "type").map(|v| parser.intern(v));
                                    parser.skip_element()?;
                                }
                                "tailEnd" => {
                                    stroke.tail_end =
                                        plain_attr(&attrs, "type").map(|v| parser.intern(v));
                                    parser.skip_element()?;
                                }
                                _ => parser.skip_element()?,
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of outline")),
                }
            }
            Ok(stroke)
        })
    }

    /// Parses `wps:style`, including each reference's `a:schemeClr/@val`.
    fn parse_shape_style(&mut self) -> Result<ShapeStyle> {
        self.nested(|parser| {
            let mut style = ShapeStyle::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) {
                            let index = plain_attr(&attrs, "idx").map(|value| parser.intern(value));
                            let scheme = parser.scheme_clr_under_ref()?;
                            match name.local() {
                                "lnRef" => {
                                    style.line_ref = index;
                                    style.line_ref_color = scheme;
                                }
                                "fillRef" => {
                                    style.fill_ref = index;
                                    style.fill_ref_color = scheme;
                                }
                                "effectRef" => {
                                    style.effect_ref = index;
                                    style.effect_ref_color = scheme;
                                }
                                "fontRef" => {
                                    style.font_ref = index;
                                    style.font_ref_color = scheme;
                                }
                                _ => {
                                    parser.skip_element()?;
                                }
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of shape style")),
                }
            }
            Ok(style)
        })
    }

    /// Consumes a style reference element and returns its first `a:schemeClr/@val`.
    fn scheme_clr_under_ref(&mut self) -> Result<Option<std::sync::Arc<str>>> {
        let mut scheme = None;
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS)
                            && name.local() == "schemeClr"
                            && scheme.is_none()
                        {
                            scheme = plain_attr(&attrs, "val").map(|value| parser.intern(value));
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of style colour reference"))
                    }
                }
            }
            Ok(())
        })?;
        Ok(scheme)
    }

    /// Parses `wps:txbx`.
    fn parse_text_box(&mut self) -> Result<TextBox> {
        let location = self.location();
        self.nested(|parser| {
            let mut text = TextBox {
                body: None,
                blocks: Vec::new(),
                location,
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if name.local() == "txbxContent"
                            && (is_wml_name(&name) || is_ns(&name, MS_WORD_2006_WML_NS))
                        {
                            // One level of text-box nesting, against its own budget. Not one
                            // block level: `wps:txbx` is the DrawingML wrapper and
                            // `w:txbxContent` the block container inside it, and a
                            // text box is ten frames of parser state where a table is
                            // one - so sharing the number would make twelve tables
                            // unreachable.
                            let location = parser.location();
                            if let Some(blocks) =
                                parser.nested_text_box(PartParser::parse_block_children)?
                            {
                                text.blocks = blocks;
                                parser.record(
                                    "w:txbxContent",
                                    SupportStatus::Supported,
                                    None,
                                    Some(location),
                                );
                            } else {
                                let limit = parser.max_text_box_nesting;
                                parser.record(
                                    "w:txbxContent",
                                    SupportStatus::Unsupported,
                                    Some(format!(
                                        "text box nested past max_text_box_nesting ({limit}); its content was skipped"
                                    )),
                                    Some(location),
                                );
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of text box")),
                }
            }
            Ok(text)
        })
    }

    /// Parses `wps:bodyPr`.
    fn parse_text_box_body(&mut self, attrs: &[Attr]) -> Result<TextBoxBody> {
        let anchor = match plain_attr(attrs, "anchor") {
            Some("t") => Some(TextAnchor::Top),
            Some("ctr") => Some(TextAnchor::Center),
            Some("b") => Some(TextAnchor::Bottom),
            _ => None,
        };
        let body = TextBoxBody {
            anchor,
            anchor_centered: optional_bool_attr(attrs, "anchorCtr"),
            left_inset: plain_attr(attrs, "lIns").and_then(parse_emu_attr),
            top_inset: plain_attr(attrs, "tIns").and_then(parse_emu_attr),
            right_inset: plain_attr(attrs, "rIns").and_then(parse_emu_attr),
            bottom_inset: plain_attr(attrs, "bIns").and_then(parse_emu_attr),
            wrap: plain_attr(attrs, "wrap").map(|value| self.intern(value)),
            vert: plain_attr(attrs, "vert").map(|value| self.intern(value)),
            rot: parse_i32_attr(attrs, "rot"),
            upright: optional_bool_attr(attrs, "upright"),
            rtl_col: optional_bool_attr(attrs, "rtlCol"),
            compat_ln_spc: optional_bool_attr(attrs, "compatLnSpc"),
            force_aa: optional_bool_attr(attrs, "forceAA"),
            from_word_art: optional_bool_attr(attrs, "fromWordArt"),
            horz_overflow: plain_attr(attrs, "horzOverflow").map(|value| self.intern(value)),
            vert_overflow: plain_attr(attrs, "vertOverflow").map(|value| self.intern(value)),
            num_col: parse_i32_attr(attrs, "numCol"),
            spc_col: parse_i32_attr(attrs, "spcCol"),
            spc_first_last_para: optional_bool_attr(attrs, "spcFirstLastPara"),
        };
        self.skip_element()?;
        Ok(body)
    }

    /// Parses a `wpg:wgp` group.
    fn parse_group(&mut self) -> Result<GroupShape> {
        let location = self.location();
        self.nested(|parser| {
            let mut group = GroupShape {
                name: None,
                descr: None,
                xfrm: None,
                children: Vec::new(),
                location,
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if matches!(name.local(), "wgp" | "grpSp") && is_group_ns(&name) {
                            // Nested groups must be matched before the generic
                            // `is_group_ns` property skip: `grpSp` is itself a
                            // group-ns element and used to fall into `_ => skip`.
                            group.children.push(Graphic::Group(parser.parse_group()?));
                        } else if name.local() == "wsp" && is_shape_ns(&name) {
                            group.children.push(Graphic::Shape(parser.parse_shape()?));
                        } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "pic" {
                            group
                                .children
                                .push(Graphic::Picture(parser.parse_picture()?));
                        } else if is_group_ns(&name) {
                            match name.local() {
                                "cNvPr" => {
                                    group.name =
                                        plain_attr(&attrs, "name").map(|v| parser.intern(v));
                                    group.descr =
                                        plain_attr(&attrs, "descr").map(|v| parser.intern(v));
                                    parser.skip_element()?;
                                }
                                "grpSpPr" => {
                                    group.xfrm = Some(parser.parse_group_transform(&attrs)?);
                                }
                                _ => parser.skip_element()?,
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of group")),
                }
            }
            parser.record(
                "wpg:wgp",
                SupportStatus::Supported,
                None,
                Some(group.location.clone()),
            );
            Ok(group)
        })
    }

    /// Parses `wpg:grpSpPr` (start consumed), reading its `a:xfrm` child.
    fn parse_group_transform(&mut self, _attrs: &[Attr]) -> Result<GroupTransform> {
        self.nested(|parser| {
            let mut parts = XfrmParts::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "xfrm" {
                            parts = parser.parse_xfrm_parts(&attrs)?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of group transform"))
                    }
                }
            }
            Ok(GroupTransform {
                offset: parts.offset,
                extent: parts.extent,
                child_offset: parts.child_offset,
                child_extent: parts.child_extent,
                rot: parts.rot,
                flip_h: parts.flip_h,
                flip_v: parts.flip_v,
            })
        })
    }

    /// Reads the text content of the current element (start consumed).
    fn read_element_text(&mut self) -> Result<String> {
        self.nested(|parser| {
            let mut out = String::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::Text(text) | XmlEvent::CData(text) => out.push_str(text.as_ref()),
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of text content")),
                }
            }
            Ok(out)
        })
    }

    /// Parses `wp:docPr` attributes.
    fn parse_doc_pr(&mut self, attrs: &[Attr]) -> DocPr {
        DocPr {
            id: plain_attr(attrs, "id").and_then(|value| value.trim().parse().ok()),
            name: plain_attr(attrs, "name").map(|value| self.intern(value)),
            descr: plain_attr(attrs, "descr").map(|value| self.intern(value)),
            title: plain_attr(attrs, "title").map(|value| self.intern(value)),
        }
    }
}

/// An `a:xfrm` part set.
#[derive(Default)]
struct XfrmParts {
    offset: Option<(Emu, Emu)>,
    extent: Option<Extent>,
    child_offset: Option<(Emu, Emu)>,
    child_extent: Option<Extent>,
    rot: Option<i32>,
    flip_h: bool,
    flip_v: bool,
}

/// Returns `true` if a name belongs to `namespace`.
fn is_ns(name: &QName, namespace: &str) -> bool {
    name.ns.as_ref().is_some_and(|ns| ns == namespace)
}

/// Returns `true` for locked-canvas elements in Strict or Transitional form.
fn is_locked_canvas_ns(name: &QName) -> bool {
    is_ns(name, LOCKED_CANVAS_STRICT_NS) || is_ns(name, LOCKED_CANVAS_TRANSITIONAL_NS)
}

/// Writes a start tag with Strict namespace prefixes for locked-canvas capture.
fn write_start_markup(out: &mut String, name: &QName, attrs: &[Attr]) {
    let prefix = markup_prefix(name.ns.as_ref().map(|ns| ns.as_str()));
    out.push('<');
    if let Some(prefix) = prefix {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(name.local());
    if name.local() == "lockedCanvas" {
        // Declare the vocabularies the canvas subtree uses so the fragment is
        // well-formed when re-emitted inside `a:graphicData`.
        out.push_str(" xmlns:lc=\"");
        out.push_str(LOCKED_CANVAS_STRICT_NS);
        out.push('"');
        out.push_str(" xmlns:a=\"");
        out.push_str(DRAWINGML_STRICT_NS);
        out.push('"');
        out.push_str(" xmlns:r=\"");
        out.push_str(RELS_STRICT_NS);
        out.push('"');
    }
    for attr in attrs {
        let attr_prefix = markup_prefix(attr.name.ns.as_ref().map(|ns| ns.as_str()));
        out.push(' ');
        if let Some(prefix) = attr_prefix {
            // Unprefixed attributes stay unprefixed (XML Namespaces).
            if attr.name.ns.is_some() {
                out.push_str(prefix);
                out.push(':');
            }
        }
        out.push_str(attr.name.local());
        out.push_str("=\"");
        let value = match attr.name.local() {
            "uri" => strict_ooxml_core::ns::registry::strict_form(&attr.value)
                .unwrap_or(attr.value.as_str()),
            _ => attr.value.as_str(),
        };
        let _ = strict_ooxml_core::xml::escape::escape_attr_into(out, value);
        out.push('"');
    }
    out.push('>');
}

/// Writes an end tag with the same prefix policy as [`write_start_markup`].
fn write_end_markup(out: &mut String, name: &QName) {
    let prefix = markup_prefix(name.ns.as_ref().map(|ns| ns.as_str()));
    out.push_str("</");
    if let Some(prefix) = prefix {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(name.local());
    out.push('>');
}

fn escape_text_into_markup(out: &mut String, text: &str) {
    let _ = strict_ooxml_core::xml::escape::escape_text_into(out, text);
}

/// Stable prefix for a namespace URI when serialising locked-canvas markup.
fn markup_prefix(ns: Option<&str>) -> Option<&'static str> {
    match ns {
        Some(LOCKED_CANVAS_STRICT_NS) | Some(LOCKED_CANVAS_TRANSITIONAL_NS) => Some("lc"),
        Some(DRAWINGML_STRICT_NS)
        | Some("http://schemas.openxmlformats.org/drawingml/2006/main") => Some("a"),
        Some(RELS_STRICT_NS)
        | Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships") => {
            Some("r")
        }
        Some(PICTURE_STRICT_NS)
        | Some("http://schemas.openxmlformats.org/drawingml/2006/picture") => Some("pic"),
        Some("http://www.w3.org/XML/1998/namespace") => Some("xml"),
        _ => None,
    }
}

/// Returns `true` for shape elements in the Strict, Microsoft or legacy
/// wordprocessingDrawing namespace.
fn is_shape_ns(name: &QName) -> bool {
    is_ns(name, WORD_PROCESSING_SHAPE_STRICT_NS)
        || is_ns(name, MS_WORD_PROCESSING_SHAPE_NS)
        || is_ns(name, WORDPROCESSING_DRAWING_STRICT_NS)
}

/// Returns `true` for group elements in the Strict, Microsoft or legacy
/// wordprocessingDrawing namespace.
fn is_group_ns(name: &QName) -> bool {
    is_ns(name, WORD_PROCESSING_GROUP_STRICT_NS)
        || is_ns(name, MS_WORD_PROCESSING_GROUP_NS)
        || is_ns(name, WORDPROCESSING_DRAWING_STRICT_NS)
}

/// Returns `true` if a name is in the WML main namespace.
fn is_wml_name(name: &QName) -> bool {
    is_ns(name, crate::WML_STRICT_NS)
}

/// Parses an `a:srcRect` crop.
fn parse_src_rect(attrs: &[Attr]) -> SrcRect {
    SrcRect {
        left: parse_src_component(attrs, "l"),
        top: parse_src_component(attrs, "t"),
        right: parse_src_component(attrs, "r"),
        bottom: parse_src_component(attrs, "b"),
    }
}

/// Reads one `a:srcRect` edge. A bare integer is thousandths of a percent.
/// A percentage (`1.253%`, `0%`, `0.010%`) is the Strict spelling of the same unit.
fn parse_src_component(attrs: &[Attr], local: &str) -> i32 {
    plain_attr(attrs, local)
        .and_then(src_component_value)
        .unwrap_or(0)
}

fn src_component_value(raw: &str) -> Option<i32> {
    let text = raw.trim();
    if let Some(number) = text.strip_suffix('%') {
        return percent_to_thousandths(number.trim());
    }
    text.parse().ok()
}

fn percent_to_thousandths(text: &str) -> Option<i32> {
    let negative = text.starts_with('-');
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let whole = whole.parse::<i32>().ok()?;
    let mut padded = fraction.to_owned();
    while padded.len() < 3 {
        padded.push('0');
    }
    let fraction = padded.parse::<i32>().ok()?;
    let magnitude = whole.checked_mul(1000)?.checked_add(fraction)?;
    Some(if negative { -magnitude } else { magnitude })
}

impl PartParser<'_> {
    /// Parses an extent from `cx`/`cy` attributes (AUD-50 records invalid EMUs).
    fn parse_extent(&mut self, attrs: &[Attr]) -> Extent {
        Extent {
            cx: Emu(self.parse_emu(attrs, "cx")),
            cy: Emu(self.parse_emu(attrs, "cy")),
        }
    }

    /// Parses an offset from `x`/`y` attributes.
    fn parse_offset(&mut self, attrs: &[Attr]) -> (Emu, Emu) {
        (
            Emu(self.parse_emu(attrs, "x")),
            Emu(self.parse_emu(attrs, "y")),
        )
    }

    /// Parses an `wp:effectExtent`.
    fn parse_effect_extent(&mut self, attrs: &[Attr]) -> EffectExtent {
        EffectExtent {
            left: Emu(self.parse_emu(attrs, "l")),
            top: Emu(self.parse_emu(attrs, "t")),
            right: Emu(self.parse_emu(attrs, "r")),
            bottom: Emu(self.parse_emu(attrs, "b")),
        }
    }

    /// Parses an EMU attribute; missing/invalid values become `0` with a `partial` record.
    fn parse_emu(&mut self, attrs: &[Attr], local: &str) -> i64 {
        if let Some(value) = plain_attr(attrs, local).and_then(|value| value.trim().parse().ok()) {
            value
        } else {
            self.record(
                &format!("wp:@{local}"),
                SupportStatus::Partial,
                Some(format!("invalid or missing EMU attribute {local}")),
                Some(self.location()),
            );
            0
        }
    }
}

/// Parses an EMU-valued attribute.
fn parse_emu_attr(value: &str) -> Option<Emu> {
    value.trim().parse::<i64>().ok().map(Emu)
}

/// Parses a `u32` attribute.
fn parse_u32_attr(attrs: &[Attr], local: &str) -> Option<u32> {
    plain_attr(attrs, local).and_then(|value| value.trim().parse().ok())
}

/// Parses an `i32` attribute.
fn parse_i32_attr(attrs: &[Attr], local: &str) -> Option<i32> {
    plain_attr(attrs, local).and_then(|value| value.trim().parse().ok())
}

/// Parses an on/off attribute (`1`/`true`/`on`).
fn bool_attr(attrs: &[Attr], local: &str) -> bool {
    plain_attr(attrs, local).is_some_and(bool_value)
}

/// Parses an optional on/off attribute, preserving explicit `0`/`false`.
fn optional_bool_attr(attrs: &[Attr], local: &str) -> Option<bool> {
    plain_attr(attrs, local).map(bool_value)
}

/// Returns `true` when an attribute is present with a false-ish value.
fn attr_is_false(attrs: &[Attr], local: &str) -> bool {
    plain_attr(attrs, local).is_some_and(|value| !bool_value(value))
}

/// Interprets an OOXML boolean lexical value.
fn bool_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on"
    )
}

/// Extracts the colour of a fill (first stop for gradients).
fn fill_color(fill: &ShapeFill) -> Option<ShapeColor> {
    match fill {
        ShapeFill::Solid { color } => Some(color.clone()),
        ShapeFill::Gradient { stops, .. } => stops.first().map(|stop| stop.color.clone()),
        ShapeFill::Pattern { foreground, .. } => foreground.clone(),
        ShapeFill::None => None,
    }
}
