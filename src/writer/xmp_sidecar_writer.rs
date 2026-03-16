//! XMP sidecar file writer — generates standalone .xmp files.

use crate::writer::xmp_writer;

/// Write XMP sidecar file content from properties.
pub fn write_xmp_sidecar(properties: &[xmp_writer::XmpProperty]) -> Vec<u8> {
    xmp_writer::build_xmp(properties).into_bytes()
}
