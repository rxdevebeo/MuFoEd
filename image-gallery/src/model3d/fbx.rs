//! Binary FBX (version 7.x): meshes with normals, texture coordinates and
//! materials, textures, and the model hierarchy with its local transforms.
//! Animation stacks, skins and the ASCII variant are not read.

// FBX stores 64-bit indices and doubles; they are narrowed to the scene's
// u32 and f32 after range checks.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::collections::HashMap;
use std::path::Path;

use glam::{EulerRot, Quat, Vec2, Vec3, Vec4};

use super::scene::{Material, Mesh, Node, Primitive, Scene, Transform, UpAxis};
use crate::Error;

const MAGIC: &[u8] = b"Kaydara FBX Binary  \0";

/// Whether the data is a binary FBX file.
pub(crate) fn is_fbx(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

/// A property value of an FBX node record.
#[derive(Clone, Debug)]
enum Value {
    Int(i64),
    Float(f64),
    Text(String),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    Other,
}

impl Value {
    fn int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Float(value) => Some(*value as i64),
            _ => None,
        }
    }

    fn float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// A node record.
#[derive(Clone, Debug, Default)]
struct Record {
    name: String,
    values: Vec<Value>,
    children: Vec<Record>,
}

impl Record {
    fn child(&self, name: &str) -> Option<&Record> {
        self.children.iter().find(|child| child.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Record> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// The first value of a child record, such as `Vertices`.
    fn first(&self, name: &str) -> Option<&Value> {
        self.child(name)?.values.first()
    }

    /// The values of a `Properties70` entry: `P: "name", type, label, flags, values...`.
    fn property(&self, name: &str) -> Option<&[Value]> {
        self.child("Properties70")?
            .children_named("P")
            .find(|entry| entry.values.first().and_then(Value::text) == Some(name))
            .map(|entry| entry.values.get(4..).unwrap_or_default())
    }

    fn property_vec3(&self, name: &str) -> Option<Vec3> {
        let values = self.property(name)?;
        Some(Vec3::new(
            values.first()?.float()? as f32,
            values.get(1)?.float()? as f32,
            values.get(2)?.float()? as f32,
        ))
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    wide: bool,
}

fn short() -> Error {
    Error::Codec("FBX: the file ends early".into())
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(short)?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let bytes = self.take(8)?;
        let mut array = [0; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(array))
    }

    fn offset(&mut self) -> Result<u64, Error> {
        if self.wide {
            self.u64()
        } else {
            self.u32().map(u64::from)
        }
    }

    /// One node record and its children; `None` at a list's null terminator.
    fn record(&mut self, depth: usize) -> Result<Option<Record>, Error> {
        if depth > 64 {
            return Err(Error::Codec("FBX: nesting too deep".into()));
        }
        let end = usize::try_from(self.offset()?).map_err(|_| short())?;
        let count = self.offset()?;
        let _list_length = self.offset()?;
        let name_length = usize::from(self.u8()?);
        if end == 0 {
            return Ok(None);
        }
        if end > self.data.len() || end < self.at {
            return Err(Error::Codec("FBX: a record points outside the file".into()));
        }
        let name = String::from_utf8_lossy(self.take(name_length)?).into_owned();
        let mut values = Vec::new();
        for _ in 0..count.min(1 << 20) {
            values.push(self.value()?);
        }
        let mut children = Vec::new();
        while self.at < end {
            match self.record(depth + 1)? {
                Some(child) => children.push(child),
                None => break,
            }
        }
        self.at = end;
        Ok(Some(Record {
            name,
            values,
            children,
        }))
    }

    fn value(&mut self) -> Result<Value, Error> {
        let code = self.u8()?;
        Ok(match code {
            b'Y' => {
                let bytes = self.take(2)?;
                Value::Int(i64::from(i16::from_le_bytes([bytes[0], bytes[1]])))
            }
            b'C' => Value::Int(i64::from(self.u8()?)),
            b'I' => Value::Int(i64::from(self.u32()?.cast_signed())),
            b'L' => Value::Int(self.u64()?.cast_signed()),
            b'F' => Value::Float(f64::from(f32::from_bits(self.u32()?))),
            b'D' => Value::Float(f64::from_bits(self.u64()?)),
            b'S' | b'R' => {
                let length = self.u32()? as usize;
                let bytes = self.take(length)?;
                if code == b'S' {
                    Value::Text(String::from_utf8_lossy(bytes).into_owned())
                } else {
                    Value::Other
                }
            }
            b'f' | b'd' | b'l' | b'i' | b'b' => self.array(code)?,
            _ => return Err(Error::Codec(format!("FBX: unknown property type {code}"))),
        })
    }

    fn array(&mut self, code: u8) -> Result<Value, Error> {
        let count = self.u32()? as usize;
        let encoding = self.u32()?;
        let length = self.u32()? as usize;
        let raw = self.take(length)?;
        let size = match code {
            b'f' | b'i' => 4,
            b'd' | b'l' => 8,
            _ => 1,
        };
        let expected = count.checked_mul(size).ok_or_else(short)?;
        let bytes = if encoding == 1 {
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, expected)
                .map_err(|_| Error::Codec("FBX: bad compressed array".into()))?
        } else {
            raw.to_vec()
        };
        if bytes.len() < expected {
            return Err(short());
        }
        let chunks = bytes[..expected].chunks_exact(size);
        Ok(match code {
            b'f' => Value::Floats(
                chunks
                    .map(|c| f64::from(f32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                    .collect(),
            ),
            b'd' => Value::Floats(
                chunks
                    .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                    .collect(),
            ),
            b'i' => Value::Ints(
                chunks
                    .map(|c| i64::from(i32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                    .collect(),
            ),
            b'l' => Value::Ints(
                chunks
                    .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                    .collect(),
            ),
            _ => Value::Ints(chunks.map(|c| i64::from(c[0])).collect()),
        })
    }
}

/// Parses the record tree.
fn parse(data: &[u8]) -> Result<Record, Error> {
    if !is_fbx(data) {
        return Err(Error::Codec(
            "FBX: not a binary FBX file (ASCII FBX is not read)".into(),
        ));
    }
    let mut reader = Reader {
        data,
        at: 23,
        wide: false,
    };
    let version = reader.u32()?;
    reader.wide = version >= 7500;
    let mut root = Record::default();
    while reader.at < data.len() {
        match reader.record(0) {
            Ok(Some(record)) => root.children.push(record),
            Ok(None) | Err(_) => break,
        }
    }
    if root.children.is_empty() {
        return Err(Error::Codec("FBX: no records".into()));
    }
    Ok(root)
}

/// Reads a binary FBX file; textures are looked for beside `folder`.
// One pass over the object graph, top to bottom.
#[allow(clippy::too_many_lines)]
pub(crate) fn load(data: &[u8], folder: Option<&Path>) -> Result<Scene, Error> {
    let root = parse(data)?;
    let objects = root
        .child("Objects")
        .ok_or_else(|| Error::Codec("FBX: no objects".into()))?;
    let id = |record: &Record| record.values.first().and_then(Value::int);
    // Connections: child -> parents (object-object), and object-property links.
    let mut parents: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut property_links: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    if let Some(connections) = root.child("Connections") {
        for link in connections.children_named("C") {
            let kind = link
                .values
                .first()
                .and_then(Value::text)
                .unwrap_or_default();
            let (Some(child), Some(parent)) = (
                link.values.get(1).and_then(Value::int),
                link.values.get(2).and_then(Value::int),
            ) else {
                continue;
            };
            if kind == "OO" {
                parents.entry(child).or_default().push(parent);
            } else if kind == "OP" {
                let property = link
                    .values
                    .get(3)
                    .and_then(Value::text)
                    .unwrap_or_default()
                    .to_owned();
                property_links
                    .entry(child)
                    .or_default()
                    .push((parent, property));
            }
        }
    }
    let up = root
        .child("GlobalSettings")
        .and_then(|settings| settings.property("UpAxis"))
        .and_then(|values| values.first()?.int())
        .map_or(
            UpAxis::Y,
            |axis| if axis == 2 { UpAxis::Z } else { UpAxis::Y },
        );
    let mut scene = Scene {
        up,
        ..Scene::default()
    };

    // Textures and materials.
    let mut texture_of_material: HashMap<i64, usize> = HashMap::new();
    for texture in objects.children_named("Texture") {
        let Some(texture_id) = id(texture) else {
            continue;
        };
        let file = texture
            .first("RelativeFilename")
            .and_then(Value::text)
            .filter(|name| !name.is_empty())
            .or_else(|| texture.first("FileName").and_then(Value::text));
        let loaded = file.and_then(|file| {
            let folder = folder?;
            let file = file.replace('\\', "/");
            let candidate = folder.join(&file);
            let path = if candidate.is_file() {
                candidate
            } else {
                folder.join(Path::new(&file).file_name()?)
            };
            super::mesh_formats::texture_file(&path)
        });
        if let Some(loaded) = loaded {
            scene.textures.push(loaded);
            let index = scene.textures.len() - 1;
            for (parent, property) in property_links.get(&texture_id).into_iter().flatten() {
                if property.contains("Diffuse") || property.contains("BaseColor") {
                    texture_of_material.insert(*parent, index);
                }
            }
        }
    }
    let mut materials: HashMap<i64, usize> = HashMap::new();
    for material in objects.children_named("Material") {
        let Some(material_id) = id(material) else {
            continue;
        };
        let colour = material
            .property_vec3("DiffuseColor")
            .or_else(|| material.property_vec3("Diffuse"))
            .unwrap_or(Vec3::splat(0.8));
        let opacity = material
            .property("Opacity")
            .and_then(|values| values.first()?.float())
            .map_or(1.0, |value| value as f32);
        let texture = texture_of_material.get(&material_id).copied();
        scene.materials.push(Material {
            name: object_name(material),
            base_color: if texture.is_some() {
                Vec4::new(1.0, 1.0, 1.0, opacity)
            } else {
                colour.extend(opacity)
            },
            texture,
            double_sided: true,
            emissive: material
                .property_vec3("EmissiveColor")
                .unwrap_or(Vec3::ZERO)
                * material
                    .property("EmissiveFactor")
                    .and_then(|values| values.first()?.float())
                    .map_or(1.0, |value| value as f32),
        });
        materials.insert(material_id, scene.materials.len() - 1);
    }

    // Models (nodes).
    let mut nodes: HashMap<i64, usize> = HashMap::new();
    for model in objects.children_named("Model") {
        let Some(model_id) = id(model) else {
            continue;
        };
        let translation = model.property_vec3("Lcl Translation").unwrap_or(Vec3::ZERO);
        let rotation = model.property_vec3("Lcl Rotation").unwrap_or(Vec3::ZERO);
        let pre = model.property_vec3("PreRotation").unwrap_or(Vec3::ZERO);
        let scale = model.property_vec3("Lcl Scaling").unwrap_or(Vec3::ONE);
        let euler = |degrees: Vec3| {
            Quat::from_euler(
                EulerRot::ZYX,
                degrees.z.to_radians(),
                degrees.y.to_radians(),
                degrees.x.to_radians(),
            )
        };
        scene.nodes.push(Node {
            name: object_name(model),
            transform: Transform {
                translation,
                rotation: euler(pre) * euler(rotation),
                scale,
            },
            ..Node::default()
        });
        nodes.insert(model_id, scene.nodes.len() - 1);
    }

    // Geometry, attached to the models that use it.
    for geometry in objects.children_named("Geometry") {
        let Some(geometry_id) = id(geometry) else {
            continue;
        };
        let models: Vec<usize> = parents
            .get(&geometry_id)
            .into_iter()
            .flatten()
            .filter_map(|parent| nodes.get(parent).copied())
            .collect();
        let Some(&first_model) = models.first() else {
            continue;
        };
        // Materials of the model, in connection order.
        let model_id = nodes
            .iter()
            .find(|(_, &index)| index == first_model)
            .map(|(&id, _)| id);
        let model_materials: Vec<usize> = objects
            .children_named("Material")
            .filter_map(|material| {
                let material_id = id(material)?;
                let linked = parents
                    .get(&material_id)?
                    .iter()
                    .any(|parent| Some(*parent) == model_id);
                linked
                    .then(|| materials.get(&material_id).copied())
                    .flatten()
            })
            .collect();
        let Some(primitives) = mesh_primitives(geometry, &model_materials) else {
            continue;
        };
        scene.meshes.push(Mesh {
            name: object_name(geometry),
            primitives,
            weights: Vec::new(),
        });
        let mesh = scene.meshes.len() - 1;
        for model in models {
            scene.nodes[model].mesh = Some(mesh);
        }
    }

    // The hierarchy: model -> parent model, or the scene root (ID 0).
    for (&model_id, &index) in &nodes {
        for parent in parents.get(&model_id).into_iter().flatten() {
            if let Some(&parent_index) = nodes.get(parent) {
                scene.nodes[parent_index].children.push(index);
            }
        }
    }
    scene.finish();
    Ok(scene)
}

/// The display name of an object: `Name::Class` → `Name`.
fn object_name(record: &Record) -> String {
    let full = record
        .values
        .get(1)
        .and_then(Value::text)
        .unwrap_or_default();
    full.split(['\0', '\u{1}'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// A layer element's values for each polygon vertex, resolving its mapping
/// and reference modes.
fn layer_values(
    layer: &Record,
    data_name: &str,
    index_name: &str,
    width: usize,
    polygon_vertex_count: usize,
    vertex_of: &[usize],
    polygon_of: &[usize],
) -> Option<Vec<Vec<f64>>> {
    let Some(Value::Floats(data)) = layer.first(data_name) else {
        return None;
    };
    let indices = match layer.first(index_name) {
        Some(Value::Ints(indices)) => Some(indices.as_slice()),
        _ => None,
    };
    let mapping = layer
        .first("MappingInformationType")
        .and_then(Value::text)
        .unwrap_or("ByPolygonVertex");
    let direct = layer
        .first("ReferenceInformationType")
        .and_then(Value::text)
        == Some("Direct");
    let item = |slot: usize| -> Option<Vec<f64>> {
        let element = if direct {
            slot
        } else {
            usize::try_from(*indices?.get(slot)?).ok()?
        };
        data.get(element * width..element * width + width)
            .map(<[f64]>::to_vec)
    };
    (0..polygon_vertex_count)
        .map(|corner| {
            let slot = match mapping {
                "ByVertice" | "ByVertex" | "ByControlPoint" => vertex_of[corner],
                "ByPolygon" => polygon_of[corner],
                "AllSame" => 0,
                _ => corner,
            };
            item(slot)
        })
        .collect()
}

/// The triangles of a `Geometry` record, one primitive per material.
#[allow(clippy::too_many_lines)]
fn mesh_primitives(geometry: &Record, model_materials: &[usize]) -> Option<Vec<Primitive>> {
    let Some(Value::Floats(points)) = geometry.first("Vertices") else {
        return None;
    };
    let Some(Value::Ints(polygon_vertices)) = geometry.first("PolygonVertexIndex") else {
        return None;
    };
    // Corners: control point of each, and the polygon each belongs to.
    let mut vertex_of = Vec::with_capacity(polygon_vertices.len());
    let mut polygon_of = Vec::with_capacity(polygon_vertices.len());
    let mut polygons: Vec<std::ops::Range<usize>> = Vec::new();
    let mut start = 0;
    for (corner, &raw) in polygon_vertices.iter().enumerate() {
        // A negative index ends a polygon: it is the bitwise NOT of the last vertex.
        let vertex = if raw < 0 { !raw } else { raw };
        vertex_of.push(usize::try_from(vertex).ok()?);
        polygon_of.push(polygons.len());
        if raw < 0 {
            polygons.push(start..corner + 1);
            start = corner + 1;
        }
    }
    let corners = polygon_vertices.len();
    let normals = geometry.child("LayerElementNormal").and_then(|layer| {
        layer_values(
            layer,
            "Normals",
            "NormalsIndex",
            3,
            corners,
            &vertex_of,
            &polygon_of,
        )
    });
    let uvs = geometry.child("LayerElementUV").and_then(|layer| {
        layer_values(layer, "UV", "UVIndex", 2, corners, &vertex_of, &polygon_of)
    });
    let material_slots: Option<Vec<i64>> =
        geometry.child("LayerElementMaterial").and_then(|layer| {
            let Some(Value::Ints(slots)) = layer.first("Materials") else {
                return None;
            };
            let all_same =
                layer.first("MappingInformationType").and_then(Value::text) == Some("AllSame");
            Some(if all_same {
                vec![slots.first().copied().unwrap_or(0); polygons.len()]
            } else {
                slots.clone()
            })
        });
    let mut by_material: Vec<(Option<usize>, Primitive)> = Vec::new();
    for (polygon, range) in polygons.iter().enumerate() {
        let material = material_slots
            .as_ref()
            .and_then(|slots| slots.get(polygon))
            .and_then(|&slot| model_materials.get(usize::try_from(slot).ok()?).copied())
            .or_else(|| model_materials.first().copied());
        let at = if let Some(at) = by_material.iter().position(|(known, _)| *known == material) {
            at
        } else {
            by_material.push((
                material,
                Primitive {
                    material,
                    ..Primitive::default()
                },
            ));
            by_material.len() - 1
        };
        let primitive = &mut by_material[at].1;
        let base = u32::try_from(primitive.positions.len()).ok()?;
        for corner in range.clone() {
            let vertex = vertex_of[corner];
            let point = points.get(vertex * 3..vertex * 3 + 3)?;
            primitive
                .positions
                .push(Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32));
            if let Some(normal) = normals.as_ref().and_then(|normals| normals.get(corner)) {
                primitive.normals.push(Vec3::new(
                    normal[0] as f32,
                    normal[1] as f32,
                    normal[2] as f32,
                ));
            }
            if let Some(uv) = uvs.as_ref().and_then(|uvs| uvs.get(corner)) {
                primitive
                    .uvs
                    .push(Vec2::new(uv[0] as f32, 1.0 - uv[1] as f32));
            }
        }
        let count = u32::try_from(range.len()).ok()?;
        for offset in 1..count.saturating_sub(1) {
            primitive
                .triangles
                .push([base, base + offset, base + offset + 1]);
        }
    }
    Some(
        by_material
            .into_iter()
            .map(|(_, mut primitive)| {
                if primitive.normals.len() != primitive.positions.len() {
                    primitive.normals.clear();
                }
                if primitive.uvs.len() != primitive.positions.len() {
                    primitive.uvs.clear();
                }
                primitive
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a 7.4 FBX record.
    fn record(name: &str, values: &[Vec<u8>], children: &[Vec<u8>], at: usize) -> Vec<u8> {
        let properties: Vec<u8> = values.concat();
        let mut nested: Vec<u8> = Vec::new();
        // Children were built for their own absolute offsets.
        for child in children {
            nested.extend_from_slice(child);
        }
        if !children.is_empty() {
            nested.extend_from_slice(&[0; 13]);
        }
        let end = at + 13 + name.len() + properties.len() + nested.len();
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(end).expect("small").to_le_bytes());
        out.extend_from_slice(&u32::try_from(values.len()).expect("small").to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(properties.len())
                .expect("small")
                .to_le_bytes(),
        );
        out.push(u8::try_from(name.len()).expect("short"));
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&properties);
        out.extend_from_slice(&nested);
        out
    }

    fn long(value: i64) -> Vec<u8> {
        let mut out = vec![b'L'];
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn text(value: &str) -> Vec<u8> {
        let mut out = vec![b'S'];
        out.extend_from_slice(&u32::try_from(value.len()).expect("short").to_le_bytes());
        out.extend_from_slice(value.as_bytes());
        out
    }

    fn doubles(values: &[f64]) -> Vec<u8> {
        let mut out = vec![b'd'];
        out.extend_from_slice(&u32::try_from(values.len()).expect("few").to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(values.len() * 8).expect("few").to_le_bytes());
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    fn ints(values: &[i32]) -> Vec<u8> {
        let mut out = vec![b'i'];
        out.extend_from_slice(&u32::try_from(values.len()).expect("few").to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(values.len() * 4).expect("few").to_le_bytes());
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    /// Builds a file of top-level records, laying out offsets in order.
    fn file(build: impl Fn(usize) -> Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&[0x1A, 0x00]);
        out.extend_from_slice(&7400u32.to_le_bytes());
        for record in build(out.len()) {
            out.extend_from_slice(&record);
        }
        out.extend_from_slice(&[0; 13]);
        out
    }

    #[test]
    fn reads_a_quad_with_a_model() {
        // Records are built from the innermost out, each knowing its offset.
        let data = file(|start| {
            let objects_header = 13 + "Objects".len();
            let geometry_at = start + objects_header;
            let geometry_values = [long(10), text("Quad\0\u{1}Geometry"), text("Mesh")];
            let geometry_header = geometry_at
                + 13
                + "Geometry".len()
                + geometry_values.iter().map(Vec::len).sum::<usize>();
            let vertices_values = [doubles(&[
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
            ])];
            let vertices = record("Vertices", &vertices_values, &[], geometry_header);
            let index_values = [ints(&[0, 1, 2, !3])];
            let indices = record(
                "PolygonVertexIndex",
                &index_values,
                &[],
                geometry_header + vertices.len(),
            );
            let geometry = record(
                "Geometry",
                &geometry_values,
                &[vertices, indices],
                geometry_at,
            );
            let model_at = geometry_at + geometry.len();
            let model = record(
                "Model",
                &[long(20), text("Box\0\u{1}Model"), text("Mesh")],
                &[],
                model_at,
            );
            let objects = record("Objects", &[], &[geometry, model], start);
            let connections_at = start + objects.len();
            let link_values = [text("OO"), long(10), long(20)];
            let link = record(
                "C",
                &link_values,
                &[],
                connections_at + 13 + "Connections".len(),
            );
            let connections = record("Connections", &[], &[link], connections_at);
            vec![objects, connections]
        });
        let scene = load(&data, None).expect("loads");
        assert_eq!(scene.stats().triangles, 2);
        assert_eq!(scene.nodes[0].name, "Box");
        assert_eq!(scene.nodes[0].mesh, Some(0));
        assert!(load(b"Kaydara FBX Binary  \0\x1a\0", None).is_err());
        assert!(!is_fbx(b"; FBX 7.4.0 project file"));
    }
}
