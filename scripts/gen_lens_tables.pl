#!/usr/bin/perl
# The lens name lookups ExifTool identifies a lens by.
#
# `XMP::PrintLensID` picks one of these by the Make written in the file and
# looks the LensID up in it; `Exif::PrintLensID` narrows a "A or B or C" entry
# down using focal length and aperture, which it can only do because the table
# holds the alternatives under fractional keys -- 4, 4.1, 4.2. A table read as
# integers alone loses every one of those, so these are emitted whole, with
# their keys as the strings Perl holds them as.
#
# Usage: perl scripts/gen_lens_tables.pl /path/to/exiftool/lib > src/tags/lens_tables_generated.rs

use strict;
use warnings;

my $lib = $ARGV[0] || '../exiftool/lib';
die "Cannot find $lib\n" unless -d $lib;
unshift @INC, $lib;

# (Rust name, module, hash name). SigmaRaw::sigmaLensTypes is empty in
# ExifTool -- XMP::PrintLensID gives up on a Sigma file because of it, so the
# emptiness is the behaviour and is kept rather than papered over with
# Sigma::sigmaLensTypes.
my @TABLES = (
    [ 'canonLensTypes',  'Canon',     'canonLensTypes'  ],
    [ 'nikonLensIDs',    'Nikon',     'nikonLensIDs'    ],
    [ 'pentaxLensTypes', 'Pentax',    'pentaxLensTypes' ],
    [ 'sonyLensTypes',   'Sony',      'sonyLensTypes'   ],
    [ 'sigmaLensTypes',  'SigmaRaw',  'sigmaLensTypes'  ],
    [ 'samsungLensTypes','Samsung',   'samsungLensTypes'],
    [ 'leicaLensTypes',  'Panasonic', 'leicaLensTypes'  ],
    # Reached from Exif::PrintLensID's Sony branch rather than by Make.
    [ 'sonyLensTypes2',  'Sony',      'sonyLensTypes2'  ],
    [ 'sigmaLensTypesFull', 'Sigma',  'sigmaLensTypes'  ],
    [ 'metabonesID',     'Minolta',   'metabonesID'     ],
);

my @skipped;
my %emitted;
for my $t (@TABLES) {
    my ($name, $mod, $hash) = @$t;
    eval "require Image::ExifTool::$mod; 1" or do {
        push @skipped, "$mod: $@";
        next;
    };
    no strict 'refs';
    my $ref = \%{"Image::ExifTool::${mod}::${hash}"};
    use strict 'refs';
    my @rows;
    for my $k (sort keys %$ref) {
        my $v = $ref->{$k};
        if (ref $v eq 'CODE') {
            # An `OTHER => sub` fallback. `XMP::PrintLensID` looks the id up in
            # the hash directly and never reaches it.
            push @skipped, sprintf("%s{%s}: an OTHER handler, which this lookup never reaches", $name, $k);
            next;
        }
        if (ref $v) {
            # metabonesID holds references, not names: the code that reads it
            # only asks whether the key is there. Dropping the key would answer
            # that question wrongly, so it is kept with no name.
            push @skipped, sprintf("%s{%s}: value is a %s reference, kept as a key with no name",
                                   $name, $k, ref $v);
            push @rows, [$k, ''];
            next;
        }
        push @rows, [$k, $v];
    }
    $emitted{$name} = \@rows;
}

sub esc {
    my $s = shift;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    return $s;
}

print <<'HDR';
//! Auto-generated lens name lookups, from ExifTool's own tables.
//!
//! Do not edit: regenerate with
//! `perl scripts/gen_lens_tables.pl ../exiftool/lib > src/tags/lens_tables_generated.rs`.
//!
//! The keys are the strings Perl holds them as, not numbers: ExifTool records
//! the lenses that share a LensID under fractional keys -- 4, 4.1, 4.2 -- and
//! `Exif::PrintLensID` narrows them down by focal length and aperture. A table
//! read as integers alone has no 4.1 in it and answers "Canon EF 35-105mm
//! f/3.5-4.5 or Sigma Lens" where ExifTool answers with the lens.
//!
//! Each table is sorted by key, so a lookup is a binary search.

HDR

for my $t (@TABLES) {
    my $name = $t->[0];
    my $rows = $emitted{$name} or next;
    my $const = uc $name;
    $const =~ s/([a-z])([A-Z])/$1_$2/g;
    printf "/// `Image::ExifTool::%s::%s`, %d entries.\n", $t->[1], $t->[2], scalar @$rows;
    printf "pub static %s: &[(&str, &str)] = &[\n", uc $name;
    printf "    (\"%s\", \"%s\"),\n", esc($_->[0]), esc($_->[1]) for @$rows;
    print "];\n\n";
}

print "/// The table `XMP::PrintLensID` names for a maker, or an empty one\n";
print "/// where ExifTool's is empty too.\n";
print "#[must_use]\n";
print "pub fn table(name: &str) -> Option<&'static [(&'static str, &'static str)]> {\n";
print "    Some(match name {\n";
for my $t (@TABLES) {
    next unless $emitted{$t->[0]};
    printf "        \"%s\" => %s,\n", $t->[0], uc $t->[0];
}
print "        _ => return None,\n    })\n}\n";

printf STDERR "Generated %d tables, %d entries.\n",
    scalar(keys %emitted), scalar(map { @$_ } values %emitted);
printf STDERR "SKIPPED %d -- these are NOT silently dropped:\n", scalar @skipped;
printf STDERR "  %s\n", $_ for @skipped;
