//! The scene every loader produces, and its animation.
//!
//! A scene is a node hierarchy (glTF's model, which the other formats fit
//! into): nodes carry a rest transform and may instance a mesh, optionally
//! skinned; animations drive node translations, rotations, scales and morph
//! weights. [`Scene::pose`] evaluates the animation at a time and
//! [`Scene::geometry`] bakes the posed meshes into world-space triangles.

use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

/// Which axis points up in the file. Z-up models are turned to Y-up so the
/// ground plane is always `y = 0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpAxis {
    /// Y up (glTF, OBJ, FBX by default).
    #[default]
    Y,
    /// Z up (STL, 3MF, many CAD exports).
    Z,
}

/// An RGBA8 texture.
#[derive(Clone, Debug, Default)]
pub struct Texture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Unpremultiplied RGBA, row by row.
    pub rgba: Vec<u8>,
}

impl Texture {
    /// The colour at texture coordinates (wrapping, nearest texel), 0..=1.
    pub fn sample(&self, uv: Vec2) -> Vec4 {
        if self.width == 0 || self.height == 0 {
            return Vec4::ONE;
        }
        let u = uv.x - uv.x.floor();
        let v = uv.y - uv.y.floor();
        // Truncation picks the texel; both are below the size.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let (x, y) = (
            ((u * self.width as f32) as u32).min(self.width - 1),
            ((v * self.height as f32) as u32).min(self.height - 1),
        );
        let at = (y as usize * self.width as usize + x as usize) * 4;
        match self.rgba.get(at..at + 4) {
            Some(texel) => {
                Vec4::new(
                    f32::from(texel[0]),
                    f32::from(texel[1]),
                    f32::from(texel[2]),
                    f32::from(texel[3]),
                ) / 255.0
            }
            None => Vec4::ONE,
        }
    }
}

/// A surface's look.
#[derive(Clone, Debug)]
pub struct Material {
    /// Name, for the properties panel.
    pub name: String,
    /// Base colour, linear RGBA.
    pub base_color: Vec4,
    /// Base colour texture.
    pub texture: Option<usize>,
    /// Both faces are visible.
    pub double_sided: bool,
    /// Emitted colour.
    pub emissive: Vec3,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_color: Vec4::new(0.8, 0.8, 0.8, 1.0),
            texture: None,
            double_sided: true,
            emissive: Vec3::ZERO,
        }
    }
}

/// Displacements of one morph target.
#[derive(Clone, Debug, Default)]
pub struct MorphTarget {
    /// Position offsets, one per vertex (or empty).
    pub positions: Vec<Vec3>,
    /// Normal offsets, one per vertex (or empty).
    pub normals: Vec<Vec3>,
}

/// One drawable piece of a mesh, with one material.
#[derive(Clone, Debug, Default)]
pub struct Primitive {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Vertex normals; empty means flat shading from the faces.
    pub normals: Vec<Vec3>,
    /// Texture coordinates (or empty).
    pub uvs: Vec<Vec2>,
    /// Vertex colours, linear RGBA (or empty).
    pub colors: Vec<Vec4>,
    /// Skin joints, indices into the skin's joint list (or empty).
    pub joints: Vec<[u16; 4]>,
    /// Skin weights (or empty).
    pub weights: Vec<[f32; 4]>,
    /// Morph targets.
    pub targets: Vec<MorphTarget>,
    /// Triangles.
    pub triangles: Vec<[u32; 3]>,
    /// Line segments.
    pub lines: Vec<[u32; 2]>,
    /// The material.
    pub material: Option<usize>,
}

/// A mesh: primitives sharing morph weights.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// Name, for the properties panel.
    pub name: String,
    /// The pieces.
    pub primitives: Vec<Primitive>,
    /// Morph weights when no animation sets them.
    pub weights: Vec<f32>,
}

/// A local transform: scale, then rotation, then translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Translation.
    pub translation: Vec3,
    /// Rotation.
    pub rotation: Quat,
    /// Scale.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    /// The transform of a matrix (assumed affine, no shear).
    pub fn from_matrix(matrix: Mat4) -> Self {
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// The matrix.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// A node of the hierarchy.
#[derive(Clone, Debug, Default)]
pub struct Node {
    /// Name, for the properties panel.
    pub name: String,
    /// Rest transform relative to the parent.
    pub transform: Transform,
    /// The mesh it shows.
    pub mesh: Option<usize>,
    /// The skin deforming that mesh.
    pub skin: Option<usize>,
    /// Child nodes.
    pub children: Vec<usize>,
}

/// Joints and their inverse bind matrices.
#[derive(Clone, Debug, Default)]
pub struct Skin {
    /// Joint nodes.
    pub joints: Vec<usize>,
    /// One per joint; identity when the file has none.
    pub inverse_bind: Vec<Mat4>,
}

/// What a channel animates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    /// The node's translation.
    Translation,
    /// The node's rotation.
    Rotation,
    /// The node's scale.
    Scale,
    /// The morph weights of the node's mesh.
    Weights,
}

/// How values between keyframes are found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Hold the previous key.
    Step,
    /// Straight lines (spherical for rotations).
    Linear,
    /// Hermite splines with in and out tangents per key.
    Cubic,
}

/// One animated property of one node.
#[derive(Clone, Debug)]
pub struct Channel {
    /// The node.
    pub node: usize,
    /// The property.
    pub property: Property,
    /// The interpolation.
    pub interpolation: Interpolation,
    /// Key times, seconds, ascending.
    pub times: Vec<f32>,
    /// Key values, flattened; for cubic splines each key holds in-tangent,
    /// value and out-tangent.
    pub values: Vec<f32>,
}

/// A named animation clip.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    /// Name.
    pub name: String,
    /// The channels.
    pub channels: Vec<Channel>,
    /// Length, seconds.
    pub duration: f32,
}

/// A loaded model.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// All nodes.
    pub nodes: Vec<Node>,
    /// Nodes without a parent, in drawing order.
    pub roots: Vec<usize>,
    /// Meshes.
    pub meshes: Vec<Mesh>,
    /// Materials.
    pub materials: Vec<Material>,
    /// Textures.
    pub textures: Vec<Texture>,
    /// Skins.
    pub skins: Vec<Skin>,
    /// Animation clips.
    pub animations: Vec<Animation>,
    /// The file's up axis.
    pub up: UpAxis,
}

/// Node transforms and morph weights at one moment.
#[derive(Clone, Debug)]
pub struct Pose {
    /// World matrix of every node.
    pub world: Vec<Mat4>,
    /// Morph weights per node (empty when the node's mesh has none).
    pub weights: Vec<Vec<f32>>,
}

/// One triangle of baked geometry.
#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    /// Vertex indices into the geometry.
    pub vertices: [u32; 3],
    /// Material.
    pub material: Option<usize>,
    /// Use the face normal.
    pub flat: bool,
}

/// World-space geometry of a pose, ready to draw.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// Positions.
    pub positions: Vec<Vec3>,
    /// Normals (zero where the primitive had none).
    pub normals: Vec<Vec3>,
    /// Texture coordinates.
    pub uvs: Vec<Vec2>,
    /// Colours.
    pub colors: Vec<Vec4>,
    /// Triangles.
    pub triangles: Vec<Triangle>,
    /// Line segments.
    pub lines: Vec<[u32; 2]>,
}

impl Geometry {
    /// The axis-aligned bounds of every vertex, or `None` when empty.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.positions.iter().filter(|point| point.is_finite());
        let first = *points.next()?;
        Some(points.fold((first, first), |(low, high), point| {
            (low.min(*point), high.max(*point))
        }))
    }
}

/// Counts for the properties panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Vertices.
    pub vertices: usize,
    /// Triangles.
    pub triangles: usize,
    /// Nodes.
    pub nodes: usize,
    /// Meshes.
    pub meshes: usize,
    /// Materials.
    pub materials: usize,
    /// Textures.
    pub textures: usize,
    /// Animation clips.
    pub animations: usize,
    /// Skins.
    pub skins: usize,
}

/// Rotation turning Z-up coordinates into Y-up ones.
fn up_correction(up: UpAxis) -> Mat4 {
    match up {
        UpAxis::Y => Mat4::IDENTITY,
        UpAxis::Z => Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
    }
}

impl Scene {
    /// Counts of the scene's parts.
    pub fn stats(&self) -> Stats {
        let mut stats = Stats {
            nodes: self.nodes.len(),
            meshes: self.meshes.len(),
            materials: self.materials.len(),
            textures: self.textures.len(),
            animations: self.animations.len(),
            skins: self.skins.len(),
            ..Stats::default()
        };
        for node in &self.nodes {
            if let Some(mesh) = node.mesh.and_then(|index| self.meshes.get(index)) {
                for primitive in &mesh.primitives {
                    stats.vertices += primitive.positions.len();
                    stats.triangles += primitive.triangles.len();
                }
            }
        }
        stats
    }

    /// Makes every node without a parent a root, if the loader left
    /// `roots` empty, and drops references to missing parts.
    pub fn finish(&mut self) {
        let node_count = self.nodes.len();
        for node in &mut self.nodes {
            node.children.retain(|&child| child < node_count);
            if node.mesh.is_some_and(|mesh| mesh >= self.meshes.len()) {
                node.mesh = None;
            }
            if node.skin.is_some_and(|skin| skin >= self.skins.len()) {
                node.skin = None;
            }
        }
        if self.roots.is_empty() {
            let mut has_parent = vec![false; node_count];
            for node in &self.nodes {
                for &child in &node.children {
                    has_parent[child] = true;
                }
            }
            self.roots = (0..node_count)
                .filter(|&index| !has_parent[index])
                .collect();
        }
        self.roots.retain(|&root| root < node_count);
        for animation in &mut self.animations {
            animation
                .channels
                .retain(|channel| channel.node < node_count && !channel.times.is_empty());
            animation.duration = animation
                .channels
                .iter()
                .filter_map(|channel| channel.times.last().copied())
                .fold(0.0, f32::max);
        }
    }

    /// The pose at `time` seconds of `animation` (the rest pose for `None`).
    pub fn pose(&self, animation: Option<usize>, time: f32) -> Pose {
        let mut locals: Vec<Transform> = self.nodes.iter().map(|node| node.transform).collect();
        let mut weights: Vec<Vec<f32>> = self
            .nodes
            .iter()
            .map(|node| {
                node.mesh
                    .and_then(|mesh| self.meshes.get(mesh))
                    .map(|mesh| mesh.weights.clone())
                    .unwrap_or_default()
            })
            .collect();
        if let Some(clip) = animation.and_then(|index| self.animations.get(index)) {
            for channel in &clip.channels {
                apply_channel(
                    channel,
                    time,
                    &mut locals[channel.node],
                    &mut weights[channel.node],
                );
            }
        }
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        let mut visited = vec![false; self.nodes.len()];
        let mut stack: Vec<(usize, Mat4)> = self
            .roots
            .iter()
            .rev()
            .map(|&root| (root, up_correction(self.up)))
            .collect();
        while let Some((index, parent)) = stack.pop() {
            if std::mem::replace(&mut visited[index], true) {
                continue;
            }
            let matrix = parent * locals[index].matrix();
            world[index] = matrix;
            for &child in self.nodes[index].children.iter().rev() {
                stack.push((child, matrix));
            }
        }
        Pose { world, weights }
    }

    /// The posed meshes as world-space geometry.
    pub fn geometry(&self, pose: &Pose) -> Geometry {
        let mut out = Geometry::default();
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(mesh) = node.mesh.and_then(|mesh| self.meshes.get(mesh)) else {
                continue;
            };
            let skin = node.skin.and_then(|skin| self.skins.get(skin));
            let joints: Option<Vec<Mat4>> = skin.map(|skin| {
                skin.joints
                    .iter()
                    .enumerate()
                    .map(|(joint, &node)| {
                        let inverse = skin
                            .inverse_bind
                            .get(joint)
                            .copied()
                            .unwrap_or(Mat4::IDENTITY);
                        pose.world.get(node).copied().unwrap_or(Mat4::IDENTITY) * inverse
                    })
                    .collect()
            });
            let weights = pose.weights.get(index).map_or(&[][..], Vec::as_slice);
            for primitive in &mesh.primitives {
                append(
                    &mut out,
                    primitive,
                    pose.world[index],
                    joints.as_deref(),
                    weights,
                );
            }
        }
        out
    }
}

/// Adds one posed primitive to the geometry.
fn append(
    out: &mut Geometry,
    primitive: &Primitive,
    world: Mat4,
    joints: Option<&[Mat4]>,
    weights: &[f32],
) {
    let Ok(base) = u32::try_from(out.positions.len()) else {
        return;
    };
    let count = primitive.positions.len();
    let skinned =
        joints.filter(|_| primitive.joints.len() == count && primitive.weights.len() == count);
    for vertex in 0..count {
        let mut position = primitive.positions[vertex];
        let mut normal = primitive.normals.get(vertex).copied().unwrap_or(Vec3::ZERO);
        for (target, &weight) in primitive.targets.iter().zip(weights) {
            if weight.abs() > f32::EPSILON {
                if let Some(offset) = target.positions.get(vertex) {
                    position += *offset * weight;
                }
                if let Some(offset) = target.normals.get(vertex) {
                    normal += *offset * weight;
                }
            }
        }
        let matrix = match skinned {
            Some(joints) => {
                let mut sum = Mat4::ZERO;
                let mut total = 0.0;
                for (&joint, &weight) in primitive.joints[vertex]
                    .iter()
                    .zip(&primitive.weights[vertex])
                {
                    if weight > 0.0 {
                        if let Some(matrix) = joints.get(usize::from(joint)) {
                            sum += *matrix * weight;
                            total += weight;
                        }
                    }
                }
                if total > 0.0 {
                    sum * (1.0 / total)
                } else {
                    world
                }
            }
            None => world,
        };
        out.positions.push(matrix.transform_point3(position));
        let normal_matrix = Mat3::from_mat4(matrix).inverse().transpose();
        let normal = normal_matrix * normal;
        out.normals.push(normal.normalize_or_zero());
        out.uvs
            .push(primitive.uvs.get(vertex).copied().unwrap_or(Vec2::ZERO));
        out.colors
            .push(primitive.colors.get(vertex).copied().unwrap_or(Vec4::ONE));
    }
    let flat = primitive.normals.len() != count;
    let in_range = |index: u32| (index as usize) < count;
    for triangle in &primitive.triangles {
        if triangle.iter().all(|&index| in_range(index)) {
            out.triangles.push(Triangle {
                vertices: triangle.map(|index| base + index),
                material: primitive.material,
                flat,
            });
        }
    }
    for line in &primitive.lines {
        if line.iter().all(|&index| in_range(index)) {
            out.lines.push(line.map(|index| base + index));
        }
    }
}

/// Sets the animated property of one node at `time`.
fn apply_channel(channel: &Channel, time: f32, local: &mut Transform, weights: &mut Vec<f32>) {
    let keys = channel.times.len();
    let width = match channel.property {
        Property::Translation | Property::Scale => 3,
        Property::Rotation => 4,
        Property::Weights => {
            let per_key = if channel.interpolation == Interpolation::Cubic {
                keys * 3
            } else {
                keys
            };
            if per_key == 0 {
                return;
            }
            channel.values.len() / per_key
        }
    };
    if width == 0 {
        return;
    }
    let value = sample(channel, time, width);
    if value.len() < width {
        return;
    }
    match channel.property {
        Property::Translation => local.translation = Vec3::from_slice(&value),
        Property::Scale => local.scale = Vec3::from_slice(&value),
        Property::Rotation => {
            let rotation = Quat::from_xyzw(value[0], value[1], value[2], value[3]);
            if rotation.length_squared() > 1e-12 {
                local.rotation = rotation.normalize();
            }
        }
        Property::Weights => {
            weights.resize(width, 0.0);
            weights.copy_from_slice(&value[..width]);
        }
    }
}

/// The channel's value at `time`, `width` numbers.
// Spline formulas keep their textbook names (h00..h11, a, b, t).
#[allow(clippy::many_single_char_names, clippy::similar_names)]
fn sample(channel: &Channel, time: f32, width: usize) -> Vec<f32> {
    let times = &channel.times;
    let cubic = channel.interpolation == Interpolation::Cubic;
    let stride = if cubic { width * 3 } else { width };
    let key = |index: usize| -> Vec<f32> {
        let start = index * stride + if cubic { width } else { 0 };
        channel
            .values
            .get(start..start + width)
            .map(<[f32]>::to_vec)
            .unwrap_or_default()
    };
    let last = times.len() - 1;
    if time <= times[0] || last == 0 {
        return key(0);
    }
    if time >= times[last] {
        return key(last);
    }
    let next = times
        .partition_point(|&key_time| key_time <= time)
        .min(last);
    let previous = next - 1;
    let span = times[next] - times[previous];
    let t = if span > 0.0 {
        (time - times[previous]) / span
    } else {
        0.0
    };
    let (a, b) = (key(previous), key(next));
    if a.len() < width || b.len() < width {
        return Vec::new();
    }
    match channel.interpolation {
        Interpolation::Step => a,
        Interpolation::Linear if channel.property == Property::Rotation => {
            let qa = Quat::from_xyzw(a[0], a[1], a[2], a[3]).normalize();
            let qb = Quat::from_xyzw(b[0], b[1], b[2], b[3]).normalize();
            qa.slerp(qb, t).to_array().to_vec()
        }
        Interpolation::Linear => a.iter().zip(&b).map(|(x, y)| x + (y - x) * t).collect(),
        Interpolation::Cubic => {
            let tangent = |index: usize, offset: usize| -> Vec<f32> {
                let start = index * stride + offset;
                channel
                    .values
                    .get(start..start + width)
                    .map_or_else(|| vec![0.0; width], <[f32]>::to_vec)
            };
            let out_tangent = tangent(previous, 2 * width);
            let in_tangent = tangent(next, 0);
            let (t2, t3) = (t * t, t * t * t);
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            (0..width)
                .map(|i| {
                    h00 * a[i]
                        + h10 * span * out_tangent[i]
                        + h01 * b[i]
                        + h11 * span * in_tangent[i]
                })
                .collect()
        }
    }
}

/// Fills missing normals of every primitive by averaging the faces around
/// each vertex. Loaders call it for formats whose vertices are shared
/// between faces on purpose (smooth surfaces).
pub(crate) fn smooth_normals(primitive: &mut Primitive) {
    if primitive.normals.len() == primitive.positions.len() {
        return;
    }
    let mut normals = vec![Vec3::ZERO; primitive.positions.len()];
    for triangle in &primitive.triangles {
        let [a, b, c] = triangle.map(|index| index as usize);
        let (Some(pa), Some(pb), Some(pc)) = (
            primitive.positions.get(a),
            primitive.positions.get(b),
            primitive.positions.get(c),
        ) else {
            continue;
        };
        let face = (*pb - *pa).cross(*pc - *pa);
        for index in [a, b, c] {
            normals[index] += face;
        }
    }
    primitive.normals = normals.into_iter().map(Vec3::normalize_or_zero).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle_scene() -> Scene {
        let mut scene = Scene {
            meshes: vec![Mesh {
                primitives: vec![Primitive {
                    positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
                    triangles: vec![[0, 1, 2]],
                    targets: vec![MorphTarget {
                        positions: vec![Vec3::ZERO, Vec3::ZERO, Vec3::Y],
                        normals: Vec::new(),
                    }],
                    ..Primitive::default()
                }],
                weights: vec![0.0],
                ..Mesh::default()
            }],
            nodes: vec![
                Node {
                    children: vec![1],
                    transform: Transform {
                        translation: Vec3::new(10.0, 0.0, 0.0),
                        ..Transform::default()
                    },
                    ..Node::default()
                },
                Node {
                    mesh: Some(0),
                    ..Node::default()
                },
            ],
            animations: vec![Animation {
                name: "move".into(),
                channels: vec![
                    Channel {
                        node: 1,
                        property: Property::Translation,
                        interpolation: Interpolation::Linear,
                        times: vec![0.0, 2.0],
                        values: vec![0.0, 0.0, 0.0, 0.0, 4.0, 0.0],
                    },
                    Channel {
                        node: 1,
                        property: Property::Weights,
                        interpolation: Interpolation::Step,
                        times: vec![0.0, 1.0],
                        values: vec![0.0, 1.0],
                    },
                ],
                duration: 0.0,
            }],
            ..Scene::default()
        };
        scene.finish();
        scene
    }

    #[test]
    fn poses_follow_the_hierarchy_and_the_clip() {
        let scene = triangle_scene();
        assert_eq!(scene.roots, vec![0]);
        assert!((scene.animations[0].duration - 2.0).abs() < 1e-6);
        let rest = scene.geometry(&scene.pose(None, 0.0));
        assert_eq!(rest.positions[0], Vec3::new(10.0, 0.0, 0.0));
        let half = scene.geometry(&scene.pose(Some(0), 1.0));
        assert!((half.positions[0] - Vec3::new(10.0, 2.0, 0.0)).length() < 1e-5);
        // Step weights switched the morph target on at one second.
        assert!((half.positions[2] - Vec3::new(10.0, 4.0, 0.0)).length() < 1e-5);
        let end = scene.geometry(&scene.pose(Some(0), 5.0));
        assert!((end.positions[0] - Vec3::new(10.0, 4.0, 0.0)).length() < 1e-5);
        assert_eq!(end.triangles.len(), 1);
        assert!(end.triangles[0].flat);
        assert_eq!(scene.stats().triangles, 1);
    }

    #[test]
    fn rotations_slerp_and_cubic_splines_hit_their_keys() {
        let channel = Channel {
            node: 0,
            property: Property::Rotation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            values: [
                Quat::IDENTITY,
                Quat::from_rotation_y(std::f32::consts::PI / 2.0),
            ]
            .iter()
            .flat_map(Quat::to_array)
            .collect(),
        };
        let mut local = Transform::default();
        apply_channel(&channel, 0.5, &mut local, &mut Vec::new());
        let expected = Quat::from_rotation_y(std::f32::consts::PI / 4.0);
        assert!(local.rotation.angle_between(expected) < 1e-4);

        let cubic = Channel {
            node: 0,
            property: Property::Translation,
            interpolation: Interpolation::Cubic,
            times: vec![0.0, 1.0],
            // in, value, out per key.
            values: vec![
                0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 5.0, 6.0, 0.0,
                0.0, 0.0,
            ],
        };
        let mut local = Transform::default();
        apply_channel(&cubic, 1.0, &mut local, &mut Vec::new());
        assert_eq!(local.translation, Vec3::new(4.0, 5.0, 6.0));
        apply_channel(&cubic, 0.5, &mut local, &mut Vec::new());
        assert!((local.translation - Vec3::new(2.5, 3.5, 4.5)).length() < 1e-5);
    }

    #[test]
    fn skinning_follows_the_joints() {
        let mut scene = Scene {
            meshes: vec![Mesh {
                primitives: vec![Primitive {
                    positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
                    triangles: vec![[0, 1, 2]],
                    joints: vec![[0, 0, 0, 0]; 3],
                    weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
                    ..Primitive::default()
                }],
                ..Mesh::default()
            }],
            nodes: vec![
                Node {
                    mesh: Some(0),
                    skin: Some(0),
                    ..Node::default()
                },
                Node {
                    transform: Transform {
                        translation: Vec3::new(0.0, 0.0, 5.0),
                        ..Transform::default()
                    },
                    ..Node::default()
                },
            ],
            skins: vec![Skin {
                joints: vec![1],
                inverse_bind: vec![Mat4::IDENTITY],
            }],
            ..Scene::default()
        };
        scene.finish();
        let geometry = scene.geometry(&scene.pose(None, 0.0));
        assert_eq!(geometry.positions[1], Vec3::new(1.0, 0.0, 5.0));
    }

    #[test]
    fn z_up_scenes_stand_on_the_ground() {
        let mut scene = triangle_scene();
        scene.up = UpAxis::Z;
        scene.nodes[0].transform.translation = Vec3::ZERO;
        let geometry = scene.geometry(&scene.pose(None, 0.0));
        // Z-up (0,1,0) is "forward"; turned to Y-up it lies at -Z.
        assert!((geometry.positions[2] - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5);
    }
}
