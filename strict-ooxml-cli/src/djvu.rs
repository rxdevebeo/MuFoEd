//! `DjVu` CLI entry points; offline by default, recognition only on explicit request.
use super::{EXIT_ERROR, EXIT_OK, EXIT_PROBLEM};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};
use strict_ooxml::{DjvuDocument, DjvuLimits, DjvuOptions, Mode};
struct Args {
    file: PathBuf,
    out: Option<PathBuf>,
    mode: Mode,
    pages: Option<(usize, usize)>,
    images: bool,
    max_image_pixels: usize,
    ocr: bool,
    model: Option<String>,
    endpoint: Option<String>,
}
impl Args {
    fn parse(args: &[String], dump: bool) -> Result<Self, String> {
        let mut result = Self {
            file: PathBuf::new(),
            out: None,
            mode: Mode::Semantic,
            pages: None,
            images: true,
            max_image_pixels: DjvuLimits::default().max_ink_pixels,
            ocr: false,
            model: None,
            endpoint: None,
        };
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            match flag {
                "--out" | "--mode" | "--pages" | "--ocr-model" | "--ocr-endpoint"
                | "--max-image-pixels"
                    if !dump =>
                {
                    i += 1;
                    let value = args
                        .get(i)
                        .filter(|v| !v.is_empty() && !v.starts_with("--"))
                        .ok_or_else(|| format!("{flag} requires a value"))?;
                    match flag {
                        "--out" => result.out = Some(PathBuf::from(value)),
                        "--mode" => {
                            result.mode = match value.as_str() {
                                "semantic" => Mode::Semantic,
                                "visual" => Mode::Visual,
                                _ => return Err("--mode requires semantic or visual".into()),
                            }
                        }
                        "--pages" => {
                            let (a, b) = value.split_once('-').unwrap_or((value, value));
                            let (start, end) = (
                                a.parse::<usize>().map_err(|_| "invalid page range")?,
                                b.parse::<usize>().map_err(|_| "invalid page range")?,
                            );
                            if start == 0 || start > end {
                                return Err("--pages requires 1-based ascending N-M".into());
                            }
                            result.pages = Some((start, end));
                        }
                        "--max-image-pixels" => {
                            result.max_image_pixels = value
                                .parse()
                                .map_err(|_| "--max-image-pixels requires an integer")?;
                            if result.max_image_pixels == 0 {
                                return Err("--max-image-pixels must be positive".into());
                            }
                        }
                        "--ocr-model" => result.model = Some(value.clone()),
                        _ => result.endpoint = Some(value.clone()),
                    }
                }
                "--no-images" if !dump => result.images = false,
                "--ocr" if !dump => result.ocr = true,
                value if value.starts_with('-') => return Err(format!("unknown option {value}")),
                value => {
                    if !result.file.as_os_str().is_empty() {
                        return Err("accepts one input file".into());
                    }
                    result.file = PathBuf::from(value);
                }
            }
            i += 1;
        }
        if result.file.as_os_str().is_empty() {
            return Err("requires a DjVu path".into());
        }
        if !dump && result.out.is_none() {
            return Err("from-djvu requires --out <file.docx>".into());
        }
        if !result.ocr && (result.model.is_some() || result.endpoint.is_some()) {
            return Err("OCR configuration requires --ocr".into());
        }
        #[cfg(not(feature = "ocr-ollama"))]
        if result.ocr {
            return Err(
                "--ocr requires a build with --features ocr-ollama (no model request was made)"
                    .into(),
            );
        }
        Ok(result)
    }
}
/// Maps `DjVu` metadata keys to `docProps/core.xml` fields (SC-D9 / L4).
fn core_properties(metadata: &strict_ooxml::DjvuMetadata) -> Option<strict_ooxml::CoreProperties> {
    let props = strict_ooxml::CoreProperties {
        title: metadata.get("Title").map(str::to_owned),
        creator: metadata.get("Author").map(str::to_owned),
        subject: metadata.get("Subject").map(str::to_owned),
        description: metadata.get("Description").map(str::to_owned),
        keywords: metadata.get("Keywords").map(str::to_owned),
    };
    (!props.is_empty()).then_some(props)
}

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len > max as u64 {
        return Err("DjVu limit exceeded: max_file_bytes".into());
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("DjVu limit exceeded: max_file_bytes".into());
    }
    Ok(bytes)
}
fn open(path: &Path, limits: DjvuLimits) -> Result<DjvuDocument, String> {
    let input = read_bounded(path, limits.max_file_bytes)?;
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    let base = canonical.parent().ok_or("input has no directory")?;
    let mut remaining = limits.max_file_bytes - input.len();
    DjvuDocument::open_with_resolver(&input, limits, |id| {
        let path = base
            .join(id)
            .canonicalize()
            .map_err(|e| strict_ooxml::DjvuError::Component(format!("{id}: {e}")))?;
        if path.parent() != Some(base) {
            return Err(strict_ooxml::DjvuError::Component(
                "component escapes the input directory".into(),
            ));
        }
        let bytes = read_bounded(&path, remaining).map_err(strict_ooxml::DjvuError::Component)?;
        remaining -= bytes.len();
        Ok(bytes)
    })
    .map_err(|e| e.to_string())
}
pub(super) fn run(args: &[String], dump: bool) -> ExitCode {
    match execute(args, dump) {
        Ok(false) => ExitCode::from(EXIT_OK),
        Ok(true) => ExitCode::from(EXIT_PROBLEM),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}
fn execute(args: &[String], dump: bool) -> Result<bool, String> {
    let args = Args::parse(args, dump)?;
    let mut reader = open(
        &args.file,
        DjvuLimits {
            max_ink_pixels: args.max_image_pixels,
            ..DjvuLimits::default()
        },
    )?;
    if dump {
        print!("{}", reader.dump());
        for page in reader.pages() {
            println!(
                "page {}: {}x{} px, {} dpi, rotation {}, text {}",
                page.number,
                page.info.width,
                page.info.height,
                page.info.dpi,
                page.info.rotation,
                match &page.text {
                    strict_ooxml::DjvuTextLayer::Readable(_) => "readable",
                    strict_ooxml::DjvuTextLayer::Empty => "empty",
                    strict_ooxml::DjvuTextLayer::Unusable { .. } => "unusable",
                }
            );
        }
        return Ok(false);
    }
    let mut options = DjvuOptions::default()
        .mode(args.mode)
        .embed_images(args.images);
    options.pages = args.pages;
    #[cfg(feature = "ocr-ollama")]
    if args.ocr {
        let mut config = strict_ooxml::OllamaConfig::from_env();
        if let Some(model) = args.model {
            config.model = model;
        }
        if let Some(endpoint) = args.endpoint {
            config.endpoint = endpoint;
        }
        let vision = std::sync::Arc::new(strict_ooxml::OllamaVision::new(config));
        options.text_recovery = Some(vision.clone());
        options.figure_classifier = Some(vision);
    }
    let converted = strict_ooxml::convert_djvu(&mut reader, &options).map_err(|e| e.to_string())?;
    let mut bag = strict_ooxml::MediaBag::new();
    for (id, bytes) in &converted.media {
        bag.insert(id.clone(), bytes.clone());
    }
    let written = strict_ooxml::write_package(
        &converted.document,
        Some(&bag),
        &strict_ooxml::WriteOptions {
            core_properties: core_properties(&reader.metadata),
            ..strict_ooxml::WriteOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let out = args.out.ok_or("missing output path")?;
    if out.canonicalize().ok() == args.file.canonicalize().ok() {
        return Err("output must differ from the input file".into());
    }
    std::fs::write(&out, &written.bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    print!("{}", converted.report);
    for loss in written.report.losses() {
        eprintln!("{loss}");
    }
    println!("wrote {} ({} bytes)", out.display(), written.bytes.len());
    strict_ooxml::verify_no_silent_loss(&written.report)?;
    Ok(!converted.report.is_lossless() || strict_ooxml::dropped_count(&written.report) > 0)
}
