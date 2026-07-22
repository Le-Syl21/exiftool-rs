#!/usr/bin/perl
# Generate the sets of tags ExifTool stores at priority 0.
#
# `FoundTag` (ExifTool.pm:9469-9472) resolves a tag's priority in three steps:
#
#     my $priority = $$tagInfo{Priority};
#     unless (defined $priority) {
#         $priority = $$tbl{PRIORITY};
#         $priority = 0 if not defined $priority and $$tagInfo{Avoid};
#     }
#
# so `Avoid => 1` demotes a tag to priority 0 whenever neither the tag nor its
# table states a priority of its own. A priority-0 duplicate never displaces a
# stored value (the stored one is promoted to 1 first), which is how ExifTool
# keeps `IPTC:TimeCreated` even though `XMP-photomech:TimeCreated` is found
# later in the file.
#
# XMP is the one family where a name lookup is safe: its family-1 group is the
# namespace, so `(XMP-photomech, TimeCreated)` names exactly one tagInfo. A
# check over every table confirms it — no `(XMP-*, name)` pair carries priority
# 0 in one table and a higher priority in another.
#
# EXIF and QuickTime cannot use a name key — `IFD0`, `UserData` and `ItemList`
# each hold two tagInfos of the same name at different priorities (Exif.pm's
# 0xfe54 Contrast against 0xa408 Contrast, QuickTime.pm's `titl` against
# `©nam` Title). They are keyed on the tag ID instead, which is what the
# decoder has in hand when it builds the tag:
#
#   * EXIF     — the numeric IFD tag, over every table whose family 0 is EXIF.
#                Keyed on `(id, name)` because 0x011c is PlanarConfiguration
#                (priority 0) in Exif::Main and Gamma (priority 1) in
#                PanasonicRaw::Main; the pair is collision-free.
#   * QuickTime — the atom code, per table (ItemList / UserData / Keys).
#
# Usage: perl scripts/gen_priority0.pl ../exiftool > src/tags/priority0_generated.rs

use strict;
use warnings;

my $dir = shift or die "usage: $0 <path-to-exiftool-checkout>\n";
unshift @INC, "$dir/lib";
require Image::ExifTool;

Image::ExifTool::LoadAllTables();
my $all = \%Image::ExifTool::allTables; ## no critic

# Resolve a tagInfo's priority exactly as FoundTag does, or undef when the
# tagInfo carries no priority of its own (ExifTool's "normal default" of 1).
sub resolve_priority {
    my ($tagInfo, $tablePriority) = @_;
    my $priority = $$tagInfo{Priority};
    unless (defined $priority) {
        $priority = $tablePriority;
        $priority = 0 if not defined $priority and $$tagInfo{Avoid};
    }
    return $priority;
}

my %seen;     # XMP: "group1\tname"
my %below;    # XMP: "group1\tname", for a priority BELOW 0
my %exif;     # EXIF: numeric id => { name => priority }
my %qt;       # QuickTime: "TableName\tatom code" => priority

# The QuickTime tables our reader decodes atom by atom. Microsoft::Xtra is
# listed for completeness of the record, even though its 452 priority-0
# properties are all `Avoid => 1` and share one family-1 group (`Microsoft`).
my %QT_TABLES = (
    'Image::ExifTool::QuickTime::ItemList' => 'ItemList',
    'Image::ExifTool::QuickTime::UserData' => 'UserData',
    'Image::ExifTool::QuickTime::Keys'     => 'Keys',
);

foreach my $tableName (sort keys %$all) {
    my $table = Image::ExifTool::GetTagTable($tableName);
    my $family0 = $$table{GROUPS}{0} // '';
    my $tablePriority = $$table{PRIORITY};
    my $qtGroup = $QT_TABLES{$tableName};
    next unless $family0 eq 'XMP' or $family0 eq 'EXIF' or $qtGroup;
    foreach my $id (Image::ExifTool::TagTableKeys($table)) {
        foreach my $tagInfo (Image::ExifTool::GetTagInfoList($table, $id)) {
            next unless ref $tagInfo eq 'HASH';
            next if $$tagInfo{SubDirectory} and not defined $$tagInfo{Writable};
            my $priority = resolve_priority($tagInfo, $tablePriority);
            my $name = $$tagInfo{Name};

            if ($qtGroup) {
                my $key = "$qtGroup\t$id";
                my $p = defined $priority ? $priority : 1;
                # An atom code that is priority 0 for one tagInfo and not for
                # another would make the key ambiguous; none is, and the check
                # keeps it that way.
                die "QuickTime $key: priority $p vs $qt{$key}\n"
                    if defined $qt{$key} and $qt{$key} != $p;
                $qt{$key} = $p;
            }

            if ($family0 eq 'EXIF' and $id =~ /^\d+$/) {
                my $p = defined $priority ? $priority : 1;
                die sprintf("EXIF 0x%04x %s: priority %d vs %d\n", $id, $name, $p,
                            $exif{$id}{$name})
                    if defined $exif{$id}{$name} and $exif{$id}{$name} != $p;
                $exif{$id}{$name} = $p;
            }

            next unless $family0 eq 'XMP';
            next unless defined $priority and $priority <= 0;
            my $group1 = $$table{GROUPS}{1}
                // ($$tagInfo{Groups} ? $$tagInfo{Groups}{1} : undef)
                // next;
            next unless $group1 =~ /^XMP-/;
            if ($priority < 0) {
                $below{"$group1\t$name"} = 1;
            } else {
                $seen{"$group1\t$name"} = 1;
            }
        }
    }
}

my @entries = sort keys %seen;

# Rust byte-string literal for an atom code, which may hold the \xa9 of the
# classic QuickTime "©" atoms.
sub byte_literal {
    my ($s) = @_;
    my $out = '';
    foreach my $c (split //, $s) {
        my $o = ord $c;
        if ($o >= 0x20 and $o < 0x7f and $c ne '"' and $c ne '\\') {
            $out .= $c;
        } else {
            $out .= sprintf('\\x%02x', $o);
        }
    }
    return qq{b"$out"};
}

print <<"HEADER";
//! AUTO-GENERATED by `scripts/gen_priority0.pl` — do not edit.
//!
//! The tags ExifTool stores at priority 0, so that finding one never displaces
//! a value already stored under the same tag name. Most get there through
//! `Avoid => 1`, which `FoundTag` (ExifTool.pm:9472) turns into a priority of 0
//! when neither the tag nor its table sets one; the rest carry an explicit
//! `Priority => 0`, on the tag (Exif.pm) or on the table (the XMP-exif,
//! XMP-tiff and XMP-exifEX mirrors).
//!
//! Both cases leave the resolved priority *defined*, so both are promoted to 1
//! inside the PRIORITY_DIR (ExifTool.pm:9552-9555) — `Avoid` and
//! `Priority => 0` are indistinguishable downstream.
//!
//! XMP is keyed on `(family 1, tag name)`, which is unambiguous there because
//! family 1 is the namespace. EXIF and QuickTime are keyed on the tag ID, which
//! is what their decoders hold: a name key would be ambiguous, since `IFD0`,
//! `ItemList` and `UserData` each carry two same-named tags of different
//! priority.

/// `(family1, name)` pairs, sorted for binary search.
static XMP_PRIORITY0: &[(&str, &str)] = &[
HEADER

foreach my $entry (@entries) {
    my ($group1, $name) = split /\t/, $entry;
    print qq{    ("$group1", "$name"),\n};
}

print <<'MIDDLE';
];

/// Whether ExifTool stores the XMP property `name` of namespace group `family1`
/// at priority 0.
pub fn xmp_is_priority0(family1: &str, name: &str) -> bool {
    XMP_PRIORITY0
        .binary_search_by(|&(g, n)| g.cmp(family1).then_with(|| n.cmp(name)))
        .is_ok()
}

/// `(family1, name)` pairs ExifTool stores BELOW priority 0, sorted for binary
/// search. A negative priority is not the same as 0: FoundTag only promotes a
/// stored priority that is false — `unless ($oldPriority) { $oldPriority = 1 }`
/// (ExifTool.pm:9544-9551) — and only raises a 0 to 1 inside the PRIORITY_DIR
/// (:9554), so a negative one is left alone on both counts. It therefore never
/// displaces anything and can itself be displaced by a priority-0 tag.
static XMP_PRIORITY_BELOW0: &[(&str, &str)] = &[
MIDDLE

foreach my $entry (sort keys %below) {
    my ($group1, $name) = split /\t/, $entry;
    print qq{    ("$group1", "$name"),\n};
}

print <<'MIDDLE1B';
];

/// Whether ExifTool stores the XMP property `name` of namespace group `family1`
/// below priority 0 — see [`XMP_PRIORITY_BELOW0`].
pub fn xmp_is_below_priority0(family1: &str, name: &str) -> bool {
    XMP_PRIORITY_BELOW0
        .binary_search_by(|&(g, n)| g.cmp(family1).then_with(|| n.cmp(name)))
        .is_ok()
}

/// `(IFD tag id, tag name)` pairs an EXIF-family table stores at priority 0,
/// sorted for binary search. The name is part of the key because 0x011c is
/// PlanarConfiguration (priority 0) in `Exif::Main` but Gamma (priority 1) in
/// `PanasonicRaw::Main`.
static EXIF_PRIORITY0: &[(u16, &str)] = &[
MIDDLE1B

foreach my $id (sort { $a <=> $b } keys %exif) {
    foreach my $name (sort keys %{$exif{$id}}) {
        next unless $exif{$id}{$name} == 0;
        printf("    (0x%04x, \"%s\"),\n", $id, $name);
    }
}

print <<'MIDDLE2';
];

/// Whether ExifTool stores the EXIF tag `id`, read as `name`, at priority 0.
pub fn exif_is_priority0(id: u16, name: &str) -> bool {
    EXIF_PRIORITY0
        .binary_search_by(|&(i, n)| i.cmp(&id).then_with(|| n.cmp(name)))
        .is_ok()
}

/// QuickTime `ItemList` atom codes stored at priority 0, sorted.
#[rustfmt::skip]
static ITEM_LIST_PRIORITY0: &[&[u8]] = &[
MIDDLE2

sub print_qt {
    my ($group) = @_;
    foreach my $key (sort grep { /^\Q$group\E\t/ } keys %qt) {
        next unless $qt{$key} == 0;
        my (undef, $id) = split /\t/, $key, 2;
        print '    ', byte_literal($id), ",\n";
    }
}

print_qt('ItemList');

print <<'MIDDLE3';
];

/// QuickTime `UserData` atom codes stored at priority 0, sorted.
#[rustfmt::skip]
static USER_DATA_PRIORITY0: &[&[u8]] = &[
MIDDLE3

print_qt('UserData');

print <<'MIDDLE4';
];

/// QuickTime `Keys` key names stored at priority 0, sorted.
#[rustfmt::skip]
static KEYS_PRIORITY0: &[&[u8]] = &[
MIDDLE4

print_qt('Keys');

print <<'FOOTER';
];

/// Whether ExifTool stores the `ilst` item of code `atom` at priority 0 when the
/// list is an iTunes item list (`QuickTime::ItemList`).
pub fn item_list_is_priority0(atom: &[u8]) -> bool {
    ITEM_LIST_PRIORITY0.binary_search(&atom).is_ok()
}

/// Whether ExifTool stores the `udta` atom `atom` at priority 0
/// (`QuickTime::UserData`).
pub fn user_data_is_priority0(atom: &[u8]) -> bool {
    USER_DATA_PRIORITY0.binary_search(&atom).is_ok()
}

/// Whether ExifTool stores the `Keys` metadata key `key` at priority 0
/// (`QuickTime::Keys`).
pub fn keys_is_priority0(key: &[u8]) -> bool {
    KEYS_PRIORITY0.binary_search(&key).is_ok()
}
FOOTER
