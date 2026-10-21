//! Bounded rich clipboard snapshots. Import and external image I/O run on the
//! document worker, after the clipboard's memory has been copied and released.
use std::io::Read as _;
pub(crate) const MAX_BYTES: usize = 32 * 1024 * 1024;
/// The largest clipboard picture taken, in pixels (an 8K screen is 33 M).
const MAX_PIXELS: usize = 40 * 1024 * 1024;
#[derive(Clone, Debug, Default)]
pub(crate) struct Rich {
    pub rtf: Option<Vec<u8>>,
    pub html: Option<Vec<u8>>,
    /// A picture as pixels (a screenshot, an image copied from a viewer),
    /// encoded as PNG.
    pub image: Option<Vec<u8>>,
    /// That picture's width and height in pixels.
    pub image_size: Option<(usize, usize)>,
}

impl Rich {
    fn any(&self) -> bool {
        self.rtf.is_some() || self.html.is_some() || self.image.is_some()
    }
}

/// What the clipboard holds besides plain text: RTF and HTML (Windows reads
/// both, Linux and macOS HTML), and a picture as pixels. `None` when it holds
/// none of them.
pub(crate) fn read() -> Result<Option<Rich>, String> {
    // The Windows clipboard opens once at a time: the formats are read and
    // released before the picture is asked for.
    #[cfg(windows)]
    let (mut rich, error) = match read_formats() {
        Ok(rich) => (rich, None),
        Err(error) => (Rich::default(), Some(error)),
    };
    #[cfg(not(windows))]
    let (mut rich, error) = (
        Rich {
            html: read_html(),
            ..Rich::default()
        },
        None::<String>,
    );
    if let Some((png, size)) = read_image() {
        rich.image = Some(png);
        rich.image_size = Some(size);
    }
    match (rich.any(), error) {
        (true, _) => Ok(Some(rich)),
        (false, Some(error)) => Err(error),
        (false, None) => Ok(None),
    }
}

/// HTML from the platform clipboard (`text/html`, `public.html`).
#[cfg(not(windows))]
fn read_html() -> Option<Vec<u8>> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let html = clipboard.get().html().ok()?;
    (!html.trim().is_empty() && html.len() <= MAX_BYTES).then(|| html.into_bytes())
}

/// The clipboard's picture as PNG and its size, when it holds pixels within
/// the limit.
fn read_image() -> Option<(Vec<u8>, (usize, usize))> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let image = clipboard.get().image().ok()?;
    let pixels = image.width.checked_mul(image.height)?;
    if pixels == 0 || pixels > MAX_PIXELS {
        return None;
    }
    let png = encode_png(image.width, image.height, &image.bytes)?;
    Some((png, (image.width, image.height)))
}

/// RGBA rows as a PNG file.
fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(
        &mut out,
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(windows)]
#[allow(unsafe_code)] // Narrow Win32 ownership boundary; no borrowed handles escape.
fn read_formats() -> Result<Rich, String> {
    use windows_sys::Win32::System::{
        DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
            RegisterClipboardFormatW,
        },
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    };
    struct Close;
    impl Drop for Close {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("clipboard is busy".into());
        }
        let _close = Close;
        let read_format = |name: &str| -> Result<Option<Vec<u8>>, String> {
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let format = RegisterClipboardFormatW(name.as_ptr());
            if format == 0 || IsClipboardFormatAvailable(format) == 0 {
                return Ok(None);
            }
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return Err("clipboard data unavailable".into());
            }
            let size = GlobalSize(handle);
            if size == 0 || size > MAX_BYTES {
                return Err("clipboard format exceeds the size limit".into());
            }
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                return Err("clipboard memory unavailable".into());
            }
            let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
            GlobalUnlock(handle);
            Ok(Some(bytes))
        };
        let rtf = read_format("Rich Text Format");
        let html = read_format("HTML Format");
        if !matches!(rtf, Ok(Some(_))) && !matches!(html, Ok(Some(_))) {
            if let Some(error) = rtf.as_ref().err().or_else(|| html.as_ref().err()) {
                return Err(error.clone());
            }
        }
        Ok(Rich {
            rtf: rtf.ok().flatten(),
            html: html.ok().flatten(),
            image: None,
            image_size: None,
        })
    }
}

/// CF_HTML offsets count UTF-8 bytes. No character slicing or unchecked indices.
pub(crate) fn html_fragment(bytes: &[u8]) -> Result<(&[u8], Option<String>), String> {
    let bytes = bytes.split(|b| *b == 0).next().unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let header = &text[..text.find('<').unwrap_or(text.len())];
    let number = |key: &str| {
        header
            .lines()
            .find_map(|l| l.strip_prefix(key))?
            .trim()
            .parse::<usize>()
            .ok()
    };
    let base = header
        .lines()
        .find_map(|l| l.strip_prefix("SourceURL:"))
        .map(|s| s.trim().to_owned());
    if let (Some(start), Some(end)) = (number("StartFragment:"), number("EndFragment:")) {
        return text
            .get(start..end)
            .map(|s| (s.as_bytes(), base))
            .ok_or_else(|| "invalid CF_HTML offsets".into());
    }
    if let Some(start) = text.find("<!--StartFragment-->") {
        let start = start + "<!--StartFragment-->".len();
        let end = text[start..]
            .find("<!--EndFragment-->")
            .map_or(text.len(), |n| start + n);
        return Ok((&bytes[start..end], base));
    }
    let start = text.find('<').ok_or("clipboard has no HTML fragment")?;
    Ok((&bytes[start..], base))
}
/// A bounded worker-side resolver. The library itself never accesses I/O.
pub(crate) struct Resources(std::time::Instant);
impl Resources {
    pub(crate) fn new() -> Self {
        Self(std::time::Instant::now() + std::time::Duration::from_secs(30))
    }
}
impl strict_ooxml::HtmlResolver for Resources {
    fn resolve(&self, source: &str, max: usize) -> Result<Vec<u8>, String> {
        let remaining = self
            .0
            .checked_duration_since(std::time::Instant::now())
            .ok_or("image loading time limit exceeded")?;
        resource(
            source,
            max,
            remaining.min(std::time::Duration::from_secs(8)),
        )
    }
}
fn resource(source: &str, max: usize, timeout: std::time::Duration) -> Result<Vec<u8>, String> {
    let url = url::Url::parse(source).map_err(|e| e.to_string())?;
    match url.scheme() {
        "file" => {
            let path = url.to_file_path().map_err(|()| "invalid image file URL")?;
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            file.take(max.saturating_add(1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > max {
                return Err("image exceeds size limit".into());
            }
            Ok(bytes)
        }
        "http" | "https" => {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .build()
                .new_agent();
            let mut response = agent.get(url.as_str()).call().map_err(|e| e.to_string())?;
            response
                .body_mut()
                .with_config()
                .limit(max as u64)
                .read_to_vec()
                .map_err(|e| e.to_string())
        }
        _ => Err("unsupported image URL scheme".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_pixels_become_a_png_of_their_size() {
        let rgba: Vec<u8> = (0..3 * 2)
            .flat_map(|n| [n * 40, 0, 255 - n * 40, 255])
            .collect();
        let png = encode_png(3, 2, &rgba).expect("png");
        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let mut reader = decoder.read_info().expect("header");
        let mut pixels = vec![0; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut pixels).expect("frame");
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(&pixels[..info.buffer_size()], rgba.as_slice());
        // Rows shorter than the size say do not make a file.
        assert!(encode_png(3, 2, &rgba[..8]).is_none());
    }

    #[test]
    fn cf_html_uses_byte_offsets_and_rejects_invalid_boundaries() {
        let prefix="Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\nSourceURL:https://example.test/page\r\n";
        let fragment = "<p>Привет 🙂</p>";
        let content =
            format!("{prefix}<html>Пролог<!--StartFragment-->{fragment}<!--EndFragment--></html>");
        let start = content.find(fragment).unwrap();
        let end = start + fragment.len();
        let content = content
            .replacen(
                "StartFragment:0000000000",
                &format!("StartFragment:{start:010}"),
                1,
            )
            .replacen(
                "EndFragment:0000000000",
                &format!("EndFragment:{end:010}"),
                1,
            );
        let (part, base) = html_fragment(content.as_bytes()).unwrap();
        assert_eq!(part, fragment.as_bytes());
        assert_eq!(base.as_deref(), Some("https://example.test/page"));
        assert!(html_fragment(b"StartFragment:999\r\nEndFragment:1000\r\n<p>x</p>").is_err());
        let (part, _) =
            html_fragment(b"<html><!--StartFragment--><b>x</b><!--EndFragment--></html>").unwrap();
        assert_eq!(part, b"<b>x</b>");
    }
}
