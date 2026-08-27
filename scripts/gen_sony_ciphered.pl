#!/usr/bin/perl
# Generate decoders for Sony's ciphered MakerNote sub-tables from ExifTool's
# Sony.pm.
#
# These tables (Tag2010*, Tag9050*, Tag94xx) are byte-substitution enciphered
# and addressed by BYTE OFFSET, not by element index -- their FORMAT is int8u
# and individual fields override it. Model-specific variants and per-field
# Conditions are carried over as regular expressions rather than translated by
# hand, so the port stays faithful to the source.
#
# Anything this script cannot express is reported on stderr and counted. It
# never drops a field silently: a silent skip is what hid these tables for so
# long.
#
# Usage: perl scripts/gen_sony_ciphered.pl /path/to/exiftool/lib > src/tags/sony_ciphered_generated.rs

use strict;
use warnings;

my $lib = $ARGV[0] || '../exiftool/lib';
my $pm  = "$lib/Image/ExifTool/Sony.pm";
die "Cannot read $pm\n" unless -f $pm;

open my $fh, '<', $pm or die $!;
my $src = do { local $/; <$fh> };
close $fh;

my ($et_version) = $src =~ /\$VERSION\s*=\s*'([^']+)'/;

# Some fields do not spell out their PrintConv: they splice in a shared hash,
# `%releaseMode2,` on a line of its own. Collect those so a reference resolves
# instead of yielding a bare number where ExifTool prints a phrase.
my %shared;
while ($src =~ /^(?:my\s+)?%(\w+)\s*=\s*\((.*?)\n\);/gms) {
    $shared{$1} = $2;
}

$et_version ||= '?';

# ---------------------------------------------------------------- collection
my (@tables, @skipped);
my %re_index; my @re_list;

sub re_id {
    my ($pat) = @_;
    return $re_index{$pat} if exists $re_index{$pat};
    my $id = scalar @re_list;
    push @re_list, $pat;
    $re_index{$pat} = $id;
    return $id;
}

# Field formats we can read. Anything else is reported, not guessed.
my %WIDTH = (
    int8u  => 1, int8s  => 1,
    int16u => 2, int16s => 2,
    int32u => 4, int32s => 4,
);

# Every Sony binary sub-table, whether or not it is enciphered: the tables the
# Main entries point at are the ones a reader has to decode itself, and half of
# them (ShotInfo, Panorama, Tag202a) are in plain sight.
while ($src =~ /^%Image::ExifTool::Sony::(\w+)\s*=\s*\((.*?)\n\);/gms) {
    my ($table, $body) = ($1, $2);
    next if $table eq 'Main';
    # `%binaryDataAttrs` splices FIRST_ENTRY in, so half the binary tables
    # never say the word.
    next unless $body =~ /FIRST_ENTRY/ or $body =~ /%binaryDataAttrs/;
    # Enciphered tables are byte-substituted before they can be read; the rest
    # are not, and deciphering one of those would turn it to noise.
    my $enciphered = $body =~ /ProcessEnciphered/ ? 1 : 0;
    # `PRIORITY => 0` says a tag from this table must not displace one of the
    # same name read earlier: the Main table's HighISONoiseReduction is the one
    # ExifTool prints, not the copy inside Tag2010i.
    my $low_priority = $body =~ /PRIORITY\s*=>\s*0/ ? 1 : 0;

    # Each of these tables declares its own family-2 default, and it is Image
    # rather than the Camera one might assume: these are properties of the shot.
    my ($grp2) = $body =~ /GROUPS\s*=>\s*\{[^}]*2\s*=>\s*'(\w+)'/;
    $grp2 ||= 'Camera';

    my ($fmt) = $body =~ /FORMAT\s*=>\s*'(\w+)'/;
    $fmt ||= 'int8u';
    unless (exists $WIDTH{$fmt}) {
        push @skipped, "$table: unsupported table FORMAT '$fmt'";
        next;
    }
    my $unit = $WIDTH{$fmt};

    # A field is written two ways: a block over several lines, or all on one.
    # Counting brackets reads both, and -- unlike a regex for the block form --
    # a single-line field can no longer swallow the two that follow it, which
    # is how TiffMeteringImageWidth ended up with the Format of a field two
    # entries further down.
    my @raw_fields;
    my @lines = split /\n/, $body;
    for (my $i = 0; $i <= $#lines; ++$i) {
        next unless $lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+)\s*=>\s*\{/;
        my $off_s = $1;
        my ($depth, $text) = (0, '');
        for (my $j = $i; $j <= $#lines; ++$j) {
            $text .= $lines[$j] . "\n";
            $depth++ while $lines[$j] =~ /\{/g;
            $depth-- while $lines[$j] =~ /\}/g;
            if ($depth <= 0) {
                $i = $j;
                last;
            }
        }
        push @raw_fields, [$off_s, $text];
    }

    my @fields;
    for my $rf (@raw_fields) {
        my ($off_s, $fb) = @$rf;
        my $off   = $off_s =~ /^0x/ ? hex($off_s) : int($off_s);

        # `Hidden => 1` keeps a field out of the output -- but it is still
        # read, because the value behind it is what a later field is
        # conditioned on. Dropping it outright left CameraTemperature with
        # nothing to test.
        my $hidden = $fb =~ /Hidden\s*=>\s*1/ ? 1 : 0;
        # A field that opens a sub-table of its own is not a scalar to read.
        if ($fb =~ /SubDirectory\s*=>/) {
            my ($sub) = $fb =~ /TagTable\s*=>\s*'Image::ExifTool::Sony::(\w+)'/;
            push @skipped, sprintf("%s 0x%04x: sub-directory into %s", $table, $off, $sub // '?');
            next;
        }

        my ($name) = $fb =~ /Name\s*=>\s*'([^']+)'/;
        unless ($name) {
            push @skipped, sprintf("%s 0x%04x: no Name", $table, $off);
            next;
        }

        my ($ffmt) = $fb =~ /Format\s*=>\s*'([^']+)'/;
        $ffmt ||= $fmt;
        # A fixed-size array of a scalar type is just N reads joined by spaces,
        # which is how ExifTool prints one. Large ones are sub-structures with a
        # decoder of their own, so they stay reported rather than flattened.
        my $count_n = 1;
        # `string[20]` and `undef[6]` are N bytes read as text, which is how
        # ExifTool prints them.
        if ($ffmt =~ /^(\w+)\[(\d+)\]$/ and exists $WIDTH{$1}) {
            ($ffmt, $count_n) = ($1, $2);
            if ($count_n > 64) {
                push @skipped, "$table 0x" . sprintf('%04x', $off) . " $name: array of $count_n, likely a sub-structure";
                next;
            }
        }
        # Per-field condition. Only a bare Model regex is portable; anything
        # referring to other parse state is reported and the field dropped.
        my $re = undef;
        my $neg = 0;
        my ($cond) = $fb =~ /Condition\s*=>\s*q\{(.*?)\}\s*,/ms;
        ($cond) = $fb =~ /Condition\s*=>\s*'([^']*)'/ unless defined $cond;
        if (defined $cond) {
            $cond =~ s/\s+/ /g;
            $cond =~ s/^ | $//g;
            # [^\/]* rather than .+? : a compound condition ("... and
            # $$self{Software} =~ /.../") would otherwise capture up to its last
            # slash and yield a pattern that is not a regex at all.
            if ($cond =~ /^\s*\$\$self\{Model\}\s*(=~|!~)\s*\/([^\/]*)\/\s*$/) {
                $neg = ($1 eq '!~');
                my $pat = $2;
                if ($pat =~ /\(\?[=!<]/) {
                    push @skipped, "$table $name: lookaround in condition";
                    next;
                }
                $pat =~ s/\\b/\\b/g;   # regex-lite understands \b
                $re = re_id($pat);
            } else {
                # Anything else goes through the same compiler the variant
                # conditions use, in its field mode.
                my @c = compile_cond($cond, 'field');
                unless (@c) {
                    push @skipped, "$table 0x" . sprintf('%04x', $off) . " $name: condition needs parse state -- $cond";
                    next;
                }
                $re = { expr => $c[0] };
            }
        }

        # `string[20]` and `undef[6]` are N bytes read as text.
        if ($ffmt =~ /^(string|undef)\[(\d+)\]$/) {
            push @fields, { off => $off, name => $name, fmt => $1, n => $2,
                            re => $re, neg => $neg, conv => {},
                            vconv => undef, pconv => undef, hidden => $hidden };
            next;
        }
        unless (exists $WIDTH{$ffmt}) {
            push @skipped, "$table 0x" . sprintf('%04x', $off) . " $name: format '$ffmt'";
            next;
        }

        my %conv;
        my $conv_src;
        if ($fb =~ /PrintConv\s*=>\s*\{(.*?)\n\s{8}\}/s) {
            $conv_src = $1;
        } elsif ($fb =~ /PrintConv\s*=>\s*\{([^{}\n]*)\}/) {
            # Short tables are written on one line: { 0 => 'No', 1 => 'Yes'}.
            # Requiring a multi-line block left those printing raw numbers.
            $conv_src = $1;
        } elsif ($fb =~ /PrintConv\s*=>\s*\\%(\w+)/ and exists $shared{$1}) {
            # A reference to a shared table, such as the 313-entry lens list.
            # Without following it a lens prints as 49475 instead of its name.
            $conv_src = $shared{$1};
        } elsif ($fb =~ /^\s+%(\w+),\s*$/m and exists $shared{$1}) {
            my $body = $shared{$1};
            ($conv_src) = $body =~ /PrintConv\s*=>\s*\{(.*?)\n\s{4}\}/s;
        }
        if (defined $conv_src) {
            my $cs = $conv_src;
            while ($cs =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*'((?:[^'\\]|\\.)*)'/g) {
                my $k = $1; my $v = $2;
                $k = $k =~ /^0x/ ? hex($k) : int($k);
                $v =~ s/\\'/'/g;
                $conv{$k} = $v;
            }
        }

        # Conversions written as Perl expressions rather than a hash. They are
        # carried over verbatim and evaluated by tags::conv_expr, which declines
        # anything outside its grammar so the raw value survives untouched.
        my ($rconv) = $fb =~ /RawConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        $rconv =~ s/\\'/'/g if defined $rconv;
        my ($vconv) = $fb =~ /ValueConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        my ($pconv) = $fb =~ /PrintConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        for ($vconv, $pconv) { $_ =~ s/\\'/'/g if defined }

        push @fields, {
            off => $off, name => $name, fmt => $ffmt, n => $count_n,
            re => $re, neg => $neg, conv => \%conv,
            vconv => $vconv, pconv => $pconv, hidden => $hidden, rconv => $rconv,
        };
    }

    next unless @fields;
    push @tables, { name => $table, unit => $unit, fields => \@fields, grp2 => $grp2,
                    enciphered => $enciphered, low_priority => $low_priority };
}

# ------------------------------------------------- variant selection (Main)
# ExifTool picks the variant of each ciphered tag with a Condition on the Main
# table entry. Those conditions are compiled here into Rust predicates over the
# model name and the raw (still enciphered) bytes -- which is what ExifTool
# tests them against, since the Condition runs before the block is deciphered.
#
# A condition this cannot express is REPORTED, never treated as "no condition".
# Reading a `q{...}` condition as absent is what made every multi-line arm
# unconditional, so an ILCE-9 was decoded with the DSC-HX10V table.
my @selectors;
my ($main) = $src =~ /^%Image::ExifTool::Sony::Main\s*=\s*\((.*?)\n\);/ms;
$main ||= '';

# A byte pattern anchored at the start: `[\x07\x09\x0a]`, `\xb6..\x01`.
# Returns a Rust slice-of-slices, one entry per byte, empty for a wildcard.
sub byte_prefix {
    my ($re) = @_;
    return undef unless $re =~ s/^\^//;
    my @pos;
    while (length $re) {
        if ($re =~ s/^\[((?:\\x[0-9a-fA-F]{2})+)\]//) {
            my @set = map { hex } $1 =~ /\\x([0-9a-fA-F]{2})/g;
            push @pos, \@set;
        } elsif ($re =~ s/^\\0//) {
            push @pos, [0];
        } elsif ($re =~ s/^\\x([0-9a-fA-F]{2})//) {
            push @pos, [hex $1];
        } elsif ($re =~ s/^\.//) {
            push @pos, [];
        } elsif ($re =~ s/^([A-Za-z0-9 ])//) {
            push @pos, [ord $1];
        } else {
            return undef;    # something this cannot express
        }
    }
    return undef unless @pos;
    return '&[' . join(', ', map { '&[' . join(', ', map { sprintf '0x%02x', $_ } @$_) . ']' } @pos) . ']';
}

# Compile one condition. Returns (rust_expr, flag_expr) or (), where flag_expr
# says when `$$self{DoubleCipher} = 1` inside it would have run -- Perl's `and`
# and `or` short-circuit, so the assignment only happens on some paths.
sub compile_cond {
    my ($cond, $mode) = @_;
    $mode ||= 'variant';
    $cond =~ s/\s+/ /g;
    $cond =~ s/^ | $//g;
    return ('true', 'false') unless length $cond;

    # `A ? B : C`, at the top level.
    {
        my ($depth, $i) = (0, 0);
        while ($i < length $cond) {
            my $c = substr($cond, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            if (!$depth and $c eq '?') {
                my $rest = substr($cond, $i + 1);
                my ($d2, $j) = (0, 0);
                while ($j < length $rest) {
                    my $k = substr($rest, $j, 1);
                    $d2++ if $k eq '(';
                    $d2-- if $k eq ')';
                    last if !$d2 and $k eq ':';
                    ++$j;
                }
                if ($j < length $rest) {
                    my @c = compile_cond(substr($cond, 0, $i), $mode);
                    my @t = compile_cond(substr($rest, 0, $j), $mode);
                    my @f = compile_cond(substr($rest, $j + 1), $mode);
                    return () unless @c and @t and @f;
                    return ("(if $c[0] { $t[0] } else { $f[0] })",
                            "(if $c[0] { $t[1] } else { $f[1] })");
                }
            }
            ++$i;
        }
    }

    # `A or B` / `A and B`, splitting at the top level only.
    for my $op (' or ', ' and ') {
        my ($depth, $i) = (0, 0);
        while ($i < length $cond) {
            my $c = substr($cond, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            if (!$depth && substr($cond, $i, length $op) eq $op) {
                my @l = compile_cond(substr($cond, 0, $i), $mode);
                my @r = compile_cond(substr($cond, $i + length $op), $mode);
                return () unless @l and @r;
                # `or` only reaches its right side when the left was false;
                # `and` only when it was true.
                my ($rust, $flag) = $op eq ' or '
                    ? ("($l[0] || $r[0])", "($l[1] || (!$l[0] && $r[1]))")
                    : ("($l[0] && $r[0])", "($l[1] || ($l[0] && $r[1]))");
                $flag = 'false' if $l[1] eq 'false' and $r[1] eq 'false';
                return ($rust, $flag);
            }
            ++$i;
        }
    }
    if ($cond =~ /^\((.*)\)$/) {
        my $inner = $1;
        # Only strip brackets that wrap the whole thing.
        my ($depth, $ok) = (0, 1);
        for my $i (0 .. length($inner) - 1) {
            my $c = substr($inner, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            $ok = 0 if $depth < 0;
        }
        return compile_cond($inner, $mode) if $ok and $depth == 0;
    }
    if ($cond =~ /^not (.*)$/) {
        my @i = compile_cond($1, $mode);
        return () unless @i;
        return ("!($i[0])", $i[1]);
    }
    # The model name.
    if ($cond =~ m{^\$\$self\{Model\} (=~|!~) /(.*)/$}) {
        my ($op, $pat) = ($1, $2);
        return () if $pat =~ /\(\?[=!<]/;    # lookaround
        return (sprintf('%sMODEL_RE_%d.is_match(model)', $op eq '!~' ? '!' : '', re_id($pat)), 'false');
    }
    # The raw bytes of the block.
    if ($cond =~ m{^\$\$valPt =~ /(.*?)/[a-z]*$}) {
        my $re = $1;
        # A leading optional group is two patterns: with it and without.
        if ($re =~ /^\^\(([^)]*)\)\?(.*)$/) {
            my ($opt, $rest) = ($1, $2);
            my $with = byte_prefix("^$opt$rest");
            my $without = byte_prefix("^$rest");
            return () unless defined $with and defined $without;
            return ("(prefix_matches(data, $with) || prefix_matches(data, $without))", 'false');
        }
        my $pat = byte_prefix($re);
        return () unless defined $pat;
        return ("prefix_matches(data, $pat)", 'false');
    }
    # `$count` is how many elements the tag holds, which is how ExifTool tells
    # one 0x0010 layout from another.
    if ($cond =~ /^\$count (==|!=|<=|>=|<|>) (\d+)$/) {
        return ("count $1 $2", 'false');
    }
    # `$format eq "undef"`.
    if ($cond =~ /^\$format (eq|ne) "(\w+)"$/) {
        return (sprintf('format %s "%s"', $1 eq 'eq' ? '==' : '!=', $2), 'false');
    }
    # `$$self{Panorama} = (...)` is an assignment whose value is the test.
    if ($cond =~ /^\$\$self\{Panorama\} = \((.*)\)$/) {
        return compile_cond($1, $mode);
    }
    # `$$self{DoubleCipher} = 1` is an assignment: it is always true, and it
    # records that this block is doubly enciphered.
    return ('true', 'true') if $cond =~ /^\$\$self\{DoubleCipher\} = 1$/;
    return ('double_cipher', 'false') if $cond eq '$$self{DoubleCipher}';
    # Sony sets this while reading a panoramic shot; the reader passes it in.
    return ('panorama', 'false') if $mode eq 'variant' and $cond eq '$$self{Panorama}';

    # Inside a table, `$$self{X}` is a DATAMEMBER an earlier field of the same
    # table stored -- CameraTemperature is only valid for some values of the
    # TempTest2 that precedes it.
    if ($mode eq 'field') {
        if ($cond =~ /^\$\$self\{(\w+)\}$/) {
            return (sprintf('dm_get(&dm, "%s").is_some_and(|v| v != 0.0)', $1), 'false');
        }
        if ($cond =~ /^\$\$self\{(\w+)\} (==|!=|<=|>=|<|>) (-?[\d.]+)$/) {
            my ($dm, $op, $num) = ($1, $2, $3);
            $num .= '.0' unless $num =~ /\./;
            $op = '==' if $op eq '==';
            return (sprintf('dm_get(&dm, "%s").is_some_and(|v| v %s %s)', $dm, $op, $num), 'false');
        }
    }
    return ();
}

while ($main =~ /^\s{4}(0x[0-9a-fA-F]+)\s*=>\s*\[(.*?)\}\],\s*$/gms) {
    my $tag = hex($1);
    my $arms = $2;
    my @choices;
    while ($arms =~ /\{(.*?)SubDirectory\s*=>\s*\{[^}]*TagTable\s*=>\s*'Image::ExifTool::Sony::(\w+)'/gs) {
        my ($arm, $tbl) = ($1, $2);
        my ($nm) = $arm =~ /Name\s*=>\s*'([^']+)'/;
        next unless $nm;
        unless (grep { $_->{name} eq $tbl } @tables) {
            push @skipped, sprintf("variant 0x%04x -> %s: no table generated for it", $tag, $tbl);
            next;
        }
        # A condition is written three ways: '...', "..." or q{...}.
        my ($cond) = $arm =~ /Condition\s*=>\s*q\{(.*?)\}\s*,/ms;
        ($cond) = $arm =~ /Condition\s*=>\s*'([^']*)'/ unless defined $cond;
        ($cond) = $arm =~ /Condition\s*=>\s*"([^"]*)"/ unless defined $cond;
        my $unconditional = !defined $cond;
        my @c = compile_cond($cond // '');
        unless (@c) {
            push @skipped, sprintf("variant 0x%04x -> %s: cannot express -- %s",
                                   $tag, $tbl, join ' ', split ' ', $cond // '');
            next;
        }
        push @choices, { tbl => $tbl, expr => $c[0], dbl => $c[1], uncond => $unconditional };
    }
    push @selectors, { tag => $tag, choices => \@choices } if @choices;
}

# Tags with a single, unconditional sub-directory are declared as a plain hash
# rather than an array of alternatives.
while ($main =~ /^\s{4}(0x[0-9a-fA-F]+)\s*=>\s*\{(.*?)^\s{4}\},/gms) {
    my $tag = hex($1);
    my $b   = $2;
    next if grep { $_->{tag} == $tag } @selectors;
    my ($tbl) = $b =~ /TagTable\s*=>\s*'Image::ExifTool::Sony::(\w+)'/;
    next unless $tbl;
    unless (grep { $_->{name} eq $tbl } @tables) {
        push @skipped, sprintf("variant 0x%04x -> %s: no table generated for it", $tag, $tbl);
        next;
    }
    # A lone sub-directory can still carry a Condition -- Panorama's decides
    # whether the block holds a panorama at all.
    my ($cond) = $b =~ /Condition\s*=>\s*q\{(.*?)\}\s*,/ms;
    ($cond) = $b =~ /Condition\s*=>\s*'([^']*)'/ unless defined $cond;
    my @c = defined $cond ? compile_cond($cond) : ('true', 'false');
    unless (@c) {
        push @skipped, sprintf("variant 0x%04x -> %s: cannot express -- %s",
                               $tag, $tbl, join ' ', split ' ', $cond);
        next;
    }
    push @selectors, { tag => $tag,
                       choices => [ { tbl => $tbl, expr => $c[0], dbl => $c[1], uncond => 1 } ] };
}
@selectors = sort { $a->{tag} <=> $b->{tag} } @selectors;

# ------------------------------------------------------------------ emission
my $nf = 0; $nf += scalar @{$_->{fields}} for @tables;

print <<"HDR";
//! Auto-generated decoders for Sony's ciphered MakerNote sub-tables.
//!
//! Source: ExifTool $et_version, lib/Image/ExifTool/Sony.pm.
//! Generated by scripts/gen_sony_ciphered.pl -- DO NOT EDIT MANUALLY.
//!
//! These tables are byte-substitution enciphered (see metadata::sony_decrypt)
//! and addressed by byte offset. Model conditions are carried over from the
//! Perl source verbatim rather than translated, so they cannot drift from it.
//!
//! Tables: ${\ scalar @tables }, fields: $nf.

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::tags::conv_expr::{self, Val as Conv};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

HDR

for my $i (0 .. $#re_list) {
    my $p = $re_list[$i];
    $p =~ s/\\/\\\\/g;
    $p =~ s/"/\\"/g;
    printf "static MODEL_RE_%d: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"%s\").unwrap());\n", $i, $p;
}
print "\n";

print <<'RD';
fn u8_at(d: &[u8], o: usize) -> Option<u8> { d.get(o).copied() }
fn i8_at(d: &[u8], o: usize) -> Option<i8> { d.get(o).map(|v| *v as i8) }
fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}
fn i16_at(d: &[u8], o: usize) -> Option<i16> {
    Some(i16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}
fn i32_at(d: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}

/// N bytes read as text. A `string` stops at its first NUL; an `undef` keeps
/// what is there. Both come back as the bytes they are, so a value that is not
/// ASCII survives to whatever decodes it later.
fn text_at(d: &[u8], o: usize, n: usize, stop_at_nul: bool) -> Option<String> {
    let raw = d.get(o..o + n)?;
    let end = if stop_at_nul {
        raw.iter().position(|b| *b == 0).unwrap_or(raw.len())
    } else {
        raw.len()
    };
    Some(raw[..end].iter().map(|b| *b as char).collect())
}

fn text_tag(name: &str, default_group2: &'static str, text: String, priority: i32) -> Tag {
    mk_prio(name, text.clone(), Value::String(text), default_group2, priority)
}

/// The most recent value read under this name in the table being decoded.
///
/// ExifTool calls these DATAMEMBERs: a field stores its value so a later field
/// can be conditioned on it, which is how one offset holds LensType2 on an
/// E-mount body and LensType on an A-mount one.
fn dm_get(dm: &[(&str, f64)], name: &str) -> Option<f64> {
    dm.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

fn mk(name: &str, print_value: String, raw: Value, default_group2: &'static str) -> Tag {
    mk_prio(name, print_value, raw, default_group2, 0)
}

fn mk_prio(
    name: &str,
    print_value: String,
    raw: Value,
    default_group2: &'static str,
    priority: i32,
) -> Tag {
    // Each table declares its own family-2 default -- Image for these -- and a
    // few tags override it. Stamping them all the same put two of them in the
    // wrong category.
    let family2 = crate::tags::group2::family2_for("MakerNotes", "Sony", name, default_group2)
        .unwrap_or(default_group2);
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: "Sony".into(),
            family2: family2.into(),
            family3: crate::tag::MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value,
        priority,
    }
}

RD

# dispatcher
print "/// Decode one deciphered Sony sub-table. `data` must already be deciphered.\n";
print "#[must_use]\n";
print "/// Whether a table's block is byte-substitution enciphered in the file.\n";
print "///\n/// Deciphering one that is not turns it to noise, which is why this is\n";
print "/// read off ExifTool's PROCESS_PROC rather than assumed.\n";
print "#[must_use]\n";
print "pub fn is_enciphered(table: &str) -> bool {\n";
print "    matches!(table,\n";
{
    my @enc = map { "\"$_->{name}\"" } grep { $_->{enciphered} } @tables;
    print "        " . join(" | ", @enc) . "\n";
}
print "    )\n}\n\n";

print "pub fn decode(table: &str, data: &[u8], model: &str) -> Vec<Tag> {\n";
print "    match table {\n";
printf("        \"%s\" => %s(data, model),\n", $_->{name}, lc $_->{name}) for @tables;
print "        _ => Vec::new(),\n    }\n}\n\n";

print <<'SELECTOR';
/// Which ciphered sub-table a Sony MakerNote tag uses, and whether choosing it
/// records that the block is doubly enciphered.
pub struct Variant {
    pub table: &'static str,
    pub sets_double_cipher: bool,
}

/// Whether the raw bytes start the way a pattern says. An empty set at a
/// position is `.` -- any byte will do.
fn prefix_matches(data: &[u8], pat: &[&[u8]]) -> bool {
    data.len() >= pat.len()
        && pat.iter().zip(data).all(|(set, b)| set.is_empty() || set.contains(b))
}

/// Mirrors the Condition chain on the Sony Main table entry.
///
/// `data` is the block as it sits in the file -- still enciphered, because
/// that is what ExifTool tests these conditions against.
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn variant_for(
    tag: u16,
    model: &str,
    data: &[u8],
    count: usize,
    format: &str,
    double_cipher: bool,
    panorama: bool,
) -> Option<Variant> {
    let _ = (data, count, format, double_cipher, panorama);
    match tag {
SELECTOR
for my $s (@selectors) {
    printf "        0x%04x => {\n", $s->{tag};
    my $unconditional = 0;
    for my $c (@{$s->{choices}}) {
        if ($c->{expr} eq 'true') {
            $unconditional = 1;
            printf "            Some(Variant { table: \"%s\", sets_double_cipher: %s })\n",
                $c->{tbl}, $c->{dbl};
            last;   # nothing after an unconditional arm can be reached
        }
        printf "            if %s {\n                return Some(Variant { table: \"%s\", sets_double_cipher: %s });\n            }\n",
            $c->{expr}, $c->{tbl}, $c->{dbl};
    }
    print  "            None\n" unless $unconditional;
    print  "        }\n";
}
print "        _ => None,\n    }\n}\n\n";

for my $t (@tables) {
    printf "fn %s(data: &[u8], model: &str) -> Vec<Tag> {\n", lc $t->{name};
    printf "    const GRP2: &str = \"%s\";\n", $t->{grp2};
    printf "    const PRIO: i32 = %s;\n",
        ($t->{low_priority} ? 'crate::tag::PRIORITY_EXPLICIT_ZERO' : '0');
    print  "    let _ = model;\n" unless grep { defined $_->{re} } @{$t->{fields}};
    print  "    let mut tags = Vec::new();\n";
    print  "    let mut dm: Vec<(&str, f64)> = Vec::new();\n";
    print  "    let _ = &dm;\n";
    for my $f (sort { $a->{off} <=> $b->{off} } @{$t->{fields}}) {
        my $reader = { int8u => 'u8_at', int8s => 'i8_at', int16u => 'u16_at',
                       int16s => 'i16_at', int32u => 'u32_at', int32s => 'i32_at' }->{$f->{fmt}};
        my $ind = "    ";
        if (defined $f->{re}) {
            if (ref $f->{re} eq 'HASH') {
                printf "%sif %s {\n", $ind, $f->{re}{expr};
            } else {
                printf "%sif %sMODEL_RE_%d.is_match(model) {\n", $ind, ($f->{neg} ? '!' : ''), $f->{re};
            }
            $ind = "        ";
        }
        # `string[N]` and `undef[N]` are N bytes read as text. A string stops
        # at its first NUL, as ExifTool's reader does.
        if ($f->{fmt} eq 'string' or $f->{fmt} eq 'undef') {
            printf "%sif let Some(text) = text_at(data, 0x%x, %d, %s) {\n",
                $ind, $f->{off}, $f->{n}, ($f->{fmt} eq 'string' ? 'true' : 'false');
            printf "%s    tags.push(text_tag(\"%s\", GRP2, text, PRIO));\n", $ind, $f->{name}
                unless $f->{hidden};
            printf "%s}\n", $ind;
            print  "    }\n" if defined $f->{re};
            next;
        }
        if (($f->{n} // 1) > 1) {
            my $w = { int8u => 1, int8s => 1, int16u => 2, int16s => 2, int32u => 4, int32s => 4 }->{$f->{fmt}};
            printf "%s{\n", $ind;
            printf "%s    let mut parts = Vec::new();\n", $ind;
            printf "%s    for k in 0..%d {\n", $ind, $f->{n};
            printf "%s        match %s(data, 0x%x + k * %d) {\n", $ind, $reader, $f->{off}, $w;
            printf "%s            Some(x) => parts.push(x.to_string()),\n", $ind;
            printf "%s            None => { parts.clear(); break }\n", $ind;
            printf "%s        }\n%s    }\n", $ind, $ind;
            printf "%s    if !parts.is_empty() {\n", $ind;
            my $joined = "parts.join(\" \")";
            if (defined $f->{pconv} and $f->{pconv} =~ /unpack\s+"H\*"/) {
                # `unpack "H*", pack "C*", split " ", $val`: the bytes as hex.
                printf "%s        let hex: String = parts.iter().map(|p| format!(\"{:02x}\", p.parse::<u32>().unwrap_or(0))).collect();\n", $ind;
                printf "%s        tags.push(mk_prio(\"%s\", hex, Value::String(%s), GRP2, PRIO));\n", $ind, $f->{name}, $joined
                    unless $f->{hidden};
            } else {
                printf "%s        let s = %s;\n", $ind, $joined;
                printf "%s        tags.push(mk_prio(\"%s\", s.clone(), Value::String(s), GRP2, PRIO));\n", $ind, $f->{name}
                    unless $f->{hidden};
            }
            printf "%s    }\n%s}\n", $ind, $ind;
            print  "    }\n" if defined $f->{re};
            next;
        }
        printf "%sif let Some(v) = %s(data, 0x%x) {\n", $ind, $reader, $f->{off};
        printf "%s    dm.push((\"%s\", f64::from(v)));\n", $ind, $f->{name};
        # A RawConv can rule the value out entirely -- `$val ? $val : undef`
        # means "only when it is not zero" -- or reshape it. One this evaluator
        # declines leaves the raw value, which is the honest answer.
        my $raw_guard = 0;
        if (defined $f->{rconv}) {
            my $t = $f->{rconv}; $t =~ s/\\/\\\\/g; $t =~ s/"/\\"/g;
            printf "%s    let rc = conv_expr::eval(\"%s\", &Conv::Num(f64::from(v)));\n", $ind, $t;
            printf "%s    if rc.as_ref() != Some(&Conv::Undef) {\n", $ind;
            printf "%s        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]\n", $ind;
            printf "%s        let v = rc.map_or(v, |x| x.as_num() as _);\n", $ind;
            $raw_guard = 1;
            $ind .= "    ";
        }
        my %conv = %{$f->{conv}};
        if (%conv) {
            printf "%s    let s = match v as i64 {\n", $ind;
            for my $k (sort { $a <=> $b } keys %conv) {
                my $v = $conv{$k};
                $v =~ s/\\/\\\\/g; $v =~ s/"/\\"/g;
                printf "%s        %d => \"%s\".to_string(),\n", $ind, $k, $v;
            }
            printf "%s        other => other.to_string(),\n", $ind;
            printf "%s    };\n", $ind;
            printf "%s    tags.push(mk_prio(\"%s\", s, Value::I32(v as i32), GRP2, PRIO));\n", $ind, $f->{name}
                unless $f->{hidden};
        } elsif (defined $f->{vconv} or defined $f->{pconv}) {
            my $esc = sub { my $t = shift; $t =~ s/\\/\\\\/g; $t =~ s/"/\\"/g; $t };
            printf "%s    let mut cv = Conv::Num(f64::from(v));\n", $ind;
            if (defined $f->{vconv}) {
                printf "%s    if let Some(x) = conv_expr::eval(\"%s\", &cv) { cv = x; }\n",
                    $ind, $esc->($f->{vconv});
            }
            printf "%s    let raw = Value::F64(cv.as_num());\n", $ind;
            if (defined $f->{pconv}) {
                printf "%s    if let Some(x) = conv_expr::eval(\"%s\", &cv) { cv = x; }\n",
                    $ind, $esc->($f->{pconv});
            }
            printf "%s    tags.push(mk_prio(\"%s\", cv.as_string(), raw, GRP2, PRIO));\n", $ind, $f->{name}
                unless $f->{hidden};
        } else {
            printf "%s    tags.push(mk_prio(\"%s\", v.to_string(), Value::I32(v as i32), GRP2, PRIO));\n", $ind, $f->{name}
                unless $f->{hidden};
        }
        if ($raw_guard) {
            $ind = substr($ind, 4);
            printf "%s    }\n", $ind;
        }
        printf "%s}\n", $ind;
        print  "    }\n" if defined $f->{re};
    }
    print "    tags\n}\n\n";
}

print "#[cfg(test)]\nmod tests {\n    use super::*;\n\n";
print "    /// Every generated pattern must compile: a bad one would otherwise\n";
print "    /// only surface as a panic on whichever file first reaches it.\n";
print "    #[test]\n    fn every_model_pattern_compiles() {\n";
printf "        LazyLock::force(&MODEL_RE_%d);\n", $_ for (0 .. $#re_list);
print "    }\n}\n";

printf STDERR "Generated %d tables, %d fields, %d model patterns.\n", scalar @tables, $nf, scalar @re_list;
printf STDERR "SKIPPED %d field(s) -- these are NOT silently dropped:\n", scalar @skipped;
printf STDERR "  %s\n", $_ for @skipped;
