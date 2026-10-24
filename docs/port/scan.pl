#!/usr/bin/perl
# Scan all ExifTool tag tables and count code embedded in tagInfo.
use strict; use warnings; no warnings 'once';
use lib '/home/sylvain/dev/exiftool/lib';
use Image::ExifTool;
use File::Find;
my $root = '/home/sylvain/dev/exiftool/lib';
my @mods;
find(sub { return unless /\.pm$/; my $p = $File::Find::name; $p =~ s{^$root/}{}; $p =~ s{\.pm$}{}; $p =~ s{/}{::}g; push @mods, $p }, "$root/Image/ExifTool");
for my $m (sort @mods) {
    next if $m =~ /::(Lang|Charset)::/ or $m =~ /BuildTagLookup|TagInfoXML|HtmlDump|Plot/;
    eval "require $m"; warn "$m: $@" if $@;
}
# also .pl files that define tables
for my $pl (qw(XMP2 QuickTimeStream)) { eval { require "Image/ExifTool/$pl.pl" }; }
my (%tables, %cnt, %pat, %ex, %vals);
sub walk_pkg {
    my $pkg = shift;
    no strict 'refs';
    for my $name (keys %{"${pkg}::"}) {
        if ($name =~ /::$/) { walk_pkg("$pkg\::" . substr($name,0,-2)); next; }
        my $glob = ${"${pkg}::"}{$name};
        next unless ref(\$glob) eq 'GLOB' and *{$glob}{HASH};
        my $h = *{$glob}{HASH};
        my $is = exists $$h{GROUPS} || exists $$h{PROCESS_PROC} || exists $$h{WRITE_PROC} || exists $$h{NOTES};
        unless ($is) { for (values %$h) { if (ref $_ eq 'HASH' and exists $$_{Name}) { $is = 1; last } } }
        $tables{"${pkg}::$name"} = $h if $is;
    }
}
walk_pkg('Image::ExifTool');
my %tagcount; my $ninfo = 0;
my @codekeys = qw(Condition RawConv ValueConv PrintConv RawConvInv ValueConvInv PrintConvInv Hook WriteCheck DelCheck WriteHook);
sub classify {
    my ($key, $v, $where) = @_;
    my $t = ref $v;
    if ($t eq '') { $cnt{$key}{str}++; $vals{$key}{$v}++; patterns($key, $v, $where) if defined $v; }
    elsif ($t eq 'CODE') { $cnt{$key}{code}++ }
    elsif ($t eq 'HASH') {
        $cnt{$key}{hash}++;
        if ($key =~ /PrintConv/) {
            $cnt{"$key.OTHER"}{ref $$v{OTHER} || 'str'}++ if exists $$v{OTHER};
            $cnt{"$key.BITMASK"}{n}++ if exists $$v{BITMASK};
            $cnt{"$key.Notes"}{n}++ if exists $$v{Notes};
            $cnt{"$key.PrintHex"}{n}++ if exists $$v{PrintHex};
        }
    }
    elsif ($t eq 'ARRAY') { $cnt{$key}{array}++; for (@$v) { classify("$key\[]", $_, $where) if defined } }
    else { $cnt{$key}{$t}++ }
}
sub patterns {
    my ($k, $v, $w) = @_;
    my %p = (
        multistmt => scalar($v =~ /;\s*\S/),
        self      => scalar($v =~ /\$self|\$\$self/),
        subst_e   => scalar($v =~ m{s/.*/.*/\w*e\w*}s || $v =~ m{s\{.*\}\{.*\}\w*e}s),
        regex     => scalar($v =~ /=~|!~/),
        modcall   => scalar($v =~ /Image::ExifTool::\w+::\w+\(|Image::ExifTool::\w+\(/),
        selfmeth  => scalar($v =~ /\$self->\w+/),
        valarray  => scalar($v =~ /\$val\[|\$prt\[|\$raw\[|\@val/),
        valPt     => scalar($v =~ /\$\$valPt|\$valPt/),
        dataPt    => scalar($v =~ /\$\$dataPt|\$dataPt/),
        unpack    => scalar($v =~ /\bunpack\b|\bpack\b/),
        sprintf   => scalar($v =~ /sprintf/),
        ternary   => scalar($v =~ /\?.*:/s),
        loop      => scalar($v =~ /\b(foreach|for|while|map|grep)\b/),
        myvar     => scalar($v =~ /\bmy\b/),
        return_   => scalar($v =~ /\breturn\b/),
        multiline => scalar($v =~ /\n/),
        tagInfo   => scalar($v =~ /\$tagInfo|\$\$tagInfo/),
        evalnest  => scalar($v =~ /\beval\b/),
    );
    $pat{$k}{total}++;
    for (keys %p) { if ($p{$_}) { $pat{$k}{$_}++; push @{$ex{$k}{$_}}, "$w: $v" if @{$ex{$k}{$_} || []} < 400; } }
}
my %seen;
sub walk_info {
    my ($info, $where) = @_;
    return if $seen{$info}++;
    $ninfo++;
    for my $k (keys %$info) { $tagcount{$k}++; }
    for my $k (@codekeys) { classify($k, $$info{$k}, $where) if exists $$info{$k}; }
    if (my $sd = $$info{SubDirectory}) {
        if (ref $sd eq 'HASH') { for (keys %$sd) { $cnt{"SubDirectory.$_"}{ref $$sd{$_} || 'str'}++; } }
    }
}
for my $tn (sort keys %tables) {
    my $t = $tables{$tn};
    for my $k (keys %$t) {
        my $v = $$t{$k};
        if ($k =~ /^[A-Z_]+$/ and $k ne 'OTHER' ) { $cnt{"TABLE.$k"}{ref $v || 'str'}++; next unless $k eq 'OTHER'; }
        if ($k eq 'OTHER') { $cnt{"TABLE.OTHER"}{ref $v || 'str'}++; next }
        if (ref $v eq 'HASH') { walk_info($v, "$tn\{$k}") }
        elsif (ref $v eq 'ARRAY') { $cnt{'conditional_list'}{n}++; for (@$v) { walk_info($_, "$tn\{$k}") if ref $_ eq 'HASH' } }
        elsif (!ref $v) { $cnt{'tag_as_name_string'}{n}++ }
    }
}
print "TABLES ", scalar(keys %tables), " TAGINFOS $ninfo\n";
for my $k (sort keys %cnt) { print "CNT $k ", join(' ', map {"$_=$cnt{$k}{$_}"} sort keys %{$cnt{$k}}), "\n"; }
for my $k (sort keys %pat) { print "PAT $k ", join(' ', map {"$_=$pat{$k}{$_}"} sort keys %{$pat{$k}}), "\n"; }
for my $k (sort keys %vals) { print "UNIQ $k ", scalar(keys %{$vals{$k}}), "\n"; }
my @top = sort { $tagcount{$b} <=> $tagcount{$a} } keys %tagcount;
print "KEYS ", join(' ', map {"$_=$tagcount{$_}"} @top), "\n";
open my $fh, '>', '/tmp/claude-1000/-home-sylvain-dev/65f320b5-d334-4de2-b6aa-70d9c4b873ac/scratchpad/examples.txt';
for my $k (sort keys %ex) { for my $p (sort keys %{$ex{$k}}) { print $fh "=== $k $p\n"; print $fh "$_\n---\n" for @{$ex{$k}{$p}}[0..19]; } }
# unique strings dump
open my $u, '>', '/tmp/claude-1000/-home-sylvain-dev/65f320b5-d334-4de2-b6aa-70d9c4b873ac/scratchpad/uniq.txt';
for my $k (sort keys %vals) { for (sort keys %{$vals{$k}}) { my $s = $_; $s =~ s/\n/\\n/g; print $u "$k\t$vals{$k}{$_}\t$s\n"; } }
