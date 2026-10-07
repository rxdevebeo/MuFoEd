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
    Bookmark, BookmarkId, CommentId, Directional, DirectionalKind, DirectionalVal, Field,
    FieldChar, Hyperlink, Inline, OpaqueInline, Run, RunContent, Symbol, TextNode,
};
use crate::model::props::{ParagraphProperties, RunProperties, Section};
use crate::model::revision::{Revision, RevisionKind};
use crate::model::support::SupportStatus;
use crate::model::values::{BreakKind, FieldCharType, Rsids, Space};
use crate::RELS_STRICT_NS;

use super::dispatch::{body_kind, inline_kind, run_kind, BodyKind, InlineKind, RunKind};
use super::{
    attr_in_ns, feature_id_for, is_wml, parse_u32, val_attr, wml_attr, PartParser, MCE_NS, W14_NS,
    XML_NS,
};

/// Fields of `w:sdtPr` the model keeps.
#[derive(Default)]
pub(crate) struct ParsedSdtPr {
    /// `w:tag`.
    pub tag: Option<Arc<str>>,
    /// `w:alias`.
    pub alias: Option<Arc<str>>,
    /// `w:id`.
    pub id: Option<Arc<str>>,
    /// `w:placeholder/w:docPart`.
    pub placeholder: Option<Arc<str>>,
    /// `w:showingPlcHdr`.
    pub showing_placeholder: bool,
    /// `w:rPr` of the placeholder, including half-point `w:sz`.
    pub run_props: Option<RunProperties>,
}

impl PartParser<'_> {
    /// Parses the `w:document` root and its `w:body`.
    ///
    /// Sections are not returned here: AUD-40 collects them after the body is
    /// built, with the same document-order walk used for header/footer sync.
    pub(crate) fn parse_document_root(&mut self) -> Result<Body> {
        self.nested(|parser| {
            parser.expect_root("document")?;

            let mut body = Body::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if name.local() == "body" && is_wml(&name) {
                            // Bare `w:sectPr` as a body child is the final section.
                            parser.capture_body_section = true;
                            body.blocks = parser.parse_block_children()?;
                            parser.capture_body_section = false;
                        } else if name.local() == "background" && is_wml(&name) {
                            parser.record(
                                "w:background",
                                SupportStatus::Partial,
                                None,
                                Some(parser.location()),
                            );
                            parser.skip_element()?;
                        } else {
                            parser.record_foreign(&name);
                            let _ = attrs;
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of document part")),
                }
            }
            parser.expect_end_of_part()?;
            Ok(body)
        })
    }

    /// Parses block-level children until the current element's end.
    pub(crate) fn parse_block_children(&mut self) -> Result<Vec<Block>> {
        self.nested(|parser| {
            let mut blocks = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_mce(&name) && name.local() == "AlternateContent" {
                            parser.parse_mce_alternate_content_blocks(&attrs, &mut blocks)?;
                            continue;
                        }
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        parser.parse_block_element_into(&name, &attrs, &mut blocks)?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of block content")),
                }
            }
            Ok(blocks)
        })
    }

    /// Dispatches one block element (start already consumed) into `blocks`.
    ///
    /// Every block-level element in the document arrives here - body children,
    /// table-cell children, the children of a structured document tag - so this
    /// is the one place where block nesting can be counted for all of them
    /// without each parser remembering to.
    pub(crate) fn parse_block_element_into(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        blocks: &mut Vec<Block>,
    ) -> Result<()> {
        if PartParser::counts_block_nesting(name) {
            return self.nested_block(|parser| parser.dispatch_block_element(name, attrs, blocks));
        }
        self.dispatch_block_element(name, attrs, blocks)
    }

    /// The body of [`parse_block_element_into`](Self::parse_block_element_into),
    /// with the nesting count already opened by the caller.
    fn dispatch_block_element(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        blocks: &mut Vec<Block>,
    ) -> Result<()> {
        // A text box is a paragraph. This frame stays live for every nested box,
        // so the other arms — tables, sections, revisions — live in a function
        // that a paragraph does not call. Debug builds reserve a slot for every
        // local of a function, and six of those combined frames have to fit in
        // the 1 MiB stack the hostile suite uses.
        if body_kind(name.local()) == BodyKind::Paragraph {
            blocks.push(Block::Paragraph(self.parse_paragraph(attrs)?));
            return Ok(());
        }
        self.dispatch_block_element_rest(name, attrs, blocks)
    }

    /// Block elements other than `w:p`.
    ///
    /// Split from [`dispatch_block_element`](Self::dispatch_block_element) so a
    /// nested text box does not keep this match's locals on the stack.
    fn dispatch_block_element_rest(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        blocks: &mut Vec<Block>,
    ) -> Result<()> {
        match body_kind(name.local()) {
            BodyKind::Paragraph => {
                blocks.push(Block::Paragraph(self.parse_paragraph(attrs)?));
            }
            BodyKind::Table => blocks.push(Block::Table(self.parse_table()?)),
            BodyKind::Sdt => blocks.push(Block::SdtBlock(self.parse_sdt(true)?)),
            BodyKind::AltChunk => blocks.push(Block::AltChunk(self.parse_alt_chunk(attrs)?)),
            BodyKind::Section => {
                let location = self.location();
                record_unmodelled_revision_attrs(self, "w:sectPr", attrs);
                let properties = self.parse_section_properties()?;
                // Only a bare `w:sectPr` at body top level is the final section.
                // Nested block depth covers sdt/table; revisions clear the flag below.
                if self.capture_body_section && self.block_depth == 0 {
                    self.body_section = Some(Section {
                        properties,
                        location,
                    });
                }
            }
            BodyKind::Inserted | BodyKind::Deleted | BodyKind::MovedTo | BodyKind::MovedFrom => {
                let kind = RevisionKind::from_local(name.local()).unwrap_or(RevisionKind::Insert);
                let revision = self.parse_revision_attrs(kind, attrs);
                self.record(
                    kind.feature_id(),
                    SupportStatus::Supported,
                    None,
                    Some(self.location()),
                );
                let was_capture = self.capture_body_section;
                self.capture_body_section = false;
                let mut children = self.parse_block_children()?;
                self.capture_body_section = was_capture;
                stamp_revision_on_blocks(&mut children, &revision);
                blocks.append(&mut children);
            }
            BodyKind::Transparent => {
                // AUD-42: customXml / smartTag — keep content, drop the wrapper.
                let feature = feature_id_for(name);
                self.record(
                    &feature,
                    SupportStatus::Partial,
                    Some("wrapper dropped, content kept".to_owned()),
                    Some(self.location()),
                );
                let was_capture = self.capture_body_section;
                self.capture_body_section = false;
                let mut children = self.parse_block_children()?;
                self.capture_body_section = was_capture;
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
        record_unmodelled_revision_attrs(self, "w:p", attrs);
        let location = self.location();
        self.nested(|parser| {
            let mut props = ParagraphProperties::default();
            let mut mark_revision = None;
            let mut inlines = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if name.local() == "pPr" && is_wml(&name) {
                            let (parsed, revision) = parser.parse_paragraph_properties()?;
                            props = parsed;
                            mark_revision = revision;
                        } else if crate::parse::is_math(&name) {
                            parser.parse_math_into(&name, &mut inlines)?;
                        } else if is_mce(&name) && name.local() == "AlternateContent" {
                            // AUD-50: before the foreign/`!is_wml` path.
                            parser.parse_mce_alternate_content_inlines(&attrs, &mut inlines)?;
                        } else if is_wml(&name) {
                            parser.parse_inline_into(&name, &attrs, &mut inlines)?;
                        } else {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of paragraph")),
                }
            }

            Ok(Paragraph {
                props,
                inlines,
                rsids: rsids_from_attrs(attrs),
                revision: mark_revision,
                para_id: attr_in_ns(attrs, W14_NS, "paraId").map(ParaId::new),
                text_id: attr_in_ns(attrs, W14_NS, "textId").map(TextId::new),
                location,
            })
        })
    }

    /// Parses inline children until the current element's end.
    fn parse_inline_children(&mut self) -> Result<Vec<Inline>> {
        self.nested(|parser| {
            let mut out = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if crate::parse::is_math(&name) {
                            parser.parse_math_into(&name, &mut out)?;
                        } else if is_mce(&name) && name.local() == "AlternateContent" {
                            // AUD-50: resolve before the foreign/`!is_wml` path.
                            parser.parse_mce_alternate_content_inlines(&attrs, &mut out)?;
                        } else if is_wml(&name) {
                            parser.parse_inline_into(&name, &attrs, &mut out)?;
                        } else {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of inline content")),
                }
            }
            Ok(out)
        })
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
        // `w:r` is the text-box path. Bookmarks, comments and the other arms
        // reserve their locals for the whole function in a debug build, so they
        // are not in this frame.
        if inline_kind(name.local()) == InlineKind::Run {
            out.push(Inline::Run(self.parse_run(attrs)?));
            return Ok(());
        }
        self.parse_inline_into_rest(name, attrs, out)
    }

    /// Paragraph children other than `w:r`.
    ///
    /// See [`parse_inline_into`](Self::parse_inline_into) for why this is separate.
    #[allow(clippy::too_many_lines)]
    fn parse_inline_into_rest(
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
                if let Some(id) = wml_attr(attrs, "id").and_then(parse_u32) {
                    out.push(Inline::FootnoteRef(id));
                    self.record(
                        "w:footnoteReference",
                        SupportStatus::Supported,
                        None,
                        Some(location),
                    );
                } else {
                    self.record(
                        "w:footnoteReference",
                        SupportStatus::Partial,
                        Some("footnote reference without a valid w:id was skipped".to_owned()),
                        Some(location),
                    );
                }
                self.skip_element()?;
            }
            InlineKind::EndnoteRef => {
                if let Some(id) = wml_attr(attrs, "id").and_then(parse_u32) {
                    out.push(Inline::EndnoteRef(id));
                    self.record(
                        "w:endnoteReference",
                        SupportStatus::Supported,
                        None,
                        Some(location),
                    );
                } else {
                    self.record(
                        "w:endnoteReference",
                        SupportStatus::Partial,
                        Some("endnote reference without a valid w:id was skipped".to_owned()),
                        Some(location),
                    );
                }
                self.skip_element()?;
            }
            InlineKind::Sdt => out.push(Inline::SdtInline(self.parse_sdt(false)?)),
            InlineKind::Inserted
            | InlineKind::Deleted
            | InlineKind::MovedTo
            | InlineKind::MovedFrom => {
                let kind = RevisionKind::from_local(name.local()).unwrap_or(RevisionKind::Insert);
                let revision = self.parse_revision_attrs(kind, attrs);
                self.record(
                    kind.feature_id(),
                    SupportStatus::Supported,
                    None,
                    Some(location),
                );
                let mut children = self.parse_inline_children()?;
                stamp_revision_on_inlines(&mut children, &revision);
                out.append(&mut children);
            }
            InlineKind::Transparent => {
                let feature = feature_id_for(name);
                self.record(
                    &feature,
                    SupportStatus::Partial,
                    Some("wrapper dropped, content kept".to_owned()),
                    Some(location),
                );
                let mut children = self.parse_inline_children()?;
                out.append(&mut children);
            }
            InlineKind::Dir | InlineKind::Bdo => {
                let kind = if inline_kind(name.local()) == InlineKind::Dir {
                    DirectionalKind::Dir
                } else {
                    DirectionalKind::Bdo
                };
                let val = wml_attr(attrs, "val")
                    .and_then(DirectionalVal::from_strict)
                    .unwrap_or(DirectionalVal::Ltr);
                let inlines = self.parse_inline_children()?;
                out.push(Inline::Directional(Directional {
                    kind,
                    val,
                    inlines,
                    location,
                }));
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
    pub(crate) fn parse_run(&mut self, attrs: &[Attr]) -> Result<Run> {
        record_unmodelled_revision_attrs(self, "w:r", attrs);
        let location = self.location();
        self.nested(|parser| {
            let mut props = crate::model::props::RunProperties::default();
            let mut content = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        parser.on_run_start(&name, &attrs, &mut props, &mut content)?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of run")),
                }
            }
            Ok(Run {
                props,
                content,
                revision: None,
                location,
            })
        })
    }

    /// One child of `w:r` on the text-box path.
    ///
    /// A drawing is parsed here, and every other child goes to
    /// [`on_run_start_rest`](Self::on_run_start_rest). The two cannot share a
    /// function: a debug build keeps every local of the function that is on the
    /// stack, and a text box is a drawing inside a run.
    fn on_run_start(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        props: &mut crate::model::props::RunProperties,
        content: &mut Vec<RunContent>,
    ) -> Result<()> {
        if is_wml(name) && run_kind(name.local()) == RunKind::Drawing {
            content.push(RunContent::Drawing(self.parse_drawing()?));
            return Ok(());
        }
        self.on_run_start_rest(name, attrs, props, content)
    }

    /// Run children other than `w:drawing`.
    #[allow(clippy::too_many_lines)]
    fn on_run_start_rest(
        &mut self,
        name: &QName,
        attrs: &[Attr],
        props: &mut crate::model::props::RunProperties,
        content: &mut Vec<RunContent>,
    ) -> Result<()> {
        if is_mce(name) && name.local() == "AlternateContent" {
            // AUD-50: Choice/Fallback before the `!is_wml` skip.
            self.parse_mce_alternate_content_run(attrs, content)?;
            return Ok(());
        }
        if !is_wml(name) {
            self.record_foreign(name);
            self.skip_element()?;
            return Ok(());
        }
        match run_kind(name.local()) {
            RunKind::Text | RunKind::DeletedText => {
                content.push(RunContent::Text(self.parse_text_element(attrs)?));
            }
            RunKind::Tab => {
                content.push(RunContent::Tab);
                self.skip_element()?;
            }
            RunKind::Break => {
                let kind = wml_attr(attrs, "type")
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
                let kind = wml_attr(attrs, "fldCharType")
                    .and_then(FieldCharType::from_strict)
                    .unwrap_or(FieldCharType::Begin);
                let dirty = wml_attr(attrs, "dirty").is_some_and(parse_on_off_value);
                content.push(RunContent::FieldChar(FieldChar { kind, dirty }));
                self.skip_element()?;
            }
            RunKind::FootnoteRef => {
                if let Some(id) = wml_attr(attrs, "id").and_then(parse_u32) {
                    content.push(RunContent::FootnoteRef(id));
                    self.record(
                        "w:footnoteReference",
                        SupportStatus::Supported,
                        None,
                        Some(self.location()),
                    );
                } else {
                    self.record(
                        "w:footnoteReference",
                        SupportStatus::Partial,
                        Some("footnote reference without a valid w:id was skipped".to_owned()),
                        Some(self.location()),
                    );
                }
                self.skip_element()?;
            }
            RunKind::EndnoteRef => {
                if let Some(id) = wml_attr(attrs, "id").and_then(parse_u32) {
                    content.push(RunContent::EndnoteRef(id));
                    self.record(
                        "w:endnoteReference",
                        SupportStatus::Supported,
                        None,
                        Some(self.location()),
                    );
                } else {
                    self.record(
                        "w:endnoteReference",
                        SupportStatus::Partial,
                        Some("endnote reference without a valid w:id was skipped".to_owned()),
                        Some(self.location()),
                    );
                }
                self.skip_element()?;
            }
            RunKind::Ptab => {
                // All three attributes are `use="required"`. An element
                // missing one cannot be written back without inventing a
                // value, so it is recorded and skipped rather than
                // defaulted - the schema names the defect, and a
                // defaulted one would not be named at all.
                let alignment = wml_attr(attrs, "alignment");
                let relative_to = wml_attr(attrs, "relativeTo");
                let leader = wml_attr(attrs, "leader");
                match (alignment, relative_to, leader) {
                    (Some(alignment), Some(relative_to), Some(leader)) => {
                        content.push(RunContent::Ptab {
                            alignment: self.intern(alignment),
                            relative_to: self.intern(relative_to),
                            leader: self.intern(leader),
                        });
                        self.record(
                            "w:ptab",
                            SupportStatus::Supported,
                            None,
                            Some(self.location()),
                        );
                    }
                    _ => {
                        self.record(
                            "w:ptab",
                            SupportStatus::Partial,
                            Some("w:ptab requires alignment, relativeTo and leader".to_owned()),
                            Some(self.location()),
                        );
                    }
                }
                self.skip_element()?;
            }
            RunKind::CommentReference => {
                // Without the anchor a w:commentRangeStart/End pair
                // is a range that points at nothing: the comment text
                // survives in comments.xml and no run refers to it,
                // so Word shows a comment that is not attached to
                // any word. The two halves of a range have to be
                // written together or neither should be.
                let id = wml_attr(attrs, "id").and_then(parse_u32).unwrap_or(0);
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
                // AUD-45: decode hex case-insensitively; only the
                // Private Use Area window `F000..=F0FF` is remapped.
                if let (Some(font), Some(code)) = (wml_attr(attrs, "font"), wml_attr(attrs, "char"))
                {
                    match decode_sym_char(code) {
                        Some(character) => {
                            content.push(RunContent::Symbol(Symbol {
                                font: self.intern(font),
                                character,
                            }));
                        }
                        None => {
                            self.record(
                                "w:sym",
                                SupportStatus::Unsupported,
                                Some(format!("invalid symbol code '{code}'")),
                                Some(self.location()),
                            );
                        }
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
                *props = self.parse_run_properties()?;
            }
            RunKind::Separator => {
                let feature = feature_id_for(name);
                self.record(
                    &feature,
                    SupportStatus::Supported,
                    None,
                    Some(self.location()),
                );
                self.skip_element()?;
            }
            RunKind::Opaque => {
                self.record_foreign(name);
                content.push(RunContent::Opaque(self.capture_opaque_inline(name, attrs)));
                self.skip_element()?;
            }
        }
        Ok(())
    }

    /// Reads `w:id` / `w:author` / `w:date` from a tracked-change wrapper.
    fn parse_revision_attrs(&mut self, kind: RevisionKind, attrs: &[Attr]) -> Revision {
        let id = wml_attr(attrs, "id").and_then(parse_u32).unwrap_or(0);
        let author = wml_attr(attrs, "author").map(|value| self.intern(value));
        let date = wml_attr(attrs, "date").map(|value| self.intern(value));
        Revision {
            kind,
            id,
            author,
            date,
        }
    }
}

/// Stamps a revision onto every run inside the given inlines (nested containers too).
fn stamp_revision_on_inlines(inlines: &mut [Inline], revision: &Revision) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                if run.revision.is_none() {
                    run.revision = Some(revision.clone());
                }
            }
            Inline::Hyperlink(link) => stamp_revision_on_inlines(&mut link.inlines, revision),
            Inline::Field(field) => stamp_revision_on_inlines(&mut field.inlines, revision),
            Inline::SdtInline(sdt) => stamp_revision_on_inlines(&mut sdt.inlines, revision),
            Inline::Directional(dir) => stamp_revision_on_inlines(&mut dir.inlines, revision),
            Inline::Drawing(_)
            | Inline::Break(_)
            | Inline::Tab
            | Inline::BookmarkStart(_)
            | Inline::BookmarkEnd(_)
            | Inline::CommentRangeStart(_)
            | Inline::CommentRangeEnd(_)
            | Inline::CommentReference(_)
            | Inline::FootnoteRef(_)
            | Inline::EndnoteRef(_)
            | Inline::Math(_)
            | Inline::MathParagraph(_)
            | Inline::Opaque(_) => {}
        }
    }
}

/// Stamps a revision onto runs (and paragraph marks) inside block children.
fn stamp_revision_on_blocks(blocks: &mut [Block], revision: &Revision) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                if paragraph.revision.is_none() {
                    paragraph.revision = Some(revision.clone());
                }
                stamp_revision_on_inlines(&mut paragraph.inlines, revision);
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        stamp_revision_on_blocks(&mut cell.blocks, revision);
                    }
                }
            }
            Block::SdtBlock(sdt) => stamp_revision_on_blocks(&mut sdt.blocks, revision),
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
}

// Re-open the impl block for the remaining helpers that lived after parse_run.
impl PartParser<'_> {
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
        self.nested(|parser| {
            let mut text = String::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::Text(chunk) | XmlEvent::CData(chunk) => text.push_str(&chunk),
                    XmlEvent::StartElement { name, .. } => {
                        parser.record_foreign(&name);
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of text")),
                }
            }
            Ok(text)
        })
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
        self.nested(|parser| {
            let mut tag = None;
            let mut alias = None;
            let mut id = None;
            let mut placeholder = None;
            let mut showing_placeholder = false;
            let mut run_props = None;
            let mut end_run_props = None;
            let mut blocks = Vec::new();
            let mut inlines = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "sdtPr" => {
                                let props = parser.parse_sdt_properties()?;
                                tag = props.tag.or(tag);
                                alias = props.alias.or(alias);
                                id = props.id.or(id);
                                placeholder = props.placeholder.or(placeholder);
                                showing_placeholder |= props.showing_placeholder;
                                run_props = props.run_props.or(run_props);
                            }
                            "sdtEndPr" => end_run_props = parser.parse_sdt_end_properties()?,
                            "sdtContent" => {
                                if is_block {
                                    let mut found = parser.parse_block_children()?;
                                    blocks.append(&mut found);
                                } else {
                                    let mut found = parser.parse_inline_children()?;
                                    inlines.append(&mut found);
                                }
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of structured document tag"))
                    }
                }
            }
            Ok(SdtContainer {
                tag,
                alias,
                id,
                placeholder,
                showing_placeholder,
                run_props,
                end_run_props,
                blocks,
                inlines,
                location,
            })
        })
    }

    /// Parses `w:sdtEndPr`, which holds the end marker's `w:rPr`.
    fn parse_sdt_end_properties(&mut self) -> Result<Option<crate::model::props::RunProperties>> {
        self.nested(|parser| {
            let mut props = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_wml(&name) && name.local() == "rPr" {
                            props = Some(parser.parse_run_properties()?);
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of structured document end"))
                    }
                }
            }
            Ok(props)
        })
    }

    /// Parses `w:sdtPr`.
    pub(crate) fn parse_sdt_properties(&mut self) -> Result<ParsedSdtPr> {
        self.nested(|parser| {
            let mut parsed = ParsedSdtPr::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        match name.local() {
                            "tag" => {
                                parsed.tag = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "alias" => {
                                parsed.alias = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "id" => {
                                parsed.id = val_attr(&attrs).map(|value| parser.intern(value));
                                parser.skip_element()?;
                            }
                            "showingPlcHdr" => {
                                parsed.showing_placeholder = true;
                                parser.skip_element()?;
                            }
                            "placeholder" => {
                                // `w:placeholder`'s own content is `w:docPart/@w:val`
                                // (`CT_Placeholder`); the element carries no value
                                // itself, so writing a sentinel string back as the
                                // placeholder could never round-trip to anything a
                                // reader actually asked for. Absent a `w:docPart`
                                // value, there is nothing to carry and the field
                                // stays `None`.
                                parsed.placeholder = parser.parse_sdt_placeholder()?;
                            }
                            "rPr" => {
                                parsed.run_props = Some(parser.parse_run_properties()?);
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of sdt properties")),
                }
            }
            Ok(parsed)
        })
    }

    /// Parses `w:sdtPr/w:placeholder`, returning `w:docPart/@w:val` when present.
    fn parse_sdt_placeholder(&mut self) -> Result<Option<Arc<str>>> {
        self.nested(|parser| {
            let mut doc_part = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if name.local() == "docPart" {
                            doc_part = val_attr(&attrs).map(|value| parser.intern(value));
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of sdt placeholder"))
                    }
                }
            }
            Ok(doc_part)
        })
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

    /// Records an unknown element as unsupported.
    ///
    /// Elements that ISO/IEC 29500-1 defines but that provably cannot change
    /// this renderer's output are reclassified by [`harmless_element`] so that
    /// real Word/LibreOffice Strict files are not reported as blocked on a
    /// technicality (`STAGE-5C-REWORK-1` D2). The information is kept: the
    /// feature is still listed, with the reason why it does not apply.
    ///
    /// `mc:AlternateContent` is handled by [`Self::parse_mce_alternate_content_inlines`]
    /// / [`Self::parse_mce_alternate_content_run`] (AUD-50) before this path.
    pub(crate) fn record_foreign(&mut self, name: &QName) {
        let feature = feature_id_for(name);
        let (status, message) = if let Some((status, reason)) = harmless_element(name) {
            (status, Some(reason.to_owned()))
        } else {
            (SupportStatus::Unsupported, None)
        };
        self.record(&feature, status, message, Some(self.location()));
    }

    /// Resolves `mc:AlternateContent` into body/block content (AUD-50).
    fn parse_mce_alternate_content_blocks(
        &mut self,
        attrs: &[Attr],
        blocks: &mut Vec<Block>,
    ) -> Result<()> {
        self.parse_mce_alternate_content(attrs, |parser, _branch_attrs| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_mce(&name) && name.local() == "AlternateContent" {
                            parser.parse_mce_alternate_content_blocks(&attrs, blocks)?;
                        } else if is_wml(&name) {
                            parser.parse_block_element_into(&name, &attrs, blocks)?;
                        } else {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of mc:Choice block content"))
                    }
                }
            }
            Ok(())
        })
    }

    /// Resolves `mc:AlternateContent` into paragraph inlines (AUD-50).
    fn parse_mce_alternate_content_inlines(
        &mut self,
        attrs: &[Attr],
        out: &mut Vec<Inline>,
    ) -> Result<()> {
        self.parse_mce_alternate_content(attrs, |parser, branch_attrs| {
            let mut nested = parser.parse_inline_children_flat()?;
            let _ = branch_attrs;
            out.append(&mut nested);
            Ok(())
        })
    }

    /// Resolves `mc:AlternateContent` into run content (AUD-50).
    fn parse_mce_alternate_content_run(
        &mut self,
        attrs: &[Attr],
        content: &mut Vec<RunContent>,
    ) -> Result<()> {
        self.parse_mce_alternate_content(attrs, |parser, _branch_attrs| {
            // Chosen branch may contain `w:drawing` / `w:t` / nested math, etc.
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            match run_kind(name.local()) {
                                RunKind::Drawing => {
                                    content.push(RunContent::Drawing(parser.parse_drawing()?));
                                }
                                RunKind::Text | RunKind::DeletedText => {
                                    content
                                        .push(RunContent::Text(parser.parse_text_element(&attrs)?));
                                }
                                _ => {
                                    parser.skip_element()?;
                                }
                            }
                        } else if crate::parse::is_math(&name) {
                            // Math inside a run-level Choice is uncommon; skip with a record.
                            parser.record(
                                &feature_id_for(&name),
                                SupportStatus::Partial,
                                Some("math inside mc:Choice run branch".to_owned()),
                                Some(parser.location()),
                            );
                            parser.skip_element()?;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of mc:Choice content"))
                    }
                }
            }
            Ok(())
        })
    }

    /// Shared `mc:AlternateContent` walker: ProcessChoice against
    /// [`crate::SUPPORTED_MCE_NAMESPACES`].
    pub(super) fn parse_mce_alternate_content(
        &mut self,
        attrs: &[Attr],
        mut take_branch: impl FnMut(&mut PartParser<'_>, &[Attr]) -> Result<()>,
    ) -> Result<()> {
        let location = self.location();
        let mut xmlns = collect_xmlns(attrs);
        self.record(
            "mc:AlternateContent",
            SupportStatus::Supported,
            Some("resolved per ProcessChoice".to_owned()),
            Some(location),
        );
        self.nested(|parser| {
            let mut chosen = false;
            let mut fallback_attrs: Option<Vec<Attr>> = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_mce(&name) {
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "Choice" => {
                                merge_xmlns(&mut xmlns, &attrs);
                                let requires = attrs
                                    .iter()
                                    .find(|attr| attr.name.local() == "Requires")
                                    .map_or("", |attr| attr.value.as_str());
                                if !chosen && mce_requires_understood(requires, &xmlns) {
                                    chosen = true;
                                    take_branch(parser, &attrs)?;
                                } else {
                                    parser.skip_element()?;
                                }
                            }
                            "Fallback" => {
                                if chosen {
                                    parser.skip_element()?;
                                } else {
                                    // Defer: we may still see a later Choice (schema order is
                                    // Choice* Fallback?, so Fallback is last — take it now).
                                    let _ = fallback_attrs.replace(attrs.clone());
                                    take_branch(parser, &attrs)?;
                                    chosen = true;
                                }
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of mc:AlternateContent"))
                    }
                }
            }
            let _ = fallback_attrs;
            Ok(())
        })
    }

    /// Like [`Self::parse_inline_children`] but for a branch that is already open.
    fn parse_inline_children_flat(&mut self) -> Result<Vec<Inline>> {
        let mut out = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if crate::parse::is_math(&name) {
                        self.parse_math_into(&name, &mut out)?;
                    } else if is_mce(&name) && name.local() == "AlternateContent" {
                        self.parse_mce_alternate_content_inlines(&attrs, &mut out)?;
                    } else if is_wml(&name) {
                        self.parse_inline_into(&name, &attrs, &mut out)?;
                    } else {
                        self.record_foreign(&name);
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => {
                    return Err(self.invalid("unexpected end of mc:Choice inline content"))
                }
            }
        }
        Ok(out)
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

fn is_mce(name: &QName) -> bool {
    name.ns.as_ref().is_some_and(|ns| ns.as_str() == MCE_NS)
}

fn collect_xmlns(attrs: &[Attr]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    merge_xmlns(&mut map, attrs);
    map
}

fn merge_xmlns(map: &mut std::collections::HashMap<String, String>, attrs: &[Attr]) {
    for attr in attrs {
        let local = attr.name.local();
        if local == "xmlns" {
            // default xmlns — unused for Requires prefixes
            continue;
        }
        if attr.name.prefix.as_deref() == Some("xmlns") {
            map.insert(local.to_owned(), attr.value.clone());
        }
    }
}

fn mce_requires_understood(
    requires: &str,
    xmlns: &std::collections::HashMap<String, String>,
) -> bool {
    let prefixes: Vec<&str> = requires.split_ascii_whitespace().collect();
    if prefixes.is_empty() {
        return false;
    }
    prefixes.iter().all(|prefix| {
        let uri = xmlns
            .get(*prefix)
            .map(String::as_str)
            .or_else(|| known_mce_prefix_uri(prefix));
        uri.is_some_and(|uri| crate::SUPPORTED_MCE_NAMESPACES.contains(&uri))
    })
}

fn known_mce_prefix_uri(prefix: &str) -> Option<&'static str> {
    match prefix {
        "wps" => Some(crate::MS_WORD_PROCESSING_SHAPE_NS),
        "wpg" => Some(crate::MS_WORD_PROCESSING_GROUP_NS),
        "m" => Some(crate::MATH_STRICT_NS),
        _ => None,
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

/// Revision attributes the model does not store.
///
/// `w:p` keeps `rsidR`, `rsidRDefault`, `rsidP` and `rsidDel`. `rsidRPr` has
/// no field, and a run stores none of them. Skipping one without a feature id
/// is a silent inventory loss.
fn record_unmodelled_revision_attrs(parser: &mut PartParser<'_>, element: &str, attrs: &[Attr]) {
    const MODELLED_ON_PARAGRAPH: &[&str] = &["rsidR", "rsidRDefault", "rsidP", "rsidDel"];
    for attr in attrs {
        let local = attr.name.local();
        let modelled = element == "w:p" && MODELLED_ON_PARAGRAPH.contains(&local);
        if modelled || !local.starts_with("rsid") {
            continue;
        }
        parser.record(
            &format!("{element}@{local}"),
            crate::model::support::SupportStatus::Partial,
            Some("revision id is not written back".to_owned()),
            Some(parser.location()),
        );
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

/// Decodes `w:sym/@w:char` (AUD-45).
///
/// Hex is case-insensitive. Codes in `0xF000..=0xF0FF` map into the low byte
/// (Private Use Area window Word uses for Symbol fonts); other codes stay as-is.
/// The result must be a valid XML 1.0 character.
fn decode_sym_char(code: &str) -> Option<char> {
    let value = u32::from_str_radix(code, 16).ok()?;
    let codepoint = if (0xF000..=0xF0FF).contains(&value) {
        value - 0xF000
    } else {
        value
    };
    let character = char::from_u32(codepoint)?;
    strict_ooxml_core::xml::escape::is_xml_char(character).then_some(character)
}
