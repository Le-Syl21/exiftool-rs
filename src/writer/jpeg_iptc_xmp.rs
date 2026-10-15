//! IPTC (APP13 Photoshop) and XMP (APP1, with extended XMP) edits of a JPEG,
//! following ExifTool's `WriteJPEG` (Writer.pl).
//!
//! - **APP13**: consecutive `Photoshop 3.0` segments form one resource list,
//!   which is edited by [`photoshop_irb::edit_irb`]: only the IPTC resource
//!   changes, the others are kept. The list is written back split over as
//!   many segments as it needs; a list left empty removes the segments.
//! - **XMP**: the standard packet and the extended XMP it points to
//!   (`xmpNote:HasExtendedXMP`) are read as one, as ExifTool does, and
//!   edited in place by [`xmp_edit::edit_xmp`]. The result replaces every
//!   XMP segment, standard and extended; a packet too large for one segment
//!   is split into standard and extended XMP again.
//!
//! New segments go where ExifTool creates them: after the leading APP0
//! (JFIF) and APP1 (EXIF) segments, APP13 before XMP.

use crate::error::{Error, Result};
use crate::writer::extended_xmp;
use crate::writer::photoshop_irb::{self, IptcChanges};
use crate::writer::xmp_edit::{self, XmpChanges, XmpOutcome};

const PS_HEADER: &[u8] = b"Photoshop 3.0\0";
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const EXT_HEADER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
/// Largest segment payload (the length field counts itself).
const MAX_PAYLOAD: usize = 0xFFFD;

struct Segment {
    marker: u8,
    /// Offset of the 0xFF.
    start: usize,
    /// Past the payload.
    end: usize,
    /// The payload (after the length field).
    payload: std::ops::Range<usize>,
}

/// The segments before the start of scan, and the offset of the SOS marker
/// (or the end of the file).
fn segments(jpeg: &[u8]) -> Result<(Vec<Segment>, usize)> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err(Error::InvalidData("not a JPEG file".into()));
    }
    let mut out = Vec::new();
    let mut pos = 2;
    while pos + 2 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            return Err(Error::InvalidData("JPEG marker expected".into()));
        }
        let marker = jpeg[pos + 1];
        if marker == 0xFF {
            pos += 1; // fill byte
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            return Ok((out, pos));
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }
        if pos + 4 > jpeg.len() {
            break;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            return Err(Error::InvalidData("truncated JPEG segment".into()));
        }
        out.push(Segment {
            marker,
            start: pos,
            end: pos + 2 + len,
            payload: pos + 4..pos + 2 + len,
        });
        pos += 2 + len;
    }
    Ok((out, jpeg.len()))
}

/// `WriteMultiSegment`: `data` behind `header`, over as many segments as
/// it takes.
fn push_multi(out: &mut Vec<u8>, marker: u8, header: &[u8], data: &[u8]) {
    let room = MAX_PAYLOAD - header.len();
    for chunk in data.chunks(room) {
        push_segment(out, marker, &[header, chunk].concat());
    }
}

fn push_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

/// The XMP segments of a packet (standard, then extended if it is too big).
fn xmp_segments(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if packet.len() + XMP_HEADER.len() <= MAX_PAYLOAD {
        push_segment(&mut out, 0xE1, &[XMP_HEADER, packet].concat());
    } else {
        let (std, ext) = extended_xmp::split_xmp(packet);
        push_segment(&mut out, 0xE1, &std);
        for e in ext {
            push_segment(&mut out, 0xE1, &e);
        }
    }
    out
}

/// The GUID a standard packet names in `xmpNote:HasExtendedXMP`.
fn extended_guid(packet: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(packet).ok()?;
    let i = text.find(":HasExtendedXMP")?;
    let rest = &text[i + ":HasExtendedXMP".len()..];
    let rest = rest.trim_start();
    let rest = if let Some(r) = rest.strip_prefix('=') {
        r.trim_start().strip_prefix(['\'', '"'])?
    } else {
        rest.strip_prefix('>')?
    };
    let guid: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (guid.len() == 32).then_some(guid)
}

/// Apply IPTC and XMP edits to a JPEG.
pub fn rewrite(jpeg: &[u8], iptc: &IptcChanges, xmp: &XmpChanges) -> Result<Vec<u8>> {
    if iptc.is_empty() && xmp.is_empty() {
        return Ok(jpeg.to_vec());
    }
    let (segs, sos) = segments(jpeg)?;
    let seg_data = |s: &Segment| &jpeg[s.payload.clone()];

    // What replaces each segment (None: keep it), and what is inserted
    // before the segment at an index (segs.len(): before SOS).
    let mut replace: Vec<Option<Vec<u8>>> = vec![None; segs.len()];
    let mut new_app13: Option<Vec<u8>> = None;
    let mut new_xmp: Option<Vec<u8>> = None;

    // ── APP13 ────────────────────────────────────────────────────────────
    if !iptc.is_empty() {
        let is_ps = |s: &Segment| s.marker == 0xED && seg_data(s).starts_with(PS_HEADER);
        let first = segs.iter().position(is_ps);
        let run: Vec<usize> = match first {
            Some(f) => (f..segs.len()).take_while(|&i| is_ps(&segs[i])).collect(),
            None => Vec::new(),
        };
        let irb: Vec<u8> = run
            .iter()
            .flat_map(|&i| seg_data(&segs[i])[PS_HEADER.len()..].iter().copied())
            .collect();
        if let Some(new) = photoshop_irb::edit_irb(&irb, iptc)? {
            let mut bytes = Vec::new();
            if !new.is_empty() {
                push_multi(&mut bytes, 0xED, PS_HEADER, &new);
            }
            match run.first() {
                Some(&f) => {
                    replace[f] = Some(bytes);
                    for &i in &run[1..] {
                        replace[i] = Some(Vec::new());
                    }
                }
                None => new_app13 = Some(bytes),
            }
        }
    }

    // ── XMP ──────────────────────────────────────────────────────────────
    if !xmp.is_empty() {
        let std_idx: Vec<usize> = (0..segs.len())
            .filter(|&i| segs[i].marker == 0xE1 && seg_data(&segs[i]).starts_with(XMP_HEADER))
            .collect();
        let ext_idx: Vec<usize> = (0..segs.len())
            .filter(|&i| segs[i].marker == 0xE1 && seg_data(&segs[i]).starts_with(EXT_HEADER))
            .collect();
        let packets: Vec<&[u8]> = std_idx
            .iter()
            .map(|&i| &seg_data(&segs[i])[XMP_HEADER.len()..])
            .collect();
        let mut changes = xmp.clone();
        let mut main: Option<Vec<u8>> = packets.first().map(|p| p.to_vec());
        let mut merged_extended = false;
        // Further standard packets (seldom seen) are read along with the first.
        for p in packets.iter().skip(1) {
            if let Some(m) = &main {
                main = Some(xmp_edit::merge_packets(m, p)?);
            }
        }
        // The extended XMP the standard packet points to.
        if let Some(guid) = main.as_deref().and_then(extended_guid) {
            let mut chunks: Vec<(u32, u32, &[u8])> = Vec::new();
            for &i in &ext_idx {
                let d = seg_data(&segs[i]);
                if d.len() < EXT_HEADER.len() + 40 {
                    return Err(Error::InvalidXmp("truncated extended XMP".into()));
                }
                let h = EXT_HEADER.len();
                if &d[h..h + 32] != guid.as_bytes() {
                    continue; // not the standard extension: ExifTool drops it
                }
                let size = u32::from_be_bytes([d[h + 32], d[h + 33], d[h + 34], d[h + 35]]);
                let off = u32::from_be_bytes([d[h + 36], d[h + 37], d[h + 38], d[h + 39]]);
                chunks.push((off, size, &d[h + 40..]));
            }
            if !chunks.is_empty() {
                chunks.sort_by_key(|c| c.0);
                let size = chunks[0].1 as usize;
                let mut ext = Vec::with_capacity(size);
                for (off, _, d) in &chunks {
                    if *off as usize != ext.len() {
                        return Err(Error::InvalidXmp("incomplete extended XMP".into()));
                    }
                    ext.extend_from_slice(d);
                }
                if ext.len() != size {
                    return Err(Error::InvalidXmp("incomplete extended XMP".into()));
                }
                let m = main.take().expect("checked above");
                main = Some(xmp_edit::merge_packets(&m, &ext)?);
                merged_extended = true;
            }
        }
        let mut outcome = xmp_edit::edit_xmp(main.as_deref(), &changes)?;
        if merged_extended && matches!(outcome, XmpOutcome::Rewritten(_)) {
            // The pointer to the extended part goes: the packet is written
            // whole, and split again only if it has to be.
            changes
                .delete_tags
                .push((Some("xmpnote".into()), "hasextendedxmp".into()));
            outcome = xmp_edit::edit_xmp(main.as_deref(), &changes)?;
        }
        let all_xmp: Vec<usize> = std_idx.iter().chain(&ext_idx).copied().collect();
        match outcome {
            XmpOutcome::Unchanged => {}
            XmpOutcome::Removed => {
                for i in all_xmp {
                    replace[i] = Some(Vec::new());
                }
            }
            XmpOutcome::Rewritten(packet) => {
                let bytes = xmp_segments(&packet);
                match std_idx.first() {
                    Some(&f) => {
                        for i in all_xmp {
                            replace[i] = Some(Vec::new());
                        }
                        replace[f] = Some(bytes);
                    }
                    None => {
                        for i in ext_idx {
                            replace[i] = Some(Vec::new());
                        }
                        new_xmp = Some(bytes);
                    }
                }
            }
        }
    }

    // Where new segments go: after the leading JFIF/EXIF segments.
    let insert_at = segs
        .iter()
        .position(|s| {
            !(s.marker == 0xE0 || (s.marker == 0xE1 && seg_data(s).starts_with(b"Exif\0")))
        })
        .unwrap_or(segs.len());

    let mut out = Vec::with_capacity(jpeg.len() + 1024);
    out.extend_from_slice(&jpeg[..2]);
    let mut prev = 2;
    for (i, s) in segs.iter().enumerate() {
        // Bytes between segments (fill bytes, standalone markers).
        out.extend_from_slice(&jpeg[prev..s.start]);
        if i == insert_at {
            if let Some(b) = new_app13.take() {
                out.extend_from_slice(&b);
            }
            if let Some(b) = new_xmp.take() {
                out.extend_from_slice(&b);
            }
        }
        match &replace[i] {
            Some(b) => out.extend_from_slice(b),
            None => out.extend_from_slice(&jpeg[s.start..s.end]),
        }
        prev = s.end;
    }
    out.extend_from_slice(&jpeg[prev..sos]);
    if let Some(b) = new_app13.take() {
        out.extend_from_slice(&b);
    }
    if let Some(b) = new_xmp.take() {
        out.extend_from_slice(&b);
    }
    let sos_out = out.len();
    out.extend_from_slice(&jpeg[sos..]);
    // The segments changed size: shift the AFCP and MPF offsets (past the
    // SOS marker, where `jpeg_writer` counts the scan data from).
    crate::writer::jpeg_writer::fix_offsets_after_scan(jpeg, &mut out, sos + 2, sos_out + 2);
    // The trailers (AFCP, FotoStation, MIE...) are edited by `jpeg_trailers`.
    Ok(out)
}

/// The IPTC-IIM block of a JPEG, if it has one.
pub fn read_iptc(jpeg: &[u8]) -> Option<Vec<u8>> {
    let (segs, _) = segments(jpeg).ok()?;
    let irb: Vec<u8> = segs
        .iter()
        .filter(|s| s.marker == 0xED && jpeg[s.payload.clone()].starts_with(PS_HEADER))
        .flat_map(|s| {
            jpeg[s.payload.start + PS_HEADER.len()..s.payload.end]
                .iter()
                .copied()
        })
        .collect();
    photoshop_irb::find_iptc(&irb).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::iptc_writer::{check_value, lookup_tag, IptcRequest};

    fn jpeg(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for (m, p) in segments {
            push_segment(&mut out, *m, p);
        }
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        out
    }

    fn city(v: &str) -> IptcChanges {
        let tag = lookup_tag("City").unwrap();
        IptcChanges {
            delete_all: false,
            tags: vec![IptcRequest {
                tag,
                values: vec![check_value(tag, v).unwrap()],
                edit_only: false,
            }],
        }
    }

    #[test]
    fn creates_app13_after_jfif_and_exif() {
        let src = jpeg(&[
            (0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0".to_vec()),
            (0xE1, b"Exif\0\0MM\0\x2a\0\0\0\x08\0\0".to_vec()),
            (0xDB, vec![0; 4]),
        ]);
        let out = rewrite(&src, &city("Paris"), &XmpChanges::default()).unwrap();
        let (segs, _) = segments(&out).unwrap();
        assert_eq!(
            segs.iter().map(|s| s.marker).collect::<Vec<_>>(),
            [0xE0, 0xE1, 0xED, 0xDB]
        );
        assert_eq!(
            read_iptc(&out).unwrap(),
            b"\x1c\x02\x5a\x00\x05Paris\x1c\x02\x00\x00\x02\x00\x04"
        );
    }

    #[test]
    fn extended_xmp_is_merged_and_dropped() {
        let main = "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/'>\n<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n <rdf:Description rdf:about=''\n  xmlns:xmpNote='http://ns.adobe.com/xmp/note/'>\n  <xmpNote:HasExtendedXMP>0123456789ABCDEF0123456789ABCDEF</xmpNote:HasExtendedXMP>\n </rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end='w'?>";
        let ext = "<x:xmpmeta xmlns:x='adobe:ns:meta/'>\n<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n <rdf:Description rdf:about=''\n  xmlns:pdf='http://ns.adobe.com/pdf/1.3/'>\n  <pdf:Producer>P</pdf:Producer>\n </rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n";
        let mut e = EXT_HEADER.to_vec();
        e.extend_from_slice(b"0123456789ABCDEF0123456789ABCDEF");
        e.extend_from_slice(&(ext.len() as u32).to_be_bytes());
        e.extend_from_slice(&0u32.to_be_bytes());
        e.extend_from_slice(ext.as_bytes());
        let src = jpeg(&[(0xE1, [XMP_HEADER, main.as_bytes()].concat()), (0xE1, e)]);
        // Deleting a property leaves one packet holding the extended part.
        let changes = XmpChanges {
            delete_tags: vec![(None, "nosuchtag".into())],
            set: vec![xmp_edit::XmpSet {
                uri: "http://purl.org/dc/elements/1.1/".into(),
                prefix: "dc".into(),
                property: "title".into(),
                values: vec!["T".into()],
                kind: xmp_edit::XmpValueKind::LangAlt,
            }],
            toolkit: "test".into(),
            ..Default::default()
        };
        let out = rewrite(&src, &IptcChanges::default(), &changes).unwrap();
        let (segs, _) = segments(&out).unwrap();
        assert_eq!(segs.len(), 1);
        let text = String::from_utf8_lossy(&out[segs[0].payload.clone()]).to_string();
        assert!(text.contains("<pdf:Producer>P</pdf:Producer>"));
        assert!(!text.contains("HasExtendedXMP"));
        // XMP:all removes the extended segment as well.
        let del = XmpChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = rewrite(&src, &IptcChanges::default(), &del).unwrap();
        assert!(segments(&out).unwrap().0.is_empty());
    }
}
