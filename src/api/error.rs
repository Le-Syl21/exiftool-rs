//! The facade's error type: the root crate's `exiftool_rs::Error`, variant
//! for variant, so code matching on it keeps compiling.

use std::fmt;
use std::io;

/// All errors that can occur in exiftool operations.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Io(io::Error),
    UnsupportedFileType(String),
    InvalidData(String),
    TagNotFound(String),
    InvalidTiffHeader,
    InvalidExif(String),
    InvalidIptc(String),
    InvalidXmp(String),
    TruncatedData { expected: usize, actual: usize },
    ConversionError(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::UnsupportedFileType(s) => write!(f, "unsupported file type: {s}"),
            Error::InvalidData(s) => write!(f, "invalid data: {s}"),
            Error::TagNotFound(s) => write!(f, "tag not found: {s}"),
            Error::InvalidTiffHeader => write!(f, "invalid TIFF header"),
            Error::InvalidExif(s) => write!(f, "invalid EXIF data: {s}"),
            Error::InvalidIptc(s) => write!(f, "invalid IPTC data: {s}"),
            Error::InvalidXmp(s) => write!(f, "invalid XMP data: {s}"),
            Error::TruncatedData { expected, actual } => {
                write!(f, "truncated data: expected {expected} bytes, got {actual}")
            }
            Error::ConversionError(s) => write!(f, "value conversion error: {s}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_the_root_crate() {
        assert_eq!(Error::UnsupportedFileType("xyz".into()).to_string(), "unsupported file type: xyz");
        assert_eq!(Error::InvalidTiffHeader.to_string(), "invalid TIFF header");
        assert_eq!(Error::TruncatedData { expected: 100, actual: 50 }.to_string(), "truncated data: expected 100 bytes, got 50");
        let err: Error = io::Error::new(io::ErrorKind::PermissionDenied, "no access").into();
        assert!(matches!(err, Error::Io(_)));
        assert!(err.to_string().contains("no access"));
    }
}
