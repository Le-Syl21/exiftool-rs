#!/usr/bin/perl
# Generate the EXIF write table: every writable tag of Exif::Main and
# GPS::Main, with what WriteExif.pl needs to store a value: the tag id, its
# `WriteGroup` (the directory it is created in), its `Writable` format, its
# `Count`, the inverse conversion it takes (as a variant of `ExifConv`, or
# `Unported` for Perl code with no port), the inverse of a PrintConv hash,
# and its `Protected` level (SetNewValuesFromFile does not copy those).
#
# Usage: perl scripts/gen_exif_write.pl ../exiftool > src/tags/exif_write_generated.rs

use strict;
use warnings;

my $dir = shift or die "usage: $0 <path-to-exiftool-checkout>\n";
unshift @INC, "$dir/lib";
require Image::ExifTool;
require Image::ExifTool::Exif;
require Image::ExifTool::GPS;

sub q_rs {
    my $s = shift;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    $s =~ s/([\x00-\x1f])/sprintf('\\x%02x', ord $1)/ge;
    return "\"$s\"";
}

# The directories the EXIF editor writes.
my %groups = map { $_ => 1 } qw(IFD0 ExifIFD GPS InteropIFD IFD1);

# The inverse conversions of the table, as the Perl code they run, by the
# ExifConv variant that ports them. Code refs are matched by tag name.
my %byCode = (
    '$val' => 'None',
    '$self->InverseDateTime($val,0)' => 'DateTime',
    '$val=~s/\s*mm$//;$val' => 'StripMm',
    '$val=~s/\s*m$//;$val' => 'StripM',
    '$val=~s/\s*m$//; $val' => 'StripM',
    '$val=~s/ ?C//; $val' => 'StripC',
    'Image::ExifTool::Exif::ConvertParameter($val)' => 'ConvertParameter',
    '$val =~ tr/./ /; $val' => 'DotsToSpaces',
    '$val=~tr/,//d; $val' => 'RemoveCommas',
    '$val=~tr/-/ /; $val' => 'DashesToSpaces',
    '$val=~tr/.//d; $val=~/^\d{4}$/ ? $val : $val =~ /^\d{3}$/ ? "0$val" : undef' => 'ExifVersion',
    '$val=~tr/.//d; $val=~/^\d{4}$/ ? $val : undef' => 'FlashpixVersion',
);
my %byValueCode = (
    '$self->Encode($val,"UTF8")' => 'Utf8',
    '$self->Encode($val,"UCS2","II") . "\0\0"' => 'Ucs2',
    'pack("H*", $val)' => 'Hex',
    '$val=~/^(\d+)\s*$/ ? $1 : ($val=~/\.(\d+)/ ? $1 : undef)' => 'SubSec',
    '$val=~tr/,//d; $val' => 'RemoveCommas',
    '($val=~/^\d+$/ and $val < 256) ? chr($val) : $val' => 'ChrIfSmall',
    'chr($val & 0xff)' => 'Chr',
);

sub classify {
    my ($ti, $p, $v, $r) = @_;
    my $name = $$ti{Name};
    return 'Unported' if $r ne '' and not ($r eq 'Image::ExifTool::Exif::EncodeExifText($self,$val)'
                                          or ($name eq 'Copyright' and $r eq '$val . "\0"'));
    return 'ExifText' if $r eq 'Image::ExifTool::Exif::EncodeExifText($self,$val)';
    return 'Copyright' if $name eq 'Copyright';
    # Pairs of inverse conversions.
    return 'Apex' if $p eq '$val' and $v eq '$val>0 ? 2*log($val)/log(2) : 0';
    return 'ShutterApex' if $p eq 'Image::ExifTool::Exif::ConvertFraction($val)'
                         and $v eq '$val>0 ? -log($val)/log(2) : -100';
    return 'GpsLat' if $p eq 'Image::ExifTool::GPS::ToDegrees($val,undef,"lat")';
    return 'GpsLon' if $p eq 'Image::ExifTool::GPS::ToDegrees($val,undef,"lon")';
    return 'GpsAltitude' if $p eq '$val=~s/\s*m$//;$val'
                         and $v eq '$val=~/((?=\d|\.\d)\d*(?:\.\d*)?)/ ? $1 : undef';
    return 'GpsTimeStamp' if $name eq 'GPSTimeStamp';
    return 'GpsDateStamp' if $name eq 'GPSDateStamp';
    return 'LensInfo' if $p eq 'CODE' and $name eq 'LensInfo';
    return 'OffsetTime' if $p eq 'CODE' and $name =~ /^OffsetTime/;
    my $pk = $p eq '' ? 'None' : $byCode{$p};
    my $vk = $v eq '' ? 'None' : $byValueCode{$v};
    return 'Unported' unless defined $pk and defined $vk;
    return $pk if $vk eq 'None';
    return $vk if $pk eq 'None';
    return 'Unported';
}

my @rows;
foreach my $tname ('Image::ExifTool::Exif::Main', 'Image::ExifTool::GPS::Main') {
    my $table = Image::ExifTool::GetTagTable($tname);
    foreach my $id (sort { $a <=> $b } grep /^\d+$/, Image::ExifTool::TagTableKeys($table)) {
        foreach my $ti (Image::ExifTool::GetTagInfoList($table, $id)) {
            my $format = $$ti{Writable};
            next unless $format and not $$ti{SubDirectory};
            next if $format !~ /^(int8u|int16u|int32u|int16s|int32s|rational64u|rational64s|float|double|string|undef)$/;
            my $group = $$ti{WriteGroup} || $$table{WRITE_GROUP};
            next unless $group and $groups{$group};
            my $code = sub { my $c = shift; defined $c ? (ref $c ? 'CODE' : $c) : '' };
            my ($p, $v, $r) = map { $code->($$ti{$_}) } qw(PrintConvInv ValueConvInv RawConvInv);
            s/^\s+|\s+$//g, s/\s+/ /g foreach $p, $v, $r;
            my $conv;
            my $pc = $$ti{PrintConv};
            # A PrintConv with no inverse is reversed by its hash, if any;
            # a ValueConv with no inverse can't be reversed.
            if ((defined $pc and not ref $pc and $p eq '') or (defined $$ti{ValueConv} and $v eq '' and $p eq '')) {
                $conv = 'Unported';
            } else {
                $conv = classify($ti, $p, $v, $r);
            }
            my (@pairs, $strict);
            $strict = 'false';
            if (ref $pc eq 'HASH') {
                $strict = $$pc{OTHER} ? 'false' : 'true';
                $conv = 'Unported' if $$pc{BITMASK};
                foreach my $k (sort keys %$pc) {
                    next if $k =~ /^(OTHER|BITMASK|Notes|PrintHex|PrintString|PrintSort)$/;
                    next if ref $$pc{$k};
                    push @pairs, '(' . q_rs($$pc{$k}) . ', ' . q_rs($k) . ')';
                }
            } elsif (ref $pc) {
                $conv = 'Unported';
            }
            my $count = $$ti{Count} || 0;
            $count = 0 if $count < 0;
            # The format the value is read in, when it differs (an int8u
            # list stored as undef, for instance).
            my $read = $$ti{Format} || $format;
            push @rows, sprintf("    ExifWriteInfo { name: %s, group: %s, tag: 0x%04x, format: %s, value_format: %s, count: %d, conv: ExifConv::%s, print_inv: &[%s], print_strict: %s, protected: %d },\n",
                q_rs($$ti{Name}), q_rs($group), $id, q_rs($format), q_rs($read), $count, $conv,
                join(', ', @pairs), $strict, $$ti{Protected} || 0);
        }
    }
}
@rows = sort { lc($a) cmp lc($b) } @rows;

print <<'EOT';
//! Auto-generated EXIF write table, from ExifTool's Exif and GPS tag tables.
//!
//! Generated by `scripts/gen_exif_write.pl`. DO NOT EDIT MANUALLY.

/// The inverse conversions a value goes through before it is stored
/// (`PrintConvInv`, then `ValueConvInv`, then `RawConvInv`), ported in
/// `writer::exif_edit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifConv {
    /// None (or the inverse of a PrintConv hash, in `print_inv`).
    None,
    /// Perl code with no port: the tag is not written.
    Unported,
    /// `InverseDateTime($val, 0)`: any date, time zone removed.
    DateTime,
    /// `$val=~s/\s*mm$//`.
    StripMm,
    /// `$val=~s/\s*m$//`.
    StripM,
    /// `$val=~s/ ?C//`.
    StripC,
    /// Exif.pm `ConvertParameter` (Normal/Low/High).
    ConvertParameter,
    /// `tr/./ /` (version numbers).
    DotsToSpaces,
    /// `tr/,//d`.
    RemoveCommas,
    /// `tr/-/ /`.
    DashesToSpaces,
    /// `tr/.//d`, then four digits (a three-digit version gets a leading 0).
    ExifVersion,
    /// `tr/.//d`, then four digits.
    FlashpixVersion,
    /// `Encode($val, "UTF8")`.
    Utf8,
    /// `Encode($val, "UCS2", "II")` plus a null terminator (XP tags).
    Ucs2,
    /// `pack("H*", $val)`.
    Hex,
    /// The digits of a sub-second value.
    SubSec,
    /// `chr($val)` for a number below 256.
    ChrIfSmall,
    /// `chr($val & 0xff)`.
    Chr,
    /// `EncodeExifText`: an 8-byte character code, then the text.
    ExifText,
    /// Copyright: `photographer` [+ `\0` + `editor`] + `\0`.
    Copyright,
    /// `2*log($val)/log(2)` (APEX aperture).
    Apex,
    /// `ConvertFraction`, then `-log($val)/log(2)` (APEX shutter speed).
    ShutterApex,
    /// GPS.pm `ToDegrees(lat)`, then `ToDMS` as three rationals.
    GpsLat,
    /// GPS.pm `ToDegrees(lon)`, then `ToDMS` as three rationals.
    GpsLon,
    /// `$val=~s/\s*m$//`, then the first unsigned number.
    GpsAltitude,
    /// GPS.pm GPSTimeStamp: `HH:MM:SS[.ss]` as three rationals.
    GpsTimeStamp,
    /// GPS.pm GPSDateStamp: `YYYY:mm:dd`.
    GpsDateStamp,
    /// Exif.pm `ConvertLensInfo`, then four rationals.
    LensInfo,
    /// Exif.pm `InverseOffsetTime`: `+HH:MM`.
    OffsetTime,
}

/// One writable EXIF tag.
#[derive(Debug)]
pub struct ExifWriteInfo {
    pub name: &'static str,
    /// The directory the tag is created in (`WriteGroup`).
    pub group: &'static str,
    pub tag: u16,
    /// ExifTool's `Writable` format.
    pub format: &'static str,
    /// The format of the value itself (`Format`, else `Writable`).
    pub value_format: &'static str,
    /// Number of values, 0 for any.
    pub count: u32,
    pub conv: ExifConv,
    /// PrintConv hash, inverted: (printed value, raw value).
    pub print_inv: &'static [(&'static str, &'static str)],
    /// The PrintConv hash has no fallback for other values.
    pub print_strict: bool,
    /// ExifTool's `Protected` level: 1 unsafe, 2 protected.
    pub protected: u8,
}

/// Every writable EXIF tag, sorted by name (case-insensitive).
pub static EXIF_WRITE_TAGS: &[ExifWriteInfo] = &[
EOT
print @rows;
print "];\n";
