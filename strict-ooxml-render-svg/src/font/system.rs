//! The fonts installed on this machine, offered as caller-supplied fonts.
//!
//! Off by default: a rendering with the bundled faces is the same on every
//! machine (ADR-0006). With this module a caller can ask for the machine's
//! own fonts instead; the set is still explicit and named by its hash
//! ([`FontAssets::set_hash`]), so a report says which fonts a rendering used.
//!
//! [`index`] reads only the table directory and the `name`, `OS/2` and `head`
//! tables of each file, a few kilobytes, and is built once per process;
//! [`assets_for`] reads the programs of the families a document names.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use strict_ooxml_wml::model::Document;

use super::assets::{FontAsset, FontAssetError, FontAssetLimits, FontAssets};

/// One face of an installed font file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemFace {
    /// Family name (`name` ID 1), as the font spells it.
    pub family: String,
    /// `OS/2.fsSelection` (or `head.macStyle`) bold.
    pub bold: bool,
    /// `OS/2.fsSelection` (or `head.macStyle`) italic.
    pub italic: bool,
    /// The file.
    pub path: PathBuf,
    /// The face within a collection; 0 for a single font.
    pub face_index: u32,
}

/// The directories this platform installs fonts in, per user ones included.
#[must_use]
pub fn font_directories() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(windows) {
        let windows = std::env::var_os("WINDIR")
            .or_else(|| std::env::var_os("SystemRoot"))
            .map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        dirs.push(windows.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = &home {
            dirs.push(home.join("Library").join("Fonts"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = &home {
            dirs.push(home.join(".local").join("share").join("fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

/// Files and depth a scan visits at most.
const MAX_FILES: usize = 20_000;
const MAX_DEPTH: usize = 8;

/// Faces of the font files under `dirs`, in directory order, files sorted by
/// name; unreadable files are skipped.
#[must_use]
pub fn scan(dirs: &[PathBuf]) -> Vec<SystemFace> {
    let mut files = Vec::new();
    for dir in dirs {
        collect_files(dir, 0, &mut files);
    }
    let mut faces = Vec::new();
    for path in files {
        if let Ok(mut file) = std::fs::File::open(&path) {
            faces.extend(read_faces(&mut file, &path).unwrap_or_default());
        }
    }
    faces
}

/// The faces installed on this machine, scanned once per process.
#[must_use]
pub fn index() -> &'static [SystemFace] {
    static INDEX: OnceLock<Vec<SystemFace>> = OnceLock::new();
    INDEX.get_or_init(|| scan(&font_directories()))
}

/// The families `document` names: its font table, and its theme's typefaces.
#[must_use]
pub fn document_families(document: &Document) -> Vec<String> {
    let mut families: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        let name = name.trim();
        if !name.is_empty()
            && !families
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
        {
            families.push(name.to_owned());
        }
    };
    if let Some(table) = &document.font_table {
        for entry in &table.fonts {
            add(&entry.name);
        }
    }
    if let Some(theme) = &document.theme {
        for set in [&theme.fonts.major, &theme.fonts.minor] {
            for face in [&set.latin, &set.east_asia, &set.cs] {
                if let Some(name) = &face.name {
                    add(name);
                }
            }
        }
    }
    families
}

/// The installed programs of `families` (matched without regard to case), as
/// a font set; a family installed twice takes the first file found.
///
/// # Errors
///
/// A [`FontAssetError`] when the programs break `limits` or a file changed
/// since the index was built.
pub fn assets_for(
    families: &[String],
    faces: &[SystemFace],
    limits: FontAssetLimits,
) -> Result<Vec<FontAsset>, FontAssetError> {
    let wanted: Vec<String> = families
        .iter()
        .map(|family| family.to_lowercase())
        .collect();
    let mut taken: Vec<(String, bool, bool)> = Vec::new();
    let mut assets = Vec::new();
    let mut total = 0usize;
    for face in faces {
        let key = (face.family.to_lowercase(), face.bold, face.italic);
        if !wanted.contains(&key.0) || taken.contains(&key) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&face.path) else {
            continue;
        };
        if bytes.len() > limits.max_asset_bytes {
            continue;
        }
        total = total.saturating_add(bytes.len());
        if total > limits.max_total_bytes || assets.len() >= limits.max_assets {
            return Err(FontAssetError::TotalTooLarge);
        }
        assets.push(
            FontAsset::new(&face.family, face.bold, face.italic, bytes).face_index(face.face_index),
        );
        taken.push(key);
    }
    Ok(assets)
}

/// The installed fonts `document` names, from this machine's [`index`].
///
/// # Errors
///
/// A [`FontAssetError`] as [`FontAssets::new`] returns it.
pub fn system_fonts_for(document: &Document) -> Result<FontAssets, FontAssetError> {
    let limits = FontAssetLimits::default();
    let assets = assets_for(&document_families(document), index(), limits)?;
    FontAssets::new(assets, limits)
}

/// At most this many installed faces stand in for scripts the bundle lacks.
const MAX_SCRIPT_FALLBACKS: usize = 4;

/// Installed faces for the characters of `document` that no bundled face
/// draws (Telugu, say), as fallback programs for any language
/// ([`FontAsset::fallback`]): layout then measures that text with a face
/// that has it, and a painter names that face instead of a bundled one
/// without the glyphs.
///
/// Upright regular faces are preferred, then the index order; a face is
/// taken when it draws any character still missing. Characters no
/// installed face draws are remembered for the process, so a document full
/// of them reads the fonts once.
#[must_use]
pub fn script_fallbacks(
    document: &Document,
    faces: &[SystemFace],
    limits: FontAssetLimits,
) -> Vec<FontAsset> {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static NOWHERE: Mutex<BTreeSet<char>> = Mutex::new(BTreeSet::new());
    let bundled = super::builtin::bundled_families();
    let nowhere = NOWHERE.lock().map(|set| set.clone()).unwrap_or_default();
    let mut missing: BTreeSet<char> = document_chars(document)
        .into_iter()
        .filter(|ch| !ch.is_whitespace() && !ch.is_control() && !nowhere.contains(ch))
        .filter(|ch| {
            !bundled
                .iter()
                .any(|family| super::builtin::bundled_has_glyph(family, *ch))
        })
        .collect();
    let mut assets = Vec::new();
    if missing.is_empty() {
        return assets;
    }
    let mut order: Vec<&SystemFace> = faces.iter().collect();
    order.sort_by_key(|face| face.bold || face.italic);
    let mut total = 0usize;
    for face in order {
        if missing.is_empty() || assets.len() >= MAX_SCRIPT_FALLBACKS {
            break;
        }
        let Ok(bytes) = std::fs::read(&face.path) else {
            continue;
        };
        if bytes.len() > limits.max_asset_bytes
            || total.saturating_add(bytes.len()) > limits.max_total_bytes
        {
            continue;
        }
        let Ok(font) = skrifa::FontRef::from_index(&bytes, face.face_index) else {
            continue;
        };
        let charmap = skrifa::MetadataProvider::charmap(&font);
        let drawn: Vec<char> = missing
            .iter()
            .copied()
            .filter(|ch| charmap.map(*ch).is_some())
            .collect();
        if drawn.is_empty() {
            continue;
        }
        for ch in &drawn {
            missing.remove(ch);
        }
        total += bytes.len();
        assets.push(
            FontAsset::new(&face.family, face.bold, face.italic, bytes)
                .face_index(face.face_index)
                .fallback(""),
        );
    }
    if assets.len() < MAX_SCRIPT_FALLBACKS {
        // Every face was read: what is still missing is drawn by none.
        if let Ok(mut set) = NOWHERE.lock() {
            set.extend(missing);
        }
    }
    assets
}

/// The characters of the document's body text.
fn document_chars(document: &Document) -> std::collections::BTreeSet<char> {
    use strict_ooxml_wml::model::{Block, Inline, RunContent};
    fn inlines(list: &[Inline], out: &mut std::collections::BTreeSet<char>) {
        for inline in list {
            match inline {
                Inline::Run(run) => {
                    for content in &run.content {
                        if let RunContent::Text(text) = content {
                            out.extend(text.text.chars());
                        }
                    }
                }
                Inline::Hyperlink(link) => inlines(&link.inlines, out),
                Inline::SdtInline(sdt) => inlines(&sdt.inlines, out),
                Inline::Directional(directional) => inlines(&directional.inlines, out),
                _ => {}
            }
        }
    }
    fn blocks(list: &[Block], out: &mut std::collections::BTreeSet<char>) {
        for block in list {
            match block {
                Block::Paragraph(paragraph) => inlines(&paragraph.inlines, out),
                Block::Table(table) => {
                    for row in &table.rows {
                        for cell in &row.cells {
                            blocks(&cell.blocks, out);
                        }
                    }
                }
                Block::SdtBlock(sdt) => blocks(&sdt.blocks, out),
                _ => {}
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    blocks(&document.body.blocks, &mut out);
    out
}

fn collect_files(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || files.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if files.len() >= MAX_FILES {
            return;
        }
        if path.is_dir() {
            collect_files(&path, depth + 1, files);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "ttf" | "otf" | "ttc" | "otc"
                )
            })
        {
            files.push(path);
        }
    }
}

fn read_at(file: &mut (impl Read + Seek), offset: u64, length: usize) -> Option<Vec<u8>> {
    if length > 1 << 20 {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = vec![0u8; length];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let field = bytes.get(at..at.checked_add(2)?)?;
    <[u8; 2]>::try_from(field).ok().map(u16::from_be_bytes)
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field = bytes.get(at..at.checked_add(4)?)?;
    <[u8; 4]>::try_from(field).ok().map(u32::from_be_bytes)
}

/// The faces of one font file: every face of a collection.
fn read_faces(file: &mut (impl Read + Seek), path: &Path) -> Option<Vec<SystemFace>> {
    let head = read_at(file, 0, 12)?;
    let directories: Vec<u32> = if head.get(..4) == Some(b"ttcf") {
        let count = be_u32(&head, 8)?.min(256);
        let offsets = read_at(file, 12, usize::try_from(count).ok()? * 4)?;
        (0..usize::try_from(count).ok()?)
            .filter_map(|index| be_u32(&offsets, index * 4))
            .collect()
    } else {
        vec![0]
    };
    let mut faces = Vec::new();
    for (index, directory) in directories.into_iter().enumerate() {
        if let Some(mut found) = read_face(file, u64::from(directory)) {
            for face in &mut found {
                face.path = path.to_path_buf();
                face.face_index = u32::try_from(index).ok()?;
            }
            faces.extend(found);
        }
    }
    Some(faces)
}

/// One face: a [`SystemFace`] per distinct family name (a font may name its
/// family in several languages: `SimSun` and `宋体`).
fn read_face(file: &mut (impl Read + Seek), directory: u64) -> Option<Vec<SystemFace>> {
    let header = read_at(file, directory, 12)?;
    let tables = usize::from(be_u16(&header, 4)?);
    let records = read_at(file, directory + 12, tables.checked_mul(16)?)?;
    let mut find = |tag: &[u8; 4]| -> Option<Vec<u8>> {
        (0..tables).find_map(|table| {
            let at = table * 16;
            (records.get(at..at + 4) == Some(tag.as_slice())).then_some(())?;
            let offset = be_u32(&records, at + 8)?;
            let length = usize::try_from(be_u32(&records, at + 12)?).ok()?;
            read_at(file, u64::from(offset), length)
        })
    };
    let name = find(b"name")?;
    let (mut bold, mut italic) = (false, false);
    if let Some(os2) = find(b"OS/2") {
        let selection = be_u16(&os2, 62)?;
        bold = selection & 0x0020 != 0;
        italic = selection & 0x0001 != 0;
    } else if let Some(head) = find(b"head") {
        let style = be_u16(&head, 44)?;
        bold = style & 0x0001 != 0;
        italic = style & 0x0002 != 0;
    }
    let families = family_names(&name);
    Some(
        families
            .into_iter()
            .map(|family| SystemFace {
                family,
                bold,
                italic,
                path: PathBuf::new(),
                face_index: 0,
            })
            .collect(),
    )
}

/// The distinct family names (`name` ID 1) of a `name` table: Unicode and
/// Windows records (UTF-16BE), and Macintosh Roman ones in ASCII.
fn family_names(name: &[u8]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let (Some(count), Some(storage)) = (be_u16(name, 2), be_u16(name, 4)) else {
        return names;
    };
    for record in 0..usize::from(count) {
        let at = 6 + record * 12;
        let (Some(platform), Some(encoding), Some(id), Some(length), Some(offset)) = (
            be_u16(name, at),
            be_u16(name, at + 2),
            be_u16(name, at + 6),
            be_u16(name, at + 8),
            be_u16(name, at + 10),
        ) else {
            break;
        };
        if id != 1 {
            continue;
        }
        let start = usize::from(storage) + usize::from(offset);
        let Some(bytes) = name.get(start..start + usize::from(length)) else {
            continue;
        };
        let text = match (platform, encoding) {
            (0, _) | (3, 0 | 1 | 10) => {
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .filter_map(|pair| <[u8; 2]>::try_from(pair).ok().map(u16::from_be_bytes))
                    .collect();
                String::from_utf16(&units).ok()
            }
            (1, 0) if bytes.is_ascii() => std::str::from_utf8(bytes).ok().map(str::to_owned),
            _ => None,
        };
        if let Some(text) = text.map(|text| text.trim().to_owned()) {
            if !text.is_empty() && !names.contains(&text) {
                names.push(text);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{family_names, read_faces};

    #[test]
    fn a_bundled_face_reads_its_family_and_style() {
        let source = crate::font::face_source("Arial", true, false).expect("bundled");
        let mut cursor = std::io::Cursor::new(source.data.to_vec());
        let faces = read_faces(&mut cursor, std::path::Path::new("arimo.ttf")).expect("faces");
        assert!(faces.iter().any(|face| face.family == "Arimo"), "{faces:?}");
        assert!(faces.iter().all(|face| face.face_index == 0));
        assert!(family_names(&[0, 0, 0, 0, 0, 6]).is_empty());
    }
}
