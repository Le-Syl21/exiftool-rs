# Print everything ExifTool extracts from a file, for comparing perl and the
# port: every tag (duplicates and unknown ones included), with its groups,
# its printed value and its raw value. Runs under both.
#   perl -I EXIFTOOL_LIB scripts/dump_info.pl FILE
use strict;
use Image::ExifTool;
my $file = shift;
my %val;
foreach my $pc (1, 0) {
    my $et = Image::ExifTool->new;
    $et->Options(Duplicates => 1, Unknown => 2, PrintConv => $pc, Binary => 0);
    my $info = $et->ImageInfo($file);
    foreach my $tag ($et->GetTagList($info, 'File')) {
        my $v = $$info{$tag};
        if (ref $v eq 'SCALAR') {
            $v = '(binary ' . length($$v) . ')';
        } elsif (ref $v eq 'ARRAY') {
            $v = join(' // ', map { ref $_ eq 'SCALAR' ? '(binary)' : ref $_ ? '(' . ref($_) . ')' : $_ } @$v);
        } elsif (ref $v) {
            $v = '(' . ref($v) . ')';
        }
        $v = '' unless defined $v;
        $v =~ s/([\x00-\x1f\x7f])/sprintf('\\x%.2x', ord $1)/ge;
        my $g = join ':', map { defined $_ ? $_ : '' } ($et->GetGroup($tag))[0, 1, 2];
        push @{$val{$tag}}, ($pc ? 'P' : 'V') . " [$g] $tag = " . substr($v, 0, 500);
    }
}
foreach my $tag (sort keys %val) {
    print "$_\n" foreach @{$val{$tag}};
}
