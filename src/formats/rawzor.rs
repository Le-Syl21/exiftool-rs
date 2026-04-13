//! Rawzor compressed RAW format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_rawzor(data: &[u8]) -> Result<Vec<Tag>> {
    // Header:
    //  0: "rawzor" (6 bytes)
    //  6: int16u Required SDK version
    //  8: int16u Creator SDK version
    // 10: int64u RWZ file size
    // 18: int64u Original file size
    // 26: reserved (12 bytes)
    // 38: int64u metadata offset
    if data.len() < 46 || !data.starts_with(b"rawzor") {
        return Err(Error::InvalidData("not a Rawzor file".into()));
    }

    let mut tags = Vec::new();

    let req_vers = u16::from_le_bytes([data[6], data[7]]);
    let creator_vers = u16::from_le_bytes([data[8], data[9]]);
    let rwz_size = u64::from_le_bytes([
        data[10], data[11], data[12], data[13], data[14], data[15], data[16], data[17],
    ]);
    let orig_size = u64::from_le_bytes([
        data[18], data[19], data[20], data[21], data[22], data[23], data[24], data[25],
    ]);

    tags.push(mktag(
        "Rawzor",
        "RawzorRequiredVersion",
        "Rawzor Required Version",
        Value::String(format!("{:.2}", req_vers as f64 / 100.0)),
    ));
    tags.push(mktag(
        "Rawzor",
        "RawzorCreatorVersion",
        "Rawzor Creator Version",
        Value::String(format!("{:.2}", creator_vers as f64 / 100.0)),
    ));
    tags.push(mktag(
        "Rawzor",
        "OriginalFileSize",
        "Original File Size",
        Value::String(orig_size.to_string()),
    ));
    if rwz_size > 0 {
        let factor = orig_size as f64 / rwz_size as f64;
        tags.push(mktag(
            "Rawzor",
            "CompressionFactor",
            "Compression Factor",
            Value::String(format!("{:.2}", factor)),
        ));
    }

    // Check version - max supported is 1.99 (199)
    if req_vers > 199 {
        // Version too new, just return what we have
        return Ok(tags);
    }

    // Metadata decompression requires bzip2 which is not available.
    // The Perl ExifTool issues a warning in this case too.
    // We still return the header-level tags.

    Ok(tags)
}
