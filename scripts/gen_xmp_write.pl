#!/usr/bin/perl
# Generate the XMP write table: every top-level, writable property of the
# XMP namespace tables (XMP.pm, XMP2.pl and the vendor modules), with what
# the writer needs to put a value where ExifTool would:
#
#   * the family-1 group (`XMP-dc`), the tag name, the namespace prefix and
#     URI, and the property name;
#   * the value kind (simple, lang-alt, Bag, Seq, Alt) and the `Writable`
#     type CheckXMP formats the value by (`integer`, `real`, `rational`,
#     `date`, `boolean`, or a plain string);
#   * `Avoid`, which SetNewValue uses to pick the namespace a tag is created
#     in when several define it (`XMP:Description` goes to XMP-dc, not to
#     the avoided XMP-xmp one);
#   * the inverse of a PrintConv hash, and whether a conversion written in
#     Perl stands in the way (such a tag is not written).
#
# Structures and their flattened fields are left out.
#
# Usage: perl scripts/gen_xmp_write.pl ../exiftool > src/tags/xmp_write_generated.rs

use strict;
use warnings;
no warnings "once";

my $dir = shift or die "usage: $0 <path-to-exiftool-checkout>\n";
unshift @INC, "$dir/lib";
require Image::ExifTool;
require Image::ExifTool::XMP;

Image::ExifTool::LoadAllTables();

sub q_rs {
    my $s = shift;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    $s =~ s/\n/\\n/g;
    $s =~ s/([\x00-\x1f])/sprintf('\\x%02x', ord $1)/ge;
    return "\"$s\"";
}

# The Rust conversion standing for a pair of inverse conversions
# (PrintConvInv, ValueConvInv), or 'Unported' for code with no port. The
# patterns are those of ExifTool 13.59's XMP tables.
sub classify {
    my ($p, $v, $name, $writable) = @_;
    my $date = q{$self->InverseDateTime($val)};
    # PrintConvInv first, then ValueConvInv. A date property's
    # InverseDateTime is FormatXMPDate's job (`format_xmp_date`).
    my $pk;
    if ($p eq '' or $p eq '$val' or $p eq $date or
        ($writable eq 'date' and $p =~ /^\$self->InverseDateTime\(\$val\b/))
    {
        $pk = '';
    } elsif ($p =~ /^Image::ExifTool::GPS::ToDegrees\(\$val, 1, "(lat|lon)"\)$/) {
        $pk = $1 eq 'lat' ? 'Lat' : 'Lon';
    } elsif ($p eq '$val=~s/\s*mm$//;$val') {
        $pk = 'StripMm';
    } elsif ($p =~ /^\$val=~s\/\\s\*m\$\/\/; ?\$val$/) {
        $pk = 'StripM';
    } elsif ($p eq 'Image::ExifTool::Exif::ConvertParameter($val)') {
        $pk = 'ConvertParameter';
    } elsif ($p eq 'Image::ExifTool::Exif::ConvertFraction($val)') {
        $pk = 'Fraction';
    } elsif ($p eq 'CODE' and $name eq 'LensInfo') {
        $pk = 'LensInfo';
    } elsif ($p =~ /^\$val =~ s\[Tagged:/) {
        return 'PhotoMechPrefs';
    } elsif ($p =~ /pack\('C\*', split/) {
        return 'DottedVersion';
    } elsif ($p eq '$val =~ /^\w{4}$/ ? unpack("N", $val) : $val') {
        return 'FourCc';
    } else {
        return 'Unported';
    }
    my $vk;
    if ($v eq '' or $v eq '$val') {
        $vk = '';
    } elsif ($v =~ /^Image::ExifTool::GPS::ToDMS\(\$self, \$val, 2, "[NE]"\)$/) {
        $vk = 'Dms';
    } elsif ($v =~ /EncodeBase64\(\$val\)/) {
        $vk = 'Base64';
    } elsif ($v eq 'GetUnixTime($val)') {
        $vk = 'UnixTime';
    } elsif ($v eq '"http://ns.useplus.org/ldf/vocab/$val"') {
        $vk = 'PlusVocab';
    } elsif ($v eq 'Image::ExifTool::IPTC::IptcTime($val)') {
        $vk = 'IptcTime';
    } elsif ($v =~ /^warn "Expected one or more integer values"/) {
        $vk = 'IntegerList';
    } elsif ($v eq '$val=~/((?=\d|\.\d)\d*(?:\.\d*)?)/ ? $1 : undef') {
        $vk = 'Number';
    } elsif ($v eq '$val>0 ? 2*log($val)/log(2) : 0') {
        $vk = 'Apex';
    } elsif ($v eq '$val>0 ? -log($val)/log(2) : 0') {
        $vk = 'ShutterApex';
    } elsif ($v eq '"/$val"') {
        $vk = 'PdfName';
    } elsif ($v =~ /WritePDFValue\(\$self,\$val,"date"\)/) {
        $vk = 'PdfDate';
    } elsif ($v eq 'CODE' and $name eq 'LensInfo') {
        $vk = 'Rationals';
    } else {
        return 'Unported';
    }
    my %known = (
        '' => 'None', 'Lat' => 'GpsDegreesLat', 'Lon' => 'GpsDegreesLon',
        'LatDms' => 'GpsDmsLat', 'LonDms' => 'GpsDmsLon', 'StripMm' => 'StripMm',
        'StripM' => 'StripM', 'StripMNumber' => 'GpsAltitude',
        'ConvertParameter' => 'ConvertParameter', 'FractionShutterApex' => 'ShutterApex',
        'Apex' => 'Apex', 'Base64' => 'Base64', 'UnixTime' => 'UnixTime',
        'PlusVocab' => 'PlusVocab', 'IptcTime' => 'IptcTime', 'IntegerList' => 'IntegerList',
        'PdfName' => 'PdfName', 'PdfDate' => 'PdfDate', 'LensInfoRationals' => 'LensInfo',
    );
    return $known{$pk . $vk} || 'Unported';
}

my %skipNS = map { $_ => 1 } qw(x rdf xmlns xml);
my (%seen, @rows);
foreach my $tname (sort keys %Image::ExifTool::allTables) {
    my $table = $Image::ExifTool::allTables{$tname};
    next unless ref $table eq 'HASH' and $$table{GROUPS} and ($$table{GROUPS}{0} || '') eq 'XMP';
    my $ns = $$table{NAMESPACE};
    next unless defined $ns;
    my ($prefix, $uri);
    if (ref $ns eq 'HASH') {
        ($prefix) = keys %$ns;
        $uri = $$ns{$prefix};
    } else {
        $prefix = $ns;
        $uri = $Image::ExifTool::XMP::nsURI{$prefix};
    }
    next unless defined $prefix and defined $uri and not $skipNS{$prefix};
    my $group1 = $$table{GROUPS}{1} || "XMP-$prefix";
    foreach my $id (sort(Image::ExifTool::TagTableKeys($table))) {
        foreach my $ti (Image::ExifTool::GetTagInfoList($table, $id)) {
            next if $$ti{Struct} or defined $$ti{Flat} or $$ti{SubDirectory};
            next if $$ti{RootTagInfo} or $$ti{PropertyPath};
            my $writable = defined $$ti{Writable} ? $$ti{Writable} : $$table{WRITABLE};
            next unless $writable;
            next if $id =~ /[^\w.-]/;     # not a plain property name
            my $name = $$ti{Name};
            next if $seen{"$group1\t$name"}++;
            my $list = $$ti{List} || '';
            $list = 'Bag' if $list eq '1';
            my $kind;
            if ($writable eq 'lang-alt') {
                next if $list;              # list of lang-alt: not handled
                $kind = 'LangAlt';
            } else {
                $kind = $list ? $list : 'Simple';
            }
            next unless $kind =~ /^(Simple|LangAlt|Bag|Seq|Alt)$/;
            $writable = 'string' if $writable eq 'lang-alt' or $writable eq '1';
            my $conv = 'None';
            my @pairs;
            my $strict = 'false';
            my $pc = $$ti{PrintConv};
            if (ref $pc eq 'HASH') {
                $strict = $$pc{OTHER} ? 'false' : 'true';
                $conv = 'Unported' if $$pc{BITMASK};
                foreach my $k (sort keys %$pc) {
                    next if $k =~ /^(OTHER|BITMASK|Notes|PrintHex|PrintString|PrintSort)$/;
                    next if ref $$pc{$k};
                    push @pairs, '(' . q_rs($$pc{$k}) . ', ' . q_rs($k) . ')';
                }
            }
            # The inverse conversions, as the Perl code they run.
            my $pci = $$ti{PrintConvInv};
            my $vci = $$ti{ValueConvInv};
            my $code = sub { my $c = shift; defined $c ? (ref $c ? 'CODE' : $c) : '' };
            my ($p, $v) = ($code->($pci), $code->($vci));
            s/^\s+|\s+$//g, s/\s+/ /g foreach $p, $v;
            # A PrintConv with no inverse is reversed by its hash, if any.
            if (defined $pc and not ref $pc and not defined $pci and
                not ($writable eq 'date' and $pc =~ /ConvertDateTime/))
            {
                $conv = 'Unported';
            }
            # A ValueConv with no inverse can't be reversed.
            $conv = 'Unported' if defined $$ti{ValueConv} and not defined $vci;
            $conv = 'Unported' if $$ti{RawConvInv};
            if ($conv eq 'None') {
                $conv = classify($p, $v, $$ti{Name}, $writable);
            }
            $writable = 'string' unless $writable =~ /^(string|integer|real|rational|date|boolean)$/;
            push @rows, sprintf("    XmpWriteInfo { group1: %s, name: %s, prefix: %s, uri: %s, property: %s, kind: XmpKind::%s, writable: XmpType::%s, avoid: %s, conv: XmpConv::%s, print_inv: &[%s], print_strict: %s },\n",
                q_rs($group1), q_rs($name), q_rs($prefix), q_rs($uri), q_rs($id), $kind,
                ucfirst($writable), $$ti{Avoid} ? 'true' : 'false', $conv, join(', ', @pairs), $strict);
        }
    }
}
@rows = sort { lc($a) cmp lc($b) } @rows;

print <<'EOT';
//! Auto-generated XMP write table, from ExifTool's XMP tag tables.
//!
//! Generated by `scripts/gen_xmp_write.pl`. DO NOT EDIT MANUALLY.

/// How a property holds its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmpKind {
    Simple,
    LangAlt,
    Bag,
    Seq,
    Alt,
}

/// ExifTool's `Writable` type, which CheckXMP formats the value by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmpType {
    String,
    Integer,
    Real,
    Rational,
    Date,
    Boolean,
}

/// The inverse conversions a value goes through before `CheckXMP`
/// (`PrintConvInv`, then `ValueConvInv`), ported in `writer::xmp_tags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmpConv {
    /// None (or the inverse of a PrintConv hash, in `print_inv`).
    None,
    /// Perl code with no port: the tag is not written.
    Unported,
    /// GPS.pm `ToDegrees($val, 1, "lat"|"lon")`.
    GpsDegreesLat,
    GpsDegreesLon,
    /// `ToDegrees` then `ToDMS($self, $val, 2, "N"|"E")`.
    GpsDmsLat,
    GpsDmsLon,
    /// `$val=~s/\s*mm$//`.
    StripMm,
    /// `$val=~s/\s*m$//`.
    StripM,
    /// `$val=~s/\s*m$//`, then the first unsigned number.
    GpsAltitude,
    /// Exif.pm `ConvertParameter` (Normal/Low/High).
    ConvertParameter,
    /// `2*log($val)/log(2)` (APEX aperture).
    Apex,
    /// `ConvertFraction`, then `-log($val)/log(2)` (APEX shutter speed).
    ShutterApex,
    /// WriteXMP.pl `EncodeBase64`.
    Base64,
    /// `InverseDateTime`, then `GetUnixTime`.
    UnixTime,
    /// The PLUS vocabulary URI prefix.
    PlusVocab,
    /// WriteIPTC.pl `IptcTime`.
    IptcTime,
    /// Integers separated by spaces, or the value is rejected.
    IntegerList,
    /// A PDF name: `/` prefix.
    PdfName,
    /// `InverseDateTime`, then a PDF date.
    PdfDate,
    /// Exif.pm `ConvertLensInfo`, then four rationals.
    LensInfo,
    /// PhotoMechanic `Prefs`: `Tagged:..., ColorClass:...` → `t:c:r:f`.
    PhotoMechPrefs,
    /// `a.b.c.d` → the int32u of those four bytes.
    DottedVersion,
    /// Four word characters → their int32u.
    FourCc,
}

/// One writable XMP property.
#[derive(Debug)]
pub struct XmpWriteInfo {
    pub group1: &'static str,
    pub name: &'static str,
    pub prefix: &'static str,
    pub uri: &'static str,
    pub property: &'static str,
    pub kind: XmpKind,
    pub writable: XmpType,
    pub avoid: bool,
    pub conv: XmpConv,
    /// PrintConv hash, inverted: (printed value, raw value).
    pub print_inv: &'static [(&'static str, &'static str)],
    /// The PrintConv hash has no fallback for other values.
    pub print_strict: bool,
}

/// Every writable top-level XMP property, sorted by group and name
/// (case-insensitive).
pub static XMP_WRITE_TAGS: &[XmpWriteInfo] = &[
EOT
print @rows;
print "];\n";
