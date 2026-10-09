//! Reading the drawing Word caches beside a `SmartArt` diagram (`dsp:drawing`).
//!
//! A `dgm:relIds` names the diagram's data part (`r:dm`). Word records, inside
//! that data part, the relationship id of a pre-rendered drawing part:
//! `dgm:dataModel/dgm:extLst/a:ext/dsp:dataModelExt/@relId`. The relationship
//! itself is declared by the part that holds the diagram (the main document
//! part in every file Word writes), with the type [`DIAGRAM_DRAWING_REL`]; the
//! data part's own relationships are tried as a fallback.
//!
//! `PartParser::diagram_for` follows that chain, reads the drawing part
//! through the package (so the package's part-size limits and its normalizer
//! apply) and parses its `dsp:spTree` with the same shape, fill, outline and
//! text-body parsers a `wps:wsp` uses. Every failure is a support record, never
//! a document error: a diagram whose drawing cannot be read keeps its
//! placeholder.

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::{Result, SourceLocation};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::rels::RelType;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent, XmlReader};

use crate::model::block::{Block, Paragraph};
use crate::model::drawing::{
    DiagramDrawing, Graphic, GroupShape, Shape, ShapeColor, ShapeFill, ShapeGeometry, ShapeStroke,
    ShapeStyle, TextBox, MAX_DIAGRAM_SHAPES,
};
use crate::model::inline::{Inline, Run, RunContent, TextNode};
use crate::model::props::{ParagraphProperties, RunProperties};
use crate::model::support::SupportStatus;
use crate::model::values::{
    BreakKind, HalfPoints, Justification, Rsids, Space, Spacing, ThemeColor, ThemeColorRef,
    TriState, Twips,
};

use super::{plain_attr, PartParser};

/// The Office 2008 diagram-drawing namespace (`dsp:`).
pub const DSP_NS: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";

/// The relationship type of a diagram's cached drawing part.
pub const DIAGRAM_DRAWING_REL: &str =
    "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing";

/// Transitional DrawingML main namespace (a normalized part carries the Strict
/// one; both are recognised here).
const DRAWINGML_TRANSITIONAL_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// Diagram drawings already read by one part parser, by diagram data part.
pub(crate) struct DiagramCache {
    limits: ResourceLimits,
    parsed: HashMap<PartId, Option<Arc<DiagramDrawing>>>,
}

impl DiagramCache {
    /// An empty cache reading parts under `limits`.
    pub(crate) fn new(limits: &ResourceLimits) -> Self {
        Self {
            limits: *limits,
            parsed: HashMap::new(),
        }
    }
}

/// How many more shapes one drawing part may contribute.
struct ShapeBudget {
    left: usize,
    dropped: usize,
}

impl ShapeBudget {
    /// Takes one shape from the budget; `false` once it is spent.
    fn take(&mut self) -> bool {
        if self.left == 0 {
            self.dropped = self.dropped.saturating_add(1);
            false
        } else {
            self.left -= 1;
            true
        }
    }
}

/// `(id, name, descr)` of a `dsp:cNvPr`.
type NonVisual = (Option<u32>, Option<Arc<str>>, Option<Arc<str>>);

fn is_dsp(name: &QName) -> bool {
    name.ns.as_ref().is_some_and(|ns| ns == DSP_NS)
}

fn is_drawingml(name: &QName) -> bool {
    name.ns
        .as_ref()
        .is_some_and(|ns| ns == crate::DRAWINGML_STRICT_NS || ns == DRAWINGML_TRANSITIONAL_NS)
}

impl PartParser<'_> {
    /// The cached drawing of the diagram whose data part `data_rel` names, if
    /// it can be read.
    pub(crate) fn diagram_for(
        &mut self,
        data_rel: &str,
        location: &SourceLocation,
    ) -> Option<Arc<DiagramDrawing>> {
        let Some(data_part) = self.resolve_relationship_target(data_rel) else {
            self.record_diagram_gap(
                &format!("r:dm '{data_rel}' does not resolve to a diagram data part"),
                location,
            );
            return None;
        };
        if let Some(cached) = self.diagrams.parsed.get(&data_part) {
            return cached.clone();
        }
        let drawing = match self.read_diagram_drawing(&data_part) {
            Ok(drawing) => {
                if drawing.truncated {
                    self.record(
                        "dsp:spTree",
                        SupportStatus::Partial,
                        Some(format!(
                            "shapes past {MAX_DIAGRAM_SHAPES} in {} were dropped",
                            drawing.part
                        )),
                        Some(location.clone()),
                    );
                }
                self.record(
                    "dsp:drawing",
                    SupportStatus::Supported,
                    None,
                    Some(location.clone()),
                );
                Some(Arc::new(drawing))
            }
            Err(message) => {
                self.record_diagram_gap(&message, location);
                None
            }
        };
        self.diagrams.parsed.insert(data_part, drawing.clone());
        drawing
    }

    fn record_diagram_gap(&mut self, message: &str, location: &SourceLocation) {
        self.record(
            "dgm:relIds",
            SupportStatus::Partial,
            Some(format!("{message}; drawn as a placeholder")),
            Some(location.clone()),
        );
    }

    /// Finds, reads and parses the drawing part of the diagram whose data part
    /// is `data_part`.
    fn read_diagram_drawing(
        &mut self,
        data_part: &PartId,
    ) -> std::result::Result<DiagramDrawing, String> {
        let limits = self.diagrams.limits;
        let package = self.package;
        let data = package.read_part(data_part).map_err(|error| {
            format!("diagram data part {data_part} could not be read ({error})")
        })?;
        let rel_id = drawing_rel_id(data, data_part, &limits)?;
        let drawing_part = self.diagram_drawing_target(&rel_id, data_part)?;
        let bytes = package.read_part(&drawing_part).map_err(|error| {
            format!("diagram drawing part {drawing_part} could not be read ({error})")
        })?;
        let mut parser = PartParser::new(package, drawing_part.clone(), bytes, &limits)
            .map_err(|error| error.to_string())?;
        let parsed = parser.parse_diagram_drawing_root();
        let support = std::mem::take(&mut parser.support);
        let media = std::mem::take(&mut parser.media);
        drop(parser);
        self.support.merge(support);
        super::merge_media(&mut self.media, &media);
        parsed.map_err(|error| {
            format!("diagram drawing part {drawing_part} could not be parsed ({error})")
        })
    }

    /// The part a `dsp:dataModelExt/@relId` names: a `diagramDrawing`
    /// relationship of the current part, or failing that of the data part.
    fn diagram_drawing_target(
        &self,
        rel_id: &str,
        data_part: &PartId,
    ) -> std::result::Result<PartId, String> {
        for source in [&self.part, data_part] {
            let Ok(rel) = self.package.resolve_relationship(source, rel_id) else {
                continue;
            };
            let is_drawing = rel.raw_type == DIAGRAM_DRAWING_REL
                || matches!(&rel.rel_type, RelType::Other(uri) if uri == DIAGRAM_DRAWING_REL);
            if !is_drawing {
                continue;
            }
            return match rel.resolved.clone() {
                Some(part) if self.package.part(&part).is_some() => Ok(part),
                Some(part) => Err(format!("diagram drawing part {part} is missing")),
                None => Err(format!("relationship '{rel_id}' has no internal target")),
            };
        }
        Err(format!(
            "dsp:dataModelExt relId '{rel_id}' is not a diagramDrawing relationship"
        ))
    }

    /// Parses a whole `dsp:drawing` part.
    fn parse_diagram_drawing_root(&mut self) -> Result<DiagramDrawing> {
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. }
                    if is_dsp(&name) && name.local() == "drawing" =>
                {
                    break;
                }
                XmlEvent::Text(text) | XmlEvent::CData(text)
                    if super::is_prolog_whitespace(&text) => {}
                _ => return Err(self.invalid("expected 'dsp:drawing' root element")),
            }
        }
        let mut drawing = DiagramDrawing {
            part: self.part.clone(),
            xfrm: None,
            shapes: Vec::new(),
            truncated: false,
        };
        let mut budget = ShapeBudget {
            left: MAX_DIAGRAM_SHAPES,
            dropped: 0,
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. }
                    if is_dsp(&name) && name.local() == "spTree" =>
                {
                    let tree = self.parse_dsp_group(&mut budget)?;
                    drawing.xfrm = tree.xfrm;
                    drawing.shapes = tree.children;
                }
                XmlEvent::StartElement { .. } => self.skip_element()?,
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of diagram drawing")),
            }
        }
        self.expect_end_of_part()?;
        drawing.truncated = budget.dropped > 0;
        Ok(drawing)
    }

    /// Parses `dsp:spTree` or `dsp:grpSp` (start consumed).
    fn parse_dsp_group(&mut self, budget: &mut ShapeBudget) -> Result<GroupShape> {
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
                    XmlEvent::StartElement { name, attrs } if is_dsp(&name) => {
                        match name.local() {
                            "sp" if budget.take() => {
                                group
                                    .children
                                    .push(Graphic::Shape(parser.parse_dsp_shape()?));
                            }
                            "grpSp" if budget.take() => {
                                let child = parser.parse_dsp_group(budget)?;
                                group.children.push(Graphic::Group(child));
                            }
                            "nvGrpSpPr" => {
                                let (_, group_name, descr) = parser.parse_dsp_non_visual()?;
                                group.name = group_name;
                                group.descr = descr;
                            }
                            "grpSpPr" => {
                                let transform = parser.parse_group_transform(&attrs)?;
                                // Word writes an empty `dsp:grpSpPr`; an empty
                                // transform is no transform.
                                if transform.extent.is_some() || transform.offset.is_some() {
                                    group.xfrm = Some(transform);
                                }
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of diagram group")),
                }
            }
            Ok(group)
        })
    }

    /// Reads `dsp:cNvPr` out of a `dsp:nvSpPr` / `dsp:nvGrpSpPr` (start
    /// consumed): `(id, name, descr)`.
    fn parse_dsp_non_visual(&mut self) -> Result<NonVisual> {
        self.nested(|parser| {
            let mut found: NonVisual = (None, None, None);
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if name.local() == "cNvPr" {
                            found.0 = plain_attr(&attrs, "id").and_then(|v| v.trim().parse().ok());
                            found.1 = plain_attr(&attrs, "name")
                                .filter(|v| !v.is_empty())
                                .map(|v| parser.intern(v));
                            found.2 = plain_attr(&attrs, "descr").map(|v| parser.intern(v));
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of non-visual properties"));
                    }
                }
            }
            Ok(found)
        })
    }

    /// Parses a `dsp:sp` (start consumed).
    fn parse_dsp_shape(&mut self) -> Result<Shape> {
        let location = self.location();
        self.nested(|parser| {
            let mut shape = Shape {
                name: None,
                descr: None,
                nv_id: None,
                bw_mode: None,
                tx_box: None,
                sp_locks: None,
                effects: None,
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
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } if is_dsp(&name) => {
                        parser.parse_dsp_shape_child(&mut shape, name.local(), &attrs)?;
                    }
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of diagram shape")),
                }
            }
            if let Some(style) = shape.style.take() {
                apply_style_defaults(&mut shape, &style);
                shape.style = Some(style);
            }
            parser.record(
                "dsp:sp",
                SupportStatus::Supported,
                None,
                Some(shape.location.clone()),
            );
            Ok(shape)
        })
    }

    /// One child of `dsp:sp` (start consumed).
    fn parse_dsp_shape_child(
        &mut self,
        shape: &mut Shape,
        local: &str,
        attrs: &[Attr],
    ) -> Result<()> {
        match local {
            "nvSpPr" => {
                let (id, shape_name, descr) = self.parse_dsp_non_visual()?;
                shape.nv_id = id;
                shape.name = shape_name;
                shape.descr = descr;
            }
            "spPr" => {
                shape.bw_mode = plain_attr(attrs, "bwMode").map(|value| self.intern(value));
                self.parse_shape_sp_pr(shape)?;
            }
            "style" => shape.style = Some(self.parse_shape_style()?),
            "txBody" => shape.text = Some(self.parse_dsp_text_body()?),
            "txXfrm" => {
                self.record(
                    "dsp:txXfrm",
                    SupportStatus::Partial,
                    Some("diagram text is laid out in its shape's box".to_owned()),
                    Some(self.location()),
                );
                self.skip_element()?;
            }
            _ => self.skip_element()?,
        }
        Ok(())
    }

    /// Parses `dsp:txBody` (start consumed) into a text box of paragraphs.
    fn parse_dsp_text_body(&mut self) -> Result<TextBox> {
        let location = self.location();
        self.nested(|parser| {
            let mut text = TextBox {
                body: None,
                blocks: Vec::new(),
                location,
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } if is_drawingml(&name) => {
                        match name.local() {
                            "bodyPr" => text.body = Some(parser.parse_text_box_body(&attrs)?),
                            "p" => text
                                .blocks
                                .push(Block::Paragraph(parser.parse_drawingml_paragraph()?)),
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of text body")),
                }
            }
            Ok(text)
        })
    }

    /// Parses a DrawingML `a:p` (start consumed) into a WML paragraph.
    ///
    /// Kept: alignment (`a:pPr/@algn`), runs (`a:r`, `a:fld` by its cached
    /// text) with size, bold, italic and a solid colour, and line breaks. The
    /// paragraph's own spacing is not modelled; it is laid out with none.
    fn parse_drawingml_paragraph(&mut self) -> Result<Paragraph> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = ParagraphProperties {
                spacing: Some(Spacing {
                    before: Some(Twips(0)),
                    after: Some(Twips(0)),
                    ..Spacing::default()
                }),
                ..ParagraphProperties::default()
            };
            let mut inlines = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } if is_drawingml(&name) => {
                        match name.local() {
                            "pPr" => {
                                props.alignment = plain_attr(&attrs, "algn").and_then(alignment);
                                parser.skip_element()?;
                            }
                            "r" | "fld" => inlines.push(Inline::Run(parser.parse_drawingml_run()?)),
                            "br" => {
                                let location = parser.location();
                                parser.skip_element()?;
                                inlines.push(Inline::Run(Run {
                                    props: RunProperties::default(),
                                    content: vec![RunContent::Break(BreakKind::TextWrapping)],
                                    revision: None,
                                    location,
                                }));
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of paragraph")),
                }
            }
            Ok(Paragraph {
                props,
                inlines,
                rsids: Rsids::default(),
                revision: None,
                para_id: None,
                text_id: None,
                location,
            })
        })
    }

    /// Parses a DrawingML `a:r` / `a:fld` (start consumed) into a WML run.
    fn parse_drawingml_run(&mut self) -> Result<Run> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = RunProperties::default();
            let mut text = String::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } if is_drawingml(&name) => {
                        match name.local() {
                            "rPr" => parser.parse_drawingml_run_props(&attrs, &mut props)?,
                            "t" => text.push_str(&parser.read_element_text()?),
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of run")),
                }
            }
            Ok(Run {
                props,
                content: vec![RunContent::Text(TextNode {
                    text,
                    space: Space::Preserve,
                })],
                revision: None,
                location,
            })
        })
    }

    /// Parses `a:rPr` (start consumed) into `props`.
    fn parse_drawingml_run_props(
        &mut self,
        attrs: &[Attr],
        props: &mut RunProperties,
    ) -> Result<()> {
        // `ST_TextFontSize`: hundredths of a point, 1..4000 pt.
        props.size = plain_attr(attrs, "sz")
            .and_then(|value| value.trim().parse::<i32>().ok())
            .filter(|value| (100..=400_000).contains(value))
            .map(|value| HalfPoints(value / 50));
        props.bold = toggle(plain_attr(attrs, "b"));
        props.italic = toggle(plain_attr(attrs, "i"));
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. }
                        if is_drawingml(&name) && name.local() == "solidFill" =>
                    {
                        if let Some(color) = parser.parse_fill_color()? {
                            set_run_color(props, color);
                        }
                    }
                    XmlEvent::StartElement { .. } => parser.skip_element()?,
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of run properties")),
                }
            }
            Ok(())
        })
    }
}

/// The `dsp:dataModelExt/@relId` of a diagram data part.
fn drawing_rel_id(
    bytes: Vec<u8>,
    part: &PartId,
    limits: &ResourceLimits,
) -> std::result::Result<String, String> {
    let mut reader =
        XmlReader::from_vec(bytes, part.clone(), limits).map_err(|error| error.to_string())?;
    loop {
        match reader.next_event().map_err(|error| error.to_string())? {
            XmlEvent::StartElement { name, attrs }
                if is_dsp(&name) && name.local() == "dataModelExt" =>
            {
                return plain_attr(&attrs, "relId")
                    .map(str::to_owned)
                    .ok_or_else(|| "dsp:dataModelExt has no relId".to_owned());
            }
            XmlEvent::Eof => {
                return Err(format!(
                    "diagram data part {part} names no cached drawing (dsp:dataModelExt)"
                ));
            }
            _ => {}
        }
    }
}

/// `ST_TextAlignType` to a WML justification.
fn alignment(value: &str) -> Option<Justification> {
    match value {
        "l" => Some(Justification::Start),
        "ctr" => Some(Justification::Center),
        "r" => Some(Justification::End),
        "just" | "justLow" => Some(Justification::Both),
        "dist" | "thaiDist" => Some(Justification::Distribute),
        _ => None,
    }
}

/// A DrawingML boolean attribute as a run toggle.
fn toggle(value: Option<&str>) -> TriState {
    match value.map(str::trim) {
        Some("1" | "true") => TriState::On,
        Some("0" | "false") => TriState::Off,
        _ => TriState::Absent,
    }
}

/// Copies a DrawingML colour onto a run.
fn set_run_color(props: &mut RunProperties, color: ShapeColor) {
    if let Some(value) = color.value {
        props.color = Some(value);
    } else if let Some(theme) = color.theme {
        props.color_theme = Some(theme);
    }
}

/// Applies a shape's `dsp:style` references where the shape has no explicit
/// value: the fill and line colours (`a:fillRef`, `a:lnRef` with a non-zero
/// index) and the text colour (`a:fontRef`).
fn apply_style_defaults(shape: &mut Shape, style: &ShapeStyle) {
    let referenced = |index: Option<&Arc<str>>| index.is_some_and(|idx| idx.trim() != "0");
    if shape.fill.is_none() && referenced(style.fill_ref.as_ref()) {
        if let Some(slot) = &style.fill_ref_color {
            shape.fill = Some(ShapeFill::Solid {
                color: scheme_color(slot),
            });
        }
    }
    if shape.stroke.is_none() && referenced(style.line_ref.as_ref()) {
        if let Some(slot) = &style.line_ref_color {
            shape.stroke = Some(ShapeStroke {
                color: Some(scheme_color(slot)),
                ..ShapeStroke::default()
            });
        }
    }
    let (Some(slot), Some(text)) = (&style.font_ref_color, shape.text.as_mut()) else {
        return;
    };
    for block in &mut text.blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &mut paragraph.inlines {
            if let Inline::Run(run) = inline {
                if run.props.color.is_none() && run.props.color_theme.is_none() {
                    run.props.color_theme = scheme_color(slot).theme;
                }
            }
        }
    }
}

/// A theme colour reference to `slot` with no modifiers.
fn scheme_color(slot: &Arc<str>) -> ShapeColor {
    ShapeColor {
        value: None,
        theme: Some(ThemeColorRef {
            color: ThemeColor::new(Arc::clone(slot)),
            tint: None,
            shade: None,
        }),
    }
}
