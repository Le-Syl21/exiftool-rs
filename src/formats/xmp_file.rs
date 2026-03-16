//! XMP sidecar file reader (.xmp, .xml, .svg, .inx).
//!
//! Reads standalone XMP files as pure XML metadata.

use crate::error::Result;
use crate::metadata::XmpReader;
use crate::tag::Tag;

pub fn read_xmp(data: &[u8]) -> Result<Vec<Tag>> {
    XmpReader::read(data)
}
