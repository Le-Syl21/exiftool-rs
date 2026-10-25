# Call ExifTool functions on a list of argument sets and print each result,
# for comparing a ported sub with its Perl original (run under perl and
# under the port, then diff).
#   perl -I EXIFTOOL_LIB scripts/fn_oracle.pl CASES
# CASES: one call per line, tab-separated: function name, then arguments.
# An argument "$et" is a new ExifTool object, "undef" is undef, "\N" a
# newline; anything else is a string.
use strict;
use Image::ExifTool;
my $et = Image::ExifTool->new;
open my $fh, '<', shift or die;
while (my $line = <$fh>) {
    chomp $line;
    next if $line eq '' or $line =~ /^#/;
    my ($func, @args) = split /\t/, $line, -1;
    @args = map { $_ eq '$et' ? $et : $_ eq 'undef' ? undef : do { (my $x = $_) =~ s/\\N/\n/g; $x } } @args;
    (my $mod = $func) =~ s/::[^:]+$//;
    (my $file = $mod) =~ s{::}{/}g;
    eval { require "$file.pm" } if $mod ne 'Image::ExifTool';
    no strict 'refs';
    my @r = eval { &$func(@args) };
    my $s = $@ ? 'DIE ' . (split / at /, $@)[0] : join(' | ', map { defined $_ ? (ref $_ ? ref $_ : $_) : 'undef' } @r);
    my $args = join(' , ', map { defined $_ ? (ref $_ ? ref $_ : $_) : 'undef' } @args);
    print "$func($args) => $s\n";
}
