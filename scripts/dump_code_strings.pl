#!/usr/bin/perl
# Dump every distinct Perl code string held in ExifTool's tag tables, for the
# interpreter's coverage test. Record format: "KIND LENGTH\n" then the bytes
# and a newline.
#   perl scripts/dump_code_strings.pl ../exiftool/lib > tests/data/code_strings.txt
use strict; use warnings; no warnings 'once';
use File::Find;
my $root = shift or die "usage: $0 EXIFTOOL_LIB\n";
unshift @INC, $root;
require Image::ExifTool;
my @mods;
find(sub { return unless /\.pm$/; my $p = $File::Find::name; $p =~ s{^\Q$root\E/}{}; $p =~ s{\.pm$}{}; $p =~ s{/}{::}g; push @mods, $p }, "$root/Image/ExifTool");
for my $m (sort @mods) {
    next if $m =~ /::(Lang|Charset)::/ or $m =~ /BuildTagLookup|TagInfoXML|HtmlDump|Plot/;
    eval "require $m";
}
for my $pl (qw(XMP2 QuickTimeStream)) { eval { require "Image/ExifTool/$pl.pl" }; }
my %tables;
sub walk_pkg {
    my $pkg = shift;
    no strict 'refs';
    for my $name (keys %{"${pkg}::"}) {
        if ($name =~ /::$/) { walk_pkg("$pkg\::" . substr($name, 0, -2)); next; }
        my $glob = ${"${pkg}::"}{$name};
        next unless ref(\$glob) eq 'GLOB' and *{$glob}{HASH};
        my $h = *{$glob}{HASH};
        my $is = exists $$h{GROUPS} || exists $$h{PROCESS_PROC} || exists $$h{WRITE_PROC} || exists $$h{NOTES};
        unless ($is) { for (values %$h) { if (ref $_ eq 'HASH' and exists $$_{Name}) { $is = 1; last } } }
        $tables{"${pkg}::$name"} = $h if $is;
    }
}
walk_pkg('Image::ExifTool');
my @keys = qw(Condition RawConv ValueConv PrintConv RawConvInv ValueConvInv PrintConvInv Hook WriteCheck DelCheck);
my %seen;
sub add {
    my ($kind, $v) = @_;
    return unless defined $v;
    if (ref $v eq 'ARRAY') { add($kind, $_) for @$v; return; }
    if (ref $v eq 'HASH') { add("$kind.OTHER", $$v{OTHER}) if $kind =~ /PrintConv/ and exists $$v{OTHER}; return; }
    return if ref $v;
    $seen{"$kind\0$v"} = 1;
}
for my $t (sort keys %tables) {
    my $h = $tables{$t};
    add('Table.CONDITION', $$h{CONDITION}) if ref $h eq 'HASH';
    for my $tag (keys %$h) {
        my $info = $$h{$tag};
        my @infos = ref $info eq 'ARRAY' ? @$info : ($info);
        for my $i (@infos) {
            next unless ref $i eq 'HASH';
            add($_, $$i{$_}) for grep { exists $$i{$_} } @keys;
            if (ref $$i{Require} eq 'HASH' or ref $$i{Desire} eq 'HASH') { }
        }
    }
}
binmode STDOUT;
for (sort keys %seen) {
    my ($kind, $v) = split /\0/, $_, 2;
    print "$kind ", length($v), "\n", $v, "\n";
}
