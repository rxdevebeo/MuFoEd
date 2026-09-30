//! Parsing of `w:drawing`: inline pictures, anchored drawings, shapes, groups
//! and text boxes (`STAGE-2 §8`, `STAGE-5B-TASK.md` §5.1–§5.4).

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::drawing::{
    AnchorDrawing, BlipRef, CustomGeometry, DocPr, Drawing, DrawingKind, EffectExtent, Extent,
    ForeignRefs, GradientStop, Graphic, GroupShape, GroupTransform, InlineDrawing, MediaItem,
    MediaKind, PathCommand, Picture, Position, Shape, ShapeColor, ShapeFill, ShapeGeometry,
    ShapeStroke, ShapeStyle, SrcRect, TextAnchor, TextBox, TextBoxBody, Wrap, WrapKind, Xfrm,
};
use crate::model::support::SupportStatus;
use crate::model::values::{Color, Emu, ThemeColor, ThemeColorRef};
use crate::{
    CHART_STRICT_NS, DIAGRAM_STRICT_NS, DRAWINGML_STRICT_NS, MS_WORD_2006_WML_NS,
    MS_WORD_PROCESSING_GROUP_NS, MS_WORD_PROCESSING_SHAPE_NS, PICTURE_STRICT_NS, RELS_STRICT_NS,
    WORDPROCESSING_DRAWING_STRICT_NS, WORD_PROCESSING_GROUP_STRICT_NS,
    WORD_PROCESSING_SHAPE_STRICT_NS,
};

use super::{attr_in_ns, plain_attr, PartParser};

/// `r:id` on a `c:chart`.
const R_ID: &str = "id";
/// `r:dm` on a `dgm:relIds` - the diagram data part.
const REL_DM: &str = "dm";
/// `r:lo` on a `dgm:relIds` - the diagram layout part.
const REL_LO: &str = "lo";
/// `r:qs` on a `dgm:relIds` - the diagram quick-style part.
const REL_QS: &str = "qs";
/// `r:cs` on a `dgm:relIds` - the diagram colour part.
const REL_CS: &str = "cs";

impl PartParser<'_> {
    /// Parses a `w:drawing` element; its start element has been consumed.
    pub(crate) fn parse_drawing(&mut self) -> Result<Drawing> {
        let location = self.location();
        self.enter()?;
        let mut kind = DrawingKind::Opaque(crate::model::inline::OpaqueInline {
            namespace: self.intern(crate::WML_STRICT_NS),
            local: self.intern("drawing"),
            attributes: Vec::new(),
            location: location.clone(),
        });
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) && name.local() == "inline" {
                        let inline = self.parse_inline_drawing()?;
                        kind = DrawingKind::Inline(inline);
                    } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                        && name.local() == "anchor"
                    {
                        let anchor = self.parse_anchor(&attrs)?;
                        kind = DrawingKind::Anchor(anchor);
                    } else {
                        self.record(
                            &super::feature_id_for(&name),
                            SupportStatus::Unsupported,
                            Some("drawing content is not supported".to_owned()),
                            Some(self.location()),
                        );
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of drawing")),
            }
        }
        self.leave();
        Ok(Drawing { kind, location })
    }

    /// Parses `wp:inline`.
    fn parse_inline_drawing(&mut self) -> Result<InlineDrawing> {
        let location = self.location();
        self.enter()?;
        let mut inline = InlineDrawing {
            extent: None,
            doc_pr: None,
            graphic_uri: None,
            graphic: Box::new(Graphic::None),
            location,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) && name.local() == "extent" {
                        inline.extent = Some(parse_extent(&attrs));
                        self.skip_element()?;
                    } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                        && name.local() == "docPr"
                    {
                        inline.doc_pr = Some(self.parse_doc_pr(&attrs));
                        self.skip_element()?;
                    } else if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS)
                        && name.local() == "effectExtent"
                    {
                        self.skip_element()?;
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphic" {
                        let (uri, graphic) = self.parse_graphic()?;
                        inline.graphic_uri = uri;
                        inline.graphic = Box::new(graphic);
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of inline drawing")),
            }
        }
        self.leave();
        Ok(inline)
    }

    /// Parses `wp:anchor`.
    fn parse_anchor(&mut self, attrs: &[Attr]) -> Result<AnchorDrawing> {
        let location = self.location();
        self.enter()?;
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
            location,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) {
                        match name.local() {
                            "positionH" => anchor.position_h = Some(self.parse_position(&attrs)?),
                            "positionV" => anchor.position_v = Some(self.parse_position(&attrs)?),
                            "extent" => {
                                anchor.extent = Some(parse_extent(&attrs));
                                self.skip_element()?;
                            }
                            "effectExtent" => {
                                anchor.effect_extent = Some(parse_effect_extent(&attrs));
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
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of anchor")),
            }
        }
        self.leave();
        self.record(
            "wp:anchor",
            SupportStatus::Supported,
            None,
            Some(anchor.location.clone()),
        );
        Ok(anchor)
    }

    /// Parses `wp:positionH`/`wp:positionV`.
    fn parse_position(&mut self, attrs: &[Attr]) -> Result<Position> {
        let mut position = Position {
            relative_from: plain_attr(attrs, "relativeFrom").map(|value| self.intern(value)),
            align: None,
            offset: None,
        };
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) {
                        if name.local() == "posOffset" {
                            let text = self.read_element_text()?;
                            position.offset = text.trim().parse::<i64>().ok().map(Emu);
                        } else if name.local() == "align" {
                            let text = self.read_element_text()?;
                            let value = text.trim().to_owned();
                            if !value.is_empty() {
                                position.align = Some(self.intern(&value));
                            }
                        } else {
                            self.skip_element()?;
                        }
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of position")),
            }
        }
        self.leave();
        Ok(position)
    }

    /// Parses a `wp:wrap*` element.
    fn parse_wrap(&mut self, kind: WrapKind, attrs: &[Attr]) -> Result<Wrap> {
        let wrap = Wrap {
            kind,
            wrap_text: plain_attr(attrs, "wrapText").map(|value| self.intern(value)),
            dist_left: parse_u32_attr(attrs, "distL"),
            dist_right: parse_u32_attr(attrs, "distR"),
            dist_top: parse_u32_attr(attrs, "distT"),
            dist_bottom: parse_u32_attr(attrs, "distB"),
        };
        self.skip_element()?;
        Ok(wrap)
    }

    /// Parses `a:graphic`, returning `(uri, graphic)`.
    fn parse_graphic(&mut self) -> Result<(Option<std::sync::Arc<str>>, Graphic)> {
        self.enter()?;
        let mut graphic_uri = None;
        let mut graphic = Graphic::None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphicData" {
                        let (uri, parsed) = self.parse_graphic_data(&attrs)?;
                        graphic_uri = uri;
                        graphic = parsed;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of graphic")),
            }
        }
        self.leave();
        Ok((graphic_uri, graphic))
    }

    /// Parses `a:graphicData`, returning `(uri, graphic)`.
    fn parse_graphic_data(
        &mut self,
        attrs: &[Attr],
    ) -> Result<(Option<std::sync::Arc<str>>, Graphic)> {
        let uri = plain_attr(attrs, "uri").map(|value| self.intern(value));
        self.enter()?;
        let mut graphic = Graphic::None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement {
                    name,
                    attrs: element,
                } => {
                    if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "pic" {
                        graphic = Graphic::Picture(self.parse_picture()?);
                    } else if name.local() == "wsp" && is_shape_ns(&name) {
                        if !is_ns(&name, WORD_PROCESSING_SHAPE_STRICT_NS) {
                            self.record(
                                "wps:wsp",
                                SupportStatus::Partial,
                                Some("Microsoft/legacy shape namespace compatibility".to_owned()),
                                Some(self.location()),
                            );
                        }
                        graphic = Graphic::Shape(self.parse_shape()?);
                    } else if name.local() == "wgp" && is_group_ns(&name) {
                        if !is_ns(&name, WORD_PROCESSING_GROUP_STRICT_NS) {
                            self.record(
                                "wpg:wgp",
                                SupportStatus::Partial,
                                Some("Microsoft/legacy group namespace compatibility".to_owned()),
                                Some(self.location()),
                            );
                        }
                        graphic = Graphic::Group(self.parse_group()?);
                    } else if name.local() == "chart" {
                        // The attributes of *this* element, not of the
                        // `a:graphicData` that carries it: `r:id` is where the
                        // chart part is named, and reading the wrong element's
                        // attributes looks exactly like a document with no
                        // reference at all.
                        graphic = Graphic::Chart(self.foreign_refs(&element, &[R_ID]));
                        self.skip_element()?;
                    } else if name.local() == "relIds" {
                        // `dgm:relIds` carries four ids in a fixed order, and the
                        // order is the only thing that says which is which: the
                        // attributes have no positional meaning in XML.
                        graphic = Graphic::Diagram(
                            self.foreign_refs(&element, &[REL_DM, REL_LO, REL_QS, REL_CS]),
                        );
                        self.skip_element()?;
                    } else if is_ns(&name, CHART_STRICT_NS) || is_ns(&name, DIAGRAM_STRICT_NS) {
                        graphic = Graphic::Other;
                        self.skip_element()?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of graphic data")),
            }
        }
        self.leave();
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
        self.enter()?;
        let mut picture = Picture {
            name: None,
            descr: None,
            blip: None,
            extent: None,
            src_rect: None,
            xfrm: None,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "nvPicPr" {
                        let (name, descr) = self.parse_nv_pic_pr()?;
                        picture.name = name;
                        picture.descr = descr;
                    } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "blipFill" {
                        let (blip, src_rect) = self.parse_blip_fill()?;
                        picture.blip = blip;
                        picture.src_rect = src_rect;
                    } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "spPr" {
                        let (extent, xfrm) = self.parse_sp_pr()?;
                        picture.extent = extent;
                        picture.xfrm = xfrm;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of picture")),
            }
        }
        self.leave();
        Ok(picture)
    }

    /// Parses `pic:nvPicPr`.
    fn parse_nv_pic_pr(
        &mut self,
    ) -> Result<(Option<std::sync::Arc<str>>, Option<std::sync::Arc<str>>)> {
        self.enter()?;
        let mut name = None;
        let mut descr = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement {
                    name: element,
                    attrs,
                } => {
                    if is_ns(&element, PICTURE_STRICT_NS) && element.local() == "cNvPr" {
                        name = plain_attr(&attrs, "name").map(|value| self.intern(value));
                        descr = plain_attr(&attrs, "descr").map(|value| self.intern(value));
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of nvPicPr")),
            }
        }
        self.leave();
        Ok((name, descr))
    }

    /// Parses `pic:blipFill`, resolving the image reference and crop.
    fn parse_blip_fill(&mut self) -> Result<(Option<BlipRef>, Option<SrcRect>)> {
        self.enter()?;
        let mut blip = None;
        let mut src_rect = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "blip" {
                        let embed = attr_in_ns(&attrs, RELS_STRICT_NS, "embed").map(RelId::new);
                        let link = attr_in_ns(&attrs, RELS_STRICT_NS, "link").map(RelId::new);
                        let resolved = embed
                            .as_ref()
                            .and_then(|id| self.resolve_relationship_target(id.as_str()));
                        if let Some(part) = &resolved {
                            let content_type = self.content_type(part);
                            let kind = content_type.as_deref().map_or_else(
                                || {
                                    let extension =
                                        part.as_str().rsplit_once('.').map_or("", |(_, ext)| ext);
                                    MediaKind::from_extension(extension)
                                },
                                MediaKind::from_content_type,
                            );
                            self.media.insert(MediaItem {
                                part: part.clone(),
                                content_type,
                                kind,
                            });
                        }
                        blip = Some(BlipRef {
                            embed,
                            link,
                            resolved,
                            location: self.location(),
                        });
                        self.skip_element()?;
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "srcRect" {
                        src_rect = Some(parse_src_rect(&attrs));
                        self.skip_element()?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of blipFill")),
            }
        }
        self.leave();
        Ok((blip, src_rect))
    }

    /// Parses `pic:spPr`, extracting the extent and transform.
    fn parse_sp_pr(&mut self) -> Result<(Option<Extent>, Option<Xfrm>)> {
        self.enter()?;
        let mut extent = None;
        let mut xfrm = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "ext" {
                        extent = Some(parse_extent(&attrs));
                        self.skip_element()?;
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "xfrm" {
                        let parts = self.parse_xfrm_parts(&attrs)?;
                        if parts.extent.is_some() {
                            extent = parts.extent;
                        }
                        if parts.offset.is_some()
                            || parts.rot.is_some()
                            || parts.flip_h
                            || parts.flip_v
                        {
                            xfrm = Some(Xfrm {
                                offset: parts.offset,
                                rot: parts.rot,
                                flip_h: parts.flip_h,
                                flip_v: parts.flip_v,
                            });
                        }
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of shape properties")),
            }
        }
        self.leave();
        Ok((extent, xfrm))
    }

    /// Parses a `wps:wsp` shape.
    fn parse_shape(&mut self) -> Result<Shape> {
        let location = self.location();
        self.enter()?;
        let mut shape = Shape {
            name: None,
            descr: None,
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
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_shape_ns(&name) {
                        match name.local() {
                            "cNvPr" => {
                                shape.name = plain_attr(&attrs, "name").map(|v| self.intern(v));
                                shape.descr = plain_attr(&attrs, "descr").map(|v| self.intern(v));
                                self.skip_element()?;
                            }
                            "spPr" => self.parse_shape_sp_pr(&mut shape)?,
                            "style" => shape.style = Some(self.parse_shape_style()?),
                            "txbx" => shape.text = Some(self.parse_text_box()?),
                            "bodyPr" => body = Some(self.parse_text_box_body(&attrs)?),
                            _ => self.skip_element()?,
                        }
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of shape")),
            }
        }
        self.leave();
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
        self.record(
            "wps:wsp",
            SupportStatus::Supported,
            None,
            Some(shape.location.clone()),
        );
        Ok(shape)
    }

    /// Parses `wps:spPr` into `shape`.
    fn parse_shape_sp_pr(&mut self, shape: &mut Shape) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) {
                        match name.local() {
                            "xfrm" => {
                                let parts = self.parse_xfrm_parts(&attrs)?;
                                shape.offset = parts.offset;
                                shape.extent = parts.extent;
                                if parts.rot.is_some() || parts.flip_h || parts.flip_v {
                                    shape.xfrm = Some(Xfrm {
                                        offset: None,
                                        rot: parts.rot,
                                        flip_h: parts.flip_h,
                                        flip_v: parts.flip_v,
                                    });
                                }
                            }
                            "prstGeom" => {
                                let preset = plain_attr(&attrs, "prst")
                                    .map_or_else(|| "rect".to_owned(), str::to_owned);
                                shape.geometry = ShapeGeometry::Preset(self.intern(&preset));
                                self.skip_element()?;
                            }
                            "custGeom" => {
                                shape.geometry = ShapeGeometry::Custom(self.parse_cust_geom()?);
                            }
                            "solidFill" | "gradFill" | "pattFill" | "noFill" | "blipFill"
                            | "grpFill" => {
                                if shape.fill.is_none() {
                                    shape.fill = self.parse_shape_fill(&name, &attrs)?;
                                } else {
                                    self.skip_element()?;
                                }
                            }
                            "ln" => shape.stroke = Some(self.parse_shape_stroke(&attrs)?),
                            _ => self.skip_element()?,
                        }
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of shape properties")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses `a:xfrm` attributes and children.
    fn parse_xfrm_parts(&mut self, attrs: &[Attr]) -> Result<XfrmParts> {
        let mut parts = XfrmParts {
            rot: parse_i32_attr(attrs, "rot"),
            flip_h: bool_attr(attrs, "flipH"),
            flip_v: bool_attr(attrs, "flipV"),
            ..XfrmParts::default()
        };
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "off" {
                        parts.offset = Some(parse_offset(&attrs));
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "ext" {
                        parts.extent = Some(parse_extent(&attrs));
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "chOff" {
                        parts.child_offset = Some(parse_offset(&attrs));
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "chExt" {
                        parts.child_extent = Some(parse_extent(&attrs));
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of transform")),
            }
        }
        self.leave();
        Ok(parts)
    }

    /// Parses `a:custGeom` (a subset: moveTo/lnTo/cubicBezTo/close).
    fn parse_cust_geom(&mut self) -> Result<CustomGeometry> {
        self.enter()?;
        let mut geometry = CustomGeometry::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "pathLst" {
                        self.parse_path_list(&mut geometry)?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of custom geometry")),
            }
        }
        self.leave();
        self.record(
            "a:custGeom",
            SupportStatus::Partial,
            Some("custom geometry supports moveTo/lnTo/cubicBezTo/close only".to_owned()),
            Some(self.location()),
        );
        Ok(geometry)
    }

    /// Parses `a:pathLst`.
    fn parse_path_list(&mut self, geometry: &mut CustomGeometry) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "path" {
                        geometry.width = plain_attr(&attrs, "w")
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        geometry.height = plain_attr(&attrs, "h")
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        self.parse_path(geometry)?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of path list")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses one `a:path`.
    fn parse_path(&mut self, geometry: &mut CustomGeometry) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if !is_ns(&name, DRAWINGML_STRICT_NS) {
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "moveTo" => {
                            let point = self.parse_first_point()?;
                            geometry.commands.push(PathCommand::MoveTo {
                                x: point.0,
                                y: point.1,
                            });
                        }
                        "lnTo" => {
                            let point = self.parse_first_point()?;
                            geometry.commands.push(PathCommand::LineTo {
                                x: point.0,
                                y: point.1,
                            });
                        }
                        "cubicBezTo" => {
                            let points = self.parse_points()?;
                            if points.len() >= 3 {
                                geometry.commands.push(PathCommand::CubicBezTo {
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
                            geometry.commands.push(PathCommand::Close);
                            self.skip_element()?;
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of path")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses the first `a:pt` of the current element.
    fn parse_first_point(&mut self) -> Result<(i64, i64)> {
        let points = self.parse_points()?;
        Ok(points.first().copied().unwrap_or((0, 0)))
    }

    /// Parses all `a:pt` children of the current element.
    fn parse_points(&mut self) -> Result<Vec<(i64, i64)>> {
        self.enter()?;
        let mut points = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "pt" {
                        let x = plain_attr(&attrs, "x").and_then(|v| v.trim().parse().ok());
                        let y = plain_attr(&attrs, "y").and_then(|v| v.trim().parse().ok());
                        if let (Some(x), Some(y)) = (x, y) {
                            points.push((x, y));
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of point list")),
            }
        }
        self.leave();
        Ok(points)
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
                let mut foreground = None;
                let mut background = None;
                self.enter()?;
                loop {
                    match self.next_event()? {
                        XmlEvent::StartElement { name, attrs: _ } => {
                            if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "fgClr" {
                                foreground = self.parse_child_color()?;
                            } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "bgClr" {
                                background = self.parse_child_color()?;
                            } else {
                                self.skip_element()?;
                            }
                        }
                        XmlEvent::EndElement { .. } => break,
                        XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                        XmlEvent::Eof => return Err(self.invalid("unexpected end of pattern fill")),
                    }
                }
                self.leave();
                Ok(Some(ShapeFill::Pattern {
                    preset,
                    foreground,
                    background,
                }))
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
        self.enter()?;
        let mut color = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if color.is_none() && is_ns(&name, DRAWINGML_STRICT_NS) {
                        color = self.parse_shape_color(&name, &attrs)?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of fill")),
            }
        }
        self.leave();
        Ok(color)
    }

    /// Parses one colour element child of a fill wrapper.
    fn parse_child_color(&mut self) -> Result<Option<ShapeColor>> {
        self.enter()?;
        let mut color = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if color.is_none() && is_ns(&name, DRAWINGML_STRICT_NS) {
                        color = self.parse_shape_color(&name, &attrs)?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of colour")),
            }
        }
        self.leave();
        Ok(color)
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
                let mut tint = None;
                let mut shade = None;
                self.enter()?;
                loop {
                    match self.next_event()? {
                        XmlEvent::StartElement { name, attrs } => {
                            if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "tint" {
                                tint = plain_attr(&attrs, "val").map(|v| self.intern(v));
                            } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "shade" {
                                shade = plain_attr(&attrs, "val").map(|v| self.intern(v));
                            }
                            self.skip_element()?;
                        }
                        XmlEvent::EndElement { .. } => break,
                        XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                        XmlEvent::Eof => {
                            return Err(self.invalid("unexpected end of scheme colour"));
                        }
                    }
                }
                self.leave();
                Ok(slot.map(|slot| ShapeColor {
                    value: None,
                    theme: Some(ThemeColorRef {
                        color: ThemeColor::new(slot),
                        tint,
                        shade,
                    }),
                }))
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
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "gsLst" {
                        self.parse_gradient_stops(&mut stops)?;
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "lin" {
                        angle = parse_i32_attr(&attrs, "ang");
                        self.skip_element()?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of gradient fill")),
            }
        }
        self.leave();
        Ok(ShapeFill::Gradient { stops, angle })
    }

    /// Parses `a:gsLst`.
    fn parse_gradient_stops(&mut self, stops: &mut Vec<GradientStop>) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "gs" {
                        let position = parse_i32_attr(&attrs, "pos").unwrap_or(0);
                        let color = self.parse_fill_color()?.unwrap_or_default();
                        stops.push(GradientStop { position, color });
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of gradient stops")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses `a:ln`.
    fn parse_shape_stroke(&mut self, attrs: &[Attr]) -> Result<ShapeStroke> {
        let mut stroke = ShapeStroke {
            width: plain_attr(attrs, "w")
                .and_then(|v| v.trim().parse::<i64>().ok())
                .map(Emu),
            ..ShapeStroke::default()
        };
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) {
                        match name.local() {
                            "noFill" => {
                                stroke.none = true;
                                self.skip_element()?;
                            }
                            "solidFill" | "gradFill" | "pattFill" => {
                                if let Some(fill) = self.parse_shape_fill(&name, &attrs)? {
                                    stroke.color = fill_color(&fill);
                                }
                            }
                            "prstDash" => {
                                stroke.dash = plain_attr(&attrs, "val").map(|v| self.intern(v));
                                self.skip_element()?;
                            }
                            "headEnd" => {
                                stroke.head_end =
                                    plain_attr(&attrs, "type").map(|v| self.intern(v));
                                self.skip_element()?;
                            }
                            "tailEnd" => {
                                stroke.tail_end =
                                    plain_attr(&attrs, "type").map(|v| self.intern(v));
                                self.skip_element()?;
                            }
                            _ => self.skip_element()?,
                        }
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of outline")),
            }
        }
        self.leave();
        Ok(stroke)
    }

    /// Parses `wps:style`.
    fn parse_shape_style(&mut self) -> Result<ShapeStyle> {
        self.enter()?;
        let mut style = ShapeStyle::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) {
                        let index = parse_i32_attr(&attrs, "idx");
                        match name.local() {
                            "lnRef" => style.line_ref = index,
                            "fillRef" => style.fill_ref = index,
                            "effectRef" => style.effect_ref = index,
                            "fontRef" => style.font_ref = index,
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of shape style")),
            }
        }
        self.leave();
        Ok(style)
    }

    /// Parses `wps:txbx`.
    fn parse_text_box(&mut self) -> Result<TextBox> {
        let location = self.location();
        self.enter()?;
        let mut text = TextBox {
            body: None,
            blocks: Vec::new(),
            location,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if name.local() == "txbxContent"
                        && (is_wml_name(&name) || is_ns(&name, MS_WORD_2006_WML_NS))
                    {
                        let (blocks, _) = self.parse_block_children()?;
                        text.blocks = blocks;
                        self.record(
                            "w:txbxContent",
                            SupportStatus::Supported,
                            None,
                            Some(self.location()),
                        );
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of text box")),
            }
        }
        self.leave();
        Ok(text)
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
            anchor_centered: bool_attr(attrs, "anchorCtr"),
            left_inset: plain_attr(attrs, "lIns").and_then(parse_emu_attr),
            top_inset: plain_attr(attrs, "tIns").and_then(parse_emu_attr),
            right_inset: plain_attr(attrs, "rIns").and_then(parse_emu_attr),
            bottom_inset: plain_attr(attrs, "bIns").and_then(parse_emu_attr),
        };
        self.skip_element()?;
        Ok(body)
    }

    /// Parses a `wpg:wgp` group.
    fn parse_group(&mut self) -> Result<GroupShape> {
        let location = self.location();
        self.enter()?;
        let mut group = GroupShape {
            name: None,
            descr: None,
            xfrm: None,
            children: Vec::new(),
            location,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_group_ns(&name) {
                        match name.local() {
                            "cNvPr" => {
                                group.name = plain_attr(&attrs, "name").map(|v| self.intern(v));
                                group.descr = plain_attr(&attrs, "descr").map(|v| self.intern(v));
                                self.skip_element()?;
                            }
                            "grpSpPr" => group.xfrm = Some(self.parse_group_transform(&attrs)?),
                            _ => self.skip_element()?,
                        }
                    } else if name.local() == "wsp" && is_shape_ns(&name) {
                        group.children.push(Graphic::Shape(self.parse_shape()?));
                    } else if name.local() == "wgp" && is_group_ns(&name) {
                        group.children.push(Graphic::Group(self.parse_group()?));
                    } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "pic" {
                        group.children.push(Graphic::Picture(self.parse_picture()?));
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of group")),
            }
        }
        self.leave();
        self.record(
            "wpg:wgp",
            SupportStatus::Supported,
            None,
            Some(group.location.clone()),
        );
        Ok(group)
    }

    /// Parses `wpg:grpSpPr` (start consumed), reading its `a:xfrm` child.
    fn parse_group_transform(&mut self, _attrs: &[Attr]) -> Result<GroupTransform> {
        self.enter()?;
        let mut parts = XfrmParts::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "xfrm" {
                        parts = self.parse_xfrm_parts(&attrs)?;
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of group transform")),
            }
        }
        self.leave();
        Ok(GroupTransform {
            offset: parts.offset,
            extent: parts.extent,
            child_offset: parts.child_offset,
            child_extent: parts.child_extent,
            rot: parts.rot,
            flip_h: parts.flip_h,
            flip_v: parts.flip_v,
        })
    }

    /// Reads the text content of the current element (start consumed).
    fn read_element_text(&mut self) -> Result<String> {
        self.enter()?;
        let mut out = String::new();
        loop {
            match self.next_event()? {
                XmlEvent::Text(text) | XmlEvent::CData(text) => out.push_str(text.as_ref()),
                XmlEvent::StartElement { .. } => self.skip_element()?,
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Eof => return Err(self.invalid("unexpected end of text content")),
            }
        }
        self.leave();
        Ok(out)
    }

    /// Parses `wp:docPr` attributes.
    fn parse_doc_pr(&mut self, attrs: &[Attr]) -> DocPr {
        DocPr {
            id: plain_attr(attrs, "id").and_then(|value| value.trim().parse().ok()),
            name: plain_attr(attrs, "name").map(|value| self.intern(value)),
            descr: plain_attr(attrs, "descr").map(|value| self.intern(value)),
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

/// Parses an extent from `cx`/`cy` attributes.
fn parse_extent(attrs: &[Attr]) -> Extent {
    Extent {
        cx: Emu(parse_emu(attrs, "cx")),
        cy: Emu(parse_emu(attrs, "cy")),
    }
}

/// Parses an offset from `x`/`y` attributes.
fn parse_offset(attrs: &[Attr]) -> (Emu, Emu) {
    (Emu(parse_emu(attrs, "x")), Emu(parse_emu(attrs, "y")))
}

/// Parses an `wp:effectExtent`.
fn parse_effect_extent(attrs: &[Attr]) -> EffectExtent {
    EffectExtent {
        left: Emu(parse_emu(attrs, "l")),
        top: Emu(parse_emu(attrs, "t")),
        right: Emu(parse_emu(attrs, "r")),
        bottom: Emu(parse_emu(attrs, "b")),
    }
}

/// Parses an `a:srcRect` crop.
fn parse_src_rect(attrs: &[Attr]) -> SrcRect {
    SrcRect {
        left: parse_i32_attr(attrs, "l").unwrap_or(0),
        top: parse_i32_attr(attrs, "t").unwrap_or(0),
        right: parse_i32_attr(attrs, "r").unwrap_or(0),
        bottom: parse_i32_attr(attrs, "b").unwrap_or(0),
    }
}

/// Parses an EMU attribute.
fn parse_emu(attrs: &[Attr], local: &str) -> i64 {
    plain_attr(attrs, local)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
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
