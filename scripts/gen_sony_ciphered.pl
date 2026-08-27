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

while ($src =~ /^%Image::ExifTool::Sony::(Tag(?:2010[a-z]?|9050[a-z]?|94[0-9a-f]{2}[a-z]?))\s*=\s*\((.*?)\n\);/gms) {
    my ($table, $body) = ($1, $2);

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

    my @fields;
    while ($body =~ /^\s{4}(0x[0-9a-fA-F]+|\d+)\s*=>\s*\{(.*?)^\s{4}\}/gms) {
        my $off_s = $1;
        my $fb    = $2;
        my $off   = $off_s =~ /^0x/ ? hex($off_s) : int($off_s);

        my ($name) = $fb =~ /Name\s*=>\s*'([^']+)'/;
        next unless $name;

        my ($ffmt) = $fb =~ /Format\s*=>\s*'([^']+)'/;
        $ffmt ||= $fmt;
        # A fixed-size array of a scalar type is just N reads joined by spaces,
        # which is how ExifTool prints one. Large ones are sub-structures with a
        # decoder of their own, so they stay reported rather than flattened.
        my $count_n = 1;
        if ($ffmt =~ /^(\w+)\[(\d+)\]$/ and exists $WIDTH{$1}) {
            ($ffmt, $count_n) = ($1, $2);
            if ($count_n > 64) {
                push @skipped, "$table 0x" . sprintf('%04x', $off) . " $name: array of $count_n, likely a sub-structure";
                next;
            }
        }
        unless (exists $WIDTH{$ffmt}) {
            push @skipped, "$table 0x" . sprintf('%04x', $off) . " $name: format '$ffmt'";
            next;
        }

        # Per-field condition. Only a bare Model regex is portable; anything
        # referring to other parse state is reported and the field dropped.
        my $re = undef;
        my $neg = 0;
        if ($fb =~ /Condition\s*=>\s*'([^']*)'/) {
            my $cond = $1;
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
            } elsif ($cond =~ /^\s*\$\$self\{(\w+)\}\s*(==|!=)\s*(\d+)\s*$/) {
                # A DATAMEMBER set by an earlier field of this same table:
                # LensMount is read at 0x0105 and decides how 0x0107 is named.
                # Fields are emitted in offset order, so the value is there.
                $re = { dm => $1, op => $2, num => $3 };
            } else {
                push @skipped, "$table 0x" . sprintf('%04x', $off) . " $name: condition needs parse state -- $cond";
                next;
            }
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
        my ($vconv) = $fb =~ /ValueConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        my ($pconv) = $fb =~ /PrintConv\s*=>\s*'((?:[^'\\]|\\.)*)'/;
        for ($vconv, $pconv) { $_ =~ s/\\'/'/g if defined }

        push @fields, {
            off => $off, name => $name, fmt => $ffmt, n => $count_n,
            re => $re, neg => $neg, conv => \%conv,
            vconv => $vconv, pconv => $pconv,
        };
    }

    next unless @fields;
    push @tables, { name => $table, unit => $unit, fields => \@fields, grp2 => $grp2 };
}

# ------------------------------------------------- variant selection (Main)
# ExifTool picks the variant of each ciphered tag with a Condition on the Main
# table entry. Carried over as regexes for the same reason as above.
my @selectors;
my ($main) = $src =~ /^%Image::ExifTool::Sony::Main\s*=\s*\((.*?)\n\);/ms;
$main ||= '';
while ($main =~ /^\s{4}(0x[0-9a-fA-F]+)\s*=>\s*\[(.*?)\}\],\s*$/gms) {
    my $tag = hex($1);
    my $arms = $2;
    my @choices;
    while ($arms =~ /Name\s*=>\s*'([^']+)'.*?(?:Condition\s*=>\s*'([^']*)'.*?)?TagTable\s*=>\s*'Image::ExifTool::Sony::(\w+)'/gs) {
        my ($nm, $cond, $tbl) = ($1, $2, $3);
        next unless grep { $_->{name} eq $tbl } @tables;
        if (!defined $cond || $cond !~ /\S/) {
            push @choices, { tbl => $tbl, re => undef, neg => 0 };
        } elsif ($cond =~ /^\s*\$\$self\{Model\}\s*(=~|!~)\s*\/([^\/]*)\/\s*$/) {
            my $neg = ($1 eq '!~');
            my $pat = $2;
            if ($pat =~ /\(\?[=!<]/) { push @skipped, "variant $tbl: lookaround"; next; }
            push @choices, { tbl => $tbl, re => re_id($pat), neg => $neg };
        } else {
            push @skipped, sprintf("variant 0x%04x -> %s: condition needs parse state -- %s", $tag, $tbl, $cond);
        }
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
    next unless $tbl and grep { $_->{name} eq $tbl } @tables;
    push @selectors, { tag => $tag, choices => [ { tbl => $tbl, re => undef, neg => 0 } ] };
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

/// The most recent value read under this name in the table being decoded.
///
/// ExifTool calls these DATAMEMBERs: a field stores its value so a later field
/// can be conditioned on it, which is how one offset holds LensType2 on an
/// E-mount body and LensType on an A-mount one.
fn dm_get(dm: &[(&str, f64)], name: &str) -> Option<f64> {
    dm.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

fn mk(name: &str, print_value: String, raw: Value, default_group2: &'static str) -> Tag {
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
        priority: 0,
    }
}

RD

# dispatcher
print "/// Decode one deciphered Sony sub-table. `data` must already be deciphered.\n";
print "#[must_use]\n";
print "pub fn decode(table: &str, data: &[u8], model: &str) -> Vec<Tag> {\n";
print "    match table {\n";
printf("        \"%s\" => %s(data, model),\n", $_->{name}, lc $_->{name}) for @tables;
print "        _ => Vec::new(),\n    }\n}\n\n";

print "/// Which ciphered sub-table a Sony MakerNote tag uses on this body.\n";
print "///\n/// Mirrors the Condition chain on the Sony Main table entry.\n";
print "#[must_use]\n";
print "pub fn variant_for(tag: u16, model: &str) -> Option<&'static str> {\n";
print "    match tag {\n";
for my $s (@selectors) {
    printf "        0x%04x => {\n", $s->{tag};
    my $unconditional = 0;
    for my $c (@{$s->{choices}}) {
        if (defined $c->{re}) {
            printf "            if %sMODEL_RE_%d.is_match(model) { return Some(\"%s\"); }\n",
                ($c->{neg} ? '!' : ''), $c->{re}, $c->{tbl};
        } else {
            printf "            Some(\"%s\")\n", $c->{tbl};
            $unconditional = 1;
            last;   # nothing after an unconditional arm can be reached
        }
    }
    print  "            None\n" unless $unconditional;
    print  "        }\n";
}
print "        _ => None,\n    }\n}\n\n";

for my $t (@tables) {
    printf "fn %s(data: &[u8], model: &str) -> Vec<Tag> {\n", lc $t->{name};
    printf "    const GRP2: &str = \"%s\";\n", $t->{grp2};
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
                printf "%sif dm_get(&dm, \"%s\") %s Some(%s.0) {\n",
                    $ind, $f->{re}{dm}, ($f->{re}{op} eq '==' ? '==' : '!='), $f->{re}{num};
            } else {
                printf "%sif %sMODEL_RE_%d.is_match(model) {\n", $ind, ($f->{neg} ? '!' : ''), $f->{re};
            }
            $ind = "        ";
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
                printf "%s        tags.push(mk(\"%s\", hex, Value::String(%s), GRP2));\n", $ind, $f->{name}, $joined;
            } else {
                printf "%s        let s = %s;\n", $ind, $joined;
                printf "%s        tags.push(mk(\"%s\", s.clone(), Value::String(s), GRP2));\n", $ind, $f->{name};
            }
            printf "%s    }\n%s}\n", $ind, $ind;
            print  "    }\n" if defined $f->{re};
            next;
        }
        printf "%sif let Some(v) = %s(data, 0x%x) {\n", $ind, $reader, $f->{off};
        printf "%s    dm.push((\"%s\", f64::from(v)));\n", $ind, $f->{name};
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
            printf "%s    tags.push(mk(\"%s\", s, Value::I32(v as i32), GRP2));\n", $ind, $f->{name};
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
            printf "%s    tags.push(mk(\"%s\", cv.as_string(), raw, GRP2));\n", $ind, $f->{name};
        } else {
            printf "%s    tags.push(mk(\"%s\", v.to_string(), Value::I32(v as i32), GRP2));\n", $ind, $f->{name};
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
