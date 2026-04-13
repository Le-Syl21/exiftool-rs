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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_io_error() {
        let err = Error::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        let s = format!("{}", err);
        assert!(s.contains("I/O error"), "got: {}", s);
        assert!(s.contains("gone"), "got: {}", s);
    }

    #[test]
    fn display_unsupported_file_type() {
        let err = Error::UnsupportedFileType("xyz".into());
        assert_eq!(format!("{}", err), "unsupported file type: xyz");
    }

    #[test]
    fn display_invalid_data() {
        let err = Error::InvalidData("bad header".into());
        assert_eq!(format!("{}", err), "invalid data: bad header");
    }

    #[test]
    fn display_tag_not_found() {
        let err = Error::TagNotFound("FooBar".into());
        assert_eq!(format!("{}", err), "tag not found: FooBar");
    }

    #[test]
    fn display_invalid_tiff_header() {
        let err = Error::InvalidTiffHeader;
        assert_eq!(format!("{}", err), "invalid TIFF header");
    }

    #[test]
    fn display_invalid_exif() {
        let err = Error::InvalidExif("offset overflow".into());
        assert_eq!(format!("{}", err), "invalid EXIF data: offset overflow");
    }

    #[test]
    fn display_invalid_iptc() {
        let err = Error::InvalidIptc("bad record".into());
        assert_eq!(format!("{}", err), "invalid IPTC data: bad record");
    }

    #[test]
    fn display_invalid_xmp() {
        let err = Error::InvalidXmp("malformed xml".into());
        assert_eq!(format!("{}", err), "invalid XMP data: malformed xml");
    }

    #[test]
    fn display_truncated_data() {
        let err = Error::TruncatedData {
            expected: 100,
            actual: 50,
        };
        assert_eq!(
            format!("{}", err),
            "truncated data: expected 100 bytes, got 50"
        );
    }

    #[test]
    fn display_conversion_error() {
        let err = Error::ConversionError("not a number".into());
        assert_eq!(format!("{}", err), "value conversion error: not a number");
    }

    #[test]
    fn io_error_from_conversion() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "no access");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }
}
