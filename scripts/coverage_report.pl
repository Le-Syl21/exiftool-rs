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

# `--check` makes this a gate rather than a report: it exits non-zero unless
# every table a reader can get to is reached, so a release cannot quietly lose
# one.
my $check = grep { $_ eq '--check' } @ARGV;
my @paths = grep { !/^--/ } @ARGV;
my $lib = $paths[0] || '../exiftool/lib';
my $src = $paths[1] || 'src';
die "Cannot find $lib\n" unless -d $lib;

# Everything our crate mentions, in one blob.
# Our source, file by file. Which ExifTool module a file decodes matters:
# a file whose header names `Sony.pm` may call its tables `Tag9050b` without
# repeating the module, and requiring the qualified form everywhere would
# report those as unreached when they are decoded.
my %ours_by_file;
my $ours = '';
open my $find, '-|', 'find', $src, '-name', '*.rs' or die $!;
while (my $f = <$find>) {
    chomp $f;
    open my $h, '<', $f or next;
    local $/;
    my $body = <$h>;
    close $h;
    $ours .= $body;
    $ours_by_file{$f} = $body;
}
close $find;

# The modules a file declares it decodes, from `Image::ExifTool::Mod` or
# `Mod.pm` anywhere in it.
# A file's HEADER says which module it decodes -- the body is full of
# `Image::ExifTool::Exif::PrintFNumber` and the like, which say nothing about
# the file. Only a header naming exactly one `<Module>.pm` makes a file that
# module's, and only then may its tables be named bare.
my %file_module;
for my $f (keys %ours_by_file) {
    my @head = (split /\n/, $ours_by_file{$f})[0 .. 19];
    my %m;
    for my $line (grep { defined } @head) {
        $m{$1} = 1 while $line =~ /\b(\w+)\.pm\b/g;
    }
    $file_module{$f} = (keys %m) == 1 ? (keys %m)[0] : undef;
}

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

# Our source with the generated dispatcher's own key lines removed, so a
# table counts only where a READER names it.
my $gen = 'src/tags/binary_tables_generated.rs';
my $outside = '';
for my $f (keys %ours_by_file) {
    next if $f =~ /binary_tables_generated\.rs$/;
    $outside .= $ours_by_file{$f};
}

# Which generated decoder calls which, so a table opened as a sub-directory by
# a reached one counts as reached too -- that is how the NikonCustom settings
# are decoded, from inside their ShotInfo.
my (%fn_of_table, %table_of_fn, %calls, %called_from_reached);
if (open my $h, '<', $gen) {
    local $/;
    my $body = <$h>;
    close $h;
    # The decode dispatcher's own arms, not `table_byte_order`'s: both are
    # keyed by the same strings, and matching either left every table mapped
    # to the function `Some`.
    # Written whitespace-insensitively: rustfmt reflows the longer arms into
    # a block and breaks the argument list, and a regex that assumed one line
    # silently lost every table whose name was long enough to wrap.
    while ($body =~ /"(\w+)::(\w+)" => \{?\s*(\w+)\(\s*data\s*,/g) {
        $fn_of_table{"$1\::$2"} = $3;
        $table_of_fn{$3} = "$1\::$2";
    }
    # Every call one decoder makes to another. Read from a copy: a `//g` loop
    # leaves its position on the scalar, and the next loop over the same one
    # would start where this ended.
    my $bodies = $body;
    while ($bodies =~ /^fn (\w+)\(.*?\n\}/gms) {
        my $whole = $&;
        my $from = $1;
        while ($whole =~ /\b(\w+)\(\s*sub\s*,/gs) {
            push @{$calls{$from}}, $1;
        }
    }
    # A reader can also name a Main-table id and let the generated selector
    # answer with the table -- that is how ColorData, CameraInfo and ShotInfo
    # are chosen -- so every table an arm of a selector the reader asks for
    # can return is reached as well.
    my %selector_tables;
    while ($body =~ /\("(\w+)", (0x[0-9a-fA-F]+)\) => \{(.*?)\n        \}/gs) {
        my ($mod, $id, $arms) = ($1, $2, $3);
        push @{$selector_tables{"$mod\t$id"}}, $arms =~ /Some\("(\w+::\w+)"\)/g;
    }

    # Seed: every table a reader names, then close over the call graph.
    my @queue;
    for my $t (keys %fn_of_table) {
        push @queue, $t if $outside =~ /\Q$t\E/;
    }
    for my $k (keys %selector_tables) {
        my ($mod, $id) = split /\t/, $k;
        my $dec = hex $id;
        # `variant_for("Canon", 0x4001` or the same id written any other way.
        # The call is written over several lines, the module on one and the
        # id on the next.
        my $asked = 0;
        while ($outside =~ /variant_for\s*\(\s*"\Q$mod\E"\s*,\s*(0x[0-9a-fA-F]+|\d+)\s*,/gs) {
            my $got = $1;
            $asked = 1 if ($got =~ /^0x/ ? hex($got) : $got) == $dec;
        }
        # Or the id comes from the tag being read, in which case every arm
        # the reader lists beside it counts.
        $asked = 1 if !$asked and $outside =~ /variant_for\s*\(\s*"\Q$mod\E"\s*,\s*tag_id\s*,/s;
        next unless $asked;
        for my $t (@{$selector_tables{$k}}) {
            $called_from_reached{$t} = 1;
            push @queue, $t;
        }
    }
    my %seen;
    while (my $t = shift @queue) {
        next if $seen{$t}++;
        my $fn = $fn_of_table{$t} or next;
        for my $callee (@{$calls{$fn} || []}) {
            my $ct = $table_of_fn{$callee} or next;
            next if $seen{$ct};
            $called_from_reached{$ct} = 1;
            push @queue, $ct;
        }
    }
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
        # Named the way a reader has to name it -- `Module::Table`, the key
        # our own dispatcher uses -- or on a line that names the module too.
        # A bare substring is not a mention: "Thumbnail" occurs in thousands
        # of places, and matching it counted Samsung::Thumbnail as reached
        # when nothing decodes it.
        # Named OUTSIDE the generated dispatcher. A generated decoder that
        # nothing calls is not reachable: its own `"Mod::Table" => mod_table()`
        # line would otherwise be the only evidence, and generating a table
        # would be enough to count it.
        my $reached = $outside =~ /\Q$module\E::\Q$table\E/;
        # Or opened as a sub-directory by a table that is itself reached.
        $reached ||= $called_from_reached{"$module\::$table"};
        unless ($reached) {
            # Or named on its own in a file that says which module it decodes.
            for my $f (keys %ours_by_file) {
                next unless defined $file_module{$f} and $file_module{$f} eq $module;
                next unless $ours_by_file{$f} =~ /\b\Q$table\E\b/;
                $reached = 1;
                last;
            }
        }
        push @{$missing{$module}}, $table unless $reached;
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

if ($check) {
    # A table nothing points at is not something a reader can fail to reach,
    # so it is not counted against this gate -- it is named above instead.
    my $orphans = 0;
    $orphans += scalar @{$orphan{$_}} for keys %orphan;
    my $unreached = 0;
    for my $mod (keys %missing) {
        for my $t (@{$missing{$mod}}) {
            $unreached++ unless grep { $_ eq $t } @{$orphan{$mod} || []};
        }
    }
    my $reachable = $t - $orphans;
    print "\n";
    if ($unreached == 0) {
        printf "COUNTER THREE OK — %d / %d reachable sub-tables are reached.\n",
            $reachable, $reachable;
    } else {
        printf "COUNTER THREE FAILED — %d of %d reachable sub-tables are not reached.\n",
            $unreached, $reachable;
        exit 1;
    }
}
