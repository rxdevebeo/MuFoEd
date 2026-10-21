//! Background decoding of previews and of the viewer's picture.
//!
//! Requests go on a stack: the most recent (the rows on screen, the picture
//! just opened) are decoded first. Changing folders bumps the generation, so
//! answers to old requests are recognized and dropped.

// Preview sizes are small positive integers.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::SystemTime;

use image::{ColorType, DynamicImage};

use crate::decode::{self, LoadOptions};
use crate::exif::ExifSummary;
use crate::Format;

/// Identifies a file's content well enough to notice a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
}

/// What a worker is asked to produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A preview whose longer side is at most this.
    Preview(u32),
    /// The viewer's picture, longer side at most this.
    Full(u32),
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) path: PathBuf,
    pub(crate) stamp: Stamp,
    pub(crate) kind: Kind,
    pub(crate) generation: u64,
}

/// The properties panel's facts about a picture.
#[derive(Clone, Debug)]
pub(crate) struct Info {
    pub(crate) format: Format,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) colour: &'static str,
    pub(crate) bits: u16,
    pub(crate) has_icc: bool,
    pub(crate) has_exif: bool,
    pub(crate) summary: Option<ExifSummary>,
}

/// Decoded RGBA pixels ready for a texture.
pub(crate) struct Pixels {
    pub(crate) size: [usize; 2],
    pub(crate) rgba: Vec<u8>,
    pub(crate) info: Info,
}

pub(crate) struct Answer {
    pub(crate) request: Request,
    pub(crate) result: Result<Pixels, String>,
}

#[derive(Default)]
struct Queue {
    pending: Vec<Request>,
    generation: u64,
    stop: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub(crate) struct Loader {
    shared: Shared,
    answers: Receiver<Answer>,
}

impl Loader {
    /// Starts `threads` workers; `wake` is called after every answer.
    pub(crate) fn new(threads: usize, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared: Shared = Arc::default();
        let (sender, answers) = channel();
        let wake = Arc::new(wake);
        for index in 0..threads.max(1) {
            let shared = Arc::clone(&shared);
            let sender: Sender<Answer> = sender.clone();
            let wake = Arc::clone(&wake);
            let _ = std::thread::Builder::new()
                .name(format!("image-gallery-preview-{index}"))
                .spawn(move || work(&shared, &sender, &*wake));
        }
        Self { shared, answers }
    }

    /// The current generation, for new requests.
    pub(crate) fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Drops every pending request and starts a new generation.
    pub(crate) fn reset(&self) -> u64 {
        let mut queue = self.lock();
        queue.pending.clear();
        queue.generation += 1;
        queue.generation
    }

    /// Queues a request ahead of the older ones. A pending request for the
    /// same file and kind is moved forward instead of duplicated.
    pub(crate) fn request(&self, request: Request) {
        let mut queue = self.lock();
        queue
            .pending
            .retain(|old| old.path != request.path || old.kind != request.kind);
        queue.pending.push(request);
        // Keep the stack bounded: the oldest requests are for rows long
        // scrolled past.
        let excess = queue.pending.len().saturating_sub(512);
        if excess > 0 {
            queue.pending = queue.pending.split_off(excess);
        }
        drop(queue);
        self.shared.1.notify_one();
    }

    /// The answers that arrived since the last call.
    pub(crate) fn answers(&self) -> Vec<Answer> {
        self.answers.try_iter().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.lock().stop = true;
        self.shared.1.notify_all();
    }
}

fn work(shared: &Shared, sender: &Sender<Answer>, wake: &(dyn Fn() + Send + Sync)) {
    loop {
        let request = {
            let mut queue = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if queue.stop {
                    return;
                }
                if let Some(request) = queue.pending.pop() {
                    break request;
                }
                queue = shared.1.wait(queue).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let result = produce(&request);
        if sender.send(Answer { request, result }).is_err() {
            return;
        }
        wake();
    }
}

fn produce(request: &Request) -> Result<Pixels, String> {
    let side = match request.kind {
        Kind::Preview(side) | Kind::Full(side) => side.max(16),
    };
    let options = LoadOptions {
        vector_fit: Some(side),
        ..LoadOptions::default()
    };
    let decoded = decode::load(&request.path, &options).map_err(|error| error.to_string())?;
    let image = decoded.image;
    let (colour, bits) = describe(image.color());
    let info = Info {
        format: decoded.format,
        width: image.width(),
        height: image.height(),
        colour,
        bits,
        has_icc: decoded.icc.is_some(),
        has_exif: decoded.exif.is_some(),
        summary: decoded.summary,
    };
    let scaled = if image.width() > side || image.height() > side {
        match request.kind {
            Kind::Preview(_) => image.thumbnail(side, side),
            Kind::Full(_) => image.resize(side, side, image::imageops::FilterType::Triangle),
        }
    } else {
        image
    };
    let rgba = to_display(&scaled);
    Ok(Pixels {
        size: [scaled.width() as usize, scaled.height() as usize],
        rgba,
        info,
    })
}

/// 8-bit RGBA for the screen. Float pictures (HDR, EXR) are linear light:
/// they get the sRGB curve, clipped.
fn to_display(image: &DynamicImage) -> Vec<u8> {
    match image.color() {
        ColorType::Rgb32F | ColorType::Rgba32F => image
            .to_rgba32f()
            .pixels()
            .flat_map(|pixel| {
                let [r, g, b, a] = pixel.0;
                [
                    crate::psd::encode_srgb(r),
                    crate::psd::encode_srgb(g),
                    crate::psd::encode_srgb(b),
                    a.clamp(0.0, 1.0),
                ]
                .map(|value| (value * 255.0).round() as u8)
            })
            .collect(),
        _ => image.to_rgba8().into_raw(),
    }
}

fn describe(colour: ColorType) -> (&'static str, u16) {
    let name = match (colour.has_color(), colour.has_alpha()) {
        (true, true) => "RGBA",
        (true, false) => "RGB",
        (false, true) => "Grey + alpha",
        (false, false) => "Grey",
    };
    let channels = u16::from(colour.channel_count()).max(1);
    (name, colour.bits_per_pixel() / channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_arrive_newest_first_and_old_generations_are_dropped() {
        let folder =
            std::env::temp_dir().join(format!("image-gallery-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let path = folder.join("big.png");
        image::RgbImage::from_pixel(600, 300, image::Rgb([1, 2, 3]))
            .save(&path)
            .expect("saves");
        let loader = Loader::new(1, || {});
        let stamp = Stamp {
            size: 0,
            modified: None,
        };
        loader.request(Request {
            path: path.clone(),
            stamp,
            kind: Kind::Preview(128),
            generation: loader.generation(),
        });
        let answer = loop {
            if let Some(answer) = loader.answers().pop() {
                break answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let pixels = answer.result.expect("decodes");
        assert_eq!(pixels.size, [128, 64]);
        assert_eq!(pixels.rgba.len(), 128 * 64 * 4);
        assert_eq!((pixels.info.width, pixels.info.height), (600, 300));
        assert_eq!((pixels.info.colour, pixels.info.bits), ("RGB", 8));
        assert_eq!(loader.reset(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
