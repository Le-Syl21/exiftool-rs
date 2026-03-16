//! Extended XMP support for JPEG files.
//!
//! When XMP data exceeds 65502 bytes (JPEG segment limit minus headers),
//! it must be split across multiple APP1 segments using Extended XMP.
//!
//! Standard XMP segment:
//!   FF E1 [len] "http://ns.adobe.com/xap/1.0/\0" [standard XMP]
//!
//! Extended XMP segments:
//!   FF E1 [len] "http://ns.adobe.com/xmp/extension/\0" [GUID:32] [total_len:4] [offset:4] [data]

const XMP_STD_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_EXT_HEADER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const MAX_STD_XMP: usize = 65502 - 29; // 65502 - header length

/// Split XMP data into standard + extended segments for JPEG embedding.
///
/// Returns (standard_segment_data, extended_segments).
/// Each segment includes the appropriate header.
pub fn split_xmp(xmp_data: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    if xmp_data.len() <= MAX_STD_XMP {
        // Fits in a single standard segment
        let mut seg = Vec::with_capacity(XMP_STD_HEADER.len() + xmp_data.len());
        seg.extend_from_slice(XMP_STD_HEADER);
        seg.extend_from_slice(xmp_data);
        return (seg, Vec::new());
    }

    // Need to split: put xpacket wrapper in standard, actual RDF in extended
    // Find a good split point (after x:xmpmeta close or at xpacket boundary)
    let _xmp_str = String::from_utf8_lossy(xmp_data);

    // Standard part: minimal XMP with HasExtendedXMP property
    let guid = compute_md5_hex(xmp_data);

    let standard_xmp = format!(
        "<?xpacket begin='\u{FEFF}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
         <x:xmpmeta xmlns:x='adobe:ns:meta/'>\n\
         <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\
         <rdf:Description rdf:about=''\n\
           xmlns:xmpNote='http://ns.adobe.com/xmp/note/'>\n\
           <xmpNote:HasExtendedXMP>{}</xmpNote:HasExtendedXMP>\n\
         </rdf:Description>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end='w'?>",
        guid
    );

    let mut std_seg = Vec::new();
    std_seg.extend_from_slice(XMP_STD_HEADER);
    std_seg.extend_from_slice(standard_xmp.as_bytes());

    // Extended part: full XMP split into 65000-byte chunks
    let ext_data = xmp_data;
    let total_len = ext_data.len() as u32;
    let chunk_size = 65000 - XMP_EXT_HEADER.len() - 32 - 4 - 4; // header + guid + total_len + offset
    let guid_bytes = guid.as_bytes();

    let mut ext_segments = Vec::new();
    let mut offset = 0u32;

    while (offset as usize) < ext_data.len() {
        let remaining = ext_data.len() - offset as usize;
        let chunk_len = remaining.min(chunk_size);
        let chunk = &ext_data[offset as usize..offset as usize + chunk_len];

        let mut seg = Vec::new();
        seg.extend_from_slice(XMP_EXT_HEADER);
        // GUID (32 ASCII hex chars)
        seg.extend_from_slice(guid_bytes);
        // Total length (4 bytes BE)
        seg.extend_from_slice(&total_len.to_be_bytes());
        // Offset (4 bytes BE)
        seg.extend_from_slice(&offset.to_be_bytes());
        // Data
        seg.extend_from_slice(chunk);

        ext_segments.push(seg);
        offset += chunk_len as u32;
    }

    (std_seg, ext_segments)
}

/// Reassemble extended XMP from multiple segments.
pub fn reassemble_xmp(
    standard: &[u8],
    extended_segments: &[(&[u8], u32, u32)], // (data, total_len, offset)
) -> Vec<u8> {
    if extended_segments.is_empty() {
        return standard.to_vec();
    }

    // Find total extended length
    let total_len = extended_segments.first().map(|s| s.1 as usize).unwrap_or(0);
    if total_len == 0 {
        return standard.to_vec();
    }

    let mut extended = vec![0u8; total_len];
    for &(data, _, offset) in extended_segments {
        let start = offset as usize;
        let end = (start + data.len()).min(total_len);
        extended[start..end].copy_from_slice(&data[..end - start]);
    }

    extended
}

/// Simple MD5 hash as hex string (32 chars).
fn compute_md5_hex(data: &[u8]) -> String {
    // Minimal MD5 implementation for GUID generation
    let hash = simple_md5(data);
    hash.iter().map(|b| format!("{:02X}", b)).collect()
}

/// Minimal MD5 hash (RFC 1321). Returns 16-byte digest.
fn simple_md5(data: &[u8]) -> [u8; 16] {
    // Constants
    let s: [u32; 64] = [
        7,12,17,22,7,12,17,22,7,12,17,22,7,12,17,22,
        5,9,14,20,5,9,14,20,5,9,14,20,5,9,14,20,
        4,11,16,23,4,11,16,23,4,11,16,23,4,11,16,23,
        6,10,15,21,6,10,15,21,6,10,15,21,6,10,15,21,
    ];
    let k: [u32; 64] = [
        0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
        0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
        0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
        0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
        0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
        0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
        0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
        0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // Pre-processing: add padding
    let orig_len = data.len();
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    let bit_len = (orig_len as u64) * 8;
    msg.extend_from_slice(&bit_len.to_le_bytes());

    // Process 512-bit chunks
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, c) in chunk.chunks(4).enumerate() {
            if i < 16 {
                m[i] = u32::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0), c.get(3).copied().unwrap_or(0)]);
            }
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5*i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3*i + 5) % 16),
                _ => (c ^ (b | (!d)), (7*i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g]))
                .rotate_left(s[i])
            );
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
}

