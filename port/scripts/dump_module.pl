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
use strict;
use warnings;
no warnings 'once';
use B ();
use B::Deparse ();

my ($lib, $module, $out) = @ARGV;
die "usage: $0 EXIFTOOL_LIB MODULE OUTFILE\n" unless defined $out;
unshift @INC, $lib;
$Image::ExifTool::configFile = '';
require Image::ExifTool;

my %before = %INC;
my @calls;
if ($module ne 'Image::ExifTool') {
    no strict 'refs';
    no warnings 'redefine';
    *Image::ExifTool::AddCompositeTags = sub ($;$) { push @calls, ['Image::ExifTool::AddCompositeTags', [@_]] };
    my $file = $module;
    $file =~ s{::}{/}g;
    require "$file.pm" if -e "$lib/$file.pm";
    require "$file.pl" if -e "$lib/$file.pl";
}
my @requires;
foreach my $f (sort keys %INC) {
    next if exists $before{$f};
    next unless $f =~ m{^Image/ExifTool/(.*)\.p[lm]$};
    my $m = "Image::ExifTool::$1";
    $m =~ s{/}{::}g;
    push @requires, $m unless $m eq $module;
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
    my $stash = \%{"${module}::"};
    foreach my $name (sort keys %$stash) {
        next if $name =~ /::$/ or $name =~ /^(BEGIN|END|INIT|CHECK|UNITCHECK|__ANON__|EXPORT|EXPORT_OK|EXPORT_TAGS|ISA|VERSION|AUTOLOAD|DESTROY|import)$/ and $name ne 'ISA';
        my $glob = $$stash{$name};
        next unless ref(\$glob) eq 'GLOB';
        my $full = "${module}::$name";
        push @vars, ['$', $full, ${*{$glob}{SCALAR}}] if defined ${*{$glob}{SCALAR}};
        push @vars, ['@', $full, *{$glob}{ARRAY}] if *{$glob}{ARRAY};
        push @vars, ['%', $full, *{$glob}{HASH}] if *{$glob}{HASH};
    }
}

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
my (%lex, @lexOrder);
{
    no strict 'refs';
    (my $path = $module) =~ s{::}{/}g;
    my $stash = \%{"${module}::"};
    foreach my $name (sort keys %$stash) {
        my $glob = $$stash{$name};
        next unless ref(\$glob) eq 'GLOB' and *{$glob}{CODE};
        my $cv = B::svref_2object(*{$glob}{CODE});
        next unless $cv->FILE =~ m{\Q$path\E\.p[lm]$} or $cv->FILE =~ m{/Write\w*\.pl$};
        my $padl = $cv->PADLIST;
        next unless $padl and $padl->can('ARRAY');
        my @pl = $padl->ARRAY;
        next unless @pl > 1 and $pl[0]->can('ARRAY') and $pl[1]->can('ARRAY');
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
