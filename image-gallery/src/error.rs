//! The crate's error type.

use std::path::PathBuf;

/// Why reading, processing or writing a picture failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file concerned.
        path: PathBuf,
        /// The operating system's error.
        source: std::io::Error,
    },
    /// The format is recognized but cannot be read by this build.
    #[error("{0}: reading this format is not supported")]
    Unsupported(&'static str),
    /// The format is not recognized at all.
    #[error("unknown image format")]
    UnknownFormat,
    /// The picture is larger than the configured limit.
    #[error("picture of {width}x{height} exceeds the size limit")]
    TooLarge {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// The decoder or encoder rejected the data.
    #[error("{0}")]
    Codec(String),
    /// The operation's parameters do not fit the picture.
    #[error("{0}")]
    Invalid(String),
    /// A decoder panicked; the panic was contained.
    #[error("the decoder failed unexpectedly: {0}")]
    Panic(String),
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl From<image::ImageError> for Error {
    fn from(error: image::ImageError) -> Self {
        match error {
            image::ImageError::Unsupported(detail) => Self::Codec(detail.to_string()),
            image::ImageError::Limits(_) => {
                Self::Codec("the picture exceeds the size limit".into())
            }
            other => Self::Codec(other.to_string()),
        }
    }
}

/// Runs `body`, turning a panic inside a third-party codec into an error.
pub(crate) fn contain<T>(body: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(payload) => {
            let text = payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".to_owned());
            Err(Error::Panic(text))
        }
    }
}
