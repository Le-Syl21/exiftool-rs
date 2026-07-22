//! Radiance HDR format reader.
//!
//! Mirrors `Image::ExifTool::Radiance::ProcessHDR` (Radiance.pm lines 66-107).

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

/// The named entries of `%Image::ExifTool::Radiance::Main` (Radiance.pm lines
/// 21-61), keyed by the lower-cased header key as ExifTool stores them.
fn radiance_tag_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "software" => "Software",
        "view" => "View",
        "format" => "Format",
        "exposure" => "Exposure",
        "gamma" => "Gamma",
        "colorcorr" => "ColorCorrection",
        "pixaspect" => "PixelAspectRatio",
        "primaries" => "ColorPrimaries",
        _ => return None,
    })
}

/// The PrintConv of the `_orient` tag (Radiance.pm lines 32-42).
fn orientation_print_conv(orient: &str) -> Option<&'static str> {
    Some(match orient {
        "-Y +X" => "Horizontal (normal)",
        "-Y -X" => "Mirror horizontal",
        "+Y -X" => "Rotate 180",
        "+Y +X" => "Mirror vertical",
        "+X -Y" => "Mirror horizontal and rotate 270 CW",
        "+X +Y" => "Rotate 90 CW",
        "-X +Y" => "Mirror horizontal and rotate 90 CW",
        "-X -Y" => "Rotate 270 CW",
        _ => return None,
    })
}

/// Name a header key ExifTool has no entry for, as ProcessHDR does
/// (Radiance.pm lines 96-102): drop every character outside `-_a-zA-Z0-9`,
/// require more than one to survive, then upper-case the first.
fn dynamic_tag_name(key: &str) -> Option<String> {
    let name: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if name.chars().count() <= 1 {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    Some(format!("{}{}", first.to_uppercase(), chars.as_str()))
}

/// Split a header line the way ProcessHDR's `/^(.*)?\s*=\s*(.*)/` does
/// (Radiance.pm line 88). `(.*)` is greedy, so the key runs to the LAST `=` on
/// the line, the `\s*` before it matches nothing and any whitespace in front of
/// the `=` stays part of the key; the `\s*` after it does eat the whitespace in
/// front of the value. A line with no `=` is not a key/value pair at all.
fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let eq = line.rfind('=')?;
    let value = line[eq + 1..].trim_start_matches(|c: char| c.is_ascii_whitespace());
    Some((&line[..eq], value))
}

pub fn read_hdr(data: &[u8]) -> Result<Vec<Tag>> {
    // `return 0 unless ... $buff =~ /^#\?(RADIANCE|RGBE)\x0a/s` (Radiance.pm
    // line 75): the signature is a line of its own.
    let Some(first_end) = data.iter().position(|b| *b == b'\n') else {
        return Err(Error::InvalidData("not a Radiance HDR file".into()));
    };
    let magic = &data[..first_end];
    if magic != b"#?RADIANCE" && magic != b"#?RGBE" {
        return Err(Error::InvalidData("not a Radiance HDR file".into()));
    }

    let mut tags = Vec::new();
    // `local $/ = "\x0a"`, then `chomp`: lines end at a newline and only the
    // newline is stripped, so a CR stays part of the value.
    let mut lines = data[first_end..]
        .split(|b| *b == b'\n')
        .skip(1)
        .map(crate::encoding::decode_utf8_or_latin1);

    for line in lines.by_ref() {
        // `last unless length($buff) > 0 and length($buff) < 4096`: an empty
        // line ends the header, and so does an absurdly long one.
        if line.is_empty() || line.len() >= 4096 {
            break;
        }
        if let Some(rest) = line.strip_prefix('#') {
            // `s/^#\s*//` (Radiance.pm line 84).
            let comment = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if !comment.is_empty() {
                tags.push(mktag(
                    "Radiance",
                    "Comment",
                    "Comment",
                    Value::String(comment.to_string()),
                ));
            }
            continue;
        }
        let Some((key, value)) = split_key_value(&line) else {
            // Anything that is not a comment and holds no `=` is a command.
            tags.push(mktag(
                "Radiance",
                "Command",
                "Command",
                Value::String(line.clone()),
            ));
            continue;
        };
        let key = key.to_lowercase();
        let name = match radiance_tag_name(&key) {
            Some(name) => name.to_string(),
            None => match dynamic_tag_name(&key) {
                Some(name) => name,
                // `next unless length($name) > 1`: nothing is extracted.
                None => continue,
            },
        };
        tags.push(mktag(
            "Radiance",
            &name,
            &name,
            Value::String(value.to_string()),
        ));
    }

    // The line after the header carries the image dimensions (Radiance.pm
    // lines 103-107). Height is always the first number and width the second,
    // whichever axes name them.
    if let Some(line) = lines.next() {
        if let Some((orient, height, width)) = parse_resolution(&line) {
            let print = orientation_print_conv(&orient)
                .map(str::to_string)
                .unwrap_or(orient);
            tags.push(mktag(
                "Radiance",
                "Orientation",
                "Orientation",
                Value::String(print),
            ));
            tags.push(mktag(
                "File",
                "ImageHeight",
                "Image Height",
                Value::U32(height),
            ));
            tags.push(mktag(
                "File",
                "ImageWidth",
                "Image Width",
                Value::U32(width),
            ));
        }
    }

    Ok(tags)
}

/// `/([-+][XY])\s*(\d+)\s*([-+][XY])\s*(\d+)/` (Radiance.pm line 104), which
/// yields `("$1 $3", $2, $4)`.
fn parse_resolution(line: &str) -> Option<(String, u32, u32)> {
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let Some((axis1, mut j)) = match_axis(&bytes, i) else {
            i += 1;
            continue;
        };
        j = skip_spaces(&bytes, j);
        let Some((first, mut j)) = match_digits(&bytes, j) else {
            i += 1;
            continue;
        };
        j = skip_spaces(&bytes, j);
        let Some((axis2, mut j)) = match_axis(&bytes, j) else {
            i += 1;
            continue;
        };
        j = skip_spaces(&bytes, j);
        let Some((second, _)) = match_digits(&bytes, j) else {
            i += 1;
            continue;
        };
        return Some((format!("{axis1} {axis2}"), first, second));
    }
    None
}

fn match_axis(chars: &[char], i: usize) -> Option<(String, usize)> {
    let sign = *chars.get(i)?;
    let axis = *chars.get(i + 1)?;
    if matches!(sign, '-' | '+') && matches!(axis, 'X' | 'Y') {
        Some((format!("{sign}{axis}"), i + 2))
    } else {
        None
    }
}

fn skip_spaces(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn match_digits(chars: &[char], i: usize) -> Option<(u32, usize)> {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j == i {
        return None;
    }
    chars[i..j]
        .iter()
        .collect::<String>()
        .parse()
        .ok()
        .map(|v| (v, j))
}
