//! The XML-based formats: 3MF (a ZIP package of XML, the 3D printing
//! format) and COLLADA (`.dae`): geometry, materials, textures and the node
//! hierarchy. COLLADA skins and animations are not read.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

use super::scene::{smooth_normals, Material, Mesh, Node, Primitive, Scene, Transform, UpAxis};
use crate::Error;

/// The largest XML part read.
const MAX_XML: u64 = 512 << 20;

fn numbers(text: &str) -> impl Iterator<Item = f32> + '_ {
    text.split_ascii_whitespace()
        .filter_map(|word| word.parse().ok())
}

/// `#RRGGBB` or `#RRGGBBAA` as linear-ish RGBA 0..=1.
fn hex_colour(text: &str) -> Option<Vec4> {
    let hex = text.trim().strip_prefix('#')?;
    let channel = |at: usize| {
        u8::from_str_radix(hex.get(at..at + 2)?, 16)
            .ok()
            .map(|value| f32::from(value) / 255.0)
    };
    let alpha = if hex.len() >= 8 { channel(6)? } else { 1.0 };
    Some(Vec4::new(channel(0)?, channel(2)?, channel(4)?, alpha))
}

fn xml_error(format: &str, error: &roxmltree::Error) -> Error {
    Error::Codec(format!("{format}: {error}"))
}

// ----- 3MF -------------------------------------------------------------------

/// Reads a 3MF package.
#[allow(clippy::too_many_lines)]
pub(crate) fn load_3mf(data: &[u8]) -> Result<Scene, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|error| Error::Codec(format!("3MF: {error}")))?;
    let model_path = model_part(&mut archive).unwrap_or_else(|| "3D/3dmodel.model".to_owned());
    let text = read_part(&mut archive, &model_path)?;
    let document = roxmltree::Document::parse(&text).map_err(|error| xml_error("3MF", &error))?;
    let root = document.root_element();
    let resources = root
        .children()
        .find(|node| node.has_tag_name_local("resources"))
        .ok_or_else(|| Error::Codec("3MF: no resources".into()))?;

    let mut scene = Scene {
        up: UpAxis::Z,
        ..Scene::default()
    };
    // Base material groups: id -> material indices in the scene.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for group in resources
        .children()
        .filter(|node| node.has_tag_name_local("basematerials"))
    {
        let Some(id) = group.attribute("id") else {
            continue;
        };
        let mut indices = Vec::new();
        for base in group
            .children()
            .filter(|node| node.has_tag_name_local("base"))
        {
            scene.materials.push(Material {
                name: base.attribute("name").unwrap_or_default().to_owned(),
                base_color: base
                    .attribute("displaycolor")
                    .and_then(hex_colour)
                    .unwrap_or(Vec4::new(0.8, 0.8, 0.8, 1.0)),
                ..Material::default()
            });
            indices.push(scene.materials.len() - 1);
        }
        groups.insert(id.to_owned(), indices);
    }
    // Objects: id -> mesh index and components.
    let mut meshes: HashMap<String, usize> = HashMap::new();
    let mut components: HashMap<String, Vec<(String, Mat4)>> = HashMap::new();
    for object in resources
        .children()
        .filter(|node| node.has_tag_name_local("object"))
    {
        let Some(id) = object.attribute("id") else {
            continue;
        };
        let default_material = object
            .attribute("pid")
            .and_then(|pid| groups.get(pid))
            .and_then(|group| {
                group.get(
                    object
                        .attribute("pindex")
                        .and_then(|i| i.parse::<usize>().ok())
                        .unwrap_or(0),
                )
            })
            .copied();
        if let Some(mesh) = object
            .children()
            .find(|node| node.has_tag_name_local("mesh"))
        {
            let mesh = read_3mf_mesh(mesh, default_material, &groups);
            scene.meshes.push(Mesh {
                name: object.attribute("name").unwrap_or(id).to_owned(),
                primitives: mesh,
                weights: Vec::new(),
            });
            meshes.insert(id.to_owned(), scene.meshes.len() - 1);
        }
        if let Some(list) = object
            .children()
            .find(|node| node.has_tag_name_local("components"))
        {
            let parts = list
                .children()
                .filter(|node| node.has_tag_name_local("component"))
                .filter_map(|component| {
                    Some((
                        component.attribute("objectid")?.to_owned(),
                        component
                            .attribute("transform")
                            .map_or(Mat4::IDENTITY, matrix_3mf),
                    ))
                })
                .collect();
            components.insert(id.to_owned(), parts);
        }
    }
    let build = root
        .children()
        .find(|node| node.has_tag_name_local("build"))
        .ok_or_else(|| Error::Codec("3MF: no build".into()))?;
    for item in build
        .children()
        .filter(|node| node.has_tag_name_local("item"))
    {
        let Some(object) = item.attribute("objectid") else {
            continue;
        };
        let transform = item
            .attribute("transform")
            .map_or(Mat4::IDENTITY, matrix_3mf);
        let node = instance_3mf(&mut scene, object, transform, &meshes, &components, 0);
        if let Some(node) = node {
            scene.roots.push(node);
        }
    }
    scene.finish();
    Ok(scene)
}

/// Adds a node for an object (and its components) and returns its index.
fn instance_3mf(
    scene: &mut Scene,
    object: &str,
    transform: Mat4,
    meshes: &HashMap<String, usize>,
    components: &HashMap<String, Vec<(String, Mat4)>>,
    depth: usize,
) -> Option<usize> {
    if depth > 16 {
        return None;
    }
    let index = scene.nodes.len();
    scene.nodes.push(Node {
        name: object.to_owned(),
        transform: Transform::from_matrix(transform),
        mesh: meshes.get(object).copied(),
        ..Node::default()
    });
    if let Some(parts) = components.get(object) {
        for (child, matrix) in parts {
            if let Some(child) = instance_3mf(scene, child, *matrix, meshes, components, depth + 1)
            {
                scene.nodes[index].children.push(child);
            }
        }
    }
    Some(index)
}

/// A 3MF transform: twelve numbers of a row-vector affine matrix.
fn matrix_3mf(text: &str) -> Mat4 {
    let m: Vec<f32> = numbers(text).collect();
    if m.len() != 12 {
        return Mat4::IDENTITY;
    }
    Mat4::from_cols(
        Vec4::new(m[0], m[1], m[2], 0.0),
        Vec4::new(m[3], m[4], m[5], 0.0),
        Vec4::new(m[6], m[7], m[8], 0.0),
        Vec4::new(m[9], m[10], m[11], 1.0),
    )
}

/// The triangles of a 3MF mesh, one primitive per material.
fn read_3mf_mesh(
    mesh: roxmltree::Node<'_, '_>,
    default_material: Option<usize>,
    groups: &HashMap<String, Vec<usize>>,
) -> Vec<Primitive> {
    let positions: Vec<Vec3> = mesh
        .children()
        .find(|node| node.has_tag_name_local("vertices"))
        .map(|vertices| {
            vertices
                .children()
                .filter(|node| node.has_tag_name_local("vertex"))
                .map(|vertex| {
                    let coordinate = |name| {
                        vertex
                            .attribute(name)
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(0.0)
                    };
                    Vec3::new(coordinate("x"), coordinate("y"), coordinate("z"))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut by_material: Vec<(Option<usize>, Vec<[u32; 3]>)> = Vec::new();
    if let Some(triangles) = mesh
        .children()
        .find(|node| node.has_tag_name_local("triangles"))
    {
        for triangle in triangles
            .children()
            .filter(|node| node.has_tag_name_local("triangle"))
        {
            let index = |name| {
                triangle
                    .attribute(name)
                    .and_then(|value| value.parse::<u32>().ok())
            };
            let (Some(a), Some(b), Some(c)) = (index("v1"), index("v2"), index("v3")) else {
                continue;
            };
            let material = triangle
                .attribute("pid")
                .and_then(|pid| groups.get(pid))
                .and_then(|group| group.get(index("p1").unwrap_or(0) as usize))
                .copied()
                .or(default_material);
            match by_material.iter_mut().find(|(known, _)| *known == material) {
                Some((_, list)) => list.push([a, b, c]),
                None => by_material.push((material, vec![[a, b, c]])),
            }
        }
    }
    by_material
        .into_iter()
        .map(|(material, triangles)| Primitive {
            positions: positions.clone(),
            triangles,
            material,
            ..Primitive::default()
        })
        .collect()
}

/// The model part named by the package relationships.
fn model_part(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Option<String> {
    let rels = read_part(archive, "_rels/.rels").ok()?;
    let document = roxmltree::Document::parse(&rels).ok()?;
    document
        .descendants()
        .filter(|node| node.has_tag_name_local("Relationship"))
        .find(|node| {
            node.attribute("Type")
                .is_some_and(|kind| kind.ends_with("/3dmodel"))
        })
        .and_then(|node| node.attribute("Target"))
        .map(|target| target.trim_start_matches('/').to_owned())
}

fn read_part(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, Error> {
    let file = archive
        .by_name(name)
        .map_err(|error| Error::Codec(format!("3MF: {name}: {error}")))?;
    let mut text = String::new();
    file.take(MAX_XML)
        .read_to_string(&mut text)
        .map_err(|error| Error::Codec(format!("3MF: {name}: {error}")))?;
    Ok(text)
}

/// Tag-name matching that ignores the namespace.
trait LocalName {
    fn has_tag_name_local(&self, name: &str) -> bool;
}

impl LocalName for roxmltree::Node<'_, '_> {
    fn has_tag_name_local(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name() == name
    }
}

// ----- COLLADA ---------------------------------------------------------------

/// A COLLADA document's libraries, by ID.
struct Collada<'a, 'input> {
    ids: HashMap<&'a str, roxmltree::Node<'a, 'input>>,
}

impl<'a, 'input> Collada<'a, 'input> {
    fn get(&self, url: &str) -> Option<roxmltree::Node<'a, 'input>> {
        self.ids.get(url.trim_start_matches('#')).copied()
    }
}

/// Reads a COLLADA document; textures are looked for beside `folder`.
pub(crate) fn load_dae(data: &[u8], folder: Option<&Path>) -> Result<Scene, Error> {
    let text = String::from_utf8_lossy(data);
    let document =
        roxmltree::Document::parse(&text).map_err(|error| xml_error("COLLADA", &error))?;
    let root = document.root_element();
    if !root.has_tag_name_local("COLLADA") {
        return Err(Error::Codec("COLLADA: not a COLLADA document".into()));
    }
    let collada = Collada {
        ids: document
            .descendants()
            .filter_map(|node| Some((node.attribute("id")?, node)))
            .collect(),
    };
    let up = match root
        .descendants()
        .find(|node| node.has_tag_name_local("up_axis"))
        .and_then(|node| node.text())
        .map(str::trim)
    {
        Some("Z_UP") => UpAxis::Z,
        _ => UpAxis::Y,
    };
    let mut scene = Scene {
        up,
        ..Scene::default()
    };
    let mut materials: HashMap<String, usize> = HashMap::new();
    let mut geometries: HashMap<String, usize> = HashMap::new();
    let mut images: HashMap<String, Option<usize>> = HashMap::new();
    let visual = root
        .descendants()
        .find(|node| node.has_tag_name_local("instance_visual_scene"))
        .and_then(|node| collada.get(node.attribute("url")?))
        .or_else(|| {
            root.descendants()
                .find(|node| node.has_tag_name_local("visual_scene"))
        })
        .ok_or_else(|| Error::Codec("COLLADA: no visual scene".into()))?;
    let mut context = DaeContext {
        collada: &collada,
        scene: &mut scene,
        materials: &mut materials,
        geometries: &mut geometries,
        images: &mut images,
        folder,
    };
    for node in visual
        .children()
        .filter(|node| node.has_tag_name_local("node"))
    {
        if let Some(index) = context.node(node, 0) {
            context.scene.roots.push(index);
        }
    }
    scene.finish();
    Ok(scene)
}

struct DaeContext<'c, 'a, 'input> {
    collada: &'c Collada<'a, 'input>,
    scene: &'c mut Scene,
    /// Material ID -> scene material.
    materials: &'c mut HashMap<String, usize>,
    /// Geometry ID and material bindings -> scene mesh.
    geometries: &'c mut HashMap<String, usize>,
    /// Image ID -> scene texture.
    images: &'c mut HashMap<String, Option<usize>>,
    folder: Option<&'c Path>,
}

impl DaeContext<'_, '_, '_> {
    fn node(&mut self, element: roxmltree::Node<'_, '_>, depth: usize) -> Option<usize> {
        if depth > 64 {
            return None;
        }
        let mut matrix = Mat4::IDENTITY;
        for child in element.children().filter(roxmltree::Node::is_element) {
            let values: Vec<f32> = child
                .text()
                .map(|text| numbers(text).collect())
                .unwrap_or_default();
            let step = match (child.tag_name().name(), values.as_slice()) {
                ("matrix", values) if values.len() == 16 => {
                    // COLLADA matrices are written row by row.
                    Mat4::from_cols_slice(values).transpose()
                }
                ("translate", [x, y, z]) => Mat4::from_translation(Vec3::new(*x, *y, *z)),
                ("scale", [x, y, z]) => Mat4::from_scale(Vec3::new(*x, *y, *z)),
                ("rotate", [x, y, z, angle]) => Mat4::from_quat(Quat::from_axis_angle(
                    Vec3::new(*x, *y, *z).normalize_or_zero(),
                    angle.to_radians(),
                )),
                _ => continue,
            };
            matrix *= step;
        }
        let index = self.scene.nodes.len();
        self.scene.nodes.push(Node {
            name: element
                .attribute("name")
                .or_else(|| element.attribute("id"))
                .unwrap_or_default()
                .to_owned(),
            transform: Transform::from_matrix(matrix),
            ..Node::default()
        });
        for instance in element
            .children()
            .filter(|node| node.has_tag_name_local("instance_geometry"))
        {
            let Some(mesh) = self.geometry(instance) else {
                continue;
            };
            if self.scene.nodes[index].mesh.is_none() {
                self.scene.nodes[index].mesh = Some(mesh);
            } else {
                // A second geometry on one node gets a child node of its own.
                let child = self.scene.nodes.len();
                self.scene.nodes.push(Node {
                    mesh: Some(mesh),
                    ..Node::default()
                });
                self.scene.nodes[index].children.push(child);
            }
        }
        for child in element
            .children()
            .filter(|node| node.has_tag_name_local("node"))
        {
            if let Some(child) = self.node(child, depth + 1) {
                self.scene.nodes[index].children.push(child);
            }
        }
        Some(index)
    }

    /// The mesh of an `instance_geometry`, with its material bindings.
    fn geometry(&mut self, instance: roxmltree::Node<'_, '_>) -> Option<usize> {
        let url = instance.attribute("url")?;
        let mut bindings: Vec<(String, String)> = instance
            .descendants()
            .filter(|node| node.has_tag_name_local("instance_material"))
            .filter_map(|node| {
                Some((
                    node.attribute("symbol")?.to_owned(),
                    node.attribute("target")?.to_owned(),
                ))
            })
            .collect();
        bindings.sort();
        let key = format!("{url}|{bindings:?}");
        if let Some(&mesh) = self.geometries.get(&key) {
            return Some(mesh);
        }
        let geometry = self.collada.get(url)?;
        let mesh = geometry
            .children()
            .find(|node| node.has_tag_name_local("mesh"))?;
        let mut primitives = Vec::new();
        for part in mesh.children().filter(|node| {
            node.is_element()
                && matches!(
                    node.tag_name().name(),
                    "triangles" | "polylist" | "polygons"
                )
        }) {
            let material = part.attribute("material").and_then(|symbol| {
                let target = bindings
                    .iter()
                    .find(|(known, _)| known == symbol)
                    .map_or(symbol, |(_, target)| target.as_str());
                self.material(target)
            });
            if let Some(mut primitive) = self.primitive(part) {
                primitive.material = material;
                smooth_normals(&mut primitive);
                primitives.push(primitive);
            }
        }
        self.scene.meshes.push(Mesh {
            name: geometry.attribute("name").unwrap_or_default().to_owned(),
            primitives,
            weights: Vec::new(),
        });
        let index = self.scene.meshes.len() - 1;
        self.geometries.insert(key, index);
        Some(index)
    }

    /// A `<source>`'s floats and stride.
    fn source(&self, url: &str) -> Option<(Vec<f32>, usize)> {
        let source = self.collada.get(url)?;
        // A <vertices> element stands for its POSITION source.
        if source.has_tag_name_local("vertices") {
            let position = source.children().find(|node| {
                node.has_tag_name_local("input") && node.attribute("semantic") == Some("POSITION")
            })?;
            return self.source(position.attribute("source")?);
        }
        let array = source
            .children()
            .find(|node| node.has_tag_name_local("float_array"))?;
        let floats: Vec<f32> = array
            .text()
            .map(|text| numbers(text).collect())
            .unwrap_or_default();
        let stride = source
            .descendants()
            .find(|node| node.has_tag_name_local("accessor"))
            .and_then(|node| node.attribute("stride"))
            .and_then(|stride| stride.parse::<usize>().ok())
            .filter(|&stride| stride > 0)
            .unwrap_or(3);
        Some((floats, stride))
    }

    /// The triangles of a `triangles`, `polylist` or `polygons` element,
    /// with each distinct index combination as a vertex.
    // Indices from the document are converted to the vertex table's u32.
    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
    fn primitive(&self, part: roxmltree::Node<'_, '_>) -> Option<Primitive> {
        struct Input {
            offset: usize,
            data: Vec<f32>,
            stride: usize,
        }
        let mut inputs: [Option<Input>; 4] = [None, None, None, None];
        let mut width = 0;
        for input in part
            .children()
            .filter(|node| node.has_tag_name_local("input"))
        {
            let offset = input
                .attribute("offset")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            width = width.max(offset + 1);
            let slot = match input.attribute("semantic") {
                Some("VERTEX") => 0,
                Some("NORMAL") => 1,
                Some("TEXCOORD") if input.attribute("set").is_none_or(|set| set == "0") => 2,
                Some("COLOR") => 3,
                _ => continue,
            };
            if inputs[slot].is_some() {
                continue;
            }
            let Some((data, stride)) = input.attribute("source").and_then(|url| self.source(url))
            else {
                continue;
            };
            inputs[slot] = Some(Input {
                offset,
                data,
                stride,
            });
            // The <vertices> element may also carry normals and coordinates.
            if slot == 0 {
                if let Some(vertices) = input
                    .attribute("source")
                    .and_then(|url| self.collada.get(url))
                {
                    for extra in vertices
                        .children()
                        .filter(|node| node.has_tag_name_local("input"))
                    {
                        let extra_slot = match extra.attribute("semantic") {
                            Some("NORMAL") => 1,
                            Some("TEXCOORD") => 2,
                            Some("COLOR") => 3,
                            _ => continue,
                        };
                        if inputs[extra_slot].is_none() {
                            if let Some((data, stride)) =
                                extra.attribute("source").and_then(|url| self.source(url))
                            {
                                inputs[extra_slot] = Some(Input {
                                    offset,
                                    data,
                                    stride,
                                });
                            }
                        }
                    }
                }
            }
        }
        inputs[0].as_ref()?;
        let width = width.max(1);
        let indices_of = |node: roxmltree::Node<'_, '_>| -> Vec<usize> {
            node.text()
                .map(|text| {
                    text.split_ascii_whitespace()
                        .filter_map(|word| word.parse().ok())
                        .collect()
                })
                .unwrap_or_default()
        };
        // Polygons as lists of corners (each corner = `width` indices).
        let mut polygons: Vec<Vec<usize>> = Vec::new();
        match part.tag_name().name() {
            "triangles" => {
                if let Some(p) = part.children().find(|node| node.has_tag_name_local("p")) {
                    polygons = indices_of(p)
                        .chunks_exact(3 * width)
                        .map(<[usize]>::to_vec)
                        .collect();
                }
            }
            "polylist" => {
                let counts = part
                    .children()
                    .find(|node| node.has_tag_name_local("vcount"))
                    .map(indices_of)
                    .unwrap_or_default();
                if let Some(p) = part.children().find(|node| node.has_tag_name_local("p")) {
                    let all = indices_of(p);
                    let mut at = 0;
                    for count in counts {
                        let end = at + count * width;
                        if let Some(corners) = all.get(at..end) {
                            polygons.push(corners.to_vec());
                        }
                        at = end;
                    }
                }
            }
            _ => {
                for p in part.children().filter(|node| node.has_tag_name_local("p")) {
                    polygons.push(indices_of(p));
                }
            }
        }
        let mut out = Primitive::default();
        let mut table: HashMap<Vec<usize>, u32> = HashMap::new();
        for polygon in polygons {
            let corners: Vec<u32> = polygon
                .chunks_exact(width)
                .map(|corner| {
                    if let Some(&index) = table.get(corner) {
                        return index;
                    }
                    let index = out.positions.len() as u32;
                    let read = |slot: usize, size: usize| -> Option<Vec<f32>> {
                        let input = inputs[slot].as_ref()?;
                        let element = *corner.get(input.offset)?;
                        input
                            .data
                            .get(element * input.stride..element * input.stride + size)
                            .map(<[f32]>::to_vec)
                    };
                    let position = read(0, 3).map_or(Vec3::ZERO, |p| Vec3::from_slice(&p));
                    out.positions.push(position);
                    if let Some(normal) = read(1, 3) {
                        out.normals.push(Vec3::from_slice(&normal));
                    }
                    if let Some(uv) = read(2, 2) {
                        out.uvs.push(Vec2::new(uv[0], 1.0 - uv[1]));
                    }
                    if let Some(colour) = read(3, 3) {
                        let alpha = read(3, 4).map_or(1.0, |c| c[3]);
                        out.colors
                            .push(Vec4::new(colour[0], colour[1], colour[2], alpha));
                    }
                    table.insert(corner.to_vec(), index);
                    index
                })
                .collect();
            if let Some((&first, rest)) = corners.split_first() {
                for pair in rest.windows(2) {
                    out.triangles.push([first, pair[0], pair[1]]);
                }
            }
        }
        let count = out.positions.len();
        if out.uvs.len() != count {
            out.uvs.clear();
        }
        if out.normals.len() != count {
            out.normals.clear();
        }
        if out.colors.len() != count {
            out.colors.clear();
        }
        Some(out)
    }

    /// The scene material of a COLLADA material ID.
    fn material(&mut self, url: &str) -> Option<usize> {
        let id = url.trim_start_matches('#').to_owned();
        if let Some(&index) = self.materials.get(&id) {
            return Some(index);
        }
        let material = self.collada.get(&id)?;
        let effect = material
            .children()
            .find(|node| node.has_tag_name_local("instance_effect"))
            .and_then(|node| self.collada.get(node.attribute("url")?));
        let mut out = Material {
            name: material.attribute("name").unwrap_or(&id).to_owned(),
            ..Material::default()
        };
        if let Some(effect) = effect {
            if let Some(diffuse) = effect
                .descendants()
                .find(|node| node.has_tag_name_local("diffuse"))
            {
                if let Some(colour) = diffuse
                    .children()
                    .find(|node| node.has_tag_name_local("color"))
                {
                    let values: Vec<f32> = colour
                        .text()
                        .map(|text| numbers(text).collect())
                        .unwrap_or_default();
                    if values.len() >= 3 {
                        out.base_color = Vec4::new(
                            values[0],
                            values[1],
                            values[2],
                            values.get(3).copied().unwrap_or(1.0),
                        );
                    }
                }
                if let Some(texture) = diffuse
                    .children()
                    .find(|node| node.has_tag_name_local("texture"))
                {
                    out.base_color = Vec4::ONE;
                    out.texture = texture
                        .attribute("texture")
                        .and_then(|sampler| self.texture(effect, sampler));
                }
            }
        }
        self.scene.materials.push(out);
        let index = self.scene.materials.len() - 1;
        self.materials.insert(id, index);
        Some(index)
    }

    /// Follows a sampler (COLLADA 1.4 newparam chain or 1.5 `instance_image`)
    /// to an image file and loads it.
    fn texture(&mut self, effect: roxmltree::Node<'_, '_>, sampler: &str) -> Option<usize> {
        let param = |sid: &str| {
            effect.descendants().find(|node| {
                node.has_tag_name_local("newparam") && node.attribute("sid") == Some(sid)
            })
        };
        let image_id = match param(sampler) {
            Some(newparam) => {
                let sampler2d = newparam
                    .descendants()
                    .find(|node| node.has_tag_name_local("sampler2D"))?;
                if let Some(instance) = sampler2d
                    .children()
                    .find(|node| node.has_tag_name_local("instance_image"))
                {
                    instance
                        .attribute("url")?
                        .trim_start_matches('#')
                        .to_owned()
                } else {
                    let surface_sid = sampler2d
                        .children()
                        .find(|node| node.has_tag_name_local("source"))?
                        .text()?
                        .trim()
                        .to_owned();
                    let surface = param(&surface_sid)?;
                    surface
                        .descendants()
                        .find(|node| node.has_tag_name_local("init_from"))?
                        .text()?
                        .trim()
                        .to_owned()
                }
            }
            // Some exporters name the image directly.
            None => sampler.to_owned(),
        };
        if let Some(&known) = self.images.get(&image_id) {
            return known;
        }
        let file = self.collada.get(&image_id).and_then(|image| {
            let init = image
                .descendants()
                .find(|node| node.has_tag_name_local("init_from"))?;
            let reference = init
                .children()
                .find(|node| node.has_tag_name_local("ref"))
                .and_then(|node| node.text())
                .or_else(|| init.text())?;
            Some(
                reference
                    .trim()
                    .trim_start_matches("file://")
                    .replace("%20", " "),
            )
        });
        let texture = file.and_then(|file| {
            let folder = self.folder?;
            let texture = super::mesh_formats::texture_file(&folder.join(file.replace('\\', "/")))?;
            self.scene.textures.push(texture);
            Some(self.scene.textures.len() - 1)
        });
        self.images.insert(image_id, texture);
        texture
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn reads_a_3mf_package() {
        let model = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <basematerials id="5"><base name="Red" displaycolor="#FF0000"/></basematerials>
    <object id="1" type="model" pid="5" pindex="0">
      <mesh>
        <vertices><vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/><vertex x="0" y="10" z="0"/></vertices>
        <triangles><triangle v1="0" v2="1" v3="2"/></triangles>
      </mesh>
    </object>
  </resources>
  <build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></build>
</model>"##;
        let mut data = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut data));
            let options = zip::write::SimpleFileOptions::default();
            writer
                .start_file("3D/3dmodel.model", options)
                .expect("file");
            writer.write_all(model.as_bytes()).expect("write");
            writer.finish().expect("finish");
        }
        let scene = load_3mf(&data).expect("loads");
        assert_eq!(scene.stats().triangles, 1);
        assert_eq!(scene.up, UpAxis::Z);
        assert_eq!(scene.materials[0].base_color, Vec4::new(1.0, 0.0, 0.0, 1.0));
        let geometry = scene.geometry(&scene.pose(None, 0.0));
        // Shifted by the build transform, then turned Z-up to Y-up.
        assert!((geometry.positions[0].x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn reads_a_collada_document() {
        let dae = br##"<?xml version="1.0"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <asset><up_axis>Z_UP</up_axis></asset>
  <library_effects><effect id="fx"><profile_COMMON><technique sid="t"><lambert>
    <diffuse><color>0 1 0 1</color></diffuse></lambert></technique></profile_COMMON></effect></library_effects>
  <library_materials><material id="green" name="Green"><instance_effect url="#fx"/></material></library_materials>
  <library_geometries><geometry id="quad" name="Quad"><mesh>
    <source id="pos"><float_array id="pa" count="12">0 0 0 1 0 0 1 1 0 0 1 0</float_array>
      <technique_common><accessor source="#pa" count="4" stride="3"/></technique_common></source>
    <vertices id="verts"><input semantic="POSITION" source="#pos"/></vertices>
    <polylist material="mat" count="1"><input semantic="VERTEX" source="#verts" offset="0"/>
      <vcount>4</vcount><p>0 1 2 3</p></polylist>
  </mesh></geometry></library_geometries>
  <library_visual_scenes><visual_scene id="s"><node id="n" name="Quad">
    <translate>0 0 2</translate>
    <instance_geometry url="#quad"><bind_material><technique_common>
      <instance_material symbol="mat" target="#green"/></technique_common></bind_material></instance_geometry>
  </node></visual_scene></library_visual_scenes>
  <scene><instance_visual_scene url="#s"/></scene>
</COLLADA>"##;
        let scene = load_dae(dae, None).expect("loads");
        assert_eq!(scene.stats().triangles, 2);
        assert_eq!(scene.up, UpAxis::Z);
        assert_eq!(scene.materials[0].name, "Green");
        assert_eq!(scene.materials[0].base_color, Vec4::new(0.0, 1.0, 0.0, 1.0));
        let geometry = scene.geometry(&scene.pose(None, 0.0));
        // Z 2 becomes Y 2 once the scene stands up.
        assert!((geometry.positions[0].y - 2.0).abs() < 1e-5);
        assert!(load_dae(b"<svg/>", None).is_err());
    }

    #[test]
    fn colours_parse() {
        assert_eq!(
            hex_colour("#00FF0080"),
            Some(Vec4::new(0.0, 1.0, 0.0, 128.0 / 255.0))
        );
        assert_eq!(hex_colour("red"), None);
        assert_eq!(matrix_3mf("1 2 3"), Mat4::IDENTITY);
    }
}
