#!/usr/bin/perl
# Dump the package variables of one ExifTool module, as the module leaves
# them once loaded, for the Rust runtime to load back.
#
#   perl scripts/dump_module.pl EXIFTOOL_LIB MODULE OUTFILE
#
# MODULE is "Image::ExifTool" for ExifTool.pm itself, or a module name such
# as "Image::ExifTool::BMP". The module is loaded in a fresh process with
# Image::ExifTool already loaded (no config file). Its calls to
# AddCompositeTags are recorded instead of run, so its tables are dumped as
# its source defines them; the runtime replays the calls.
#
# Output format (all integers are LEB128 varints):
#   "PLD1"
#   requires: count, then module names loaded by this one
#   calls:    count, then (function name, argument count, argument nodes)
#   vars:     count, then (sigil byte, full name, node)
#   lexicals: count, then (sigil byte, name, node): the file's `my`
#             variables, found through the subs that use them
# Nodes:
#   0 undef | 1 int (zigzag) | 11 uint | 2 float (8 bytes LE) | 3 string
#   4 array: id, count, nodes | 5 hash: id, class, count, (key, node)...
#   6 scalar ref: id, node | 7 back reference: id
#   8 named code: full name | 9 anonymous code: id, name, deparsed source
#   10 global: sigil byte, full name (a package variable of another module)
#   12 regexp: pattern | 13 other reference: description
#   14 glob reference: *PACKAGE::NAME
use strict;
use warnings;
no warnings 'once';
use B ();
use B::Deparse ();

my ($lib, $module, $out) = @ARGV;
die "usage: $0 EXIFTOOL_LIB MODULE OUTFILE\n" unless defined $out;
unshift @INC, $lib;
$Image::ExifTool::configFile = '';
# the variables before the module is loaded: those it creates in other
# packages (ExifTool.pm's %Image::ExifTool::JFIF::Main...) are its too
my %varsBefore;
%varsBefore = var_names() if $module eq 'Image::ExifTool';
require Image::ExifTool;

# A .pl file (Writer.pl, WriteExif.pl, XMP2.pl...) is code of another
# package that its module loads on demand: load that module first, as
# perl does, then dump what the file adds (the variables it creates).
my ($plFile, $pkg) = (undef, $module);
{
    (my $file = $module) =~ s{::}{/}g;
    if ($module ne 'Image::ExifTool' and not -e "$lib/$file.pm" and -e "$lib/$file.pl") {
        $plFile = "$file.pl";
        open my $src, '<', "$lib/$plFile" or die "$plFile: $!\n";
        while (<$src>) { if (/^package\s+([\w:]+)\s*;/) { $pkg = $1; last } }
        close $src;
        (my $parent = $pkg) =~ s{::}{/}g;
        require "$parent.pm" if $pkg ne 'Image::ExifTool' and -e "$lib/$parent.pm";
    }
}
sub var_names {
    no strict 'refs';
    my %v;
    foreach my $p (all_packages('Image::ExifTool')) {
        foreach my $name (keys %{"${p}::"}) {
            next if $name =~ /::$/;
            my $glob = ${"${p}::"}{$name};
            next unless ref(\$glob) eq 'GLOB';
            $v{"*${p}::$name"} = 1;
            $v{"\$${p}::$name"} = 1 if defined ${*{$glob}{SCALAR}};
            # (an array or hash counts once it has content: a mention of
            # %Image::ExifTool::JPEG::Main elsewhere creates it empty)
            $v{"\@${p}::$name"} = 1 if *{$glob}{ARRAY} and @{*{$glob}{ARRAY}};
            $v{"%${p}::$name"} = 1 if *{$glob}{HASH} and %{*{$glob}{HASH}};
        }
    }
    return %v;
}
%varsBefore = var_names() unless $module eq 'Image::ExifTool';
# the subs before the module is loaded: those it imports (`use Image::ExifTool
# qw(:Utils)`, Writer.pl's FindTagInfo...) are replayed as aliases
my %codeBefore;
sub code_names {
    no strict 'refs';
    my %c;
    foreach my $p (all_packages('Image::ExifTool')) {
        foreach my $name (keys %{"${p}::"}) {
            next if $name =~ /::$/;
            my $glob = ${"${p}::"}{$name};
            next unless ref(\$glob) eq 'GLOB' and *{$glob}{CODE};
            $c{"${p}::$name"} = *{$glob}{CODE};
        }
    }
    return %c;
}
%codeBefore = code_names();
my %before = %INC;
my @calls;
if ($module ne 'Image::ExifTool') {
    no strict 'refs';
    no warnings 'redefine';
    # (only the module's own calls: those of the modules it loads are in
    # their own dumps)
    (my $own = $module) =~ s{::}{/}g;
    *Image::ExifTool::AddCompositeTags = sub ($;$) {
        push @calls, ['Image::ExifTool::AddCompositeTags', [@_]] if (caller(0))[1] =~ m{\Q$own\E\.p[lm]$};
    };
    # a tag the module adds to another module's table (Validate.pm adds
    # Validate to ExifTool's Extra table): replayed too, and done here
    my $addTag = \&Image::ExifTool::AddTagToTable;
    *Image::ExifTool::AddTagToTable = sub ($$;$$) {
        push @calls, ['Image::ExifTool::AddTagToTable', [@_]] if (caller(0))[1] =~ m{\Q$own\E\.p[lm]$};
        return $addTag->(@_);
    };
    my $file = $module;
    $file =~ s{::}{/}g;
    require "$file.pm" if -e "$lib/$file.pm";
    require "$file.pl" if -e "$lib/$file.pl";
}
my @requires;
push @requires, $pkg if $plFile and $pkg ne 'Image::ExifTool';
foreach my $f (sort keys %INC) {
    next if exists $before{$f};
    next unless $f =~ m{^Image/ExifTool/(.*)\.p[lm]$};
    my $m = "Image::ExifTool::$1";
    $m =~ s{/}{::}g;
    push @requires, $m unless $m eq $module or $m eq $pkg;
}
# the subs this load imported: a sub whose own name is another one
{
    my %after = code_names();
    my @pairs;
    foreach my $target (sort keys %after) {
        next if $codeBefore{$target};
        my $gv = B::svref_2object($after{$target})->GV;
        next unless $gv and $gv->can('STASH');
        my $source = $gv->STASH->NAME . '::' . $gv->NAME;
        next if $source eq $target or $source =~ /__ANON__/;
        push @pairs, $target, $source;
    }
    push @calls, ['Internals::PldImport', \@pairs] if @pairs;
}
# module-level code that depends on the run (config file, user-defined
# tags, queued plug-in tags...): the port runs it once the dump is loaded
my %initCall = (
    'Image::ExifTool'              => 'Image::ExifTool::InitConfig',
    'Image::ExifTool::TagLookup'   => 'Image::ExifTool::TagLookup::InitTagLookup',
    'Image::ExifTool::Shortcuts'   => 'Image::ExifTool::Shortcuts::InitShortcuts',
    'Image::ExifTool::Geolocation' => 'Image::ExifTool::Geolocation::InitGeolocation',
);
# the variables a .pl file created (not those of the modules it loaded,
# which have their own dumps)
my %plVars;
{
    my %after = var_names();
    my %others = map { $_ => 1 } @requires;
    foreach my $v (keys %after) {
        next if $varsBefore{$v} or $v =~ /^\*/;
        # a scalar is the file's only if its name is new (an existing
        # variable that was still undefined, like $swapBytes, is not)
        next if $v =~ /^\$/ and $varsBefore{'*' . substr($v, 1)};
        (my $p = substr($v, 1)) =~ s/::\w+$//;
        next if $others{$p} and $p ne $pkg;
        # a .pm module's own package is dumped whole below
        next if not $plFile and $p eq $module;
        $plVars{$v} = 1;
    }
}

# File lexicals: the file-scope `my` variables the module's code uses,
# found in the pads of its subs (flagged OUTER) and of the anonymous subs
# in its tables (ASF's RawConv sub reads `my %streamType`)
my (%lex, @lexOrder);
sub pad_outer {
    my $cv = shift;
    my $padl = $cv->PADLIST;
    return unless $padl and $padl->can('ARRAY');
    my @pl = $padl->ARRAY;
    return unless @pl > 1 and $pl[0]->can('ARRAY') and $pl[1]->can('ARRAY');
    my @n = $pl[0]->ARRAY;
    my @v = $pl[1]->ARRAY;
    for my $i (0..$#n) {
        my $nm = $n[$i];
        next unless $nm->can('PV') and defined $nm->PV and $nm->PV =~ /^[\$\@%]\w+$/;
        next unless $nm->FLAGS & B::PADNAMEf_OUTER();
        next if $lex{$nm->PV};
        $lex{$nm->PV} = $v[$i]->object_2svref;
        push @lexOrder, $nm->PV;
    }
}
my $deparse = B::Deparse->new('-sC');
my %id;        # refaddr -> node id
my $next = 0;
my $buf = '';

# Package variables of other packages, found by reference, are dumped as
# references to them, so loading a module never duplicates another one.
my %global;    # refaddr -> [sigil, name]
my %ownVars;
sub index_package {
    my ($pkg, $own) = @_;
    no strict 'refs';
    my $stash = \%{"${pkg}::"};
    foreach my $name (keys %$stash) {
        next if $name =~ /::$/;
        my $glob = $$stash{$name};
        next unless ref(\$glob) eq 'GLOB';
        my $full = "${pkg}::$name";
        if (my $h = *{$glob}{HASH}) { $global{0 + $h} = ['%', $full] unless $own; }
        if (my $a = *{$glob}{ARRAY}) { $global{0 + $a} = ['@', $full] unless $own; }
    }
}
sub all_packages {
    my ($pkg) = @_;
    no strict 'refs';
    my @p = ($pkg);
    foreach my $name (keys %{"${pkg}::"}) {
        next unless $name =~ /^(\w+)::$/;
        push @p, all_packages("${pkg}::$1");
    }
    return @p;
}
foreach my $p (all_packages('Image::ExifTool')) {
    index_package($p, $p eq $module);
}
# a .pl file's own variables are dumped, not referred to
foreach my $v (keys %plVars) {
    no strict 'refs';
    my ($sigil, $full) = (substr($v, 0, 1), substr($v, 1));
    delete $global{0 + \%{$full}} if $sigil eq '%';
    delete $global{0 + \@{$full}} if $sigil eq '@';
}

sub varint {
    my $n = shift;
    my $s = '';
    do {
        my $b = $n & 0x7f;
        $n >>= 7;
        $b |= 0x80 if $n;
        $s .= chr($b);
    } while ($n);
    return $s;
}
sub str { my $s = shift; utf8::encode($s) if utf8::is_utf8($s); return varint(length $s) . $s; }

sub node {
    my ($v, $top) = @_;
    unless (defined $v) { $buf .= "\0"; return; }
    if (my $r = ref $v) {
        my $addr = 0 + $v;
        if (!$top and $global{$addr}) {
            my ($sigil, $name) = @{$global{$addr}};
            $buf .= chr(10) . $sigil . str($name);
            return;
        }
        if (defined $id{$addr}) { $buf .= chr(7) . varint($id{$addr}); return; }
        my $class = Scalar::Util::blessed($v);
        my $type = Scalar::Util::reftype($v);
        if ($type eq 'CODE') {
            my $cv = B::svref_2object($v);
            my $gv = $cv->GV;
            my $name = ($gv and $gv->can('NAME')) ? $gv->STASH->NAME . '::' . $gv->NAME : '__ANON__';
            if ($name !~ /__ANON__$/) {
                $buf .= chr(8) . str($name);
                return;
            }
            # (only the module's own subs: another file's lexicals are not
            # this module's)
            (my $own = $plFile // "$module.pm") =~ s{::}{/}g;
            pad_outer($cv) if $cv->FILE =~ m{\Q$own\E$};
            my $src = eval { $deparse->coderef2text($v) };
            $src = "{ die 'deparse failed' }" unless defined $src;
            $id{$addr} = $next++;
            $buf .= chr(9) . varint($id{$addr}) . str($name) . str("sub $src");
            return;
        }
        if ($type eq 'REGEXP' or ($class and $class eq 'Regexp')) {
            $buf .= chr(12) . str("$v");
            return;
        }
        $id{$addr} = $next++;
        if ($type eq 'ARRAY') {
            $buf .= chr(4) . varint($id{$addr}) . varint(scalar @$v);
            node($_) foreach @$v;
        } elsif ($type eq 'HASH') {
            $buf .= chr(5) . varint($id{$addr}) . str(defined $class ? $class : '') . varint(scalar keys %$v);
            foreach my $k (sort keys %$v) {
                $buf .= str($k);
                node($$v{$k});
            }
        } elsif ($type eq 'SCALAR' or $type eq 'REF') {
            $buf .= chr(6) . varint($id{$addr});
            node($$v);
        } elsif ($type eq 'GLOB') {
            # \*STDOUT (the TextOut option's default): the handle, by name
            my $gv = B::svref_2object($v);
            $buf .= chr(14) . str('*' . $gv->STASH->NAME . '::' . $gv->NAME);
        } else {
            $buf .= chr(13) . str("$type");
        }
        return;
    }
    # A plain scalar: keep perl's own idea of its type.
    my $sv = B::svref_2object(\$v);
    my $flags = $sv->FLAGS;
    if ($flags & B::SVf_POK) {
        $buf .= chr(3) . str($v);
    } elsif ($flags & B::SVf_IOK) {
        if ($flags & B::SVf_IVisUV) {
            $buf .= chr(11) . varint($v);
        } else {
            my $i = $v;
            $buf .= chr(1) . varint($i >= 0 ? $i * 2 : -$i * 2 - 1);
        }
    } elsif ($flags & B::SVf_NOK) {
        $buf .= chr(2) . pack('d<', $v);
    } else {
        $buf .= chr(3) . str("$v");
    }
}

require Scalar::Util;
my @vars;
{
    no strict 'refs';
    foreach my $v (sort keys %plVars) {
        my ($sigil, $full) = (substr($v, 0, 1), substr($v, 1));
        push @vars, ['$', $full, ${$full}] if $sigil eq '$';
        push @vars, ['@', $full, \@{$full}] if $sigil eq '@';
        push @vars, ['%', $full, \%{$full}] if $sigil eq '%';
    }
    # WriteQuickTime.pl's top-level code changes QuickTime's own tables
    # (marks the Preferred tags): the port replays it after loading
    push @calls, ['Image::ExifTool::QuickTime::InitWriteQuickTime', []] if $module eq 'Image::ExifTool::WriteQuickTime';
}
unless ($plFile) {
    no strict 'refs';
    my $stash = \%{"${module}::"};
    foreach my $name (sort keys %$stash) {
        # (the directories the dump found are this machine's: InitGeolocation
        # sets them at run time)
        next if $module eq 'Image::ExifTool::Geolocation' and $name =~ /^(geoDir|altDir)$/;
        next if $name =~ /::$/ or $name =~ /^(BEGIN|END|INIT|CHECK|UNITCHECK|__ANON__|ISA|AUTOLOAD|DESTROY|import)$/ and $name ne 'ISA';
        my $glob = $$stash{$name};
        next unless ref(\$glob) eq 'GLOB';
        my $full = "${module}::$name";
        # (not what another module put in this package before it loaded,
        # as ExifTool.pm's %Image::ExifTool::JPEG::SOF: that one's dump has it)
        push @vars, ['$', $full, ${*{$glob}{SCALAR}}] if defined ${*{$glob}{SCALAR}} and not $varsBefore{"\$$full"};
        push @vars, ['@', $full, *{$glob}{ARRAY}] if *{$glob}{ARRAY} and not $varsBefore{"\@$full"};
        push @vars, ['%', $full, *{$glob}{HASH}] if *{$glob}{HASH} and not $varsBefore{"%$full"};
    }
    # a Charset module defines its table in the parent package:
    # %Image::ExifTool::Charset::Latin in Charset/Latin.pm
    if ($module =~ /^(Image::ExifTool::Charset)::(\w+)$/) {
        my $full = "$1::$2";
        push @vars, ['%', $full, \%{$full}] if %{$full};
    }
}

push @calls, [$initCall{$module}, []] if $initCall{$module};
$buf = 'PLD1' . varint(scalar @requires);
$buf .= str($_) foreach @requires;
$buf .= varint(scalar @calls);
foreach my $c (@calls) {
    $buf .= str($$c[0]) . varint(scalar @{$$c[1]});
    node($_) foreach @{$$c[1]};
}
$buf .= varint(scalar @vars);
foreach my $v (@vars) {
    my ($sigil, $full, $val) = @$v;
    $buf .= $sigil . str($full);
    # Arrays and hashes are dumped by content (they are the variable).
    if ($sigil eq '$') {
        node($val, 1);
    } else {
        node($val, 1);
    }
}
# File lexicals: each named sub of the module keeps the file-scope `my`
# variables it uses in its pad, flagged OUTER.
{
    no strict 'refs';
    (my $path = $module) =~ s{::}{/}g;
    # (a .pl file's subs are in the package it declares)
    my $stash = \%{"${pkg}::"};
    foreach my $name (sort keys %$stash) {
        my $glob = $$stash{$name};
        next unless ref(\$glob) eq 'GLOB' and *{$glob}{CODE};
        my $cv = B::svref_2object(*{$glob}{CODE});
        if ($plFile) {
            next unless $cv->FILE =~ m{\Q$plFile\E$};
        } else {
            next unless $cv->FILE =~ m{\Q$path\E\.p[lm]$} or $cv->FILE =~ m{/Write\w*\.pl$};
        }
        pad_outer($cv);
    }
}
$buf .= varint(scalar @lexOrder);
foreach my $n (@lexOrder) {
    my $r = $lex{$n};
    my $sigil = substr($n, 0, 1);
    $buf .= $sigil . str(substr($n, 1));
    node($sigil eq '$' ? $$r : $r, 1);
}
open my $fh, '>:raw', $out or die "$out: $!\n";
print $fh $buf;
close $fh;
