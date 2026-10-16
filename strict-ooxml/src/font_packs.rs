//! Font packs: the large script fonts the library does not bundle, fetched on
//! request and kept in a local cache (feature `font-packs`; fetching needs
//! `font-download`).
//!
//! The bundle carries Latin, Greek, Cyrillic, Hebrew, Arabic and Tibetan
//! faces. Text in Han, kana, hangul, the Indic scripts, Thai, Lao, Khmer,
//! Myanmar or Ethiopic has no glyph there and is drawn as the missing-glyph
//! box. A pack is a Noto family for one such script (CJK: 40 MB; the others
//! 0.2-1.2 MB) that draws, as a fallback, what a run's face lacks.
//!
//! Nothing is fetched without the caller asking ([`Pack::install`]). The
//! catalog below is the only source: each file is fetched over HTTPS from its
//! pinned URL, must have its pinned size and SHA-256, and is checked again
//! against the pin every time it is used ([`Pack::assets`]).
//!
//! The cache is `STRICTLIB_FONT_DIR` when set, else
//! `%LOCALAPPDATA%\StrictLib\fonts` on Windows and
//! `$XDG_CACHE_HOME/strictlib/fonts` (`~/.cache/strictlib/fonts`) elsewhere.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::{FontAsset, FontAssetLimits, StrictDocument};

/// One pack: the faces for one script.
pub struct Pack {
    /// The name the user types.
    pub id: &'static str,
    /// What it covers, for `fonts list`.
    pub title: &'static str,
    /// The code points that need it, inclusive ranges in ascending order.
    ranges: &'static [(u32, u32)],
    files: &'static [PackFile],
    faces: &'static [PackFace],
}

/// One file of a pack, pinned. `size` 0 is a licence text of any size up to
/// 64 KiB; the hash decides.
pub struct PackFile {
    /// The file's name in the pack's folder.
    pub name: &'static str,
    /// Where it is fetched from (HTTPS, `raw.githubusercontent.com`).
    pub url: &'static str,
    /// Its size in bytes; 0 for a licence of any size up to 64 KiB.
    pub size: u64,
    /// Its SHA-256, as 64 hex digits.
    pub sha256: &'static str,
}

/// One face a pack delivers.
struct PackFace {
    /// The [`PackFile::name`] it is in.
    file: &'static str,
    face_index: u32,
    family: &'static str,
    bold: bool,
    /// The language it serves as a fallback for (BCP 47; empty for any).
    language: &'static str,
}

const CJK_REGULAR: &str = "NotoSansCJK-Regular.ttc";
const CJK_BOLD: &str = "NotoSansCJK-Bold.ttc";

/// Han, kana, hangul and the CJK punctuation, symbols and compatibility forms.
const CJK_RANGES: &[(u32, u32)] = &[
    (0x1100, 0x11FF),
    (0x2E80, 0x2FDF),
    (0x3000, 0x303F),
    (0x3040, 0x30FF),
    (0x3100, 0x312F),
    (0x3130, 0x318F),
    (0x3190, 0x31FF),
    (0x3200, 0x9FFF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7AF),
    (0xD7B0, 0xD7FF),
    (0xF900, 0xFAFF),
    (0xFE30, 0xFE4F),
    (0xFF00, 0xFFEF),
    (0x2_0000, 0x3_FFFF),
];

/// The faces of both collections, one per language. Simplified Chinese first:
/// Han text in a run that names no language falls to the first.
const CJK_FACES: &[PackFace] = &[
    cjk_face(CJK_REGULAR, 2, "Noto Sans CJK SC", false, "zh-cn"),
    cjk_face(CJK_BOLD, 2, "Noto Sans CJK SC", true, "zh-cn"),
    cjk_face(CJK_REGULAR, 3, "Noto Sans CJK TC", false, "zh-tw"),
    cjk_face(CJK_BOLD, 3, "Noto Sans CJK TC", true, "zh-tw"),
    cjk_face(CJK_REGULAR, 4, "Noto Sans CJK HK", false, "zh-hk"),
    cjk_face(CJK_BOLD, 4, "Noto Sans CJK HK", true, "zh-hk"),
    cjk_face(CJK_REGULAR, 0, "Noto Sans CJK JP", false, "ja"),
    cjk_face(CJK_BOLD, 0, "Noto Sans CJK JP", true, "ja"),
    cjk_face(CJK_REGULAR, 1, "Noto Sans CJK KR", false, "ko"),
    cjk_face(CJK_BOLD, 1, "Noto Sans CJK KR", true, "ko"),
];

const fn cjk_face(
    file: &'static str,
    face_index: u32,
    family: &'static str,
    bold: bool,
    language: &'static str,
) -> PackFace {
    PackFace {
        file,
        face_index,
        family,
        bold,
        language,
    }
}

/// A single-script Noto pack from `google/fonts` at a pinned commit: the
/// variable font (drawn at its default instance) and its licence.
macro_rules! noto_pack {
    ($id:literal, $title:literal, $family:literal, $dir:literal, $file:literal, $size:literal,
     $sha:literal, $license_sha:literal, $ranges:expr) => {
        Pack {
            id: $id,
            title: $title,
            ranges: $ranges,
            files: &[
                PackFile {
                    name: $file,
                    url: concat!(
                        "https://raw.githubusercontent.com/google/fonts/",
                        "d47d546884f75bbf718d3ddbb8d4627c68b9c782/ofl/",
                        $dir,
                        "/",
                        $file
                    ),
                    size: $size,
                    sha256: $sha,
                },
                PackFile {
                    name: "OFL.txt",
                    url: concat!(
                        "https://raw.githubusercontent.com/google/fonts/",
                        "d47d546884f75bbf718d3ddbb8d4627c68b9c782/ofl/",
                        $dir,
                        "/OFL.txt"
                    ),
                    size: 0,
                    sha256: $license_sha,
                },
            ],
            faces: &[PackFace {
                file: $file,
                face_index: 0,
                family: $family,
                bold: false,
                language: "",
            }],
        }
    };
}

/// Every pack, in the order `fonts list` shows them.
pub const CATALOG: &[Pack] = &[
    Pack {
        id: "cjk",
        title: "Chinese, Japanese, Korean (Noto Sans CJK, regular and bold)",
        ranges: CJK_RANGES,
        files: &[
            PackFile {
                name: CJK_REGULAR,
                url: concat!(
                    "https://raw.githubusercontent.com/notofonts/noto-cjk/",
                    "165c01b46ea533872e002e0785ff17e44f6d97d8/Sans/OTC/NotoSansCJK-Regular.ttc"
                ),
                size: 19_484_784,
                sha256: "b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a",
            },
            PackFile {
                name: CJK_BOLD,
                url: concat!(
                    "https://raw.githubusercontent.com/notofonts/noto-cjk/",
                    "165c01b46ea533872e002e0785ff17e44f6d97d8/Sans/OTC/NotoSansCJK-Bold.ttc"
                ),
                size: 20_050_760,
                sha256: "faa5f3656a78b2e2d450d27fe8382c778bc2b6bb5ea29c986664a6a435056ceb",
            },
            PackFile {
                name: "LICENSE",
                url: concat!(
                    "https://raw.githubusercontent.com/notofonts/noto-cjk/",
                    "165c01b46ea533872e002e0785ff17e44f6d97d8/LICENSE"
                ),
                size: 4301,
                sha256: "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
            },
        ],
        faces: CJK_FACES,
    },
    noto_pack!(
        "devanagari",
        "Devanagari: Hindi, Marathi, Nepali, Sanskrit",
        "Noto Sans Devanagari",
        "notosansdevanagari",
        "NotoSansDevanagari[wdth,wght].ttf",
        641_944,
        "14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e",
        "a216f6f8d85c7228093e0ee5e258d9d377e6671f68acb4db1930b29583d0f331",
        &[(0x0900, 0x097F), (0x1CD0, 0x1CFF), (0xA8E0, 0xA8FF)]
    ),
    noto_pack!(
        "bengali",
        "Bengali, Assamese",
        "Noto Sans Bengali",
        "notosansbengali",
        "NotoSansBengali[wdth,wght].ttf",
        463_668,
        "dcd42978094e584a849c84a51450eeac40c8826057d566ea6d4b9627a403a05a",
        "754f0e221aa7d5a915489f3bf1f20fe53ddc35ab2834a4d91656d78f9622de70",
        &[(0x0980, 0x09FF)]
    ),
    noto_pack!(
        "gurmukhi",
        "Gurmukhi: Punjabi",
        "Noto Sans Gurmukhi",
        "notosansgurmukhi",
        "NotoSansGurmukhi[wdth,wght].ttf",
        268_608,
        "1e6f728fa620e566f842d81e220265813faa12771214765d289c98e035adc5f2",
        "3f7451b7e2c8381be0c5712f7b0dd5c2d75fe787ae16a99f18b7fa45627a0fde",
        &[(0x0A00, 0x0A7F)]
    ),
    noto_pack!(
        "gujarati",
        "Gujarati",
        "Noto Sans Gujarati",
        "notosansgujarati",
        "NotoSansGujarati[wdth,wght].ttf",
        672_904,
        "9901d8552f1dd5d2c50dbd4caa6f6e174e74e8264f06594ab259ae6e7b1ac428",
        "c0b88977aa18b5e4fd05d646d560da89fde61b6581fa4507cb00dd90bd1bf7d4",
        &[(0x0A80, 0x0AFF)]
    ),
    noto_pack!(
        "oriya",
        "Oriya: Odia",
        "Noto Sans Oriya",
        "notosansoriya",
        "NotoSansOriya[wdth,wght].ttf",
        613_116,
        "910342275d619081b896b4ed25fd9ad6ff320c25591b810de1fcc4beb153ba82",
        "c90ff15ade263a49148e35af34e9c177c6d56881e81260b29f10123e9e227eca",
        &[(0x0B00, 0x0B7F)]
    ),
    noto_pack!(
        "tamil",
        "Tamil",
        "Noto Sans Tamil",
        "notosanstamil",
        "NotoSansTamil[wdth,wght].ttf",
        340_668,
        "aa3a9b321f4b0bb2c40203ffbde9af89713227866e0e13f76e5b9eeea727cf88",
        "f8ff8ce7d0a81bf8d5e121c635ef027250c531f2fd37d5988b8dd6e45f19d7f1",
        &[(0x0B80, 0x0BFF)]
    ),
    noto_pack!(
        "telugu",
        "Telugu",
        "Noto Sans Telugu",
        "notosanstelugu",
        "NotoSansTelugu[wdth,wght].ttf",
        888_808,
        "e618af7bf999df192ed4f388eba2e563f2b5015034e9cbb317b5bd793bd7334d",
        "481c72a8f1b4f645a7e6b10326be41b2da2c15cb470ff48a07fab773eed00102",
        &[(0x0C00, 0x0C7F)]
    ),
    noto_pack!(
        "kannada",
        "Kannada",
        "Noto Sans Kannada",
        "notosanskannada",
        "NotoSansKannada[wdth,wght].ttf",
        641_372,
        "cca4f3b3a8cb12fb261f1b43baf5d2f7f59d90fe123d41f0065ed3a183997ec9",
        "9d49a4e56df9b36403b139f48bf07fd87cc5e716cb3203c54b04b674adb16238",
        &[(0x0C80, 0x0CFF)]
    ),
    noto_pack!(
        "malayalam",
        "Malayalam",
        "Noto Sans Malayalam",
        "notosansmalayalam",
        "NotoSansMalayalam[wdth,wght].ttf",
        504_204,
        "312e0e7c3cc15fa09eb42a8f749eeb246b593ed420e3c81aafe8d910c3a6fb56",
        "1df18163a0bf60f02131401ba1609df90051f40c4047684dda1c8a2e9586f7f2",
        &[(0x0D00, 0x0D7F)]
    ),
    noto_pack!(
        "sinhala",
        "Sinhala",
        "Noto Sans Sinhala",
        "notosanssinhala",
        "NotoSansSinhala[wdth,wght].ttf",
        1_181_956,
        "9bd93e407a278075be403324063bc94a7e306c44de4df81214e932330c22eecf",
        "2d6f7c43bce61f4b1919379f901bc613484f5285f520b6d29bb7c1f31b17e841",
        &[(0x0D80, 0x0DFF)]
    ),
    noto_pack!(
        "thai",
        "Thai",
        "Noto Sans Thai",
        "notosansthai",
        "NotoSansThai[wdth,wght].ttf",
        218_652,
        "5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8",
        "2e98fd23a52d253db8612cd5942c8f2ff4111b21d2367050fdca91d8ccc374a0",
        &[(0x0E00, 0x0E7F)]
    ),
    noto_pack!(
        "lao",
        "Lao",
        "Noto Sans Lao",
        "notosanslao",
        "NotoSansLao[wdth,wght].ttf",
        176_620,
        "9608b94603a82d09a8038946f9775242f99e3b3459b7f1e4d5b335b578cd7ab3",
        "a42993999944845fb5af693ea678a372a053db1f0981d55e911dcfa9d330f279",
        &[(0x0E80, 0x0EFF)]
    ),
    noto_pack!(
        "myanmar",
        "Myanmar: Burmese",
        "Noto Sans Myanmar",
        "notosansmyanmar",
        "NotoSansMyanmar[wdth,wght].ttf",
        784_832,
        "7abbbfbe2514105d7ce94937aee3feb2ba89b73a256c8b77b5866bd9b83e32ec",
        "246a75859267af7da466823969d2e2b407ed8455ee5f74f4c8d63d8783be9b57",
        &[(0x1000, 0x109F), (0xA9E0, 0xA9FF), (0xAA60, 0xAA7F)]
    ),
    noto_pack!(
        "ethiopic",
        "Ethiopic: Amharic, Tigrinya",
        "Noto Sans Ethiopic",
        "notosansethiopic",
        "NotoSansEthiopic[wdth,wght].ttf",
        1_143_168,
        "0dbccc00b22d180ebd6a4bd8a733918a29e709fa6798023adc3e6cd40da65077",
        "72606b23f312cb25973958f2892d4d2c2012deabadbf0f763232624a9649fc69",
        &[(0x1200, 0x139F), (0x2D80, 0x2DDF), (0xAB00, 0xAB2F)]
    ),
    noto_pack!(
        "khmer",
        "Khmer",
        "Noto Sans Khmer",
        "notosanskhmer",
        "NotoSansKhmer[wdth,wght].ttf",
        359_788,
        "f37a8431a0c5d5ed2f81a767417546aca576a81fb7eff9c924d46aecf828f2ca",
        "be0407f060aea48787ff9e75d8d3aedef70aef113b3ce9aca26fdaacd10b1870",
        &[(0x1780, 0x17FF), (0x19E0, 0x19FF)]
    ),
];

/// The licence files of `google/fonts` differ only in the copyright line; any
/// size up to this is accepted, the pin decides.
#[cfg(feature = "font-download")]
const LICENSE_MAX_BYTES: u64 = 64 * 1024;

impl Pack {
    /// The files to fetch.
    #[must_use]
    pub fn files(&self) -> &'static [PackFile] {
        self.files
    }

    /// Bytes to download.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    /// Whether the pack draws `ch`.
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        let code = u32::from(ch);
        self.ranges
            .iter()
            .any(|&(first, last)| (first..=last).contains(&code))
    }

    /// Downloads the pack's files that are not in place into `root/<id>`.
    ///
    /// # Errors
    ///
    /// See [`install_files`].
    #[cfg(feature = "font-download")]
    pub fn install(&self, root: &Path) -> Result<(), String> {
        install_files(self.files, &root.join(self.id))
    }

    /// Whether every file is in `dir` with its pinned size (the hash is
    /// checked when the fonts are used).
    pub fn is_installed(&self, root: &Path) -> bool {
        files_present(&root.join(self.id), self.files)
    }

    /// The pack's faces from `root`, pinned and marked as fallbacks.
    ///
    /// # Errors
    ///
    /// A file cannot be read, or is larger than `limits` allow.
    pub fn assets(&self, root: &Path, limits: FontAssetLimits) -> Result<Vec<FontAsset>, String> {
        let dir = root.join(self.id);
        let mut assets = Vec::with_capacity(self.faces.len());
        for file in self.files {
            let faces: Vec<&PackFace> = self
                .faces
                .iter()
                .filter(|face| face.file == file.name)
                .collect();
            if faces.is_empty() {
                continue;
            }
            let path = dir.join(file.name);
            let bytes = read_capped(&path, limits.max_asset_bytes)?;
            let pin = decode_pin(file.sha256)?;
            let bytes: std::sync::Arc<[u8]> = bytes.into();
            for face in faces {
                assets.push(
                    FontAsset::new(face.family, face.bold, false, bytes.clone())
                        .face_index(face.face_index)
                        .pinned(pin)
                        .fallback(face.language),
                );
            }
        }
        Ok(assets)
    }
}

/// Whether every one of `files` is in `dir` with its pinned size.
pub fn files_present(dir: &Path, files: &[PackFile]) -> bool {
    files.iter().all(|file| {
        std::fs::metadata(dir.join(file.name))
            .is_ok_and(|meta| meta.is_file() && (file.size == 0 || meta.len() == file.size))
    })
}

/// A file's bytes, refused past `cap`.
///
/// # Errors
///
/// It cannot be read.
pub fn read_capped(path: &Path, cap: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(bytes)
}

fn decode_pin(hex: &str) -> Result<[u8; 32], String> {
    decode_sha256(hex).ok_or_else(|| format!("malformed pin {hex}"))
}

/// A SHA-256 written as 64 hex digits.
fn decode_sha256(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// The pack named `id`.
pub fn pack(id: &str) -> Option<&'static Pack> {
    CATALOG
        .iter()
        .find(|pack| pack.id.eq_ignore_ascii_case(id.trim()))
}

/// The cache directory.
pub fn cache_dir() -> Option<PathBuf> {
    cache_root("STRICTLIB_FONT_DIR", "fonts")
}

/// `env` when set, else `<local cache>/StrictLib/<leaf>` on Windows and
/// `$XDG_CACHE_HOME/strictlib/<leaf>` (`~/.cache/strictlib/<leaf>`) elsewhere.
pub fn cache_root(env: &str, leaf: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(env).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join("StrictLib").join(leaf));
    }
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|dir| !dir.is_empty())
                .map(|home| PathBuf::from(home).join(".cache"))
        })
        .map(|dir| dir.join("strictlib").join(leaf))
}

/// The packs the text of `document` needs, in catalog order: the character
/// data of every part under `/word/` is scanned for characters in a pack's
/// ranges. Attribute values are not text: a theme names `ＭＳ 明朝` for
/// Japanese and `w:rFonts` names `宋体` in documents with no CJK text.
pub fn needed(document: &StrictDocument) -> Vec<&'static Pack> {
    let package = document.package();
    let mut wanted = vec![false; CATALOG.len()];
    for part in package.parts() {
        let xml = part
            .content_type
            .as_deref()
            .is_some_and(|kind| kind.ends_with("xml"));
        if !xml || !part.id.as_str().starts_with("/word/") {
            continue;
        }
        let Ok(bytes) = package.read_part(&part.id) else {
            continue;
        };
        mark_needed(&String::from_utf8_lossy(&bytes), &mut wanted);
    }
    CATALOG
        .iter()
        .zip(wanted)
        .filter_map(|(pack, wanted)| wanted.then_some(pack))
        .collect()
}

fn mark_needed(xml: &str, wanted: &mut [bool]) {
    let mut in_tag = false;
    let mut quote: Option<char> = None;
    for ch in xml.chars() {
        match (in_tag, quote, ch) {
            (false, _, '<') => in_tag = true,
            (true, None, '>') => in_tag = false,
            (true, None, '"' | '\'') => quote = Some(ch),
            (true, Some(open), _) if open == ch => quote = None,
            (false, _, _) if u32::from(ch) >= 0x0900 => mark_char(ch, wanted),
            _ => {}
        }
    }
}

fn mark_char(ch: char, wanted: &mut [bool]) {
    for (pack, flag) in CATALOG.iter().zip(wanted.iter_mut()) {
        if !*flag && pack.covers(ch) {
            *flag = true;
        }
    }
}

/// Downloads the `files` that are not in place (with their pinned size and
/// hash) into `dir`.
///
/// # Errors
///
/// The folder cannot be made, or a file cannot be fetched or does not match
/// its pin.
#[cfg(feature = "font-download")]
pub fn install_files(files: &[PackFile], dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let agent = download::agent();
    for file in files {
        let path = dir.join(file.name);
        if std::fs::metadata(&path).is_ok_and(|meta| file.size == 0 || meta.len() == file.size)
            && sha256_file(&path).is_ok_and(|hash| hash == file.sha256)
        {
            continue;
        }
        download::fetch(&agent, file, &path)?;
    }
    Ok(())
}

/// Hashes a file.
///
/// # Errors
///
/// It cannot be read.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::Digest as _;
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(feature = "font-download")]
mod download {
    use std::io::{Read, Write as _};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use sha2::{Digest as _, Sha256};

    use super::{hex, PackFile, LICENSE_MAX_BYTES};

    /// The only host the catalog names.
    const HOST: &str = "https://raw.githubusercontent.com/";

    pub(super) fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            // The host serves the files directly; a redirect is refused, not
            // followed.
            .max_redirects(0)
            .http_status_as_error(false)
            .https_only(true)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_global(Some(Duration::from_secs(900)))
            .user_agent(concat!(
                "strict-ooxml-font-packs/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .into()
    }

    /// Streams `file` to `<path>.part`, checks its size and hash, and renames
    /// it into place.
    pub(super) fn fetch(agent: &ureq::Agent, file: &PackFile, path: &Path) -> Result<(), String> {
        if !file.url.starts_with(HOST) {
            return Err(format!("refusing {}: not on {HOST}", file.url));
        }
        let mut response = agent
            .get(file.url)
            .call()
            .map_err(|error| format!("GET {}: {error}", file.url))?;
        let status = response.status();
        if status.as_u16() != 200 {
            return Err(format!("GET {}: HTTP {status}", file.url));
        }
        let cap = if file.size == 0 {
            LICENSE_MAX_BYTES
        } else {
            file.size
        };
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        let mut body = response.body_mut().as_reader().take(cap.saturating_add(1));
        let result = write_verified(&mut body, file, cap, &partial).and_then(|()| {
            std::fs::rename(&partial, path)
                .map_err(|error| format!("cannot move {} into place: {error}", path.display()))
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        result
    }

    /// Copies `body` to `partial` and checks it is the pinned file.
    fn write_verified(
        body: &mut dyn Read,
        file: &PackFile,
        cap: u64,
        partial: &Path,
    ) -> Result<(), String> {
        let mut out = std::fs::File::create(partial)
            .map_err(|error| format!("cannot create {}: {error}", partial.display()))?;
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            let read = body
                .read(&mut buffer)
                .map_err(|error| format!("GET {}: {error}", file.url))?;
            if read == 0 {
                break;
            }
            let chunk = buffer.get(..read).unwrap_or_default();
            hasher.update(chunk);
            out.write_all(chunk)
                .map_err(|error| format!("cannot write {}: {error}", partial.display()))?;
            written = written.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        }
        out.sync_all()
            .map_err(|error| format!("cannot write {}: {error}", partial.display()))?;
        let expected = if file.size == 0 { cap } else { file.size };
        if written > cap || (file.size != 0 && written != file.size) {
            return Err(format!(
                "{}: {written} bytes, expected {expected}",
                file.url
            ));
        }
        let hash = hex(&hasher.finalize());
        if hash != file.sha256 {
            return Err(format!(
                "{}: SHA-256 {hash} does not match the pin",
                file.url
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{mark_needed, pack, CATALOG};

    #[test]
    fn the_catalog_is_pinned_and_well_formed() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|pack| pack.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for pack in CATALOG {
            assert!(!pack.faces.is_empty(), "{}", pack.id);
            for file in pack.files {
                assert!(file.url.starts_with("https://raw.githubusercontent.com/"));
                assert!(super::decode_sha256(file.sha256).is_some());
            }
            for face in pack.faces {
                assert!(!face.family.is_empty(), "{}", pack.id);
                assert!(pack.files.iter().any(|file| file.name == face.file));
            }
            assert!(pack.ranges.windows(2).all(|pair| pair[0].1 < pair[1].0));
        }
    }

    #[test]
    fn text_selects_the_packs_of_its_scripts() {
        let mut wanted = vec![false; CATALOG.len()];
        mark_needed(
            "<w:t>Hello, мир! नमस्ते</w:t><w:rFonts w:eastAsia='ກ' w:cs=\"a>ក\"/>\
             <w:t xml:space=\"preserve\">ภาษาไทย 漢字</w:t>",
            &mut wanted,
        );
        let ids: Vec<&str> = CATALOG
            .iter()
            .zip(&wanted)
            .filter_map(|(pack, wanted)| wanted.then_some(pack.id))
            .collect();
        assert_eq!(ids, ["cjk", "devanagari", "thai"]);
        assert_eq!(pack(" Thai ").map(|pack| pack.id), Some("thai"));
    }
}
