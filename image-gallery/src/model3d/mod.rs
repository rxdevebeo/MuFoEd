//! 3D models: loading, animation and software rendering.
//!
//! Formats: glTF 2.0 / GLB (with skins, morph targets and animations), FBX
//! (binary), COLLADA, OBJ with MTL, 3MF, STL, PLY and OFF. Every loader
//! produces a [`Scene`]; [`render`] draws a pose of it, which is how the
//! gallery makes previews and how its viewer shows models.

pub mod render;
pub mod scene;

mod fbx;
mod gltf_format;
mod mesh_formats;
mod xml_formats;

use std::path::Path;

pub use render::{Camera, Options, Shading, View};
pub use scene::{Scene, Stats, UpAxis};

use crate::Error;

/// The model file kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// glTF JSON or binary GLB.
    Gltf,
    /// Autodesk FBX (binary).
    Fbx,
    /// COLLADA.
    Collada,
    /// Wavefront OBJ.
    Obj,
    /// 3D Manufacturing Format.
    ThreeMf,
    /// Stereolithography.
    Stl,
    /// Polygon File Format.
    Ply,
    /// Object File Format.
    Off,
}

/// The extensions of model files.
pub const EXTENSIONS: &[&str] = &[
    "gltf", "glb", "fbx", "dae", "obj", "3mf", "stl", "ply", "off",
];

impl Kind {
    /// The kind of an extension.
    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "gltf" | "glb" => Self::Gltf,
            "fbx" => Self::Fbx,
            "dae" => Self::Collada,
            "obj" => Self::Obj,
            "3mf" => Self::ThreeMf,
            "stl" => Self::Stl,
            "ply" => Self::Ply,
            "off" => Self::Off,
            _ => return None,
        })
    }

    /// The kind the content shows, `hint` deciding when it does not.
    pub fn detect(data: &[u8], hint: Option<Self>) -> Option<Self> {
        let head = &data[..data.len().min(1024)];
        let text = String::from_utf8_lossy(head);
        let trimmed = text.trim_start();
        if head.starts_with(b"glTF") {
            Some(Self::Gltf)
        } else if fbx::is_fbx(head) {
            Some(Self::Fbx)
        } else if head.starts_with(b"ply") {
            Some(Self::Ply)
        } else if trimmed.starts_with("OFF") || trimmed.starts_with("COFF") {
            Some(Self::Off)
        } else if text.contains("<COLLADA") {
            Some(Self::Collada)
        } else {
            hint
        }
    }

    /// A short display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Gltf => "glTF",
            Self::Fbx => "FBX",
            Self::Collada => "COLLADA",
            Self::Obj => "OBJ",
            Self::ThreeMf => "3MF",
            Self::Stl => "STL",
            Self::Ply => "PLY",
            Self::Off => "OFF",
        }
    }
}

/// Reads a model file, with the files it refers to beside it (glTF buffers
/// and images, OBJ materials, textures).
pub fn load(path: &Path) -> Result<Scene, Error> {
    let hint = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(Kind::from_extension);
    let data = std::fs::read(path).map_err(|error| Error::io(path, error))?;
    let kind = Kind::detect(&data, hint).ok_or(Error::UnknownFormat)?;
    crate::error::contain(|| match kind {
        Kind::Gltf => gltf_format::load(path),
        Kind::Obj => mesh_formats::load_obj(path),
        Kind::Collada => xml_formats::load_dae(&data, path.parent()),
        Kind::Fbx => fbx::load(&data, path.parent()),
        _ => decode(&data, kind),
    })
}

/// Reads a self-contained model held in memory.
pub fn load_bytes(data: &[u8], hint: Option<Kind>) -> Result<Scene, Error> {
    let kind = Kind::detect(data, hint).ok_or(Error::UnknownFormat)?;
    crate::error::contain(|| decode(data, kind))
}

fn decode(data: &[u8], kind: Kind) -> Result<Scene, Error> {
    match kind {
        Kind::Gltf => gltf_format::load_bytes(data),
        Kind::Fbx => fbx::load(data, None),
        Kind::Collada => xml_formats::load_dae(data, None),
        Kind::ThreeMf => xml_formats::load_3mf(data),
        Kind::Stl => mesh_formats::load_stl(data),
        Kind::Ply => mesh_formats::load_ply(data),
        Kind::Off => mesh_formats::load_off(data),
        Kind::Obj => Err(Error::Codec("OBJ needs its file path for materials".into())),
    }
}

/// Renders the rest pose (or the first frame of the first animation when
/// the rest pose is empty) from the default angle, `side` pixels square.
pub fn preview(scene: &Scene, side: u32) -> image::RgbaImage {
    let mut pose = scene.pose(None, 0.0);
    let mut geometry = scene.geometry(&pose);
    if geometry.triangles.is_empty() && !scene.animations.is_empty() {
        pose = scene.pose(Some(0), 0.0);
        geometry = scene.geometry(&pose);
    }
    let camera = Camera::framing(geometry.bounds());
    let options = Options {
        supersample: 2,
        ..Options::default()
    };
    render::render(
        &geometry,
        &scene.materials,
        &scene.textures,
        &camera,
        &options,
        side,
        side,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_content_and_extension() {
        assert_eq!(Kind::detect(b"glTF\x02", None), Some(Kind::Gltf));
        assert_eq!(
            Kind::detect(b"ply\nformat ascii", Some(Kind::Stl)),
            Some(Kind::Ply)
        );
        assert_eq!(Kind::detect(b"<?xml?><COLLADA>", None), Some(Kind::Collada));
        assert_eq!(Kind::detect(b"solid x", Some(Kind::Stl)), Some(Kind::Stl));
        assert_eq!(Kind::detect(b"v 0 0 0", None), None);
        for extension in EXTENSIONS {
            assert!(Kind::from_extension(extension).is_some(), "{extension}");
        }
    }

    #[test]
    fn previews_show_something() {
        let scene = load_bytes(b"OFF\n3 1 0\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n", None).expect("loads");
        let picture = preview(&scene, 64);
        assert_eq!(picture.dimensions(), (64, 64));
        let background = Options::default().background;
        assert!(picture.pixels().any(|pixel| pixel.0 != background));
    }
}
