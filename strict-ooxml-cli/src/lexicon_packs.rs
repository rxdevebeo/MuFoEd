//! Word lists for recognised text, fetched on request like the font packs.
//!
//! A vision model drops diacritics (`nemacki` for `nemački`); a language's
//! word forms put them back ([`strict_ooxml::Lexicon`]). A pack is a Hunspell
//! dictionary and, to choose between restorations, a frequency list - several
//! megabytes the library does not carry. `strict-ooxml lexicons install <id>`
//! or `--fetch-lexicon` downloads one; `--ocr-lang <tag>` on `from-pdf` uses
//! the installed pack of that language. Every file is pinned by URL, size and
//! SHA-256 and checked again each time it is loaded.
//!
//! The cache is `STRICTLIB_LEXICON_DIR`, else `%LOCALAPPDATA%\StrictLib\lexicons`
//! or `~/.cache/strictlib/lexicons`.
// Loading a word list serves recognition only (feature `ocr-ollama`); the
// pack commands work in every build.
#![cfg_attr(not(feature = "ocr-ollama"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::font_packs::{self, PackFile};

/// One language's word list.
struct LexiconPack {
    /// The name the user types: the primary language subtag it serves.
    id: &'static str,
    title: &'static str,
    /// The tag the word list is for.
    language: &'static str,
    dic: &'static str,
    aff: &'static str,
    frequencies: Option<&'static str>,
    files: &'static [PackFile],
}

macro_rules! libreoffice {
    ($path:literal) => {
        concat!(
            "https://raw.githubusercontent.com/LibreOffice/dictionaries/",
            "5fe575dbcfeb789e2ab2d9731b88ac0dacc4f453/",
            $path
        )
    };
}

macro_rules! frequency_words {
    ($path:literal) => {
        concat!(
            "https://raw.githubusercontent.com/hermitdave/FrequencyWords/",
            "525f9b560de45753a5ea01069454e72e9aa541c6/",
            $path
        )
    };
}

const CATALOG: &[LexiconPack] = &[LexiconPack {
    id: "sr",
    title: "Serbian, Latin script (Hunspell by Milutin Smiljanic, LGPL-3/MPL-2/GPL-3; \
            OpenSubtitles frequencies by Hermit Dave, CC BY-SA 4.0)",
    language: "sr-Latn",
    dic: "sr-Latn.dic",
    aff: "sr-Latn.aff",
    frequencies: Some("sr_50k.txt"),
    files: &[
        PackFile {
            name: "sr-Latn.dic",
            url: libreoffice!("sr/sr-Latn.dic"),
            size: 3_817_505,
            sha256: "f51950195e5bd0aaf155d3488517d4dbc4c8fbc772ecbafbf9131e69a257b1ff",
        },
        PackFile {
            name: "sr-Latn.aff",
            url: libreoffice!("sr/sr-Latn.aff"),
            size: 684_540,
            sha256: "38b99cae0005ff20b015d829580a0f3533ffabc1f889a54144a74e90069548e6",
        },
        PackFile {
            name: "README.txt",
            url: libreoffice!("sr/README.txt"),
            size: 0,
            sha256: "2a38abe899cefc84dcf40f7f242d36560542bceab062d4bed2da18f1db19c8b1",
        },
        PackFile {
            name: "sr_50k.txt",
            url: frequency_words!("content/2018/sr/sr_50k.txt"),
            size: 641_509,
            sha256: "c84eaf5e324fe5deb0aa491935bff5e9338b6cd687f6f7aacc56988594a39859",
        },
        PackFile {
            name: "FrequencyWords-README.md",
            url: frequency_words!("README.md"),
            size: 0,
            sha256: "a92fcc50e27f7b2d5860ec1c1ce794dc4c892d945bfa4c8c3136cdfd213ee710",
        },
    ],
}];

/// The largest file a pack may have.
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

fn cache_dir() -> Option<PathBuf> {
    font_packs::cache_root("STRICTLIB_LEXICON_DIR", "lexicons")
}

/// The pack for a language tag: its primary subtag (`sr-Latn-RS` is `sr`).
fn pack_for(language: &str) -> Option<&'static LexiconPack> {
    let primary = language.split(['-', '_']).next().unwrap_or("").trim();
    CATALOG
        .iter()
        .find(|pack| pack.id.eq_ignore_ascii_case(primary))
}

impl LexiconPack {
    fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    fn is_installed(&self, root: &Path) -> bool {
        font_packs::files_present(&root.join(self.id), self.files)
    }

    fn install(&self, root: &Path) -> Result<(), String> {
        font_packs::install_files("lexicons", self.id, self.files, &root.join(self.id))
    }

    /// Reads, checks against the pins and builds the word list.
    fn load(&self, root: &Path) -> Result<strict_ooxml::Lexicon, String> {
        let dir = root.join(self.id);
        let read = |name: &str| -> Result<String, String> {
            let file = self
                .files
                .iter()
                .find(|file| file.name == name)
                .ok_or_else(|| format!("{name} is not in pack {}", self.id))?;
            let path = dir.join(name);
            let hash = font_packs::sha256_file(&path)?;
            if hash != file.sha256 {
                return Err(format!(
                    "{} does not match its pinned SHA-256; reinstall the pack \
                     (strict-ooxml lexicons remove {id}; strict-ooxml lexicons install {id})",
                    path.display(),
                    id = self.id
                ));
            }
            let bytes = font_packs::read_capped(&path, MAX_FILE_BYTES)?;
            String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))
        };
        let dic = read(self.dic)?;
        let aff = read(self.aff)?;
        let frequencies = self.frequencies.map(read).transpose()?;
        strict_ooxml::Lexicon::from_hunspell(self.language, &dic, &aff, frequencies.as_deref())
            .map_err(|error| error.to_string())
    }
}

/// The word list for `language` when its pack is installed, fetched first
/// when `fetch` is set; `None` (with a note) when there is no pack for the
/// language or it is not installed.
pub(super) fn lexicon_for(
    language: &str,
    fetch: bool,
) -> Result<Option<Arc<strict_ooxml::Lexicon>>, String> {
    let Some(pack) = pack_for(language) else {
        eprintln!("note: no word list for '{language}'; recognised words are left as read");
        return Ok(None);
    };
    let root = cache_dir().ok_or("no lexicon cache directory (set STRICTLIB_LEXICON_DIR)")?;
    if !pack.is_installed(&root) {
        if !fetch {
            eprintln!(
                "note: the '{id}' word list restores diacritics a model drops; \
                 `strict-ooxml lexicons install {id}` downloads it ({size}), or pass --fetch-lexicon",
                id = pack.id,
                size = font_packs::megabytes(pack.size())
            );
            return Ok(None);
        }
        pack.install(&root)?;
    }
    let started = std::time::Instant::now();
    let lexicon = pack.load(&root)?;
    eprintln!(
        "lexicon: {} ({} word forms, built in {:.1}s)",
        lexicon.language(),
        lexicon.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(Some(Arc::new(lexicon)))
}

/// `lexicons list|install|remove|path`.
pub(super) fn run(args: &[String]) -> Result<(), String> {
    let (command, rest) = args
        .split_first()
        .ok_or("'lexicons' expects list, install, remove or path")?;
    match command.as_str() {
        "list" => {
            let root = cache_dir();
            for pack in CATALOG {
                let state = match &root {
                    Some(root) if pack.is_installed(root) => "installed",
                    _ => "-",
                };
                println!(
                    "{:<6} {:>8}  {:<9}  {}",
                    pack.id,
                    font_packs::megabytes(pack.size()),
                    state,
                    pack.title
                );
            }
            match root {
                Some(root) => println!("cache: {}", root.display()),
                None => println!("cache: none (set STRICTLIB_LEXICON_DIR)"),
            }
            Ok(())
        }
        "path" => {
            let root =
                cache_dir().ok_or("no lexicon cache directory (set STRICTLIB_LEXICON_DIR)")?;
            println!("{}", root.display());
            Ok(())
        }
        "install" | "remove" => {
            let packs = selected(rest)?;
            let root =
                cache_dir().ok_or("no lexicon cache directory (set STRICTLIB_LEXICON_DIR)")?;
            for pack in packs {
                if command == "install" {
                    if pack.is_installed(&root) {
                        println!("{}: already installed", pack.id);
                    } else {
                        pack.install(&root)?;
                    }
                } else {
                    let dir = root.join(pack.id);
                    if dir.exists() {
                        std::fs::remove_dir_all(&dir)
                            .map_err(|error| format!("cannot remove {}: {error}", dir.display()))?;
                        println!("{}: removed", pack.id);
                    }
                }
            }
            Ok(())
        }
        other => Err(format!("unknown 'lexicons' command '{other}'")),
    }
}

fn selected(ids: &[String]) -> Result<Vec<&'static LexiconPack>, String> {
    if ids.is_empty() {
        return Err("name the word lists (see 'strict-ooxml lexicons list'), or 'all'".to_owned());
    }
    if ids.iter().any(|id| id == "all") {
        return Ok(CATALOG.iter().collect());
    }
    ids.iter()
        .map(|id| {
            CATALOG
                .iter()
                .find(|pack| pack.id.eq_ignore_ascii_case(id.trim()))
                .ok_or_else(|| format!("no word list '{id}' (see 'strict-ooxml lexicons list')"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{pack_for, CATALOG};

    #[test]
    fn a_language_tag_finds_its_pack() {
        assert_eq!(pack_for("sr").map(|pack| pack.id), Some("sr"));
        assert_eq!(pack_for("sr-Latn-RS").map(|pack| pack.id), Some("sr"));
        assert!(pack_for("xx").is_none());
        for pack in CATALOG {
            for file in pack.files {
                assert!(file.url.starts_with("https://raw.githubusercontent.com/"));
                assert!(super::super::fonts::decode_sha256(file.sha256).is_some());
            }
            assert!(pack.files.iter().any(|file| file.name == pack.dic));
            assert!(pack.files.iter().any(|file| file.name == pack.aff));
        }
    }
}
