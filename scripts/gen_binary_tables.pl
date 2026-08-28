#!/usr/bin/perl
# Decoders for ExifTool's binary sub-tables, whatever maker they belong to.
#
# A binary sub-table is a block of bytes addressed by index: ExifTool's
# ProcessBinaryData reads the entry at `($index - FIRST_ENTRY) * sizeof(FORMAT)`
# and a field's own Format says what to read there. This generator turns those
# tables into Rust decoders straight from the Perl, so a table is reached
# because ExifTool defines it and not because someone transcribed it.
#
# It is the maker-agnostic companion to gen_sony_ciphered.pl, which does the
# same for Sony's enciphered blocks and carries the Sony-only machinery --
# the cipher, the Main-table variant selector -- that has no equivalent here.
#
# Everything it cannot express is named on stderr. Nothing is skipped quietly.
#
# Usage: perl scripts/gen_binary_tables.pl /path/to/exiftool/lib > src/tags/binary_tables_generated.rs

use strict;
use warnings;

my $lib = $ARGV[0] || '../exiftool/lib';
die "Cannot find $lib\n" unless -d $lib;

# Which tables to emit, by module. A table named here pulls in the tables its
# own fields point at, so the list is the entry points rather than the closure.
my %WANTED = (
    Canon => [qw(
        ColorData1 ColorData2 ColorData3 ColorData4 ColorData5 ColorData6
        ColorData7 ColorData8 ColorData9 ColorData10 ColorData11 ColorData12
        ColorDataUnknown
    )],
);

# Main-table ids whose sub-table is chosen by a chain of conditions. The arms
# are read from the module's own Main table, so the choice is ExifTool's.
my %SELECTORS = (
    Canon => [0x4001],
);

my %WIDTH = (
    int8u => 1, int8s => 1,
    int16u => 2, int16s => 2,
    int32u => 4, int32s => 4,
    rational32u => 4, rational32s => 4,
);
my %IS_RATIONAL = (rational32u => 1, rational32s => 1);

my @skipped;
my @tables;      # emitted, in order
my %by_name;     # name -> table
my %pending;     # tables still to read, per module

sub note { push @skipped, $_[0] }

# ---------------------------------------------------------------- reading
sub read_module {
    my ($module) = @_;
    open my $h, '<', "$lib/Image/ExifTool/$module.pm" or die "$module.pm: $!\n";
    local $/;
    my $src = <$h>;
    close $h;
    return $src;
}

# The `my %name = (...)` hashes a table splices whole fields out of.
sub shared_hashes {
    my ($src) = @_;
    my %shared;
    while ($src =~ /^my %(\w+)\s*=\s*\((.*?)\n\);/gms) {
        $shared{$1} = $2;
    }
    return %shared;
}

sub table_body {
    my ($src, $module, $table) = @_;
    return $1 if $src =~ /^%Image::ExifTool::\Q$module\E::\Q$table\E\s*=\s*\((.*?)\n\);/ms;
    return undef;
}

# A field body, by counting braces from the line that opens it.
sub scan_fields {
    my ($body) = @_;
    my @out;
    my @lines = split /\n/, $body;
    for (my $i = 0; $i <= $#lines; ++$i) {
        # `0x43 => 'ColorTempAsShot'`: a name and nothing else.
        if ($lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+)\s*=>\s*'([^']+)'\s*,/) {
            push @out, [$1, "Name => '$2',", 0];
            next;
        }
        # `0x47 => [{...},{...}]`: alternatives, the first whose condition
        # holds being the one ExifTool takes.
        if ($lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+)\s*=>\s*\[/) {
            my $off_s = $1;
            my ($depth, $text, $seen) = (0, '', 0);
            for (my $j = $i; $j <= $#lines; ++$j) {
                $text .= $lines[$j] . "\n";
                while ($lines[$j] =~ /\{/g) { $depth++; $seen = 1 }
                $depth-- while $lines[$j] =~ /\}/g;
                if ($seen and $depth <= 0 and $lines[$j] =~ /\]/) { $i = $j; last }
            }
            my (@arms, $cur);
            my $d = 0;
            for my $c (split //, $text) {
                if ($c eq '{') { $d++; $cur = '' unless defined $cur }
                $cur .= $c if defined $cur;
                if ($c eq '}') {
                    $d--;
                    if ($d == 0 and defined $cur) { push @arms, $cur; undef $cur }
                }
            }
            push @out, [$off_s, $_, 1] for @arms;
            next;
        }
        next unless $lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+)\s*=>\s*\{/;
        my $off_s = $1;
        my ($depth, $text) = (0, '');
        for (my $j = $i; $j <= $#lines; ++$j) {
            $text .= $lines[$j] . "\n";
            $depth++ while $lines[$j] =~ /\{/g;
            $depth-- while $lines[$j] =~ /\}/g;
            if ($depth <= 0) { $i = $j; last }
        }
        push @out, [$off_s, $text, 0];
    }
    return @out;
}

# A Condition, with `q{...}` taken by counting braces.
sub field_cond {
    my ($fb) = @_;
    my $c;
    if ($fb =~ /Condition\s*=>\s*q\{/g) {
        my $from = pos($fb);
        my ($depth, $i) = (1, $from);
        while ($i < length($fb) and $depth) {
            my $ch = substr($fb, $i, 1);
            $depth++ if $ch eq '{';
            $depth-- if $ch eq '}';
            ++$i;
        }
        $c = substr($fb, $from, $i - $from - 1) unless $depth;
    }
    ($c) = $fb =~ /Condition\s*=>\s*'([^']*)'/ unless defined $c;
    ($c) = $fb =~ /Condition\s*=>\s*"([^"]*)"/ unless defined $c;
    return undef unless defined $c;
    $c =~ s/\s+/ /g;
    $c =~ s/^ | $//g;
    return $c;
}

my @re_list;
sub re_id {
    my ($pat) = @_;
    for my $i (0 .. $#re_list) { return $i if $re_list[$i] eq $pat }
    push @re_list, $pat;
    return $#re_list;
}

# A condition as a Rust expression, or nothing when it cannot be one.
sub compile_cond {
    my ($cond) = @_;
    $cond =~ s/^\s+|\s+$//g;
    return ('true') if $cond eq '';
    # `A and B`, `A or B` at the top level.
    for my $op (' or ', ' and ') {
        my ($depth, $i) = (0, 0);
        while ($i < length $cond) {
            my $c = substr($cond, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            if (!$depth and substr($cond, $i, length $op) eq $op) {
                my @l = compile_cond(substr($cond, 0, $i));
                my @r = compile_cond(substr($cond, $i + length $op));
                return () unless @l and @r;
                return ($op eq ' or ' ? "($l[0] || $r[0])" : "($l[0] && $r[0])");
            }
            ++$i;
        }
    }
    if ($cond =~ /^\((.*)\)$/) {
        my $inner = $1;
        my ($depth, $ok) = (0, 1);
        for my $i (0 .. length($inner) - 1) {
            my $c = substr($inner, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            $ok = 0 if $depth < 0;
        }
        return compile_cond($inner) if $ok and $depth == 0;
    }
    return ("!(" . (compile_cond($1))[0] . ")") if $cond =~ /^not (.*)$/ and compile_cond($1);
    if ($cond =~ m{^\$\$self\{Model\} (=~|!~) /(.*)/$}) {
        my ($op, $pat) = ($1, $2);
        return () if $pat =~ /\(\?[=!<]/;
        return sprintf('%sMODEL_RE_%d.is_match(model)', $op eq '!~' ? '!' : '', re_id($pat));
    }
    if ($cond =~ /^\$count (==|!=|<=|>=|<|>) (\d+)$/) {
        return "count $1 $2";
    }
    if ($cond =~ /^\$format (eq|ne) "(\w+)"$/) {
        return sprintf('format %s "%s"', $1 eq 'eq' ? '==' : '!=', $2);
    }
    if ($cond =~ m{^\$\$valPt (=~|!~) /(.*?)/[a-z]*$}) {
        my ($op, $re) = ($1, $2);
        my $pat = byte_prefix($re);
        return () unless defined $pat;
        return ($op eq '!~' ? '!' : '') . "prefix_matches(data, $pat)";
    }
    # `$$self{ColorDataVersion} == -3`: what an earlier field of this table
    # stored under that name.
    if ($cond =~ /^\$\$self\{(\w+)\} (==|!=|<=|>=|<|>) (-?[\d.]+)$/) {
        my ($dm, $op, $num) = ($1, $2, $3);
        $num .= '.0' unless $num =~ /\./;
        return sprintf('dm_get(dm, "%s").is_some_and(|v| v %s %s)', $dm, $op, $num);
    }
    if ($cond =~ /^defined \$\$self\{(\w+)\}$/) {
        return sprintf('dm_get(dm, "%s").is_some()', $1);
    }
    if ($cond =~ /^\$\$self\{(\w+)\}$/) {
        return sprintf('dm_get(dm, "%s").is_some_and(|v| v != 0.0)', $1);
    }
    return ();
}

# `^[\0-\x40]` and friends: a fixed prefix of bytes, as a Rust slice of
# Option<(min, max)> -- None where the pattern accepts anything.
sub byte_prefix {
    my ($re) = @_;
    return undef unless $re =~ s/^\^//;
    my @out;
    while (length $re) {
        if ($re =~ s/^\[\\?(\\x[0-9a-fA-F]{2}|\\0|.)-\\?(\\x[0-9a-fA-F]{2}|\\0|.)\]//) {
            my ($a, $b) = (chr_of($1), chr_of($2));
            return undef unless defined $a and defined $b;
            push @out, "Some(($a, $b))";
            next;
        }
        if ($re =~ s/^(\\x[0-9a-fA-F]{2}|\\0)//) {
            my $c = chr_of($1);
            return undef unless defined $c;
            push @out, "Some(($c, $c))";
            next;
        }
        if ($re =~ s/^\.//) { push @out, 'None'; next }
        if ($re =~ s/^([A-Za-z0-9 _\/-])//) {
            my $c = ord $1;
            push @out, "Some(($c, $c))";
            next;
        }
        return undef;
    }
    return '&[' . join(', ', @out) . ']';
}

sub chr_of {
    my ($s) = @_;
    return hex(substr($s, 2)) if $s =~ /^\\x/;
    return 0 if $s eq '\\0';
    return ord($s) if length($s) == 1;
    return undef;
}

# ---------------------------------------------------------------- parsing
sub parse_table {
    my ($module, $table, $src, $shared) = @_;
    return if $by_name{"$module\::$table"};
    my $body = table_body($src, $module, $table);
    unless (defined $body) {
        note("$module\::$table: no such table in the module");
        return;
    }
    # Comment-only lines are not code: a commented-out field read as live is
    # a tag ExifTool never has.
    $body =~ s/^\s*#.*$//gm;

    my ($fmt) = $body =~ /FORMAT\s*=>\s*'(\w+)'/;
    $fmt ||= 'int8u';
    unless (exists $WIDTH{$fmt}) {
        note("$module\::$table: FORMAT '$fmt'");
        return;
    }
    my ($first) = $body =~ /FIRST_ENTRY\s*=>\s*(\d+)/;
    $first = 0 unless defined $first;
    my ($grp2) = $body =~ /GROUPS\s*=>\s*\{[^}]*2\s*=>\s*'(\w+)'/;
    $grp2 ||= 'Image';
    my $prio0 = $body =~ /PRIORITY\s*=>\s*0/ ? 1 : 0;

    my $t = {
        module => $module, name => $table, fmt => $fmt, width => $WIDTH{$fmt},
        first => $first, grp2 => $grp2, prio0 => $prio0,
        fields => [], subdirs => [],
    };
    $by_name{"$module\::$table"} = $t;
    push @tables, $t;

    my (%arm_prev, %arm_dead);
    for my $rf (scan_fields($body)) {
        my ($off_s, $fb, $is_arm) = @$rf;
        my $off = $off_s =~ /^0x/ ? hex($off_s) : int($off_s);

        # A whole field can be one splice: `0x0210 => { %selfTimerB2010 }`.
        for (1 .. 4) {
            my $before = $fb;
            $fb =~ s/(^|[{,]\s*)%(\w+)\s*(?=[,}]|$)/exists $shared->{$2} ? "$1$shared->{$2}," : "$1%$2"/gme;
            last if $fb eq $before;
        }

        my $guard;
        if ($is_arm) {
            next if $arm_dead{$off};
            my $c_src = field_cond($fb);
            my $own = 'true';
            if (defined $c_src) {
                my @c = compile_cond($c_src);
                unless (@c) {
                    note("$module\::$table $off_s: alternative, condition -- $c_src");
                    $arm_dead{$off} = 1;
                    next;
                }
                $own = $c[0];
            }
            my @g = map { "!($_)" } @{$arm_prev{$off} || []};
            push @{$arm_prev{$off}}, $own;
            push @g, $own unless $own eq 'true';
            $guard = @g ? join(' && ', @g) : undef;
        } else {
            my $c_src = field_cond($fb);
            if (defined $c_src) {
                my @c = compile_cond($c_src);
                unless (@c) {
                    note("$module\::$table $off_s: condition -- $c_src");
                    next;
                }
                $guard = $c[0] eq 'true' ? undef : $c[0];
            }
        }

        my ($name) = $fb =~ /Name\s*=>\s*'([^']+)'/;
        unless ($name) {
            note("$module\::$table $off_s: no Name");
            next;
        }
        # `Unknown => 1` keeps a tag out of the output unless -u is given, which
        # is not the default; the value is still read, because a later field
        # can be conditioned on it.
        my $hidden = $fb =~ /Unknown\s*=>\s*1/ ? 1 : 0;
        my ($ffmt) = $fb =~ /Format\s*=>\s*'([^']+)'/;
        $ffmt ||= $fmt;

        if ($fb =~ /SubDirectory\s*=>/) {
            my ($mod, $sub) = $fb =~ /TagTable\s*=>\s*'Image::ExifTool::(\w+)::(\w+)'/;
            unless ($sub) {
                note("$module\::$table $off_s: sub-directory with no table named");
                next;
            }
            my ($sfmt, $slen) = $ffmt =~ /^(\w+)\[(\d+)\]$/;
            # `undef[120]` and `string[16]` are that many bytes.
            my $sw = defined $sfmt
                ? ($WIDTH{$sfmt} // (($sfmt eq 'undef' or $sfmt eq 'string') ? 1 : undef))
                : undef;
            unless (defined $slen and defined $sw) {
                note("$module\::$table $off_s: sub-directory into $sub, of unknown length ($ffmt)");
                next;
            }
            push @{$t->{subdirs}}, {
                off => $off, mod => $mod, sub => $sub, cond => $guard,
                len => $slen * $sw,
            };
            push @{$pending{$mod}}, $sub;
            next;
        }

        my $count = 1;
        if ($ffmt =~ /^(string|undef)\[(\d+)\]$/) {
            push @{$t->{fields}}, {
                off => $off, name => $name, fmt => $1, n => $2, hidden => $hidden,
                cond => $guard, conv => {}, text => 1,
            };
            next;
        }
        if ($ffmt =~ /^(\w+)\[(\d+)\]$/) {
            ($ffmt, $count) = ($1, $2);
        }
        if ($ffmt =~ /\[/) {
            note("$module\::$table $off_s $name: count is not a number ($ffmt)");
            next;
        }
        unless (exists $WIDTH{$ffmt}) {
            note("$module\::$table $off_s $name: format '$ffmt'");
            next;
        }
        if ($count > 64) {
            note("$module\::$table $off_s $name: array of $count, likely a sub-structure");
            next;
        }

        my ($mask) = $fb =~ /Mask\s*=>\s*(0x[0-9a-fA-F]+|\d+)/;
        $mask = hex($mask) if defined $mask and $mask =~ /^0x/;
        my ($dmname) = $fb =~ /DataMember\s*=>\s*'(\w+)'/;
        unless (defined $dmname) {
            ($dmname) = $fb =~ /RawConv\s*=>\s*'\$\$self\{(\w+)\}\s*=\s*\$val'/;
        }
        my ($rconv) = $fb =~ /RawConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        my ($vconv) = $fb =~ /ValueConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        my ($pconv) = $fb =~ /PrintConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        unless (defined $vconv) {
            ($vconv) = $fb =~ /ValueConv\s*=>\s*\\&(\w+)/;
            $vconv = "Image::ExifTool::${module}::$vconv(\$val)" if defined $vconv;
        }
        unless (defined $pconv) {
            ($pconv) = $fb =~ /PrintConv\s*=>\s*\\&(\w+)/;
            $pconv = "Image::ExifTool::${module}::$pconv(\$val)" if defined $pconv;
        }
        for ($rconv, $vconv, $pconv) { s/\\'/'/g if defined }

        my %conv;
        if ($fb =~ /PrintConv\s*=>\s*\{(.*?)\n\s*\},/s) {
            my $c = $1;
            while ($c =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*'((?:[^'\\]|\\.)*)'/g) {
                my ($k, $v) = ($1, $2);
                $k = $k =~ /^0x/ ? hex($k) : int($k);
                $v =~ s/\\'/'/g;
                $conv{$k} = $v;
            }
        }

        push @{$t->{fields}}, {
            off => $off, name => $name, fmt => $ffmt, n => $count, hidden => $hidden,
            cond => $guard, conv => \%conv, mask => $mask, dmname => $dmname,
            rconv => $rconv, vconv => $vconv, pconv => $pconv,
        };
    }
}

# ---------------------------------------------------------------- collect
my %src_of;
my %shared_of;
for my $mod (sort keys %WANTED) {
    push @{$pending{$mod}}, @{$WANTED{$mod}};
}
while (grep { @{$pending{$_} || []} } keys %pending) {
    for my $mod (sort keys %pending) {
        while (my $tbl = shift @{$pending{$mod}}) {
            unless ($src_of{$mod}) {
                $src_of{$mod} = read_module($mod);
                $shared_of{$mod} = { shared_hashes($src_of{$mod}) };
            }
            parse_table($mod, $tbl, $src_of{$mod}, $shared_of{$mod});
        }
    }
}

# ---------------------------------------------------------------- emission
sub esc { my $s = shift; $s =~ s/\\/\\\\/g; $s =~ s/"/\\"/g; $s }
sub fn_name { my $t = shift; lc "$t->{module}_$t->{name}" }

my $nf = 0; $nf += scalar @{$_->{fields}} for @tables;

print <<"HDR";
//! Auto-generated decoders for ExifTool's binary sub-tables.
//!
//! Do not edit: regenerate with
//! `perl scripts/gen_binary_tables.pl ../exiftool/lib > src/tags/binary_tables_generated.rs`.
//!
//! ${\ scalar @tables} tables, $nf fields. A binary sub-table is a block of
//! bytes addressed by index: ExifTool's ProcessBinaryData reads the entry at
//! `(index - FIRST_ENTRY) * sizeof(FORMAT)`, and a field's own Format says
//! what to read there. What the generator could not express is on its stderr.
#![allow(clippy::too_many_lines, clippy::match_same_arms, clippy::unreadable_literal)]

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::tags::conv_expr::{self, Val as Conv};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Which end the file puts first.
pub type ByteOrder = crate::metadata::exif::ByteOrderMark;

/// What the fields of this block have read so far, by the name ExifTool
/// stores them under.
pub type State = Vec<(String, f64)>;

fn dm_get(dm: &State, name: &str) -> Option<f64> {
    dm.iter().rev().find(|(n, _)| n == name).map(|(_, v)| *v)
}

fn u8_at(d: &[u8], o: usize) -> Option<u8> { d.get(o).copied() }
fn i8_at(d: &[u8], o: usize) -> Option<i8> { d.get(o).map(|b| *b as i8) }

fn u16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u16> {
    let b = [*d.get(o)?, *d.get(o + 1)?];
    Some(if bo == ByteOrder::BigEndian { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
}
fn i16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i16> { u16_at(d, o, bo).map(|v| v as i16) }

fn u32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u32> {
    let b = [*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?];
    Some(if bo == ByteOrder::BigEndian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
}
fn i32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i32> { u32_at(d, o, bo).map(|v| v as i32) }

/// ExifTool's rational32 is two 16-bit halves -- four bytes, not the eight of
/// the rational64 EXIF writes. A zero denominator reads as infinity, and 0/0
/// as nothing at all.
fn rat32u_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(u16_at(d, o, bo)?), f64::from(u16_at(d, o + 2, bo)?))
}
fn rat32s_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(i16_at(d, o, bo)?), f64::from(i16_at(d, o + 2, bo)?))
}
fn ratio(n: f64, d: f64) -> Option<f64> {
    if d == 0.0 { return if n == 0.0 { None } else { Some(f64::INFINITY) }; }
    Some(n / d)
}

/// N bytes as text. A `string` stops at its first NUL, as ExifTool's reader
/// does; an `undef` is the bytes as they are.
fn text_at(d: &[u8], o: usize, n: usize, stop_at_nul: bool) -> Option<String> {
    let raw = d.get(o..o + n)?;
    let end = if stop_at_nul {
        raw.iter().position(|b| *b == 0).unwrap_or(raw.len())
    } else {
        raw.len()
    };
    Some(raw[..end].iter().map(|b| *b as char).collect())
}

/// Whether the block opens with these bytes, `None` accepting anything.
fn prefix_matches(d: &[u8], pat: &[Option<(u8, u8)>]) -> bool {
    if d.len() < pat.len() { return false; }
    pat.iter().zip(d).all(|(p, b)| p.is_none_or(|(lo, hi)| *b >= lo && *b <= hi))
}

fn mk(
    name: &str,
    id: u16,
    print_value: String,
    raw: Value,
    grp1: &'static str,
    grp2: &'static str,
    priority: i32,
) -> Tag {
    Tag {
        id: TagId::Numeric(id),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: grp1.into(),
            family2: grp2.into(),
            family3: crate::tag::MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value,
        priority,
    }
}

HDR

# model patterns
for my $i (0 .. $#re_list) {
    printf "static MODEL_RE_%d: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"%s\").expect(\"generated pattern\"));\n",
        $i, esc($re_list[$i]);
}
print "\n" if @re_list;

print "/// Decode one binary sub-table by the name ExifTool gives it.\n";
print "#[must_use]\n";
print "pub fn decode(table: &str, data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {\n";
print "    match table {\n";
printf("        \"%s\" => %s(data, model, bo, dm),\n", $_->{name}, fn_name($_)) for @tables;
print "        _ => Vec::new(),\n    }\n}\n\n";

for my $t (@tables) {
    printf "/// `Image::ExifTool::%s::%s` -- FORMAT %s, FIRST_ENTRY %d.\n",
        $t->{module}, $t->{name}, $t->{fmt}, $t->{first};
    printf "fn %s(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {\n", fn_name($t);
    printf "    const GRP1: &str = \"%s\";\n", $t->{module};
    printf "    const GRP2: &str = \"%s\";\n", $t->{grp2};
    printf "    const PRIO: i32 = %s;\n",
        $t->{prio0} ? 'crate::tag::PRIORITY_EXPLICIT_ZERO' : '0';
    print  "    let mut tags = Vec::new();\n";
    print  "    let _ = (model, bo, &dm);\n";

    for my $f (sort { $a->{off} <=> $b->{off} } @{$t->{fields}}) {
        my $byte = ($f->{off} - $t->{first}) * $t->{width};
        my $ind = "    ";
        if (defined $f->{cond}) {
            printf "%sif %s {\n", $ind, $f->{cond};
            $ind .= "    ";
        }
        if ($f->{text}) {
            printf "%sif let Some(text) = text_at(data, 0x%x, %d, %s) {\n",
                $ind, $byte, $f->{n}, ($f->{fmt} eq 'string' ? 'true' : 'false');
            printf "%s    tags.push(mk(\"%s\", 0x%x, text.clone(), Value::String(text), GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
            printf "%s}\n", $ind;
            print  "    }\n" if defined $f->{cond};
            next;
        }
        my $reader = {
            int8u => 'u8_at', int8s => 'i8_at',
            int16u => 'u16_at', int16s => 'i16_at',
            int32u => 'u32_at', int32s => 'i32_at',
            rational32u => 'rat32u_at', rational32s => 'rat32s_at',
        }->{$f->{fmt}};
        my $args = $f->{fmt} =~ /^int8/ ? '' : ', bo';
        my $w = $WIDTH{$f->{fmt}};

        if ($f->{n} > 1) {
            if ($IS_RATIONAL{$f->{fmt}}) {
                note(sprintf("%s::%s 0x%x", $t->{module}, $t->{name}, $f->{off}) . " $f->{name}: array of rationals");
                next;
            }
            printf "%s{\n", $ind;
            printf "%s    let mut parts = Vec::new();\n", $ind;
            printf "%s    for k in 0..%d {\n", $ind, $f->{n};
            printf "%s        match %s(data, 0x%x + k * %d%s) {\n", $ind, $reader, $byte, $w, $args;
            printf "%s            Some(x) => parts.push(x.to_string()),\n", $ind;
            printf "%s            None => { parts.clear(); break }\n", $ind;
            printf "%s        }\n%s    }\n", $ind, $ind;
            printf "%s    if !parts.is_empty() {\n", $ind;
            printf "%s        let s = parts.join(\" \");\n", $ind;
            # An array carries its conversions as much as a scalar does:
            # RawMeasuredRGGB is four int32u read back with their halves
            # exchanged, and joining them without that gives four other
            # numbers entirely.
            if (defined $f->{vconv} or defined $f->{pconv}) {
                printf "%s        let mut cv = Conv::Str(s.clone());\n", $ind;
                printf "%s        if let Some(x) = conv_expr::eval(\"%s\", &cv) { cv = x; }\n",
                    $ind, esc($f->{vconv}) if defined $f->{vconv};
                printf "%s        let raw = Value::String(cv.as_string());\n", $ind;
                printf "%s        if let Some(x) = conv_expr::eval(\"%s\", &cv) { cv = x; }\n",
                    $ind, esc($f->{pconv}) if defined $f->{pconv};
                printf "%s        tags.push(mk(\"%s\", 0x%x, cv.as_string(), raw, GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off} unless $f->{hidden};
            } else {
                printf "%s        tags.push(mk(\"%s\", 0x%x, s.clone(), Value::String(s), GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off} unless $f->{hidden};
            }
            printf "%s    }\n%s}\n", $ind, $ind;
            print  "    }\n" if defined $f->{cond};
            next;
        }

        printf "%sif let Some(v) = %s(data, 0x%x%s) {\n", $ind, $reader, $byte, $args;
        if (defined $f->{mask}) {
            my ($shift, $m) = (0, $f->{mask});
            until ($m & 1) { $m >>= 1; ++$shift }
            printf "%s    let v = %s;\n", $ind,
                $shift ? sprintf('(v & %#x) >> %d', $f->{mask}, $shift) : sprintf('v & %#x', $f->{mask});
        }
        printf "%s    dm.push((\"%s\".to_string(), f64::from(v)));\n",
            $ind, $f->{dmname} // $f->{name};
        my $guard = 0;
        if (defined $f->{rconv}) {
            printf "%s    let rc = conv_expr::eval(\"%s\", &Conv::Num(f64::from(v)));\n",
                $ind, esc($f->{rconv});
            printf "%s    if rc.as_ref() != Some(&Conv::Undef) {\n", $ind;
            printf "%s        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]\n", $ind;
            printf "%s        let v = rc.map_or(v, |x| x.as_num() as _);\n", $ind;
            $guard = 1;
            $ind .= "    ";
        }
        my %conv = %{$f->{conv}};
        if (%conv) {
            printf "%s    let s = match v as i64 {\n", $ind;
            for my $k (sort { $a <=> $b } keys %conv) {
                printf "%s        %d => \"%s\".to_string(),\n", $ind, $k, esc($conv{$k});
            }
            printf "%s        other => other.to_string(),\n", $ind;
            printf "%s    };\n", $ind;
            printf "%s    tags.push(mk(\"%s\", 0x%x, s, Value::I32(v as i32), GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
        } elsif (defined $f->{vconv} or defined $f->{pconv}) {
            printf "%s    let mut cv = Conv::Num(f64::from(v));\n", $ind;
            printf "%s    if let Some(x) = conv_expr::eval(\"%s\", &cv) { cv = x; }\n",
                $ind, esc($f->{vconv}) if defined $f->{vconv};
            printf "%s    let raw = Value::F64(cv.as_num());\n", $ind;
            printf "%s    if let Some(x) = conv_expr::eval(\"%s\", &cv) { cv = x; }\n",
                $ind, esc($f->{pconv}) if defined $f->{pconv};
            printf "%s    tags.push(mk(\"%s\", 0x%x, cv.as_string(), raw, GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
        } else {
            printf "%s    tags.push(mk(\"%s\", 0x%x, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
        }
        if ($guard) {
            $ind = substr($ind, 4);
            printf "%s    }\n", $ind;
        }
        printf "%s}\n", $ind;
        print  "    }\n" if defined $f->{cond};
    }

    for my $sd (@{$t->{subdirs}}) {
        my $byte = ($sd->{off} - $t->{first}) * $t->{width};
        my $ind = "    ";
        if (defined $sd->{cond}) {
            printf "%sif %s {\n", $ind, $sd->{cond};
            $ind .= "    ";
        }
        my $target = $by_name{"$sd->{mod}::$sd->{sub}"};
        unless ($target) {
            note(sprintf("%s::%s 0x%x:", $t->{module}, $t->{name}, $sd->{off}) . " sub-directory into $sd->{sub}, not generated");
            print "    }\n" if defined $sd->{cond};
            next;
        }
        printf "%sif let Some(sub) = data.get(0x%x..0x%x + %d) {\n", $ind, $byte, $byte, $sd->{len};
        printf "%s    tags.extend(%s(sub, model, bo, dm));\n", $ind, fn_name($target);
        printf "%s}\n", $ind;
        print  "    }\n" if defined $sd->{cond};
    }
    print "    tags\n}\n\n";
}

# ------------------------------------------------------- variant selectors
print <<'SEL';
/// Which sub-table a Main-table id opens, by the conditions ExifTool writes
/// on it.
///
/// `None` means no arm matched, which for an id whose arms are all
/// sub-directories means ExifTool extracts nothing at all.
#[must_use]
pub fn variant_for(module: &str, tag: u16, data: &[u8], count: usize) -> Option<&'static str> {
    let _ = (data, count);
    match (module, tag) {
SEL
for my $mod (sort keys %SELECTORS) {
    my $src = $src_of{$mod} // read_module($mod);
    my $main = table_body($src, $mod, 'Main');
    unless (defined $main) {
        note("$mod\::Main: no such table");
        next;
    }
    $main =~ s/^\s*#.*$//gm;
    for my $tag (@{$SELECTORS{$mod}}) {
        my $hex = sprintf '0x%04x', $tag;
        my ($arms) = $main =~ /^\s{4}\Q$hex\E\s*=>\s*\[(.*?)\n\s{4}\],/ms;
        unless (defined $arms) {
            note(sprintf("%s::Main %s: no list of alternatives", $mod, $hex));
            next;
        }
        printf "        (\"%s\", %s) => {\n", $mod, $hex;
        my (@arm_bodies, $cur);
        my $d = 0;
        for my $c (split //, $arms) {
            if ($c eq '{') { $d++; $cur = '' unless defined $cur }
            $cur .= $c if defined $cur;
            if ($c eq '}') { $d--; if ($d == 0 and defined $cur) { push @arm_bodies, $cur; undef $cur } }
        }
        my $unconditional = 0;
        for my $a (@arm_bodies) {
            my ($sub) = $a =~ /TagTable\s*=>\s*'Image::ExifTool::\w+::(\w+)'/;
            next unless $sub;
            unless ($by_name{"$mod\::$sub"}) {
                note(sprintf("%s::Main %s -> %s: no decoder generated for it", $mod, $hex, $sub));
                next;
            }
            my $c_src = field_cond($a);
            unless (defined $c_src) {
                printf "            Some(\"%s\")\n", $sub;
                $unconditional = 1;
                last;
            }
            my @c = compile_cond($c_src);
            unless (@c) {
                note(sprintf("%s::Main %s -> %s: condition -- %s", $mod, $hex, $sub, $c_src));
                next;
            }
            printf "            if %s {\n                return Some(\"%s\");\n            }\n", $c[0], $sub;
        }
        print  "            None\n" unless $unconditional;
        print  "        }\n";
    }
}
print "        _ => None,\n    }\n}\n\n";

print "#[cfg(test)]\nmod tests {\n    use super::*;\n\n";
print "    /// Every generated pattern must compile: a bad one would otherwise\n";
print "    /// only surface as a panic on whichever file first reaches it.\n";
print "    #[test]\n    fn every_model_pattern_compiles() {\n";
printf "        LazyLock::force(&MODEL_RE_%d);\n", $_ for (0 .. $#re_list);
print "    }\n}\n";

printf STDERR "Generated %d tables, %d fields, %d model patterns.\n",
    scalar @tables, $nf, scalar @re_list;
printf STDERR "SKIPPED %d -- these are NOT silently dropped:\n", scalar @skipped;
printf STDERR "  %s\n", $_ for @skipped;
