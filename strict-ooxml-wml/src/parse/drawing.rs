//! Parsing of `w:drawing` and inline picture graphics (STAGE-2 §8).

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::drawing::{
    AnchorStub, BlipRef, DocPr, Drawing, DrawingKind, Extent, InlineDrawing, MediaItem, MediaKind,
    Picture,
};
use crate::model::support::SupportStatus;
use crate::model::values::Emu;
use crate::{
    DRAWINGML_STRICT_NS, PICTURE_STRICT_NS, RELS_STRICT_NS, WORDPROCESSING_DRAWING_STRICT_NS,
};

use super::{attr_in_ns, plain_attr, PartParser};

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
                            Some("drawing content is not supported in Stage 2".to_owned()),
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
            picture: None,
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
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphic" {
                        let (uri, picture) = self.parse_graphic()?;
                        inline.graphic_uri = uri;
                        inline.picture = picture;
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

    /// Parses `a:graphic`, returning `(uri, picture)`.
    fn parse_graphic(&mut self) -> Result<(Option<std::sync::Arc<str>>, Option<Picture>)> {
        self.enter()?;
        let mut graphic_uri = None;
        let mut picture = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "graphicData" {
                        let (uri, pic) = self.parse_graphic_data(&attrs)?;
                        graphic_uri = uri;
                        picture = pic;
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
        Ok((graphic_uri, picture))
    }

    /// Parses `a:graphicData`, returning `(uri, picture)`.
    fn parse_graphic_data(
        &mut self,
        attrs: &[Attr],
    ) -> Result<(Option<std::sync::Arc<str>>, Option<Picture>)> {
        let uri = plain_attr(attrs, "uri").map(|value| self.intern(value));
        self.enter()?;
        let mut picture = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "pic" {
                        picture = Some(self.parse_picture()?);
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
        Ok((uri, picture))
    }

    /// Parses `pic:pic`.
    fn parse_picture(&mut self) -> Result<Picture> {
        self.enter()?;
        let mut picture = Picture {
            name: None,
            descr: None,
            blip: None,
            extent: None,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "nvPicPr" {
                        let (name, descr) = self.parse_nv_pic_pr()?;
                        picture.name = name;
                        picture.descr = descr;
                    } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "blipFill" {
                        picture.blip = self.parse_blip_fill()?;
                    } else if is_ns(&name, PICTURE_STRICT_NS) && name.local() == "spPr" {
                        picture.extent = self.parse_sp_pr()?;
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

    /// Parses `pic:blipFill`, resolving the image reference.
    fn parse_blip_fill(&mut self) -> Result<Option<BlipRef>> {
        self.enter()?;
        let mut blip = None;
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
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of blipFill")),
            }
        }
        self.leave();
        Ok(blip)
    }

    /// Parses `pic:spPr`, extracting the geometry extent.
    fn parse_sp_pr(&mut self) -> Result<Option<Extent>> {
        self.enter()?;
        let mut extent = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "ext" {
                        extent = Some(parse_extent(&attrs));
                        self.skip_element()?;
                    } else if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "xfrm" {
                        extent = self.parse_xfrm()?;
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
        Ok(extent)
    }

    /// Parses `a:xfrm`, returning its `a:ext`.
    fn parse_xfrm(&mut self) -> Result<Option<Extent>> {
        self.enter()?;
        let mut extent = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, DRAWINGML_STRICT_NS) && name.local() == "ext" {
                        extent = Some(parse_extent(&attrs));
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of transform")),
            }
        }
        self.leave();
        Ok(extent)
    }

    /// Parses `wp:anchor` as an unsupported stub.
    fn parse_anchor(&mut self, _attrs: &[Attr]) -> Result<AnchorStub> {
        let location = self.location();
        self.enter()?;
        let mut doc_pr = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_ns(&name, WORDPROCESSING_DRAWING_STRICT_NS) && name.local() == "docPr" {
                        doc_pr = Some(self.parse_doc_pr(&attrs));
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of anchor")),
            }
        }
        self.leave();
        self.record(
            "wp:anchor",
            SupportStatus::Unsupported,
            Some("floating drawings are Stage 5".to_owned()),
            Some(location.clone()),
        );
        Ok(AnchorStub { doc_pr, location })
    }
}

/// Returns `true` if a name belongs to `namespace`.
fn is_ns(name: &QName, namespace: &str) -> bool {
    name.ns.as_ref().is_some_and(|ns| ns == namespace)
}

/// Parses an extent from `cx`/`cy` attributes.
fn parse_extent(attrs: &[Attr]) -> Extent {
    Extent {
        cx: Emu(parse_emu(attrs, "cx")),
        cy: Emu(parse_emu(attrs, "cy")),
    }
}

fn parse_emu(attrs: &[Attr], local: &str) -> i64 {
    plain_attr(attrs, local)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

impl PartParser<'_> {
    /// Parses `wp:docPr` attributes.
    fn parse_doc_pr(&mut self, attrs: &[Attr]) -> DocPr {
        DocPr {
            id: plain_attr(attrs, "id").and_then(|value| value.trim().parse().ok()),
            name: plain_attr(attrs, "name").map(|value| self.intern(value)),
            descr: plain_attr(attrs, "descr").map(|value| self.intern(value)),
        }
    }
}
