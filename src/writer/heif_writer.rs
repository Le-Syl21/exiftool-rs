//! HEIF/HEIC/AVIF metadata writer.
//!
//! HEIF uses ISOBMFF (same as MP4) with additional item property boxes.
//! Metadata is stored in meta/iprp/ipco boxes.
//! We delegate to the MP4 writer for the common atom structure.

use crate::error::Result;
use crate::writer::mp4_writer;

/// Write metadata to HEIF/HEIC/AVIF files.
/// These use the same ISOBMFF container as MP4.
pub fn write_heif(
    source: &[u8],
    new_tags: &[(&[u8; 4], &str)],
    new_xmp: Option<&[u8]>,
) -> Result<Vec<u8>> {
    // HEIF uses the same atom structure as MP4
    mp4_writer::write_mp4(source, new_tags, new_xmp)
}
