//! RealAudio Metafile (RAM/RPM) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_ram(data: &[u8]) -> Result<Vec<Tag>> {
    // RAM files are text files with URLs, one per line
    // Must start with a valid URL or protocol
    if data.len() < 4 {
        return Err(Error::InvalidData("not a RAM file".into()));
    }

    let text = crate::encoding::decode_utf8_or_latin1(data);
    // Check for valid start: must begin with a URL-like protocol
    let _first_line = text.lines().next().unwrap_or("").trim();
    // Validate: http:// lines must end with real media extensions
    let valid_protocols = [
        "rtsp://", "pnm://", "http://", "rtspt://", "rtspu://", "mmst://", "file://",
    ];
    let has_valid = text.lines().any(|line| {
        let l = line.trim();
        valid_protocols.iter().any(|p| l.starts_with(p))
    });
    if !has_valid && !text.starts_with(".RMF") && !data.starts_with(b".ra\xfd") {
        return Err(Error::InvalidData("not a Real RAM file".into()));
    }

    let mut tags = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Validate http:// URLs
        if line.starts_with("http://")
            && !line.ends_with(".ra")
            && !line.ends_with(".rm")
            && !line.ends_with(".rv")
            && !line.ends_with(".rmvb")
            && !line.ends_with(".smil")
        {
            continue;
        }
        if valid_protocols.iter().any(|p| line.starts_with(p)) {
            tags.push(mktag("Real", "URL", "URL", Value::String(line.into())));
        }
    }

    Ok(tags)
}
