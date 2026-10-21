//! A software renderer: depth-buffered triangles with lighting and textures,
//! wireframes, the ground plane with its grid, axes and bounds.
//!
//! It draws into an RGBA buffer the widget shows as a texture, so it runs
//! wherever the gallery does, without a GPU context of its own.

// Pixel coordinates are bounded by the buffer size; rasterizer formulas keep
// their usual short names (s0, w1, x0...).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names
)]

use glam::{Mat4, Vec2, Vec3, Vec4, Vec4Swizzles};
use rayon::prelude::*;

use super::scene::{Geometry, Material, Texture};

/// An orbiting camera around a target point (Y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The point looked at.
    pub target: Vec3,
    /// Turn around the vertical axis, radians.
    pub yaw: f32,
    /// Elevation, radians (positive looks down).
    pub pitch: f32,
    /// Distance from the target.
    pub distance: f32,
    /// Vertical field of view, radians.
    pub fov: f32,
    /// Parallel projection instead of perspective.
    pub orthographic: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.6,
            pitch: 0.45,
            distance: 3.0,
            fov: 45f32.to_radians(),
            orthographic: false,
        }
    }
}

/// Standard viewpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Looking along -Z.
    Front,
    /// Looking along +Z.
    Back,
    /// Looking along +X.
    Left,
    /// Looking along -X.
    Right,
    /// Looking down.
    Top,
    /// Looking up.
    Bottom,
    /// Three-quarter view from above.
    Isometric,
}

impl Camera {
    /// A camera framing `bounds` from the default angle.
    pub fn framing(bounds: Option<(Vec3, Vec3)>) -> Self {
        let mut camera = Self::default();
        camera.fit(bounds);
        camera
    }

    /// Frames `bounds`, keeping the angles.
    pub fn fit(&mut self, bounds: Option<(Vec3, Vec3)>) {
        let (low, high) = bounds.unwrap_or((Vec3::splat(-1.0), Vec3::splat(1.0)));
        self.target = (low + high) / 2.0;
        let radius = ((high - low).length() / 2.0).max(1e-4);
        self.distance = radius / (self.fov / 2.0).sin() * 1.05;
    }

    /// Turns to a standard viewpoint.
    pub fn look(&mut self, view: View) {
        let quarter = std::f32::consts::FRAC_PI_2;
        let limit = quarter - 1e-3;
        (self.yaw, self.pitch) = match view {
            View::Front => (0.0, 0.0),
            View::Back => (2.0 * quarter, 0.0),
            View::Left => (-quarter, 0.0),
            View::Right => (quarter, 0.0),
            View::Top => (0.0, limit),
            View::Bottom => (0.0, -limit),
            View::Isometric => (0.6, 0.45),
        };
    }

    /// The camera position.
    pub fn eye(&self) -> Vec3 {
        let direction = Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        );
        self.target + direction * self.distance
    }

    /// Orbits by a mouse drag of `delta` points.
    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.01;
        let limit = std::f32::consts::FRAC_PI_2 - 1e-3;
        self.pitch = (self.pitch + delta.y * 0.01).clamp(-limit, limit);
    }

    /// Moves the target in the view plane by `delta` points of a view
    /// `height` points tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        let eye = self.eye();
        let forward = (self.target - eye).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(forward);
        let world_per_point = 2.0 * self.distance * (self.fov / 2.0).tan() / height.max(1.0);
        self.target += (-right * delta.x + up * delta.y) * world_per_point;
    }

    /// Zooms by `factor` (above 1 moves closer).
    pub fn zoom(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.distance = (self.distance / factor).clamp(1e-4, 1e7);
        }
    }

    /// The view-projection matrix for an aspect ratio, with depth from 0
    /// (near) to 1 (far).
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), self.target, Vec3::Y);
        let near = (self.distance * 0.01).max(1e-5);
        let far = self.distance * 100.0 + 1.0;
        let projection = if self.orthographic {
            let half_height = self.distance * (self.fov / 2.0).tan();
            let half_width = half_height * aspect;
            Mat4::orthographic_rh(
                -half_width,
                half_width,
                -half_height,
                half_height,
                near,
                far,
            )
        } else {
            Mat4::perspective_rh(self.fov, aspect, near, far)
        };
        projection * view
    }
}

/// How surfaces are lit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shading {
    /// Smooth lighting from the vertex normals.
    #[default]
    Smooth,
    /// Faceted lighting from the face normals.
    Flat,
    /// Colours and textures only.
    Unlit,
    /// Normals as colours.
    Normals,
    /// Nothing filled (pair with the wireframe).
    Hidden,
}

/// What to draw and how.
// Independent display switches, not a state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Surface shading.
    pub shading: Shading,
    /// Use the materials' textures.
    pub textures: bool,
    /// Use vertex colours.
    pub vertex_colors: bool,
    /// Draw triangle edges.
    pub wireframe: bool,
    /// Draw the ground plane (y = 0) and its grid.
    pub ground: bool,
    /// Draw the X, Y, Z axes.
    pub axes: bool,
    /// Draw the bounding box.
    pub bounds: bool,
    /// Hide faces turned away from the camera (unless double-sided).
    pub backface_culling: bool,
    /// Background colour, RGBA.
    pub background: [u8; 4],
    /// Samples per pixel along each axis (1 to 3); 2 smooths edges.
    pub supersample: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            shading: Shading::Smooth,
            textures: true,
            vertex_colors: true,
            wireframe: false,
            ground: true,
            axes: false,
            bounds: false,
            backface_culling: false,
            background: [46, 48, 54, 255],
            supersample: 2,
        }
    }
}

/// A vertex in clip space with its attributes.
#[derive(Clone, Copy, Debug)]
struct ClipVertex {
    clip: Vec4,
    normal: Vec3,
    uv: Vec2,
    color: Vec4,
    world: Vec3,
}

impl ClipVertex {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            clip: self.clip.lerp(other.clip, t),
            normal: self.normal.lerp(other.normal, t),
            uv: self.uv.lerp(other.uv, t),
            color: self.color.lerp(other.color, t),
            world: self.world.lerp(other.world, t),
        }
    }
}

/// How far outside an edge a pixel centre may lie and still be drawn: a
/// centre exactly on an edge shared by two triangles must not fall through
/// both of them by rounding.
const EDGE: f32 = -1e-4;

/// Smallest `w` a vertex may have in front of the camera.
const NEAR_W: f32 = 1e-5;

/// Clips a polygon against the near plane (`z >= 0` in glam's 0..1 depth).
fn clip_near(polygon: &[ClipVertex]) -> Vec<ClipVertex> {
    let inside = |vertex: &ClipVertex| vertex.clip.z >= 0.0 && vertex.clip.w > NEAR_W;
    let mut out = Vec::with_capacity(polygon.len() + 2);
    for (index, current) in polygon.iter().enumerate() {
        let next = &polygon[(index + 1) % polygon.len()];
        if inside(current) {
            out.push(*current);
        }
        if inside(current) != inside(next) {
            let t = current.clip.z / (current.clip.z - next.clip.z);
            let mut point = current.lerp(next, t.clamp(0.0, 1.0));
            point.clip.w = point.clip.w.max(NEAR_W);
            out.push(point);
        }
    }
    out
}

/// The frame being drawn: colour and depth, `width` × `height`.
struct Target {
    width: usize,
    height: usize,
    color: Vec<[f32; 4]>,
    depth: Vec<f32>,
}

impl Target {
    fn to_screen(&self, clip: Vec4) -> Vec3 {
        let ndc = clip.xyz() / clip.w;
        Vec3::new(
            f32::midpoint(ndc.x, 1.0) * self.width as f32,
            (1.0 - ndc.y) / 2.0 * self.height as f32,
            ndc.z,
        )
    }

    /// Draws a depth-tested line between two clip-space points.
    fn line(&mut self, a: Vec4, b: Vec4, colour: [f32; 4], bias: f32) {
        let polygon = [a, b].map(|clip| ClipVertex {
            clip,
            normal: Vec3::ZERO,
            uv: Vec2::ZERO,
            color: Vec4::ZERO,
            world: Vec3::ZERO,
        });
        // Clip the segment against the near plane.
        let (mut p, mut q) = (polygon[0], polygon[1]);
        let inside = |vertex: &ClipVertex| vertex.clip.z >= 0.0 && vertex.clip.w > NEAR_W;
        match (inside(&p), inside(&q)) {
            (false, false) => return,
            (true, false) => q = p.lerp(&q, p.clip.z / (p.clip.z - q.clip.z)),
            (false, true) => p = q.lerp(&p, q.clip.z / (q.clip.z - p.clip.z)),
            (true, true) => {}
        }
        let (start, end) = (self.to_screen(p.clip), self.to_screen(q.clip));
        if !(start.is_finite() && end.is_finite()) {
            return;
        }
        let steps = (end - start)
            .truncate()
            .abs()
            .max_element()
            .ceil()
            .clamp(1.0, 16_384.0) as usize;
        for step in 0..=steps {
            let point = start.lerp(end, step as f32 / steps as f32);
            let (x, y) = (point.x.floor(), point.y.floor());
            if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
                continue;
            }
            let at = y as usize * self.width + x as usize;
            if point.z - bias <= self.depth[at] {
                let alpha = colour[3];
                let pixel = &mut self.color[at];
                for channel in 0..3 {
                    pixel[channel] = pixel[channel] * (1.0 - alpha) + colour[channel] * alpha;
                }
            }
        }
    }
}

/// Everything a triangle's shading needs.
struct Shader<'a> {
    materials: &'a [Material],
    textures: &'a [Texture],
    options: Options,
    light: Vec3,
    eye: Vec3,
}

impl Shader<'_> {
    fn shade(
        &self,
        vertex: &ClipVertex,
        face_normal: Vec3,
        material: Option<usize>,
        flat: bool,
    ) -> Option<[f32; 4]> {
        let material = material.and_then(|index| self.materials.get(index));
        let mut colour = material.map_or(Vec4::new(0.8, 0.8, 0.8, 1.0), |material| {
            material.base_color
        });
        if self.options.vertex_colors {
            colour *= vertex.color;
        }
        if self.options.textures {
            if let Some(texture) = material
                .and_then(|material| material.texture)
                .and_then(|index| self.textures.get(index))
            {
                colour *= texture.sample(vertex.uv);
            }
        }
        // Cut-out transparency: mostly transparent texels are holes.
        if colour.w < 0.5 {
            return None;
        }
        let normal = if flat
            || self.options.shading == Shading::Flat
            || vertex.normal.length_squared() < 1e-12
        {
            face_normal
        } else {
            vertex.normal.normalize()
        };
        // Light the side facing the camera.
        let to_eye = (self.eye - vertex.world).normalize_or_zero();
        let normal = if normal.dot(to_eye) < 0.0 {
            -normal
        } else {
            normal
        };
        let rgb = match self.options.shading {
            Shading::Normals => normal * 0.5 + Vec3::splat(0.5),
            Shading::Unlit => colour.xyz(),
            Shading::Smooth | Shading::Flat | Shading::Hidden => {
                let key = normal.dot(self.light).max(0.0);
                let fill = normal.dot(to_eye).max(0.0);
                let half = (self.light + to_eye).normalize_or_zero();
                let specular = normal.dot(half).max(0.0).powf(32.0) * 0.15;
                let emissive = material.map_or(Vec3::ZERO, |material| material.emissive);
                colour.xyz() * (0.18 + 0.62 * key + 0.25 * fill) + Vec3::splat(specular) + emissive
            }
        };
        Some([rgb.x, rgb.y, rgb.z, 1.0])
    }
}

/// Renders the geometry into an RGBA8 picture of `width` × `height`.
pub fn render(
    geometry: &Geometry,
    materials: &[Material],
    textures: &[Texture],
    camera: &Camera,
    options: &Options,
    width: u32,
    height: u32,
) -> image::RgbaImage {
    let scale = options.supersample.clamp(1, 3) as usize;
    let (width, height) = (
        width.clamp(1, 8192) as usize,
        height.clamp(1, 8192) as usize,
    );
    let (big_width, big_height) = (width * scale, height * scale);
    let background = options.background.map(|channel| f32::from(channel) / 255.0);
    let mut target = Target {
        width: big_width,
        height: big_height,
        color: vec![background; big_width * big_height],
        depth: vec![f32::INFINITY; big_width * big_height],
    };
    let matrix = camera.matrix(big_width as f32 / big_height as f32);
    let eye = camera.eye();
    let shader = Shader {
        materials,
        textures,
        options: *options,
        light: (Vec3::new(0.4, 1.0, 0.6) + (eye - camera.target).normalize_or_zero() * 0.5)
            .normalize(),
        eye,
    };
    let bounds = geometry.bounds();
    if options.shading != Shading::Hidden {
        fill(&mut target, geometry, &matrix, &shader);
    }
    if options.ground {
        ground(&mut target, &matrix, bounds, camera);
    }
    let edge = |a: Vec3, b: Vec3, colour: [f32; 4], target: &mut Target| {
        target.line(matrix * a.extend(1.0), matrix * b.extend(1.0), colour, 2e-4);
    };
    if options.wireframe {
        let colour = if options.shading == Shading::Hidden {
            [0.85, 0.9, 1.0, 1.0]
        } else {
            [0.05, 0.05, 0.08, 0.6]
        };
        for triangle in &geometry.triangles {
            let [a, b, c] = triangle
                .vertices
                .map(|index| geometry.positions[index as usize]);
            edge(a, b, colour, &mut target);
            edge(b, c, colour, &mut target);
            edge(c, a, colour, &mut target);
        }
    }
    for line in &geometry.lines {
        let [a, b] = line.map(|index| geometry.positions[index as usize]);
        edge(a, b, [0.9, 0.9, 0.9, 1.0], &mut target);
    }
    if options.bounds {
        if let Some((low, high)) = bounds {
            let corner = |i: usize| {
                Vec3::new(
                    if i & 1 == 0 { low.x } else { high.x },
                    if i & 2 == 0 { low.y } else { high.y },
                    if i & 4 == 0 { low.z } else { high.z },
                )
            };
            for (a, b) in [
                (0, 1),
                (2, 3),
                (4, 5),
                (6, 7),
                (0, 2),
                (1, 3),
                (4, 6),
                (5, 7),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ] {
                edge(corner(a), corner(b), [1.0, 0.8, 0.2, 1.0], &mut target);
            }
        }
    }
    if options.axes {
        let size = bounds.map_or(1.0, |(low, high)| {
            (high - low).max_element().max(1e-3) * 0.6
        });
        for (axis, colour) in [
            (Vec3::X, [0.95, 0.3, 0.3, 1.0]),
            (Vec3::Y, [0.3, 0.9, 0.3, 1.0]),
            (Vec3::Z, [0.35, 0.5, 1.0, 1.0]),
        ] {
            edge(Vec3::ZERO, axis * size, colour, &mut target);
        }
    }
    resolve(&target, width, height, scale)
}

/// Rows per parallel band of the rasterizer.
const BAND: usize = 32;

/// A triangle ready to rasterize.
struct Prepared {
    vertices: [ClipVertex; 3],
    face: Vec3,
    material: Option<usize>,
    flat: bool,
    double_sided: bool,
}

/// Fills the triangles, rows in parallel bands.
#[allow(clippy::too_many_lines)]
fn fill(target: &mut Target, geometry: &Geometry, matrix: &Mat4, shader: &Shader<'_>) {
    let clip: Vec<Vec4> = geometry
        .positions
        .par_iter()
        .map(|position| *matrix * position.extend(1.0))
        .collect();
    // Screen-space triangles after near clipping, with their face normal.
    let mut prepared: Vec<Prepared> = Vec::new();
    for triangle in &geometry.triangles {
        let [ia, ib, ic] = triangle.vertices.map(|index| index as usize);
        let vertex = |index: usize| ClipVertex {
            clip: clip[index],
            normal: geometry.normals[index],
            uv: geometry.uvs[index],
            color: geometry.colors[index],
            world: geometry.positions[index],
        };
        let (a, b, c) = (vertex(ia), vertex(ib), vertex(ic));
        let face = (b.world - a.world)
            .cross(c.world - a.world)
            .normalize_or_zero();
        let material = triangle.material;
        let double_sided = material
            .and_then(|index| shader.materials.get(index))
            .is_none_or(|material| material.double_sided);
        let polygon = clip_near(&[a, b, c]);
        for i in 1..polygon.len().saturating_sub(1) {
            prepared.push(Prepared {
                vertices: [polygon[0], polygon[i], polygon[i + 1]],
                face,
                material,
                flat: triangle.flat,
                double_sided,
            });
        }
    }
    let width = target.width;
    let height = target.height;
    let to_screen = |clip: Vec4| {
        let ndc = clip.xyz() / clip.w;
        Vec3::new(
            f32::midpoint(ndc.x, 1.0) * width as f32,
            (1.0 - ndc.y) / 2.0 * height as f32,
            ndc.z,
        )
    };
    let screens: Vec<[Vec3; 3]> = prepared
        .iter()
        .map(|triangle| triangle.vertices.map(|v| to_screen(v.clip)))
        .collect();
    let culling = shader.options.backface_culling;
    target
        .color
        .par_chunks_mut(BAND * width)
        .zip(target.depth.par_chunks_mut(BAND * width))
        .enumerate()
        .for_each(|(band, (colors, depths))| {
            let top = band * BAND;
            let bottom = (top + colors.len() / width).min(height);
            for (index, triangle) in prepared.iter().enumerate() {
                let vertices = &triangle.vertices;
                let [s0, s1, s2] = screens[index];
                if !(s0.is_finite() && s1.is_finite() && s2.is_finite()) {
                    continue;
                }
                let area = (s1.x - s0.x) * (s2.y - s0.y) - (s1.y - s0.y) * (s2.x - s0.x);
                if area.abs() < 1e-12 {
                    continue;
                }
                // Counter-clockwise in the file is clockwise on this
                // y-down screen: negative area faces the camera.
                if culling && !triangle.double_sided && area > 0.0 {
                    continue;
                }
                let min_y = s0.y.min(s1.y).min(s2.y).floor().max(top as f32) as usize;
                let max_y = (s0.y.max(s1.y).max(s2.y).ceil().min(bottom as f32)).max(0.0) as usize;
                if min_y >= max_y {
                    continue;
                }
                let min_x = s0.x.min(s1.x).min(s2.x).floor().max(0.0) as usize;
                let max_x = (s0.x.max(s1.x).max(s2.x).ceil().min(width as f32)).max(0.0) as usize;
                let inverse_w = vertices.map(|vertex| 1.0 / vertex.clip.w);
                for y in min_y..max_y {
                    let py = y as f32 + 0.5;
                    for x in min_x..max_x {
                        let px = x as f32 + 0.5;
                        let w0 = ((s1.x - px) * (s2.y - py) - (s1.y - py) * (s2.x - px)) / area;
                        let w1 = ((s2.x - px) * (s0.y - py) - (s2.y - py) * (s0.x - px)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < EDGE || w1 < EDGE || w2 < EDGE {
                            continue;
                        }
                        let depth = w0 * s0.z + w1 * s1.z + w2 * s2.z;
                        let at = (y - top) * width + x;
                        if depth >= depths[at] || depth < 0.0 {
                            continue;
                        }
                        // Perspective-correct attributes.
                        let weights = [w0 * inverse_w[0], w1 * inverse_w[1], w2 * inverse_w[2]];
                        let sum = weights[0] + weights[1] + weights[2];
                        let blend = |pick: &dyn Fn(&ClipVertex) -> Vec4| {
                            (pick(&vertices[0]) * weights[0]
                                + pick(&vertices[1]) * weights[1]
                                + pick(&vertices[2]) * weights[2])
                                / sum
                        };
                        let fragment = ClipVertex {
                            clip: Vec4::ZERO,
                            normal: blend(&|v| v.normal.extend(0.0)).xyz(),
                            uv: blend(&|v| v.uv.extend(0.0).extend(0.0)).xy(),
                            color: blend(&|v| v.color),
                            world: blend(&|v| v.world.extend(0.0)).xyz(),
                        };
                        if let Some(colour) =
                            shader.shade(&fragment, triangle.face, triangle.material, triangle.flat)
                        {
                            depths[at] = depth;
                            colors[at] = colour;
                        }
                    }
                }
            }
        });
}

/// The ground: a translucent plane at y = 0 under the model with a grid.
fn ground(target: &mut Target, matrix: &Mat4, bounds: Option<(Vec3, Vec3)>, camera: &Camera) {
    let (low, high) = bounds.unwrap_or((Vec3::splat(-1.0), Vec3::splat(1.0)));
    let span = (high - low).max_element().max(1e-3);
    // A step of 1, 2 or 5 times a power of ten, about a tenth of the model.
    let raw = span / 10.0;
    let power = 10f32.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * power)
        .find(|&step| step >= raw)
        .unwrap_or(power * 10.0);
    let centre = Vec2::new(camera.target.x, camera.target.z);
    let half = (span * 1.5 / step).ceil() * step;
    let (x0, x1) = (
        (centre.x / step).round() * step - half,
        (centre.x / step).round() * step + half,
    );
    let (z0, z1) = (
        (centre.y / step).round() * step - half,
        (centre.y / step).round() * step + half,
    );
    // The plane: blended over everything behind it, without writing depth.
    let corners = [
        Vec3::new(x0, 0.0, z0),
        Vec3::new(x1, 0.0, z0),
        Vec3::new(x1, 0.0, z1),
        Vec3::new(x0, 0.0, z1),
    ];
    let plane: Vec<ClipVertex> = corners
        .iter()
        .map(|corner| ClipVertex {
            clip: *matrix * corner.extend(1.0),
            normal: Vec3::Y,
            uv: Vec2::ZERO,
            color: Vec4::ZERO,
            world: *corner,
        })
        .collect();
    let polygon = clip_near(&plane);
    let tint = [0.55, 0.62, 0.75, 0.18];
    for i in 1..polygon.len().saturating_sub(1) {
        let screens =
            [polygon[0], polygon[i], polygon[i + 1]].map(|vertex| target.to_screen(vertex.clip));
        blend_triangle(target, screens, tint);
    }
    let lines = ((x1 - x0) / step).round() as i32;
    for i in 0..=lines {
        let offset = i as f32 * step;
        let major = ((x0 + offset) / step).round() as i64 % 5 == 0;
        let colour = if major {
            [0.75, 0.78, 0.85, 0.55]
        } else {
            [0.6, 0.63, 0.7, 0.3]
        };
        target.line(
            *matrix * Vec4::new(x0 + offset, 0.0, z0, 1.0),
            *matrix * Vec4::new(x0 + offset, 0.0, z1, 1.0),
            colour,
            1e-4,
        );
        let major = ((z0 + offset) / step).round() as i64 % 5 == 0;
        let colour = if major {
            [0.75, 0.78, 0.85, 0.55]
        } else {
            [0.6, 0.63, 0.7, 0.3]
        };
        target.line(
            *matrix * Vec4::new(x0, 0.0, z0 + offset, 1.0),
            *matrix * Vec4::new(x1, 0.0, z0 + offset, 1.0),
            colour,
            1e-4,
        );
    }
}

/// Blends a flat colour over a screen triangle where it is in front.
fn blend_triangle(target: &mut Target, [s0, s1, s2]: [Vec3; 3], colour: [f32; 4]) {
    let area = (s1.x - s0.x) * (s2.y - s0.y) - (s1.y - s0.y) * (s2.x - s0.x);
    if area.abs() < 1e-12 || !(s0.is_finite() && s1.is_finite() && s2.is_finite()) {
        return;
    }
    let min_x = s0.x.min(s1.x).min(s2.x).floor().max(0.0) as usize;
    let max_x = (s0.x.max(s1.x).max(s2.x).ceil().min(target.width as f32)).max(0.0) as usize;
    let min_y = s0.y.min(s1.y).min(s2.y).floor().max(0.0) as usize;
    let max_y = (s0.y.max(s1.y).max(s2.y).ceil().min(target.height as f32)).max(0.0) as usize;
    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let w0 = ((s1.x - px) * (s2.y - py) - (s1.y - py) * (s2.x - px)) / area;
            let w1 = ((s2.x - px) * (s0.y - py) - (s2.y - py) * (s0.x - px)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < EDGE || w1 < EDGE || w2 < EDGE {
                continue;
            }
            let depth = w0 * s0.z + w1 * s1.z + w2 * s2.z;
            let at = y * target.width + x;
            if depth < target.depth[at] {
                let pixel = &mut target.color[at];
                for channel in 0..3 {
                    pixel[channel] =
                        pixel[channel] * (1.0 - colour[3]) + colour[channel] * colour[3];
                }
            }
        }
    }
}

/// Averages the supersampled frame down to the output size.
fn resolve(target: &Target, width: usize, height: usize, scale: usize) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(width as u32, height as u32);
    let samples = (scale * scale) as f32;
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let mut sum = [0.0f32; 4];
        for dy in 0..scale {
            for dx in 0..scale {
                let source = target.color
                    [(y as usize * scale + dy) * target.width + x as usize * scale + dx];
                for (total, value) in sum.iter_mut().zip(source) {
                    *total += value;
                }
            }
        }
        *pixel =
            image::Rgba(sum.map(|value| (value / samples * 255.0).clamp(0.0, 255.0).round() as u8));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model3d::scene::Triangle;

    fn quad() -> Geometry {
        Geometry {
            positions: vec![
                Vec3::new(-1.0, 0.0, -1.0),
                Vec3::new(1.0, 0.0, -1.0),
                Vec3::new(1.0, 0.0, 1.0),
                Vec3::new(-1.0, 0.0, 1.0),
            ]
            .into_iter()
            .map(|p| p + Vec3::Y)
            .collect(),
            normals: vec![Vec3::Y; 4],
            uvs: vec![Vec2::ZERO; 4],
            colors: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 4],
            triangles: vec![
                Triangle {
                    vertices: [0, 2, 1],
                    material: None,
                    flat: false,
                },
                Triangle {
                    vertices: [0, 3, 2],
                    material: None,
                    flat: false,
                },
            ],
            lines: Vec::new(),
        }
    }

    #[test]
    fn draws_the_model_over_the_background() {
        let geometry = quad();
        let mut camera = Camera::framing(geometry.bounds());
        camera.look(View::Top);
        let options = Options {
            ground: false,
            supersample: 1,
            ..Options::default()
        };
        let picture = render(&geometry, &[], &[], &camera, &options, 64, 64);
        let centre = picture.get_pixel(32, 32).0;
        assert!(centre[0] > centre[1] + 50, "{centre:?}");
        assert_eq!(picture.get_pixel(0, 0).0, options.background);
    }

    #[test]
    fn the_ground_and_wireframe_change_the_picture() {
        let geometry = quad();
        let camera = Camera::framing(geometry.bounds());
        let plain = render(
            &geometry,
            &[],
            &[],
            &camera,
            &Options {
                ground: false,
                ..Options::default()
            },
            48,
            48,
        );
        let grounded = render(&geometry, &[], &[], &camera, &Options::default(), 48, 48);
        assert_ne!(plain, grounded);
        let wire = render(
            &geometry,
            &[],
            &[],
            &camera,
            &Options {
                shading: Shading::Hidden,
                wireframe: true,
                ground: false,
                ..Options::default()
            },
            48,
            48,
        );
        assert!(wire.pixels().any(|pixel| pixel.0[2] > 200));
    }

    #[test]
    fn geometry_behind_the_camera_is_clipped_safely() {
        let geometry = quad();
        let camera = Camera {
            target: Vec3::new(0.0, 1.0, 0.0),
            distance: 0.5,
            pitch: -0.2,
            ..Camera::default()
        };
        let picture = render(&geometry, &[], &[], &camera, &Options::default(), 32, 32);
        assert_eq!(picture.dimensions(), (32, 32));
    }

    #[test]
    fn the_camera_orbits_pans_and_zooms() {
        let mut camera = Camera::framing(Some((Vec3::ZERO, Vec3::ONE)));
        let start = camera.eye();
        camera.orbit(Vec2::new(50.0, 0.0));
        assert!((camera.eye() - camera.target).length() - camera.distance < 1e-4);
        assert_ne!(camera.eye(), start);
        let distance = camera.distance;
        camera.zoom(2.0);
        assert!((camera.distance - distance / 2.0).abs() < 1e-5);
        let target = camera.target;
        camera.pan(Vec2::new(10.0, 0.0), 100.0);
        assert_ne!(camera.target, target);
        camera.look(View::Top);
        assert!(camera.eye().y > camera.target.y);
    }
}
