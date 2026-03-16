//! RTF (Rich Text Format) reader - extract basic metadata from RTF info group.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_rtf(data: &[u8]) -> Result<Vec<Tag>> {
    if !data.starts_with(b"{\\rtf") {
        return Err(Error::InvalidData("not an RTF file".into()));
    }

    let mut tags = Vec::new();
    let text = String::from_utf8_lossy(data);

    // Extract {\info ... } group
    if let Some(info_start) = text.find("{\\info") {
        let info_text = &text[info_start..];

        let fields = [
            ("\\title ", "Title"),
            ("\\subject ", "Subject"),
            ("\\author ", "Author"),
            ("\\manager ", "Manager"),
            ("\\company ", "Company"),
            ("\\operator ", "LastModifiedBy"),
            ("\\category ", "Category"),
            ("\\keywords ", "Keywords"),
            ("\\comment ", "Comment"),
            ("\\doccomm ", "Comments"),
        ];

        for (keyword, name) in &fields {
            if let Some(value) = extract_rtf_field(info_text, keyword) {
                tags.push(mk(name, name, Value::String(value)));
            }
        }

        // Date fields: {\creatim\yrN\moN\dyN\hrN\minN\secN}
        if let Some(dt) = extract_rtf_date(info_text, "\\creatim") {
            tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
        }
        if let Some(dt) = extract_rtf_date(info_text, "\\revtim") {
            tags.push(mk("ModifyDate", "Modify Date", Value::String(dt)));
        }
    }

    // Page count from document properties
    if let Some(val) = extract_rtf_int(&text, "\\nofpages") {
        tags.push(mk("Pages", "Pages", Value::U32(val)));
    }
    if let Some(val) = extract_rtf_int(&text, "\\nofwords") {
        tags.push(mk("Words", "Words", Value::U32(val)));
    }
    if let Some(val) = extract_rtf_int(&text, "\\nofchars") {
        tags.push(mk("Characters", "Characters", Value::U32(val)));
    }

    Ok(tags)
}

fn extract_rtf_field(text: &str, keyword: &str) -> Option<String> {
    let pos = text.find(keyword)?;
    let rest = &text[pos + keyword.len()..];
    // Value is between { and } after keyword, or until next \ or }
    let end = rest.find('}')?;
    let value = rest[..end].trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn extract_rtf_date(text: &str, keyword: &str) -> Option<String> {
    let pos = text.find(keyword)?;
    let rest = &text[pos..];
    let end = rest.find('}')?;
    let block = &rest[..end];

    let yr = extract_rtf_num(block, "\\yr")?;
    let mo = extract_rtf_num(block, "\\mo").unwrap_or(1);
    let dy = extract_rtf_num(block, "\\dy").unwrap_or(1);
    let hr = extract_rtf_num(block, "\\hr").unwrap_or(0);
    let min = extract_rtf_num(block, "\\min").unwrap_or(0);
    let sec = extract_rtf_num(block, "\\sec").unwrap_or(0);

    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", yr, mo, dy, hr, min, sec))
}

fn extract_rtf_num(text: &str, keyword: &str) -> Option<u32> {
    let pos = text.find(keyword)?;
    let rest = &text[pos + keyword.len()..];
    let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

fn extract_rtf_int(text: &str, keyword: &str) -> Option<u32> {
    extract_rtf_num(text, keyword)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup { family0: "RTF".into(), family1: "RTF".into(), family2: "Document".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}
