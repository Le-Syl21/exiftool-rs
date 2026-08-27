#!/usr/bin/perl
# Generate the family-2 (category) lookup tables from ExifTool itself.
#
# ExifTool derives family 2 from each tag's `Groups => { 2 => '...' }`, falling
# back to the containing table's `GROUPS` default. Rather than re-parse the
# ~200 `Image::ExifTool::*` modules by hand, we ask ExifTool to do it: `-listx`
# dumps every table with its `g0`/`g1`/`g2` defaults and every tag with the
# families it overrides. That output is ExifTool's own resolution of the
# fallback chain, so it is authoritative by construction and follows upstream
# across releases.
#
# The same tag NAME can legitimately carry different categories in different
# tables (`BitsPerSample` is Image in one place and Audio in another), so the
# result is emitted as three tiers of decreasing specificity, keyed on the
# families we already assign:
#
#   FAMILY2_BY_G0_G1_NAME   "<g0>\x01<g1>\x01<name>"
#   FAMILY2_BY_G0_NAME      "<g0>\x01<name>"
#   FAMILY2_BY_NAME         "<name>"
#
# A key can legitimately carry several categories, one per table that defines
# it, and all of them are kept, most frequent first. ExifTool has no name
# lookup at all — a tag's category comes from its own table — so these tiers
# are an approximation for a reader that does not know which table it read.
# Listing every category lets `crate::tags::group2::family2_for` keep whatever
# the parser already decided when the parser DOES know, and fall back to the
# common case when it does not.
#
# Keeping only the most frequent one was a majority vote, and it lost: Sony's
# LensType is Image in nine sub-tables and Camera in the Main table, so the
# vote overwrote the Camera the reader had correctly stamped.
#
# A tier entry is omitted when its winning categories are identical to the ones
# the next (less specific) tier would return, which is the common case — that
# prunes ~50000 redundant rows down to ~25000 meaningful ones.
#
# Usage (from the crate root):
#   perl scripts/gen_group2.pl [path/to/exiftool/dir] > src/tags/group2_generated.rs
#
# Default Perl ExifTool location: /home/sylvain/dev/exiftool.

use strict;
use warnings;

my $exiftool_dir = $ARGV[0] || '/home/sylvain/dev/exiftool';
die "Perl ExifTool not found at $exiftool_dir/exiftool\n"
    unless -f "$exiftool_dir/exiftool";

unshift @INC, "$exiftool_dir/lib";
require Image::ExifTool;
require Image::ExifTool::XMP;

my $version = $Image::ExifTool::VERSION;

# ── Collect (g0, g1, name) => { category => table count } ───────────────────
#
# The walk mirrors `Image::ExifTool::TagInfoXML::Write` — load every table,
# flatten the XMP structures, then ask ExifTool itself for each tag's groups —
# with one deliberate difference. TagInfoXML overwrites family 1 with the tag's
# WriteGroup before printing ("use common write group for group 1 (unless
# fake)", TagInfoXML.pm line 191), so the `g1` attribute in `-listx` output is
# the group a tag is WRITTEN to, not the group it is READ in. Every writable
# maker-note tag carries `WriteGroup => 'MakerNotes'`, so `-listx` files them
# all under family 1 `MakerNotes` and hides the per-maker group they are
# actually read in (`Minolta`, `Casio`, `Pentax`, `Sigma`, …). `GetGroup`
# returns the real read groups, including genuine per-tag `Groups => { 1 => ...
# }` overrides such as H264's GPS tags (H264.pm line 241).
Image::ExifTool::LoadAllTables();
my $et = Image::ExifTool->new;
no warnings 'once';    # %Image::ExifTool::allTables is populated above

# ── The family-2 override a top-level XMP tag carries ON ITS OWN ────────────
#
# A field of a variable-namespace XMP structure (`NAMESPACE => undef`, the only
# pre-defined one being MWG's %sExtensions, MWG.pm line 406) does not take its
# category from the namespace it is written in. XMP.pm lines 3573-3583 build its
# tagInfo from the corresponding top-level tag and copy across *only that tag's
# own* `Groups{2}`:
#
#     delete $$tagInfo{Groups};
#     $$tagInfo{Groups}{2} = $$sti{Groups}{2} if $$sti{Groups};
#
# so a top-level tag that relies on its table's GROUPS default carries nothing
# over, and the field falls back to the containing structure's table default.
# The three tiers below cannot express this: they store the *resolved* category,
# with the table default already merged in, and cannot tell an explicit
# `Groups => { 2 => ... }` from an inherited one.
#
# This must be collected BEFORE the main walk: `GetGroup` fills a tagInfo's
# missing families in from its table and flags it `GotGroups`
# (ExifTool.pm lines 3828-3838), which erases the very distinction we are after.
#
# The tables walked are exactly the ones XMP.pm line 3591 reaches through
# `$Image::ExifTool::XMP::Main{$ns}{SubDirectory}{TagTable}`. The empty string
# records "this tag has no `Groups{2}` of its own".
# ── The category an XMP namespace gives a property it does not describe ─────
#
# HandleXMPTag looks the property's namespace up in `%Image::ExifTool::XMP::Main`
# and reads its table (XMP.pm lines 3445-3448); only when there is no such entry
# does it fall back to `Image::ExifTool::XMP::other` (line 3471), whose
# `GROUPS => { 2 => 'Unknown' }` (line 2740) is what an un-schemed namespace
# gets. A property a KNOWN namespace has no entry for is invented in that
# namespace's own table (line 3595) and so takes ITS default — `XMP-pdfx` has
# `GROUPS => { 1 => 'XMP-pdfx', 2 => 'Document' }` (XMP.pm line 1246), which is
# why an arbitrary pdfx property is Document and not Unknown.
my %ns_default;
my %ns_raw_keys;
my %own;
for my $ns (sort keys %Image::ExifTool::XMP::Main) {
    my $tg = $Image::ExifTool::XMP::Main{$ns};
    next unless ref $tg eq 'HASH' and $$tg{SubDirectory};
    my $tbl = Image::ExifTool::GetTagTable($$tg{SubDirectory}{TagTable}) or next;
    my $g1 = $$tbl{GROUPS}{1};
    my $g2 = $$tbl{GROUPS}{2};
    $ns_default{$g1} = $g2 if defined $g1 and defined $g2 and $g1 =~ /^XMP-/;
    # ── The namespace's case-sensitive raw hash keys ────────────────────────
    #
    # HandleXMPTag resolves a top-level property with `GetTagInfo`, which keys
    # the namespace table on the property's raw XML local name CASE-SENSITIVELY
    # (XMP.pm line 3591). A `dc:Creator` therefore never finds the `creator`
    # entry: it is invented instead (line 3595) and takes the table's GROUPS{2}
    # default (`%ns_default` above). Collected BEFORE AddFlattenedTags, so only
    # the real property keys are recorded — a struct field (`exif:Flash`'s
    # `Fired`) is not a top-level key and is resolved through its struct, not by
    # this lookup. The keys are NOT uniformly `lcfirst(Name)` (`exif:ExifVersion`
    # is stored under `ExifVersion`), so they must come from the table itself.
    if (defined $g1 and $g1 =~ /^XMP-/) {
        $ns_raw_keys{$g1}{$_} = 1 for Image::ExifTool::TagTableKeys($tbl);
    }
    Image::ExifTool::XMP::AddFlattenedTags($tbl);
    for my $tag_id (Image::ExifTool::TagTableKeys($tbl)) {
        for my $ti (Image::ExifTool::GetTagInfoList($tbl, $tag_id)) {
            my $name = $$ti{Name};
            next unless defined $name and $name =~ /^[A-Za-z0-9_\-]+$/;
            $own{$name}{ ($$ti{Groups} && $$ti{Groups}{2}) ? $$ti{Groups}{2} : '' } = 1;
        }
    }
}

my (%k3, %k2, %k1);
for my $table_name (sort keys %Image::ExifTool::allTables) {
    my $table = Image::ExifTool::GetTagTable($table_name);
    # Structured XMP properties are also extracted under flattened names; the
    # table only grows them on demand, exactly as TagInfoXML does.
    Image::ExifTool::XMP::AddFlattenedTags($table)
        if $$table{GROUPS} and ($$table{GROUPS}{0} || '') eq 'XMP';
    for my $tag_id (Image::ExifTool::TagTableKeys($table)) {
        for my $tag_info (Image::ExifTool::GetTagInfoList($table, $tag_id)) {
            # Same filter as TagInfoXML: a sub-directory that cannot be written
            # is a container, not a tag, and hidden tags are never extracted.
            next unless $$tag_info{Writable} or not $$tag_info{SubDirectory};
            next if $$tag_info{Hidden};
            my $name = $$tag_info{Name};
            next unless defined $name and $name =~ /^[A-Za-z0-9_\-]+$/;
            my ($g0, $g1, $g2) = $et->GetGroup($tag_info);
            $_ = defined($_) ? $_ : '' for ($g0, $g1, $g2);
            # The pseudo-tag ForceWrite has "*" for every family: it belongs to
            # no group.
            next if grep { !length($_) || $_ eq '*' } ($g0, $g1, $g2);

            $k3{"$g0\x01$g1\x01$name"}{$g2}++;
            $k2{"$g0\x01$name"}{$g2}++;
            $k1{$name}{$g2}++;
        }
    }
}

# ── Composite tags: every category counts equally ───────────────────────────
#
# AddCompositeTags merges every module's Composite table into the single
# `%Image::ExifTool::Composite` (ExifTool.pm line 2302), whose GROUPS supply
# families 0 and 1, so all Composite tags share one key per NAME. A name several
# modules define then collapses: `Duration` is `Other` in AIFF::Composite and
# Vorbis::Composite, `Audio` in APE::Composite and `Video` in MPEG::Composite.
# Frequency means nothing here — each entry is a distinct definition and only the
# composite engine knows which one it evaluated — so level the counts and let
# every category through, which makes `family2_for` defer to the caller.
for my $h (\%k3, \%k2) {
    for my $key (keys %$h) {
        next unless $key =~ /^Composite\x01/;
        $_ = 1 for values %{$h->{$key}};
    }
}

# The categories tied for the most tables under one key, sorted. Everything
# else is dropped: it never wins, so storing it would only bloat the tables.
# Every category this key is seen with, the most frequent first.
#
# Keeping only the most frequent one was a majority vote: Sony's LensType is
# Image in nine sub-tables and Camera in the Main table, and the vote erased
# the Camera the reader had correctly stamped. `choose` prefers whichever
# candidate the reader already picked, so listing them all lets a reader that
# knows its table keep its answer, while one with no opinion still gets the
# common case first.
sub top {
    my ($h, $all) = @_;
    return () unless $h;
    # The most specific tier lists every category it has seen, most frequent
    # first, so a reader that knows which table it read keeps its answer. The
    # looser tiers keep only the winner: there, a disagreement is far more
    # likely to be a reader's arbitrary default than a real table difference,
    # and deferring to it would preserve the mistake. `Model` alone is Camera,
    # Video, Audio, Image and Other across ExifTool.
    return sort { $h->{$b} <=> $h->{$a} or $a cmp $b } keys %$h if $all;
    my $max = 0;
    for my $c (values %$h) { $max = $c if $c > $max }
    return sort grep { $h->{$_} == $max } keys %$h;
}

sub set_key { my ($h, $all) = @_; return join(',', top($h, $all)) }

# ── Prune tiers that add nothing over the next fallback ─────────────────────
#
# Mirrors the tier chain of `family2_for`, including its two special cases:
#
#   * an XMP property that no tier matches is Unknown
#     (`%Image::ExifTool::XMP::other`) and never reaches the bare-name tier, so
#     XMP keys must be pruned against "Unknown" rather than against `%k1`;
#   * every family-0 `XML` table invents a tag for each property it meets, so a
#     name that reaches the end of the chain has no entry anywhere and
#     `family2_for` keeps the reader's category instead of guessing from the
#     bare name. Nothing may be pruned there: a pruned key would fall through to
#     that guard rather than to `%k1`.
my $NEVER = "\x00";    # cannot be a category, so no key is ever pruned against it

sub fallback_after_g0 {
    my ($g0, $name) = @_;
    return 'Unknown' if $g0 eq 'XMP';
    return $NEVER if $g0 eq 'XML';
    return set_key($k1{$name});
}

my %keep2;
for my $key (sort keys %k2) {
    my ($g0, $name) = split /\x01/, $key;
    # An XMP property's family 1 IS its namespace, and ExifTool resolves the
    # property in that namespace's table alone (XMP.pm line 3591 reaches it
    # through `$Image::ExifTool::XMP::Main{$ns}{SubDirectory}{TagTable}`); a
    # name it does not hold goes to `XMP::other`, Unknown. A tier keyed on the
    # bare name would answer for a namespace the property was never in, so
    # `family2_for` does not consult tier 2 under XMP and nothing is emitted
    # for it. Tier 3 is then pruned against Unknown, which keeps every XMP key.
    next if $g0 eq 'XMP';
    $keep2{$key} = 1 if set_key($k2{$key}) ne fallback_after_g0($g0, $name);
}
my %keep3;
for my $key (sort keys %k3) {
    my ($g0, undef, $name) = split /\x01/, $key;
    my $fallback = $keep2{"$g0\x01$name"}
        ? set_key($k2{"$g0\x01$name"})
        : fallback_after_g0($g0, $name);
    $keep3{$key} = 1 if set_key($k3{$key}, 1) ne $fallback;
}

# ── Emit ────────────────────────────────────────────────────────────────────
print <<"HEADER";
//! Auto-generated family-2 (category) lookup tables, from ExifTool $version.
//!
//! Generated by `scripts/gen_group2.pl`. DO NOT EDIT MANUALLY.
//!
//! Three tiers of decreasing specificity, each sorted by key so the lookup in
//! [`super::group2`] can binary-search them. A key maps to every category
//! ExifTool most often assigns to it; more than one means the tables tie and the
//! category depends on which table the tag came from.

/// `(key, categories)`, sorted by key.
pub type Family2Entry = (&'static str, &'static [&'static str]);
HEADER

sub emit {
    my ($var, $doc, $map, $keep, $all) = @_;
    my @keys = sort grep { !$keep || $keep->{$_} } keys %$map;
    printf "\n/// %s (%d entries).\npub static %s: &[Family2Entry] = &[\n", $doc, scalar @keys, $var;
    for my $key (@keys) {
        my $rust_key = $key;
        $rust_key =~ s/\x01/\\u{1}/g;
        my $cats = join ', ', map { "\"$_\"" } top($map->{$key}, $all);
        print "    (\"$rust_key\", &[$cats]),\n";
    }
    print "];\n";
}

emit('FAMILY2_BY_G0_G1_NAME', 'Keyed on `"<family0>\\u{1}<family1>\\u{1}<name>"`', \%k3, \%keep3, 1);
emit('FAMILY2_BY_G0_NAME',    'Keyed on `"<family0>\\u{1}<name>"`',                \%k2, \%keep2);
emit('FAMILY2_BY_NAME',       'Keyed on `"<name>"`',                               \%k1, undef);

# ── Emit the per-namespace default ──────────────────────────────────────────
{
    my @keys = sort keys %ns_default;
    printf "\n/// The family 2 each XMP namespace table gives a property it does not\n"
        . "/// describe (%d entries), sorted by group name.\n"
        . "///\n/// A namespace ExifTool has no table for is absent: such a property goes to\n"
        . "/// `%%Image::ExifTool::XMP::other` instead, whose category is Unknown.\n"
        . "pub static XMP_NAMESPACE_FAMILY2: &[(&str, &str)] = &[\n", scalar @keys;
    print qq{    ("$_", "$ns_default{$_}"),\n} for @keys;
    print "];\n";
}

# ── Emit the per-namespace case-sensitive raw-key set ───────────────────────
#
# The actual hash keys of each XMP namespace table, exactly as `GetTagInfo`
# keys them (XMP.pm line 3591). A top-level property whose raw local name is
# absent here is one ExifTool has no entry for and invents in the namespace's
# own table (line 3595), so it takes that table's category from
# `XMP_NAMESPACE_FAMILY2` rather than the category of a same-named real tag.
{
    my @nss = sort keys %ns_raw_keys;
    printf "\n/// Case-sensitive raw XML local names each XMP namespace table holds\n"
        . "/// (%d namespaces), keyed on family 1 and sorted.\n"
        . "///\n/// These are the hash keys `GetTagInfo` matches against; a top-level\n"
        . "/// property whose local name is absent is invented and takes the namespace\n"
        . "/// table's default category (see [`XMP_NAMESPACE_FAMILY2`]).\n"
        . "pub static XMP_NAMESPACE_RAW_KEYS: &[(&str, &[&str])] = &[\n", scalar @nss;
    for my $g1 (@nss) {
        my @keys = sort keys %{$ns_raw_keys{$g1}};
        my $list = join ', ', map { "\"$_\"" } @keys;
        print qq{    ("$g1", &[$list]),\n};
    }
    print "];\n";
}

# ── Emit the XMP own-override table ─────────────────────────────────────────
#
# See the collection of %own above for the rule this serves. A name that yields
# several different answers is undecidable from a flattened tag name alone (the
# field's namespace is not recoverable from it) and the caller keeps what it
# had. Names whose only answer is "" are omitted: the caller already falls back
# to the containing structure's table default when a name is absent.
{
    my @keys = sort grep { join('', keys %{$own{$_}}) ne '' } keys %own;
    printf "\n/// Own family-2 override of each top-level XMP tag NAME (%d entries).\n"
        . "///\n/// `\"\"` means the tag declares none of its own. Several entries mean the\n"
        . "/// answer depends on the namespace and cannot be decided from the name.\n"
        . "pub static XMP_OWN_FAMILY2_BY_NAME: &[Family2Entry] = &[\n", scalar @keys;
    for my $name (@keys) {
        my $cats = join ', ', map { "\"$_\"" } sort keys %{$own{$name}};
        print "    (\"$name\", &[$cats]),\n";
    }
    print "];\n";
}
