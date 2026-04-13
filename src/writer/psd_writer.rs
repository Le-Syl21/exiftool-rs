//! PSD/PSB metadata writer.
//!
//! Updates IRB (Image Resource Block) sections: IPTC, XMP, EXIF.

use crate::error::{Error, Result};

/// Rewrite PSD file with updated IRB resources.
pub fn write_psd(
    source: &[u8],
    new_iptc: Option<&[u8]>,
    new_xmp: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if source.len() < 26 || !source.starts_with(b"8BPS") {
        return Err(Error::InvalidData("not a PSD file".into()));
    }

    let version = u16::from_be_bytes([source[4], source[5]]);
    let _is_psb = version == 2;

    let mut output = Vec::with_capacity(source.len());

    // Copy header (26 bytes)
    output.extend_from_slice(&source[..26]);

    // Copy color mode data section
    let mut pos = 26;
    if pos + 4 > source.len() {
        return Err(Error::InvalidData("truncated PSD".into()));
    }
    let color_len = u32::from_be_bytes([
        source[pos],
        source[pos + 1],
        source[pos + 2],
        source[pos + 3],
    ]) as usize;
    output.extend_from_slice(&source[pos..pos + 4 + color_len]);
    pos += 4 + color_len;

    // Rewrite IRB section
    if pos + 4 > source.len() {
        return Err(Error::InvalidData("truncated PSD".into()));
    }
    let irb_len = u32::from_be_bytes([
        source[pos],
        source[pos + 1],
        source[pos + 2],
        source[pos + 3],
    ]) as usize;
    let irb_start = pos + 4;
    let irb_end = irb_start + irb_len;
    pos = irb_start;

    let mut new_irb = Vec::new();
    let mut wrote_iptc = false;
    let mut wrote_xmp = false;

    // Copy existing IRBs, replacing IPTC (0x0404) and XMP (0x0424) if new data provided
    while pos + 12 <= irb_end {
        if &source[pos..pos + 4] != b"8BIM" {
            break;
        }
        let resource_id = u16::from_be_bytes([source[pos + 4], source[pos + 5]]);
        let name_len = source[pos + 6] as usize;
        let name_padded = name_len + 1 + ((name_len + 1) % 2);
        let data_pos = pos + 6 + name_padded;
        if data_pos + 4 > irb_end {
            break;
        }
        let data_len = u32::from_be_bytes([
            source[data_pos],
            source[data_pos + 1],
            source[data_pos + 2],
            source[data_pos + 3],
        ]) as usize;
        let data_padded = data_len + (data_len % 2);
        let block_end = data_pos + 4 + data_padded;

        match resource_id {
            0x0404 if new_iptc.is_some() => {
                write_irb_block(&mut new_irb, 0x0404, new_iptc.unwrap());
                wrote_iptc = true;
            }
            0x0424 if new_xmp.is_some() => {
                write_irb_block(&mut new_irb, 0x0424, new_xmp.unwrap());
                wrote_xmp = true;
            }
            _ => {
                // Copy as-is
                new_irb.extend_from_slice(&source[pos..block_end.min(irb_end)]);
            }
        }

        pos = block_end;
    }

    // Add new blocks if not already written
    if !wrote_iptc {
        if let Some(iptc) = new_iptc {
            write_irb_block(&mut new_irb, 0x0404, iptc);
        }
    }
    if !wrote_xmp {
        if let Some(xmp) = new_xmp {
            write_irb_block(&mut new_irb, 0x0424, xmp);
        }
    }

    // Write new IRB section
    output.extend_from_slice(&(new_irb.len() as u32).to_be_bytes());
    output.extend_from_slice(&new_irb);

    // Copy rest of file (layer data + image data)
    let rest_start = irb_end;
    if rest_start < source.len() {
        output.extend_from_slice(&source[rest_start..]);
    }

    Ok(output)
}

fn write_irb_block(output: &mut Vec<u8>, resource_id: u16, data: &[u8]) {
    output.extend_from_slice(b"8BIM");
    output.extend_from_slice(&resource_id.to_be_bytes());
    output.push(0); // Name length 0
    output.push(0); // Padding
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(data);
    if data.len() % 2 != 0 {
        output.push(0);
    }
}
