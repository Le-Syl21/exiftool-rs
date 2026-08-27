#!/usr/bin/perl
# Which sub-table a MakerNote tag uses on a given body, taken from ExifTool.
#
# Most manufacturers store the same tag in a different layout per camera, and
# ExifTool picks between them with a Condition on the Main table entry. Those
# conditions are regular expressions on the model name, and translating them by
# hand is how a port drifts: `/EOS-1D X$/` is anchored at the end, so a hand
# written `contains("1D X")` also claims the 1D X Mark II -- a body ExifTool has
# no table for -- and decodes it with the wrong layout. Wrong values are worse
# than missing ones, so the patterns are carried over verbatim.
#
# Conditions that need more than the model (data format, byte count, or values
# parsed earlier) are reported, not guessed.
#
# Usage: perl scripts/gen_variant_selectors.pl /path/to/exiftool/lib > src/tags/variant_selectors_generated.rs

use strict;
use warnings;

my $lib = $ARGV[0] || '../exiftool/lib';
die "Cannot find $lib\n" unless -d $lib;

my @modules = qw(Canon Nikon Sony Olympus Pentax Panasonic FujiFilm Samsung Minolta Casio Ricoh Sanyo);


# Expand a finite anchored pattern into the literal byte prefixes it matches.
# Only alternation and single-character classes are accepted: these conditions
# test a version stamp such as /^0210/ or /^030[01]/, never a real expression.
sub prefix_literals {
    my ($pat) = @_;
    return () unless $pat =~ s/^\^//;
    return () if $pat =~ /[.*+?\\(){}\$]/;
    my @out = ('');
    while (length $pat) {
        if ($pat =~ s/^\[([^\]]+)\]//) {
            my @c = split //, $1;
            @out = map { my $p = $_; map { $p . $_ } @c } @out;
        } elsif ($pat =~ s/^([A-Za-z0-9_ -])//) {
            my $c = $1;
            @out = map { $_ . $c } @out;
        } else {
            return ();
        }
    }
    return @out;
}

# Turn one ExifTool Condition into a Rust boolean expression, or undef.
# Supported terms, combined with `and`: a Model regex, a data-prefix test, and
# an element count. Everything else is refused rather than approximated.
sub cond_to_rust {
    my ($cond, $re_id) = @_;
    my @terms;
    # ExifTool writes the same access two ways depending on the module's age.
    $cond =~ s/\$self->\{/\$\$self\{/g;
    for my $term (split /\s+and\s+/, $cond) {
        $term =~ s/^\s*\(?\s*//; $term =~ s/\s*\)?\s*$//;
        if ($term =~ /^\$\$self\{Model\}\s*(=~|!~)\s*\/([^\/]*)\/$/) {
            my ($op, $pat) = ($1, $2);
            return undef if $pat =~ /\(\?[=!<]/;
            push @terms, sprintf('%sRE_%d.is_match(model)', ($op eq '!~' ? '!' : ''), $re_id->($pat));
        } elsif ($term =~ /^\$\$self\{Model\}\s+(eq|ne)\s+"([^"]*)"$/) {
            push @terms, sprintf('model %s "%s"', ($1 eq 'eq' ? '==' : '!='), $2);
        } elsif ($term =~ /^\$\$valPt\s*(=~|!~)\s*\/([^\/]*)\/$/) {
            my ($op, $pat) = ($1, $2);
            my @lits = prefix_literals($pat);
            return undef unless @lits;
            my $any = join(' || ', map { sprintf('data.starts_with(b"%s")', $_) } @lits);
            push @terms, ($op eq '!~' ? "!($any)" : "($any)");
        } elsif ($term =~ /^\$format\s+(eq|ne)\s+"(\w+)"$/) {
            push @terms, sprintf('format %s "%s"', ($1 eq 'eq' ? '==' : '!='), $2);
        } elsif ($term =~ /^\$count\s*==\s*(\d+)$/) {
            push @terms, "count == $1";
        } elsif ($term =~ /^\$count\s*!=\s*(\d+)$/) {
            push @terms, "count != $1";
        } elsif ($term =~ /^\$count\s*==\s*(\d+)((?:\s*(?:or|\|\|)\s*\$count\s*==\s*\d+)+)$/) {
            my @n = ($1); push @n, $2 =~ /(\d+)/g;
            push @terms, '(' . join(' || ', map { "count == $_" } @n) . ')';
        } else {
            return undef;
        }
    }
    return join(' && ', @terms);
}

my (@entries, @skipped);
my (%re_index, @re_list);
sub re_id {
    my ($p) = @_;
    return $re_index{$p} if exists $re_index{$p};
    push @re_list, $p;
    return $re_index{$p} = $#re_list;
}

my $et_version = '?';
for my $module (@modules) {
    my $file = "$lib/Image/ExifTool/$module.pm";
    next unless -f $file;
    open my $h, '<', $file or next;
    my $content = do { local $/; <$h> };
    close $h;
    ($et_version) = $content =~ /\$VERSION\s*=\s*'([^']+)'/ if $et_version eq '?';

    my ($main) = $content =~ /^%Image::ExifTool::\Q$module\E::Main\s*=\s*\((.*?)\n\);/ms;
    next unless $main;

    # Tags whose value is a list of alternatives.
    while ($main =~ /^\s{4}(0x[0-9a-fA-F]+)\s*=>\s*\[(.*?)\n\s{4}\],/gms) {
        my $tag  = hex($1);
        my $arms = $2;
        my @choices;
        my $incomplete = 0;
        while ($arms =~ /Name\s*=>\s*'([^']+)'(.*?)TagTable\s*=>\s*'Image::ExifTool::\Q$module\E::(\w+)'/gs) {
            my ($nm, $mid, $tbl) = ($1, $2, $3);
            my ($cond) = $mid =~ /Condition\s*=>\s*'([^']*)'/;
            if (!defined $cond) {
                # An unconditional arm is a fallback: ExifTool only reaches it
                # once every earlier condition has failed. If we could not
                # express one of those, we cannot know we would have reached
                # this one, and answering anyway would hand back the wrong
                # layout. Stop here and say we do not know.
                if ($incomplete) {
                    push @skipped, sprintf("%s 0x%04x %s: fallback unreachable, an earlier condition was not expressible", $module, $tag, $nm);
                    last;
                }
                push @choices, { tbl => $tbl, re => undef, neg => 0 };
                last;
            } elsif (defined(my $expr = cond_to_rust($cond, \&re_id))) {
                push @choices, { tbl => $tbl, expr => $expr };
            } else {
                push @skipped, sprintf("%s 0x%04x %s: %s", $module, $tag, $nm, $cond);
                $incomplete = 1;
            }
        }
        push @entries, { module => $module, tag => $tag, choices => \@choices } if @choices;
    }
}

# ------------------------------------------------------------------ emission
print <<"HDR";
//! Auto-generated MakerNote sub-table selection, from ExifTool $et_version.
//!
//! Generated by scripts/gen_variant_selectors.pl -- DO NOT EDIT MANUALLY.
//!
//! Manufacturers store the same tag in a different layout per body, and
//! ExifTool chooses between them with a regular expression on the model name.
//! Those expressions are carried over as written rather than translated, so a
//! pattern anchored at the end of the model cannot quietly become a substring
//! test here.

use std::sync::LazyLock;

use regex_lite::Regex;

HDR

for my $i (0 .. $#re_list) {
    my $p = $re_list[$i];
    $p =~ s/\\/\\\\/g;
    $p =~ s/"/\\"/g;
    printf "static RE_%d: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"%s\").unwrap());\n", $i, $p;
}

print <<'FN';

/// The sub-table ExifTool would use for this MakerNote tag on this body.
///
/// `None` means no alternative applies -- which is an answer, not a failure:
/// ExifTool falls back to an "unknown" table there, and decoding with the
/// nearest-looking layout instead would invent values.
#[must_use]
pub fn variant_for(
    module: &str,
    tag: u16,
    model: &str,
    data: &[u8],
    count: usize,
    format: &str,
) -> Option<&'static str> {
    let _ = (data, count, format);
    match (module, tag) {
FN

for my $e (sort { $a->{module} cmp $b->{module} || $a->{tag} <=> $b->{tag} } @entries) {
    printf "        (\"%s\", 0x%04x) => {\n", $e->{module}, $e->{tag};
    for my $c (@{$e->{choices}}) {
        if (defined $c->{expr}) {
            printf "            if %s { return Some(\"%s\"); }\n", $c->{expr}, $c->{tbl};
        } else {
            printf "            return Some(\"%s\");\n", $c->{tbl};
        }
    }
    print  "            None\n        }\n";
}
print "        _ => None,\n    }\n}\n";

print "\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n";
print "    /// A bad pattern must fail here, not on whichever file first reaches it.\n";
print "    #[test]\n    fn every_pattern_compiles() {\n";
printf "        LazyLock::force(&RE_%d);\n", $_ for (0 .. $#re_list);
print "    }\n\n";
print <<'T2';
    /// The anchors matter: ExifTool's `/EOS-1D X$/` does not claim the Mark II,
    /// and a substring test would.
    #[test]
    fn anchored_patterns_do_not_over_match() {
        assert_eq!(variant_for("Canon", 0x000d, "Canon EOS-1D X", b"", 0, "int8u"), Some("CameraInfo1DX"));
        assert_eq!(variant_for("Canon", 0x000d, "Canon EOS-1D X Mark II", b"", 0, "int8u"), None);
    }
}
T2

printf STDERR "Generated %d selectors over %d patterns.\n", scalar @entries, scalar @re_list;
printf STDERR "SKIPPED %d condition(s), needing more than the model:\n", scalar @skipped;
printf STDERR "  %s\n", $_ for @skipped;
