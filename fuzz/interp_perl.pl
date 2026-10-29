#!/usr/bin/perl
# Run an input of the `interp` target in perl, as the target runs it in the
# port: the first byte picks the program (from interp_programs.rs), the
# rest is split on 0xFF into $a, $b and $c. Prints $@ (the die, if any), to
# compare with the port's.
#
# usage: perl interp_perl.pl INPUT
use strict;
use warnings;
use FindBin;

open my $fh, '<', "$FindBin::Bin/interp_programs.rs" or die $!;
my $src = do { local $/; <$fh> };
my @programs = $src =~ /r#"(.*?)"#/sg;
open my $in, '<:raw', $ARGV[0] or die $!;
my $data = do { local $/; <$in> };
my $pick = ord substr($data, 0, 1, '');
my $program = $programs[$pick % @programs];
my @parts = split /\xff/, $data, 3;
no strict;
no warnings;
my ($a, $b, $c) = map { defined $_ ? $_ : '' } @parts[0..2];
print "program ", $pick % @programs, "\n";
eval "eval { $program }; print \"die: \$\@\"; 1" or print "compile: $@";
print "\n";
