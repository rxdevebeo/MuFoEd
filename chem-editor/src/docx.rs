//! Adapter that writes the document through `StrictLib`'s public model and
//! shared package writer, without patching OOXML.
//!
//! Structures become an editable drawing: every atom is a text box and every
//! bond a line, and each shape carries a `chem:v1:…` name so the drawing can be
//! read back. Reactions become an editable paragraph of runs with real
//! subscripts and superscripts. Import reverses both for this crate's own
//! output; foreign drawings are reported as unsupported.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]

use std::fmt::Write as _;
use std::sync::Arc;

use strict_ooxml::{model as w, StrictDocument, WriteOptions};
use strict_ooxml_core::error::SourceLocation;

use crate::element;
use crate::model::{
    Atom, Bond, BondOrder, BondStereo, ChemDocument, ChemObject, Marker, Part, Point, Reaction,
    Species, State, Structure,
};
use crate::render::text_width;

/// A standard editable package, with explicit interoperability limits.
pub struct DocxExport {
    /// Complete Strict DOCX package.
    pub bytes: Vec<u8>,
    /// Named conversion limitations.
    pub issues: Vec<String>,
}

/// Imported document. Unsupported input returns an error instead of replacing
/// the open document.
pub struct DocxImport {
    /// Editable chemistry document.
    pub document: ChemDocument,
    /// Reconstruction and fidelity notes.
    pub issues: Vec<String>,
}

const FONT_HALF_POINTS: i32 = 27;
const SMALL_HALF_POINTS: i32 = 19;

fn loc() -> SourceLocation {
    SourceLocation::unknown()
}

fn emu(px: f32) -> w::Emu {
    w::Emu((f64::from(px) * 9525.0).round() as i64)
}

fn px(value: w::Emu) -> f32 {
    (value.0 as f64 / 9525.0) as f32
}

fn rgb(value: [u8; 3]) -> w::drawing::ShapeColor {
    w::drawing::ShapeColor {
        value: Some(w::Color::new(format!(
            "{:02X}{:02X}{:02X}",
            value[0], value[1], value[2]
        ))),
        ..Default::default()
    }
}

fn extent(size: Point) -> w::Extent {
    w::Extent {
        cx: emu(size.x),
        cy: emu(size.y),
    }
}

fn base_shape(id: u32, position: Point, size: Point) -> w::drawing::Shape {
    w::drawing::Shape {
        name: Some(format!("chem:v1:shape:{id}").into()),
        descr: None,
        nv_id: Some(id),
        bw_mode: None,
        tx_box: None,
        sp_locks: None,
        connector: false,
        effects: None,
        geometry: w::drawing::ShapeGeometry::None,
        xfrm: None,
        offset: Some((emu(position.x), emu(position.y))),
        extent: Some(extent(size)),
        fill: None,
        stroke: None,
        text: None,
        style: None,
        location: loc(),
    }
}

fn run(text: &str, size: i32, align: Option<w::VertAlign>, color: [u8; 3]) -> w::Inline {
    let props = w::props::RunProperties {
        fonts: Some(w::Fonts {
            ascii: Some("Calibri".into()),
            h_ansi: Some("Calibri".into()),
            ..Default::default()
        }),
        size: Some(w::HalfPoints(size)),
        size_cs: Some(w::HalfPoints(size)),
        color: Some(w::Color::new(format!(
            "{:02X}{:02X}{:02X}",
            color[0], color[1], color[2]
        ))),
        vert_align: align,
        ..Default::default()
    };
    w::Inline::Run(w::Run {
        props,
        content: vec![w::RunContent::Text(w::TextNode {
            text: text.into(),
            space: w::Space::Preserve,
        })],
        revision: None,
        location: loc(),
    })
}

fn paragraph(inlines: Vec<w::Inline>) -> w::Paragraph {
    w::Paragraph {
        props: w::props::ParagraphProperties::default(),
        inlines,
        rsids: w::Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: loc(),
    }
}

impl ChemDocument {
    /// Exports the document as a DOCX with editable shapes and runs.
    ///
    /// # Errors
    ///
    /// Rejects invalid documents and package failures.
    pub fn to_docx(&self) -> Result<DocxExport, String> {
        self.validate()?;
        let mut blocks: Vec<w::Block> = vec![];
        let mut width = 320.0_f32;
        let mut next_id = 1_u32;
        let mut issues = vec![];
        for object in &self.objects {
            match object {
                ChemObject::Structure(structure) => {
                    if structure.atoms.is_empty() {
                        continue;
                    }
                    let (graphic, size, id) = structure_graphic(structure, &mut next_id);
                    width = width.max(size.x + 80.0);
                    let drawing = inline_drawing(id, graphic, size, "Structure");
                    blocks.push(w::Block::Paragraph(paragraph(vec![w::Inline::Drawing(
                        drawing,
                    )])));
                }
                ChemObject::Reaction(reaction) => {
                    blocks.push(w::Block::Paragraph(reaction_paragraph(reaction)));
                    width = width.max(reaction_width(reaction) + 80.0);
                    if !reaction.arrow.above.is_empty() || !reaction.arrow.below.is_empty() {
                        issues.push(
                            "Arrow conditions are written inline after the arrow, not above/below"
                                .into(),
                        );
                    }
                }
            }
        }
        if blocks.is_empty() {
            blocks.push(w::Block::Paragraph(paragraph(vec![])));
        }
        let page = format!(
            "{{\\rtf1\\ansi\\paperw{}\\paperh{}\\margl300\\margr300\\margt300\\margb300\\par}}",
            (width * 15.0 + 600.0).ceil() as i32,
            (480.0_f32 * 15.0 + 600.0).ceil() as i32
        );
        let seed =
            StrictDocument::import_rtf(page.as_bytes(), &strict_ooxml::RtfOptions::default())
                .map_err(|error| error.to_string())?;
        let mut document = seed.document.document().clone();
        document.body.blocks = blocks;
        let output = strict_ooxml::write_package(&document, None, &WriteOptions::default())
            .map_err(|error| error.to_string())?;
        output.report.verify_no_silent_loss()?;
        if strict_ooxml::dropped_count(&output.report) > 0 {
            return Err(format!("DOCX writer rejected content: {}", output.report));
        }
        Ok(DocxExport {
            bytes: output.bytes,
            issues,
        })
    }

    /// Imports a DOCX written by [`Self::to_docx`].
    ///
    /// # Errors
    ///
    /// Rejects unsupported content, foreign drawings and malformed packages.
    pub fn from_docx(bytes: &[u8]) -> Result<DocxImport, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("DOCX input exceeds the size limit".into());
        }
        let options = strict_ooxml::OpenOptions::default()
            .conformance(strict_ooxml::ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(strict_ooxml::TransitionalNormalizer::new()));
        let opened = StrictDocument::open_reader(bytes, &options).map_err(|e| e.to_string())?;
        let mut document = ChemDocument::default();
        let mut issues = vec![];
        let mut next_id = 1_u64;
        for block in &opened.document().body.blocks {
            let w::Block::Paragraph(paragraph) = block else {
                return Err("Only paragraphs are supported on import".into());
            };
            if paragraph.inlines.is_empty() {
                continue;
            }
            let mut drawing = None;
            let mut text = String::new();
            for inline in &paragraph.inlines {
                match inline {
                    w::Inline::Drawing(d) => drawing = Some(d),
                    w::Inline::Run(r) => {
                        for content in &r.content {
                            if let w::RunContent::Text(node) = content {
                                let subscript = r.props.vert_align == Some(w::VertAlign::Subscript);
                                let superscript =
                                    r.props.vert_align == Some(w::VertAlign::Superscript);
                                if superscript {
                                    text.push('^');
                                }
                                text.push_str(&node.text);
                                let _ = subscript;
                            }
                        }
                    }
                    _ => return Err("Unsupported content in the chemistry document".into()),
                }
            }
            if let Some(drawing) = drawing {
                if let Some(structure) = import_structure(drawing, &mut next_id)? {
                    document.objects.push(ChemObject::Structure(structure));
                    continue;
                }
            }
            let normalized = normalize_import_text(&text);
            if normalized.contains("->") || normalized.contains("<-") || normalized.contains("<=") {
                if let Ok(mut reaction) = crate::mhchem::parse(&normalized) {
                    reaction.id = next_id;
                    next_id += 1;
                    document.objects.push(ChemObject::Reaction(reaction));
                    issues.push("Reaction arrow conditions are placed inline".into());
                }
            }
        }
        if document.objects.is_empty() {
            return Err("The DOCX has no chemistry this editor can read".into());
        }
        document.validate()?;
        Ok(DocxImport { document, issues })
    }
}

/// Group name that marks a chemistry drawing for the host.
pub const DRAWING_GROUP: &str = "Chemistry";

impl ChemDocument {
    /// Builds one inline drawing for the whole document.
    ///
    /// Structures become named atom/bond shapes (so [`ChemDocument::from_drawing`]
    /// can read them back); reactions are drawn as plain shapes. Everything is
    /// stacked vertically inside one group named [`DRAWING_GROUP`], which the
    /// host uses to recognize the object and reopen the editor.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the document is empty or invalid.
    pub fn to_drawing(&self) -> Result<w::Drawing, String> {
        self.validate()?;
        let theme = crate::render::Theme::default();
        let mut next_id = 1_u32;
        let mut children: Vec<w::drawing::Graphic> = vec![];
        let mut cursor = 0.0_f32;
        let mut max_width = 48.0_f32;
        for object in &self.objects {
            match object {
                ChemObject::Structure(structure) if structure.atoms.is_empty() => {}
                ChemObject::Structure(structure) => {
                    let (min, max) = structure.bounds();
                    let size = Point::new((max.x - min.x).max(1.0), (max.y - min.y).max(1.0));
                    let offset = Point::new(24.0 - min.x, cursor + 24.0 - min.y);
                    push_structure_shapes(structure, offset, &mut next_id, &mut children);
                    cursor += size.y + 28.0;
                    max_width = max_width.max(size.x + 48.0);
                }
                ChemObject::Reaction(_) => {
                    let prims = crate::render::object_prims(object, &theme, false);
                    let Some((min, max)) = crate::render::prims_bounds(&prims) else {
                        continue;
                    };
                    let delta = Point::new(24.0 - min.x, cursor + 24.0 - min.y);
                    for prim in &prims {
                        let shifted = shift_prim(prim, delta);
                        let shape = prim_shape(&shifted, next_id);
                        next_id += 1;
                        children.push(w::drawing::Graphic::Shape(shape));
                    }
                    cursor += (max.y - min.y) + 28.0;
                    max_width = max_width.max(max.x - min.x + 48.0);
                }
            }
        }
        if children.is_empty() {
            return Err("There is nothing to draw".into());
        }
        let size = Point::new(max_width.max(48.0), (cursor + 24.0).max(48.0));
        let group_id = next_id;
        let graphic = w::drawing::Graphic::Group(w::drawing::GroupShape {
            nv_id: Some(group_id),
            name: Some(DRAWING_GROUP.into()),
            descr: None,
            xfrm: Some(w::drawing::GroupTransform {
                offset: Some((w::Emu(0), w::Emu(0))),
                extent: Some(extent(size)),
                child_offset: Some((w::Emu(0), w::Emu(0))),
                child_extent: Some(extent(size)),
                ..Default::default()
            }),
            locks: None,
            bw_mode: None,
            children,
            location: loc(),
        });
        Ok(inline_drawing(group_id, graphic, size, DRAWING_GROUP))
    }

    /// Reads structures back from a drawing built by [`ChemDocument::to_drawing`].
    ///
    /// Reactions are not encoded in the drawing and are reported in the issue
    /// list; the host keeps its own source for those.
    ///
    /// # Errors
    ///
    /// Rejects a drawing that is not a chemistry group.
    pub fn from_drawing(drawing: &w::Drawing) -> Result<(Self, Vec<String>), String> {
        let w::DrawingKind::Inline(inline) = &drawing.kind else {
            return Err("Expected an inline chemistry drawing".into());
        };
        let w::drawing::Graphic::Group(group) = inline.graphic.as_ref() else {
            return Err("Expected a chemistry group".into());
        };
        if group.name.as_deref() != Some(DRAWING_GROUP) {
            return Err("Not a chemistry drawing".into());
        }
        let mut per: std::collections::BTreeMap<u64, (Vec<Atom>, Vec<Bond>)> =
            std::collections::BTreeMap::new();
        let mut plain = 0_usize;
        for child in &group.children {
            let w::drawing::Graphic::Shape(shape) = child else {
                plain += 1;
                continue;
            };
            let name = shape.name.as_deref().unwrap_or_default();
            if let Some(rest) = name.strip_prefix("chem:v1:atom:") {
                let fields: Vec<&str> = rest.split(':').collect();
                if fields.len() != 5 {
                    return Err("Malformed atom shape".into());
                }
                let structure = fields[0].parse::<u64>().map_err(|e| e.to_string())?;
                let id = fields[1].parse::<u64>().map_err(|e| e.to_string())?;
                let element = fields[2].parse::<u8>().map_err(|e| e.to_string())?;
                let charge = fields[3].parse::<i8>().map_err(|e| e.to_string())?;
                let hydrogens = if fields[4] == "-" {
                    None
                } else {
                    Some(fields[4].parse::<u8>().map_err(|e| e.to_string())?)
                };
                let offset = shape.offset.ok_or("Missing atom position")?;
                let size = shape.extent.ok_or("Missing atom extent")?;
                per.entry(structure).or_default().0.push(Atom {
                    id,
                    element,
                    position: Point::new(
                        px(offset.0) + px(size.cx) / 2.0,
                        px(offset.1) + px(size.cy) / 2.0,
                    ),
                    charge,
                    hydrogens,
                });
            } else if let Some(rest) = name.strip_prefix("chem:v1:bond:") {
                let fields: Vec<&str> = rest.split(':').collect();
                if fields.len() != 6 {
                    return Err("Malformed bond shape".into());
                }
                let structure = fields[0].parse::<u64>().map_err(|e| e.to_string())?;
                per.entry(structure).or_default().1.push(Bond {
                    id: fields[1].parse::<u64>().map_err(|e| e.to_string())?,
                    a: fields[2].parse::<u64>().map_err(|e| e.to_string())?,
                    b: fields[3].parse::<u64>().map_err(|e| e.to_string())?,
                    order: parse_order(fields[4]),
                    stereo: parse_stereo(fields[5]),
                });
            } else {
                plain += 1;
            }
        }
        let mut document = ChemDocument::default();
        for (id, (atoms, bonds)) in per {
            if atoms.is_empty() {
                continue;
            }
            document.objects.push(ChemObject::Structure(Structure {
                id,
                origin: Point::default(),
                atoms,
                bonds,
            }));
        }
        document.validate()?;
        let mut issues = vec![];
        if plain > 0 {
            issues.push("Reactions are not stored in the drawing".into());
        }
        Ok((document, issues))
    }
}

fn push_structure_shapes(
    structure: &Structure,
    offset: Point,
    next_id: &mut u32,
    out: &mut Vec<w::drawing::Graphic>,
) {
    let (min, _) = structure.bounds();
    let shift = Point::new(min.x - 24.0, min.y - 24.0);
    for bond in &structure.bonds {
        let (Some(a), Some(b)) = (
            structure
                .atom(bond.a)
                .map(|atom| atom.position.add(structure.origin)),
            structure
                .atom(bond.b)
                .map(|atom| atom.position.add(structure.origin)),
        ) else {
            continue;
        };
        let a = a.sub(shift).add(offset);
        let b = b.sub(shift).add(offset);
        let id = *next_id;
        *next_id += 1;
        out.push(bond_shape(structure.id, bond, a, b, id));
    }
    for atom in &structure.atoms {
        let world = atom.position.add(structure.origin).sub(shift).add(offset);
        let id = *next_id;
        *next_id += 1;
        out.push(atom_shape(structure, atom, world, id));
    }
}

fn shift_prim(prim: &crate::render::Prim, delta: Point) -> crate::render::Prim {
    use crate::render::Prim;
    match prim {
        Prim::Line {
            from,
            to,
            width,
            color,
        } => Prim::Line {
            from: from.add(delta),
            to: to.add(delta),
            width: *width,
            color: *color,
        },
        Prim::Polygon { points, fill } => Prim::Polygon {
            points: points.iter().map(|p| p.add(delta)).collect(),
            fill: *fill,
        },
        Prim::Text {
            pos,
            text,
            size,
            color,
            align,
            italic,
        } => Prim::Text {
            pos: pos.add(delta),
            text: text.clone(),
            size: *size,
            color: *color,
            align: *align,
            italic: *italic,
        },
    }
}

fn prim_shape(prim: &crate::render::Prim, id: u32) -> w::drawing::Shape {
    use crate::render::{Align, Prim};
    match prim {
        Prim::Line {
            from,
            to,
            width,
            color,
        } => {
            let min = Point::new(from.x.min(to.x), from.y.min(to.y));
            let size = Point::new(
                (from.x - to.x).abs().max(1.0),
                (from.y - to.y).abs().max(1.0),
            );
            let mut shape = base_shape(id, min, size);
            shape.geometry = w::drawing::ShapeGeometry::Custom(w::drawing::CustomGeometry {
                paths: vec![w::drawing::GeometryPath {
                    width: emu(size.x).0,
                    height: emu(size.y).0,
                    fill: Some("none".into()),
                    stroke: Some(true),
                    commands: vec![
                        w::drawing::PathCommand::MoveTo {
                            x: emu(from.x - min.x).0,
                            y: emu(from.y - min.y).0,
                        },
                        w::drawing::PathCommand::LineTo {
                            x: emu(to.x - min.x).0,
                            y: emu(to.y - min.y).0,
                        },
                    ],
                }],
                ..Default::default()
            });
            shape.fill = Some(w::drawing::ShapeFill::None);
            shape.stroke = Some(w::drawing::ShapeStroke {
                color: Some(rgb(*color)),
                width: Some(emu(*width)),
                ..Default::default()
            });
            shape
        }
        Prim::Polygon { points, fill } => {
            let min = points
                .iter()
                .fold(Point::new(f32::INFINITY, f32::INFINITY), |min, p| {
                    Point::new(min.x.min(p.x), min.y.min(p.y))
                });
            let max = points.iter().fold(
                Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
                |max, p| Point::new(max.x.max(p.x), max.y.max(p.y)),
            );
            let size = Point::new((max.x - min.x).max(1.0), (max.y - min.y).max(1.0));
            let mut commands = vec![];
            for (index, point) in points.iter().enumerate() {
                let (x, y) = (emu(point.x - min.x).0, emu(point.y - min.y).0);
                commands.push(if index == 0 {
                    w::drawing::PathCommand::MoveTo { x, y }
                } else {
                    w::drawing::PathCommand::LineTo { x, y }
                });
            }
            commands.push(w::drawing::PathCommand::Close);
            let mut shape = base_shape(id, min, size);
            shape.geometry = w::drawing::ShapeGeometry::Custom(w::drawing::CustomGeometry {
                paths: vec![w::drawing::GeometryPath {
                    width: emu(size.x).0,
                    height: emu(size.y).0,
                    fill: Some("norm".into()),
                    stroke: Some(false),
                    commands,
                }],
                ..Default::default()
            });
            shape.fill = Some(w::drawing::ShapeFill::Solid { color: rgb(*fill) });
            shape.stroke = Some(w::drawing::ShapeStroke {
                none: true,
                ..Default::default()
            });
            shape
        }
        Prim::Text {
            pos,
            text,
            size,
            color,
            align,
            ..
        } => {
            let width = text_width(text, *size) + 2.0;
            let left = match align {
                Align::Left => pos.x,
                Align::Center => pos.x - width / 2.0,
                Align::Right => pos.x - width,
            };
            #[allow(clippy::cast_possible_truncation)]
            let half_points = (size * 1.5).round() as i32;
            let mut shape = base_shape(
                id,
                Point::new(left, pos.y - size * 0.8),
                Point::new(width, size * 1.6),
            );
            shape.tx_box = Some(true);
            shape.geometry = w::drawing::ShapeGeometry::Preset("rect".into());
            shape.fill = Some(w::drawing::ShapeFill::None);
            shape.stroke = Some(w::drawing::ShapeStroke {
                none: true,
                ..Default::default()
            });
            shape.text = Some(w::drawing::TextBox {
                body: Some(w::drawing::TextBoxBody {
                    anchor: Some(w::drawing::TextAnchor::Center),
                    wrap: Some("none".into()),
                    left_inset: Some(w::Emu(0)),
                    right_inset: Some(w::Emu(0)),
                    top_inset: Some(w::Emu(0)),
                    bottom_inset: Some(w::Emu(0)),
                    ..Default::default()
                }),
                blocks: vec![w::Block::Paragraph(paragraph(vec![run(
                    text,
                    half_points.max(1),
                    None,
                    *color,
                )]))],
                location: loc(),
            });
            shape
        }
    }
}

fn inline_drawing(id: u32, graphic: w::drawing::Graphic, size: Point, name: &str) -> w::Drawing {
    w::Drawing {
        kind: w::DrawingKind::Inline(w::drawing::InlineDrawing {
            extent: Some(extent(size)),
            effect_extent: None,
            doc_pr: Some(w::drawing::DocPr {
                id: Some(id),
                name: Some(name.into()),
                ..Default::default()
            }),
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            graphic_uri: None,
            graphic: Box::new(graphic),
            location: loc(),
        }),
        location: loc(),
    }
}

fn structure_graphic(
    structure: &Structure,
    next_id: &mut u32,
) -> (w::drawing::Graphic, Point, u32) {
    let (min, max) = structure.bounds();
    let size = Point::new(
        (max.x - min.x + 48.0).max(48.0),
        (max.y - min.y + 48.0).max(48.0),
    );
    let shift = Point::new(min.x - 24.0, min.y - 24.0);
    let mut children = vec![];
    for bond in &structure.bonds {
        let (Some(a), Some(b)) = (
            structure
                .atom(bond.a)
                .map(|atom| atom.position.add(structure.origin)),
            structure
                .atom(bond.b)
                .map(|atom| atom.position.add(structure.origin)),
        ) else {
            continue;
        };
        let a = a.sub(shift);
        let b = b.sub(shift);
        let id = *next_id;
        *next_id += 1;
        children.push(bond_shape(structure.id, bond, a, b, id));
    }
    for atom in &structure.atoms {
        let id = *next_id;
        *next_id += 1;
        let world = atom.position.add(structure.origin).sub(shift);
        children.push(atom_shape(structure, atom, world, id));
    }
    let group_id = *next_id;
    *next_id += 1;
    let graphic = w::drawing::Graphic::Group(w::drawing::GroupShape {
        nv_id: Some(group_id),
        name: Some(format!("chem:v1:structure:{}", structure.id).into()),
        descr: None,
        xfrm: Some(w::drawing::GroupTransform {
            offset: Some((w::Emu(0), w::Emu(0))),
            extent: Some(extent(size)),
            child_offset: Some((w::Emu(0), w::Emu(0))),
            child_extent: Some(extent(size)),
            ..Default::default()
        }),
        locks: None,
        bw_mode: None,
        children,
        location: loc(),
    });
    (graphic, size, group_id)
}

fn bond_shape(structure_id: u64, bond: &Bond, a: Point, b: Point, id: u32) -> w::drawing::Graphic {
    let min = Point::new(a.x.min(b.x), a.y.min(b.y));
    let size = Point::new((a.x - b.x).abs().max(1.0), (a.y - b.y).abs().max(1.0));
    let mut shape = base_shape(id, min, size);
    shape.connector = true;
    shape.name = Some(
        format!(
            "chem:v1:bond:{structure_id}:{}:{}:{}:{}:{}",
            bond.id,
            bond.a,
            bond.b,
            order_code(bond.order),
            stereo_code(bond.stereo)
        )
        .into(),
    );
    let path_width = emu(size.x).0;
    let path_height = emu(size.y).0;
    let a_rel = Point::new(a.x - min.x, a.y - min.y);
    let b_rel = Point::new(b.x - min.x, b.y - min.y);
    let delta = Point::new(b.x - a.x, b.y - a.y);
    let length = delta.length();
    let perp = if length > 0.001 {
        Point::new(-delta.y / length, delta.x / length)
    } else {
        Point::new(0.0, 0.0)
    };
    let emu_point = |p: Point| (emu(p.x).0, emu(p.y).0);
    match bond.stereo {
        BondStereo::Wedge => {
            let half = 5.0_f32.min(length * 0.35);
            let (ax, ay) = emu_point(a_rel);
            let (b1x, b1y) =
                emu_point(Point::new(b_rel.x + perp.x * half, b_rel.y + perp.y * half));
            let (b2x, b2y) =
                emu_point(Point::new(b_rel.x - perp.x * half, b_rel.y - perp.y * half));
            let commands = vec![
                w::drawing::PathCommand::MoveTo { x: ax, y: ay },
                w::drawing::PathCommand::LineTo { x: b1x, y: b1y },
                w::drawing::PathCommand::LineTo { x: b2x, y: b2y },
                w::drawing::PathCommand::Close,
            ];
            shape.geometry = w::drawing::ShapeGeometry::Custom(w::drawing::CustomGeometry {
                paths: vec![w::drawing::GeometryPath {
                    width: path_width,
                    height: path_height,
                    fill: Some("norm".into()),
                    stroke: Some(false),
                    commands,
                }],
                ..Default::default()
            });
            shape.fill = Some(w::drawing::ShapeFill::Solid {
                color: rgb([30, 30, 30]),
            });
            shape.stroke = Some(w::drawing::ShapeStroke {
                none: true,
                ..Default::default()
            });
        }
        BondStereo::Hash => {
            let half = 5.0_f32.min(length * 0.35);
            let steps = 6_u16;
            let mut commands = vec![];
            for step in 1..=steps {
                let t = f32::from(step) / f32::from(steps);
                let center = Point::new(a_rel.x + delta.x * t, a_rel.y + delta.y * t);
                let widen = half * t;
                let (x1, y1) = emu_point(Point::new(
                    center.x + perp.x * widen,
                    center.y + perp.y * widen,
                ));
                let (x2, y2) = emu_point(Point::new(
                    center.x - perp.x * widen,
                    center.y - perp.y * widen,
                ));
                commands.push(w::drawing::PathCommand::MoveTo { x: x1, y: y1 });
                commands.push(w::drawing::PathCommand::LineTo { x: x2, y: y2 });
            }
            shape.geometry = w::drawing::ShapeGeometry::Custom(w::drawing::CustomGeometry {
                paths: vec![w::drawing::GeometryPath {
                    width: path_width,
                    height: path_height,
                    fill: Some("none".into()),
                    stroke: Some(true),
                    commands,
                }],
                ..Default::default()
            });
            shape.fill = Some(w::drawing::ShapeFill::None);
            shape.stroke = Some(w::drawing::ShapeStroke {
                color: Some(rgb([30, 30, 30])),
                width: Some(emu(1.3)),
                ..Default::default()
            });
        }
        BondStereo::Plain => {
            let commands = vec![
                w::drawing::PathCommand::MoveTo {
                    x: emu(a_rel.x).0,
                    y: emu(a_rel.y).0,
                },
                w::drawing::PathCommand::LineTo {
                    x: emu(b_rel.x).0,
                    y: emu(b_rel.y).0,
                },
            ];
            shape.geometry = w::drawing::ShapeGeometry::Custom(w::drawing::CustomGeometry {
                paths: vec![w::drawing::GeometryPath {
                    width: path_width,
                    height: path_height,
                    fill: Some("none".into()),
                    stroke: Some(true),
                    commands,
                }],
                ..Default::default()
            });
            shape.fill = Some(w::drawing::ShapeFill::None);
            shape.stroke = Some(w::drawing::ShapeStroke {
                color: Some(rgb([30, 30, 30])),
                width: Some(emu(1.8)),
                ..Default::default()
            });
        }
    }
    w::drawing::Graphic::Shape(shape)
}

fn atom_shape(structure: &Structure, atom: &Atom, world: Point, id: u32) -> w::drawing::Graphic {
    let label = atom_label(structure, atom);
    let width =
        (text_width(&label, 18.0) + text_width(&charge_text(atom.charge), 12.6) + 6.0).max(20.0);
    let height = 26.0;
    let top_left = Point::new(world.x - width / 2.0, world.y - height / 2.0);
    let mut shape = base_shape(id, top_left, Point::new(width, height));
    shape.tx_box = Some(true);
    shape.name = Some(
        format!(
            "chem:v1:atom:{}:{}:{}:{}:{}",
            structure.id,
            atom.id,
            atom.element,
            atom.charge,
            atom.hydrogens.map_or("-".to_owned(), |h| h.to_string())
        )
        .into(),
    );
    shape.geometry = w::drawing::ShapeGeometry::Preset("rect".into());
    shape.fill = Some(w::drawing::ShapeFill::None);
    shape.stroke = Some(w::drawing::ShapeStroke {
        none: true,
        ..Default::default()
    });
    let color = element::element(atom.element).map_or([30, 30, 30], element::ElementInfo::color);
    let mut inlines = vec![run(&label, FONT_HALF_POINTS, None, color)];
    if atom.charge != 0 {
        inlines.push(run(
            &charge_text(atom.charge),
            SMALL_HALF_POINTS,
            Some(w::VertAlign::Superscript),
            color,
        ));
    }
    shape.text = Some(w::drawing::TextBox {
        body: Some(w::drawing::TextBoxBody {
            anchor: Some(w::drawing::TextAnchor::Center),
            wrap: Some("none".into()),
            ..Default::default()
        }),
        blocks: vec![w::Block::Paragraph(paragraph(inlines))],
        location: loc(),
    });
    w::drawing::Graphic::Shape(shape)
}

fn atom_label(structure: &Structure, atom: &Atom) -> String {
    let symbol = atom.symbol();
    let hydrogens = structure.hydrogens(atom);
    if atom.element != 6 && hydrogens > 0 {
        if hydrogens == 1 {
            format!("{symbol}H")
        } else {
            format!("{symbol}H{hydrogens}")
        }
    } else {
        symbol.to_owned()
    }
}

fn order_code(order: BondOrder) -> &'static str {
    match order {
        BondOrder::Single => "s",
        BondOrder::Double => "d",
        BondOrder::Triple => "t",
        BondOrder::Aromatic => "a",
    }
}

fn stereo_code(stereo: BondStereo) -> &'static str {
    match stereo {
        BondStereo::Plain => "p",
        BondStereo::Wedge => "w",
        BondStereo::Hash => "h",
    }
}

fn parse_order(code: &str) -> BondOrder {
    match code {
        "d" => BondOrder::Double,
        "t" => BondOrder::Triple,
        "a" => BondOrder::Aromatic,
        _ => BondOrder::Single,
    }
}

fn parse_stereo(code: &str) -> BondStereo {
    match code {
        "w" => BondStereo::Wedge,
        "h" => BondStereo::Hash,
        _ => BondStereo::Plain,
    }
}

fn charge_text(charge: i8) -> String {
    match charge {
        0 => String::new(),
        1 => "+".into(),
        -1 => "-".into(),
        c if c > 0 => format!("{c}+"),
        c => format!("{}-", c.unsigned_abs()),
    }
}

fn reaction_paragraph(reaction: &Reaction) -> w::Paragraph {
    let mut inlines = vec![];
    let mut first = true;
    for species in &reaction.reactants {
        if !first {
            inlines.push(run(" + ", FONT_HALF_POINTS, None, [30, 30, 30]));
        }
        species_inlines(species, &mut inlines);
        first = false;
    }
    inlines.push(run("  ", FONT_HALF_POINTS, None, [30, 30, 30]));
    inlines.push(run(
        &arrow_text(reaction),
        FONT_HALF_POINTS,
        None,
        [30, 30, 30],
    ));
    inlines.push(run("  ", FONT_HALF_POINTS, None, [30, 30, 30]));
    first = true;
    for species in &reaction.products {
        if !first {
            inlines.push(run(" + ", FONT_HALF_POINTS, None, [30, 30, 30]));
        }
        species_inlines(species, &mut inlines);
        first = false;
    }
    paragraph(inlines)
}

fn arrow_text(reaction: &Reaction) -> String {
    use crate::model::ArrowKind;
    let mut text = match reaction.arrow.kind {
        ArrowKind::Right => "→".to_owned(),
        ArrowKind::Left => "←".to_owned(),
        ArrowKind::LeftRight => "↔".to_owned(),
        ArrowKind::Equilibrium => "⇌".to_owned(),
        ArrowKind::Double => "⇒".to_owned(),
        ArrowKind::ReverseEquilibrium => "⇐".to_owned(),
    };
    if !reaction.arrow.above.is_empty() {
        let _ = write!(text, "[{}]", reaction.arrow.above.join(", "));
    }
    if !reaction.arrow.below.is_empty() {
        let _ = write!(text, "[{}]", reaction.arrow.below.join(", "));
    }
    text
}

fn species_inlines(species: &Species, out: &mut Vec<w::Inline>) {
    let ink = [30, 30, 30];
    if species.coefficient != 1 {
        out.push(run(
            &species.coefficient.to_string(),
            FONT_HALF_POINTS,
            None,
            ink,
        ));
    }
    parts_inlines(&species.parts, out);
    if species.charge != 0 {
        out.push(run(
            &charge_text(species.charge),
            SMALL_HALF_POINTS,
            Some(w::VertAlign::Superscript),
            ink,
        ));
    }
    if species.state != State::None {
        out.push(run(
            &format!("({})", species.state.code()),
            FONT_HALF_POINTS,
            None,
            ink,
        ));
    }
    match species.marker {
        Marker::None => {}
        Marker::Gas => out.push(run("↑", FONT_HALF_POINTS, None, ink)),
        Marker::Precipitate => out.push(run("↓", FONT_HALF_POINTS, None, ink)),
    }
}

fn parts_inlines(parts: &[Part], out: &mut Vec<w::Inline>) {
    for part in parts {
        match part {
            Part::Element {
                symbol,
                count,
                isotope,
            } => {
                let color =
                    element::by_symbol(symbol).map_or([30, 30, 30], element::ElementInfo::color);
                if let Some(mass) = isotope {
                    out.push(run(
                        &mass.to_string(),
                        SMALL_HALF_POINTS,
                        Some(w::VertAlign::Superscript),
                        color,
                    ));
                }
                out.push(run(symbol, FONT_HALF_POINTS, None, color));
                if *count > 1 {
                    out.push(run(
                        &count.to_string(),
                        SMALL_HALF_POINTS,
                        Some(w::VertAlign::Subscript),
                        color,
                    ));
                }
            }
            Part::Group { parts, count } => {
                out.push(run("(", FONT_HALF_POINTS, None, [30, 30, 30]));
                parts_inlines(parts, out);
                out.push(run(")", FONT_HALF_POINTS, None, [30, 30, 30]));
                if *count > 1 {
                    out.push(run(
                        &count.to_string(),
                        SMALL_HALF_POINTS,
                        Some(w::VertAlign::Subscript),
                        [30, 30, 30],
                    ));
                }
            }
            Part::Dot => out.push(run(".", FONT_HALF_POINTS, None, [30, 30, 30])),
        }
    }
}

fn reaction_width(reaction: &Reaction) -> f32 {
    let estimate = |species: &[Species]| -> f32 {
        species
            .iter()
            .map(|species| text_width(&crate::mhchem::write_species(species), 18.0) + 12.0)
            .sum()
    };
    estimate(&reaction.reactants) + estimate(&reaction.products) + 120.0
}

fn normalize_import_text(text: &str) -> String {
    text.replace('→', "->")
        .replace('←', "<-")
        .replace('↔', "<->")
        .replace('⇌', "<=>")
        .replace('⇒', "=>")
        .replace('⇐', "<=")
        .replace('↑', " ^")
        .replace('↓', " v")
        .replace('\u{a0}', " ")
}

fn import_structure(drawing: &w::Drawing, next_id: &mut u64) -> Result<Option<Structure>, String> {
    let w::DrawingKind::Inline(inline) = &drawing.kind else {
        return Ok(None);
    };
    let w::drawing::Graphic::Group(group) = inline.graphic.as_ref() else {
        return Ok(None);
    };
    if !group
        .name
        .as_deref()
        .is_some_and(|name| name.starts_with("chem:v1:structure:"))
    {
        return Ok(None);
    }
    let mut atoms = vec![];
    let mut bonds = vec![];
    for child in &group.children {
        let w::drawing::Graphic::Shape(shape) = child else {
            return Err("Nested graphics are not supported".into());
        };
        let name = shape.name.as_deref().unwrap_or_default();
        if let Some(rest) = name.strip_prefix("chem:v1:atom:") {
            let fields: Vec<&str> = rest.split(':').collect();
            if fields.len() != 5 {
                return Err("Malformed atom shape".into());
            }
            let id = fields[1].parse::<u64>().map_err(|e| e.to_string())?;
            let element = fields[2].parse::<u8>().map_err(|e| e.to_string())?;
            let charge = fields[3].parse::<i8>().map_err(|e| e.to_string())?;
            let hydrogens = if fields[4] == "-" {
                None
            } else {
                Some(fields[4].parse::<u8>().map_err(|e| e.to_string())?)
            };
            let offset = shape.offset.ok_or("Missing atom position")?;
            let size = shape.extent.ok_or("Missing atom extent")?;
            atoms.push(Atom {
                id,
                element,
                position: Point::new(
                    px(offset.0) + px(size.cx) / 2.0,
                    px(offset.1) + px(size.cy) / 2.0,
                ),
                charge,
                hydrogens,
            });
        } else if let Some(rest) = name.strip_prefix("chem:v1:bond:") {
            let fields: Vec<&str> = rest.split(':').collect();
            if fields.len() != 6 {
                return Err("Malformed bond shape".into());
            }
            bonds.push(Bond {
                id: fields[1].parse::<u64>().map_err(|e| e.to_string())?,
                a: fields[2].parse::<u64>().map_err(|e| e.to_string())?,
                b: fields[3].parse::<u64>().map_err(|e| e.to_string())?,
                order: parse_order(fields[4]),
                stereo: parse_stereo(fields[5]),
            });
        }
    }
    let max_used = atoms
        .iter()
        .map(|atom| atom.id)
        .chain(bonds.iter().map(|bond| bond.id))
        .max()
        .unwrap_or(0);
    let structure_id = max_used.max(*next_id).saturating_add(1);
    *next_id = structure_id + 1;
    Ok(Some(Structure {
        id: structure_id,
        origin: Point::default(),
        atoms,
        bonds,
    }))
}
