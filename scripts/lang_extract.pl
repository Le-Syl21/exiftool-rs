#!/usr/bin/perl
# Dump ExifTool Lang translations for a given set of tag names.
# Usage: lang_extract.pl <langcode> <names_file>
# Prints "TagName\tTranslation" for every listed tag present in the Lang module.
# No :utf8 layer: the Lang *.pm files hold raw UTF-8 bytes (no `use utf8`),
# so passing the bytes straight through reproduces valid UTF-8 on disk.
use strict;
use warnings;
# Point EXIFTOOL_LIB at your ExifTool checkout's lib/ (default: sibling dev tree).
use lib ($ENV{EXIFTOOL_LIB} // "/home/sylvain/dev/exiftool/lib");

my ($lang, $names_file) = @ARGV;
exit 0 unless defined $lang && defined $names_file;

my $mod = "Image::ExifTool::Lang::$lang";
eval "require $mod";
exit 0 if $@;    # no such Lang module -> English fallback handled by caller

no strict 'refs';
my $h = \%{"${mod}::Translate"};
exit 0 unless %$h;

open(my $fh, '<', $names_file) or exit 0;
while (my $tag = <$fh>) {
    chomp $tag;
    next unless length $tag && exists $h->{$tag};
    my $v = $h->{$tag};
    $v = $v->{Description} if ref($v) eq 'HASH';
    next unless defined $v && length $v;
    print "$tag\t$v\n";
}
close $fh;
