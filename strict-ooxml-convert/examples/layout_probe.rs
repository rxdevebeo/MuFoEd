//! Asks a vision model for the layout of scanned pages and keeps everything it
//! was shown and said, for a person to look at.
//!
//! ```text
//! cargo run -p strict-ooxml-convert --features raster,ocr-ollama \
//!     --example layout_probe -- <document.pdf> --out <dir> [--pages N-M] \
//!     [--scale 2] [--prompt-file <file>]
//! ```
//!
//! For each page it writes `page-NNN.png` (the picture the model saw, the page
//! as displayed, `/Rotate` applied), `page-NNN.txt` (the answer, verbatim) and a
//! line of `probe.tsv` (seconds, picture size, answer length or the error).
//! `prompt.txt` holds the prompt used. Nothing is parsed or judged here: the
//! question is whether a model's blocks - their boxes, kinds and formatting -
//! are good enough to build a document from, and that is answered by reading
//! the answers against the pictures (`hostq` kind `ocr-probe`).
//!
//! The model is `OLLAMA_MODEL` at `OLLAMA_HOST` (`OllamaConfig::from_env`).

/// The layout request the converter uses.
#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
use strict_ooxml_ocr::ollama::LAYOUT_PROMPT;

/// Prints `message` and exits as a usage or environment error.
#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

// One straight script: arguments, then one request per page.
#[cfg(all(feature = "raster", feature = "ocr-ollama"))]
#[allow(clippy::too_many_lines)]
fn main() {
    use std::fmt::Write as _;
    use std::time::Instant;

    use strict_ooxml_ocr::ollama::OllamaVision;
    use strict_ooxml_ocr::Image;
    use strict_ooxml_pdf::raster::{RasterOptions, Rasterizer};
    use strict_ooxml_pdf::PdfLimits;

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut out = None;
    let mut pages = None;
    let mut scale = 2.0_f64;
    let mut prompt = LAYOUT_PROMPT.to_owned();
    let mut index = 0;
    while index < arguments.len() {
        let flag = arguments[index].as_str();
        let value = || {
            arguments
                .get(index + 1)
                .cloned()
                .unwrap_or_else(|| fail(&format!("{flag} requires a value")))
        };
        match flag {
            "--out" => out = Some(value()),
            "--pages" => {
                let text = value();
                let (first, last) = text.split_once('-').unwrap_or((&text, &text));
                match (first.parse::<usize>(), last.parse::<usize>()) {
                    (Ok(first), Ok(last)) if first >= 1 && first <= last => {
                        pages = Some((first, last));
                    }
                    _ => fail(&format!("--pages expects N or N-M, not {text}")),
                }
            }
            "--scale" => {
                scale = value()
                    .parse()
                    .ok()
                    .filter(|scale: &f64| scale.is_finite() && *scale > 0.0 && *scale <= 6.0)
                    .unwrap_or_else(|| fail("--scale expects a number in (0, 6]"));
            }
            "--prompt-file" => {
                let path = value();
                prompt = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| fail(&format!("cannot read {path}: {error}")));
            }
            other if other.starts_with("--") => fail(&format!("unknown option {other}")),
            other => {
                input = Some(other.to_owned());
                index += 1;
                continue;
            }
        }
        index += 2;
    }
    let input = input.unwrap_or_else(|| fail("a PDF path is required"));
    let out = std::path::PathBuf::from(out.unwrap_or_else(|| fail("--out <dir> is required")));
    std::fs::create_dir_all(&out)
        .unwrap_or_else(|error| fail(&format!("cannot create {}: {error}", out.display())));
    let bytes = std::fs::read(&input)
        .unwrap_or_else(|error| fail(&format!("cannot read {input}: {error}")));
    let rasterizer = Rasterizer::new(&bytes, PdfLimits::default())
        .unwrap_or_else(|error| fail(&format!("cannot open {input}: {error}")));
    let count = rasterizer.page_count();
    let (first, last) = pages.unwrap_or((1, count));
    let last = last.min(count);

    let client = OllamaVision::from_env();
    let model = client.config().model.clone();
    let version = client.model_version().unwrap_or_else(|error| {
        fail(&format!(
            "no model answered ({error}): is `ollama serve` running?"
        ))
    });
    println!("model: {model} {version}\npages: {first}-{last} of {count}, scale {scale}");
    let _ = std::fs::write(out.join("prompt.txt"), &prompt);

    let mut table = String::from("page\tseconds\twidth_px\theight_px\tanswer_chars\terror\n");
    let options = RasterOptions {
        scale,
        ..RasterOptions::default()
    };
    for number in first..=last {
        let started = Instant::now();
        let (size, png) = match rasterizer
            .page_size(number, &options)
            .and_then(|size| rasterizer.page_png(number, &options).map(|png| (size, png)))
        {
            Ok(pair) => pair,
            Err(error) => {
                let _ = writeln!(table, "{number}\t0\t0\t0\t0\tcannot rasterize: {error}");
                continue;
            }
        };
        let _ = std::fs::write(out.join(format!("page-{number:03}.png")), &png);
        let answer = client.ask_with_prompt(&prompt, &Image::png(png));
        let seconds = started.elapsed().as_secs_f64();
        match answer {
            Ok((text, _)) => {
                let _ = std::fs::write(out.join(format!("page-{number:03}.txt")), &text);
                let _ = writeln!(
                    table,
                    "{number}\t{seconds:.1}\t{}\t{}\t{}\t",
                    size.width,
                    size.height,
                    text.chars().count()
                );
                println!(
                    "page {number}: {seconds:.1}s, {} chars",
                    text.chars().count()
                );
            }
            Err(error) => {
                let _ = writeln!(
                    table,
                    "{number}\t{seconds:.1}\t{}\t{}\t0\t{error}",
                    size.width, size.height
                );
                println!("page {number}: {error}");
            }
        }
    }
    let _ = std::fs::write(out.join("probe.tsv"), table);
}

#[cfg(not(all(feature = "raster", feature = "ocr-ollama")))]
fn main() {
    eprintln!(
        "this probe needs the `raster` and `ocr-ollama` features:\n  \
         cargo run -p strict-ooxml-convert --features raster,ocr-ollama \
         --example layout_probe -- <document.pdf> --out <dir>"
    );
    std::process::exit(2);
}
