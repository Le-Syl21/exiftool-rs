//! Ogg Vorbis / Opus / FLAC-in-Ogg metadata writer.
//!
//! The Vorbis comments of an Ogg file are a header packet of its first
//! logical stream: the `\x03vorbis` packet (Vorbis), `OpusTags` (Opus), or
//! the `VORBIS_COMMENT` metadata block packet (FLAC). ExifTool does not
//! write Ogg; this writer edits that packet with
//! [`edit_vorbis_comment`](super::flac::edit_vorbis_comment)
//! and re-pages it the way `vorbiscomment` does:
//!
//! - the pages from the one where the comment packet starts to the first
//!   page that ends on a packet boundary after it are rebuilt (they hold
//!   only header packets; the other packets on them are copied unchanged);
//! - the later pages of that stream are renumbered when the page count
//!   changed, and every rewritten page gets a new CRC;
//! - pages of other streams, and every audio packet, are copied unchanged.
//!
//! Header pages mixing in audio data (a non-zero granule position) are
//! refused rather than re-paged.

use crate::api::error::{Error, Result};
use super::flac::{edit_vorbis_comment, VorbisChanges};

fn err(msg: impl Into<String>) -> Error {
    Error::InvalidData(format!("Ogg: {}", msg.into()))
}

/// CRC-32 of an Ogg page (polynomial 0x04C11DB7, not reflected, init 0).
fn ogg_crc(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 {
                    (r << 1) ^ 0x04C1_1DB7
                } else {
                    r << 1
                };
            }
            *e = r;
        }
        t
    });
    data.iter().fold(0u32, |crc, &b| {
        (crc << 8) ^ table[((crc >> 24) as u8 ^ b) as usize]
    })
}

#[derive(Debug, Clone)]
struct Page {
    start: usize,
    end: usize,
    header_type: u8,
    granule: u64,
    serial: u32,
    seq: u32,
    /// Lacing values.
    segs: Vec<u8>,
    /// Offset of the page data.
    data: usize,
}

impl Page {
    fn ends_packet(&self) -> bool {
        self.segs.last().is_some_and(|&s| s < 255)
    }
}

/// The complete pages of a file, and the offset of a truncated last page
/// (the end of the file when there is none).
fn parse_pages(src: &[u8]) -> Result<(Vec<Page>, usize)> {
    let mut pages = Vec::new();
    let mut pos = 0;
    while pos < src.len() {
        if src.len() - pos < 27 {
            break;
        }
        if &src[pos..pos + 4] != b"OggS" {
            return Err(err(format!("no Ogg page at offset {pos}")));
        }
        if src[pos + 4] != 0 {
            return Err(err("unknown Ogg version"));
        }
        let n = src[pos + 26] as usize;
        if pos + 27 + n > src.len() {
            break;
        }
        let segs = src[pos + 27..pos + 27 + n].to_vec();
        let data = pos + 27 + n;
        let len: usize = segs.iter().map(|&s| s as usize).sum();
        if data + len > src.len() {
            break;
        }
        let mut g = [0u8; 8];
        g.copy_from_slice(&src[pos + 6..pos + 14]);
        pages.push(Page {
            start: pos,
            end: data + len,
            header_type: src[pos + 5],
            granule: u64::from_le_bytes(g),
            serial: u32::from_le_bytes([
                src[pos + 14],
                src[pos + 15],
                src[pos + 16],
                src[pos + 17],
            ]),
            seq: u32::from_le_bytes([src[pos + 18], src[pos + 19], src[pos + 20], src[pos + 21]]),
            segs,
            data,
        });
        pos = data + len;
    }
    if pos < src.len() && !src[pos..].starts_with(b"OggS") {
        return Err(err(format!("no Ogg page at offset {pos}")));
    }
    Ok((pages, pos))
}

/// A packet of the edited stream: its bytes and the stream-page indexes it
/// starts and ends on.
struct Packet {
    bytes: Vec<u8>,
    first: usize,
    last: usize,
}

/// Build an Ogg page.
fn page_bytes(
    header_type: u8,
    granule: u64,
    serial: u32,
    seq: u32,
    segs: &[u8],
    body: &[u8],
) -> Vec<u8> {
    let mut p = Vec::with_capacity(27 + segs.len() + body.len());
    p.extend_from_slice(b"OggS");
    p.push(0);
    p.push(header_type);
    p.extend_from_slice(&granule.to_le_bytes());
    p.extend_from_slice(&serial.to_le_bytes());
    p.extend_from_slice(&seq.to_le_bytes());
    p.extend_from_slice(&[0; 4]);
    p.push(segs.len() as u8);
    p.extend_from_slice(segs);
    p.extend_from_slice(body);
    let crc = ogg_crc(&p);
    p[22..26].copy_from_slice(&crc.to_le_bytes());
    p
}

/// Page a list of header packets (granule 0 where a packet ends, -1 on a
/// page no packet ends on).
fn paginate(packets: &[Vec<u8>], serial: u32, first_seq: u32, first_flags: u8) -> Vec<Vec<u8>> {
    // (lacing value, ends a packet, bytes) for every segment.
    let mut segs: Vec<(u8, bool, &[u8])> = Vec::new();
    for p in packets {
        let mut rest: &[u8] = p;
        loop {
            let n = rest.len().min(255);
            let last = n < 255;
            segs.push((n as u8, last, &rest[..n]));
            rest = &rest[n..];
            if last {
                break;
            }
        }
    }
    let mut pages = Vec::new();
    let mut continued = false;
    for (seq, chunk) in (first_seq..).zip(segs.chunks(255)) {
        let lacing: Vec<u8> = chunk.iter().map(|s| s.0).collect();
        let body: Vec<u8> = chunk.iter().flat_map(|s| s.2.iter().copied()).collect();
        let ends = chunk.iter().any(|s| s.1);
        let mut flags = if continued { 0x01 } else { 0 };
        if seq == first_seq {
            // Only "beginning of stream" carries over to the first page.
            flags |= first_flags & 0x02;
        }
        let granule = if ends { 0 } else { u64::MAX };
        pages.push(page_bytes(flags, granule, serial, seq, &lacing, &body));
        continued = !chunk.last().is_some_and(|s| s.1);
    }
    pages
}

/// Which header packet holds the comments, and how to rewrite it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    Vorbis,
    Opus,
    Flac,
}

fn rewrite_comment_packet(codec: Codec, packet: &[u8], changes: &VorbisChanges) -> Result<Vec<u8>> {
    match codec {
        Codec::Vorbis => {
            let (c, used) = edit_vorbis_comment(&packet[7..], changes)?;
            let mut out = packet[..7].to_vec();
            out.extend(c);
            // The framing bit (and anything after it) stays.
            out.extend_from_slice(&packet[7 + used..]);
            Ok(out)
        }
        Codec::Opus => {
            let (c, used) = edit_vorbis_comment(&packet[8..], changes)?;
            let mut out = packet[..8].to_vec();
            out.extend(c);
            out.extend_from_slice(&packet[8 + used..]);
            Ok(out)
        }
        Codec::Flac => {
            let (c, used) = edit_vorbis_comment(&packet[4..], changes)?;
            let mut body = c;
            body.extend_from_slice(&packet[4 + used..]);
            if body.len() >= 1 << 24 {
                return Err(err("comment block too large"));
            }
            let n = body.len();
            let mut out = vec![packet[0], (n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend(body);
            Ok(out)
        }
    }
}

/// Rewrite an Ogg file with the Vorbis comments of its first stream edited.
pub fn write_ogg(source: &[u8], changes: &VorbisChanges) -> Result<Vec<u8>> {
    if source.len() < 27 || !source.starts_with(b"OggS") {
        return Err(Error::InvalidData("not an OGG file".into()));
    }
    if changes.is_empty() {
        return Ok(source.to_vec());
    }
    let (pages, tail) = parse_pages(source)?;
    let Some(serial) = pages.first().map(|p| p.serial) else {
        return Err(err("truncated first Ogg page"));
    };
    let stream: Vec<usize> = (0..pages.len())
        .filter(|&i| pages[i].serial == serial)
        .collect();

    // Collect packets until the comment packet is found and a page ends on
    // a packet boundary after it.
    let mut packets: Vec<Packet> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut cur_first = 0;
    let mut codec: Option<Codec> = None;
    let mut comment_idx: Option<usize> = None;
    let mut region_end: Option<usize> = None;
    'pages: for (si, &pi) in stream.iter().enumerate() {
        let p = &pages[pi];
        let mut off = p.data;
        for &s in &p.segs {
            if cur.is_empty() {
                cur_first = si;
            }
            cur.extend_from_slice(&source[off..off + s as usize]);
            off += s as usize;
            if s < 255 {
                let bytes = std::mem::take(&mut cur);
                let idx = packets.len();
                if idx == 0 {
                    codec = if bytes.starts_with(b"\x01vorbis") {
                        Some(Codec::Vorbis)
                    } else if bytes.starts_with(b"OpusHead") {
                        Some(Codec::Opus)
                    } else if bytes.starts_with(b"\x7fFLAC") {
                        Some(Codec::Flac)
                    } else {
                        return Err(err("unknown codec in the first Ogg stream"));
                    };
                } else if comment_idx.is_none() {
                    let is_comment = match codec {
                        Some(Codec::Vorbis) => bytes.starts_with(b"\x03vorbis"),
                        Some(Codec::Opus) => bytes.starts_with(b"OpusTags"),
                        Some(Codec::Flac) => bytes.first().is_some_and(|b| b & 0x7F == 4),
                        None => false,
                    };
                    if is_comment {
                        comment_idx = Some(idx);
                    } else if codec != Some(Codec::Flac) || idx > 64 {
                        return Err(err("no comment header where expected"));
                    } else if bytes.first().is_some_and(|b| b & 0x80 != 0) {
                        return Err(err("no VORBIS_COMMENT block in this FLAC stream"));
                    }
                }
                packets.push(Packet {
                    bytes,
                    first: cur_first,
                    last: si,
                });
            }
        }
        if comment_idx.is_some() && p.ends_packet() && cur.is_empty() {
            region_end = Some(si);
            break 'pages;
        }
    }
    let (Some(codec), Some(ci), Some(re)) = (codec, comment_idx, region_end) else {
        return Err(err("no complete comment header"));
    };
    // The region starts on the page where the comment packet starts, moved
    // back while that page continues an earlier packet.
    let mut rs = packets[ci].first;
    while rs > 0 && pages[stream[rs]].header_type & 0x01 != 0 {
        rs -= 1;
    }
    for &pi in &stream[rs..=re] {
        let g = pages[pi].granule;
        if g != 0 && g != u64::MAX {
            return Err(err("audio data shares the comment header pages"));
        }
    }
    let region_packets: Vec<Vec<u8>> = packets
        .iter()
        .enumerate()
        .filter(|(_, p)| p.first >= rs && p.last <= re)
        .map(|(i, p)| {
            if i == ci {
                rewrite_comment_packet(codec, &p.bytes, changes)
            } else {
                Ok(p.bytes.clone())
            }
        })
        .collect::<Result<_>>()?;
    let first = &pages[stream[rs]];
    let new_pages = paginate(&region_packets, serial, first.seq, first.header_type);
    let delta = new_pages.len() as i64 - (re - rs + 1) as i64;

    let mut out = Vec::with_capacity(source.len() + 1024);
    let mut si = 0;
    for p in &pages {
        if p.serial != serial {
            out.extend_from_slice(&source[p.start..p.end]);
            continue;
        }
        let idx = si;
        si += 1;
        if idx < rs {
            out.extend_from_slice(&source[p.start..p.end]);
        } else if idx == rs {
            for np in &new_pages {
                out.extend_from_slice(np);
            }
        } else if idx <= re {
        } else if delta == 0 {
            out.extend_from_slice(&source[p.start..p.end]);
        } else {
            let seq =
                u32::try_from(p.seq as i64 + delta).map_err(|_| err("page sequence overflow"))?;
            let mut b = source[p.start..p.end].to_vec();
            b[18..22].copy_from_slice(&seq.to_le_bytes());
            b[22..26].fill(0);
            let crc = ogg_crc(&b);
            b[22..26].copy_from_slice(&crc.to_le_bytes());
            out.extend(b);
        }
    }
    // A truncated last page (a cut sample) is copied; its sequence number
    // follows the others (its CRC was already wrong).
    let mut rest = source[tail..].to_vec();
    if delta != 0 && rest.len() >= 22 && rest[14..18] == serial.to_le_bytes() {
        let seq = u32::from_le_bytes([rest[18], rest[19], rest[20], rest[21]]);
        let seq = u32::try_from(seq as i64 + delta).map_err(|_| err("page sequence overflow"))?;
        rest[18..22].copy_from_slice(&seq.to_le_bytes());
    }
    out.extend(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(items: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(3u32.to_le_bytes());
        out.extend(b"ven");
        out.extend((items.len() as u32).to_le_bytes());
        for c in items {
            out.extend((c.len() as u32).to_le_bytes());
            out.extend(c.as_bytes());
        }
        out
    }

    /// A Vorbis-like stream: id page, comment+setup page, two audio pages.
    fn sample() -> Vec<u8> {
        let id = b"\x01vorbis-id-header".to_vec();
        let mut com = b"\x03vorbis".to_vec();
        com.extend(comment(&["TITLE=Old", "ARTIST=A"]));
        com.push(1);
        let setup = b"\x05vorbis-setup".to_vec();
        let mut f = Vec::new();
        f.extend(paginate(&[id], 7, 0, 0x02).concat());
        f.extend(paginate(&[com, setup], 7, 1, 0).concat());
        let a1 = page_bytes(0, 100, 7, 2, &[5], b"AUD-1");
        let a2 = page_bytes(0x04, 200, 7, 3, &[5], b"AUD-2");
        f.extend(a1);
        f.extend(a2);
        f
    }

    fn read(file: &[u8]) -> Vec<(String, String)> {
        crate::api::ExifTool::new()
            .extract_info_from_bytes(file, std::path::Path::new(""))
            .unwrap()
            .into_iter()
            // (the comments, not what ExifTool reads in the fake
            // identification header)
            .filter(|t| t.group.family0 == "Vorbis" && !matches!(t.name.as_str(), "VorbisVersion" | "AudioChannels" | "SampleRate" | "MaxBitrate" | "NominalBitrate" | "MinBitrate"))
            .map(|t| (t.name, t.print_value))
            .collect()
    }

    fn check_pages(file: &[u8]) {
        let (pages, _) = parse_pages(file).unwrap();
        for (i, p) in pages.iter().enumerate() {
            assert_eq!(p.seq as usize, i);
            let mut b = file[p.start..p.end].to_vec();
            let crc = u32::from_le_bytes(b[22..26].try_into().unwrap());
            b[22..26].fill(0);
            assert_eq!(ogg_crc(&b), crc);
        }
    }

    #[test]
    fn crc_matches_libogg() {
        // A known page: libogg's CRC of "OggS" + zeros is stable.
        assert_eq!(ogg_crc(b""), 0);
        assert_eq!(ogg_crc(b"\x00"), 0);
        assert_eq!(ogg_crc(b"\x01"), 0x04C1_1DB7);
    }

    #[test]
    fn edits_comments_and_repages() {
        let src = sample();
        check_pages(&src);
        let big = "x".repeat(70_000);
        let ch = VorbisChanges {
            set: vec![("Title".into(), big.clone())],
            delete: vec!["artist".into()],
            ..Default::default()
        };
        let out = write_ogg(&src, &ch).unwrap();
        check_pages(&out);
        assert_eq!(
            read(&out),
            vec![("Vendor".into(), "ven".into()), ("Title".into(), big)]
        );
        assert!(out.windows(5).any(|w| w == b"AUD-2"));
        // Shrinking back renumbers the audio pages again.
        let ch = VorbisChanges {
            set: vec![("Title".into(), "Short".into())],
            ..Default::default()
        };
        let back = write_ogg(&out, &ch).unwrap();
        check_pages(&back);
        assert_eq!(back.len(), src.len() - "ARTIST=A".len() - 4 + 2);
    }
}
