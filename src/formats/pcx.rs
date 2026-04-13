//! PCX (ZSoft PC Paintbrush) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_pcx(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 128 || data[0] != 0x0A {
        return Err(Error::InvalidData("not a PCX file".into()));
    }

    let mut tags = Vec::new();
    let manufacturer = data[0x00];
    let software_ver = data[0x01];
    let encoding = data[0x02];
    let bpp = data[0x03];
    let xmin = u16::from_le_bytes([data[0x04], data[0x05]]);
    let ymin = u16::from_le_bytes([data[0x06], data[0x07]]);
    let xmax = u16::from_le_bytes([data[0x08], data[0x09]]);
    let ymax = u16::from_le_bytes([data[0x0a], data[0x0b]]);
    let hdpi = u16::from_le_bytes([data[0x0c], data[0x0d]]);
    let vdpi = u16::from_le_bytes([data[0x0e], data[0x0f]]);
    let num_planes = data[0x41];
    let bytes_per_line = u16::from_le_bytes([data[0x42], data[0x43]]);
    let color_mode = u16::from_le_bytes([data[0x44], data[0x45]]);

    let mfr_str = match manufacturer {
        10 => "ZSoft",
        _ => "Unknown",
    };
    tags.push(mktag(
        "PCX",
        "Manufacturer",
        "Manufacturer",
        Value::String(mfr_str.into()),
    ));

    let sw_str = match software_ver {
        0 => "PC Paintbrush 2.5",
        2 => "PC Paintbrush 2.8 (with palette)",
        3 => "PC Paintbrush 2.8 (without palette)",
        4 => "PC Paintbrush for Windows",
        5 => "PC Paintbrush 3.0+",
        _ => "Unknown",
    };
    tags.push(mktag(
        "PCX",
        "Software",
        "Software",
        Value::String(sw_str.into()),
    ));

    let enc_str = match encoding {
        1 => "RLE",
        _ => "Unknown",
    };
    tags.push(mktag(
        "PCX",
        "Encoding",
        "Encoding",
        Value::String(enc_str.into()),
    ));

    tags.push(mktag(
        "PCX",
        "BitsPerPixel",
        "Bits Per Pixel",
        Value::U8(bpp),
    ));
    tags.push(mktag("PCX", "LeftMargin", "Left Margin", Value::U16(xmin)));
    tags.push(mktag("PCX", "TopMargin", "Top Margin", Value::U16(ymin)));
    tags.push(mktag(
        "PCX",
        "ImageWidth",
        "Image Width",
        Value::U16(xmax - xmin + 1),
    ));
    tags.push(mktag(
        "PCX",
        "ImageHeight",
        "Image Height",
        Value::U16(ymax - ymin + 1),
    ));
    tags.push(mktag(
        "PCX",
        "XResolution",
        "X Resolution",
        Value::U16(hdpi),
    ));
    tags.push(mktag(
        "PCX",
        "YResolution",
        "Y Resolution",
        Value::U16(vdpi),
    ));
    tags.push(mktag(
        "PCX",
        "ColorPlanes",
        "Color Planes",
        Value::U8(num_planes),
    ));
    tags.push(mktag(
        "PCX",
        "BytesPerLine",
        "Bytes Per Line",
        Value::U16(bytes_per_line),
    ));

    let cm_str = match color_mode {
        0 => "n/a",
        1 => "Color Palette",
        2 => "Grayscale",
        _ => "Unknown",
    };
    tags.push(mktag(
        "PCX",
        "ColorMode",
        "Color Mode",
        Value::String(cm_str.into()),
    ));

    Ok(tags)
}
