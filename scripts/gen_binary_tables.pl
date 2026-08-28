#!/usr/bin/perl
# Decoders for ExifTool's binary sub-tables, whatever maker they belong to.
#
# A binary sub-table is a block of bytes addressed by index: ExifTool's
# ProcessBinaryData reads the entry at `($index - FIRST_ENTRY) * sizeof(FORMAT)`
# and a field's own Format says what to read there. This generator turns those
# tables into Rust decoders straight from the Perl, so a table is reached
# because ExifTool defines it and not because someone transcribed it.
#
# It is the maker-agnostic companion to gen_sony_ciphered.pl, which does the
# same for Sony's enciphered blocks and carries the Sony-only machinery --
# the cipher, the Main-table variant selector -- that has no equivalent here.
#
# Everything it cannot express is named on stderr. Nothing is skipped quietly.
#
# Usage: perl scripts/gen_binary_tables.pl /path/to/exiftool/lib > src/tags/binary_tables_generated.rs

use strict;
use warnings;

my $lib = $ARGV[0] || '../exiftool/lib';
die "Cannot find $lib\n" unless -d $lib;
# The modules are loaded as well as read: `PrintConv => \%canonLensTypes` is a
# reference to a hash built at run time, and there is no reading it off the
# source. Loading it is how gen_print_conv.pl reaches the same tables.
unshift @INC, $lib;

# Which tables to emit, by module. A table named here pulls in the tables its
# own fields point at, so the list is the entry points rather than the closure.
my %WANTED = (
    Canon => [qw(
        ColorData1 ColorData2 ColorData3 ColorData4 ColorData5 ColorData6
        ColorData7 ColorData8 ColorData9 ColorData10 ColorData11 ColorData12
        ColorDataUnknown
        ShotInfo RawBurstInfo IAD1
        Panorama UnknownD30 MovieInfo MyColors FaceDetect1 FaceDetect2
        FaceDetect3 WBInfo TimeInfo SerialInfo MeasuredColor Flags
        ModifiedInfo PreviewImageInfo AFMicroAdj VignettingCorr
        VignettingCorr2 VignettingCorrUnknown LightingOpt Ambience MultiExp
        HDRInfo LogInfo AFConfig FocusBracketingInfo LevelInfo FocalInfo
    )],
    CanonCustom => [qw(PersonalFuncs PersonalFuncValues)],
    CanonVRD => [qw(DustInfo DLOInfo)],
    Kodak => [qw(Type9 Type2 Type3 Type4 Type5 Type6 Type7 Processing Unknown MOV)],
    Panasonic => [qw(Data1 Data2 FaceDetInfo FaceRecInfo FocusInfo SerialInfo
                     ShotInfo TimeInfo Type2)],
    PanasonicRaw => [qw(DistortionInfo WBInfo WBInfo2)],
    Casio => [qw(FaceInfo1 FaceInfo2 QVCI AVI)],
    Ricoh => [qw(FaceInfo AVI Subdir)],
    Sanyo => [qw(FaceInfo Thumbnail)],
    Reconyx => [qw(HyperFire HyperFire2 HyperFire4K MicroFire UltraFire)],
    H264 => [qw(Camera1 Camera2 FrameInfo MakeModel RecInfo)],
    Nintendo => [qw(CameraInfo)],
    Microsoft => [qw(Stitch)],
    FlashPix => [qw(PreviewInfo)],
    JPEG => [qw(Ocad)],
    CanonRaw => [qw(WhiteSample)],
    Pentax => [qw(Junk2 CAFPointInfo)],
    Nikon => [qw(LensDataUnknown)],
    NikonCustom => [qw(
        SettingsD40 SettingsD810 SettingsD850
        SettingsZ6III SettingsZ8 SettingsZ9 SettingsZ9v4
    )],
    Parrot => [qw(
        ARCoreAccel ARCoreAccel0 ARCoreGyro ARCoreGyro0 ARCoreVideo ARCoreCustom
    )],
    NikonCapture => [qw(
        DLightingHQ DLightingHS HighlightData PictureCtrl UnsharpData WBAdjData
        Brightness ColorBoost Exposure
    )],
    Photoshop => [qw(PixelInfo)],
    RIFF => [qw(AVIHeader)],
    Samsung => [qw(DualShotExtra Thumbnail2 Thumbnail OrientationInfo PictureWizard)],
    Minolta => [qw(
        CameraSettings7D CameraInfoA100 WBInfoA100 MOV1 MOV2
        CameraSettings5D CameraSettingsA100 ISInfoA100
    )],
    Olympus => [qw(MOV1 MOV2 prms AFInfo AFTargetInfo MovableInfo
                   SubjectDetectInfo Thumbnail AVI MP4 WAV
                   FocusInfo CameraSettings)],
    FujiFilm => [qw(FFMV RAFData MOV)],
    QuickTime => [qw(HTCBinary AV1Config ContentLightLevel Flip HEVCConfig)],
    FLIR => [qw(
        GainDeadData CoarseData PaintData SerialNums UnknownUUID GPS_UUID
        AFF1 AFF5 EmbeddedImage GPSInfo Header MeterLink MoreInfo Params PiP
    )],
);

# Main-table ids whose sub-table is chosen by a chain of conditions. The arms
# are read from the module's own Main table, so the choice is ExifTool's.
my %SELECTORS = (
    Canon => [0x000d, 0x4001, 0x0096, 0x4015],
    Nikon => [0x0091],
);

my %WIDTH = (
    int8u => 1, int8s => 1,
    int16u => 2, int16s => 2,
    int32u => 4, int32s => 4,
    rational32u => 4, rational32s => 4,
    # The eight-byte rational EXIF writes: two 32-bit halves.
    rational64u => 8, rational64s => 8,
    # ExifTool's `int16uRev` is a 16-bit value stored the other way round from
    # the rest of the block -- Canon writes LensType and ColorTemperature that
    # way inside a little-endian file.
    int16uRev => 2,
    float => 4, double => 8,
    # A 32-bit fixed-point value: the number divided by 65536.
    fixed32u => 4, fixed32s => 4,
);
# The modules whose tables belong to a maker note; the rest name themselves in
# family 0 (ExifTool sets it on the module's own table definitions).
my %MAKERNOTE_MODULE = map { $_ => 1 } qw(
    Canon CanonCustom CanonVRD Casio DJI FLIR FujiFilm GE HP JVC Kodak Leaf
    Minolta Motorola Nikon NikonCapture NikonCustom Olympus Panasonic Pentax
    PhaseOne Reconyx Ricoh Samsung Sanyo Sigma Sony
);

my %IS_RATIONAL = (rational32u => 1, rational32s => 1, rational64u => 1, rational64s => 1);

my @skipped;
my @tables;      # emitted, in order
my %by_name;     # name -> table
my %pending;     # tables still to read, per module

sub note { push @skipped, $_[0] }

# ---------------------------------------------------------------- reading
sub read_module {
    my ($module) = @_;
    open my $h, '<', "$lib/Image/ExifTool/$module.pm" or die "$module.pm: $!\n";
    local $/;
    my $src = <$h>;
    close $h;
    return $src;
}

# The `my %name = (...)` hashes a table splices whole fields out of.
sub shared_hashes {
    my ($src) = @_;
    my %shared;
    # The opening bracket has to end its line: without that, a one-line
    # `my %offOn = ( 0 => 'Off', 1 => 'On' );` matched as the start of a
    # multi-line hash and swallowed everything up to the next `\n);` --
    # every hash between them, and their entries with them.
    while ($src =~ /^my %(\w+)\s*=\s*\([ \t]*(?:#[^\n]*)?\n(.*?)\n\);/gms) {
        $shared{$1} = $2;
    }
    # `my %offOn = ( 0 => 'Off', 1 => 'On' );` on one line.
    while ($src =~ /^my %(\w+)\s*=\s*\(([^()\n]*)\);\s*$/gm) {
        $shared{$1} = $2;
    }
    return %shared;
}

# One of a module's package hashes, loaded rather than read.
my %loaded;
sub named_hash {
    my ($module, $name) = @_;
    unless ($loaded{$module}) {
        eval "require Image::ExifTool::$module; 1" or return undef;
        $loaded{$module} = 1;
    }
    no strict 'refs';
    my $ref = \%{"Image::ExifTool::${module}::${name}"};
    use strict 'refs';
    return %$ref ? $ref : undef;
}

sub table_body {
    my ($src, $module, $table) = @_;
    return $1 if $src =~ /^%Image::ExifTool::\Q$module\E::\Q$table\E\s*=\s*\((.*?)\n\);/ms;
    return undef;
}

# A field body, by counting braces from the line that opens it.
sub scan_fields {
    my ($body) = @_;
    my @out;
    my @lines = split /\n/, $body;
    for (my $i = 0; $i <= $#lines; ++$i) {
        # `0x43 => 'ColorTempAsShot'`: a name and nothing else.
        if ($lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\s*=>\s*'([^']+)'\s*,/) {
            push @out, [$1, "Name => '$2',", 0];
            next;
        }
        # `0x47 => [{...},{...}]`: alternatives, the first whose condition
        # holds being the one ExifTool takes.
        if ($lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\s*=>\s*\[/) {
            my $off_s = $1;
            my ($depth, $text, $seen) = (0, '', 0);
            for (my $j = $i; $j <= $#lines; ++$j) {
                $text .= $lines[$j] . "\n";
                while ($lines[$j] =~ /\{/g) { $depth++; $seen = 1 }
                $depth-- while $lines[$j] =~ /\}/g;
                if ($seen and $depth <= 0 and $lines[$j] =~ /\]/) { $i = $j; last }
            }
            my (@arms, $cur);
            my $d = 0;
            for my $c (split //, $text) {
                if ($c eq '{') { $d++; $cur = '' unless defined $cur }
                $cur .= $c if defined $cur;
                if ($c eq '}') {
                    $d--;
                    if ($d == 0 and defined $cur) { push @arms, $cur; undef $cur }
                }
            }
            push @out, [$off_s, $_, 1] for @arms;
            next;
        }
        next unless $lines[$i] =~ /^\s{4}(0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\s*=>\s*\{/;
        my $off_s = $1;
        my ($depth, $text) = (0, '');
        for (my $j = $i; $j <= $#lines; ++$j) {
            $text .= $lines[$j] . "\n";
            $depth++ while $lines[$j] =~ /\{/g;
            $depth-- while $lines[$j] =~ /\}/g;
            if ($depth <= 0) { $i = $j; last }
        }
        push @out, [$off_s, $text, 0];
    }
    return @out;
}

# The source of a conversion, whichever way it is written: `'...'`, `"..."`,
# or `q{...}` taken by counting braces. Reading only the first of those left
# MaxDataRate without its conversion and said nothing about it.
sub code_of {
    my ($fb, $key) = @_;
    my $best;
    my $at = -1;
    while ($fb =~ /\b\Q$key\E\s*=>\s*'((?:[^'\\]|\\.)*)'/g) {
        ($best, $at) = ($1, $-[0]) if $-[0] > $at;
    }
    while ($fb =~ /\b\Q$key\E\s*=>\s*q\{/g) {
        my $start = $-[0];
        my $from = pos($fb);
        my ($depth, $i) = (1, $from);
        while ($i < length($fb) and $depth) {
            my $ch = substr($fb, $i, 1);
            $depth++ if $ch eq '{';
            $depth-- if $ch eq '}';
            ++$i;
        }
        next if $depth;
        ($best, $at) = (substr($fb, $from, $i - $from - 1), $start) if $start > $at;
    }
    return undef unless defined $best;
    $best =~ s/\\'/'/g;
    return $best;
}

# A Condition, with `q{...}` taken by counting braces.
sub field_cond {
    my ($fb) = @_;
    my $c;
    if ($fb =~ /Condition\s*=>\s*q\{/g) {
        my $from = pos($fb);
        my ($depth, $i) = (1, $from);
        while ($i < length($fb) and $depth) {
            my $ch = substr($fb, $i, 1);
            $depth++ if $ch eq '{';
            $depth-- if $ch eq '}';
            ++$i;
        }
        $c = substr($fb, $from, $i - $from - 1) unless $depth;
    }
    ($c) = $fb =~ /Condition\s*=>\s*'([^']*)'/ unless defined $c;
    ($c) = $fb =~ /Condition\s*=>\s*"([^"]*)"/ unless defined $c;
    return undef unless defined $c;
    $c =~ s/\s+/ /g;
    $c =~ s/^ | $//g;
    return $c;
}

# The body of a Main-table id written as a list of alternatives. The id is
# matched by its value, not its spelling: Canon writes 0xd where the same
# table writes 0x4001.
sub main_arms {
    my ($main, $tag) = @_;
    while ($main =~ /^\s{4}(0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\s*=>\s*\[/gms) {
        # Capture before testing: `$1 =~ /^0x/` is itself a match, and it
        # clears $1 before hex() ever sees it.
        my $id_s = $1;
        my $id = $id_s =~ /^0x/ ? hex($id_s) : int($id_s);
        next unless $id == $tag;
        my $from = pos($main);
        my ($depth, $i) = (1, $from);
        while ($i < length($main) and $depth) {
            my $c = substr($main, $i, 1);
            $depth++ if $c eq '[';
            $depth-- if $c eq ']';
            ++$i;
        }
        return substr($main, $from, $i - $from - 1) unless $depth;
    }
    return undef;
}

# DATAMEMBERs a selector's condition stores by assigning the block's length.
my %count_store;
my $cur_sel;
# The byte order a SubDirectory declares for the table it opens.
my %table_byte_order;

my @re_list;
sub re_id {
    my ($pat) = @_;
    for my $i (0 .. $#re_list) { return $i if $re_list[$i] eq $pat }
    push @re_list, $pat;
    return $#re_list;
}

# A condition as a Rust expression, or nothing when it cannot be one.
sub compile_cond {
    my ($cond) = @_;
    $cond =~ s/^\s+|\s+$//g;
    return ('true') if $cond eq '';
    # `A and B`, `A or B` at the top level.
    for my $op (' or ', ' and ') {
        my ($depth, $i) = (0, 0);
        while ($i < length $cond) {
            my $c = substr($cond, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            if (!$depth and substr($cond, $i, length $op) eq $op) {
                my @l = compile_cond(substr($cond, 0, $i));
                my @r = compile_cond(substr($cond, $i + length $op));
                return () unless @l and @r;
                return ($op eq ' or ' ? "($l[0] || $r[0])" : "($l[0] && $r[0])");
            }
            ++$i;
        }
    }
    if ($cond =~ /^\((.*)\)$/) {
        my $inner = $1;
        my ($depth, $ok) = (0, 1);
        for my $i (0 .. length($inner) - 1) {
            my $c = substr($inner, $i, 1);
            $depth++ if $c eq '(';
            $depth-- if $c eq ')';
            $ok = 0 if $depth < 0;
        }
        return compile_cond($inner) if $ok and $depth == 0;
    }
    return ("!(" . (compile_cond($1))[0] . ")") if $cond =~ /^not (.*)$/ and compile_cond($1);
    # `$$self{Model} =~ /EOS 70D$/`, `$$self{Make} =~ /Kodak/i`.
    # `=~ m/.../` says the same as `=~ /.../`.
    if ($cond =~ m{^\$\$self\{(Model|Make)\} (=~|!~) m?/(.*?)/(i?)$}) {
        my ($what, $op, $pat, $ci) = ($1, $2, $3, $4);
        return () if $pat =~ /\(\?[=!<]/;
        $pat = "(?i)$pat" if $ci;
        return sprintf('%sMODEL_RE_%d.is_match(%s)',
                       $op eq '!~' ? '!' : '', re_id($pat), lc $what);
    }
    if ($cond =~ /^\$count (==|!=|<=|>=|<|>) (\d+)$/) {
        return "count $1 $2";
    }
    if ($cond =~ /^\$format (eq|ne) "(\w+)"$/) {
        return sprintf('format %s "%s"', $1 eq 'eq' ? '==' : '!=', $2);
    }
    if ($cond =~ m{^\$format (=~|!~) /\^(\w+)/$}) {
        return sprintf('%sformat.starts_with("%s")', $1 eq '!~' ? '!' : '', $2);
    }
    # `$$self{FileType} eq "CR3"`: what the reader opened.
    if ($cond =~ /^\$\$self\{FileType\} (eq|ne) "(\w+)"$/) {
        return sprintf('file_type %s "%s"', $1 eq 'eq' ? '==' : '!=', $2);
    }
    if ($cond =~ m{^\$\$valPt (=~|!~) /(.*?)/[a-z]*$}) {
        my ($op, $re) = ($1, $2);
        # `^(A|B)`: the block may open either way. Every alternative is a
        # prefix of its own, and the test is whether any of them matches.
        if ($re =~ /^\^\(([^()]*\|[^()]*)\)$/) {
            my @alts = map { byte_prefix("^$_") } split /\|/, $1;
            return () if grep { !defined } @alts;
            return sprintf('%sprefix_matches_any(data.get(__OFF__..).unwrap_or(&[]), &[%s])',
                           $op eq '!~' ? '!' : '', join(', ', @alts));
        }
        my $pat = byte_prefix($re);
        return () unless defined $pat;
        # `$$valPt` is the value of the entry the condition is written on,
        # not the block: inside a binary table it starts at that entry's own
        # bytes. __OFF__ is filled in where the field is emitted, and is 0 for
        # a Main-table id, whose value is the whole block.
        return ($op eq '!~' ? '!' : '') . "prefix_matches(data.get(__OFF__..).unwrap_or(&[]), $pat)";
    }
    # `($$self{CameraInfoCount} = $count) and ...`: an assignment used as the
    # test. ExifTool stores the block's own length whether or not this arm is
    # taken, and the sub-table indexes its last fields from it.
    if ($cond =~ /^\$\$self\{(\w+)\} = \$count$/) {
        $count_store{$cur_sel} = $1 if defined $cur_sel;
        return 'count != 0';
    }
    # `$$self{ColorDataVersion} == -3`: what an earlier field of this table
    # stored under that name.
    if ($cond =~ /^\$\$self\{(\w+)\} (==|!=|<=|>=|<|>) (-?[\d.]+)$/) {
        my ($dm, $op, $num) = ($1, $2, $3);
        $num .= '.0' unless $num =~ /\./;
        return sprintf('dm_get(dm, "%s").is_some_and(|v| v %s %s)', $dm, $op, $num);
    }
    if ($cond =~ /^defined \$\$self\{(\w+)\}$/) {
        return sprintf('dm_get(dm, "%s").is_some()', $1);
    }
    # `$$self{FirmwareVersion} ge "05.00"`: a string member, compared the way
    # Perl compares strings.
    # `$$self{ShutterMode} ne 96`: Perl's string comparison, written without
    # quotes because the value happens to be a number.
    if ($cond =~ /^\$\$self\{(\w+)\} (eq|ne|lt|le|gt|ge) (-?[\d.]+)$/) {
        my ($dm, $op, $num) = ($1, $2, $3);
        my $rust = { eq => '==', ne => '!=', lt => '<', le => '<=', gt => '>', ge => '>=' }->{$op};
        return sprintf('dm_val(dm, "%s").is_some_and(|v| v.as_string().as_str() %s "%s")',
                       $dm, $rust, $num);
    }
    if ($cond =~ /^\$\$self\{(\w+)\} (eq|ne|lt|le|gt|ge) "([^"]*)"$/) {
        my ($dm, $op, $str) = ($1, $2, $3);
        my $rust = { eq => '==', ne => '!=', lt => '<', le => '<=', gt => '>', ge => '>=' }->{$op};
        $str =~ s/\\/\\\\/g;
        $str =~ s/"/\\"/g;
        return sprintf('dm_val(dm, "%s").is_some_and(|v| v.as_string().as_str() %s "%s")',
                       $dm, $rust, $str);
    }
    if ($cond =~ /^\$\$self\{(\w+)\}$/) {
        # Perl's truth, not "non-zero": a version string is true.
        return sprintf('dm_val(dm, "%s").is_some_and(conv_expr::Val::truthy)', $1);
    }
    return ();
}

# `^[\0-\x40]` and friends: a fixed prefix of bytes, as a Rust slice of
# Option<(min, max)> -- None where the pattern accepts anything.
sub byte_prefix {
    my ($re) = @_;
    return undef unless $re =~ s/^\^//;
    my @out;
    while (length $re) {
        if ($re =~ s/^\[\\?(\\x[0-9a-fA-F]{2}|\\0|.)-\\?(\\x[0-9a-fA-F]{2}|\\0|.)\]//) {
            my ($a, $b) = (chr_of($1), chr_of($2));
            return undef unless defined $a and defined $b;
            push @out, "Some(&[($a, $b)])";
            next;
        }
        # `[\x01\x02\x10\x20]`: a set of discrete bytes rather than a range.
        if ($re =~ s/^\[((?:\\x[0-9a-fA-F]{2}|\\0|[^\]\\-])+)\]//) {
            my $set = $1;
            my @bytes;
            while (length $set) {
                if ($set =~ s/^(\\x[0-9a-fA-F]{2}|\\0)//) { push @bytes, chr_of($1); next }
                if ($set =~ s/^(.)//) { push @bytes, ord $1; next }
                last;
            }
            return undef if grep { !defined } @bytes;
            push @out, 'Some(&[' . join(', ', map { "($_, $_)" } @bytes) . '])';
            next;
        }
        if ($re =~ s/^(\\x[0-9a-fA-F]{2}|\\0)//) {
            my $c = chr_of($1);
            return undef unless defined $c;
            push @out, "Some(&[($c, $c)])";
            next;
        }
        if ($re =~ s/^\.//) { push @out, 'None'; next }
        if ($re =~ s/^\\d//) { push @out, 'Some(&[(0x30, 0x39)])'; next }
        # An escaped character stands for itself.
        if ($re =~ s/^\\([.\$^*+?()\[\]{}|\/\\])//) {
            my $c = ord $1;
            push @out, "Some(&[($c, $c)])";
            next;
        }
        if ($re =~ s/^([A-Za-z0-9 _\/-])//) {
            my $c = ord $1;
            push @out, "Some(&[($c, $c)])";
            next;
        }
        return undef;
    }
    return '&[' . join(', ', @out) . ']';
}

sub chr_of {
    my ($s) = @_;
    return hex(substr($s, 2)) if $s =~ /^\\x/;
    return 0 if $s eq '\\0';
    return ord($s) if length($s) == 1;
    return undef;
}

# ---------------------------------------------------------------- parsing
sub parse_table {
    my ($module, $table, $src, $shared) = @_;
    return if $by_name{"$module\::$table"};
    my $body = table_body($src, $module, $table);
    unless (defined $body) {
        note("$module\::$table: no such table in the module");
        return;
    }
    # Comment-only lines are not code: a commented-out field read as live is
    # a tag ExifTool never has.
    $body =~ s/^\s*#.*$//gm;

    my ($fmt) = $body =~ /FORMAT\s*=>\s*'(\w+)'/;
    $fmt ||= 'int8u';
    unless (exists $WIDTH{$fmt}) {
        note("$module\::$table: FORMAT '$fmt'");
        return;
    }
    my ($first) = $body =~ /FIRST_ENTRY\s*=>\s*(\d+)/;
    $first = 0 unless defined $first;
    # A table names its own families: FLIR's calibration blocks are APP1, not
    # MakerNotes, and its GPS box says family 1 is FLIR.
    my ($grp0) = $body =~ /GROUPS\s*=>\s*\{[^}]*0\s*=>\s*'(\w+)'/;
    my ($grp1) = $body =~ /GROUPS\s*=>\s*\{[^}]*1\s*=>\s*'(\w+)'/;
    my ($grp2) = $body =~ /GROUPS\s*=>\s*\{[^}]*2\s*=>\s*'(\w+)'/;
    # A table that says nothing takes its module's own family 0 -- which for a
    # maker-note module is MakerNotes and for RIFF is RIFF.
    $grp0 ||= $MAKERNOTE_MODULE{$module} ? 'MakerNotes' : $module;
    $grp1 ||= $module;
    $grp2 ||= 'Image';
    my $prio0 = $body =~ /PRIORITY\s*=>\s*0/ ? 1 : 0;

    my $t = {
        module => $module, name => $table, fmt => $fmt, width => $WIDTH{$fmt},
        first => $first, grp0 => $grp0, grp1 => $grp1, grp2 => $grp2, prio0 => $prio0,
        fields => [], subdirs => [],
    };
    $by_name{"$module\::$table"} = $t;
    push @tables, $t;

    my (%arm_prev, %arm_dead);
    for my $rf (scan_fields($body)) {
        my ($off_s, $fb, $is_arm) = @$rf;
        # `0.1`, `0.2`, `0.3`: three bit-fields of the same byte, told apart
        # by their Mask. The fraction only makes the key unique -- the entry
        # is the integer part -- and matching integers alone dropped every one
        # of them, and every table made of them, in silence.
        my $off = $off_s =~ /^0x/ ? hex($off_s) : int($off_s);

        # A whole field can be one splice: `0x0210 => { %selfTimerB2010 }`.
        for (1 .. 4) {
            my $before = $fb;
            $fb =~ s/(^|[{,]\s*)%(\w+)\s*(?=[,}]|$)/exists $shared->{$2} ? "$1$shared->{$2}," : "$1%$2"/gme;
            last if $fb eq $before;
        }

        my $guard;
        if ($is_arm) {
            next if $arm_dead{$off};
            my $c_src = field_cond($fb);
            my $own = 'true';
            if (defined $c_src) {
                my @c = compile_cond($c_src);
                unless (@c) {
                    note("$module\::$table $off_s: alternative, condition -- $c_src");
                    $arm_dead{$off} = 1;
                    next;
                }
                $own = $c[0];
            }
            my @g = map { "!($_)" } @{$arm_prev{$off} || []};
            push @{$arm_prev{$off}}, $own;
            push @g, $own unless $own eq 'true';
            $guard = @g ? join(' && ', @g) : undef;
        } else {
            my $c_src = field_cond($fb);
            if (defined $c_src) {
                my @c = compile_cond($c_src);
                unless (@c) {
                    note("$module\::$table $off_s: condition -- $c_src");
                    next;
                }
                $guard = $c[0] eq 'true' ? undef : $c[0];
            }
        }

        my ($name) = $fb =~ /Name\s*=>\s*'([^']+)'/;
        unless ($name) {
            note("$module\::$table $off_s: no Name");
            next;
        }
        # `Unknown => 1` keeps a tag out of the output unless -u is given, which
        # is not the default; the value is still read, because a later field
        # can be conditioned on it.
        my $hidden = $fb =~ /Unknown\s*=>\s*1/ ? 1 : 0;
        my ($ffmt) = $fb =~ /Format\s*=>\s*'([^']+)'/;
        my $fb_had_format = $ffmt;
        $ffmt ||= $fmt;

        if ($fb =~ /SubDirectory\s*=>/) {
            my ($mod, $sub) = $fb =~ /TagTable\s*=>\s*'Image::ExifTool::(\w+)::(\w+)'/;
            unless ($sub) {
                note("$module\::$table $off_s: sub-directory with no table named");
                next;
            }
            my ($sfmt, $slen) = $ffmt =~ /^(\w+)\[(0x[0-9a-fA-F]+|\d+)\]$/;
            $slen = ($slen =~ /^0x/ ? hex($slen) : int($slen)) if defined $slen;
            # `undef[120]` and `string[16]` are that many bytes.
            my $sw = defined $sfmt
                ? ($WIDTH{$sfmt} // (($sfmt eq 'undef' or $sfmt eq 'string') ? 1 : undef))
                : undef;
            # `Start => '$val'`: the entry holds an offset rather than the
            # block itself, and the sub-directory begins there. `$dirStart +
            # $val` says the same thing for a reader handed the block.
            my ($start) = $fb =~ /Start\s*=>\s*'([^']*)'/;
            if (defined $start
                and ($start eq '$val' or $start eq '$dirStart + $val')
                and defined $fb_had_format
                and exists $WIDTH{$ffmt}) {
                push @{$t->{subdirs}}, {
                    off => $off, mod => $mod, sub => $sub, cond => $guard,
                    len => undef, pointer => $ffmt,
                };
                push @{$pending{$mod}}, $sub;
                next;
            }
            my $len;
            if (defined $slen and defined $sw) {
                $len = $slen * $sw;
            } elsif (not defined $fb_had_format
                     or $fb_had_format eq 'undef'
                     or $fb_had_format eq 'string') {
                # No Format of its own: ExifTool runs the sub-directory from
                # its entry to the end of the block.
                $len = undef;
            } else {
                note("$module\::$table $off_s: sub-directory into $sub, of unknown length ($ffmt)");
                next;
            }
            push @{$t->{subdirs}}, {
                off => $off, mod => $mod, sub => $sub, cond => $guard, len => $len,
            };
            push @{$pending{$mod}}, $sub;
            next;
        }

        # Read before the text branches below: a text field carries its
        # conversions as much as a number does, and extracting them after the
        # branch that returns left every one of them without.
        my $rconv_e = code_of($fb, 'RawConv');
        my $vconv_e = code_of($fb, 'ValueConv');
        my $pconv_e = code_of($fb, 'PrintConv');
        for ($rconv_e) { s/\$\$self\{\w+\}\s*=(?![~=])\s*//g if defined }
        my ($dmname_e) = $fb =~ /DataMember\s*=>\s*'(\w+)'/;
        unless (defined $dmname_e) {
            ($dmname_e) = $fb =~ /RawConv\s*=>\s*'\(?\$\$self\{(\w+)\}\s*=\s*\$val/;
        }

        my $count = 1;
        # `Format => 'string'` with no count runs from the entry to the end
        # of the block, stopping at the first NUL, as ExifTool's reader does.
        # `unicode[N]` is UTF-16, N characters, in the block's byte order.
        if ($ffmt =~ /^unicode(?:\[(0x[0-9a-fA-F]+|\d+)\])?$/) {
            my $n2 = $1;
            $n2 = defined $n2 ? ($n2 =~ /^0x/ ? hex($n2) : int($n2)) : undef;
            push @{$t->{fields}}, {
                off => $off, name => $name, fmt => 'unicode', n => $n2,
                hidden => $hidden, cond => $guard, conv => {}, text => 1,
                rconv => $rconv_e, vconv => $vconv_e, pconv => $pconv_e,
                dmname => $dmname_e,
            };
            next;
        }
        if ($ffmt eq 'string' or $ffmt eq 'undef') {
            push @{$t->{fields}}, {
                off => $off, name => $name, fmt => $ffmt, n => undef,
                hidden => $hidden, cond => $guard, conv => {}, text => 1,
                rconv => $rconv_e, vconv => $vconv_e, pconv => $pconv_e,
                dmname => $dmname_e,
            };
            next;
        }
        if ($ffmt =~ /^(string|undef)\[(0x[0-9a-fA-F]+|\d+)\]$/) {
            my ($f2, $n2) = ($1, $2);
            $n2 = $n2 =~ /^0x/ ? hex($n2) : int($n2);
            push @{$t->{fields}}, {
                off => $off, name => $name, fmt => $f2, n => $n2, hidden => $hidden,
                cond => $guard, conv => {}, text => 1,
                rconv => $rconv_e, vconv => $vconv_e, pconv => $pconv_e,
                dmname => $dmname_e,
            };
            next;
        }
        if ($ffmt =~ /^(\w+)\[(0x[0-9a-fA-F]+|\d+)\]$/) {
            my ($f2, $c2) = ($1, $2);
            ($ffmt, $count) = ($f2, $c2 =~ /^0x/ ? hex($c2) : int($c2));
        }
        if ($ffmt =~ /\[/) {
            # A variable count shifts every entry after it (ExifTool's
            # $varSize), so the rest of the table cannot be read either.
            note("$module\::$table $off_s $name: count is not a number ($ffmt) -- and every entry after it moves with it");
            $t->{variable} = 1;
            next;
        }
        unless (exists $WIDTH{$ffmt}) {
            note("$module\::$table $off_s $name: format '$ffmt'");
            next;
        }
        # However many the Format says: ExifTool joins them all, and
        # WB_RedLevelsKelvin really is seventy-five numbers.

        my ($mask) = $fb =~ /Mask\s*=>\s*(0x[0-9a-fA-F]+|\d+)/;
        $mask = hex($mask) if defined $mask and $mask =~ /^0x/;
        my ($dmname) = $fb =~ /DataMember\s*=>\s*'(\w+)'/;
        unless (defined $dmname) {
            ($dmname) = $fb =~ /RawConv\s*=>\s*'\(?\$\$self\{(\w+)\}\s*=\s*\$val/;
        }
        my $rconv = code_of($fb, 'RawConv');
        # `($$self{FocusDistanceUpper} = $val) || undef` stores and then tests.
        # The store is the dm.push that already happened, so what is left to
        # evaluate is the test -- and without dropping the assignment the
        # evaluator declines the whole thing and a zero distance is reported
        # where ExifTool reports nothing.
        $rconv =~ s/\$\$self\{\w+\}\s*=(?![~=])\s*//g if defined $rconv;
        my $vconv = code_of($fb, 'ValueConv');
        my $pconv = code_of($fb, 'PrintConv');
        unless (defined $vconv) {
            ($vconv) = $fb =~ /ValueConv\s*=>\s*\\&(\w+)/;
            $vconv = "Image::ExifTool::${module}::$vconv(\$val)" if defined $vconv;
        }
        unless (defined $pconv) {
            ($pconv) = $fb =~ /PrintConv\s*=>\s*\\&(\w+)/;
            $pconv = "Image::ExifTool::${module}::$pconv(\$val)" if defined $pconv;
        }
        for ($rconv, $vconv, $pconv) { s/\\'/'/g if defined }

        my %conv;
        # `PrintConv => \%canonLensTypes`: a reference to one of the module's
        # own hashes, which only exists once the module is loaded.
        # A splice puts its own keys where the `%name` stands, and a key
        # written after it wins -- that is Perl's hash literal. So the last
        # PrintConv in the expanded body is the one that counts:
        # FilterEffectMonochrome splices %psInfo and then names its own.
        my $inline_at = -1;
        $inline_at = $-[0] if $fb =~ /PrintConv\s*=>\s*\{/;
        # An expression PrintConv wins over an earlier hash or reference just
        # as a later hash does.
        my $expr_at = -1;
        $expr_at = $-[0] if defined $pconv and $fb =~ /PrintConv\s*=>\s*(?:'|q\{)/;
        my $ref_at = -1;
        $ref_at = $-[0] if $fb =~ /PrintConv\s*=>\s*\\%\w+/;
        if ($ref_at > $inline_at and $ref_at > $expr_at
            and my ($href) = $fb =~ /PrintConv\s*=>\s*\\%(?:Image::ExifTool::\w+::)?(\w+)/) {
            # `\%Image::ExifTool::Minolta::minoltaLensTypes` names the module
            # it lives in; the hash is looked up there, not here.
            my ($hmod) = $fb =~ /PrintConv\s*=>\s*\\%Image::ExifTool::(\w+)::/;
            my $found = 0;
            # A `my %name = (...)` is a file-scoped lexical: invisible to the
            # symbol table, so it is read from the source as text. A package
            # hash -- %canonLensTypes -- is built at run time and can only be
            # had by loading the module.
            if (!defined $hmod and defined $shared->{$href}) {
                my $c = $shared->{$href};
                while ($c =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*'((?:[^'\\]|\\.)*)'/g) {
                    my ($k, $v) = ($1, $2);
                    $k = $k =~ /^0x/ ? hex($k) : int($k);
                    $v =~ s/\\'/'/g;
                    $conv{$k} = $v;
                    $found = 1;
                }
                # `OTHER => sub { shift }` prints the raw value, which is what
                # a key with no entry already does here.
                $found = 1 if $c =~ /OTHER\s*=>\s*sub\s*\{\s*shift\s*\}/;
            }
            unless ($found) {
                my $ref = named_hash($hmod // $module, $href);
                if ($ref) {
                    for my $k (keys %$ref) {
                        my $v = $ref->{$k};
                        if (ref $v) {
                            note("$module\::$table $off_s $name: %$href\{$k} is a " . ref($v) . " reference");
                            next;
                        }
                        # A fractional key names one of the lenses sharing an
                        # id, which only PrintLensID reaches.
                        next unless $k =~ /^-?\d+$/;
                        $conv{int $k} = $v;
                    }
                } else {
                    note("$module\::$table $off_s $name: PrintConv \\%$href, which is neither a lexical of the file nor a hash of the module");
                }
            }
        }
        # `PrintConv => { BITMASK => { 31 => '...', ... } }`: the value is a
        # set of bits, and ExifTool joins the names of those that are set.
        # Read as a plain hash it would print one name for the whole number.
        if (!%conv and $fb =~ /PrintConv\s*=>\s*\{\s*BITMASK\s*=>\s*\{(.*?)\n\s*\}/s) {
            my $bits = $1;
            my @pairs;
            while ($bits =~ /(\d+)\s*=>\s*'((?:[^'\\]|\\.)*)'/g) {
                my ($k, $v) = ($1, $2);
                $v =~ s/\\'/'/g;
                $v =~ s/'/\\'/g;
                push @pairs, "$k => '$v'";
            }
            $pconv = 'Image::ExifTool::DecodeBits($val, { ' . join(', ', @pairs) . ' })'
                if @pairs;
        }
        if (!%conv and !defined $pconv and $inline_at > $expr_at
            and $fb =~ /PrintConv\s*=>\s*\{(.*?)\n\s*\},/s) {
            my $c = $1;
            while ($c =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*'((?:[^'\\]|\\.)*)'/g) {
                my ($k, $v) = ($1, $2);
                $k = $k =~ /^0x/ ? hex($k) : int($k);
                $v =~ s/\\'/'/g;
                $conv{$k} = $v;
            }
        }

        undef $pconv if %conv;
        push @{$t->{fields}}, {
            off => $off, name => $name, fmt => $ffmt, n => $count, hidden => $hidden,
            cond => $guard, conv => \%conv, mask => $mask, dmname => $dmname,
            rconv => $rconv, vconv => $vconv, pconv => $pconv,
        };
    }
}

# ---------------------------------------------------------------- collect
my %src_of;
my %shared_of;

# A module's source and its `my %name = (...)` hashes, read once. Reading the
# source without the hashes left every spliced field without a Name.
sub module_src {
    my ($mod) = @_;
    unless ($src_of{$mod}) {
        $src_of{$mod} = read_module($mod);
        $shared_of{$mod} = { shared_hashes($src_of{$mod}) };
    }
    return $src_of{$mod};
}
for my $mod (sort keys %WANTED) {
    push @{$pending{$mod}}, @{$WANTED{$mod}};
}
# The tables a selector's arms point at are wanted too: an id whose choice is
# generated but whose targets are not would answer with a name nothing decodes.
for my $mod (sort keys %SELECTORS) {
    my $main = table_body(module_src($mod), $mod, 'Main') // next;
    $main =~ s/^\s*#.*$//gm;
    for my $tag (@{$SELECTORS{$mod}}) {
        my $arms = main_arms($main, $tag);
        next unless defined $arms;
        while ($arms =~ /TagTable\s*=>\s*'Image::ExifTool::(\w+)::(\w+)'/g) {
            push @{$pending{$1}}, $2;
        }
        # A SubDirectory can declare the byte order its table is written in --
        # Nikon reads a D700's ShotInfo big-endian and a D810's little-endian,
        # inside files that are otherwise the same.
        my (@abodies, $acur);
        my $ad = 0;
        for my $c (split //, $arms) {
            if ($c eq '{') { $ad++; $acur = '' unless defined $acur }
            $acur .= $c if defined $acur;
            if ($c eq '}') { $ad--; if ($ad == 0 and defined $acur) { push @abodies, $acur; undef $acur } }
        }
        for my $a (@abodies) {
            my ($m2, $sub) = $a =~ /TagTable\s*=>\s*'Image::ExifTool::(\w+)::(\w+)'/ or next;
            my ($bo) = $a =~ /ByteOrder\s*=>\s*'(\w+)'/ or next;
            $table_byte_order{"$m2\::$sub"} = $bo;
        }
    }
}
while (grep { @{$pending{$_} || []} } keys %pending) {
    for my $mod (sort keys %pending) {
        while (my $tbl = shift @{$pending{$mod}}) {
            my $src = module_src($mod);
            parse_table($mod, $tbl, $src, $shared_of{$mod});
        }
    }
}

my $sel_src = "";
# ------------------------------------------------------- variant selectors
$sel_src .= <<'SEL';
/// Which sub-table a Main-table id opens, by the conditions ExifTool writes
/// on it.
///
/// `None` means no arm matched, which for an id whose arms are all
/// sub-directories means ExifTool extracts nothing at all.
#[must_use]
pub fn variant_for(
    module: &str,
    tag: u16,
    make: &str,
    model: &str,
    data: &[u8],
    count: usize,
    format: &str,
) -> Option<&'static str> {
    let _ = (make, model, data, count, format);
    match (module, tag) {
SEL
for my $mod (sort keys %SELECTORS) {
    my $src = module_src($mod);
    my $main = table_body($src, $mod, 'Main');
    unless (defined $main) {
        note("$mod\::Main: no such table");
        next;
    }
    $main =~ s/^\s*#.*$//gm;
    for my $tag (@{$SELECTORS{$mod}}) {
        my $hex = sprintf '0x%04x', $tag;
        my $arms = main_arms($main, $tag);
        unless (defined $arms) {
            note(sprintf("%s::Main %s: no list of alternatives", $mod, $hex));
            next;
        }
        $cur_sel = "$mod\t$tag";
        $sel_src .= sprintf "        (\"%s\", %s) => {\n", $mod, $hex;
        my (@arm_bodies, $cur);
        my $d = 0;
        for my $c (split //, $arms) {
            if ($c eq '{') { $d++; $cur = '' unless defined $cur }
            $cur .= $c if defined $cur;
            if ($c eq '}') { $d--; if ($d == 0 and defined $cur) { push @arm_bodies, $cur; undef $cur } }
        }
        my $unconditional = 0;
        for my $a (@arm_bodies) {
            my ($sub) = $a =~ /TagTable\s*=>\s*'Image::ExifTool::\w+::(\w+)'/;
            next unless $sub;
            unless ($by_name{"$mod\::$sub"}) {
                note(sprintf("%s::Main %s -> %s: no decoder generated for it", $mod, $hex, $sub));
                next;
            }
            my $c_src = field_cond($a);
            unless (defined $c_src) {
                $sel_src .= sprintf "            Some(\"%s::%s\")\n", $mod, $sub;
                $unconditional = 1;
                last;
            }
            my @c = compile_cond($c_src);
            unless (@c) {
                note(sprintf("%s::Main %s -> %s: condition -- %s", $mod, $hex, $sub, $c_src));
                next;
            }
            (my $ce = $c[0]) =~ s/__OFF__/0/g;
            $sel_src .= sprintf "            if %s {\n                return Some(\"%s::%s\");\n            }\n", $ce, $mod, $sub;
        }
        $sel_src .= "            None\n" unless $unconditional;
        $sel_src .= "        }\n";
    }
}
$sel_src .= "        _ => None,\n    }\n}\n\n";

# The byte order a table is written in, where its SubDirectory says.
$sel_src .= <<'BO';
/// The byte order a table is written in, where the SubDirectory that opens it
/// says so. Nikon reads a D700's ShotInfo big-endian and a D810's
/// little-endian, inside files that are otherwise the same.
#[must_use]
pub fn table_byte_order(table: &str) -> Option<ByteOrder> {
    match table {
BO
for my $k (sort keys %table_byte_order) {
    $sel_src .= sprintf "        \"%s\" => Some(ByteOrder::%s),\n", $k,
        $table_byte_order{$k} eq 'LittleEndian' ? 'LittleEndian' : 'BigEndian';
}
$sel_src .= "        _ => None,\n    }\n}\n\n";

# What a selector's condition stores on the way past.
$sel_src .= <<'CSTORE';
/// What a Main-table id stores on the file while testing its own condition.
///
/// `($$self{CameraInfoCount} = $count) and ...` is an assignment used as the
/// test: ExifTool keeps the block's own length whether or not that arm is the
/// one taken, and the sub-table indexes its last fields from it. The caller
/// seeds the state with this before decoding.
#[must_use]
pub fn count_member(module: &str, tag: u16) -> Option<&'static str> {
    match (module, tag) {
CSTORE
for my $k (sort keys %count_store) {
    my ($mod, $tag) = split /\t/, $k;
    $sel_src .= sprintf "        (\"%s\", %#06x) => Some(\"%s\"),\n", $mod, $tag, $count_store{$k};
}
$sel_src .= "        _ => None,\n    }\n}\n\n";
# ---------------------------------------------------------------- emission
sub esc { my $s = shift; $s =~ s/\\/\\\\/g; $s =~ s/"/\\"/g; $s }
sub fn_name { my $t = shift; lc "$t->{module}_$t->{name}" }

my $nf = 0; $nf += scalar @{$_->{fields}} for @tables;

print <<"HDR";
//! Auto-generated decoders for ExifTool's binary sub-tables.
//!
//! Do not edit: regenerate with
//! `perl scripts/gen_binary_tables.pl ../exiftool/lib > src/tags/binary_tables_generated.rs`.
//!
//! ${\ scalar @tables} tables, $nf fields. A binary sub-table is a block of
//! bytes addressed by index: ExifTool's ProcessBinaryData reads the entry at
//! `(index - FIRST_ENTRY) * sizeof(FORMAT)`, and a field's own Format says
//! what to read there. What the generator could not express is on its stderr.
//! Generated code: the shape of a table decides what is written, so a helper
//! no table happens to need and a cast that happens to be a no-op are both
//! ordinary here rather than something to tidy away by hand.
#![allow(dead_code, unused_parens, unused_mut, unused_variables)]
#![allow(clippy::too_many_arguments, clippy::useless_conversion)]
#![allow(
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::unreadable_literal,
    clippy::unnecessary_cast,
    clippy::identity_op,
    clippy::cast_lossless
)]

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::tags::conv_expr::{self, Val as Conv};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Which end the file puts first.
pub type ByteOrder = crate::metadata::exif::ByteOrderMark;

/// What the fields of this block have read so far, by the name ExifTool
/// stores them under.
/// A member is whatever the field read: a Nikon FirmwareVersion is a string,
/// and the conditions on the Z9's custom settings compare it to one.
pub type State = Vec<(String, Conv)>;

fn dm_val<'a>(dm: &'a State, name: &str) -> Option<&'a Conv> {
    dm.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn dm_get(dm: &State, name: &str) -> Option<f64> {
    dm_val(dm, name).map(Conv::as_num)
}

/// What a conversion of this block can ask the file about.
///
/// ExifTool keeps these on the object: `\$\$self{Model}` tells one encoding of
/// TargetExposureTime from another, and `\$\$self{FILE_TYPE} eq "CRW"` decides
/// whether an ExposureTime of zero means one second or nothing at all.
struct Ctx<'a> {
    make: &'a str,
    model: &'a str,
    file_type: &'a str,
    dm: &'a State,
}

impl conv_expr::ParseState for Ctx<'_> {
    /// The reader options a conversion asks about. ExifTool's ByteUnit
    /// defaults to SI, and nothing here can set it to Binary.
    fn option(&self, name: &str) -> Option<Conv> {
        match name {
            "ByteUnit" => Some(Conv::Str("SI".to_string())),
            _ => None,
        }
    }

    fn member(&self, name: &str) -> Option<Conv> {
        match name {
            "Make" => Some(Conv::Str(self.make.to_string())),
            "Model" => Some(Conv::Str(self.model.to_string())),
            "FILE_TYPE" | "FileType" => Some(Conv::Str(self.file_type.to_string())),
            _ => dm_val(self.dm, name).cloned(),
        }
    }
}

fn u8_at(d: &[u8], o: usize) -> Option<u8> { d.get(o).copied() }
fn i8_at(d: &[u8], o: usize) -> Option<i8> { d.get(o).map(|b| *b as i8) }

fn u16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u16> {
    let b = [*d.get(o)?, *d.get(o + 1)?];
    Some(if bo == ByteOrder::BigEndian { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
}
fn i16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i16> { u16_at(d, o, bo).map(|v| v as i16) }

fn u32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u32> {
    let b = [*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?];
    Some(if bo == ByteOrder::BigEndian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
}
fn i32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i32> { u32_at(d, o, bo).map(|v| v as i32) }

fn f32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f32> {
    let b = [*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?];
    Some(if bo == ByteOrder::BigEndian { f32::from_be_bytes(b) } else { f32::from_le_bytes(b) })
}

fn f64_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    let mut b = [0u8; 8];
    b.copy_from_slice(d.get(o..o + 8)?);
    Some(if bo == ByteOrder::BigEndian { f64::from_be_bytes(b) } else { f64::from_le_bytes(b) })
}

/// A 16-bit value stored the other way round from the rest of the block.
fn u16rev_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u16> {
    let other = if bo == ByteOrder::BigEndian { ByteOrder::LittleEndian } else { ByteOrder::BigEndian };
    u16_at(d, o, other)
}

/// ExifTool's rational32 is two 16-bit halves -- four bytes, not the eight of
/// the rational64 EXIF writes. A zero denominator reads as infinity, and 0/0
/// as nothing at all.
fn rat32u_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(u16_at(d, o, bo)?), f64::from(u16_at(d, o + 2, bo)?))
}
fn rat32s_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(i16_at(d, o, bo)?), f64::from(i16_at(d, o + 2, bo)?))
}
/// The eight-byte rational EXIF writes: two 32-bit halves.
fn rat64u_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(u32_at(d, o, bo)?), f64::from(u32_at(d, o + 4, bo)?))
}
fn rat64s_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(i32_at(d, o, bo)?), f64::from(i32_at(d, o + 4, bo)?))
}

/// A 32-bit fixed-point value: the number over 65536.
fn fix32u_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    u32_at(d, o, bo).map(|v| f64::from(v) / 65536.0)
}
fn fix32s_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    i32_at(d, o, bo).map(|v| f64::from(v) / 65536.0)
}

fn ratio(n: f64, d: f64) -> Option<f64> {
    if d == 0.0 { return if n == 0.0 { None } else { Some(f64::INFINITY) }; }
    Some(n / d)
}

/// N bytes as UTF-16 text, in the block's byte order.
fn utf16_at(d: &[u8], o: usize, n: usize, bo: ByteOrder) -> Option<String> {
    let raw = d.get(o..o + n)?;
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| {
            if bo == ByteOrder::BigEndian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .take_while(|u| *u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// N bytes as text. A `string` stops at its first NUL, as ExifTool's reader
/// does; an `undef` is the bytes as they are.
fn text_at(d: &[u8], o: usize, n: usize, stop_at_nul: bool) -> Option<String> {
    let raw = d.get(o..o + n)?;
    let end = if stop_at_nul {
        raw.iter().position(|b| *b == 0).unwrap_or(raw.len())
    } else {
        raw.len()
    };
    Some(raw[..end].iter().map(|b| *b as char).collect())
}

/// Whether the block opens with these bytes. A position is `None` when the
/// pattern accepts anything there, and otherwise a set of ranges the byte has
/// to fall in -- `[\x01\x02\x10\x20]` is four of them.
type BytePat<'a> = &'a [Option<&'a [(u8, u8)]>];

fn prefix_matches(d: &[u8], pat: BytePat) -> bool {
    if d.len() < pat.len() { return false; }
    pat.iter()
        .zip(d)
        .all(|(p, b)| p.is_none_or(|set| set.iter().any(|(lo, hi)| *b >= *lo && *b <= *hi)))
}

/// Whether the block opens the way any one of these patterns says.
fn prefix_matches_any(d: &[u8], pats: &[BytePat]) -> bool {
    pats.iter().any(|p| prefix_matches(d, p))
}

fn mk(
    name: &str,
    id: u16,
    print_value: String,
    raw: Value,
    grp0: &'static str,
    grp1: &'static str,
    grp2: &'static str,
    priority: i32,
) -> Tag {
    Tag {
        id: TagId::Numeric(id),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: grp0.into(),
            family1: grp1.into(),
            family2: grp2.into(),
            family3: crate::tag::MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value,
        priority,
    }
}

HDR

# model patterns
for my $i (0 .. $#re_list) {
    printf "static MODEL_RE_%d: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"%s\").expect(\"generated pattern\"));\n",
        $i, esc($re_list[$i]);
}
print "\n" if @re_list;

print "/// Decode one binary sub-table by the name ExifTool gives it, module first:
/// `Canon::ShotInfo`. Two modules can name a table the same thing.\n";
print "#[must_use]\n";
print <<'DECODE';
pub fn decode(
    table: &str,
    data: &[u8],
    make: &str,
    model: &str,
    bo: ByteOrder,
    file_type: &str,
    format: &str,
    dm: &mut State,
) -> Vec<Tag> {
DECODE
print "    match table {\n";
# Keyed by module and table: Minolta and Olympus both call a table MOV1.
printf("        \"%s::%s\" => %s(data, make, model, bo, file_type, format, dm),\n",
       $_->{module}, $_->{name}, fn_name($_)) for @tables;
print "        _ => Vec::new(),\n    }\n}\n\n";

for my $t (@tables) {
    printf "/// `Image::ExifTool::%s::%s` -- FORMAT %s, FIRST_ENTRY %d.\n",
        $t->{module}, $t->{name}, $t->{fmt}, $t->{first};
    if ($t->{variable}) {
        printf "/// Incomplete: a field of variable length moves every entry after\n";
        printf "/// it, and this reads them where they would be without it.\n";
    }
    printf "fn %s(data: &[u8], make: &str, model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {\n", fn_name($t);
    printf "    const GRP0: &str = \"%s\";\n", $t->{grp0};
    printf "    const GRP1: &str = \"%s\";\n", $t->{grp1};
    printf "    const GRP2: &str = \"%s\";\n", $t->{grp2};
    printf "    const PRIO: i32 = %s;\n",
        $t->{prio0} ? 'crate::tag::PRIORITY_EXPLICIT_ZERO' : '0';
    # A table can be empty of readable fields -- CameraInfoUnknown16 is a
    # name ExifTool gives a layout it does not describe -- so every argument
    # has to be spoken for.
    print  "    let mut tags = Vec::new();\n";
    print  "    let _ = (data, make, model, bo, file_type, format, &dm);\n";

    for my $f (sort { $a->{off} <=> $b->{off} } @{$t->{fields}}) {
        # `my $entry = int($index) * $increment` (ExifTool.pm:9957): the byte
        # is the index times the table's format size. FIRST_ENTRY does not
        # shift it -- it only says where an -U scan starts counting -- and
        # subtracting it moved every field of a FIRST_ENTRY => 1 table by one.
        my $byte = $f->{off} * $t->{width};
        my $ind = "    ";
        if (defined $f->{cond}) {
            (my $c = $f->{cond}) =~ s/__OFF__/sprintf '0x%x', $byte/ge;
            printf "%sif %s {\n", $ind, $c;
            $ind .= "    ";
        }
        # `unicode` is UTF-16 in the block's own byte order.
        if ($f->{text} and $f->{fmt} eq 'unicode') {
            unless ($f->{hidden}) {
                printf "%sif let Some(text) = utf16_at(data, 0x%x, %s, bo) {\n",
                    $ind, $byte,
                    defined $f->{n} ? 2 * $f->{n}
                                    : sprintf('data.len().saturating_sub(0x%x)', $byte);
                printf "%s    tags.push(mk(\"%s\", 0x%x, text.clone(), Value::String(text), GRP0, GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off};
                printf "%s}\n", $ind;
            }
            print  "    }\n" if defined $f->{cond};
            next;
        }
        if ($f->{text}) {
            if ($f->{hidden}) {
                # Read but not reported, and nothing else looks at a string
                # member, so there is nothing to read it for.
                print  "    }\n" if defined $f->{cond};
                next;
            }
            printf "%sif let Some(text) = text_at(data, 0x%x, %s, %s) {\n",
                $ind, $byte,
                defined $f->{n} ? $f->{n} : sprintf('data.len().saturating_sub(0x%x)', $byte),
                ($f->{fmt} eq 'string' ? 'true' : 'false');
            # A string field stores itself where ExifTool names it a
            # DataMember: the Z9's FirmwareVersion decides which of three
            # menu-settings layouts applies.
            printf "%s    dm.push((\"%s\".to_string(), Conv::Str(text.clone())));\n",
                $ind, $f->{dmname} if defined $f->{dmname};
            # A text field carries its conversions as much as a number does:
            # SerialInfo's two entries are ruled out by a RawConv unless they
            # look like a serial number, and emitting them regardless reported
            # an empty tag ExifTool never has.
            if (defined $f->{rconv} or defined $f->{vconv} or defined $f->{pconv}) {
                printf "%s    let ctx = Ctx { make, model, file_type, dm };\n", $ind;
                printf "%s    let mut cv = Conv::Str(text);\n", $ind;
                if (defined $f->{rconv}) {
                    printf "%s    let rc = conv_expr::eval_with(\"%s\", &cv, &ctx);\n",
                        $ind, esc($f->{rconv});
                    printf "%s    if rc.as_ref() != Some(&Conv::Undef) {\n", $ind;
                    printf "%s        if let Some(x) = rc { cv = x; }\n", $ind;
                    $ind .= "    ";
                }
                printf "%s    if let Some(x) = conv_expr::eval_with(\"%s\", &cv, &ctx) { cv = x; }\n",
                    $ind, esc($f->{vconv}) if defined $f->{vconv};
                printf "%s    let raw = Value::String(cv.as_string());\n", $ind;
                printf "%s    if let Some(x) = conv_expr::eval_with(\"%s\", &cv, &ctx) { cv = x; }\n",
                    $ind, esc($f->{pconv}) if defined $f->{pconv};
                printf "%s    tags.push(mk(\"%s\", 0x%x, cv.as_string(), raw, GRP0, GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off} unless $f->{hidden};
                if (defined $f->{rconv}) {
                    $ind = substr($ind, 4);
                    printf "%s    }\n", $ind;
                }
            } else {
                printf "%s    tags.push(mk(\"%s\", 0x%x, text.clone(), Value::String(text), GRP0, GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off} unless $f->{hidden};
            }
            printf "%s}\n", $ind;
            print  "    }\n" if defined $f->{cond};
            next;
        }
        my $reader = {
            int8u => 'u8_at', int8s => 'i8_at',
            int16u => 'u16_at', int16s => 'i16_at',
            int32u => 'u32_at', int32s => 'i32_at',
            rational32u => 'rat32u_at', rational32s => 'rat32s_at',
            int16uRev => 'u16rev_at',
            float => 'f32_at', double => 'f64_at',
            rational64u => 'rat64u_at', rational64s => 'rat64s_at',
            fixed32u => 'fix32u_at', fixed32s => 'fix32s_at',
        }->{$f->{fmt}};
        my $args = $f->{fmt} =~ /^int8/ ? '' : ', bo';
        my $w = $WIDTH{$f->{fmt}};

        if ($f->{n} > 1) {
            if ($IS_RATIONAL{$f->{fmt}}) {
                note(sprintf("%s::%s 0x%x", $t->{module}, $t->{name}, $f->{off}) . " $f->{name}: array of rationals");
                next;
            }
            printf "%s{\n", $ind;
            printf "%s    let mut parts = Vec::new();\n", $ind;
            printf "%s    for k in 0..%d {\n", $ind, $f->{n};
            printf "%s        match %s(data, 0x%x + k * %d%s) {\n", $ind, $reader, $byte, $w, $args;
            printf "%s            Some(x) => parts.push(x.to_string()),\n", $ind;
            printf "%s            None => { parts.clear(); break }\n", $ind;
            printf "%s        }\n%s    }\n", $ind, $ind;
            if ($f->{hidden}) {
                printf "%s}\n", $ind;
                print  "    }\n" if defined $f->{cond};
                next;
            }
            printf "%s    if !parts.is_empty() {\n", $ind;
            printf "%s        let s = parts.join(\" \");\n", $ind;
            # An array is ruled out by a RawConv as much as a scalar is:
            # Panasonic's Face2Position is `$$self{NumFacePositions} < 2 ?
            # undef : $val`, and emitting it regardless reported five face
            # positions of zeros that ExifTool does not have.
            my $arr_guard = 0;
            if (defined $f->{rconv}) {
                printf "%s        let ctx = Ctx { make, model, file_type, dm };\n", $ind;
                printf "%s        let rc = conv_expr::eval_with(\"%s\", &Conv::Str(s.clone()), &ctx);\n",
                    $ind, esc($f->{rconv});
                printf "%s        if rc.as_ref() != Some(&Conv::Undef) {\n", $ind;
                $arr_guard = 1;
                $ind .= "    ";
            }
            # An array carries its conversions as much as a scalar does:
            # RawMeasuredRGGB is four int32u read back with their halves
            # exchanged, and joining them without that gives four other
            # numbers entirely.
            if (defined $f->{vconv} or defined $f->{pconv}) {
                printf "%s        let ctx = Ctx { make, model, file_type, dm };\n", $ind;
                printf "%s        let mut cv = Conv::Str(s.clone());\n", $ind;
                printf "%s        if let Some(x) = conv_expr::eval_with(\"%s\", &cv, &ctx) { cv = x; }\n",
                    $ind, esc($f->{vconv}) if defined $f->{vconv};
                printf "%s        let raw = Value::String(cv.as_string());\n", $ind;
                printf "%s        if let Some(x) = conv_expr::eval_with(\"%s\", &cv, &ctx) { cv = x; }\n",
                    $ind, esc($f->{pconv}) if defined $f->{pconv};
                printf "%s        tags.push(mk(\"%s\", 0x%x, cv.as_string(), raw, GRP0, GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off} unless $f->{hidden};
            } else {
                printf "%s        tags.push(mk(\"%s\", 0x%x, s.clone(), Value::String(s), GRP0, GRP1, GRP2, PRIO));\n",
                    $ind, $f->{name}, $f->{off} unless $f->{hidden};
            }
            if ($arr_guard) {
                $ind = substr($ind, 4);
                printf "%s        }\n", $ind;
            }
            printf "%s    }\n%s}\n", $ind, $ind;
            print  "    }\n" if defined $f->{cond};
            next;
        }

        printf "%sif let Some(v) = %s(data, 0x%x%s) {\n", $ind, $reader, $byte, $args;
        if (defined $f->{mask}) {
            my ($shift, $m) = (0, $f->{mask});
            until ($m & 1) { $m >>= 1; ++$shift }
            printf "%s    let v = %s;\n", $ind,
                $shift ? sprintf('(v & %#x) >> %d', $f->{mask}, $shift) : sprintf('v & %#x', $f->{mask});
        }
        printf "%s    dm.push((\"%s\".to_string(), Conv::Num(f64::from(v))));\n",
            $ind, $f->{dmname} // $f->{name};
        # `Unknown => 1` is read but not reported unless -u is given, which is
        # not the default. The value still goes into the state, because a later
        # field can be conditioned on it; the conversions produce nothing but
        # the tag, so they are not run at all.
        if ($f->{hidden}) {
            printf "%s}\n", $ind;
            print  "    }\n" if defined $f->{cond};
            next;
        }
        printf "%s    let ctx = Ctx { make, model, file_type, dm };\n", $ind
            if defined $f->{rconv} or defined $f->{vconv} or defined $f->{pconv};
        my $guard = 0;
        # A RawConv can rule the value out -- `$val ? $val : undef` -- and it
        # can change what kind of number it is: AVIHeader's FrameRate is
        # `1e6 / $val`, and casting that back to the integer the reader
        # produced turned 29.97 into 29.
        printf "%s    let base = Conv::Num(f64::from(v));\n", $ind;
        if (defined $f->{rconv}) {
            printf "%s    let rc = conv_expr::eval_with(\"%s\", &base, &ctx);\n",
                $ind, esc($f->{rconv});
            printf "%s    if rc.as_ref() != Some(&Conv::Undef) {\n", $ind;
            printf "%s        let base = rc.unwrap_or(base);\n", $ind;
            $guard = 1;
            $ind .= "    ";
        }
        my %conv = %{$f->{conv}};
        if (%conv) {
            printf "%s    #[allow(clippy::cast_possible_truncation)]\n", $ind;
            printf "%s    let s = match base.as_num() as i64 {\n", $ind;
            for my $k (sort { $a <=> $b } keys %conv) {
                printf "%s        %d => \"%s\".to_string(),\n", $ind, $k, esc($conv{$k});
            }
            printf "%s        other => other.to_string(),\n", $ind;
            printf "%s    };\n", $ind;
            printf "%s    #[allow(clippy::cast_possible_truncation)]\n", $ind;
            printf "%s    let raw = Value::I32(base.as_num() as i32);\n", $ind;
            printf "%s    tags.push(mk(\"%s\", 0x%x, s, raw, GRP0, GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
        } elsif (defined $f->{vconv} or defined $f->{pconv}) {
            printf "%s    let mut cv = base;\n", $ind;
            printf "%s    if let Some(x) = conv_expr::eval_with(\"%s\", &cv, &ctx) { cv = x; }\n",
                $ind, esc($f->{vconv}) if defined $f->{vconv};
            printf "%s    let raw = Value::F64(cv.as_num());\n", $ind;
            printf "%s    if let Some(x) = conv_expr::eval_with(\"%s\", &cv, &ctx) { cv = x; }\n",
                $ind, esc($f->{pconv}) if defined $f->{pconv};
            printf "%s    tags.push(mk(\"%s\", 0x%x, cv.as_string(), raw, GRP0, GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
        } else {
            printf "%s    #[allow(clippy::cast_possible_truncation)]\n", $ind;
            printf "%s    tags.push(mk(\"%s\", 0x%x, base.as_string(), Value::I32(base.as_num() as i32), GRP0, GRP1, GRP2, PRIO));\n",
                $ind, $f->{name}, $f->{off} unless $f->{hidden};
        }
        if ($guard) {
            $ind = substr($ind, 4);
            printf "%s    }\n", $ind;
        }
        printf "%s}\n", $ind;
        print  "    }\n" if defined $f->{cond};
    }

    for my $sd (@{$t->{subdirs}}) {
        my $byte = $sd->{off} * $t->{width};
        my $ind = "    ";
        if (defined $sd->{cond}) {
            (my $c = $sd->{cond}) =~ s/__OFF__/sprintf '0x%x', $byte/ge;
            printf "%sif %s {\n", $ind, $c;
            $ind .= "    ";
        }
        my $target = $by_name{"$sd->{mod}::$sd->{sub}"};
        unless ($target) {
            note(sprintf("%s::%s 0x%x:", $t->{module}, $t->{name}, $sd->{off}) . " sub-directory into $sd->{sub}, not generated");
            print "    }\n" if defined $sd->{cond};
            next;
        }
        if (defined $sd->{pointer}) {
            my $reader = {
                int8u => 'u8_at', int16u => 'u16_at', int32u => 'u32_at',
                int8s => 'i8_at', int16s => 'i16_at', int32s => 'i32_at',
            }->{$sd->{pointer}};
            my $args = $sd->{pointer} =~ /^int8/ ? '' : ', bo';
            printf "%sif let Some(sub) = %s(data, 0x%x%s)\n", $ind, $reader, $byte, $args;
            printf "%s    .and_then(|at| data.get(at as usize..))\n", $ind;
            printf "%s{\n", $ind;
        } elsif (defined $sd->{len}) {
            printf "%sif let Some(sub) = data.get(0x%x..0x%x + %d) {\n", $ind, $byte, $byte, $sd->{len};
        } else {
            printf "%sif let Some(sub) = data.get(0x%x..) {\n", $ind, $byte;
        }
        printf "%s    tags.extend(%s(sub, make, model, bo, file_type, format, dm));\n", $ind, fn_name($target);
        printf "%s}\n", $ind;
        print  "    }\n" if defined $sd->{cond};
    }
    print "    tags\n}\n\n";
}

$sel_src =~ s/\s+$//;
print "$sel_src\n\n";

print "#[cfg(test)]\nmod tests {\n    use super::*;\n\n";
print "    /// Every generated pattern must compile: a bad one would otherwise\n";
print "    /// only surface as a panic on whichever file first reaches it.\n";
print "    #[test]\n    fn every_model_pattern_compiles() {\n";
printf "        LazyLock::force(&MODEL_RE_%d);\n", $_ for (0 .. $#re_list);
print "    }\n}\n";

printf STDERR "Generated %d tables, %d fields, %d model patterns.\n",
    scalar @tables, $nf, scalar @re_list;
printf STDERR "SKIPPED %d -- these are NOT silently dropped:\n", scalar @skipped;
printf STDERR "  %s\n", $_ for @skipped;
