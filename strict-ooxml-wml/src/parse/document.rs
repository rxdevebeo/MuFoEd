//! Parsing of `document.xml`: body, paragraphs, runs, text and inlines.

use std::sync::Arc;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::block::{AltChunkInfo, Block, OpaqueBlock, Paragraph, SdtContainer};
use crate::model::document::Body;
use crate::model::ids::{ParaId, TextId};
use crate::model::inline::{
    Bookmark, BookmarkId, CommentId, Field, FieldChar, Hyperlink, Inline, OpaqueInline, Run,
    RunContent, Symbol, TextNode,
};
use crate::model::props::{ParagraphProperties, Section};
use crate::model::support::SupportStatus;
use crate::model::values::{BreakKind, FieldCharType, Rsids, Space};
use crate::RELS_STRICT_NS;

use super::dispatch::{body_kind, inline_kind, run_kind, BodyKind, InlineKind, RunKind};
use super::{
    attr_in_ns, feature_id_for, is_wml, parse_u32, val_attr, wml_attr, PartParser, W14_NS, XML_NS,
};

impl PartParser<'_> {
    /// Parses the `w:document` root and its `w:body`.
    pub(crate) fn parse_document_root(&mut self) -> Result<(Body, Vec<Section>)> {
        self.enter()?;
        self.expect_root("document")?;

        let mut body = Body::default();
        let mut sections = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if name.local() == "body" && is_wml(&name) {
                        let (blocks, found_sections) = self.parse_block_children()?;
                        body.blocks = blocks;
                        sections = found_sections;
                    } else if name.local() == "background" && is_wml(&name) {
                        self.record(
                            "w:background",
                            SupportStatus::Partial,
                            None,
                            Some(self.location()),
                        );
                        self.skip_element()?;
                    } else {
                        self.record_foreign(&name);
                        let _ = attrs;
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of document part")),
            }
        }
        self.leave();
        Ok((body, sections))
    }

    /// Parses block-level children until the current element's end.
    pub(crate) fn parse_block_children(&mut self) -> Result<(Vec<Block>, Vec<Section>)> {
        self.enter()?;
        let mut blocks = Vec::new();
        let mut sections = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    self.parse_block_element_into(&name, &attrs, &mut blocks, &mut sections)?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of block content")),
            }
        }
        self.leave();
        Ok((blocks, sections))
    }

    /// Dispatches one block element (start already consumed) into `blocks`.
    pub(crate) fn parse_block_element_into(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        blocks: &mut Vec<Block>,
        sections: &mut Vec<Section>,
    ) -> Result<()> {
        match body_kind(name.local()) {
            BodyKind::Paragraph => {
                let paragraph = self.parse_paragraph(attrs)?;
                if let Some(props) = &paragraph.props.section {
                    sections.push(Section {
                        properties: props.clone(),
                        location: props
                            .location
                            .clone()
                            .unwrap_or_else(|| paragraph.location.clone()),
                    });
                }
                blocks.push(Block::Paragraph(paragraph));
            }
            BodyKind::Table => blocks.push(Block::Table(self.parse_table()?)),
            BodyKind::Sdt => blocks.push(Block::SdtBlock(self.parse_sdt(true)?)),
            BodyKind::AltChunk => blocks.push(Block::AltChunk(self.parse_alt_chunk(attrs)?)),
            BodyKind::Section => {
                let location = self.location();
                let properties = self.parse_section_properties()?;
                sections.push(Section {
                    properties,
                    location,
                });
            }
            BodyKind::Inserted | BodyKind::Deleted => {
                let feature = if body_kind(name.local()) == BodyKind::Inserted {
                    "w:ins"
                } else {
                    "w:del"
                };
                self.record(
                    feature,
                    SupportStatus::Partial,
                    Some("tracked change container flattened".to_owned()),
                    Some(self.location()),
                );
                let (mut children, _) = self.parse_block_children()?;
                blocks.append(&mut children);
            }
            BodyKind::Ignored => self.skip_element()?,
            BodyKind::Opaque => {
                self.record_foreign(name);
                blocks.push(Block::Opaque(self.capture_opaque_block(name, attrs)));
                self.skip_element()?;
            }
        }
        Ok(())
    }

    /// Parses a paragraph (`w:p`); its start element has been consumed.
    fn parse_paragraph(&mut self, attrs: &[Attr]) -> Result<Paragraph> {
        let location = self.location();
        self.enter()?;
        let mut props = ParagraphProperties::default();
        let mut inlines = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if name.local() == "pPr" && is_wml(&name) {
                        props = self.parse_paragraph_properties()?;
                    } else if crate::parse::is_math(&name) {
                        self.parse_math_into(&name, &mut inlines)?;
                    } else if is_wml(&name) {
                        self.parse_inline_into(&name, &attrs, &mut inlines)?;
                    } else {
                        self.record_foreign(&name);
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of paragraph")),
            }
        }
        self.leave();

        Ok(Paragraph {
            props,
            inlines,
            rsids: rsids_from_attrs(attrs),
            para_id: attr_in_ns(attrs, W14_NS, "paraId").map(ParaId::new),
            text_id: attr_in_ns(attrs, W14_NS, "textId").map(TextId::new),
            location,
        })
    }

    /// Parses inline children until the current element's end.
    fn parse_inline_children(&mut self) -> Result<Vec<Inline>> {
        self.enter()?;
        let mut out = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if crate::parse::is_math(&name) {
                        self.parse_math_into(&name, &mut out)?;
                    } else if is_wml(&name) {
                        self.parse_inline_into(&name, &attrs, &mut out)?;
                    } else {
                        self.record_foreign(&name);
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of inline content")),
            }
        }
        self.leave();
        Ok(out)
    }

    /// Dispatches one OMML element into `out` (`STAGE-5C-TASK.md` §3.1.1).
    ///
    /// `m:oMath` and `m:oMathPara` are paragraph content, so they arrive
    /// through the inline path; only the math namespace reaches here.
    fn parse_math_into(&mut self, name: &QName, out: &mut Vec<Inline>) -> Result<()> {
        match name.local() {
            "oMath" => out.push(Inline::Math(crate::parse::math::parse_omath(self)?)),
            "oMathPara" => out.push(Inline::MathParagraph(crate::parse::math::parse_omath_para(
                self,
            )?)),
            other => {
                self.record(
                    &format!("m:{other}"),
                    SupportStatus::Partial,
                    Some("OMML element outside a formula".to_owned()),
                    Some(self.location()),
                );
                self.skip_element()?;
            }
        }
        Ok(())
    }

    /// Dispatches one paragraph child element into `out`.
    fn parse_inline_into(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        let location = self.location();
        match inline_kind(name.local()) {
            InlineKind::Run => out.push(Inline::Run(self.parse_run(attrs)?)),
            InlineKind::Hyperlink => out.push(Inline::Hyperlink(self.parse_hyperlink(attrs)?)),
            InlineKind::Field => out.push(Inline::Field(self.parse_fld_simple(attrs)?)),
            InlineKind::Drawing => out.push(Inline::Drawing(self.parse_drawing()?)),
            InlineKind::BookmarkStart => {
                // Both attributes are read, and they are not the same attribute:
                // `w:id` pairs the start with its end, `w:name` is what a
                // hyperlink anchor and a REF field target. Keeping only the id
                // made every internal link lose its destination and made the
                // written element schema-invalid (`CT_Bookmark` requires the
                // name). A producer that wrote no name is recorded rather than
                // patched here.
                let id = wml_attr(attrs, "id").or_else(|| wml_attr(attrs, "name"));
                if let Some(id) = id {
                    let name = wml_attr(attrs, "name").unwrap_or_default();
                    if name.is_empty() {
                        self.record_value("w:bookmarkStart/@w:name", "", &self.location());
                    }
                    out.push(Inline::BookmarkStart(Bookmark::new(id, name)));
                }
                self.skip_element()?;
            }
            InlineKind::BookmarkEnd => {
                if let Some(id) = wml_attr(attrs, "id") {
                    out.push(Inline::BookmarkEnd(BookmarkId::new(id)));
                }
                self.skip_element()?;
            }
            InlineKind::CommentRangeStart => {
                if let Some(id) = wml_attr(attrs, "id") {
                    out.push(Inline::CommentRangeStart(CommentId::new(id)));
                }
                self.skip_element()?;
            }
            InlineKind::CommentRangeEnd => {
                if let Some(id) = wml_attr(attrs, "id") {
                    out.push(Inline::CommentRangeEnd(CommentId::new(id)));
                }
                self.skip_element()?;
            }
            InlineKind::CommentReference => {
                if let Some(id) = wml_attr(attrs, "id") {
                    out.push(Inline::CommentReference(CommentId::new(id)));
                }
                self.record(
                    "w:commentReference",
                    SupportStatus::Ignored,
                    Some("comment bodies are parsed in Stage 5".to_owned()),
                    Some(location),
                );
                self.skip_element()?;
            }
            InlineKind::FootnoteRef => {
                let id = wml_attr(attrs, "id").and_then(parse_u32).unwrap_or(0);
                out.push(Inline::FootnoteRef(id));
                self.record(
                    "w:footnoteReference",
                    SupportStatus::Supported,
                    None,
                    Some(location),
                );
                self.skip_element()?;
            }
            InlineKind::EndnoteRef => {
                let id = wml_attr(attrs, "id").and_then(parse_u32).unwrap_or(0);
                out.push(Inline::EndnoteRef(id));
                self.record(
                    "w:endnoteReference",
                    SupportStatus::Supported,
                    None,
                    Some(location),
                );
                self.skip_element()?;
            }
            InlineKind::Sdt => out.push(Inline::SdtInline(self.parse_sdt(false)?)),
            InlineKind::Inserted | InlineKind::Deleted => {
                let feature = if inline_kind(name.local()) == InlineKind::Inserted {
                    "w:ins"
                } else {
                    "w:del"
                };
                self.record(
                    feature,
                    SupportStatus::Partial,
                    Some("tracked change container flattened".to_owned()),
                    Some(location),
                );
                let mut children = self.parse_inline_children()?;
                out.append(&mut children);
            }
            InlineKind::Ignored => self.skip_element()?,
            InlineKind::Opaque => {
                self.record_foreign(name);
                out.push(Inline::Opaque(self.capture_opaque_inline(name, attrs)));
                self.skip_element()?;
            }
        }
        Ok(())
    }

    /// Parses a run (`w:r`); its start element has been consumed.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_run(&mut self, _attrs: &[Attr]) -> Result<Run> {
        let location = self.location();
        self.enter()?;
        let mut props = crate::model::props::RunProperties::default();
        let mut content = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match run_kind(name.local()) {
                        RunKind::Text | RunKind::DeletedText => {
                            content.push(RunContent::Text(self.parse_text_element(&attrs)?));
                        }
                        RunKind::Tab => {
                            content.push(RunContent::Tab);
                            self.skip_element()?;
                        }
                        RunKind::Break => {
                            let kind = wml_attr(&attrs, "type")
                                .and_then(BreakKind::from_strict)
                                .unwrap_or(BreakKind::TextWrapping);
                            content.push(RunContent::Break(kind));
                            self.skip_element()?;
                        }
                        RunKind::CarriageReturn => {
                            content.push(RunContent::CarriageReturn);
                            self.skip_element()?;
                        }
                        RunKind::Drawing => {
                            content.push(RunContent::Drawing(self.parse_drawing()?));
                        }
                        RunKind::InstrText => {
                            let text = self.parse_text_content()?;
                            let computed = super::field_is_computed(&text);
                            self.record(
                                "w:instrText",
                                if computed {
                                    SupportStatus::Supported
                                } else {
                                    SupportStatus::Partial
                                },
                                Some(if computed {
                                    "computed at render time".to_owned()
                                } else {
                                    "field result taken from cache (not computed)".to_owned()
                                }),
                                Some(self.location()),
                            );
                            content.push(RunContent::InstrText(text));
                        }
                        RunKind::FieldChar => {
                            let kind = wml_attr(&attrs, "fldCharType")
                                .and_then(FieldCharType::from_strict)
                                .unwrap_or(FieldCharType::Begin);
                            let dirty = wml_attr(&attrs, "dirty").is_some_and(parse_on_off_value);
                            content.push(RunContent::FieldChar(FieldChar { kind, dirty }));
                            self.skip_element()?;
                        }
                        RunKind::FootnoteRef => {
                            let id = wml_attr(&attrs, "id").and_then(parse_u32).unwrap_or(0);
                            content.push(RunContent::FootnoteRef(id));
                            self.record(
                                "w:footnoteReference",
                                SupportStatus::Supported,
                                None,
                                Some(self.location()),
                            );
                            self.skip_element()?;
                        }
                        RunKind::EndnoteRef => {
                            let id = wml_attr(&attrs, "id").and_then(parse_u32).unwrap_or(0);
                            content.push(RunContent::EndnoteRef(id));
                            self.record(
                                "w:endnoteReference",
                                SupportStatus::Supported,
                                None,
                                Some(self.location()),
                            );
                            self.skip_element()?;
                        }
                        RunKind::CommentReference => {
                            // Without the anchor a w:commentRangeStart/End pair
                            // is a range that points at nothing: the comment text
                            // survives in comments.xml and no run refers to it,
                            // so Word shows a comment that is not attached to
                            // any word. The two halves of a range have to be
                            // written together or neither should be.
                            let id = wml_attr(&attrs, "id").and_then(parse_u32).unwrap_or(0);
                            content.push(RunContent::CommentReference(id));
                            self.record(
                                "w:commentReference",
                                SupportStatus::Supported,
                                Some("the comment body is carried in word/comments.xml".to_owned()),
                                Some(self.location()),
                            );
                            self.skip_element()?;
                        }
                        RunKind::NoteRef => {
                            content.push(RunContent::NoteRef);
                            self.skip_element()?;
                        }
                        RunKind::Symbol => {
                            if let (Some(font), Some(code)) =
                                (wml_attr(&attrs, "font"), wml_attr(&attrs, "char"))
                            {
                                if let Some(character) =
                                    u32::from_str_radix(code.trim_start_matches('F'), 16)
                                        .ok()
                                        .and_then(char::from_u32)
                                {
                                    content.push(RunContent::Symbol(Symbol {
                                        font: self.intern(font),
                                        character,
                                    }));
                                }
                            }
                            self.skip_element()?;
                        }
                        RunKind::LastRenderedPageBreak => {
                            content.push(RunContent::LastRenderedPageBreak);
                            self.skip_element()?;
                        }
                        RunKind::NoBreakHyphen => {
                            content.push(RunContent::NoBreakHyphen);
                            self.skip_element()?;
                        }
                        RunKind::SoftHyphen => {
                            content.push(RunContent::SoftHyphen);
                            self.skip_element()?;
                        }
                        RunKind::RunProperties => {
                            props = self.parse_run_properties()?;
                        }
                        RunKind::Separator => {
                            let feature = feature_id_for(&name);
                            self.record(
                                &feature,
                                SupportStatus::Supported,
                                None,
                                Some(self.location()),
                            );
                            self.skip_element()?;
                        }
                        RunKind::Opaque => {
                            self.record_foreign(&name);
                            content.push(RunContent::Opaque(
                                self.capture_opaque_inline(&name, &attrs),
                            ));
                            self.skip_element()?;
                        }
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of run")),
            }
        }
        self.leave();
        Ok(Run {
            props,
            content,
            location,
        })
    }

    /// Parses a `w:t`/`w:delText` element, honouring `xml:space`.
    fn parse_text_element(&mut self, attrs: &[Attr]) -> Result<TextNode> {
        let space = attr_in_ns(attrs, XML_NS, "space")
            .and_then(Space::from_strict)
            .unwrap_or_default();
        let text = self.parse_text_content()?;
        Ok(TextNode { text, space })
    }

    /// Concatenates the text children of the current element.
    fn parse_text_content(&mut self) -> Result<String> {
        self.enter()?;
        let mut text = String::new();
        loop {
            match self.next_event()? {
                XmlEvent::Text(chunk) | XmlEvent::CData(chunk) => text.push_str(&chunk),
                XmlEvent::StartElement { name, .. } => {
                    self.record_foreign(&name);
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Eof => return Err(self.invalid("unexpected end of text")),
            }
        }
        self.leave();
        Ok(text)
    }

    /// Parses a hyperlink (`w:hyperlink`).
    fn parse_hyperlink(&mut self, attrs: &[Attr]) -> Result<Hyperlink> {
        let location = self.location();
        let rel_id = attr_in_ns(attrs, RELS_STRICT_NS, "id").map(RelId::new);
        let anchor = wml_attr(attrs, "anchor").map(|value| self.intern(value));
        let tooltip = wml_attr(attrs, "tooltip").map(|value| self.intern(value));
        let inlines = self.parse_inline_children()?;
        Ok(Hyperlink {
            rel_id,
            anchor,
            tooltip,
            inlines,
            location,
        })
    }

    /// Parses a simple field (`w:fldSimple`).
    fn parse_fld_simple(&mut self, attrs: &[Attr]) -> Result<Field> {
        let location = self.location();
        let instruction = wml_attr(attrs, "instr").map(|value| self.intern(value));
        let computed = instruction.as_deref().is_some_and(super::field_is_computed);
        self.record(
            "w:fldSimple",
            if computed {
                SupportStatus::Supported
            } else {
                SupportStatus::Partial
            },
            Some(if computed {
                "computed at render time".to_owned()
            } else {
                "field result taken from cache (not computed)".to_owned()
            }),
            Some(location.clone()),
        );
        let inlines = self.parse_inline_children()?;
        Ok(Field {
            instruction,
            inlines,
            location,
        })
    }

    /// Parses an alternative-format chunk (`w:altChunk`).
    fn parse_alt_chunk(&mut self, attrs: &[Attr]) -> Result<AltChunkInfo> {
        let location = self.location();
        let rel_id = attr_in_ns(attrs, RELS_STRICT_NS, "id").map(RelId::new);
        let content_type = rel_id
            .as_ref()
            .and_then(|id| self.resolve_relationship_target(id.as_str()))
            .and_then(|part| self.content_type(&part));
        self.record(
            "w:altChunk",
            SupportStatus::Unsupported,
            Some("alternative-format chunks are not embedded".to_owned()),
            Some(location.clone()),
        );
        self.skip_element()?;
        Ok(AltChunkInfo {
            rel_id,
            content_type,
            location,
        })
    }

    /// Parses a structured document tag (`w:sdt`).
    pub(crate) fn parse_sdt(&mut self, is_block: bool) -> Result<SdtContainer> {
        let location = self.location();
        self.enter()?;
        let mut tag = None;
        let mut alias = None;
        let mut id = None;
        let mut placeholder = None;
        let mut showing_placeholder = false;
        let mut blocks = Vec::new();
        let mut inlines = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "sdtPr" => {
                            let props = self.parse_sdt_properties()?;
                            tag = props.0.or(tag);
                            alias = props.1.or(alias);
                            id = props.2.or(id);
                            placeholder = props.3.or(placeholder);
                            showing_placeholder |= props.4;
                        }
                        "sdtContent" => {
                            if is_block {
                                let (mut found, _) = self.parse_block_children()?;
                                blocks.append(&mut found);
                            } else {
                                let mut found = self.parse_inline_children()?;
                                inlines.append(&mut found);
                            }
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => {
                    return Err(self.invalid("unexpected end of structured document tag"))
                }
            }
        }
        self.leave();
        Ok(SdtContainer {
            tag,
            alias,
            id,
            placeholder,
            showing_placeholder,
            blocks,
            inlines,
            location,
        })
    }

    /// Parses `w:sdtPr`, returning `(tag, alias, id, placeholder, showing)`.
    fn parse_sdt_properties(
        &mut self,
    ) -> Result<(
        Option<Arc<str>>,
        Option<Arc<str>>,
        Option<Arc<str>>,
        Option<Arc<str>>,
        bool,
    )> {
        self.enter()?;
        let mut tag = None;
        let mut alias = None;
        let mut id = None;
        let mut placeholder = None;
        let mut showing = false;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    match name.local() {
                        "tag" => tag = val_attr(&attrs).map(|value| self.intern(value)),
                        "alias" => alias = val_attr(&attrs).map(|value| self.intern(value)),
                        "id" => id = val_attr(&attrs).map(|value| self.intern(value)),
                        "showingPlcHdr" => showing = true,
                        "placeholder" => {
                            placeholder = Some(Arc::from("<placeholder>"));
                        }
                        _ => {}
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of sdt properties")),
            }
        }
        self.leave();
        Ok((tag, alias, id, placeholder, showing))
    }

    /// Captures an unknown block element as an [`OpaqueBlock`].
    fn capture_opaque_block(&mut self, name: &QName, attrs: &[Attr]) -> OpaqueBlock {
        let namespace = match &name.ns {
            None => self.intern(""),
            Some(ns) => self.intern(ns.as_str()),
        };
        OpaqueBlock {
            namespace,
            local: self.intern(name.local()),
            attributes: self.capture_attrs(attrs),
            location: self.location(),
        }
    }

    /// Captures an unknown inline element as an [`OpaqueInline`].
    pub(crate) fn capture_opaque_inline(&mut self, name: &QName, attrs: &[Attr]) -> OpaqueInline {
        let namespace = match &name.ns {
            None => self.intern(""),
            Some(ns) => self.intern(ns.as_str()),
        };
        OpaqueInline {
            namespace,
            local: self.intern(name.local()),
            attributes: self.capture_attrs(attrs),
            location: self.location(),
        }
    }

    /// Records an unknown element as unsupported (MCE is recorded as ignored).
    ///
    /// Elements that ISO/IEC 29500-1 defines but that provably cannot change
    /// this renderer's output are reclassified by [`harmless_element`] so that
    /// real Word/LibreOffice Strict files are not reported as blocked on a
    /// technicality (`STAGE-5C-REWORK-1` D2). The information is kept: the
    /// feature is still listed, with the reason why it does not apply.
    pub(crate) fn record_foreign(&mut self, name: &QName) {
        let feature = feature_id_for(name);
        let is_mce = name
            .ns
            .as_ref()
            .is_some_and(|ns| ns.as_str() == super::MCE_NS);
        let (status, message) = if is_mce {
            (
                SupportStatus::Ignored,
                Some("markup compatibility processing is Stage 6".to_owned()),
            )
        } else if let Some((status, reason)) = harmless_element(name) {
            (status, Some(reason.to_owned()))
        } else {
            (SupportStatus::Unsupported, None)
        };
        self.record(&feature, status, message, Some(self.location()));
    }

    /// Captures attributes as interned name/value pairs.
    fn capture_attrs(&mut self, attrs: &[Attr]) -> Vec<(Arc<str>, Arc<str>)> {
        attrs
            .iter()
            .map(|attr| {
                let name = attr.name.to_string();
                (self.intern(&name), self.intern(&attr.value))
            })
            .collect()
    }
}

/// Classifies a standard element that this renderer does not consume, so that
/// [`PartParser::record_foreign`] does not turn it into an `unsupported`
/// blocker on a real Word/LibreOffice Strict file
/// (`STAGE-5C-REWORK-1` D2).
///
/// Returns `(status, reason)`. `Ignored` means the element provably cannot
/// change the rendered output; `Partial` means it could in principle but this
/// renderer does not model it. The reason is kept in the Feature Report, so no
/// information is lost either way.
fn harmless_element(name: &QName) -> Option<(crate::model::support::SupportStatus, &'static str)> {
    use crate::model::support::SupportStatus;
    let ns = name.ns.as_ref().map_or("", |uri| uri.as_str());
    match (ns, name.local()) {
        // `settings.xml`: proofing, revision, custom-XML and compatibility
        // bookkeeping. None of it reaches the rendered page.
        (
            crate::WML_STRICT_NS,
            "characterSpacingControl"
            | "noPunctuationKerning"
            | "printTwoOnOne"
            | "strictFirstAndLastChars"
            | "noLineBreaksAfter"
            | "noLineBreaksBefore"
            | "savePreviewPicture"
            | "doNotValidateAgainstSchema"
            | "saveInvalidXml"
            | "ignoreMixedContent"
            | "alwaysShowPlaceholderText"
            | "doNotDemarcateInvalidXml"
            | "saveXmlDataOnly"
            | "useXSLTWhenSaving"
            | "saveThroughXslt"
            | "showXMLTags"
            | "alwaysMergeEmptyNamespace"
            | "updateFields"
            | "hdrShapeDefaults"
            | "doNotIncludeSubdocsInStats"
            | "doNotAutoCompressPictures"
            | "forceUpgrade"
            | "captions"
            | "readModeInkLockDown"
            | "smartTagType",
        ) => Some((
            SupportStatus::Ignored,
            "editing/compatibility setting without effect on the rendered page",
        )),
        (crate::WML_STRICT_NS, "rsids" | "rsid") => {
            Some((SupportStatus::Ignored, "revision save ids carry no content"))
        }
        (crate::WML_STRICT_NS, "clrSchemeMapping") => Some((
            SupportStatus::Ignored,
            "theme colour mapping is resolved directly against the theme part",
        )),
        (crate::WML_STRICT_NS, "docVars" | "attachedSchema") => Some((
            SupportStatus::Ignored,
            "custom XML/doc-variable data does not affect the rendered page",
        )),
        (crate::WML_STRICT_NS, "shapeDefaults") => Some((
            SupportStatus::Ignored,
            "document-wide default shape properties apply only to shapes that omit them",
        )),
        (crate::WML_STRICT_NS | crate::MATH_STRICT_NS, "mathPr") => Some((
            SupportStatus::Partial,
            "document math defaults (math font, bracket/break rules) are not applied; \
             the paragraph font and the Word default metrics drive the formula",
        )),
        // Microsoft extension markup in `settings.xml`: document identity and
        // co-authoring bookkeeping (5B rework accepts these namespaces).
        (
            super::W14_NS | super::W15_NS,
            "docId"
            | "conflictMode"
            | "discardImageEditingData"
            | "defaultImageDpi"
            | "chartTrackingRefBased",
        ) => Some((
            SupportStatus::Ignored,
            "Microsoft extension setting without effect on the rendered page",
        )),
        // `theme1.xml`: object/extension defaults and extra colour schemes.
        // Only the colour and font schemes reach the renderer.
        (
            crate::DRAWINGML_STRICT_NS,
            "objectDefaults" | "extraClrSchemeLst" | "extLst" | "custClrLst" | "ext",
        ) => Some((
            SupportStatus::Ignored,
            "theme default/extension data does not affect the rendered page",
        )),
        _ => None,
    }
}

/// Extracts revision identifiers from `w:p`/`w:r` attributes.
fn rsids_from_attrs(attrs: &[Attr]) -> Rsids {
    Rsids {
        run: wml_attr(attrs, "rsidR").map(Arc::from),
        run_default: wml_attr(attrs, "rsidRDefault").map(Arc::from),
        paragraph: wml_attr(attrs, "rsidP").map(Arc::from),
        deleted: wml_attr(attrs, "rsidDel").map(Arc::from),
        table_row: wml_attr(attrs, "rsidTr").map(Arc::from),
    }
}

/// Parses a boolean attribute value.
fn parse_on_off_value(value: &str) -> bool {
    matches!(value, "true" | "on" | "1")
}
