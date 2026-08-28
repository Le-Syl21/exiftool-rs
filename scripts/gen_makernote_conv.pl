#!/usr/bin/perl
# Generate src/tags/makernote_conv_generated.rs: the ValueConv and PrintConv
# EXPRESSIONS on the MakerNote Main tables.
#
# The hash-shaped conversions are already ported by gen_print_conv.pl. The ones
# written as Perl -- `$val ? ($val==0xffffffff ? "n/a" : $val) : "Auto"` --
# were not ported at all, so those tags printed their raw value. They are
# carried over verbatim here and evaluated by tags::conv_expr, which declines
# anything outside its grammar and leaves the raw value untouched.
#
# Anything this cannot express is named on stderr. Usage:
#   perl scripts/gen_makernote_conv.pl /path/to/exiftool > src/tags/makernote_conv_generated.rs

use strict;
use warnings;
no strict 'refs';
use Sub::Util qw(subname);

my $dir = $ARGV[0] || '/home/sylvain/dev/exiftool';
die "Cannot find $dir/lib\n" unless -d "$dir/lib";
unshift @INC, "$dir/lib";
require Image::ExifTool;

# The MakerNote Main tables our reader looks tags up in by numeric id.
# GoPro and Leica have no `Main` of their own in ExifTool -- GoPro's tags live
# in GPMF and its siblings, Leica's in Panasonic.pm -- so they are not listed.
my @MAKERS = qw(Sony Canon Nikon Olympus Panasonic Pentax FujiFilm Casio Ricoh
                Sigma Samsung Sanyo Minolta Apple DJI);

my (@rows, @formats, @bitmasks, @members, @skipped);
for my $maker (@MAKERS) {
    my $tbl = eval { Image::ExifTool::GetTagTable("Image::ExifTool::${maker}::Main") } or next;
    for my $id (Image::ExifTool::TagTableKeys($tbl)) {
        next unless $id =~ /^\d+$/;
        next if $id > 0xffff;
        my @infos = Image::ExifTool::GetTagInfoList($tbl, $id);
        # A conditioned chain can give one id several conversions; only an id
        # with a single entry is unambiguous from a numeric lookup.
        if (@infos > 1) {
            my %seen;
            for my $ti (@infos) {
                next unless ref $ti eq 'HASH';
                for my $k ('RawConv', 'ValueConv', 'PrintConv') {
                    my $c = $$ti{$k};
                    $seen{"$k:$c"} = 1 if defined $c and not ref $c;
                }
            }
            if (keys %seen > 1) {
                push @skipped, sprintf("%s 0x%04x: %d conditioned entries disagree on their conversions",
                                       $maker, $id, scalar @infos);
                next;
            }
        }
        for my $ti (@infos) {
            next unless ref $ti eq 'HASH';
            my $name = $$ti{Name} // next;
            # A tag can store its value on the object for a later conversion
            # to read: Pentax's ShutterCount is decrypted with the date and
            # time of the shot, which two other tags recorded.
            if (my $dm = $$ti{DataMember}) {
                push @members, [$maker, $id, $dm, $name] unless ref $dm;
            }
            # A tag can declare a format of its own, which ExifTool reads the
            # entry with whatever the file says: Sony's HDR is an int32u entry
            # `Format => 'int16u', Count => 2`, read as two 16-bit values.
            my $fmt = $$ti{Format};
            if (defined $fmt and not ref $fmt) {
                my ($base, $n) = $fmt =~ /^(\w+?)(?:\[(\d+)\])?$/;
                $n //= $$ti{Count};
                # A Count of -1 means "as many as fit", which a fixed number
                # cannot express: those are reported, not guessed at.
                if (defined $base and defined $n and $n < 0) {
                    push @skipped, sprintf("%s 0x%04x %s: Format %s with Count %d (variable)",
                                           $maker, $id, $name, $base, $n);
                } elsif (defined $base) {
                    push @formats, [$maker, $id, $base, $n // 1, $name];
                }
            }
            # A PrintConv can be a BITMASK: ExifTool runs DecodeBits over the
            # value with the named bits and the tag's BitsPerWord. Sony's
            # AFPointsUsed is ten words of eight bits.
            my $pc = $$ti{PrintConv};
            if (ref $pc eq 'HASH' and ref $$pc{BITMASK} eq 'HASH') {
                my $bits = $$ti{BitsPerWord} || 32;
                my $zero = $$pc{0};
                push @bitmasks, [$maker, $id, $bits, $zero, $$pc{BITMASK}, $name];
            }
            for my $kind ('RawConv', 'ValueConv', 'PrintConv') {
                my $c = $$ti{$kind};
                # A conversion can be a named subroutine rather than an
                # expression. Emitting the call lets tags::conv_expr dispatch
                # it, and an unported name declines there -- which leaves the
                # raw value, exactly as not emitting it would.
                if (ref $c eq 'CODE') {
                    my $sub = subname($c);
                    if ($sub and $sub !~ /__ANON__/) {
                        push @rows, [$maker, $id, $kind, $name, "$sub(\$val)"];
                    } else {
                        push @skipped, sprintf("%s 0x%04x %s: %s is an anonymous subroutine",
                                               $maker, $id, $name, $kind);
                    }
                    next;
                }
                next unless defined $c and not ref $c;   # a hash is not ours
                # A conversion that spans lines is still one expression.
                $c =~ s/\s+/ /g;
                $c =~ s/^ | $//g;
                next unless length $c;
                push @rows, [$maker, $id, $kind, $name, $c];
            }
            last;
        }
    }
}

my $ver = $Image::ExifTool::VERSION;
print <<"HEADER";
// \@generated by scripts/gen_makernote_conv.pl from ExifTool $ver. DO NOT EDIT.
//
// The ValueConv and PrintConv expressions on the MakerNote Main tables, carried
// over verbatim. `tags::conv_expr` evaluates them and declines anything outside
// its grammar, which leaves the raw value -- the same answer a reader that knew
// nothing of the conversion would give.

HEADER

for my $kind ('RawConv', 'ValueConv', 'PrintConv') {
    my $fn = { RawConv => 'raw_conv', ValueConv => 'value_conv', PrintConv => 'print_conv' }->{$kind};
    printf "/// The %s expression ExifTool applies to a MakerNote Main tag.\n", $kind;
    print  "#[must_use]\n";
    printf "pub fn %s_expr(maker: &str, tag: u16) -> Option<&'static str> {\n", $fn;
    print  "    Some(match (maker, tag) {\n";
    my %emitted;
    for my $r (sort { $a->[0] cmp $b->[0] or $a->[1] <=> $b->[1] } grep { $_->[2] eq $kind } @rows) {
        my ($maker, $id, undef, $name, $expr) = @$r;
        next if $emitted{"$maker/$id"}++;
        my $e = $expr;
        $e =~ s/\\/\\\\/g;
        $e =~ s/"/\\"/g;
        printf "        (\"%s\", %#06x) => \"%s\", // %s\n", $maker, $id, $e, $name;
    }
    print  "        _ => return None,\n    })\n}\n\n";
}

print <<'BITHEAD';
/// The bits a MakerNote Main tag names, and how wide each of its words is.
///
/// ExifTool runs DecodeBits over the value with these: each word contributes
/// its own bits, numbered from the word's start, and a value with no bit set
/// prints the table's entry for 0.
///
/// The width in bits, the name of the whole set, and the name of each bit.
pub type Bitmask = (usize, &'static str, &'static [(u32, &'static str)]);

#[must_use]
pub fn bitmask(maker: &str, tag: u16) -> Option<Bitmask> {
    Some(match (maker, tag) {
BITHEAD
{
    my %seen;
    for my $bm (sort { $a->[0] cmp $b->[0] or $a->[1] <=> $b->[1] } @bitmasks) {
        my ($maker, $id, $bits, $zero, $mask, $name) = @$bm;
        next if $seen{"$maker/$id"}++;
        my $z = defined $zero ? $zero : '';
        for ($z) { s/\\/\\\\/g; s/"/\\"/g }
        printf "        (\"%s\", %#06x) => (%d, \"%s\", &[ // %s\n", $maker, $id, $bits, $z, $name;
        for my $k (sort { $a <=> $b } keys %$mask) {
            next unless $k =~ /^\d+$/;
            my $v = $$mask{$k};
            next if ref $v;
            for ($v) { s/\\/\\\\/g; s/"/\\"/g }
            printf "            (%d, \"%s\"),\n", $k, $v;
        }
        print  "        ]),\n";
    }
}
print "        _ => return None,\n    })\n}\n\n";

print "/// The name a MakerNote Main tag stores its value under, for a later\n";
print "/// conversion to read.\n";
print "#[must_use]\n";
print "pub fn data_member(maker: &str, tag: u16) -> Option<&'static str> {\n";
print "    Some(match (maker, tag) {\n";
{
    my %seen;
    for my $m (sort { $a->[0] cmp $b->[0] or $a->[1] <=> $b->[1] } @members) {
        my ($maker, $id, $dm, $name) = @$m;
        next if $seen{"$maker/$id"}++;
        printf "        (\"%s\", %#06x) => \"%s\", // %s\n", $maker, $id, $dm, $name;
    }
}
print "        _ => return None,\n    })\n}\n\n";

print "/// The format a MakerNote Main tag declares for itself, and how many\n";
print "/// elements of it: ExifTool reads the entry that way whatever type the\n";
print "/// file gives it.\n";
print "#[must_use]\n";
print "pub fn format_override(maker: &str, tag: u16) -> Option<(&'static str, usize)> {\n";
print "    Some(match (maker, tag) {\n";
{
    my %seen;
    for my $f (sort { $a->[0] cmp $b->[0] or $a->[1] <=> $b->[1] } @formats) {
        my ($maker, $id, $base, $n, $name) = @$f;
        next if $seen{"$maker/$id"}++;
        printf "        (\"%s\", %#06x) => (\"%s\", %d), // %s\n", $maker, $id, $base, $n, $name;
    }
}
print "        _ => return None,\n    })\n}\n\n";

printf STDERR "%d expression(s) and %d format override(s) over %d makers\n",
    scalar @rows, scalar @formats, scalar @MAKERS;
if (@skipped) {
    printf STDERR "\n%d id(s) NOT emitted:\n", scalar @skipped;
    print STDERR "  $_\n" for @skipped;
}
