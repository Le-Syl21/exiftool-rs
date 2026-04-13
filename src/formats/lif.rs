//! Leica Image Format (LIF) reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::Tag;
use crate::value::Value;

pub fn read_lif(data: &[u8]) -> Result<Vec<Tag>> {
    // Validate LIF magic: 0x70 0x00 0x00 0x00 ... 0x2A ... '<' 0x00
    if data.len() < 15
        || data[0] != 0x70
        || data[1] != 0x00
        || data[2] != 0x00
        || data[3] != 0x00
        || data[8] != 0x2A
        || data[13] != b'<'
        || data[14] != 0x00
    {
        return Err(Error::InvalidData("not a LIF file".into()));
    }

    let chunk_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let xml_char_count = u32::from_le_bytes([data[9], data[10], data[11], data[12]]) as usize;
    let xml_byte_len = xml_char_count * 2; // UTF-16LE

    if chunk_size > 100_000_000 {
        return Err(Error::InvalidData("LIF XML block too large".into()));
    }
    if xml_byte_len > chunk_size {
        return Err(Error::InvalidData("corrupted LIF XML block".into()));
    }

    // XML data starts at offset 13 (after the 0x2A byte and 4-byte length, but Perl seeks back 2 bytes
    // from position 15 to read from offset 13)
    let xml_start = 13;
    let xml_end = xml_start + xml_byte_len;
    if xml_end > data.len() {
        return Err(Error::InvalidData("truncated LIF XML block".into()));
    }

    // Decode UTF-16LE to UTF-8
    let utf16_data = &data[xml_start..xml_end];
    let (decoded, _, _) = encoding_rs::UTF_16LE.decode(utf16_data);
    let xml_str = decoded.into_owned();

    // Extract XMP-style tags from the XML
    let mut tags = Vec::new();

    // Try to parse with XmpReader first (handles RDF/XMP fragments)
    if let Ok(xmp_tags) = XmpReader::read(xml_str.as_bytes()) {
        if !xmp_tags.is_empty() {
            tags.extend(xmp_tags);
        }
    }

    // Also extract key attributes from the LIF XML structure directly
    // LIF XML has elements like <Element Name="..." UniqueID="..."> with
    // <ChannelDescription>, <DimensionDescription>, etc.
    lif_extract_xml_tags(&xml_str, &mut tags);

    Ok(tags)
}

/// Extract key tags from LIF XML structure.
fn lif_extract_xml_tags(xml: &str, tags: &mut Vec<Tag>) {
    use xml::reader::{EventReader, XmlEvent};

    let reader = EventReader::from_str(xml);
    let mut element_names: Vec<String> = Vec::new();

    for event in reader {
        match event {
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => {
                let local = name.local_name.clone();

                // Collect attributes of interest
                for attr in &attributes {
                    let attr_name = &attr.name.local_name;
                    let attr_val = &attr.value;
                    if attr_val.is_empty() {
                        continue;
                    }

                    match (local.as_str(), attr_name.as_str()) {
                        ("Element", "Name") => {
                            element_names.push(attr_val.clone());
                            tags.push(mktag(
                                "LIF",
                                "ElementName",
                                "Element Name",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("DimensionDescription", "DimID") => {
                            tags.push(mktag(
                                "LIF",
                                "DimensionID",
                                "Dimension ID",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("DimensionDescription", "NumberOfElements") => {
                            tags.push(mktag(
                                "LIF",
                                "NumberOfElements",
                                "Number Of Elements",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("DimensionDescription", "Length") => {
                            tags.push(mktag(
                                "LIF",
                                "DimensionLength",
                                "Dimension Length",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("DimensionDescription", "Unit") => {
                            tags.push(mktag(
                                "LIF",
                                "DimensionUnit",
                                "Dimension Unit",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("DimensionDescription", "Origin") => {
                            tags.push(mktag(
                                "LIF",
                                "DimensionOrigin",
                                "Dimension Origin",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("ChannelDescription", "LUTName") => {
                            tags.push(mktag(
                                "LIF",
                                "ChannelLUTName",
                                "Channel LUT Name",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("ChannelDescription", "Resolution") => {
                            tags.push(mktag(
                                "LIF",
                                "ChannelResolution",
                                "Channel Resolution",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("ChannelDescription", "BytesInc") => {
                            tags.push(mktag(
                                "LIF",
                                "ChannelBytesInc",
                                "Channel Bytes Inc",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        ("TimeStampList", "NumberOfTimeStamps") => {
                            tags.push(mktag(
                                "LIF",
                                "NumberOfTimeStamps",
                                "Number Of Time Stamps",
                                Value::String(attr_val.clone()),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Ok(XmlEvent::EndElement { .. }) => {}
            Err(_) => break,
            _ => {}
        }
    }
}
