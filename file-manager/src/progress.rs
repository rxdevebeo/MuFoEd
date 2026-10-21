//! Progress shared between a background operation and the interface, and
//! the streaming copy every operation uses.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// Counters a worker updates and the interface reads.
#[derive(Debug, Default)]
pub struct Progress {
    /// Bytes processed.
    pub bytes_done: AtomicU64,
    /// Bytes expected (0 while unknown).
    pub bytes_total: AtomicU64,
    /// Files processed.
    pub files_done: AtomicU64,
    /// Files expected (0 while unknown).
    pub files_total: AtomicU64,
    /// Set to stop the operation.
    pub cancel: AtomicBool,
    current: Mutex<String>,
}

impl Progress {
    /// Whether the operation should stop.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Notes the item being processed.
    pub fn set_current(&self, name: &str) {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        current.clear();
        current.push_str(name);
    }

    /// The item being processed.
    pub fn current(&self) -> String {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Adds processed bytes.
    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_done.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Counts a finished file.
    pub fn file_done(&self) {
        self.files_done.fetch_add(1, Ordering::Relaxed);
    }

    /// The fraction done, by bytes when known, else by files.
    pub fn fraction(&self) -> f32 {
        let (done, total) = match self.bytes_total.load(Ordering::Relaxed) {
            0 => (
                self.files_done.load(Ordering::Relaxed),
                self.files_total.load(Ordering::Relaxed),
            ),
            total => (self.bytes_done.load(Ordering::Relaxed), total),
        };
        if total == 0 {
            0.0
        } else {
            // A display fraction: precision loss is irrelevant.
            #[allow(clippy::cast_precision_loss)]
            let fraction = (done as f64 / total as f64) as f32;
            fraction.clamp(0.0, 1.0)
        }
    }
}

/// The error a cancelled copy returns.
pub fn cancelled_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled")
}

/// Copies `reader` into `writer` in 1 MiB steps, counting bytes and
/// stopping when cancelled.
pub fn copy(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    progress: &Progress,
) -> std::io::Result<u64> {
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        if progress.cancelled() {
            return Err(cancelled_error());
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        writer.write_all(&buffer[..read])?;
        total += read as u64;
        progress.add_bytes(read as u64);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_counts() {
        let progress = Progress::default();
        progress.bytes_total.store(4, Ordering::Relaxed);
        let mut out = Vec::new();
        let copied = copy(&mut &b"data"[..], &mut out, &progress).expect("copies");
        assert_eq!((copied, out.as_slice()), (4, &b"data"[..]));
        assert!((progress.fraction() - 1.0).abs() < 1e-6);
        progress.cancel.store(true, Ordering::Relaxed);
        assert!(copy(&mut &b"more"[..], &mut Vec::new(), &progress).is_err());
        progress.set_current("x");
        assert_eq!(progress.current(), "x");
    }
}
