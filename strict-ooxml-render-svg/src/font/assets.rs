//! Font programs supplied by the caller (RTF RC02), on the document-font path.
//!
//! The bundled faces keep output identical on every machine; a caller who has
//! the fonts a document names (licensed files, or the fonts installed on the
//! machine, `font::system`, feature `system-fonts`) passes them here. They enter the same
//! [`FontPrograms`](super::embedded::FontPrograms) a document's own embedded
//! fonts enter, so layout measures, and SVG and PDF draw and embed, the same
//! bytes. A font the document embeds wins over a caller's one of the same
//! name and style.
//!
//! The set is reproducible: every program may be pinned by SHA-256, and
//! [`FontAssets::set_hash`] names the whole set, so a report can say which
//! fonts a rendering used.

use std::sync::Arc;

use sha2::{Digest as _, Sha256};

use super::embedded::FontProgram;

/// One program the caller supplies for one family and style.
#[derive(Clone, Debug)]
pub struct FontAsset {
    family: String,
    bold: bool,
    italic: bool,
    bytes: Arc<[u8]>,
    face_index: u32,
    sha256: Option<[u8; 32]>,
}

impl FontAsset {
    /// The program for `family` in the given style: an OpenType/TrueType file,
    /// or a collection (`.ttc`) with [`Self::face_index`] naming the face.
    #[must_use]
    pub fn new(family: &str, bold: bool, italic: bool, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            family: family.trim().to_owned(),
            bold,
            italic,
            bytes: bytes.into(),
            face_index: 0,
            sha256: None,
        }
    }

    /// Selects the face of a collection.
    #[must_use]
    pub fn face_index(mut self, index: u32) -> Self {
        self.face_index = index;
        self
    }

    /// Pins the file's SHA-256; a different file is refused.
    #[must_use]
    pub fn pinned(mut self, sha256: [u8; 32]) -> Self {
        self.sha256 = Some(sha256);
        self
    }

    /// The family name this program answers to.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The lower-cased family and the style: what two programs of one set
    /// may not share.
    #[must_use]
    pub fn key(&self) -> (String, bool, bool) {
        (self.family.to_lowercase(), self.bold, self.italic)
    }
}

/// Bounds on a set of caller-supplied fonts, checked before any is parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontAssetLimits {
    /// Programs in one set.
    pub max_assets: usize,
    /// Bytes of one file.
    pub max_asset_bytes: usize,
    /// Bytes of all files.
    pub max_total_bytes: usize,
}

impl Default for FontAssetLimits {
    fn default() -> Self {
        Self {
            max_assets: 4096,
            max_asset_bytes: 64 * 1024 * 1024,
            max_total_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// Why a set of fonts was refused; no partial set is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontAssetError {
    /// More programs than [`FontAssetLimits::max_assets`].
    TooMany(usize),
    /// A file over [`FontAssetLimits::max_asset_bytes`].
    TooLarge(String),
    /// All files together over [`FontAssetLimits::max_total_bytes`].
    TotalTooLarge,
    /// A file that is not the pinned one.
    HashMismatch(String),
    /// A file that is no usable font program, or no such face in a collection.
    Invalid(String),
    /// Two programs for one family and style.
    Duplicate(String),
}

impl std::fmt::Display for FontAssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany(count) => write!(f, "{count} font programs exceed the limit"),
            Self::TooLarge(family) => write!(f, "font program for {family:?} exceeds the limit"),
            Self::TotalTooLarge => f.write_str("font programs exceed the total byte limit"),
            Self::HashMismatch(family) => {
                write!(
                    f,
                    "font program for {family:?} does not match its pinned SHA-256"
                )
            }
            Self::Invalid(family) => write!(f, "font program for {family:?} is not usable"),
            Self::Duplicate(family) => {
                write!(f, "two font programs for {family:?} in the same style")
            }
        }
    }
}

impl std::error::Error for FontAssetError {}

/// Validated caller-supplied programs, keyed by family (case-insensitive) and
/// style. Cheap to clone; immutable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontAssets {
    faces: Vec<((String, bool, bool), Arc<FontProgram>)>,
    set_hash: [u8; 32],
}

impl FontAssets {
    /// Validates `assets`: limits, pins, that each is a font program (the face
    /// of a collection is taken out on its own), one program per family and
    /// style.
    ///
    /// # Errors
    ///
    /// The first [`FontAssetError`]; nothing is admitted then.
    pub fn new(assets: Vec<FontAsset>, limits: FontAssetLimits) -> Result<Self, FontAssetError> {
        if assets.len() > limits.max_assets {
            return Err(FontAssetError::TooMany(assets.len()));
        }
        let mut total = 0usize;
        let mut faces: Vec<((String, bool, bool), Arc<FontProgram>)> =
            Vec::with_capacity(assets.len());
        for asset in assets {
            if asset.bytes.len() > limits.max_asset_bytes {
                return Err(FontAssetError::TooLarge(asset.family));
            }
            total = total.saturating_add(asset.bytes.len());
            if total > limits.max_total_bytes {
                return Err(FontAssetError::TotalTooLarge);
            }
            if let Some(pin) = asset.sha256 {
                if <[u8; 32]>::from(Sha256::digest(&asset.bytes)) != pin {
                    return Err(FontAssetError::HashMismatch(asset.family));
                }
            }
            let key = (asset.family.to_lowercase(), asset.bold, asset.italic);
            if faces.iter().any(|(existing, _)| *existing == key) {
                return Err(FontAssetError::Duplicate(asset.family));
            }
            let bytes = single_face(&asset.bytes, asset.face_index)
                .ok_or_else(|| FontAssetError::Invalid(asset.family.clone()))?;
            let program = FontProgram::from_bytes(bytes, "ASSET")
                .ok_or_else(|| FontAssetError::Invalid(asset.family.clone()))?;
            faces.push((key, Arc::new(program)));
        }
        faces.sort_by(|(a, _), (b, _)| a.cmp(b));
        let mut hasher = Sha256::new();
        for ((family, bold, italic), program) in &faces {
            hasher.update(family.as_bytes());
            hasher.update([u8::from(*bold), u8::from(*italic)]);
            hasher.update(program.hash);
        }
        Ok(Self {
            faces,
            set_hash: hasher.finalize().into(),
        })
    }

    /// Programs in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// SHA-256 over every family, style and program in the set: the same set
    /// gives the same rendering.
    #[must_use]
    pub fn set_hash(&self) -> [u8; 32] {
        self.set_hash
    }

    /// Families in the set, lower-cased, without repeats.
    #[must_use]
    pub fn families(&self) -> Vec<&str> {
        let mut families: Vec<&str> = self
            .faces
            .iter()
            .map(|((family, _, _), _)| family.as_str())
            .collect();
        families.dedup();
        families
    }

    /// The programs, keyed by lower-cased family and style.
    pub(crate) fn programs(
        &self,
    ) -> impl Iterator<Item = (&(String, bool, bool), &Arc<FontProgram>)> {
        self.faces.iter().map(|(key, program)| (key, program))
    }
}

/// The bytes of face `index` as a font of its own: the file itself for a
/// single font (index 0), or the face's tables copied out of a collection.
#[must_use]
pub fn single_face(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
    if bytes.get(..4) != Some(b"ttcf") {
        return (index == 0).then(|| bytes.to_vec());
    }
    let read_u32 = |at: usize| -> Option<u32> {
        let field = bytes.get(at..at.checked_add(4)?)?;
        <[u8; 4]>::try_from(field).ok().map(u32::from_be_bytes)
    };
    let read_u16 = |at: usize| -> Option<u16> {
        let field = bytes.get(at..at.checked_add(2)?)?;
        <[u8; 2]>::try_from(field).ok().map(u16::from_be_bytes)
    };
    let count = read_u32(8)?;
    if index >= count {
        return None;
    }
    let directory = usize::try_from(read_u32(12 + 4 * usize::try_from(index).ok()?)?).ok()?;
    let version = read_u32(directory)?;
    let tables = usize::from(read_u16(directory + 4)?);
    let header = 12 + 16 * tables;
    let mut records = Vec::with_capacity(tables);
    for table in 0..tables {
        let at = directory + 12 + 16 * table;
        let tag = read_u32(at)?;
        let checksum = read_u32(at + 4)?;
        let offset = usize::try_from(read_u32(at + 8)?).ok()?;
        let length = usize::try_from(read_u32(at + 12)?).ok()?;
        let data = bytes.get(offset..offset.checked_add(length)?)?;
        records.push((tag, checksum, data));
    }
    let mut out = Vec::with_capacity(header + records.iter().map(|r| r.2.len() + 3).sum::<usize>());
    out.extend_from_slice(&version.to_be_bytes());
    let tables16 = u16::try_from(tables).ok()?;
    // The table directory's binary-search fields.
    let mut power = 1u16;
    let mut log = 0u16;
    while power.checked_mul(2).is_some_and(|next| next <= tables16) {
        power *= 2;
        log += 1;
    }
    let search_range = power.checked_mul(16)?;
    out.extend_from_slice(&tables16.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&log.to_be_bytes());
    out.extend_from_slice(&(tables16.checked_mul(16)?.checked_sub(search_range)?).to_be_bytes());
    let mut offset = header;
    for (tag, checksum, data) in &records {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, _, data) in &records {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}
