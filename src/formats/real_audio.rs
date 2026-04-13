//! RealAudio format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

/// Parse RealAudio (.ra) files. Mirrors ExifTool's Real.pm ProcessReal for RA.
pub fn read_real_audio(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b".ra\xfd") {
        return Err(Error::InvalidData("not a RealAudio file".into()));
    }

    let mut tags = Vec::new();
    let version = u16::from_be_bytes([data[4], data[5]]);

    // Only support version 4 currently (most common)
    if version != 4 {
        return Ok(tags);
    }

    // AudioV4: starts at offset 8
    let d = &data[8..];
    if d.len() < 40 {
        return Ok(tags);
    }

    let mut pos = 0;
    // Field 0: FourCC1 (4 bytes, undef)
    pos += 4;
    // Field 1: AudioFileSize (int32u)
    pos += 4;
    // Field 2: Version2 (int16u)
    pos += 2;
    // Field 3: HeaderSize (int32u)
    pos += 4;
    // Field 4: CodecFlavorID (int16u)
    pos += 2;
    // Field 5: CodedFrameSize (int32u)
    pos += 4;

    if pos + 4 > d.len() {
        return Ok(tags);
    }
    // Field 6: AudioBytes (int32u)
    let audio_bytes = u32::from_be_bytes([d[pos], d[pos + 1], d[pos + 2], d[pos + 3]]);
    pos += 4;
    tags.push(mktag(
        "Real",
        "AudioBytes",
        "Audio Bytes",
        Value::U32(audio_bytes),
    ));

    if pos + 4 > d.len() {
        return Ok(tags);
    }
    // Field 7: BytesPerMinute (int32u)
    let bpm = u32::from_be_bytes([d[pos], d[pos + 1], d[pos + 2], d[pos + 3]]);
    pos += 4;
    tags.push(mktag(
        "Real",
        "BytesPerMinute",
        "Bytes Per Minute",
        Value::U32(bpm),
    ));

    // Field 8: Unknown (int32u)
    pos += 4;
    // Field 9: SubPacketH (int16u)
    pos += 2;

    if pos + 2 > d.len() {
        return Ok(tags);
    }
    // Field 10: AudioFrameSize (int16u)
    let afs = u16::from_be_bytes([d[pos], d[pos + 1]]);
    pos += 2;
    tags.push(mktag(
        "Real",
        "AudioFrameSize",
        "Audio Frame Size",
        Value::U16(afs),
    ));

    // Field 11: SubPacketSize (int16u)
    pos += 2;
    // Field 12: Unknown (int16u)
    pos += 2;

    if pos + 2 > d.len() {
        return Ok(tags);
    }
    // Field 13: SampleRate (int16u)
    let sr = u16::from_be_bytes([d[pos], d[pos + 1]]);
    pos += 2;
    tags.push(mktag("Real", "SampleRate", "Sample Rate", Value::U16(sr)));

    // Field 14: Unknown (int16u)
    pos += 2;

    if pos + 2 > d.len() {
        return Ok(tags);
    }
    // Field 15: BitsPerSample (int16u)
    let bps = u16::from_be_bytes([d[pos], d[pos + 1]]);
    pos += 2;
    tags.push(mktag(
        "Real",
        "BitsPerSample",
        "Bits Per Sample",
        Value::U16(bps),
    ));

    if pos + 2 > d.len() {
        return Ok(tags);
    }
    // Field 16: Channels (int16u)
    let ch = u16::from_be_bytes([d[pos], d[pos + 1]]);
    pos += 2;
    tags.push(mktag("Real", "Channels", "Channels", Value::U16(ch)));

    if pos >= d.len() {
        return Ok(tags);
    }
    // Field 17: FourCC2Len (int8u)
    let fc2l = d[pos] as usize;
    pos += 1;
    pos += fc2l; // skip FourCC2

    if pos >= d.len() {
        return Ok(tags);
    }
    // Field 19: FourCC3Len (int8u)
    let fc3l = d[pos] as usize;
    pos += 1;
    pos += fc3l; // skip FourCC3

    if pos >= d.len() {
        return Ok(tags);
    }
    // Field 21: Unknown (int8u)
    pos += 1;

    if pos + 2 > d.len() {
        return Ok(tags);
    }
    // Field 22: Unknown (int16u)
    pos += 2;

    // Field 23: TitleLen (int8u)
    if pos >= d.len() {
        return Ok(tags);
    }
    let title_len = d[pos] as usize;
    pos += 1;

    // Field 24: Title (string[TitleLen])
    if pos + title_len <= d.len() && title_len > 0 {
        let title = crate::encoding::decode_utf8_or_latin1(&d[pos..pos + title_len]).to_string();
        tags.push(mktag("Real", "Title", "Title", Value::String(title)));
    }
    pos += title_len;

    // Field 25: ArtistLen (int8u)
    if pos >= d.len() {
        return Ok(tags);
    }
    let artist_len = d[pos] as usize;
    pos += 1;

    // Field 26: Artist
    if pos + artist_len <= d.len() && artist_len > 0 {
        let artist = crate::encoding::decode_utf8_or_latin1(&d[pos..pos + artist_len]).to_string();
        tags.push(mktag("Real", "Artist", "Artist", Value::String(artist)));
    }
    pos += artist_len;

    // Field 27: CopyrightLen (int8u)
    if pos >= d.len() {
        return Ok(tags);
    }
    let copy_len = d[pos] as usize;
    pos += 1;

    // Field 28: Copyright
    if pos + copy_len <= d.len() && copy_len > 0 {
        let copyright = crate::encoding::decode_utf8_or_latin1(&d[pos..pos + copy_len]).to_string();
        tags.push(mktag(
            "Real",
            "Copyright",
            "Copyright",
            Value::String(copyright),
        ));
    }

    Ok(tags)
}
