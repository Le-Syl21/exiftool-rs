use std::io;

/// All errors that can occur in exiftool operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("unsupported file type: {0}")]
    UnsupportedFileType(String),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("tag not found: {0}")]
    TagNotFound(String),

    #[error("invalid TIFF header")]
    InvalidTiffHeader,

    #[error("invalid EXIF data: {0}")]
    InvalidExif(String),

    #[error("invalid IPTC data: {0}")]
    InvalidIptc(String),

    #[error("invalid XMP data: {0}")]
    InvalidXmp(String),

    #[error("truncated data: expected {expected} bytes, got {actual}")]
    TruncatedData { expected: usize, actual: usize },

    #[error("value conversion error: {0}")]
    ConversionError(String),
}

pub type Result<T> = std::result::Result<T, Error>;
