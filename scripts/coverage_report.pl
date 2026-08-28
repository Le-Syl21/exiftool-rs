#!/usr/bin/perl
# What ExifTool defines that we never reach.
#
# Parity over a corpus can only find what the corpus exercises. This compares
# the two sources directly instead: every binary sub-table ExifTool defines,
# against every name our crate mentions anywhere. A table nobody names is a
# table nobody can reach, whatever the test files happen to contain.
#
# Usage: perl scripts/coverage_report.pl /path/to/exiftool/lib [src_dir]

use strict;
use warnings;

my $lib = $ARGV[0] || '../exiftool/lib';
my $src = $ARGV[1] || 'src';
die "Cannot find $lib\n" unless -d $lib;

# Everything our crate mentions, in one blob.
my $ours = '';
open my $find, '-|', 'find', $src, '-name', '*.rs' or die $!;
while (my $f = <$find>) { chomp $f; open my $h, '<', $f or next; local $/; $ours .= <$h>; close $h; }
close $find;

# Everything ExifTool itself points at. A table nothing names is one no
# reader can reach -- ExifTool's own included -- so counting it against us
# would be counting something that cannot be done.
my $lib_src = '';
{
    opendir my $d, "$lib/Image/ExifTool" or die $!;
    # TagLookup.pm names every table there is, so it says nothing about
    # whether anything points at one.
    for my $pm (sort grep { /\.pm$/ and $_ ne 'TagLookup.pm' } readdir $d) {
        open my $h, '<', "$lib/Image/ExifTool/$pm" or next;
        local $/;
        $lib_src .= <$h>;
        close $h;
    }
    closedir $d;
}

my (%total, %missing, %orphan);
opendir my $dh, "$lib/Image/ExifTool" or die $!;
for my $pm (sort grep { /\.pm$/ } readdir $dh) {
    my $module = $pm; $module =~ s/\.pm$//;
    open my $h, '<', "$lib/Image/ExifTool/$pm" or next;
    my $content = do { local $/; <$h> };
    close $h;

    while ($content =~ /^%Image::ExifTool::\Q$module\E::(\w+)\s*=\s*\((.*?)\n\);/gms) {
        my ($table, $body) = ($1, $2);
        next if $table eq 'Main';
        # Only binary sub-tables: those are the ones a reader must decode itself.
        next unless $body =~ /FIRST_ENTRY/;
        # A reference, not the definition: `'Image::ExifTool::Mod::Table'`
        # in quotes is how a SubDirectory names one. A table nothing names is
        # still counted -- shrinking the denominator would be answering an
        # easier question than the one asked -- but it is named apart, because
        # no reader can reach what its own module never points at.
        push @{$orphan{$module}}, $table
            unless $lib_src =~ /'Image::ExifTool::\Q$module\E::\Q$table\E'/;
        $total{$module}++;
        # A table we can reach is one whose name appears somewhere in our source,
        # whether in a generated file or a hand-written dispatcher.
        push @{$missing{$module}}, $table unless $ours =~ /\Q$table\E/;
    }
}
closedir $dh;

my ($t, $m) = (0, 0);
printf "%-14s %8s %8s  %s\n", 'MODULE', 'TABLES', 'MISSING', 'COVERAGE';
for my $mod (sort { scalar(@{$missing{$b} || []}) <=> scalar(@{$missing{$a} || []}) } keys %total) {
    my $miss = scalar @{$missing{$mod} || []};
    $t += $total{$mod}; $m += $miss;
    next unless $miss;
    printf "%-14s %8d %8d  %3d%%\n", $mod, $total{$mod}, $miss,
        int(100 * ($total{$mod} - $miss) / $total{$mod});
}
printf "\nTOTAL: %d binary sub-tables defined, %d never mentioned in our source (%d%% covered)\n",
    $t, $m, int(100 * ($t - $m) / $t);
if (%orphan) {
    my $n = 0;
    $n += scalar @{$orphan{$_}} for keys %orphan;
    my $reachable = $t - $n;
    printf "       of which %d cannot be reached by anyone: ExifTool defines them and\n", $n;
    printf "       never points at them, so %d is the number a reader can get to.\n", $reachable;
}

# The names, not just the count: a number says how far there is to go, a list
# says what to do next.
if (%orphan) {
    my $n = 0;
    $n += scalar @{$orphan{$_}} for keys %orphan;
    printf "\nUNREACHABLE -- %d table(s) ExifTool defines and never points at:\n", $n;
    for my $mod (sort keys %orphan) {
        printf "  %-14s %s\n", $mod, join ' ', sort @{$orphan{$mod}};
    }
}

print "\nNOT REACHED:\n";
for my $mod (sort keys %missing) {
    printf "  %-14s %s\n", $mod, join ' ', sort @{$missing{$mod}};
}
