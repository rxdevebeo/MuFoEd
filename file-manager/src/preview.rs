//! What the quick view (Ctrl+Q) and the viewer (F3) show for a file:
//! pictures through the gallery's decoders, 3D models through its model
//! panel, text with its encoding guessed, anything else as hex.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};

use image_gallery::Format;

use crate::entry::Usage;

/// Most text or bytes read for display.
pub const TEXT_LIMIT: usize = 4 << 20;

/// The content of a preview.
#[derive(Clone, Debug)]
pub enum Content {
    /// Decoded text.
    Text {
        /// The text.
        text: String,
        /// The encoding it was read in.
        encoding: &'static str,
        /// Only the start of the file was read.
        truncated: bool,
    },
    /// Raw bytes.
    Hex {
        /// The first bytes.
        bytes: Vec<u8>,
        /// The file's size.
        size: u64,
    },
    /// A picture, RGBA8.
    Picture {
        /// Width and height of the pixels.
        size: [usize; 2],
        /// The pixels.
        rgba: Vec<u8>,
        /// Format and original size.
        description: String,
    },
    /// A 3D model: shown by the gallery's model panel.
    Model(PathBuf),
    /// A folder's contents.
    Folder(Usage),
    /// The file could not be read.
    Failed(String),
}

/// Byte order marks and their encodings.
fn bom(bytes: &[u8]) -> Option<(&'static encoding_rs::Encoding, usize)> {
    encoding_rs::Encoding::for_bom(bytes)
}

/// Whether the bytes look like text: no NUL bytes in the first 8 KiB (and
/// not UTF-16, which a BOM announces).
pub fn looks_like_text(bytes: &[u8]) -> bool {
    bom(bytes).is_some() || !bytes[..bytes.len().min(8192)].contains(&0)
}

/// Decodes text: by its BOM, as UTF-8 when valid, else as Windows-1251
/// (Cyrillic), the most likely legacy encoding for the owner's files.
pub fn decode_text(bytes: &[u8]) -> (String, &'static str) {
    if let Some((encoding, length)) = bom(bytes) {
        let (text, _) = encoding.decode_without_bom_handling(&bytes[length..]);
        return (text.into_owned(), encoding.name());
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), "UTF-8"),
        // A UTF-8 file cut mid-character by the read limit.
        Err(error) if error.error_len().is_none() && bytes.len() - error.valid_up_to() < 4 => (
            String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned(),
            "UTF-8",
        ),
        Err(_) => {
            let (text, _, _) = encoding_rs::WINDOWS_1251.decode(bytes);
            (text.into_owned(), "Windows-1251")
        }
    }
}

/// Reads what to show for `path`; pictures are fitted to `side` pixels.
pub fn load(path: &Path, side: u32) -> Content {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return Content::Failed(error.to_string()),
    };
    if metadata.is_dir() {
        return Content::Folder(crate::entry::usage(path, &AtomicBool::new(false)));
    }
    match Format::from_path(path) {
        Some(Format::Model) => return Content::Model(path.to_path_buf()),
        Some(format) if format.can_read() => {
            let options = image_gallery::decode::LoadOptions {
                vector_fit: Some(side),
                ..image_gallery::decode::LoadOptions::default()
            };
            match image_gallery::decode::load(path, &options) {
                Ok(decoded) => {
                    let description = format!(
                        "{} · {} × {}",
                        decoded.format.name(),
                        decoded.image.width(),
                        decoded.image.height()
                    );
                    let image = if decoded.image.width() > side || decoded.image.height() > side {
                        decoded
                            .image
                            .resize(side, side, image::imageops::FilterType::Triangle)
                    } else {
                        decoded.image
                    };
                    let rgba = image.to_rgba8();
                    return Content::Picture {
                        size: [rgba.width() as usize, rgba.height() as usize],
                        rgba: rgba.into_raw(),
                        description,
                    };
                }
                // A picture extension on something else: show its bytes.
                Err(error) if format != Format::Raw => return Content::Failed(error.to_string()),
                Err(_) => {}
            }
        }
        _ => {}
    }
    let mut bytes = Vec::new();
    match std::fs::File::open(path)
        .and_then(|file| file.take(TEXT_LIMIT as u64).read_to_end(&mut bytes))
    {
        Ok(_) => {}
        Err(error) => return Content::Failed(error.to_string()),
    }
    let truncated = metadata.len() > bytes.len() as u64;
    if looks_like_text(&bytes) {
        let (text, encoding) = decode_text(&bytes);
        Content::Text {
            text,
            encoding,
            truncated,
        }
    } else {
        Content::Hex {
            bytes,
            size: metadata.len(),
        }
    }
}

/// Loads a preview on a worker thread.
pub struct Loading {
    /// The file.
    pub path: PathBuf,
    receiver: Receiver<Content>,
}

impl std::fmt::Debug for Loading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loading")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Loading {
    /// Starts loading `path`.
    pub fn start(path: &Path, side: u32, wake: impl Fn() + Send + 'static) -> Self {
        let (sender, receiver) = channel();
        let file = path.to_path_buf();
        let _ = std::thread::Builder::new()
            .name("file-manager-preview".into())
            .spawn(move || {
                let _ = sender.send(load(&file, side));
                wake();
            });
        Self {
            path: path.to_path_buf(),
            receiver,
        }
    }

    /// The content, once read.
    pub fn poll(&self) -> Option<Content> {
        self.receiver.try_recv().ok()
    }
}

/// Lines of a hex dump: offset, 16 bytes, and their printable characters.
pub fn hex_lines(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
    bytes.chunks(16).enumerate().map(|(row, chunk)| {
        let mut line = format!("{:08X}  ", row * 16);
        for (index, byte) in chunk.iter().enumerate() {
            line.push_str(&format!("{byte:02X} "));
            if index == 7 {
                line.push(' ');
            }
        }
        for index in chunk.len()..16 {
            line.push_str("   ");
            if index == 7 {
                line.push(' ');
            }
        }
        line.push(' ');
        line.extend(chunk.iter().map(|&byte| {
            if byte.is_ascii_graphic() || byte == b' ' {
                char::from(byte)
            } else {
                '.'
            }
        }));
        line
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_encodings_are_guessed() {
        assert_eq!(decode_text("привет".as_bytes()), ("привет".into(), "UTF-8"));
        let cp1251 = encoding_rs::WINDOWS_1251.encode("привет").0.into_owned();
        assert_eq!(decode_text(&cp1251), ("привет".into(), "Windows-1251"));
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "hi".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert!(looks_like_text(&utf16));
        assert_eq!(decode_text(&utf16).0, "hi");
        assert!(!looks_like_text(&[1, 0, 2]));
        // A UTF-8 file cut inside a character is still UTF-8.
        let cut = &"дом".as_bytes()[..5];
        assert_eq!(decode_text(cut), ("д\u{43e}".into(), "UTF-8"));
    }

    #[test]
    fn files_get_the_right_content() {
        let folder =
            std::env::temp_dir().join(format!("file-manager-preview-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("a.txt"), "line\n").expect("text");
        std::fs::write(folder.join("b.bin"), [0u8, 1, 2, 3]).expect("bytes");
        image::RgbImage::new(40, 20)
            .save(folder.join("c.png"))
            .expect("picture");
        assert!(matches!(
            load(&folder.join("a.txt"), 64),
            Content::Text { .. }
        ));
        assert!(matches!(
            load(&folder.join("b.bin"), 64),
            Content::Hex { size: 4, .. }
        ));
        match load(&folder.join("c.png"), 16) {
            Content::Picture { size, .. } => assert_eq!(size, [16, 8]),
            other => panic!("{other:?}"),
        }
        assert!(matches!(load(&folder, 64), Content::Folder(usage) if usage.files == 3));
        assert_eq!(
            hex_lines(&[0x41, 0x00]).next().expect("line").len(),
            8 + 2 + 16 * 3 + 1 + 1 + 2
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
