//! JPEG XR / HD Photo format reader.

use crate::error::{Error, Result};
use crate::tag::Tag;

pub fn read_jxr(data: &[u8]) -> Result<Vec<Tag>> {
    // JXR is TIFF-based: "II" + 0xBC byte at offset 2
    // The TIFF reader handles IFD parsing; JXR uses standard EXIF IFD tags
    // plus HD Photo-specific tags (0xBC01-0xBC82) which are in the EXIF tag tables.
    if data.len() < 8 || data[0] != b'I' || data[1] != b'I' || data[2] != 0xBC {
        return Err(Error::InvalidData("not a JPEG XR file".into()));
    }

    // Check version byte (offset 3)
    if data[3] > 1 {
        return Err(Error::InvalidData(format!(
            "JPEG XR version {} not supported",
            data[3]
        )));
    }

    // JXR uses TIFF IFD structure but with magic 0xBC instead of 0x2A (42).
    // The TIFF reader only accepts magic 42/43/0x55. Patch the magic bytes
    // to standard TIFF so the reader can parse the IFDs.
    let mut patched = data.to_vec();
    patched[2] = 0x2A; // Standard TIFF magic (42)
    patched[3] = 0x00;

    crate::formats::tiff::read_tiff(&patched)
}
