//! Host-provided, read-only paged documents. No document-format dependency here.
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
type CancelHook = Box<dyn Fn() + Send + Sync>;
#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    hook: Mutex<Option<CancelHook>>,
}
/// Cancellation shared with a host's parser and layout engine.
#[derive(Clone, Default)]
pub struct Cancellation(Arc<CancelState>);
impl Cancellation {
    /// Whether the preview was replaced or closed.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    /// Register the host's cancellation callback. It runs immediately if already cancelled.
    pub fn on_cancel(&self, callback: impl Fn() + Send + Sync + 'static) {
        let mut hook = self
            .0
            .hook
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.is_cancelled() {
            drop(hook);
            callback();
        } else {
            *hook = Some(Box::new(callback));
        }
    }
    /// Cancel this preview and invoke its registered host callback.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        let hook = self
            .0
            .hook
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(hook) = hook {
            hook();
        }
    }
}
/// One page bitmap, premultiplied RGBA over a white page background.
pub struct Page {
    /// Pixel dimensions.
    pub size: [usize; 2],
    /// Four bytes per pixel, top to bottom.
    pub rgba: Vec<u8>,
}
impl Page {
    /// Decode a host-supplied PNG and fit it inside the requested pixel bound.
    /// # Errors
    /// Returns image decoder failures or invalid size limits.
    pub fn from_png(bytes: &[u8], side: u32) -> Result<Self, String> {
        if !(1..=4096).contains(&side) {
            return Err("Invalid preview size".into());
        }
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let image = if image.width() > side || image.height() > side {
            image.resize(side, side, image::imageops::FilterType::Triangle)
        } else {
            image
        };
        let rgba = image.to_rgba8();
        let dimensions = [rgba.width() as usize, rgba.height() as usize];
        let mut rgba = rgba.into_raw();
        for p in rgba.chunks_exact_mut(4) {
            for c in 0..3 {
                p[c] = u8::try_from(u16::from(p[c]) * u16::from(p[3]) / 255).unwrap_or(255);
            }
        }
        Ok(Self {
            size: dimensions,
            rgba,
        })
    }
}
/// A document stays on its loading thread. Only page bitmaps reach the UI.
pub trait Document {
    /// Number of pages; the first page has index zero.
    fn page_count(&self) -> usize;
    /// Rasterize one page, bounded by `side` pixels on either dimension.
    /// # Errors
    /// Returns a diagnostic for a missing, unsupported or damaged page.
    fn render(&mut self, page: usize, side: u32) -> Result<Page, String>;
}
/// Installed by the host to add its supported document formats to F3 and Ctrl+Q.
pub trait Provider: Send + Sync {
    /// Cheap filename check; no disk IO on the UI thread.
    fn supports(&self, path: &Path) -> bool;
    /// Open read-only on a worker. The result need not be `Send`.
    /// # Errors
    /// Returns the host parser's diagnostic.
    fn open(&self, path: &Path, cancel: &Cancellation) -> Result<Box<dyn Document>, String>;
}
