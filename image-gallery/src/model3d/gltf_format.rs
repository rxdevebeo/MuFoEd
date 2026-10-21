//! glTF 2.0 and GLB: the full model, skins, morph targets and animations.

use std::path::Path;

use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

use super::scene::{
    Animation, Channel, Interpolation, Material, Mesh, MorphTarget, Node, Primitive, Property,
    Scene, Skin, Texture, Transform,
};
use crate::Error;

fn codec(error: &gltf::Error) -> Error {
    Error::Codec(format!("glTF: {error}"))
}

/// Reads a `.gltf` (with its buffers and images beside it) or `.glb` file.
pub(crate) fn load(path: &Path) -> Result<Scene, Error> {
    match gltf::import(path) {
        Ok((document, buffers, images)) => Ok(convert(&document, &buffers, &images)),
        // An image the bundled decoders cannot read (WebP, KTX2...) fails the
        // whole import; the geometry is still worth showing.
        Err(gltf::Error::Image(_) | gltf::Error::UnsupportedImageEncoding) => {
            let gltf = gltf::Gltf::open(path).map_err(|error| codec(&error))?;
            let base = path.parent();
            let buffers = gltf::import_buffers(&gltf.document, base, gltf.blob.clone())
                .map_err(|error| codec(&error))?;
            Ok(convert(&gltf.document, &buffers, &[]))
        }
        Err(error) => Err(codec(&error)),
    }
}

/// Reads a self-contained glTF (GLB, or JSON with embedded data URIs).
pub(crate) fn load_bytes(data: &[u8]) -> Result<Scene, Error> {
    match gltf::import_slice(data) {
        Ok((document, buffers, images)) => Ok(convert(&document, &buffers, &images)),
        Err(gltf::Error::Image(_) | gltf::Error::UnsupportedImageEncoding) => {
            let gltf = gltf::Gltf::from_slice(data).map_err(|error| codec(&error))?;
            let buffers = gltf::import_buffers(&gltf.document, None, gltf.blob.clone())
                .map_err(|error| codec(&error))?;
            Ok(convert(&gltf.document, &buffers, &[]))
        }
        Err(error) => Err(codec(&error)),
    }
}

fn convert(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
) -> Scene {
    let buffer = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(|data| &data.0[..]);
    let mut scene = Scene {
        textures: images.iter().map(texture).collect(),
        ..Scene::default()
    };
    scene.materials = document
        .materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            Material {
                name: material.name().unwrap_or_default().to_owned(),
                base_color: Vec4::from_array(pbr.base_color_factor()),
                texture: pbr
                    .base_color_texture()
                    .map(|info| info.texture().source().index())
                    .filter(|&index| index < images.len()),
                double_sided: material.double_sided(),
                emissive: Vec3::from_array(material.emissive_factor()),
            }
        })
        .collect();
    scene.meshes = document
        .meshes()
        .map(|mesh| Mesh {
            name: mesh.name().unwrap_or_default().to_owned(),
            primitives: mesh
                .primitives()
                .filter_map(|primitive| convert_primitive(&primitive, &buffer))
                .collect(),
            weights: mesh.weights().map(<[f32]>::to_vec).unwrap_or_default(),
        })
        .collect();
    scene.skins = document
        .skins()
        .map(|skin| {
            let joints: Vec<usize> = skin.joints().map(|joint| joint.index()).collect();
            let mut inverse_bind: Vec<Mat4> = skin
                .reader(buffer)
                .read_inverse_bind_matrices()
                .map(|matrices| {
                    matrices
                        .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                        .collect()
                })
                .unwrap_or_default();
            inverse_bind.resize(joints.len(), Mat4::IDENTITY);
            Skin {
                joints,
                inverse_bind,
            }
        })
        .collect();
    scene.nodes = document
        .nodes()
        .map(|node| {
            let (translation, rotation, scale) = node.transform().decomposed();
            Node {
                name: node.name().unwrap_or_default().to_owned(),
                transform: Transform {
                    translation: Vec3::from_array(translation),
                    rotation: Quat::from_array(rotation).normalize(),
                    scale: Vec3::from_array(scale),
                },
                mesh: node.mesh().map(|mesh| mesh.index()),
                skin: node.skin().map(|skin| skin.index()),
                children: node.children().map(|child| child.index()).collect(),
            }
        })
        .collect();
    scene.roots = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .map(|default| default.nodes().map(|node| node.index()).collect())
        .unwrap_or_default();
    scene.animations = document
        .animations()
        .enumerate()
        .map(|(index, animation)| Animation {
            name: animation
                .name()
                .map_or_else(|| format!("#{}", index + 1), str::to_owned),
            channels: animation
                .channels()
                .filter_map(|channel| convert_channel(&channel, &buffer))
                .collect(),
            duration: 0.0,
        })
        .collect();
    scene.finish();
    scene
}

fn convert_primitive<'a, 's, F>(primitive: &'a gltf::Primitive<'a>, buffer: &F) -> Option<Primitive>
where
    F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
{
    let reader = primitive.reader(buffer.clone());
    let positions: Vec<Vec3> = reader.read_positions()?.map(Vec3::from_array).collect();
    let count = u32::try_from(positions.len()).ok()?;
    let indices: Vec<u32> = reader.read_indices().map_or_else(
        || (0..count).collect(),
        |indices| indices.into_u32().collect(),
    );
    let mut out = Primitive {
        normals: reader
            .read_normals()
            .map(|normals| normals.map(Vec3::from_array).collect())
            .unwrap_or_default(),
        uvs: reader
            .read_tex_coords(0)
            .map(|uvs| uvs.into_f32().map(Vec2::from_array).collect())
            .unwrap_or_default(),
        colors: reader
            .read_colors(0)
            .map(|colors| colors.into_rgba_f32().map(Vec4::from_array).collect())
            .unwrap_or_default(),
        joints: reader
            .read_joints(0)
            .map(|joints| joints.into_u16().collect())
            .unwrap_or_default(),
        weights: reader
            .read_weights(0)
            .map(|weights| weights.into_f32().collect())
            .unwrap_or_default(),
        targets: reader
            .read_morph_targets()
            .map(|(positions, normals, _)| MorphTarget {
                positions: positions
                    .map(|p| p.map(Vec3::from_array).collect())
                    .unwrap_or_default(),
                normals: normals
                    .map(|n| n.map(Vec3::from_array).collect())
                    .unwrap_or_default(),
            })
            .collect(),
        material: primitive.material().index(),
        positions,
        ..Primitive::default()
    };
    match primitive.mode() {
        gltf::mesh::Mode::Triangles => {
            out.triangles = indices
                .chunks_exact(3)
                .map(|t| [t[0], t[1], t[2]])
                .collect();
        }
        gltf::mesh::Mode::TriangleStrip => {
            out.triangles = indices
                .windows(3)
                .enumerate()
                .map(|(i, t)| {
                    if i % 2 == 0 {
                        [t[0], t[1], t[2]]
                    } else {
                        [t[1], t[0], t[2]]
                    }
                })
                .collect();
        }
        gltf::mesh::Mode::TriangleFan => {
            if let Some((&first, rest)) = indices.split_first() {
                out.triangles = rest
                    .windows(2)
                    .map(|pair| [first, pair[0], pair[1]])
                    .collect();
            }
        }
        gltf::mesh::Mode::Lines => {
            out.lines = indices.chunks_exact(2).map(|l| [l[0], l[1]]).collect();
        }
        gltf::mesh::Mode::LineStrip | gltf::mesh::Mode::LineLoop => {
            out.lines = indices.windows(2).map(|l| [l[0], l[1]]).collect();
            if primitive.mode() == gltf::mesh::Mode::LineLoop {
                if let (Some(&first), Some(&last)) = (indices.first(), indices.last()) {
                    out.lines.push([last, first]);
                }
            }
        }
        gltf::mesh::Mode::Points => {}
    }
    Some(out)
}

fn convert_channel<'a, 's, F>(channel: &gltf::animation::Channel<'a>, buffer: &F) -> Option<Channel>
where
    F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
{
    use gltf::animation::util::ReadOutputs;
    let reader = channel.reader(buffer.clone());
    let times: Vec<f32> = reader.read_inputs()?.collect();
    let (property, values): (Property, Vec<f32>) = match reader.read_outputs()? {
        ReadOutputs::Translations(values) => (Property::Translation, values.flatten().collect()),
        ReadOutputs::Rotations(values) => {
            (Property::Rotation, values.into_f32().flatten().collect())
        }
        ReadOutputs::Scales(values) => (Property::Scale, values.flatten().collect()),
        ReadOutputs::MorphTargetWeights(values) => (Property::Weights, values.into_f32().collect()),
    };
    let interpolation = match channel.sampler().interpolation() {
        gltf::animation::Interpolation::Step => Interpolation::Step,
        gltf::animation::Interpolation::Linear => Interpolation::Linear,
        gltf::animation::Interpolation::CubicSpline => Interpolation::Cubic,
    };
    Some(Channel {
        node: channel.target().node().index(),
        property,
        interpolation,
        times,
        values,
    })
}

/// An image of the document as RGBA8.
fn texture(image: &gltf::image::Data) -> Texture {
    use gltf::image::Format;
    let pixels = &image.pixels;
    let count = image.width as usize * image.height as usize;
    let rgba: Vec<u8> = match image.format {
        Format::R8 => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        Format::R8G8 => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        Format::R8G8B8 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        Format::R8G8B8A8 => pixels.clone(),
        // 16-bit samples are little-endian: keep the high byte.
        Format::R16 => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[1], p[1], p[1], 255])
            .collect(),
        Format::R16G16 => pixels
            .chunks_exact(4)
            .flat_map(|p| [p[1], p[1], p[1], p[3]])
            .collect(),
        Format::R16G16B16 => pixels
            .chunks_exact(6)
            .flat_map(|p| [p[1], p[3], p[5], 255])
            .collect(),
        Format::R16G16B16A16 => pixels
            .chunks_exact(8)
            .flat_map(|p| [p[1], p[3], p[5], p[7]])
            .collect(),
        Format::R32G32B32FLOAT | Format::R32G32B32A32FLOAT => {
            let channels = if image.format == Format::R32G32B32FLOAT {
                3
            } else {
                4
            };
            pixels
                .chunks_exact(4 * channels)
                .flat_map(|p| {
                    let float = |i: usize| {
                        f32::from_le_bytes([p[4 * i], p[4 * i + 1], p[4 * i + 2], p[4 * i + 3]])
                    };
                    let value = |i: usize| unit_byte(crate::psd::encode_srgb(float(i)));
                    let alpha = if channels == 4 {
                        unit_byte(float(3))
                    } else {
                        255
                    };
                    [value(0), value(1), value(2), alpha]
                })
                .collect()
        }
    };
    if rgba.len() == count * 4 {
        Texture {
            width: image.width,
            height: image.height,
            rgba,
        }
    } else {
        Texture::default()
    }
}

/// A 0..=1 value as a byte.
// Clamped to 0..=1 before scaling, so the cast cannot truncate.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn unit_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle GLB with an animated translation, assembled by hand.
    pub(crate) fn glb() -> Vec<u8> {
        let mut bin = Vec::new();
        for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 1.0] {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0f32, 0.0, 0.0, 0.0, 2.0, 0.0] {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        let json = r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "name": "tri"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "buffers": [{"byteLength": 68}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 8},
                {"buffer": 0, "byteOffset": 44, "byteLength": 24}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [1,1,0]},
                {"bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1]},
                {"bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3"}
            ],
            "animations": [{"name": "lift", "samplers": [{"input": 1, "output": 2}],
                "channels": [{"sampler": 0, "target": {"node": 0, "path": "translation"}}]}]
        }"#;
        let mut json = json.as_bytes().to_vec();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = b"glTF".to_vec();
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(total).expect("small").to_le_bytes());
        out.extend_from_slice(&u32::try_from(json.len()).expect("small").to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&u32::try_from(bin.len()).expect("small").to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn reads_a_glb_with_its_animation() {
        let scene = load_bytes(&glb()).expect("loads");
        assert_eq!(scene.stats().triangles, 1);
        assert_eq!(scene.animations.len(), 1);
        assert_eq!(scene.animations[0].name, "lift");
        assert!((scene.animations[0].duration - 1.0).abs() < 1e-6);
        let moved = scene.geometry(&scene.pose(Some(0), 0.5));
        assert!((moved.positions[0] - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
        assert!(load_bytes(b"glTF\x02\0\0\0garbage").is_err());
    }
}
