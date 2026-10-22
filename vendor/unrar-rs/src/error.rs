use std::io;

/// Errors returned while reading or extracting an archive.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("not a RAR archive")]
    NotArchive,

    #[error("RAR format {0} is not supported by this build")]
    UnsupportedFormat(u8),

    #[error("unsupported feature: {0}")]
    Unsupported(String),

    #[error("corrupt archive: {0}")]
    Corrupt(String),

    #[error("archive is encrypted and no password was supplied")]
    PasswordRequired,

    #[error("wrong password")]
    WrongPassword,

    #[error("checksum mismatch for {0}")]
    Checksum(String),

    #[error("file {0} is split across volumes and the remaining volumes are missing")]
    MissingVolume(String),

    #[error("unpacked data exceeds the declared size")]
    SizeMismatch,
}

pub type Result<T> = std::result::Result<T, Error>;
