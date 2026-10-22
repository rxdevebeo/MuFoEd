//! The plain mesh formats: Wavefront OBJ (with MTL materials and textures),
//! STL (binary and ASCII), PLY (ASCII and binary) and OFF.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use glam::{Vec2, Vec3, Vec4};

use super::scene::{smooth_normals, Material, Mesh, Node, Primitive, Scene, Texture, UpAxis};
use crate::Error;

/// A scene of one node per mesh, all at the origin.
fn scene_of(
    meshes: Vec<Mesh>,
    materials: Vec<Material>,
    textures: Vec<Texture>,
    up: UpAxis,
) -> Scene {
    let mut scene = Scene {
        nodes: (0..meshes.len())
            .map(|index| Node {
                name: meshes[index].name.clone(),
                mesh: Some(index),
                ..Node::default()
            })
            .collect(),
        meshes,
        materials,
        textures,
        up,
        ..Scene::default()
    };
    scene.finish();
    scene
}

/// A texture file as RGBA8, if it can be read.
pub(crate) fn texture_file(path: &Path) -> Option<Texture> {
    let decoded = crate::decode::load(path, &crate::decode::LoadOptions::default()).ok()?;
    let rgba = decoded.image.to_rgba8();
    Some(Texture {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// Reads an OBJ file with its MTL libraries and textures.
pub(crate) fn load_obj(path: &Path) -> Result<Scene, Error> {
    let options = tobj::LoadOptions {
        single_index: true,
        triangulate: true,
        ignore_points: true,
        ignore_lines: false,
    };
    let (models, materials) =
        tobj::load_obj(path, &options).map_err(|error| Error::Codec(format!("OBJ: {error}")))?;
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    let mut textures = Vec::new();
    let mut by_file: HashMap<String, usize> = HashMap::new();
    let materials: Vec<Material> = materials
        .unwrap_or_default()
        .into_iter()
        .map(|material| {
            let texture = material.diffuse_texture.as_ref().and_then(|file| {
                if let Some(&index) = by_file.get(file) {
                    return Some(index);
                }
                let texture = texture_file(&folder.join(file.replace('\\', "/")))?;
                textures.push(texture);
                by_file.insert(file.clone(), textures.len() - 1);
                Some(textures.len() - 1)
            });
            let [r, g, b] = material.diffuse.unwrap_or([0.8, 0.8, 0.8]);
            Material {
                name: material.name,
                base_color: Vec4::new(r, g, b, material.dissolve.unwrap_or(1.0)),
                texture,
                double_sided: true,
                emissive: material.emissive.map_or(Vec3::ZERO, Vec3::from_array),
            }
        })
        .collect();
    let meshes = models
        .into_iter()
        .map(|model| {
            let mesh = model.mesh;
            let mut primitive = Primitive {
                positions: mesh
                    .positions
                    .chunks_exact(3)
                    .map(Vec3::from_slice)
                    .collect(),
                normals: mesh.normals.chunks_exact(3).map(Vec3::from_slice).collect(),
                // OBJ's V axis points up; images are stored top row first.
                uvs: mesh
                    .texcoords
                    .chunks_exact(2)
                    .map(|uv| Vec2::new(uv[0], 1.0 - uv[1]))
                    .collect(),
                colors: mesh
                    .vertex_color
                    .chunks_exact(3)
                    .map(|c| Vec4::new(c[0], c[1], c[2], 1.0))
                    .collect(),
                triangles: mesh
                    .indices
                    .chunks_exact(3)
                    .map(|t| [t[0], t[1], t[2]])
                    .collect(),
                material: mesh.material_id.filter(|&id| id < materials.len()),
                ..Primitive::default()
            };
            smooth_normals(&mut primitive);
            Mesh {
                name: model.name,
                primitives: vec![primitive],
                weights: Vec::new(),
            }
        })
        .collect();
    Ok(scene_of(meshes, materials, textures, UpAxis::Y))
}

/// Reads an STL file (binary or ASCII). Faces keep their own normals: STL
/// is mostly CAD output, where edges are meant to be sharp.
pub(crate) fn load_stl(data: &[u8]) -> Result<Scene, Error> {
    let mesh = stl_io::read_stl(&mut Cursor::new(data))
        .map_err(|error| Error::Codec(format!("STL: {error}")))?;
    let mut primitive = Primitive::default();
    for face in &mesh.faces {
        let base = u32::try_from(primitive.positions.len())
            .map_err(|_| Error::Codec("STL: too many faces".into()))?;
        for &vertex in &face.vertices {
            let point = mesh
                .vertices
                .get(vertex)
                .ok_or_else(|| Error::Codec("STL: vertex index out of range".into()))?;
            primitive.positions.push(Vec3::from_array(point.0));
        }
        primitive.triangles.push([base, base + 1, base + 2]);
    }
    let meshes = vec![Mesh {
        name: "STL".into(),
        primitives: vec![primitive],
        weights: Vec::new(),
    }];
    Ok(scene_of(meshes, Vec::new(), Vec::new(), UpAxis::Z))
}

/// The scalar types of PLY properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// Whether colours of this type run 0..=255 (as opposed to 0..=1).
    fn is_integer(self) -> bool {
        !matches!(self, Self::F32 | Self::F64)
    }
}

#[derive(Clone, Debug)]
struct Property {
    name: String,
    kind: Scalar,
    /// The count type of a list property.
    list: Option<Scalar>,
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

/// Reads the values of PLY data, ASCII or binary.
struct Values<'a> {
    encoding: Encoding,
    data: &'a [u8],
    at: usize,
    words: std::str::SplitAsciiWhitespace<'a>,
}

impl Values<'_> {
    fn next(&mut self, kind: Scalar) -> Result<f64, Error> {
        let short = || Error::Codec("PLY: the data ends early".into());
        if self.encoding == Encoding::Ascii {
            let word = self.words.next().ok_or_else(short)?;
            return word
                .parse::<f64>()
                .map_err(|_| Error::Codec(format!("PLY: not a number: {word}")));
        }
        let size = kind.size();
        let bytes = self.data.get(self.at..self.at + size).ok_or_else(short)?;
        self.at += size;
        let mut buffer = [0u8; 8];
        buffer[..size].copy_from_slice(bytes);
        if self.encoding == Encoding::Big {
            buffer[..size].reverse();
        }
        Ok(match kind {
            Scalar::I8 => f64::from(i8::from_le_bytes([buffer[0]])),
            Scalar::U8 => f64::from(buffer[0]),
            Scalar::I16 => f64::from(i16::from_le_bytes([buffer[0], buffer[1]])),
            Scalar::U16 => f64::from(u16::from_le_bytes([buffer[0], buffer[1]])),
            Scalar::I32 => f64::from(i32::from_le_bytes([
                buffer[0], buffer[1], buffer[2], buffer[3],
            ])),
            Scalar::U32 => f64::from(u32::from_le_bytes([
                buffer[0], buffer[1], buffer[2], buffer[3],
            ])),
            Scalar::F32 => f64::from(f32::from_le_bytes([
                buffer[0], buffer[1], buffer[2], buffer[3],
            ])),
            Scalar::F64 => f64::from_le_bytes(buffer),
        })
    }
}

/// Most vertices or faces a PLY or OFF header may announce.
const MAX_ITEMS: usize = 50_000_000;

/// Reads a PLY file: vertex positions, normals, colours and texture
/// coordinates; faces (fan-triangulated) and edges.
// Coordinates and indices are converted from the file's f64 reading; the
// property names keep PLY's one-letter spelling.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::similar_names
)]
pub(crate) fn load_ply(data: &[u8]) -> Result<Scene, Error> {
    let header_end = data
        .windows(10)
        .position(|window| window == b"end_header")
        .ok_or_else(|| Error::Codec("PLY: no header".into()))?;
    let header = String::from_utf8_lossy(&data[..header_end]);
    let mut body = header_end + 10;
    while data
        .get(body)
        .is_some_and(|&byte| byte == b'\r' || byte == b' ')
    {
        body += 1;
    }
    if data.get(body) == Some(&b'\n') {
        body += 1;
    }
    let mut encoding = Encoding::Ascii;
    let mut elements: Vec<Element> = Vec::new();
    for line in header.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", "ascii", ..] => encoding = Encoding::Ascii,
            ["format", "binary_little_endian", ..] => encoding = Encoding::Little,
            ["format", "binary_big_endian", ..] => encoding = Encoding::Big,
            ["element", name, count] => elements.push(Element {
                name: (*name).to_owned(),
                count: count
                    .parse::<usize>()
                    .ok()
                    .filter(|&count| count <= MAX_ITEMS)
                    .ok_or_else(|| Error::Codec("PLY: bad element count".into()))?,
                properties: Vec::new(),
            }),
            ["property", "list", count, item, name] => {
                if let Some(element) = elements.last_mut() {
                    element.properties.push(Property {
                        name: (*name).to_owned(),
                        kind: Scalar::parse(item)
                            .ok_or_else(|| Error::Codec("PLY: bad type".into()))?,
                        list: Some(
                            Scalar::parse(count)
                                .ok_or_else(|| Error::Codec("PLY: bad type".into()))?,
                        ),
                    });
                }
            }
            ["property", kind, name] => {
                if let Some(element) = elements.last_mut() {
                    element.properties.push(Property {
                        name: (*name).to_owned(),
                        kind: Scalar::parse(kind)
                            .ok_or_else(|| Error::Codec("PLY: bad type".into()))?,
                        list: None,
                    });
                }
            }
            _ => {}
        }
    }
    let text = if encoding == Encoding::Ascii {
        std::str::from_utf8(&data[body..])
            .map_err(|_| Error::Codec("PLY: the text is not UTF-8".into()))?
    } else {
        ""
    };
    let mut values = Values {
        encoding,
        data,
        at: body,
        words: text.split_ascii_whitespace(),
    };
    let mut primitive = Primitive::default();
    for element in &elements {
        let position = |name: &str| {
            element
                .properties
                .iter()
                .position(|property| property.name == name)
        };
        let [x, y, z] = ["x", "y", "z"].map(position);
        let [nx, ny, nz] = ["nx", "ny", "nz"].map(position);
        let [red, green, blue, alpha] = ["red", "green", "blue", "alpha"].map(position);
        let [u, v] = [
            position("u")
                .or_else(|| position("s"))
                .or_else(|| position("texture_u")),
            position("v")
                .or_else(|| position("t"))
                .or_else(|| position("texture_v")),
        ];
        let integer_colours = red.is_some_and(|index| element.properties[index].kind.is_integer());
        let mut scalars = vec![0.0f64; element.properties.len()];
        for _ in 0..element.count {
            let mut list: Vec<u32> = Vec::new();
            for (index, property) in element.properties.iter().enumerate() {
                if let Some(count_kind) = property.list {
                    let count = values.next(count_kind)? as usize;
                    if count > 1024 {
                        return Err(Error::Codec("PLY: a face with too many corners".into()));
                    }
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        items.push(values.next(property.kind)? as u32);
                    }
                    if matches!(property.name.as_str(), "vertex_indices" | "vertex_index") {
                        list = items;
                    }
                } else {
                    scalars[index] = values.next(property.kind)?;
                }
            }
            let get = |index: Option<usize>| index.map(|index| scalars[index] as f32);
            match element.name.as_str() {
                "vertex" => {
                    if let (Some(x), Some(y), Some(z)) = (get(x), get(y), get(z)) {
                        primitive.positions.push(Vec3::new(x, y, z));
                    }
                    if let (Some(x), Some(y), Some(z)) = (get(nx), get(ny), get(nz)) {
                        primitive.normals.push(Vec3::new(x, y, z));
                    }
                    if let (Some(r), Some(g), Some(b)) = (get(red), get(green), get(blue)) {
                        let scale = if integer_colours { 255.0 } else { 1.0 };
                        let a = get(alpha).unwrap_or(scale);
                        primitive.colors.push(Vec4::new(r, g, b, a) / scale);
                    }
                    if let (Some(u), Some(v)) = (get(u), get(v)) {
                        primitive.uvs.push(Vec2::new(u, 1.0 - v));
                    }
                }
                "face" => {
                    if let Some((&first, rest)) = list.split_first() {
                        for pair in rest.windows(2) {
                            primitive.triangles.push([first, pair[0], pair[1]]);
                        }
                    }
                }
                "edge" => {
                    let (vertex1, vertex2) = (
                        element.properties.iter().position(|p| p.name == "vertex1"),
                        element.properties.iter().position(|p| p.name == "vertex2"),
                    );
                    if let (Some(a), Some(b)) = (get(vertex1), get(vertex2)) {
                        primitive.lines.push([a as u32, b as u32]);
                    }
                }
                _ => {}
            }
        }
    }
    if primitive.normals.len() != primitive.positions.len() {
        primitive.normals.clear();
        if !primitive.triangles.is_empty() {
            smooth_normals(&mut primitive);
        }
    }
    let meshes = vec![Mesh {
        name: "PLY".into(),
        primitives: vec![primitive],
        weights: Vec::new(),
    }];
    Ok(scene_of(meshes, Vec::new(), Vec::new(), UpAxis::Y))
}

/// Reads an OFF (Object File Format) file, including COFF colours.
// Indices are converted from the file's numbers.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn load_off(data: &[u8]) -> Result<Scene, Error> {
    let text =
        std::str::from_utf8(data).map_err(|_| Error::Codec("OFF: the text is not UTF-8".into()))?;
    let mut words = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(str::split_whitespace);
    let magic = words.next().unwrap_or_default();
    if !magic.ends_with("OFF") {
        return Err(Error::Codec("OFF: no header".into()));
    }
    let coloured = magic.starts_with('C');
    let mut number = || -> Result<f64, Error> {
        let word = words
            .next()
            .ok_or_else(|| Error::Codec("OFF: the data ends early".into()))?;
        word.parse::<f64>()
            .map_err(|_| Error::Codec(format!("OFF: not a number: {word}")))
    };
    let vertices = number()? as usize;
    let faces = number()? as usize;
    let _edges = number()?;
    if vertices > MAX_ITEMS || faces > MAX_ITEMS {
        return Err(Error::Codec("OFF: too many items".into()));
    }
    let mut primitive = Primitive::default();
    for _ in 0..vertices {
        let point = Vec3::new(number()? as f32, number()? as f32, number()? as f32);
        primitive.positions.push(point);
        if coloured {
            let mut colour = [0.0f32; 4];
            for channel in &mut colour {
                *channel = number()? as f32;
            }
            let scale = if colour.iter().any(|&value| value > 1.0) {
                255.0
            } else {
                1.0
            };
            primitive.colors.push(Vec4::from_array(colour) / scale);
        }
    }
    for _ in 0..faces {
        let corners = number()? as usize;
        if corners > 1024 {
            return Err(Error::Codec("OFF: a face with too many corners".into()));
        }
        let mut indices = Vec::with_capacity(corners);
        for _ in 0..corners {
            indices.push(number()? as u32);
        }
        if let Some((&first, rest)) = indices.split_first() {
            for pair in rest.windows(2) {
                primitive.triangles.push([first, pair[0], pair[1]]);
            }
        }
    }
    smooth_normals(&mut primitive);
    let meshes = vec![Mesh {
        name: "OFF".into(),
        primitives: vec![primitive],
        weights: Vec::new(),
    }];
    Ok(scene_of(meshes, Vec::new(), Vec::new(), UpAxis::Y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ascii_and_binary_stl() {
        let ascii = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        let scene = load_stl(ascii).expect("ascii");
        assert_eq!(scene.stats().triangles, 1);
        assert_eq!(scene.up, UpAxis::Z);
        let mut binary = vec![0u8; 80];
        binary.extend_from_slice(&1u32.to_le_bytes());
        for value in [
            0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0,
        ] {
            binary.extend_from_slice(&value.to_le_bytes());
        }
        binary.extend_from_slice(&[0, 0]);
        let scene = load_stl(&binary).expect("binary");
        assert_eq!(scene.stats().vertices, 3);
    }

    #[test]
    fn reads_ply_in_both_encodings() {
        let ascii = b"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 255 0 0\n1 0 0 0 255 0\n1 1 0 0 0 255\n0 1 0 255 255 255\n4 0 1 2 3\n";
        let scene = load_ply(ascii).expect("ascii");
        assert_eq!(scene.stats().triangles, 2);
        assert_eq!(
            scene.meshes[0].primitives[0].colors[0],
            Vec4::new(1.0, 0.0, 0.0, 1.0)
        );
        let mut binary = b"ply\nformat binary_big_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar uint vertex_indices\nend_header\n".to_vec();
        for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            binary.extend_from_slice(&value.to_be_bytes());
        }
        binary.push(3);
        for index in [0u32, 1, 2] {
            binary.extend_from_slice(&index.to_be_bytes());
        }
        let scene = load_ply(&binary).expect("binary");
        assert_eq!(scene.stats().triangles, 1);
        assert_eq!(scene.meshes[0].primitives[0].positions[1], Vec3::X);
        assert!(load_ply(&binary[..binary.len() - 3]).is_err());
    }

    #[test]
    fn reads_off_and_coloured_off() {
        let off = b"OFF\n# a square\n4 1 0\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
        assert_eq!(load_off(off).expect("off").stats().triangles, 2);
        let coff =
            b"COFF\n3 1 0\n0 0 0 255 0 0 255\n1 0 0 255 0 0 255\n0 1 0 255 0 0 255\n3 0 1 2\n";
        let scene = load_off(coff).expect("coff");
        assert_eq!(
            scene.meshes[0].primitives[0].colors[0],
            Vec4::new(1.0, 0.0, 0.0, 1.0)
        );
        assert!(load_off(b"OFF\n3 1").is_err());
    }

    #[test]
    fn reads_obj_with_materials() {
        let folder = std::env::temp_dir().join(format!("image-gallery-obj-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("m.mtl"), "newmtl red\nKd 1 0 0\n").expect("mtl");
        std::fs::write(
            folder.join("q.obj"),
            "mtllib m.mtl\no quad\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nusemtl red\nf 1 2 3 4\n",
        )
        .expect("obj");
        let scene = load_obj(&folder.join("q.obj")).expect("loads");
        assert_eq!(scene.stats().triangles, 2);
        assert_eq!(scene.materials[0].base_color, Vec4::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(scene.meshes[0].primitives[0].material, Some(0));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
