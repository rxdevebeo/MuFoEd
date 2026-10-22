//! Restricted RGB8 PSD interchange and lossless `MuFoEd` project metadata.
use super::{
    byte_len, invalid, Document, EditError, Layer, LayerBlendMode, LayerMask, Raster,
    MAX_DOCUMENT_BYTES, MAX_PROJECT_BYTES,
};
use ag_psd::psd::{
    BlendMode, ColorMode, ImageResources, Layer as PsdLayer, LayerAdditionalInfo, LayerMaskData,
    PixelData, ProtectedInfo, Psd, ReadOptions, WriteOptions,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

const RESOURCE_NAME: &[u8] = b"MuFoEd.Project";
const RESOURCE_ID: u16 = 4000; // Adobe's plug-in resource range; Pascal name disambiguates ownership.
#[derive(Serialize, Deserialize)]
struct Metadata {
    version: u32,
    signature: String,
    active: usize,
    mask: Vec<u8>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "super::workbench_selection::serialize_area",
        deserialize_with = "super::workbench_selection::deserialize_area"
    )]
    selection: Vec<u8>,
    recipes: Vec<Option<crate::generation::Request>>,
}
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], EditError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or_else(|| invalid("PSD section overflow"))?;
        let data = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| invalid("Truncated PSD section"))?;
        self.at = end;
        Ok(data)
    }
    fn u16(&mut self) -> Result<u16, EditError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32, EditError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn section(&mut self) -> Result<&'a [u8], EditError> {
        let len = usize::try_from(self.u32()?).map_err(|_| invalid("PSD section too large"))?;
        self.take(len)
    }
}
fn signature(doc: &Document) -> String {
    let mut hash = Sha256::new();
    hash.update(doc.width.to_le_bytes());
    hash.update(doc.height.to_le_bytes());
    for layer in &doc.layers {
        hash.update((layer.name.len() as u64).to_le_bytes());
        hash.update(layer.name.as_bytes());
        hash.update([
            u8::from(layer.visible),
            u8::from(layer.locked),
            layer.opacity,
        ]);
        hash.update(&layer.raster.pixels);
        if layer.blend_mode != LayerBlendMode::Normal {
            hash.update(b"MuFoEd.LayerBlend.v1");
            hash.update(layer.blend_mode.psd_key());
        }
        if let Some(mask) = &layer.mask {
            hash.update(b"MuFoEd.LayerMask.v1");
            hash.update([u8::from(mask.enabled), mask.default_color]);
            hash.update(&mask.pixels);
        }
    }
    format!("{hash:x}", hash = hash.finalize())
}
/// Locate the image resources after checking the header before bitmap allocation.
fn sections(bytes: &[u8]) -> Result<(usize, &[u8], &[u8]), EditError> {
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(invalid("PSD exceeds 128 MiB file limit"));
    }
    let mut c = Cursor { bytes, at: 0 };
    if c.take(4)? != b"8BPS" || c.u16()? != 1 {
        return Err(invalid(
            "Only PSD version 1 is supported (PSB is not supported)",
        ));
    }
    if c.take(6)? != [0; 6] {
        return Err(invalid("Invalid PSD reserved header bytes"));
    }
    let channels = c.u16()?;
    let height = c.u32()?;
    let width = c.u32()?;
    byte_len(width, height)?;
    if width > 30000 || height > 30000 {
        return Err(invalid("PSD dimensions exceed 30000 per axis"));
    }
    if !(3..=4).contains(&channels) || c.u16()? != 8 || c.u16()? != 3 {
        return Err(invalid(
            "PSD import supports RGB 8-bit with RGB/alpha channels only",
        ));
    }
    c.section()?;
    let offset = c.at;
    let resources = c.section()?;
    let layers = c.section()?;
    if !layers.is_empty() {
        let mut l = Cursor {
            bytes: layers,
            at: 0,
        };
        let info = l.section()?;
        if !info.is_empty() {
            let mut i = Cursor { bytes: info, at: 0 };
            let count =
                i16::from_be_bytes(i.take(2)?.try_into().expect("two bytes")).unsigned_abs();
            if count > 16
                || byte_len(width, height)?.saturating_mul(usize::from(count))
                    + byte_len(width, height)? / 4
                    > MAX_DOCUMENT_BYTES
            {
                return Err(invalid(
                    "PSD exceeds the editor's 16-layer / 128 MiB document budget",
                ));
            }
            let masks = preflight_layers(&mut i, count)?;
            if byte_len(width, height)? * usize::from(count)
                + byte_len(width, height)? / 4 * (1 + masks)
                > MAX_DOCUMENT_BYTES
            {
                return Err(invalid("PSD layer masks exceed document memory budget"));
            }
        }
        if l.at < layers.len() && !l.section()?.is_empty() {
            return Err(invalid("PSD global layer masks are not supported"));
        }
    }
    Ok((offset, resources, layers))
}
fn preflight_layers(cursor: &mut Cursor<'_>, count: u16) -> Result<usize, EditError> {
    let mut masks = 0;
    for _ in 0..count {
        cursor.take(16)?;
        let channels = cursor.u16()?;
        if channels > 5 {
            return Err(invalid("Unsupported PSD layer channels"));
        }
        let mut ids = Vec::new();
        for _ in 0..channels {
            let id = i16::from_be_bytes(cursor.take(2)?.try_into().expect("channel id"));
            if ![-2, -1, 0, 1, 2].contains(&id) || ids.contains(&id) {
                return Err(invalid("Unsupported or duplicate PSD channel"));
            }
            ids.push(id);
            cursor.u32()?;
        }
        if cursor.take(4)? != b"8BIM" {
            return Err(invalid("Invalid PSD blend signature"));
        }
        let mode = cursor.take(4)?;
        if ![b"norm".as_slice(), b"mul ", b"scrn", b"over"].contains(&mode) {
            return Err(invalid("Unsupported PSD blend mode"));
        }
        cursor.take(4)?;
        let extra = cursor.section()?;
        let mut extra = Cursor {
            bytes: extra,
            at: 0,
        };
        let mask = extra.section()?;
        if !mask.is_empty() {
            if mask.len() != 20 || mask[17] & !3 != 0 || ![0, 255].contains(&mask[16]) {
                return Err(invalid(
                    "Unsupported PSD mask flags, feather/density, real mask or default color",
                ));
            }
            masks += 1;
        } else if ids.contains(&-2) {
            return Err(invalid("PSD mask channel without mask record"));
        }
    }
    Ok(masks)
}
fn metadata(resources: &[u8]) -> Result<Option<Metadata>, EditError> {
    let mut c = Cursor {
        bytes: resources,
        at: 0,
    };
    let mut result = None;
    while c.at < resources.len() {
        if c.take(4)? != b"8BIM" {
            return Err(invalid("Unsupported PSD resource signature"));
        }
        let id = c.u16()?;
        let name_len = usize::from(c.take(1)?[0]);
        let name = c.take(name_len)?;
        if (name_len + 1) % 2 != 0 {
            c.take(1)?;
        }
        let payload = c.section()?;
        if payload.len() % 2 != 0 {
            c.take(1)?;
        }
        // Arbitrary ICC transforms are not implemented in the sRGB editor.
        if id == 1039 {
            return Err(invalid("PSD with an embedded ICC profile is not supported yet; convert/export as untagged sRGB RGB8"));
        }
        if id == RESOURCE_ID && name == RESOURCE_NAME {
            if result.is_some() {
                return Err(invalid("Duplicate MuFoEd PSD metadata"));
            }
            let mut json = Vec::new();
            flate2::read::ZlibDecoder::new(payload)
                .take(MAX_PROJECT_BYTES + 1)
                .read_to_end(&mut json)?;
            if json.len() as u64 > MAX_PROJECT_BYTES {
                return Err(invalid("PSD project metadata exceeds limit"));
            }
            result = Some(serde_json::from_slice(&json)?);
        }
    }
    Ok(result)
}
// Enforce the decoder's *uncompressed* bound while streaming, before any
// destination file is replaced; a tiny compressed payload can still exceed it.
struct MetadataWriter<W> {
    inner: W,
    remaining: u64,
}
impl<W: Write> Write for MetadataWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if u64::try_from(bytes.len()).expect("slice length") > self.remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "PSD project metadata exceeds read limit",
            ));
        }
        let count = self.inner.write(bytes)?;
        self.remaining -= u64::try_from(count).expect("written length");
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
fn inject_metadata(mut bytes: Vec<u8>, project: &Metadata) -> Result<Vec<u8>, EditError> {
    let (offset, resources, _) = sections(&bytes)?;
    let old_len = resources.len();
    let mut compressor =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut writer = MetadataWriter {
            inner: &mut compressor,
            remaining: MAX_PROJECT_BYTES,
        };
        serde_json::to_writer(&mut writer, project)?;
    }
    let payload = compressor.finish()?;
    let mut block = Vec::new();
    block.extend_from_slice(b"8BIM");
    block.extend_from_slice(&RESOURCE_ID.to_be_bytes());
    block.push(u8::try_from(RESOURCE_NAME.len()).expect("short Pascal name"));
    block.extend_from_slice(RESOURCE_NAME);
    if block.len() % 2 != 0 {
        block.push(0);
    }
    block.extend_from_slice(
        &u32::try_from(payload.len())
            .map_err(|_| invalid("PSD metadata too large"))?
            .to_be_bytes(),
    );
    block.extend_from_slice(&payload);
    if payload.len() % 2 != 0 {
        block.push(0);
    }
    let new_len =
        u32::try_from(old_len + block.len()).map_err(|_| invalid("PSD resources too large"))?;
    bytes.splice(offset + 4 + old_len..offset + 4 + old_len, block);
    bytes[offset..offset + 4].copy_from_slice(&new_len.to_be_bytes());
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(invalid("Serialized PSD exceeds 128 MiB file limit"));
    }
    Ok(bytes)
}
#[allow(clippy::float_cmp)] // PSD fill opacity must be exactly the identity, not approximately equal.
fn validate_layer(layer: &PsdLayer) -> Result<(), EditError> {
    let a = &layer.additional_info;
    let composite_lock = a
        .protected_info
        .as_ref()
        .is_some_and(|p| p.composite == Some(true));
    let partial_lock = a.protected_info.as_ref().is_some_and(|p| {
        (p.position == Some(true)) != composite_lock
            || (p.transparency == Some(true) && !composite_lock)
    }) || (layer.transparency_protected == Some(true) && !composite_lock);
    if partial_lock
        || layer.children.is_some()
        || layer.clipping == Some(true)
        || !matches!(
            layer.blend_mode,
            None | Some(
                BlendMode::Normal | BlendMode::Multiply | BlendMode::Screen | BlendMode::Overlay
            )
        )
        || a.real_mask.is_some()
        || a.filter_mask.is_some()
        || a.effects.is_some()
        || a.text.is_some()
        || a.vector_fill.is_some()
        || a.vector_stroke.is_some()
        || a.vector_mask.is_some()
        || a.adjustment.is_some()
        || a.placed_layer.is_some()
        || a.artboard.is_some()
        || a.knockout == Some(true)
        || a.fill_opacity.is_some_and(|v| v != 1.0)
        || a.filter_effects_masks.is_some()
        || a.user_mask.is_some()
        || a.blending_ranges.as_ref().is_some_and(|r| {
            let identity = |range: &[f64]| range == [0.0, 0.0, 255.0, 255.0];
            !identity(&r.composite_gray_blend_source)
                || !identity(&r.composite_graph_blend_destination_range)
                || r.ranges
                    .iter()
                    .any(|c| !identity(&c.source_range) || !identity(&c.dest_range))
        })
        || a.channel_blending_restrictions.is_some()
        || a.timeline.is_some()
        || a.animation_frames.is_some()
        || a.pixel_source.is_some()
        || a.patterns.is_some()
        || a.transparency_shapes_layer == Some(false)
    {
        return Err(invalid("Unsupported PSD layer: supported raster modes are Normal/Multiply/Screen/Overlay with plain bitmap masks; groups, vector masks, text, effects, smart objects and adjustments require rasterizing in the source editor"));
    }
    Ok(())
}
fn coordinate(value: Option<f64>) -> Result<i64, EditError> {
    let v = value.unwrap_or(0.0);
    if !v.is_finite() || v.fract() != 0.0 || v.abs() > 30000.0 {
        return Err(invalid("Invalid PSD layer bounds"));
    }
    #[allow(clippy::cast_possible_truncation)] // Finite integral value, bounded above.
    Ok(v as i64)
}
fn import_layer(mut layer: PsdLayer, width: u32, height: u32) -> Result<Layer, EditError> {
    validate_layer(&layer)?;
    let x = coordinate(layer.left)?;
    let y = coordinate(layer.top)?;
    let right = coordinate(layer.right)?;
    let bottom = coordinate(layer.bottom)?;
    // Keep all source pixels: the current editor cannot retain off-canvas extents.
    if x < 0
        || y < 0
        || right < x
        || bottom < y
        || right > i64::from(width)
        || bottom > i64::from(height)
    {
        return Err(invalid(
            "PSD layer extends outside the canvas; off-canvas pixels cannot be preserved yet",
        ));
    }
    let mut raster = Raster::solid(width, height, [0; 4])?;
    if let Some(pixels) = layer.image_data.or(layer.canvas) {
        if i64::from(pixels.width) != right - x
            || i64::from(pixels.height) != bottom - y
            || pixels.data.len()
                != usize::try_from(u64::from(pixels.width) * u64::from(pixels.height) * 4)
                    .map_err(|_| invalid("PSD layer too large"))?
        {
            return Err(invalid("PSD pixel buffer does not match layer bounds"));
        }
        let x = usize::try_from(x).expect("nonnegative bounds");
        let y = usize::try_from(y).expect("nonnegative bounds");
        let row_bytes = pixels.width as usize * 4;
        for row in 0..pixels.height as usize {
            let start = ((y + row) * width as usize + x) * 4;
            raster.pixels[start..start + row_bytes]
                .copy_from_slice(&pixels.data[row * row_bytes..(row + 1) * row_bytes]);
        }
    } else if right != x && bottom != y {
        return Err(invalid("PSD layer pixel data is missing"));
    }
    let mask = layer
        .additional_info
        .mask
        .take()
        .map(|m| import_mask(m, [x, y], width, height))
        .transpose()?;
    let blend_mode = match layer.blend_mode.unwrap_or(BlendMode::Normal) {
        BlendMode::Normal => LayerBlendMode::Normal,
        BlendMode::Multiply => LayerBlendMode::Multiply,
        BlendMode::Screen => LayerBlendMode::Screen,
        BlendMode::Overlay => LayerBlendMode::Overlay,
        _ => unreachable!("validated mode"),
    };
    let opacity = layer.opacity.unwrap_or(1.0);
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(invalid("Invalid PSD opacity"));
    }
    let locked = layer
        .additional_info
        .protected_info
        .as_ref()
        .is_some_and(|p| p.composite == Some(true) || p.position == Some(true));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Normalized byte opacity.
    let opacity = (opacity * 255.0).round() as u8;
    Ok(Layer {
        name: layer.additional_info.name.unwrap_or_else(|| "Layer".into()),
        visible: layer.hidden != Some(true),
        locked,
        opacity,
        raster,
        recipe: None,
        blend_mode,
        mask,
    })
}
fn import_mask(
    mut mask: LayerMaskData,
    layer_origin: [i64; 2],
    width: u32,
    height: u32,
) -> Result<LayerMask, EditError> {
    let default = match mask.default_color.unwrap_or(0.0) {
        0.0 => 0,
        255.0 => 255,
        _ => return Err(invalid("Invalid PSD mask default color")),
    };
    if mask.from_vector_data == Some(true)
        || mask.user_mask_density.is_some()
        || mask.user_mask_feather.is_some()
        || mask.vector_mask_density.is_some()
        || mask.vector_mask_feather.is_some()
    {
        return Err(invalid(
            "PSD mask feather/density/vector parameters are not supported",
        ));
    }
    let mut x = coordinate(mask.left)?;
    let mut y = coordinate(mask.top)?;
    let mut right = coordinate(mask.right)?;
    let mut bottom = coordinate(mask.bottom)?;
    if mask.position_relative_to_layer == Some(true) {
        x += layer_origin[0];
        right += layer_origin[0];
        y += layer_origin[1];
        bottom += layer_origin[1];
    }
    if x < 0
        || y < 0
        || right < x
        || bottom < y
        || right > i64::from(width)
        || bottom > i64::from(height)
    {
        return Err(invalid("PSD mask extends outside supported canvas bounds"));
    }
    let mut pixels = vec![default; width as usize * height as usize];
    if let Some(data) = mask.image_data.take().or(mask.canvas.take()) {
        if i64::from(data.width) != right - x
            || i64::from(data.height) != bottom - y
            || data.data.len() != byte_len(data.width, data.height)?
        {
            return Err(invalid("PSD mask pixel buffer does not match bounds"));
        }
        for row in 0..data.height as usize {
            for col in 0..data.width as usize {
                pixels[(usize::try_from(y).expect("validated mask top") + row) * width as usize
                    + usize::try_from(x).expect("validated mask left")
                    + col] = data.data[(row * data.width as usize + col) * 4];
            }
        }
    } else if right != x && bottom != y {
        return Err(invalid("PSD layer mask pixel data is missing"));
    }
    Ok(LayerMask {
        pixels,
        enabled: mask.disabled != Some(true),
        default_color: default,
    })
}
impl Document {
    /// Serialize RGB8 PSD layers, merged preview and private mask/recipe metadata.
    pub fn to_psd(&self) -> Result<Vec<u8>, EditError> {
        self.validate()?;
        if self.width > 30000 || self.height > 30000 {
            return Err(invalid("PSD dimensions are limited to 30000 per axis"));
        }
        let children = self
            .layers
            .iter()
            .map(|l| PsdLayer {
                top: Some(0.0),
                left: Some(0.0),
                right: Some(f64::from(self.width)),
                bottom: Some(f64::from(self.height)),
                opacity: Some(f64::from(l.opacity) / 255.0),
                hidden: Some(!l.visible),
                blend_mode: Some(match l.blend_mode {
                    LayerBlendMode::Normal => BlendMode::Normal,
                    LayerBlendMode::Multiply => BlendMode::Multiply,
                    LayerBlendMode::Screen => BlendMode::Screen,
                    LayerBlendMode::Overlay => BlendMode::Overlay,
                }),
                additional_info: LayerAdditionalInfo {
                    name: Some(l.name.clone()),
                    mask: l.mask.as_ref().map(|m| LayerMaskData {
                        top: Some(0.0),
                        left: Some(0.0),
                        right: Some(f64::from(self.width)),
                        bottom: Some(f64::from(self.height)),
                        default_color: Some(f64::from(m.default_color)),
                        disabled: Some(!m.enabled),
                        position_relative_to_layer: Some(false),
                        image_data: Some(PixelData {
                            width: self.width,
                            height: self.height,
                            data: m.pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
                        }),
                        ..Default::default()
                    }),
                    protected_info: Some(ProtectedInfo {
                        composite: Some(l.locked),
                        position: Some(l.locked),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                image_data: Some(PixelData {
                    width: self.width,
                    height: self.height,
                    data: l.raster.pixels.clone(),
                }),
                ..Default::default()
            })
            .collect();
        let image = self.flatten();
        let psd = Psd {
            width: f64::from(self.width),
            height: f64::from(self.height),
            bits_per_channel: Some(8.0),
            color_mode: Some(ColorMode::Rgb),
            children: Some(children),
            image_resources: Some(ImageResources {
                layer_state: Some(f64::from(
                    u16::try_from(self.active).expect("at most sixteen layers"),
                )),
                ..Default::default()
            }),
            image_data: Some(PixelData {
                width: self.width,
                height: self.height,
                data: image.pixels,
            }),
            ..Default::default()
        };
        let bytes = std::panic::catch_unwind(|| {
            ag_psd::write_psd(
                &psd,
                &WriteOptions {
                    no_background: Some(true),
                    compress: Some(true),
                    ..Default::default()
                },
            )
        })
        .map_err(|_| invalid("PSD encoder failed; destination has not been replaced"))?;
        inject_metadata(
            bytes,
            &Metadata {
                version: 1,
                signature: signature(self),
                active: self.active,
                mask: self.mask.clone(),
                selection: self.selection.clone(),
                recipes: self.layers.iter().map(|l| l.recipe.clone()).collect(),
            },
        )
    }
    /// Import the supported PSD subset. Unsupported features fail before replacing a document.
    pub fn from_psd(bytes: &[u8]) -> Result<Self, EditError> {
        let (_, resources, _) = sections(bytes)?;
        let project = metadata(resources)?;
        let psd = std::panic::catch_unwind(|| {
            ag_psd::read_psd(
                bytes,
                &ReadOptions {
                    total_memory_limit: Some(MAX_DOCUMENT_BYTES * 3),
                    use_image_data: Some(true),
                    skip_thumbnail: Some(true),
                    skip_linked_files_data: Some(true),
                    throw_for_missing_features: Some(true),
                    strict: Some(true),
                    ..Default::default()
                },
            )
        })
        .map_err(|_| invalid("Malformed PSD: decoder failed"))?
        .map_err(|e| invalid(&format!("PSD decoding failed: {e}")))?;
        let width = u32::try_from(coordinate(Some(psd.width))?)
            .map_err(|_| invalid("Invalid PSD width"))?;
        let height = u32::try_from(coordinate(Some(psd.height))?)
            .map_err(|_| invalid("Invalid PSD height"))?;
        let mut doc = if let Some(children) = psd.children.filter(|c| !c.is_empty()) {
            let layers = children
                .into_iter()
                .map(|l| import_layer(l, width, height))
                .collect::<Result<Vec<_>, _>>()?;
            Self {
                selection: Vec::new(),
                version: 1,
                width,
                height,
                mask: vec![0; byte_len(width, height)? / 4],
                layers,
                active: 0,
            }
        } else {
            let image = psd
                .image_data
                .or(psd.canvas)
                .ok_or_else(|| invalid("PSD has no layers or merged preview"))?;
            Self::from_raster(Raster::new(width, height, image.data)?)?
        };
        doc.sync_layer_version();
        if let Some(project) = project {
            if project.version != 1
                || project.signature != signature(&doc)
                || project.recipes.len() != doc.layers.len()
            {
                return Err(invalid("MuFoEd metadata no longer matches the PSD layers; refusing to restore a stale mask or recipe. Re-export a standard PSD without MuFoEd.Project metadata to import the edited layers"));
            }
            doc.active = project.active;
            doc.mask = project.mask;
            doc.selection = project.selection;
            for (layer, recipe) in doc.layers.iter_mut().zip(project.recipes) {
                layer.recipe = recipe;
            }
        }
        doc.validate()?;
        doc.normalize_selection();
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_or_stale_private_metadata_never_restores_wrong_mask() {
        let doc = Document::new(4, 3).unwrap();
        let bytes = doc.to_psd().unwrap();
        let (_, resources, _) = sections(&bytes).unwrap();
        let mut project = metadata(resources).unwrap().unwrap();
        project.signature = "stale".into();
        // The third-party codec drops private resources; add one deliberately stale block.
        let psd = ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap();
        let standard = ag_psd::write_psd(
            &psd,
            &WriteOptions {
                no_background: Some(true),
                ..Default::default()
            },
        );
        let bad = inject_metadata(standard, &project).unwrap();
        assert!(Document::from_psd(&bad)
            .unwrap_err()
            .to_string()
            .contains("stale mask"));
        assert!(
            Document::from_psd(&inject_metadata(bytes, &project).unwrap())
                .unwrap_err()
                .to_string()
                .contains("Duplicate")
        );
    }
    #[test]
    fn maximum_canvas_mask_and_selection_metadata_roundtrip_within_decoder_limit() {
        let bytes = Document::new(1, 1).unwrap().to_psd().unwrap();
        let psd = ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap();
        let standard = ag_psd::write_psd(
            &psd,
            &WriteOptions {
                no_background: Some(true),
                ..Default::default()
            },
        );
        let pixels = usize::try_from(super::super::MAX_PIXELS).unwrap();
        let project = Metadata {
            version: 1,
            signature: "test planes".into(),
            active: 0,
            mask: vec![255; pixels],
            selection: vec![255; pixels],
            recipes: vec![],
        };
        let bytes = inject_metadata(standard, &project).unwrap();
        let (_, resources, _) = sections(&bytes).unwrap();
        let restored = metadata(resources).unwrap().unwrap();
        assert!(restored.mask.len() == pixels && restored.mask.iter().all(|v| *v == 255));
        assert!(restored.selection.len() == pixels && restored.selection.iter().all(|v| *v == 255));
    }
    #[test]
    fn metadata_writer_enforces_uncompressed_bound_before_compression() {
        let mut writer = MetadataWriter {
            inner: Vec::new(),
            remaining: 3,
        };
        assert!(writer.write_all(b"four").is_err());
        assert!(writer.inner.is_empty());
        writer.write_all(b"one").unwrap();
        assert!(writer.write_all(b"!").is_err());
    }
}
