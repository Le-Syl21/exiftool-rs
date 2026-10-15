//! IPTC-IIM metadata writer.
//!
//! [`edit_iptc`] is a port of ExifTool's `DoWriteIPTC` (WriteIPTC.pl): it
//! walks the existing datasets in order, drops the ones a request replaces
//! or deletes, writes a replaced value where the old one was, appends new
//! tags at the end of their record in tag-number order, and adds or drops
//! the record's mandatory version dataset (`ApplicationRecordVersion`) as
//! ExifTool does. Values are formatted like `CheckIPTC` and `FormatIPTC`:
//! integers packed, `digits` zero-padded, strings recoded to Latin-1 unless
//! the block declares UTF-8 and padded or truncated to the tag's length
//! limits. The output is byte-identical to ExifTool's, so the MD5 digest of
//! the block (`CurrentIPTCDigest`) is the same too.

use std::collections::{BTreeMap, HashMap};

use crate::error::{Error, Result};
use crate::tags::iptc_write_generated::{IptcConv, IptcWriteInfo, IPTC_WRITE_TAGS};

/// An IPTC record to write.
pub struct IptcRecord {
    pub record: u8,
    pub dataset: u8,
    pub data: Vec<u8>,
}

/// Build IPTC-IIM binary data from a list of records.
pub fn build_iptc(records: &[IptcRecord]) -> Vec<u8> {
    let mut output = Vec::new();
    for rec in records {
        push_dataset(&mut output, rec.record, rec.dataset, &rec.data);
    }
    output
}

/// One dataset header and value, with ExifTool's extended length form
/// (`0x8004` + a 4-byte length) for a value over 32767 bytes.
fn push_dataset(out: &mut Vec<u8>, record: u8, dataset: u8, data: &[u8]) {
    out.extend_from_slice(&[0x1C, record, dataset]);
    if data.len() <= 0x7FFF {
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
    } else {
        out.extend_from_slice(&0x8004u16.to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    out.extend_from_slice(data);
}

/// Parse an IPTC-IIM block into its datasets.
///
/// Each dataset is `0x1C <record> <dataset> <len:u16-be> <data>`; a length
/// with the top bit set gives the number of bytes of a longer length that
/// follows. Parsing stops at the first byte that is not a dataset marker.
pub fn parse_iim(data: &[u8]) -> Vec<IptcRecord> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 5 <= data.len() {
        if data[pos] != 0x1C {
            break;
        }
        let record = data[pos + 1];
        let dataset = data[pos + 2];
        let Some((start, len)) = dataset_len(data, pos) else {
            break;
        };
        if start + len > data.len() {
            break;
        }
        out.push(IptcRecord {
            record,
            dataset,
            data: data[start..start + len].to_vec(),
        });
        pos = start + len;
    }
    out
}

/// (value start, value length) of the dataset whose header is at `pos`.
fn dataset_len(data: &[u8], pos: usize) -> Option<(usize, usize)> {
    let len = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as usize;
    let mut start = pos + 5;
    if len & 0x8000 == 0 {
        return Some((start, len));
    }
    let n = len & 0x7FFF;
    if n > 8 || start + n > data.len() {
        return None;
    }
    let mut l = 0usize;
    for &b in &data[start..start + n] {
        l = l.checked_mul(256)?.checked_add(b as usize)?;
    }
    start += n;
    Some((start, l))
}

/// The IPTC tag a name (case-insensitive) designates, as in IPTC.pm. A few
/// short aliases older versions of this crate accepted are kept.
pub fn lookup_tag(name: &str) -> Option<&'static IptcWriteInfo> {
    let alias = match name.to_ascii_lowercase().as_str() {
        "title" => "ObjectName",
        "author" | "byline" => "By-line",
        "authorsposition" | "bylinetitle" => "By-lineTitle",
        "sublocation" => "Sub-location",
        "state" | "provincestate" => "Province-State",
        "countrycode" => "Country-PrimaryLocationCode",
        "country" => "Country-PrimaryLocationName",
        "copyright" => "CopyrightNotice",
        "caption" | "description" => "Caption-Abstract",
        "captionwriter" => "Writer-Editor",
        _ => name,
    }
    .to_string();
    IPTC_WRITE_TAGS
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(&alias))
}

/// Map IPTC tag name to (record, dataset).
pub fn tag_name_to_iptc(name: &str) -> Option<(u8, u8)> {
    lookup_tag(name).map(|t| (t.record, t.dataset))
}

/// What a request does to one IPTC tag.
#[derive(Debug, Clone)]
pub struct IptcRequest {
    pub tag: &'static IptcWriteInfo,
    /// The values to write (several for a list tag). Empty: delete the tag.
    pub values: Vec<String>,
}

/// Validate and convert a value for `tag` the way ExifTool's inverse
/// conversions and `CheckIPTC` do. Returns the raw value, still in UTF-8
/// (it is recoded when the block's character set is known).
pub fn check_value(tag: &IptcWriteInfo, value: &str) -> Result<String> {
    let bad = |why: &str| Error::ConversionError(format!("IPTC:{}: {why}", tag.name));
    if !tag.writable {
        return Err(bad("not writable"));
    }
    let mut val = if tag.print_inv.is_empty() {
        value.to_string()
    } else if let Some((_, raw)) = tag
        .print_inv
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case(value))
    {
        raw.to_string()
    } else if tag.print_inv.iter().any(|(_, raw)| *raw == value) {
        value.to_string()
    } else {
        return Err(bad("value not in the tag's list of values"));
    };
    val = match tag.conv {
        IptcConv::None => val,
        IptcConv::Date => iptc_date(&val).ok_or_else(|| bad("invalid date (use YYYY:mm:dd)"))?,
        IptcConv::Time => iptc_time(&val).ok_or_else(|| bad("invalid time"))?,
        IptcConv::Charset => coded_charset(&val).ok_or_else(|| bad("bad character set"))?,
        IptcConv::Unported => return Err(bad("conversion not supported")),
    };
    let format = tag.format;
    if let Some(bits) = format.strip_prefix("int") {
        let bytes = bits
            .trim_end_matches(['u', 's'])
            .parse::<u32>()
            .unwrap_or(0)
            / 8;
        if !matches!(bytes, 1 | 2 | 4) {
            return Err(bad("unsupported integer size"));
        }
        let n: u64 = if let Ok(n) = val.trim().parse::<u64>() {
            n
        } else {
            let h = val.trim();
            let h = h
                .strip_prefix("0x")
                .or_else(|| h.strip_prefix("0X"))
                .unwrap_or(h);
            u64::from_str_radix(h, 16).map_err(|_| bad("not an integer"))?
        };
        if bytes < 8 && n >> (bytes * 8) != 0 {
            return Err(bad("value too large"));
        }
        return Ok(n.to_string());
    }
    let (kind, min, max) = parse_len_format(format).ok_or_else(|| bad("bad format"))?;
    if kind == "digits" {
        if !val.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad("non-numeric characters in value"));
        }
        if !val.is_empty() && val.len() < min {
            val = format!("{}{val}", "0".repeat(min - val.len()));
        }
    }
    if kind != "string" {
        if val.len() < min {
            return Err(bad(&format!("string too short (minlen is {min})")));
        }
        if val.len() > max {
            val.truncate(max);
        }
    }
    Ok(val)
}

/// (`string`/`digits`/`undef`, min, max) of a `kind[min,max]` format;
/// `kind[n]` is a fixed length and a bare kind has no limit.
fn parse_len_format(format: &str) -> Option<(&str, usize, usize)> {
    let (kind, rest) = match format.find('[') {
        Some(i) => (&format[..i], &format[i..]),
        None => (format, ""),
    };
    if !matches!(kind, "string" | "digits" | "undef") {
        return None;
    }
    if rest.is_empty() {
        return Some((kind, 0, usize::MAX));
    }
    let inner = rest.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inner.split(',');
    let min: usize = parts.next()?.trim().parse().ok()?;
    let max = match parts.next() {
        Some(m) if !m.trim().is_empty() => m.trim().parse().ok()?,
        _ => min,
    };
    Some((kind, min, max))
}

/// `IptcDate`: the first `YYYY[-:/.]mm[-:/.]dd` → `YYYYmmdd`.
fn iptc_date(val: &str) -> Option<String> {
    let b = val.as_bytes();
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    for i in 0..b.len() {
        if b.len() < i + 8 || !digits(&b[i..i + 4]) {
            continue;
        }
        let mut j = i + 4;
        let mut parts = vec![&val[i..i + 4]];
        for _ in 0..2 {
            if j < b.len() && matches!(b[j], b'-' | b':' | b'/' | b'.') {
                j += 1;
            }
            if j + 2 > b.len() || !digits(&b[j..j + 2]) {
                break;
            }
            parts.push(&val[j..j + 2]);
            j += 2;
        }
        if parts.len() == 3 {
            return Some(parts.concat());
        }
    }
    None
}

/// `IptcTime`: `HH:MM:SS[+-HH:MM|Z]` → `HHMMSS+HHMM`. ExifTool falls back
/// to the system's time zone when the value has none; this uses UTC.
fn iptc_time(val: &str) -> Option<String> {
    let v = val.trim_end();
    // The last token holding the time, with an optional zone after it.
    let token = v.rsplit(char::is_whitespace).next()?;
    let digits_end = token
        .find(|c: char| !(c.is_ascii_digit() || c == ':'))
        .unwrap_or(token.len());
    let (time, tz) = token.split_at(digits_end);
    let nums: Vec<&str> = if time.contains(':') {
        time.split(':').collect()
    } else if time.len() == 6 {
        vec![&time[0..2], &time[2..4], &time[4..6]]
    } else {
        return None;
    };
    if nums.len() != 3 || nums.iter().any(|n| n.is_empty() || n.len() > 2) {
        return None;
    }
    let n: Vec<u32> = nums.iter().map(|n| n.parse().ok()).collect::<Option<_>>()?;
    let mut out = format!("{:02}{:02}{:02}", n[0], n[1], n[2]);
    let tz = tz.trim();
    if tz.eq_ignore_ascii_case("z") || tz.is_empty() {
        out.push_str("+0000");
    } else {
        let sign = tz.chars().next()?;
        if sign != '+' && sign != '-' {
            return None;
        }
        let rest: String = tz[1..].chars().filter(|c| *c != ':').collect();
        if rest.len() < 3 || rest.len() > 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let (h, m) = rest.split_at(rest.len() - 2);
        out.push_str(&format!("{sign}{:02}{m}", h.parse::<u32>().ok()?));
    }
    Some(out)
}

/// `CodedCharacterSet`: `UTF8` → `ESC % G`, or `ESC X Y[, ...]` escapes.
fn coded_charset(val: &str) -> Option<String> {
    if val.eq_ignore_ascii_case("utf8") || val.eq_ignore_ascii_case("utf-8") {
        return Some("\x1b%G".into());
    }
    if val.to_ascii_uppercase().contains("ESC") {
        let mut s = String::new();
        let upper = val.to_string();
        let mut rest = upper.as_str();
        while let Some(i) = rest.to_ascii_uppercase().find("ESC") {
            s.push_str(&rest[..i]);
            s.push('\x1b');
            rest = rest[i + 3..].trim_start();
        }
        s.push_str(rest);
        let s = s.replace(", \x1b", "\x1b").replace(' ', "");
        return Some(s);
    }
    Some(val.to_string())
}

/// Does a `CodedCharacterSet` value declare UTF-8?
fn is_utf8_charset(val: &[u8]) -> bool {
    val == b"\x1b%G"
}

/// Format a checked value for writing (`FormatIPTC`).
fn format_value(tag: &IptcWriteInfo, val: &str, latin: bool) -> Vec<u8> {
    let format = tag.format;
    if let Some(bits) = format.strip_prefix("int") {
        let n: u64 = val.parse().unwrap_or(0);
        return match bits
            .trim_end_matches(['u', 's'])
            .parse::<u32>()
            .unwrap_or(0)
            / 8
        {
            1 => vec![n as u8],
            2 => (n as u16).to_be_bytes().to_vec(),
            _ => (n as u32).to_be_bytes().to_vec(),
        };
    }
    if !format.starts_with("string") {
        return val.as_bytes().to_vec();
    }
    let mut bytes = if tag.record != 1 && tag.record < 7 && latin && !val.is_ascii() {
        crate::encoding::encode_latin1(val)
    } else {
        val.as_bytes().to_vec()
    };
    if let Some((_, min, max)) = parse_len_format(format) {
        if bytes.len() < min {
            bytes.resize(min, b' ');
        } else if bytes.len() > max {
            bytes.truncate(max);
            if !latin {
                // Keep the UTF-8 valid: drop a split character.
                while std::str::from_utf8(&bytes).is_err() && !bytes.is_empty() {
                    bytes.pop();
                }
            }
        }
    }
    bytes
}

/// ExifTool's mandatory datasets: the version of records 1, 2 and 3.
fn mandatory(record: u8) -> Option<(u8, u16)> {
    matches!(record, 1..=3).then_some((0, 4))
}

/// Apply `requests` to the IPTC block `data` (empty when the file has none),
/// as ExifTool's `DoWriteIPTC` does. Returns the new block, empty when no
/// dataset is left.
pub fn edit_iptc(data: &[u8], requests: &[IptcRequest]) -> Result<Vec<u8>> {
    // Requests by record, then by dataset (one per tag: later ones win,
    // values of a list tag accumulate).
    let mut by_tag: BTreeMap<(u8, u8), IptcRequest> = BTreeMap::new();
    for r in requests {
        let key = (r.tag.record, r.tag.dataset);
        match by_tag.get_mut(&key) {
            Some(prev) if r.tag.list && !r.values.is_empty() && !prev.values.is_empty() => {
                prev.values.extend(r.values.iter().cloned())
            }
            _ => {
                by_tag.insert(key, r.clone());
            }
        }
    }
    let mut queue: BTreeMap<u8, std::collections::VecDeque<(u8, u8)>> = BTreeMap::new();
    for &(rec, ds) in by_tag.keys() {
        queue.entry(rec).or_default().push_back((rec, ds));
    }
    let mut record_list: std::collections::VecDeque<u8> = queue.keys().copied().collect();

    // ExifTool starts from its IPTC charset (Latin) and switches to no
    // translation when the block declares UTF-8.
    let mut latin = true;
    let mut found: HashMap<(u8, u8), u8> = HashMap::new();
    let end = data.len();
    let mut pos = 0usize;
    let mut new_data: Vec<u8> = Vec::new();
    let mut last_rec: i32 = -1;
    let mut last_rec_pos = 0usize;
    let mut all_mandatory: u32 = 0;
    let mut add_now: Option<(u8, u8)> = None;

    loop {
        let tail = pos;
        let mut rec: Option<u8> = None;
        let mut tag = 0u8;
        let mut value_ptr = pos;
        if pos + 5 <= end && data[pos] == 0x1C {
            let r = data[pos + 1];
            tag = data[pos + 2];
            if (r as i32) < last_rec {
                return Err(Error::InvalidIptc(if r == 0 {
                    "IPTC record 0 encountered".into()
                } else {
                    "IPTC doesn't conform to spec: records out of sequence".into()
                }));
            }
            let (start, len) = dataset_len(data, pos).unwrap_or((pos + 5, end));
            value_ptr = start.min(end);
            pos = start.saturating_add(len).min(end);
            rec = Some(r);
        }
        let write_rec = rec.is_none_or(|r| r as i32 != last_rec);
        if write_rec || add_now.is_some() {
            loop {
                let new_rec = record_list.front().copied();
                let key;
                if let Some(k) = add_now {
                    key = k;
                } else {
                    if new_rec.is_none_or(|n| n as i32 != last_rec) {
                        if new_data.len() > last_rec_pos && last_rec >= 0 {
                            let lr = last_rec as u8;
                            if all_mandatory > 1 {
                                // The record held only mandatory tags and one
                                // was deleted: drop it, unless a tag was added.
                                let mut num = 0;
                                for (_, &code) in found.iter().filter(|((r, _), _)| *r == lr) {
                                    if code & 0x04 != 0 {
                                        num = 0;
                                        break;
                                    }
                                    if code & 0x03 == 0x01 {
                                        num += 1;
                                    }
                                }
                                if num > 0 {
                                    new_data.truncate(last_rec_pos);
                                }
                            } else if let Some((ds, v)) = mandatory(lr) {
                                if !found.contains_key(&(lr, ds)) {
                                    push_dataset(&mut new_data, lr, ds, &v.to_be_bytes());
                                }
                            }
                        }
                        let Some(n) = new_rec else {
                            break;
                        };
                        last_rec = n as i32;
                        last_rec_pos = new_data.len();
                        all_mandatory = 1;
                    }
                    let n = new_rec.expect("set above");
                    if rec.is_some_and(|r| r <= n) {
                        break;
                    }
                    key = *queue[&n].front().expect("queued records are non-empty");
                }
                let req = &by_tag[&key];
                let f = found.get(&key).copied().unwrap_or(0);
                let do_set = if f & 0x02 != 0 {
                    f & 0x04 == 0
                } else {
                    req.tag.list || f == 0
                };
                if do_set && !req.values.is_empty() {
                    *found.entry(key).or_insert(f) |= 0x04;
                    for v in &req.values {
                        if all_mandatory > 0 && mandatory(key.0).is_none_or(|(ds, _)| ds != key.1) {
                            all_mandatory = 0;
                        }
                        let bytes = format_value(req.tag, v, latin);
                        if key == (1, 90) {
                            latin = !is_utf8_charset(&bytes);
                        }
                        push_dataset(&mut new_data, key.0, key.1, &bytes);
                    }
                }
                if add_now.take().is_some() {
                    if write_rec {
                        continue;
                    }
                    break;
                }
                let q = queue.get_mut(&key.0).expect("queued");
                q.pop_front();
                if q.is_empty() {
                    record_list.pop_front();
                }
            }
            if write_rec {
                let Some(r) = rec else {
                    break;
                };
                last_rec = r as i32;
                last_rec_pos = new_data.len();
                all_mandatory = 1;
            }
        }
        let r = rec.expect("a dataset was read");
        let key = (r, tag);
        let e = found.entry(key).or_insert(0);
        if *e == 0 {
            *e = 0x01;
        }
        if let Some(req) = by_tag.get(&key) {
            // Overwritten or deleted: drop the old value, and write the new
            // one here if its record is the one being written.
            *found.get_mut(&key).expect("inserted") |= 0x02;
            if all_mandatory > 0 {
                all_mandatory += 1;
            }
            if !req.values.is_empty() && record_list.front() == Some(&r) && found[&key] & 0x04 == 0
            {
                add_now = Some(key);
            }
            continue;
        } else if key == (1, 90) {
            latin = !is_utf8_charset(&data[value_ptr..pos]);
        }
        if all_mandatory > 0 && mandatory(r).is_none_or(|(ds, _)| ds != tag) {
            all_mandatory = 0;
        }
        new_data.extend_from_slice(&data[tail..pos]);
    }
    // Whatever follows the last dataset must be zero padding.
    if data[pos.min(end)..].iter().any(|&b| b != 0) {
        return Err(Error::InvalidIptc(
            "unrecognized data in IPTC padding".into(),
        ));
    }
    Ok(new_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(name: &str, values: &[&str]) -> IptcRequest {
        IptcRequest {
            tag: lookup_tag(name).unwrap(),
            values: values
                .iter()
                .map(|v| check_value(lookup_tag(name).unwrap(), v).unwrap())
                .collect(),
        }
    }

    #[test]
    fn parse_iim_roundtrips_build() {
        let records = vec![
            IptcRecord {
                record: 1,
                dataset: 90,
                data: vec![0x1B, 0x25, 0x47],
            },
            IptcRecord {
                record: 2,
                dataset: 80,
                data: b"Martxn".to_vec(),
            },
            IptcRecord {
                record: 2,
                dataset: 25,
                data: b"kw".to_vec(),
            },
        ];
        let built = build_iptc(&records);
        let parsed = parse_iim(&built);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].record, 1);
        assert_eq!(parsed[0].dataset, 90);
        assert_eq!(parsed[1].data, b"Martxn");
        assert_eq!(parsed[2].dataset, 25);
    }

    #[test]
    fn parse_iim_stops_on_garbage() {
        assert!(parse_iim(&[0x00, 0x01, 0x02]).is_empty());
        assert!(parse_iim(&[]).is_empty());
    }

    #[test]
    fn parse_iim_truncated_length_is_safe() {
        let bytes = [0x1C, 2, 80, 0x00, 0x0A, b'a', b'b'];
        assert!(parse_iim(&bytes).is_empty());
    }

    // `exiftool -IPTC:City=Paris` on a file without IPTC writes the tag,
    // then the mandatory ApplicationRecordVersion (4) at the end of record 2.
    #[test]
    fn new_block_gets_mandatory_version_after_the_tags() {
        let out = edit_iptc(&[], &[req("City", &["Paris"])]).unwrap();
        assert_eq!(
            out,
            b"\x1c\x02\x5a\x00\x05Paris\x1c\x02\x00\x00\x02\x00\x04"
        );
    }

    // A replaced value takes the old one's place; a new tag goes to the end
    // of its record, and the version already there is not added again.
    #[test]
    fn replace_in_place_and_append_new() {
        let old = build_iptc(&[
            IptcRecord {
                record: 2,
                dataset: 0,
                data: vec![0, 4],
            },
            IptcRecord {
                record: 2,
                dataset: 80,
                data: b"Old".to_vec(),
            },
            IptcRecord {
                record: 2,
                dataset: 80,
                data: b"Old2".to_vec(),
            },
            IptcRecord {
                record: 2,
                dataset: 120,
                data: b"Cap".to_vec(),
            },
        ]);
        let out = edit_iptc(&old, &[req("By-line", &["New"]), req("City", &["X"])]).unwrap();
        let want = build_iptc(&[
            IptcRecord {
                record: 2,
                dataset: 0,
                data: vec![0, 4],
            },
            IptcRecord {
                record: 2,
                dataset: 80,
                data: b"New".to_vec(),
            },
            IptcRecord {
                record: 2,
                dataset: 120,
                data: b"Cap".to_vec(),
            },
            IptcRecord {
                record: 2,
                dataset: 90,
                data: b"X".to_vec(),
            },
        ]);
        assert_eq!(out, want);
    }

    // Deleting the last non-mandatory tag of a record drops the record.
    #[test]
    fn record_left_with_only_its_version_is_dropped() {
        let old = build_iptc(&[
            IptcRecord {
                record: 2,
                dataset: 0,
                data: vec![0, 4],
            },
            IptcRecord {
                record: 2,
                dataset: 80,
                data: b"Who".to_vec(),
            },
        ]);
        let del = IptcRequest {
            tag: lookup_tag("By-line").unwrap(),
            values: vec![],
        };
        assert!(edit_iptc(&old, &[del]).unwrap().is_empty());
    }

    #[test]
    fn strings_are_latin1_unless_utf8_declared_and_truncated() {
        let out = edit_iptc(&[], &[req("By-line", &["Martín"])]).unwrap();
        assert!(out.windows(6).any(|w| w == b"Mart\xedn"));
        let utf8 = build_iptc(&[IptcRecord {
            record: 1,
            dataset: 90,
            data: b"\x1b%G".to_vec(),
        }]);
        let out = edit_iptc(&utf8, &[req("By-line", &["Martín"])]).unwrap();
        assert!(out.windows(7).any(|w| w == "Martín".as_bytes()));
        // By-line is string[0,32].
        let long = "x".repeat(40);
        let out = edit_iptc(&[], &[req("By-line", &[&long])]).unwrap();
        assert_eq!(&out[3..5], &[0, 32]);
    }

    #[test]
    fn conversions() {
        let d = lookup_tag("DateCreated").unwrap();
        assert_eq!(check_value(d, "2024:01:31").unwrap(), "20240131");
        let t = lookup_tag("TimeCreated").unwrap();
        assert_eq!(check_value(t, "12:34:56+02:00").unwrap(), "123456+0200");
        let u = lookup_tag("Urgency").unwrap();
        assert_eq!(check_value(u, "5 (normal urgency)").unwrap(), "5");
        assert!(check_value(lookup_tag("IPTCPictureNumber").unwrap(), "x").is_err());
    }
}
