//! MP4/QuickTime metadata writer.
//!
//! Rewrites iTunes-style metadata in the moov/udta/meta/ilst atom tree.
//! Also supports writing XMP in uuid atoms.

use crate::error::{Error, Result};

/// Rewrite an MP4/MOV file with updated metadata.
///
/// `new_tags` is a list of (ilst_key, value) pairs for iTunes metadata.
/// `new_xmp` is optional XMP data to embed as uuid atom.
pub fn write_mp4(
    source: &[u8],
    new_tags: &[(&[u8; 4], &str)],
    new_xmp: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if source.len() < 8 {
        return Err(Error::InvalidData("file too small for MP4".into()));
    }

    let mut output = Vec::with_capacity(source.len());
    let mut pos = 0;
    let mut wrote_metadata = false;

    while pos + 8 <= source.len() {
        let size = u32::from_be_bytes([source[pos], source[pos + 1], source[pos + 2], source[pos + 3]]) as usize;
        let atom_type = &source[pos + 4..pos + 8];

        let actual_size = if size == 0 {
            source.len() - pos
        } else if size == 1 && pos + 16 <= source.len() {
            u64::from_be_bytes([
                source[pos + 8], source[pos + 9], source[pos + 10], source[pos + 11],
                source[pos + 12], source[pos + 13], source[pos + 14], source[pos + 15],
            ]) as usize
        } else {
            size
        };

        if actual_size < 8 || pos + actual_size > source.len() {
            // Copy remainder
            output.extend_from_slice(&source[pos..]);
            break;
        }

        if atom_type == b"moov" {
            // Rewrite moov atom with updated metadata
            let moov_data = &source[pos + 8..pos + actual_size];
            let new_moov = rewrite_moov(moov_data, new_tags, new_xmp)?;
            let new_size = (new_moov.len() + 8) as u32;
            output.extend_from_slice(&new_size.to_be_bytes());
            output.extend_from_slice(b"moov");
            output.extend_from_slice(&new_moov);
            wrote_metadata = true;
        } else {
            // Copy atom as-is
            output.extend_from_slice(&source[pos..pos + actual_size]);
        }

        pos += actual_size;
    }

    if !wrote_metadata && (!new_tags.is_empty() || new_xmp.is_some()) {
        // No moov found (unusual) - create one
        let new_moov = create_moov(new_tags, new_xmp);
        output.extend_from_slice(&new_moov);
    }

    Ok(output)
}

/// Rewrite moov atom contents, updating/creating udta/meta/ilst.
fn rewrite_moov(
    moov_data: &[u8],
    new_tags: &[(&[u8; 4], &str)],
    new_xmp: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(moov_data.len());
    let mut pos = 0;
    let mut wrote_udta = false;

    while pos + 8 <= moov_data.len() {
        let size = u32::from_be_bytes([
            moov_data[pos], moov_data[pos + 1], moov_data[pos + 2], moov_data[pos + 3],
        ]) as usize;
        let atom_type = &moov_data[pos + 4..pos + 8];

        if size < 8 || pos + size > moov_data.len() {
            output.extend_from_slice(&moov_data[pos..]);
            break;
        }

        if atom_type == b"udta" {
            // Rewrite udta with updated meta/ilst
            let udta_content = &moov_data[pos + 8..pos + size];
            let new_udta = rewrite_udta(udta_content, new_tags)?;
            let new_size = (new_udta.len() + 8) as u32;
            output.extend_from_slice(&new_size.to_be_bytes());
            output.extend_from_slice(b"udta");
            output.extend_from_slice(&new_udta);
            wrote_udta = true;
        } else {
            output.extend_from_slice(&moov_data[pos..pos + size]);
        }

        pos += size;
    }

    // Create udta if it didn't exist
    if !wrote_udta && !new_tags.is_empty() {
        let new_udta = create_udta(new_tags);
        output.extend_from_slice(&new_udta);
    }

    // Add XMP uuid atom
    if let Some(xmp) = new_xmp {
        let uuid_xmp: [u8; 16] = [
            0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8,
            0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
        ];
        let uuid_size = (8 + 16 + xmp.len()) as u32;
        output.extend_from_slice(&uuid_size.to_be_bytes());
        output.extend_from_slice(b"uuid");
        output.extend_from_slice(&uuid_xmp);
        output.extend_from_slice(xmp);
    }

    Ok(output)
}

/// Rewrite udta atom, updating meta/ilst.
fn rewrite_udta(udta_data: &[u8], new_tags: &[(&[u8; 4], &str)]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut pos = 0;
    let mut wrote_meta = false;

    while pos + 8 <= udta_data.len() {
        let size = u32::from_be_bytes([
            udta_data[pos], udta_data[pos + 1], udta_data[pos + 2], udta_data[pos + 3],
        ]) as usize;
        let atom_type = &udta_data[pos + 4..pos + 8];

        if size < 8 || pos + size > udta_data.len() {
            output.extend_from_slice(&udta_data[pos..]);
            break;
        }

        if atom_type == b"meta" {
            // Rebuild meta with new ilst
            let meta_content = &udta_data[pos + 8..pos + size];
            let new_meta = rebuild_meta(meta_content, new_tags);
            output.extend_from_slice(&new_meta);
            wrote_meta = true;
        } else {
            output.extend_from_slice(&udta_data[pos..pos + size]);
        }

        pos += size;
    }

    if !wrote_meta && !new_tags.is_empty() {
        let meta = create_meta(new_tags);
        output.extend_from_slice(&meta);
    }

    Ok(output)
}

/// Rebuild meta atom with new ilst content.
fn rebuild_meta(_meta_data: &[u8], new_tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    // meta has 4-byte version/flags then sub-atoms
    let ilst = build_ilst(new_tags);

    let mut meta_content = Vec::new();
    // version/flags
    meta_content.extend_from_slice(&[0, 0, 0, 0]);
    // hdlr atom (required)
    let hdlr = build_hdlr();
    meta_content.extend_from_slice(&hdlr);
    // ilst atom
    let ilst_size = (ilst.len() + 8) as u32;
    meta_content.extend_from_slice(&ilst_size.to_be_bytes());
    meta_content.extend_from_slice(b"ilst");
    meta_content.extend_from_slice(&ilst);

    let meta_size = (meta_content.len() + 8) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&meta_size.to_be_bytes());
    out.extend_from_slice(b"meta");
    out.extend_from_slice(&meta_content);
    out
}

/// Build ilst atom content from tag key-value pairs.
fn build_ilst(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut ilst = Vec::new();

    for (key, value) in tags {
        // Build data atom
        let mut data_content = Vec::new();
        data_content.extend_from_slice(&[0, 0, 0, 1]); // type flags: UTF-8
        data_content.extend_from_slice(&[0, 0, 0, 0]); // reserved
        data_content.extend_from_slice(value.as_bytes());

        let data_size = (data_content.len() + 8) as u32;
        let mut data_atom = Vec::new();
        data_atom.extend_from_slice(&data_size.to_be_bytes());
        data_atom.extend_from_slice(b"data");
        data_atom.extend_from_slice(&data_content);

        // Build item atom
        let item_size = (data_atom.len() + 8) as u32;
        ilst.extend_from_slice(&item_size.to_be_bytes());
        ilst.extend_from_slice(*key);
        ilst.extend_from_slice(&data_atom);
    }

    ilst
}

/// Build hdlr atom for metadata handler.
fn build_hdlr() -> Vec<u8> {
    let mut content = Vec::new();
    content.extend_from_slice(&[0, 0, 0, 0]); // version/flags
    content.extend_from_slice(&[0, 0, 0, 0]); // pre-defined
    content.extend_from_slice(b"mdir");         // handler type
    content.extend_from_slice(&[0; 12]);        // reserved
    content.push(0);                            // name (null-terminated)

    let size = (content.len() + 8) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"hdlr");
    out.extend_from_slice(&content);
    out
}

/// Create a new udta atom with meta/ilst.
fn create_udta(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let meta = create_meta(tags);
    let size = (meta.len() + 8) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"udta");
    out.extend_from_slice(&meta);
    out
}

/// Create a new meta atom with hdlr + ilst.
fn create_meta(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    rebuild_meta(&[], tags)
}

/// Create a minimal moov with udta/meta/ilst.
fn create_moov(tags: &[(&[u8; 4], &str)], xmp: Option<&[u8]>) -> Vec<u8> {
    let mut content = Vec::new();

    if !tags.is_empty() {
        let udta = create_udta(tags);
        content.extend_from_slice(&udta);
    }

    if let Some(xmp_data) = xmp {
        let uuid_xmp: [u8; 16] = [
            0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8,
            0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
        ];
        let uuid_size = (8 + 16 + xmp_data.len()) as u32;
        content.extend_from_slice(&uuid_size.to_be_bytes());
        content.extend_from_slice(b"uuid");
        content.extend_from_slice(&uuid_xmp);
        content.extend_from_slice(xmp_data);
    }

    let size = (content.len() + 8) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"moov");
    out.extend_from_slice(&content);
    out
}

/// Map common tag names to ilst 4-byte keys.
pub fn tag_to_ilst_key(tag: &str) -> Option<[u8; 4]> {
    Some(match tag.to_lowercase().as_str() {
        "title" => [0xA9, b'n', b'a', b'm'],
        "artist" => [0xA9, b'A', b'R', b'T'],
        "album" => [0xA9, b'a', b'l', b'b'],
        "year" | "date" => [0xA9, b'd', b'a', b'y'],
        "comment" => [0xA9, b'c', b'm', b't'],
        "genre" => [0xA9, b'g', b'e', b'n'],
        "composer" | "writer" => [0xA9, b'w', b'r', b't'],
        "encoder" | "encodedby" => [0xA9, b't', b'o', b'o'],
        "grouping" => [0xA9, b'g', b'r', b'p'],
        "lyrics" => [0xA9, b'l', b'y', b'r'],
        "description" => [0xA9, b'd', b'e', b's'],
        "albumartist" => [b'a', b'A', b'R', b'T'],
        "copyright" => [b'c', b'p', b'r', b't'],
        _ => return None,
    })
}
