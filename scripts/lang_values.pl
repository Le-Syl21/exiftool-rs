#!/usr/bin/perl
# Dump ExifTool Lang PrintConv value translations for one language.
# Usage: lang_values.pl <langcode>
# Prints "TagName\tEnglishValue\tTranslation" for every translated PrintConv value.
# The Lang *.pm files hold raw UTF-8 bytes (no `use utf8`); passing them straight
# through reproduces valid UTF-8. English keys/values are ExifTool-internal.
use strict;
use warnings;
use lib ($ENV{EXIFTOOL_LIB} // "/home/sylvain/dev/exiftool/lib");

my ($lang) = @ARGV;
exit 0 unless defined $lang;
my $mod = "Image::ExifTool::Lang::$lang";
eval "require $mod";
exit 0 if $@;

no strict 'refs';
my $h = \%{"${mod}::Translate"};
exit 0 unless %$h;

for my $tag (sort keys %$h) {
    my $v = $h->{$tag};
    next unless ref($v) eq 'HASH';
    my $pc = $v->{PrintConv};
    next unless ref($pc) eq 'HASH';
    for my $eng (sort keys %$pc) {
        my $tr = $pc->{$eng};
        next unless defined $tr && length $tr;
        # Tabs are the field separator; guard against any in the data.
        next if $tag =~ /\t/ || $eng =~ /\t/ || $tr =~ /\t/;
        print "$tag\t$eng\t$tr\n";
    }
}
