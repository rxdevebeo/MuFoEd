//! Explicit recovery storage for hosts. No original project path is ever written.
//! Binary planes in a versioned gzip envelope avoid JSON pixel-array expansion.
use super::{
    byte_len, invalid, Document, EditError, Layer, LayerBlendMode, LayerMask, Raster,
    MAX_DOCUMENT_BYTES,
};
use flate2::{read::MultiGzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
const MAGIC: &[u8; 8] = b"MUFREC01";
const MAX_METADATA: usize = 4 * 1024 * 1024;
const MAX_FILE: u64 = (MAX_DOCUMENT_BYTES + MAX_METADATA + 1024 * 1024) as u64;

/// Validated draft and informational origin; origin is never an output destination.
#[derive(Debug)]
pub struct Draft {
    /// Recovered pixels, layers, masks, selection and generation recipes.
    pub document: Document,
    /// User-selected input/save path, if known. Recovery never overwrites it.
    pub source: Option<PathBuf>,
    /// UTC seconds since the Unix epoch when the checkpoint was written.
    pub saved_at: u64,
}
#[derive(PartialEq, Serialize, Deserialize)]
struct Header {
    version: u32,
    width: u32,
    height: u32,
    active: usize,
    selection: bool,
    source: Option<PathBuf>,
    saved_at: u64,
    layers: Vec<LayerInfo>,
}
#[derive(PartialEq, Serialize, Deserialize)]
struct LayerInfo {
    name: String,
    visible: bool,
    opacity: u8,
    locked: bool,
    blend_mode: LayerBlendMode,
    mask: Option<(bool, u8)>,
    recipe: Option<crate::generation::Request>,
}
fn header(document: &Document, source: Option<&Path>, saved_at: u64) -> Header {
    Header {
        version: 1,
        width: document.width,
        height: document.height,
        active: document.active,
        selection: !document.selection.is_empty(),
        source: source.map(Path::to_path_buf),
        saved_at,
        layers: document
            .layers
            .iter()
            .map(|l| LayerInfo {
                name: l.name.clone(),
                visible: l.visible,
                opacity: l.opacity,
                locked: l.locked,
                blend_mode: l.blend_mode,
                mask: l.mask.as_ref().map(|m| (m.enabled, m.default_color)),
                recipe: l.recipe.clone(),
            })
            .collect(),
    }
}
struct MetadataBuffer(Vec<u8>);
impl Write for MetadataBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_METADATA - self.0.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Recovery metadata exceeds 4 MiB limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode_header(h: &Header) -> Result<Vec<u8>, EditError> {
    let mut buffer = MetadataBuffer(Vec::new());
    serde_json::to_writer(&mut buffer, h)?;
    let bytes = buffer.0;
    let decoded: Header = serde_json::from_slice(&bytes)?;
    if decoded != *h {
        return Err(invalid("Recovery metadata cannot roundtrip exactly"));
    }
    Ok(bytes)
}
fn planes(document: &Document) -> impl Iterator<Item = &[u8]> {
    std::iter::once(document.mask.as_slice())
        .chain((!document.selection.is_empty()).then_some(document.selection.as_slice()))
        .chain(document.layers.iter().flat_map(|l| {
            std::iter::once(l.raster.pixels.as_slice())
                .chain(l.mask.as_ref().map(|m| m.pixels.as_slice()))
        }))
}
/// Stable document/origin fingerprint; timestamp is excluded so idle drafts are not rewritten.
pub fn fingerprint(document: &Document, source: Option<&Path>) -> Result<[u8; 32], EditError> {
    document.validate()?;
    let mut hash = Sha256::new();
    hash.update(encode_header(&header(document, source, 0))?);
    for plane in planes(document) {
        hash.update(plane);
    }
    Ok(hash.finalize().into())
}
fn read_plane(r: &mut impl Read, len: usize) -> Result<Vec<u8>, EditError> {
    let mut bytes = vec![0; len];
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn decode(path: &Path) -> Result<Draft, EditError> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE {
        return Err(invalid("Recovery file exceeds size limit"));
    }
    let mut magic = [0; 8];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("Unknown recovery format"));
    }
    let mut reader = MultiGzDecoder::new(file).take(MAX_FILE);
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_METADATA {
        return Err(invalid("Recovery metadata exceeds 4 MiB limit"));
    }
    let h: Header = serde_json::from_slice(&read_plane(&mut reader, length)?)?;
    if h.version != 1 || h.layers.is_empty() || h.layers.len() > 16 || h.active >= h.layers.len() {
        return Err(invalid("Invalid recovery header"));
    }
    let rgba = byte_len(h.width, h.height)?;
    let mask_count = h.layers.iter().filter(|l| l.mask.is_some()).count();
    if h.layers
        .iter()
        .any(|l| l.mask.is_some_and(|(_, d)| ![0, 255].contains(&d)))
        || rgba * h.layers.len() + rgba / 4 * (1 + usize::from(h.selection) + mask_count)
            > MAX_DOCUMENT_BYTES
    {
        return Err(invalid("Recovery document exceeds layer/memory limits"));
    }
    let mask = read_plane(&mut reader, rgba / 4)?;
    let selection = if h.selection {
        read_plane(&mut reader, rgba / 4)?
    } else {
        Vec::new()
    };
    let mut layers = Vec::new();
    for layer in h.layers {
        let raster = Raster::new(h.width, h.height, read_plane(&mut reader, rgba)?)?;
        let mask = layer
            .mask
            .map(|(enabled, default_color)| {
                Ok::<_, EditError>(LayerMask {
                    enabled,
                    default_color,
                    pixels: read_plane(&mut reader, rgba / 4)?,
                })
            })
            .transpose()?;
        layers.push(Layer {
            name: layer.name,
            visible: layer.visible,
            opacity: layer.opacity,
            locked: layer.locked,
            blend_mode: layer.blend_mode,
            recipe: layer.recipe,
            raster,
            mask,
        });
    }
    // Read through the gzip CRC/trailer. Truncation and extra decompressed data fail.
    if reader.read(&mut [0])? != 0 {
        return Err(invalid("Trailing recovery payload"));
    }
    let mut document = Document {
        version: 1,
        width: h.width,
        height: h.height,
        active: h.active,
        mask,
        selection,
        layers,
    };
    document.sync_layer_version();
    document.validate()?;
    document.normalize_selection();
    Ok(Draft {
        document,
        source: h.source,
        saved_at: h.saved_at,
    })
}
struct Lease {
    lock: Option<File>,
    lock_path: PathBuf,
    snapshot: PathBuf,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.snapshot.exists() {
            self.lock.take();
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }
}
impl Lease {
    fn discard(&self) -> Result<(), EditError> {
        match std::fs::remove_file(&self.snapshot) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
/// One live host's exclusive recovery slot. Dropping it preserves the last checkpoint.
/// Call `clear` only after an explicit save/discard or successful replacement checkpoint.
pub struct Session {
    lease: Lease,
}
impl Session {
    /// Create a unique slot and hold an OS lock until this session is dropped.
    pub fn new(directory: impl AsRef<Path>) -> Result<Self, EditError> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory)?;
        for _ in 0..8 {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|e| invalid(&format!("Recovery identity: {e}")))?;
            let id = format!("{:032x}", u128::from_le_bytes(bytes));
            let lock_path = directory.join(format!("session-{id}.lock"));
            let lock = match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            };
            lock.lock()?;
            return Ok(Self {
                lease: Lease {
                    snapshot: directory.join(format!("session-{id}.snapshot")),
                    lock: Some(lock),
                    lock_path,
                },
            });
        }
        Err(invalid("Could not reserve a recovery session"))
    }
    /// Path of this slot, always inside the explicitly supplied recovery directory.
    pub fn path(&self) -> &Path {
        &self.lease.snapshot
    }
    /// Write a complete compressed snapshot, sync it, then atomically replace the old one.
    /// Serialization/write failures leave the previous checkpoint in place.
    pub fn checkpoint(&self, document: &Document, source: Option<&Path>) -> Result<(), EditError> {
        document.validate()?;
        let metadata = encode_header(&header(
            document,
            source,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        ))?;
        let mut temp =
            tempfile::NamedTempFile::new_in(self.path().parent().expect("session parent"))?;
        temp.write_all(MAGIC)?;
        let mut gzip = GzEncoder::new(temp, Compression::fast());
        gzip.write_all(
            &u32::try_from(metadata.len())
                .expect("bounded metadata")
                .to_le_bytes(),
        )?;
        gzip.write_all(&metadata)?;
        for plane in planes(document) {
            gzip.write_all(plane)?;
        }
        let temp = gzip.finish()?;
        temp.as_file().sync_all()?;
        if temp.as_file().metadata()?.len() > MAX_FILE {
            return Err(invalid("Recovery file exceeds size limit"));
        }
        temp.persist(self.path()).map_err(|e| e.error)?;
        Ok(())
    }
    /// Remove only this host's checkpoint; never touch any source project.
    pub fn clear(&self) -> Result<(), EditError> {
        self.lease.discard()
    }
}
/// An abandoned slot held exclusively while the host asks whether to restore it.
pub struct Candidate {
    lease: Lease,
}
impl Candidate {
    /// Recovery file selected by the catalog, never a user project path.
    pub fn path(&self) -> &Path {
        &self.lease.snapshot
    }
    /// Decode with format, memory, integrity and complete-payload checks.
    pub fn load(&self) -> Result<Draft, EditError> {
        decode(self.path())
    }
    /// Explicitly discard this one recovery copy.
    pub fn discard(&self) -> Result<(), EditError> {
        self.lease.discard()
    }
}
/// Find abandoned slots; live windows and slots already claimed by another host are skipped.
/// Invalid snapshots remain candidates so the user can discard or defer them.
pub fn discover(directory: impl AsRef<Path>) -> Result<Vec<Candidate>, EditError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "snapshot")
            && path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                s.strip_prefix("session-")
                    .is_some_and(|id| id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()))
            })
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut result = Vec::new();
    for snapshot in paths {
        let lock_path = snapshot.with_extension("lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        if lock.try_lock().is_ok() && snapshot.is_file() {
            result.push(Candidate {
                lease: Lease {
                    lock: Some(lock),
                    lock_path,
                    snapshot,
                },
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write_payload(path: &Path, bytes: &[u8]) {
        let mut file = File::create(path).unwrap();
        file.write_all(MAGIC).unwrap();
        let mut gzip = GzEncoder::new(file, Compression::fast());
        gzip.write_all(bytes).unwrap();
        gzip.finish().unwrap();
    }
    #[test]
    fn forged_header_dimensions_counts_and_metadata_are_rejected_before_plane_allocation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("forged");
        let doc = Document::new(1, 1).unwrap();
        for case in 0..6 {
            let mut h = header(&doc, None, 0);
            match case {
                0 => h.width = u32::MAX,
                1 => h.layers.clear(),
                2 => h.active = 17,
                3 => h.layers[0].mask = Some((true, 42)),
                4 => {
                    h.width = 4096;
                    h.height = 4096;
                    h.layers.push(header(&doc, None, 0).layers.remove(0));
                }
                _ => h.version = 200,
            }
            let bytes = serde_json::to_vec(&h).unwrap();
            let payload = [
                u32::try_from(bytes.len()).unwrap().to_le_bytes().as_slice(),
                &bytes,
            ]
            .concat();
            write_payload(&path, &payload);
            let error = decode(&path).unwrap_err().to_string();
            assert!(!error.contains("fill whole buffer"), "{case}: {error}");
        }
        write_payload(
            &path,
            &u32::try_from(MAX_METADATA + 1).unwrap().to_le_bytes(),
        );
        assert!(decode(&path).unwrap_err().to_string().contains("metadata"));
    }
    #[test]
    fn valid_gzip_with_extra_decompressed_data_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let slot = Session::new(root.path()).unwrap();
        let doc = Document::new(1, 1).unwrap();
        slot.checkpoint(&doc, None).unwrap();
        let bytes = std::fs::read(slot.path()).unwrap();
        let mut payload = Vec::new();
        MultiGzDecoder::new(&bytes[8..])
            .read_to_end(&mut payload)
            .unwrap();
        payload.push(0);
        write_payload(slot.path(), &payload);
        assert!(decode(slot.path())
            .unwrap_err()
            .to_string()
            .contains("Trailing"));
    }
}
