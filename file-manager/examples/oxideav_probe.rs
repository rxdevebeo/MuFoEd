//! Decode local files through the exact adapter used by the preview.
use file_manager::media::Info;
// The shared adapter exposes seek/lifecycle methods used by the UI, not this report.
#[allow(dead_code)]
#[path = "../src/media/oxide.rs"]
mod oxide;
fn main() -> Result<(), String> {
    let mut failures = Vec::new();
    for path in std::env::args().skip(1) {
        let path = std::path::Path::new(&path);
        match inspect(path) {
            Ok(report) => println!("{}: {report}", path.display()),
            Err(error) => {
                println!("{}: FAILED {error}", path.display());
                failures.push(error);
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} files failed", failures.len()))
    }
}
fn inspect(path: &std::path::Path) -> Result<String, String> {
    let info = oxide::probe(path)?;
    let mut source = oxide::Source::open(path)?;
    let (mut video, mut audio, mut samples) = (0, 0, 0);
    let mut first = None;
    while let Some(frame) = source.next()? {
        match frame {
            oxide::Decoded::Video { rgba, size, .. } => {
                if first.is_none() {
                    first = Some((size, rgba));
                }
                video += 1;
            }
            oxide::Decoded::Audio { samples: pcm, .. } => {
                samples += pcm.len();
                audio += 1;
            }
        }
    }
    if let Some((size, rgba)) = first {
        image::save_buffer(
            path.with_extension("oxide.png"),
            &rgba,
            u32::try_from(size[0]).map_err(|e| e.to_string())?,
            u32::try_from(size[1]).map_err(|e| e.to_string())?,
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(format!("duration={:.3}s size={:?}, video frames={video}, audio frames={audio}, PCM samples={samples}",info.duration,info.size))
}
