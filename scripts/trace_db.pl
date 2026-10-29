# Perl side of the differential tracer: log every call of ExifTool's subs
# (name, arguments, result), in the format the port's PORT_TRACE writes.
#   PERL5DB='BEGIN { require "scripts/trace_db.pl" }' PORT_TRACE=out.txt perl -d script
package DB;
our ($sub, $single, $trace, $signal);
my $fh;
my $depth = 0;
BEGIN { open($fh, '>', $ENV{PORT_TRACE} || 'trace.txt') or die; binmode $fh; }
sub DB {}
sub summary {
    my $v = shift;
    return 'U' unless defined $v;
    if (ref $v) {
        my $r = ref $v;
        return "R:$r";
    }
    # (no regex here: it would change $1, which callers pass as arguments)
    my $s = "$v";
    my $more = length($s) > 40 ? '...#' . length($s) : '';
    $s = substr($s, 0, 40) if length $s > 40;
    my $o = '';
    for my $i (0 .. length($s) - 1) {
        my $c = ord(substr($s, $i, 1));
        $o .= ($c >= 0x20 and $c <= 0x7e and $c != 0x5c and $c != 0x7c) ? chr($c) : sprintf('\x%.2x', $c);
    }
    return "S:$o$more";
}
sub sub {
    my $name = ref $sub ? '' : $sub;
    unless ((index($name, 'Image::ExifTool') == 0 or index($name, 'File::RandomAccess') == 0) and index($name, '__ANON__') < 0) {
        no strict 'refs';
        return &$sub;
    }
    print $fh '  ' x $depth, "> $name ", join('|', map { summary($_) } @_), "\n";
    ++$depth;
    no strict 'refs';
    if (wantarray) {
        my @r = &$sub;
        --$depth;
        print $fh '  ' x $depth, "< ", join('|', map { summary($_) } @r), "\n";
        return @r;
    } elsif (defined wantarray) {
        my $r = &$sub;
        --$depth;
        print $fh '  ' x $depth, "< ", summary($r), "\n";
        return $r;
    } else {
        &$sub;
        --$depth;
        print $fh '  ' x $depth, "< void\n";
        return;
    }
}
1;
