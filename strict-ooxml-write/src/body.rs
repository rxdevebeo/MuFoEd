//! Block- and inline-level serialization: paragraphs, tables, runs.

use strict_ooxml_wml::model::block::{
    Block, Paragraph, SdtContainer, SdtProperties, Table, TableCell, TableRow,
};
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
use strict_ooxml_wml::model::values::{BreakKind, Space};

use crate::ctx::Ctx;
use crate::props::{cell_properties, paragraph_properties, row_properties, table_properties};
use crate::xml::{WriteError, XmlWriter};

/// Checks the model against the writer's block-nesting budget.
///
/// The walk itself lives in the crate that owns the model
/// ([`strict_ooxml_wml::nesting`]), because the reader counts the same
/// containers on the way in and three implementations of one rule would
/// eventually disagree - and a writer that is more permissive than its reader
/// produces a package the reader refuses.
///
/// A separate walk rather than a counter inside [`blocks`]: the serializer
/// returns `()` and its arms are twenty deep, so threading a `Result` through
/// them would touch every call site to add a check the value cannot reach.
///
/// # Errors
///
/// Returns [`WriteError::Nesting`] when `blocks` nests a container deeper than
/// its budget in `limits`.
pub fn check_block_nesting(
    blocks: &[Block],
    limits: &strict_ooxml_core::limits::ResourceLimits,
) -> Result<(), WriteError> {
    strict_ooxml_wml::nesting::check_blocks(blocks, limits).map_err(|exceeded| {
        WriteError::Nesting {
            kind: exceeded.kind,
            limit: exceeded.limit,
            actual: exceeded.actual,
        }
    })
}

/// Writes a sequence of block-level items.
pub fn blocks(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, blocks: &[Block]) {
    for block in blocks {
        block_item(ctx, xml, block);
    }
}

/// Writes one block-level item.
pub fn block_item(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, block: &Block) {
    match block {
        Block::Paragraph(paragraph) => paragraph_element(ctx, xml, paragraph),
        Block::Table(table) => table_element(ctx, xml, table),
        Block::SdtBlock(sdt) => {
            ctx.report_unsupported("w:sdt", "block-level content control", &sdt.location);
            blocks(ctx, xml, &sdt.blocks);
        }
        Block::AltChunk(info) => {
            ctx.report_unsupported("w:altChunk", "alternative format chunk", &info.location);
        }
        Block::Opaque(opaque) => {
            ctx.report_unsupported(
                &opaque.feature_id(),
                "block-level element kept in the model but not serializable",
                &opaque.location,
            );
        }
    }
}

/// Writes `w:p`.
pub fn paragraph_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, paragraph: &Paragraph) {
    xml.start("w:p");
    // ADR-0014: a Strict package carries no extension-namespace attributes, and
    // `w14` is an extension - it appears zero times in ECMA-376 Part 1 and Part 4,
    // and Strict conformance is defined on the post-MCE part (Part 1 §2.1 clause
    // ii), so a conforming processor strips exactly these. Writing them made the
    // package non-conformant on its own terms, and did it in the worst possible
    // way: undeclared, so not even an MCE processor could be told to remove them,
    // and with `00000000` for an absent `textId`, which [MS-DOCX] §2.6.2.4 forbids
    // outright ("Values MUST be greater than 0 and less than 0x80000000").
    //
    // The model keeps both fields, because reading them is necessary - `para_id`
    // identifies a paragraph across saves and an editor needs to know which
    // paragraph it is looking at. Writing them is the writer's decision, and it
    // has been made.
    if paragraph.para_id.is_some() {
        ctx.report_partial(
            "w14:paraId",
            "the paragraph carries w14:paraId/w14:textId, a Microsoft extension absent from \
             ECMA-376, so a Strict package does not write them (ADR-0014)",
            &paragraph.location,
        );
    }
    // The four revision ids w:p declares (ISO/IEC 29500-1 17.3.1.9-17.3.1.30).
    xml.attr_w_opt("rsidR", paragraph.rsids.run.as_deref());
    xml.attr_w_opt("rsidRDefault", paragraph.rsids.run_default.as_deref());
    xml.attr_w_opt("rsidP", paragraph.rsids.paragraph.as_deref());
    xml.attr_w_opt("rsidDel", paragraph.rsids.deleted.as_deref());
    if paragraph.rsids.table_row.is_some() {
        // w:rsidTr belongs to w:tr, not w:p, so there is nowhere legal to
        // write it back; the reader recorded it from a w:p and would read it
        // again, but emitting an undeclared attribute would make the part
        // schema-invalid.
        ctx.report_info(
            "w:rsidTr",
            "revision id for a table row has no schema-legal home on w:p and was not written",
            &paragraph.location,
        );
    }

    paragraph_properties(ctx, xml, &paragraph.props);

    for inline in &paragraph.inlines {
        inline_item(ctx, xml, inline);
    }

    // An empty paragraph is written as `<w:p/>`: `w:p`'s content model is
    // `EG_PContent*`, so zero children are legal, and inventing an empty run
    // would make the written model differ from the one that was parsed.
    xml.end();
}

/// Writes one inline-level item.
pub fn inline_item(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, inline: &Inline) {
    match inline {
        Inline::Run(run) => run_element(ctx, xml, run),
        Inline::Hyperlink(link) => {
            xml.start("w:hyperlink");
            // The parsed id belongs to the source package; this write emits its
            // own, and the map is empty when there is no source to ask.
            if let Some(id) = link
                .rel_id
                .as_ref()
                .and_then(|id| ctx.hyperlink_rel(id.as_str()))
            {
                xml.attr("r:id", id);
            }
            xml.attr_w_opt("anchor", link.anchor.as_deref());
            xml.attr_w_opt("tooltip", link.tooltip.as_deref());
            for child in &link.inlines {
                inline_item(ctx, xml, child);
            }
            xml.end();
        }
        Inline::Field(field) => {
            xml.start("w:fldSimple");
            xml.attr_w_opt("instr", field.instruction.as_deref());
            for child in &field.inlines {
                inline_item(ctx, xml, child);
            }
            xml.end();
        }
        Inline::Drawing(drawing) => {
            crate::drawing::drawing_element(ctx, xml, drawing);
        }
        Inline::Break(kind) => {
            xml.empty_attr_w("w:br", "type", kind.as_str());
        }
        Inline::Tab => xml.empty("w:tab"),
        Inline::SdtInline(sdt) => {
            ctx.report_unsupported("w:sdt", "inline content control", &sdt.location);
            for child in &sdt.inlines {
                inline_item(ctx, xml, child);
            }
        }
        Inline::BookmarkStart(bookmark) => {
            xml.start("w:bookmarkStart");
            xml.attr_w("id", bookmark.id.as_str());
            // `CT_Bookmark` makes `w:name` required. The writer wrote the id alone,
            // which is invalid AND wrong: the id pairs the start with its end,
            // while the name is what `w:hyperlink/@w:anchor` and a REF field point
            // at, so every internal link into a bookmark lost its destination
            // (`XS-20`).
            xml.attr_w("name", bookmark.name.as_ref());
            xml.end();
        }
        Inline::BookmarkEnd(id) => {
            xml.start("w:bookmarkEnd");
            xml.attr_w("id", id.as_str());
            xml.end();
        }
        Inline::CommentRangeStart(id) => {
            xml.start("w:commentRangeStart");
            xml.attr_w("id", id.as_str());
            xml.end();
        }
        Inline::CommentRangeEnd(id) => {
            xml.start("w:commentRangeEnd");
            xml.attr_w("id", id.as_str());
            xml.end();
        }
        Inline::CommentReference(id) => {
            xml.start("w:r");
            xml.start("w:commentReference");
            xml.attr_w("id", id.as_str());
            xml.end();
            xml.end();
        }
        Inline::FootnoteRef(id) => {
            xml.start("w:r");
            xml.empty_attr_w("w:footnoteReference", "id", id);
            xml.end();
        }
        Inline::EndnoteRef(id) => {
            xml.start("w:r");
            xml.empty_attr_w("w:endnoteReference", "id", id);
            xml.end();
        }
        Inline::Math(expression) => {
            crate::math::math_expression(ctx, xml, expression);
        }
        Inline::MathParagraph(paragraph) => {
            crate::math::math_paragraph(ctx, xml, paragraph);
        }
        Inline::Directional(dir) => {
            let name = match dir.kind {
                strict_ooxml_wml::model::inline::DirectionalKind::Dir => "w:dir",
                strict_ooxml_wml::model::inline::DirectionalKind::Bdo => "w:bdo",
            };
            xml.start(name);
            xml.attr_w("val", dir.val.as_str());
            for child in &dir.inlines {
                inline_item(ctx, xml, child);
            }
            xml.end();
        }
        Inline::Opaque(opaque) => {
            ctx.report_unsupported(
                &opaque.feature_id(),
                "inline element kept in the model but not serializable",
                &opaque.location,
            );
        }
    }
}

/// Writes `w:r` and its content.
pub fn run_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, run: &Run) {
    xml.start("w:r");
    crate::props::run_properties(xml, &run.props);
    for content in &run.content {
        run_content(ctx, xml, content);
    }
    xml.end();
}

/// Writes one run child.
pub fn run_content(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, content: &RunContent) {
    match content {
        RunContent::Text(node) => text_node(xml, node),
        RunContent::Tab => xml.empty("w:tab"),
        RunContent::Break(kind) => match kind {
            BreakKind::Page => xml.empty_attr_w("w:br", "type", "page"),
            BreakKind::Column => xml.empty_attr_w("w:br", "type", "column"),
            BreakKind::TextWrapping => xml.empty("w:br"),
        },
        RunContent::CarriageReturn => xml.empty("w:cr"),
        RunContent::Drawing(drawing) => crate::drawing::drawing_element(ctx, xml, drawing),
        RunContent::InstrText(text) => {
            xml.start("w:instrText");
            xml.attr("xml:space", "preserve");
            xml.text(text);
            xml.end();
        }
        RunContent::FieldChar(field) => {
            xml.start("w:fldChar");
            xml.attr_w("fldCharType", field.kind.as_str());
            if field.dirty {
                xml.attr_w("dirty", "true");
            }
            xml.end();
        }
        RunContent::FootnoteRef(id) => xml.empty_attr_w("w:footnoteReference", "id", id),
        RunContent::EndnoteRef(id) => xml.empty_attr_w("w:endnoteReference", "id", id),
        // The anchor, not the comment: this is the run that makes a
        // w:commentRangeStart/End pair visible. `w:commentRangeStart` and
        // `w:commentRangeEnd` are written from the inline list without a
        // containing run, and this is the element that has to sit inside one,
        // so it is empty and self-closing like the note references above.
        RunContent::CommentReference(id) => xml.empty_attr_w("w:commentReference", "id", id),
        // Three required attributes, so a start/end pair rather than the
        // self-closing form the note references use.
        RunContent::Ptab {
            alignment,
            relative_to,
            leader,
        } => {
            xml.start("w:ptab");
            xml.attr_w("alignment", alignment.as_ref());
            xml.attr_w("relativeTo", relative_to.as_ref());
            xml.attr_w("leader", leader.as_ref());
            xml.end();
        }
        RunContent::NoteRef => {
            // The note's own number is substituted by the renderer; writing the
            // element keeps the note body round-trippable. Which of the two
            // elements it is depends on the part being written — `w:endnoteRef`
            // inside `endnotes.xml` — and the model does not record that, so
            // the context does.
            match ctx.note_role() {
                Some(role) => xml.empty(role.reference_element()),
                None => ctx.report_unsupported(
                    "w:footnoteRef",
                    "a note reference marker outside a footnotes or endnotes part",
                    &strict_ooxml_core::error::SourceLocation::unknown(),
                ),
            }
        }
        RunContent::Symbol(symbol) => {
            xml.start("w:sym");
            xml.attr_w("font", symbol.font.as_ref());
            xml.attr_w("char", format!("{:04X}", symbol.character as u32));
            xml.end();
        }
        RunContent::LastRenderedPageBreak => xml.empty("w:lastRenderedPageBreak"),
        RunContent::NoBreakHyphen => xml.empty("w:noBreakHyphen"),
        RunContent::SoftHyphen => xml.empty("w:softHyphen"),
        RunContent::Opaque(opaque) => {
            ctx.report_unsupported(
                &opaque.feature_id(),
                "run child kept in the model but not serializable",
                &opaque.location,
            );
        }
    }
}

/// Writes `w:t`, including the `xml:space` attribute the model recorded.
///
/// The attribute is only written when the text actually needs it: leading or
/// trailing whitespace is stripped by an XML parser without `preserve`, so a
/// text node without edge whitespace does not carry it.
fn text_node(xml: &mut XmlWriter, node: &TextNode) {
    let needs_preserve = matches!(node.space, Space::Preserve)
        || node.text.chars().next().is_some_and(char::is_whitespace)
        || node
            .text
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
    if node.text.is_empty() && !needs_preserve {
        xml.empty("w:t");
        return;
    }
    xml.start("w:t");
    if needs_preserve {
        xml.attr("xml:space", "preserve");
    }
    xml.text(&node.text);
    xml.end();
}

/// Writes `w:tbl`.
pub fn table_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, table: &Table) {
    xml.start("w:tbl");
    table_properties(xml, &table.props);
    if !table.grid.is_empty() {
        xml.start("w:tblGrid");
        for column in &table.grid {
            match column.width {
                Some(width) => xml.empty_attr_w("w:gridCol", "w", width.0),
                None => xml.empty("w:gridCol"),
            }
        }
        xml.end();
    }
    // AUD-41: consecutive rows that share the same unwrapped `sdtPr` are
    // re-wrapped in one `w:sdt` so parse → write → parse keeps the control.
    let mut index = 0;
    while index < table.rows.len() {
        if let Some(sdt) = &table.rows[index].sdt {
            let mut end = index + 1;
            while end < table.rows.len() && table.rows[end].sdt.as_ref() == Some(sdt) {
                end += 1;
            }
            write_sdt_around(xml, sdt, |xml| {
                for row in &table.rows[index..end] {
                    table_row_element(ctx, xml, row);
                }
            });
            index = end;
        } else {
            table_row_element(ctx, xml, &table.rows[index]);
            index += 1;
        }
    }
    xml.end();
}

/// Writes one `w:tr`, re-wrapping cell-level content controls (AUD-41).
fn table_row_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, row: &TableRow) {
    xml.start("w:tr");
    row_properties(xml, &row.props);
    let mut index = 0;
    while index < row.cells.len() {
        if let Some(sdt) = &row.cells[index].sdt {
            let mut end = index + 1;
            while end < row.cells.len() && row.cells[end].sdt.as_ref() == Some(sdt) {
                end += 1;
            }
            write_sdt_around(xml, sdt, |xml| {
                for cell in &row.cells[index..end] {
                    table_cell_element(ctx, xml, cell);
                }
            });
            index = end;
        } else {
            table_cell_element(ctx, xml, &row.cells[index]);
            index += 1;
        }
    }
    xml.end();
}

/// Writes one `w:tc`.
fn table_cell_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, cell: &TableCell) {
    xml.start("w:tc");
    cell_properties(xml, &cell.props);
    blocks(ctx, xml, &cell.blocks);
    // A table cell must end with a paragraph, so an empty cell gets
    // an empty one rather than being left without block content.
    if cell.blocks.is_empty() {
        xml.start("w:p");
        xml.end();
    }
    xml.end();
}

/// Writes `<w:sdt><w:sdtPr>…</w:sdtPr><w:sdtContent>…</w:sdtContent></w:sdt>`.
fn write_sdt_around(
    xml: &mut XmlWriter,
    sdt: &SdtProperties,
    content: impl FnOnce(&mut XmlWriter),
) {
    xml.start("w:sdt");
    xml.start("w:sdtPr");
    if let Some(alias) = sdt.alias.as_deref() {
        xml.empty_attr_w("w:alias", "val", alias);
    }
    if let Some(tag) = sdt.tag.as_deref() {
        xml.empty_attr_w("w:tag", "val", tag);
    }
    if let Some(id) = sdt.id.as_deref() {
        xml.empty_attr_w("w:id", "val", id);
    }
    if sdt.placeholder.is_some() {
        xml.start("w:placeholder");
        xml.end();
    }
    if sdt.showing_placeholder {
        xml.empty("w:showingPlcHdr");
    }
    xml.end(); // sdtPr
    xml.start("w:sdtContent");
    content(xml);
    xml.end();
    xml.end();
}

/// Writes an `w:sdt` wrapper. Kept for callers that build content controls.
pub fn sdt_container(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, sdt: &SdtContainer) {
    ctx.report_unsupported("w:sdt", "content control", &sdt.location);
    blocks(ctx, xml, &sdt.blocks);
}

#[cfg(test)]
mod tests {
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::normalize::report::NormalizationReport;
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_wml::model::block::{Block, Paragraph};
    use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
    use strict_ooxml_wml::model::props::ParagraphProperties;
    use strict_ooxml_wml::model::values::{Space, Spacing, Twips};

    use super::paragraph_element;
    use crate::ctx::Ctx;
    use crate::xml::XmlWriter;

    fn location() -> SourceLocation {
        SourceLocation::new(PartId::new("/word/document.xml"), 1, 1, 0)
    }

    fn text_paragraph(text: &str, space: Space) -> Paragraph {
        Paragraph {
            props: ParagraphProperties::default(),
            inlines: vec![Inline::Run(Run {
                props: Default::default(),
                content: vec![RunContent::Text(TextNode {
                    text: text.to_owned(),
                    space,
                })],
                location: location(),
            })],
            rsids: Default::default(),
            para_id: None,
            text_id: None,
            location: location(),
        }
    }

    fn render(paragraphs: &[Paragraph]) -> String {
        let items: Vec<Block> = paragraphs
            .iter()
            .map(|paragraph| Block::Paragraph(paragraph.clone()))
            .collect();
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let mut xml = XmlWriter::new();
        crate::body::blocks(&mut ctx, &mut xml, &items);
        xml.finish().expect("balanced")
    }

    #[test]
    fn text_without_edge_whitespace_omits_xml_space() {
        let text = render(&[text_paragraph("hello", Space::Default)]);
        assert!(text.contains("<w:t>hello</w:t>"), "{text}");
    }

    #[test]
    fn leading_whitespace_forces_xml_space_preserve() {
        let text = render(&[text_paragraph(" lead", Space::Default)]);
        assert!(
            text.contains("<w:t xml:space=\"preserve\"> lead</w:t>"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_paragraph_stays_empty() {
        let paragraph = Paragraph {
            props: ParagraphProperties::default(),
            inlines: Vec::new(),
            rsids: Default::default(),
            para_id: None,
            text_id: None,
            location: location(),
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let mut xml = XmlWriter::new();
        paragraph_element(&mut ctx, &mut xml, &paragraph);
        assert_eq!(xml.finish().expect("balanced"), "<w:p/>\n");
    }

    #[test]
    fn a_paragraph_keeps_its_spacing() {
        let mut paragraph = text_paragraph("x", Space::Default);
        paragraph.props.spacing = Some(Spacing {
            after: Some(Twips(200)),
            ..Spacing::default()
        });
        let text = render(&[paragraph]);
        assert!(text.contains("w:after=\"200\""), "{text}");
    }
}
