//! AAC (Advanced Audio Coding) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_aac(data: &[u8]) -> Result<Vec<Tag>> {
    // AAC ADTS frame header: 7 bytes minimum
    if data.len() < 7 || data[0] != 0xFF || (data[1] != 0xF0 && data[1] != 0xF1) {
        return Err(Error::InvalidData("not an AAC ADTS file".into()));
    }

    // unpack as Perl: N=u32 big-endian from bytes 0-3, n=u16 from bytes 4-5, C=u8 from byte 6
    let t0 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let t1 = u16::from_be_bytes([data[4], data[5]]);
    let _t2 = data[6];

    // Validate: profile type
    // In Perl: $t[0]>>16 & 0x03 = bits 17-16 counting from right (0=LSB) of big-endian u32
    // These correspond to stream bits 14-15 in Perl's Bit016-017 numbering
    // Perl uses ProcessBitStream which reads MSB-first; bits 16-17 from stream start = byte2 bits 0-1 from MSB
    let profile_type = (t0 >> 16) & 0x03; // matches Perl $t[0]>>16 & 0x03
    if profile_type == 3 {
        return Err(Error::InvalidData("reserved AAC profile type".into()));
    }

    // Sampling rate index: stream bits 18-21
    // In Perl's ProcessBitStream: Bit018-021 = byte 2 bits 2-5 from MSB
    // In big-endian u32 t0: byte 2 is bits 15-8. Byte2 bits 2-5 from MSB = t0 bits 13-10 from right.
    // (t0 >> 10) & 0x0F
    let sr_index = (t0 >> 10) & 0x0F;
    if sr_index > 12 {
        return Err(Error::InvalidData("invalid AAC sampling rate index".into()));
    }

    // Channel configuration: stream bits 23-25
    // byte2 bit 7 from MSB (stream bit 23) = t0 bit 8 from right
    // byte3 bits 0-1 from MSB (stream bits 24-25) = t0 bits 7-6 from right
    // (t0 >> 6) & 0x07
    let channel_config = (t0 >> 6) & 0x07;

    let mut tags = Vec::new();

    // ProfileType
    let profile_name = match profile_type {
        0 => "Main",
        1 => "Low Complexity",
        2 => "Scalable Sampling Rate",
        _ => "Unknown",
    };
    tags.push(mktag(
        "AAC",
        "ProfileType",
        "Profile Type",
        Value::String(profile_name.into()),
    ));

    // SampleRate
    let sample_rates = [
        96000u32, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    if let Some(&sr) = sample_rates.get(sr_index as usize) {
        tags.push(mktag("AAC", "SampleRate", "Sample Rate", Value::U32(sr)));
    }

    // Channels
    let channels_str = match channel_config {
        0 => "?",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "5+1",
        7 => "7+1",
        _ => "?",
    };
    tags.push(mktag(
        "AAC",
        "Channels",
        "Channels",
        Value::String(channels_str.into()),
    ));

    // Frame length: bits 30-42 (13 bits)
    // $len = (($t0 << 11) & 0x1800) | (($t1 >> 5) & 0x07ff)
    let len = (((t0 as u64) << 11) & 0x1800) | (((t1 as u64) >> 5) & 0x07FF);
    let len = len as usize;

    // Try to extract Encoder from the filler payload in the frame.
    // Scan the remaining data for a printable ASCII string (like encoder name).
    if len >= 8 && data.len() >= len {
        let frame_data = &data[7..len];
        // Scan for a null-delimited printable string in the frame payload
        // The encoder string is typically in a filler element, null-terminated
        let mut i = 0;
        while i < frame_data.len() {
            // Skip null bytes
            while i < frame_data.len() && frame_data[i] == 0 {
                i += 1;
            }
            let start = i;
            // Read printable bytes
            while i < frame_data.len() && frame_data[i] >= 0x20 && frame_data[i] <= 0x7e {
                i += 1;
            }
            let end = i;
            if end - start >= 4 {
                if let Ok(enc) = std::str::from_utf8(&frame_data[start..end]) {
                    let enc = enc.trim();
                    if enc.len() >= 4 {
                        tags.push(mktag(
                            "AAC",
                            "Encoder",
                            "Encoder",
                            Value::String(enc.into()),
                        ));
                        break;
                    }
                }
            }
            i += 1;
        }
    }

    Ok(tags)
}
