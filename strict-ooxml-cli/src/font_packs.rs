//! The `fonts` commands over the library's font packs
//! ([`strict_ooxml::font_packs`]): `list`, `install`, `remove`, `needed` and
//! `path`, and the packs a render uses.
//!
//! Nothing is fetched without the user asking: `strict-ooxml fonts install
//! <pack>` or `--fetch-fonts` on a render command. Installed packs a document
//! needs are used by `render` and `to-pdf` without further flags
//! (`--no-font-packs` turns that off).

use std::path::Path;

use strict_ooxml::{FontAsset, FontAssetLimits, StrictDocument};

pub(super) use strict_ooxml::font_packs::{
    cache_dir, cache_root, files_present, needed, pack, read_capped, sha256_file, Pack, PackFile,
    CATALOG,
};

/// What the packs `document` needs contribute: the installed ones' faces, the
/// missing ones fetched first when `fetch` is set and named in a hint
/// otherwise.
pub(super) fn assets_for(
    document: &StrictDocument,
    fetch: bool,
    limits: FontAssetLimits,
) -> Result<Vec<FontAsset>, String> {
    let packs = needed(document);
    if packs.is_empty() {
        return Ok(Vec::new());
    }
    let Some(root) = cache_dir() else {
        eprintln!(
            "note: no font cache directory (set STRICTLIB_FONT_DIR); font packs are not used"
        );
        return Ok(Vec::new());
    };
    let mut assets = Vec::new();
    let mut missing = Vec::new();
    for pack in packs {
        if !pack.is_installed(&root) {
            if fetch {
                install(pack, &root)?;
            } else {
                missing.push(pack);
                continue;
            }
        }
        assets.extend(pack.assets(&root, limits)?);
    }
    if !missing.is_empty() {
        let ids: Vec<&str> = missing.iter().map(|pack| pack.id).collect();
        let size: u64 = missing.iter().map(|pack| pack.size()).sum();
        eprintln!(
            "note: the document has text the bundled fonts do not draw ({}); \
             `strict-ooxml fonts install {}` downloads {} (SIL OFL 1.1), or pass --fetch-fonts",
            ids.join(", "),
            ids.join(" "),
            megabytes(size)
        );
    }
    Ok(assets)
}

pub(super) fn megabytes(bytes: u64) -> String {
    // Display only: the precision lost converting to f64 is far below 0.1 MB.
    #[allow(clippy::cast_precision_loss)]
    let mb = bytes as f64 / 1_000_000.0;
    format!("{mb:.1} MB")
}

/// `fonts list|install|remove|needed|path`.
pub(super) fn run(args: &[String]) -> Result<(), String> {
    let (command, rest) = args
        .split_first()
        .ok_or("'fonts' expects list, install, remove, needed or path")?;
    match command.as_str() {
        "list" => {
            let root = cache_dir();
            for pack in CATALOG {
                let state = match &root {
                    Some(root) if pack.is_installed(root) => "installed",
                    _ => "-",
                };
                println!(
                    "{:<11} {:>8}  {:<9}  {}",
                    pack.id,
                    megabytes(pack.size()),
                    state,
                    pack.title
                );
            }
            match root {
                Some(root) => println!("cache: {}", root.display()),
                None => println!("cache: none (set STRICTLIB_FONT_DIR)"),
            }
            Ok(())
        }
        "path" => {
            let root = cache_dir().ok_or("no font cache directory (set STRICTLIB_FONT_DIR)")?;
            println!("{}", root.display());
            Ok(())
        }
        "install" | "remove" => {
            let packs = selected(rest)?;
            let root = cache_dir().ok_or("no font cache directory (set STRICTLIB_FONT_DIR)")?;
            for pack in packs {
                if command == "install" {
                    if pack.is_installed(&root) {
                        println!("{}: already installed", pack.id);
                    } else {
                        install(pack, &root)?;
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
        "needed" => {
            let [file] = rest else {
                return Err("'fonts needed' expects one document".to_owned());
            };
            let options = strict_ooxml_core::opc::OpenOptions::default()
                .conformance(strict_ooxml::ConformancePolicy::Permissive);
            let document = StrictDocument::open_path(file, &options)
                .map_err(|error| format!("{file}: {error}"))?;
            let root = cache_dir();
            for pack in needed(&document) {
                let state = match &root {
                    Some(root) if pack.is_installed(root) => "installed",
                    _ => "not installed",
                };
                println!("{:<11} {state}", pack.id);
            }
            Ok(())
        }
        other => Err(format!("unknown 'fonts' command '{other}'")),
    }
}

fn selected(ids: &[String]) -> Result<Vec<&'static Pack>, String> {
    if ids.is_empty() {
        return Err("name the packs (see 'strict-ooxml fonts list'), or 'all'".to_owned());
    }
    if ids.iter().any(|id| id == "all") {
        return Ok(CATALOG.iter().collect());
    }
    ids.iter()
        .map(|id| {
            pack(id).ok_or_else(|| format!("no font pack '{id}' (see 'strict-ooxml fonts list')"))
        })
        .collect()
}

/// Downloads the files of `pack` that are not in place into `root/<id>`.
fn install(pack: &Pack, root: &Path) -> Result<(), String> {
    install_files("fonts", pack.id, pack.files(), &root.join(pack.id))
}

/// Downloads the `files` of pack `id` that are not in place into `dir`.
#[cfg(feature = "font-download")]
pub(super) fn install_files(
    what: &str,
    id: &str,
    files: &[PackFile],
    dir: &Path,
) -> Result<(), String> {
    let size: u64 = files.iter().map(|file| file.size).sum();
    eprintln!(
        "{what}: installing {id} ({}) into {}",
        megabytes(size),
        dir.display()
    );
    strict_ooxml::font_packs::install_files(files, dir)?;
    println!("{id}: installed");
    Ok(())
}

#[cfg(not(feature = "font-download"))]
pub(super) fn install_files(
    what: &str,
    id: &str,
    _files: &[PackFile],
    _dir: &Path,
) -> Result<(), String> {
    Err(format!(
        "{what} pack '{id}' cannot be downloaded: this build has no download support (feature font-download)"
    ))
}
