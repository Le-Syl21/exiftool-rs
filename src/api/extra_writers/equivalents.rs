//! Equivalences between the audio tags that each format names its own way
//! (ID3 `Year`, Vorbis `DATE`, Matroska `DATE_RELEASED`, QuickTime
//! `ContentCreateDate`), for a copy to one of the built-in writers' formats.
//!
//! ExifTool copies a tag without a destination group to the destination's
//! tag of the same name, so the tags two formats name differently do not
//! carry over. For the formats ExifTool writes it stays so (the copy is
//! ExifTool's own); for those the built-in extensions write, a tag copied
//! without a destination group that the destination does not have under its
//! name is also written as the destination's tag of the same meaning, its
//! value converted (`1 of 5` is `1/5` in ID3, `TRACKNUMBER=1` and
//! `TRACKTOTAL=5` in a Vorbis comment, `PART_NUMBER` and `TOTAL_PARTS` in
//! Matroska; a full date is a year for ID3 `Year`, an ISO 8601 date for
//! Vorbis `DATE` and Matroska `DATE_RELEASED`). Never for a tag copied to a
//! group the tag list names, nor for a `set_new_value` request.
//!
//! Precedence: what the destination's format would take by name comes
//! first. A tag the source has under the destination's own name (another
//! format's tag of that name, or an unrelated tag) is written as ExifTool
//! writes it, and no equivalent is made for that destination tag; a tag of
//! the destination's own format is never converted. Among the source's
//! equivalents, the one of the destination's name comes first, then the
//! table's order (a recording time before a year).
//!
//! The same idea was proposed to ExifTool itself in
//! <https://github.com/exiftool/exiftool/issues/497>.

use crate::api::exiftool::NewValue;
use crate::api::extension::CopiedTag;

use super::flac;

/// The formats of the equivalences: those of the built-in writers, and
/// QuickTime (an M4A's iTunes tags) as a source only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Id3,
    Vorbis,
    Matroska,
    QuickTime,
}

impl Format {
    /// The format of a copied tag, by its family 0 group (and not the
    /// segment Info of a Matroska file, whose `DateTimeOriginal` is the
    /// muxing date, not the SimpleTag `DATE_RECORDED`).
    fn of(t: &CopiedTag) -> Option<Format> {
        Some(match t.family0.as_deref()? {
            "ID3" => Format::Id3,
            "Vorbis" => Format::Vorbis,
            "Matroska" if t.family1.as_deref() != Some("Info") => Format::Matroska,
            "QuickTime" => Format::QuickTime,
            _ => return None,
        })
    }
}

/// What a member of an equivalence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The value (a date, a number with or without its total, a text).
    Main,
    /// The total of a number given apart (Vorbis `TRACKTOTAL`).
    Total,
    /// ID3v2.3 `TDAT` (`DDMM`), which completes a `Year`.
    DayMonth,
    /// ID3v2.3 `TIME` (`HHMM`), which completes a `Year` and a `TDAT`.
    Time,
}

/// How the value of an equivalence is converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    /// Yes/No (ID3 and QuickTime as ExifTool prints them), 1/0 (Vorbis).
    Flag,
    Date,
    /// A number and its total.
    Number,
}

/// One meaning, named by each format.
struct Equivalence {
    kind: Kind,
    /// The source tags (format, ExifTool's name, role), by preference.
    members: &'static [(Format, &'static str, Role)],
    /// The destination tags (format, name, name of the total if apart: a
    /// destination with no total name takes `n/total` in its one tag).
    dest: &'static [(Format, &'static str, Option<&'static str>)],
}

use Format::{Id3, Matroska, QuickTime, Vorbis};
use Role::{DayMonth, Main, Time, Total};

/// The equivalences. Names are ExifTool's (ID3.pm, Vorbis.pm, Matroska.pm
/// `%StdTag`, QuickTime.pm `%ItemList`); a Vorbis field Vorbis.pm does not
/// list is named as ExifTool names it when reading (`DISCNUMBER` is
/// `Discnumber`) and written under its usual field name.
const EQUIVALENCES: &[Equivalence] = &[
    // The date: ID3v2.4 TDRC (RecordingTime), ID3v2.3 TYER (+ TDAT, TIME) and
    // ID3v1 Year; Vorbis DATE; Matroska DATE_RELEASED (the date of a
    // release, as ffmpeg and mkvtoolnix map a file's "date"), DATE_RECORDED
    // as a source; QuickTime ©day.
    Equivalence {
        kind: Kind::Date,
        members: &[
            (Vorbis, "Date", Main),
            (Matroska, "DateReleased", Main),
            (Matroska, "DateTimeOriginal", Main),
            (Id3, "RecordingTime", Main),
            (Id3, "ReleaseTime", Main),
            (Id3, "Year", Main),
            (Id3, "Date", DayMonth),
            (Id3, "Time", Time),
            (QuickTime, "ContentCreateDate", Main),
        ],
        dest: &[(Id3, "Year", None), (Vorbis, "Date", None), (Matroska, "DateReleased", None)],
    },
    // The track number: ID3 TRCK (n/total); Vorbis TRACKNUMBER and
    // TRACKTOTAL (or TOTALTRACKS); Matroska PART_NUMBER and TOTAL_PARTS, in
    // the whole-file Tag (as ffmpeg writes a track number; TOTAL_PARTS at the
    // album level is the number of tracks); QuickTime trkn ("n of total").
    Equivalence {
        kind: Kind::Number,
        members: &[
            (Id3, "Track", Main),
            (Vorbis, "TrackNumber", Main),
            (Vorbis, "Tracktotal", Total),
            (Vorbis, "Totaltracks", Total),
            (Matroska, "PartNumber", Main),
            (Matroska, "TotalParts", Total),
            (QuickTime, "TrackNumber", Main),
        ],
        dest: &[(Id3, "Track", None), (Vorbis, "TrackNumber", Some("Tracktotal")), (Matroska, "PartNumber", Some("TotalParts"))],
    },
    // The disc number: ID3 TPOS (n/total); Vorbis DISCNUMBER and DISCTOTAL
    // (or TOTALDISCS); QuickTime disk. (Matroska's would be the PART_NUMBER
    // of a level-40 Tag, which ExifTool does not tell from the track's.)
    Equivalence {
        kind: Kind::Number,
        members: &[
            (Id3, "PartOfSet", Main),
            (Vorbis, "Discnumber", Main),
            (Vorbis, "Disctotal", Total),
            (Vorbis, "Totaldiscs", Total),
            (QuickTime, "DiskNumber", Main),
        ],
        dest: &[(Id3, "PartOfSet", None), (Vorbis, "Discnumber", Some("Disctotal"))],
    },
    // The album artist: ID3 TPE2 (Band); Vorbis ALBUMARTIST (or ALBUM
    // ARTIST); QuickTime aART. (Matroska's is the ARTIST of the album-level
    // Tag, the one the writer writes, so not told from the track's.)
    Equivalence {
        kind: Kind::Text,
        members: &[(Id3, "Band", Main), (Vorbis, "Albumartist", Main), (QuickTime, "AlbumArtist", Main)],
        dest: &[(Id3, "Band", None), (Vorbis, "Albumartist", None)],
    },
    // Compilation: ID3 TCMP; Vorbis COMPILATION; QuickTime cpil.
    Equivalence {
        kind: Kind::Flag,
        members: &[(Id3, "Compilation", Main), (Vorbis, "Compilation", Main), (QuickTime, "Compilation", Main)],
        dest: &[(Id3, "Compilation", None), (Vorbis, "Compilation", None)],
    },
    // Beats per minute: ID3 TBPM; Vorbis BPM; Matroska BPM; QuickTime tmpo.
    Equivalence {
        kind: Kind::Text,
        members: &[(Id3, "BeatsPerMinute", Main), (Vorbis, "BPM", Main), (Matroska, "BPM", Main), (QuickTime, "BeatsPerMinute", Main)],
        dest: &[(Id3, "BeatsPerMinute", None), (Vorbis, "BPM", None), (Matroska, "BPM", None)],
    },
    // Lyrics: ID3 USLT; Vorbis LYRICS; Matroska LYRICS; QuickTime ©lyr.
    Equivalence {
        kind: Kind::Text,
        members: &[(Id3, "Lyrics", Main), (Vorbis, "Lyrics", Main), (Matroska, "Lyrics", Main), (QuickTime, "Lyrics", Main)],
        dest: &[(Id3, "Lyrics", None), (Vorbis, "Lyrics", None), (Matroska, "Lyrics", None)],
    },
    // Grouping: ID3 TIT1; Vorbis GROUPING; QuickTime ©grp.
    Equivalence {
        kind: Kind::Text,
        members: &[(Id3, "Grouping", Main), (Vorbis, "Grouping", Main), (QuickTime, "Grouping", Main)],
        dest: &[(Id3, "Grouping", None), (Vorbis, "Grouping", None)],
    },
];

/// The member of `eq` a copied tag is, unless it is of the format `dest`.
fn member_of(eq: &Equivalence, t: &CopiedTag, dest: Format) -> Option<(usize, Role)> {
    let f = Format::of(t).filter(|&f| f != dest)?;
    eq.members.iter().position(|&(mf, name, _)| mf == f && name.eq_ignore_ascii_case(&t.tag)).map(|i| (i, eq.members[i].2))
}

/// The group of a new edit of `tag` for `dest`: none for a tag the writer
/// takes without one, `Vorbis` for a comment field Vorbis.pm does not list.
fn group_for(dest: Format, tag: &str) -> Option<String> {
    (dest == Vorbis && !flac::is_comment_tag(tag)).then(|| "Vorbis".to_string())
}

/// Is `tag` a destination tag of an equivalence in `dest` that needs a
/// group to be written there (a Vorbis field Vorbis.pm does not list)?
fn needs_group(dest: Format, tag: &str) -> bool {
    group_for(dest, tag).is_some()
        && EQUIVALENCES.iter().flat_map(|e| e.dest.iter()).any(|&(f, main, total)| f == dest && (main.eq_ignore_ascii_case(tag) || total.is_some_and(|t| t.eq_ignore_ascii_case(tag))))
}

/// The edits of the tags a copy queues without a destination group, for a
/// file of the format `dest`: each tag as it is (written if the writer has
/// it, by name), and the equivalents of the tags the format names
/// differently (see the module). A tag of `dest`'s own format that the
/// writer takes only with a group (a Vorbis `DISCNUMBER` copied from FLAC to
/// Ogg) gets it.
pub(crate) fn edits_for_copy(dest: Format, copied: &[CopiedTag]) -> Vec<NewValue> {
    let is_member = |t: &CopiedTag| EQUIVALENCES.iter().any(|e| member_of(e, t, dest).is_some());
    let mut out: Vec<NewValue> = Vec::new();
    for t in copied.iter().filter(|t| !is_member(t)) {
        let same = Format::of(t) == Some(dest) && needs_group(dest, &t.tag);
        out.push(NewValue { tag: t.tag.clone(), group: same.then(|| "Vorbis".to_string()), value: Some(t.value.clone()) });
    }
    let native: Vec<String> = out.iter().map(|nv| nv.tag.to_ascii_lowercase()).collect();
    let has = |name: &str| native.iter().any(|n| n.eq_ignore_ascii_case(name));
    for eq in EQUIVALENCES {
        let Some(&(_, name, total_name)) = eq.dest.iter().find(|(f, _, _)| *f == dest) else { continue };
        // the members the source has, the one of the destination's name
        // first, then by the table's order
        let mut found: Vec<(usize, Role, &CopiedTag)> = copied.iter().filter_map(|t| member_of(eq, t, dest).map(|(i, r)| (i, r, t))).collect();
        found.sort_by_key(|&(i, _, t)| (!t.tag.eq_ignore_ascii_case(name), i));
        let first = |role: Role| found.iter().find(|(_, r, _)| *r == role).map(|&(_, _, t)| t);
        let Some(main) = first(Main) else { continue };
        let mut push = |tag: &str, value: String| out.push(NewValue { tag: tag.to_string(), group: group_for(dest, tag), value: Some(value) });
        match eq.kind {
            Kind::Text => {
                if !has(name) {
                    push(name, main.value.clone());
                }
            }
            Kind::Flag => {
                if let (false, Some(v)) = (has(name), flag(&main.value)) {
                    push(name, v.to_string());
                }
            }
            Kind::Date => {
                let Some(mut date) = Date::parse(&main.value) else { continue };
                // (an ID3v2.3 year with its TDAT and TIME)
                if Format::of(main) == Some(Id3) && main.tag.eq_ignore_ascii_case("Year") && date.month.is_none() {
                    date.complete(first(DayMonth).map(|t| t.value.as_str()), first(Time).map(|t| t.value.as_str()));
                }
                if !has(name) {
                    push(name, date.format(dest));
                }
            }
            Kind::Number => {
                let Some((n, mut total)) = number(&main.value) else { continue };
                if total.is_none() {
                    // (the total given apart, of the main tag's format first)
                    let mut totals: Vec<&CopiedTag> = found.iter().filter(|(_, r, _)| *r == Total).map(|&(_, _, t)| t).collect();
                    totals.sort_by_key(|t| Format::of(t) != Format::of(main));
                    total = totals.iter().find_map(|t| number(&t.value)).map(|(n, _)| n);
                }
                match total_name {
                    None => {
                        if !has(name) {
                            push(name, total.map_or_else(|| n.to_string(), |t| format!("{n}/{t}")));
                        }
                    }
                    // (not the total of another number than the one written:
                    // a Matroska stream's TrackNumber is a Vorbis TRACKNUMBER
                    // by name, without the album's TOTAL_PARTS)
                    Some(total_name) if !has(name) => {
                        push(name, n.to_string());
                        if let (false, Some(t)) = (has(total_name), total) {
                            push(total_name, t.to_string());
                        }
                    }
                    Some(_) => {}
                }
            }
        }
    }
    out
}

/// A Yes/No value as 1/0 (ID3 stores TCMP as 1/0, its writer takes either;
/// a Vorbis COMPILATION is 1/0).
fn flag(v: &str) -> Option<&'static str> {
    match v.trim().to_ascii_lowercase().as_str() {
        "yes" | "1" | "true" => Some("1"),
        "no" | "0" | "false" => Some("0"),
        _ => None,
    }
}

/// A number and its total: `5`, `1/5`, `1 of 5` (a total of 0 is none, as
/// QuickTime prints it).
fn number(v: &str) -> Option<(u32, Option<u32>)> {
    let mut runs = v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
    let n = runs.next()?.parse().ok()?;
    let total = runs.next().and_then(|s| s.parse().ok()).filter(|&t| t > 0);
    Some((n, total))
}

/// A date as the formats give it: `2024`, `2024-05-01`, `2024:05:01
/// 12:00:00` (ExifTool's), `2024-05-01T12:00:00Z`...
#[derive(Debug, Clone, PartialEq, Eq)]
struct Date {
    year: String,
    month: Option<String>,
    day: Option<String>,
    /// The time as given (`12:00:00`, `12:00:00+02:00`, `21:17:48Z`).
    time: Option<String>,
}

impl Date {
    fn parse(v: &str) -> Option<Date> {
        let v = v.trim();
        let digits = |s: &str, n: usize| s.len() >= n && s.as_bytes()[..n].iter().all(u8::is_ascii_digit);
        if !digits(v, 4) || v.as_bytes().get(4).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        let mut date = Date { year: v[..4].to_string(), month: None, day: None, time: None };
        let mut rest = &v[4..];
        let part = |rest: &mut &str| -> Option<String> {
            let r = rest.strip_prefix(['-', ':'])?;
            if !digits(r, 2) || r.as_bytes().get(2).is_some_and(u8::is_ascii_digit) {
                return None;
            }
            let p = r[..2].to_string();
            *rest = &r[2..];
            Some(p)
        };
        date.month = part(&mut rest);
        if date.month.is_some() {
            date.day = part(&mut rest);
        }
        if date.day.is_some() {
            if let Some(t) = rest.strip_prefix([' ', 'T']).map(str::trim).filter(|t| digits(t, 2) && t.as_bytes().get(2) == Some(&b':')) {
                date.time = Some(t.to_string());
            }
        }
        Some(date)
    }

    /// An ID3v2.3 year completed by its TDAT (`DDMM`) and TIME (`HHMM`).
    fn complete(&mut self, day_month: Option<&str>, time: Option<&str>) {
        let four = |s: &str| (s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit())).then(|| (s[..2].to_string(), s[2..].to_string()));
        let Some((d, m)) = day_month.map(str::trim).and_then(four) else { return };
        self.day = Some(d);
        self.month = Some(m);
        if let Some((h, mi)) = time.map(str::trim).and_then(four) {
            self.time = Some(format!("{h}:{mi}"));
        }
    }

    /// The date as `dest` stores it: a year for ID3 `Year` (TYER and ID3v1
    /// hold four digits), ISO 8601 for a Vorbis `DATE`
    /// (`2024-05-01T12:00:00`), the Matroska spec's form for its dates
    /// (`2024-05-01 12:00:00`).
    fn format(&self, dest: Format) -> String {
        if dest == Id3 {
            return self.year.clone();
        }
        let mut s = self.year.clone();
        for p in [&self.month, &self.day].into_iter().flatten() {
            s.push('-');
            s.push_str(p);
        }
        if let Some(t) = &self.time {
            s.push(if dest == Vorbis { 'T' } else { ' ' });
            s.push_str(t);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(g0: &str, g1: &str, tag: &str, value: &str) -> CopiedTag {
        CopiedTag { tag: tag.into(), value: value.into(), family0: Some(g0.into()), family1: Some(g1.into()) }
    }

    fn edits(dest: Format, copied: &[CopiedTag]) -> Vec<(Option<String>, String, String)> {
        edits_for_copy(dest, copied).into_iter().map(|nv| (nv.group, nv.tag, nv.value.unwrap())).collect()
    }

    fn e(group: Option<&str>, tag: &str, value: &str) -> (Option<String>, String, String) {
        (group.map(Into::into), tag.into(), value.into())
    }

    #[test]
    fn dates() {
        assert_eq!(Date::parse("2006").unwrap().format(Vorbis), "2006");
        assert_eq!(Date::parse("2010:02:03 21:17:48Z").unwrap().format(Vorbis), "2010-02-03T21:17:48Z");
        assert_eq!(Date::parse("2010-02-03T21:17:48").unwrap().format(Matroska), "2010-02-03 21:17:48");
        assert_eq!(Date::parse("2010:02:03 21:17:48").unwrap().format(Id3), "2010");
        assert_eq!(Date::parse("2010-02").unwrap().format(Vorbis), "2010-02");
        assert_eq!(Date::parse("20100"), None);
        assert_eq!(Date::parse("May 2010"), None);
    }

    #[test]
    fn id3_to_vorbis() {
        let copied = [
            tag("ID3", "ID3v2_3", "Title", "T"),
            tag("ID3", "ID3v2_3", "Year", "2005"),
            tag("ID3", "ID3v2_3", "Date", "0105"),
            tag("ID3", "ID3v2_3", "Time", "1230"),
            tag("ID3", "ID3v2_3", "Track", "1/5"),
            tag("ID3", "ID3v2_3", "PartOfSet", "2/3"),
            tag("ID3", "ID3v2_3", "Band", "AA"),
            tag("ID3", "ID3v2_3", "Compilation", "Yes"),
        ];
        assert_eq!(
            edits(Vorbis, &copied),
            [
                e(None, "Title", "T"),
                e(None, "Date", "2005-05-01T12:30"),
                e(None, "TrackNumber", "1"),
                e(Some("Vorbis"), "Tracktotal", "5"),
                e(Some("Vorbis"), "Discnumber", "2"),
                e(Some("Vorbis"), "Disctotal", "3"),
                e(Some("Vorbis"), "Albumartist", "AA"),
                e(Some("Vorbis"), "Compilation", "1"),
            ]
        );
    }

    #[test]
    fn quicktime_to_each() {
        let copied = [
            tag("QuickTime", "ItemList", "TrackNumber", "1 of 2"),
            tag("QuickTime", "ItemList", "DiskNumber", "1 of 3"),
            tag("QuickTime", "ItemList", "ContentCreateDate", "2006"),
            tag("QuickTime", "ItemList", "AlbumArtist", "album artist"),
        ];
        assert_eq!(edits(Id3, &copied), [e(None, "Year", "2006"), e(None, "Track", "1/2"), e(None, "PartOfSet", "1/3"), e(None, "Band", "album artist")]);
        assert_eq!(edits(Matroska, &copied), [e(None, "DateReleased", "2006"), e(None, "PartNumber", "1"), e(None, "TotalParts", "2")]);
    }

    #[test]
    fn a_name_match_comes_first() {
        // (an unrelated tag of the destination's name wins: no equivalent)
        let copied = [tag("XMP", "XMP-dc", "Date", "2001"), tag("ID3", "ID3v2_4", "RecordingTime", "2002:03:04")];
        assert_eq!(edits(Vorbis, &copied), [e(None, "Date", "2001")]);
        // (among the equivalents, the one of the destination's name, then the table's order)
        let copied = [tag("ID3", "ID3v1", "Year", "2003"), tag("ID3", "ID3v2_4", "RecordingTime", "2002:03:04")];
        assert_eq!(edits(Matroska, &copied), [e(None, "DateReleased", "2002-03-04")]);
        let copied = [tag("Matroska", "Matroska", "DateTimeOriginal", "2001:01:01"), tag("Matroska", "Matroska", "DateReleased", "2002:02:02")];
        assert_eq!(edits(Vorbis, &copied), [e(None, "Date", "2002-02-02")]);
        // (the segment's muxing date is no recording date)
        let copied = [tag("Matroska", "Info", "DateTimeOriginal", "2010:02:03 21:17:48Z")];
        assert_eq!(edits(Id3, &copied), [e(None, "DateTimeOriginal", "2010:02:03 21:17:48Z")]);
    }

    #[test]
    fn the_same_format_is_not_converted() {
        let copied = [tag("Vorbis", "Vorbis", "TrackNumber", "1/5"), tag("Vorbis", "Vorbis", "Discnumber", "2"), tag("Vorbis", "Vorbis", "Date", "2006")];
        assert_eq!(edits(Vorbis, &copied), [e(None, "TrackNumber", "1/5"), e(Some("Vorbis"), "Discnumber", "2"), e(None, "Date", "2006")]);
        let copied = [tag("Vorbis", "Vorbis", "TrackNumber", "1"), tag("Vorbis", "Vorbis", "Tracktotal", "5"), tag("Vorbis", "Vorbis", "Date", "2006-01-02")];
        assert_eq!(edits(Id3, &copied), [e(None, "Year", "2006"), e(None, "Track", "1/5")]);
    }
}
