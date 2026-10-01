#!/usr/bin/perl
# Extract ALL PrintConv hash tables from ExifTool Perl source.
# Generates them as static Rust tables, with the lookup functions that read them.
#
# Usage: perl scripts/gen_print_conv.pl /path/to/exiftool/lib > src/tags/print_conv_generated.rs
#        cargo fmt
#
# The committed file is rustfmt's layout of this output, so the regeneration
# is not finished until `cargo fmt` has run (`cargo fmt --check` gates CI).

use strict;
use warnings;
use File::Find;
# The emitted offsets count bytes, as a Rust str does.
use bytes ();
# Reading a package hash by name needs symbolic references.
no strict 'refs';

my $lib_dir = $ARGV[0] || '../exiftool/lib';
die "Cannot find $lib_dir" unless -d $lib_dir;

# Load ExifTool so the tables it builds as it loads can be read from it. A
# scan of the text cannot see `%sonyLensTypes = %$minoltaTypes`, and without
# it a lens prints as its raw id.
unshift @INC, $lib_dir;
my @loaded;

# Collect all .pm files
my @pm_files;
find(sub { push @pm_files, $File::Find::name if /\.pm$/ }, "$lib_dir/Image/ExifTool");
@pm_files = sort @pm_files;

# Storage for all extracted conversions
# Key: "Module::TagID" => { value => "string", ... }
my %all_convs;
my %tag_names; # TagID => Name mapping per module

my $total_entries = 0;
my $total_tags = 0;
my @dropped;   # conversions not emitted, reported at the end
my %all_str_convs;      # conversions keyed by the value as a string
my %all_list_convs;     # one conversion per element of a multi-value tag
my $total_str_entries = 0;

for my $file (@pm_files) {
    open my $fh, '<', $file or next;
    my $content = do { local $/; <$fh> };
    close $fh;

    # (module name is derived below; load it so its runtime tables exist)

    # Extract module name from filename
    my ($module) = $file =~ m{/(\w+)\.pm$};
    next unless $module;

    # Load it so any table it builds as it loads can be read from the symbol
    # table. A module that will not load is no reason to stop: the text scan
    # still finds everything written out literally.
    unless ($loaded[0] and grep { $_ eq $module } @loaded) {
        push @loaded, $module;
        eval { require "Image/ExifTool/$module.pm"; 1 };
    }

    # Strategy: find tag entries with PrintConv => { ... }
    # We look for patterns like:
    #   0xNNNN => {
    #       Name => 'TagName',
    #       PrintConv => {
    #           N => 'String',
    #           ...
    #       },
    #   },

    # Find every tag definition and take its whole body by counting brackets.
    # A regex that allows only two levels of nesting stops early on a tag whose
    # PrintConv is a LIST of hashes -- Sony's HDR -- and hands back a body with
    # no conversion in it at all.
    my @tag_bodies;
    while ($content =~ /(0x[0-9a-fA-F]+|[\d]+)\s*=>\s*\{/g) {
        my $tag_hex = $1;
        my $from = pos($content);
        my ($depth, $i) = (1, $from);
        while ($i < length($content) and $depth) {
            my $c = substr($content, $i, 1);
            $depth++ if $c eq '{';
            $depth-- if $c eq '}';
            ++$i;
        }
        next if $depth;   # unbalanced: not a tag definition we can read
        push @tag_bodies, [$tag_hex, substr($content, $from, $i - $from - 1)];
        pos($content) = $from;
    }

    for my $tb (@tag_bodies) {
        my ($tag_hex, $body) = @$tb;

        # Get tag name. A name is not always `\w+`: Sony has Anti-Blur and
        # APS-CSizeCapture, and requiring word characters dropped those tags
        # -- with their conversions -- without a word.
        my ($name) = $body =~ /Name\s*=>\s*['"]([\w-]+)['"]/;
        unless ($name) {
            if ($body =~ /Name\s*=>\s*['"]([^'"]+)['"]/) {
                push @dropped, "$module $tag_hex: name '$1' is not a plain identifier";
            }
            next;
        }

        # Get tag ID as number
        my $tag_id;
        if ($tag_hex =~ /^0x/) {
            $tag_id = hex($tag_hex);
        } else {
            $tag_id = int($tag_hex);
        }
        next if $tag_id > 0xFFFF;

        # A PrintConv can be a LIST of hashes, one per element of the value:
        # ExifTool splits the value on spaces, converts each element with its
        # own hash and joins the results with '; ' (ExifTool.pm:3550, 3696).
        # Sony's HDR is `[{ 0 => 'Off', ... },{ 0 => 'Uncorrected image', ... }]`.
        if ($body =~ /PrintConv\s*=>\s*\[/) {
            my $start = $-[0] + length($&);
            my ($depth, $i) = (1, $start);
            while ($i < length($body) and $depth) {
                my $c = substr($body, $i, 1);
                $depth++ if $c eq '[';
                $depth-- if $c eq ']';
                ++$i;
            }
            my $listSrc = substr($body, $start, $i - $start - 1);
            my @parts;
            my ($d, $from) = (0, undef);
            for my $j (0 .. length($listSrc) - 1) {
                my $c = substr($listSrc, $j, 1);
                if ($c eq '{') { $from = $j + 1 unless $d++; }
                elsif ($c eq '}') { push @parts, substr($listSrc, $from, $j - $from) unless --$d; }
            }
            my @convs;
            for my $part (@parts) {
                my %c;
                while ($part =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*'([^']*)'/g) {
                    # Both captures must be taken before any other match: a
                    # `=~` of its own resets $1 and $2, which left every value
                    # of this table empty.
                    my ($k, $v) = ($1, $2);
                    $k = $k =~ /^0x/ ? hex($k) : int($k);
                    $c{$k} = $v;
                }
                push @convs, \%c if %c;
            }
            if (@convs > 1) {
                $all_list_convs{"${module}::${tag_id}"} = {
                    module => $module, tag_id => $tag_id, tag_name => $name, convs => \@convs,
                };
            } elsif (@parts) {
                push @dropped, "$module $tag_hex $name: PrintConv list of "
                             . scalar(@parts) . " part(s), none usable";
            }
        }

        # A PrintConv can be keyed by the whole value as a string rather than
        # by a number: Sony's VariableLowPassFilter is `{ '0 0' => 'n/a', '1 0'
        # => 'Off', ... }` over a two-element tag. Collected apart, since the
        # numeric lookup cannot express them.
        if ($body =~ /PrintConv\s*=>\s*\{([^}]+)\}/) {
            my $conv_body = $1;
            my %str_conv;
            while ($conv_body =~ /'([^']*[^0-9'][^']*)'\s*=>\s*'([^']*)'/g) {
                $str_conv{$1} = $2;
            }
            if (%str_conv) {
                $all_str_convs{"${module}::${tag_id}"} = {
                    module => $module, tag_id => $tag_id, tag_name => $name,
                    conv => \%str_conv,
                };
                $total_str_entries += scalar keys %str_conv;
            }
        }

        # Find PrintConv hash (not subroutine)
        if ($body =~ /PrintConv\s*=>\s*\{([^}]+)\}/) {
            my $conv_body = $1;
            my %conv;

            # Parse key => 'value' pairs
            while ($conv_body =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*['"]([^'"]*)['"]/g) {
                my $key = $1;
                my $val = $2;
                if ($key =~ /^0x/) {
                    $key = hex($key);
                }
                $conv{int($key)} = $val;
            }

            next unless %conv;

            my $conv_key = "${module}::${tag_id}";
            $all_convs{$conv_key} = {
                module => $module,
                tag_id => $tag_id,
                tag_name => $name,
                conv => \%conv,
            };
            $total_entries += scalar keys %conv;
            $total_tags++;
        }

        # Also handle PrintConv that references a named hash
        # PrintConv => \%hashName
        if ($body =~ /PrintConv\s*=>\s*\\%(\w+)/) {
            my $hash_name = $1;
            # Some of these are built when the module loads rather than
            # written out -- `%sonyLensTypes = %$minoltaTypes` copies Minolta's
            # list and derives entries from it -- so a scan of the text finds
            # nothing and the lens printed as 65535. Read those from the loaded
            # module instead.
            my $runtime = \%{"Image::ExifTool::${module}::${hash_name}"};
            if (%$runtime) {
                my %conv;
                # A key like 65535.1 is a second lens sharing an id. Sorted
                # numerically the whole id comes first, and that is the entry
                # ExifTool prints -- taken in hash order it printed whichever
                # lens happened to come out of the bucket.
                for my $key (sort { $a <=> $b } grep { /^-?\d+(\.\d+)?$/ } keys %$runtime) {
                    next if ref $$runtime{$key};
                    my $k = int($key);
                    $conv{$k} = $$runtime{$key} unless exists $conv{$k};
                }
                if (%conv) {
                    my $conv_key = "${module}::${tag_id}";
                    $all_convs{$conv_key} = {
                        module => $module,
                        tag_id => $tag_id,
                        tag_name => $name,
                        conv => \%conv,
                    };
                    $total_entries += scalar keys %conv;
                    $total_tags++;
                    next;
                }
            }
            # Try to find the hash definition in the same file
            if ($content =~ /\%${hash_name}\s*=\s*\(([^;]+)\);/s) {
                my $hash_body = $1;
                my %conv;
                while ($hash_body =~ /(-?\d+|0x[0-9a-fA-F]+)\s*=>\s*['"]([^'"]*)['"]/g) {
                    my $key = $1;
                    my $val = $2;
                    if ($key =~ /^0x/) { $key = hex($key); }
                    $conv{int($key)} = $val;
                }
                if (%conv) {
                    my $conv_key = "${module}::${tag_id}";
                    $all_convs{$conv_key} = {
                        module => $module,
                        tag_id => $tag_id,
                        tag_name => $name,
                        conv => \%conv,
                    };
                    $total_entries += scalar keys %conv;
                    $total_tags++;
                }
            }
        }
    }
}

# Generate Rust code
#
# The conversions are emitted as data, not as code: a `match` of some 36 000
# arms costs minutes of LLVM time and half a megabyte of machine code, where
# the same content as sorted arrays costs neither. Every table is stored once
# (with ExifTool 13.59 the 2030 tags share 1128 distinct tables, and every
# by-name table is one of them) and so is every string, in a blob addressed by
# offsets, so the arrays hold no pointers and need no relocations.
print "//! Auto-generated PrintConv tables from ExifTool Perl source.\n";
print "//! Generated by scripts/gen_print_conv.pl\n";
print "//! Total: $total_tags tags with $total_entries conversion entries.\n";
print "//! DO NOT EDIT MANUALLY.\n\n";

# Group by module
my %by_module;
for my $key (sort keys %all_convs) {
    my $info = $all_convs{$key};
    push @{$by_module{$info->{module}}}, $info;
}

# ── String pool ─────────────────────────────────────────────────────────────
my (@strings, %string_id);
sub string_id {
    my ($s) = @_;
    unless (exists $string_id{$s}) {
        $string_id{$s} = scalar @strings;
        push @strings, $s;
    }
    return $string_id{$s};
}

# ── Table pool: a table is a list of (value, string id), sorted by value ────
my (@tables, %table_id);
sub table_id {
    my ($conv) = @_;
    my @rows = map { [$_, string_id($$conv{$_})] } sort { $a <=> $b } keys %$conv;
    # The lookup is a binary search, so no two rows may share a value.
    for my $i (1 .. $#rows) {
        die "a table holds $rows[$i - 1][0] then $rows[$i][0]: values must strictly increase\n"
            unless $rows[$i - 1][0] < $rows[$i][0];
    }
    my $sig = join ',', map { "$$_[0]:$$_[1]" } @rows;
    unless (exists $table_id{$sig}) {
        $table_id{$sig} = scalar @tables;
        push @tables, \@rows;
    }
    return $table_id{$sig};
}

# (module, tag) -> table. Tags sorted within a module for a binary search.
my (@modules, @mod_end, @tag_ids, @tag_tabs, @tag_names, %module_index);
my %list_modules = map { $all_list_convs{$_}{module} => 1 } keys %all_list_convs;
for my $module (sort(keys %{{ %by_module, %list_modules }})) {
    my $first = @tag_ids;
    for my $tag (sort { $a->{tag_id} <=> $b->{tag_id} } @{$by_module{$module} || []}) {
        my @keys = keys %{$tag->{conv}};

        # A table of one entry is not a lookup to choose from. Everything
        # else is emitted however long it is: the lens lists run to several
        # hundred entries, and capping them at a hundred is why LensType
        # printed its raw id.
        if (@keys < 2) {
            push @dropped, sprintf("%s 0x%04X %s: only %d entry",
                                   $module, $tag->{tag_id}, $tag->{tag_name}, scalar @keys);
            next;
        }
        die sprintf("%s 0x%04X comes twice or out of order\n", $module, $tag->{tag_id})
            if @tag_ids > $first and $tag_ids[-1] >= $tag->{tag_id};
        push @tag_ids, $tag->{tag_id};
        push @tag_tabs, table_id($tag->{conv});
        push @tag_names, $tag->{tag_name};
    }
    $module_index{$module} = scalar @modules;
    push @modules, $module;
    push @mod_end, scalar @tag_ids;
}

# Name -> table: the first tag of that name in `sort keys %all_convs` order,
# which sorts the ids as text ("Canon::10" before "Canon::9").
my %by_name;
for my $key (sort keys %all_convs) {
    my $info = $all_convs{$key};
    next if scalar(keys %{$info->{conv}}) < 2;
    $by_name{$info->{tag_name}} = $info unless exists $by_name{$info->{tag_name}};
}
my @names = sort keys %by_name;
my @name_tabs = map { table_id($by_name{$_}{conv}) } @names;

# One table per element of a multi-value tag, sorted by (module index, tag).
my (@lists, @list_tabs);
for my $info (sort { $module_index{$a->{module}} <=> $module_index{$b->{module}}
                     or $a->{tag_id} <=> $b->{tag_id} } values %all_list_convs) {
    my @t = map { table_id($_) } @{$info->{convs}};
    push @lists, [$module_index{$info->{module}}, $info->{tag_id}, scalar @list_tabs,
                  scalar @t, "$info->{module} $info->{tag_name}"];
    push @list_tabs, @t;
}

sub rust_str {
    my ($s) = @_;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    return "\"$s\"";
}

# Every index the tables hold is a u16 and every offset a u32. A source that
# outgrows them stops the generator here, naming the array, rather than
# leaving a wrapped index to point at the wrong string.
sub fit {
    my ($what, $bits, @values) = @_;
    my $max = 2**$bits - 1;
    for (@values) {
        die "$what: $_ does not fit in a u$bits\n" if $_ < 0 or $_ > $max;
    }
    return @values;
}

# FNV-1a, 32 bits: the same function as `fnv1a` in the emitted code.
sub fnv1a {
    my $h = 0x811c9dc5;
    for my $c (unpack 'C*', $_[0]) {
        $h ^= $c;
        $h = ($h * 0x01000193) & 0xFFFFFFFF;
    }
    return $h;
}

# An open-addressing hash table over a list of strings: slot = index + 1,
# 0 = empty, linear probing. It is sized to be at most a quarter full. A miss
# ends at the first empty slot, so a table over half full is refused outright.
sub hash_slots {
    my ($what, @keys) = @_;
    my $size = 1;
    $size <<= 1 while $size < 4 * @keys;
    die sprintf("%s: %d keys in %d slots is over half full\n", $what, scalar @keys, $size)
        if 2 * @keys > $size;
    my @slots = (0) x $size;
    for my $i (0 .. $#keys) {
        my $s = fnv1a($keys[$i]) & ($size - 1);
        $s = ($s + 1) & ($size - 1) while $slots[$s];
        $slots[$s] = $i + 1;
    }
    return @slots;
}

# Emit a blob of strings plus where each one ends. The ends count bytes, as
# Rust does, and the blob carries its total so the build checks the count.
sub emit_blob {
    my ($doc, $blob, $ends, $list) = @_;
    my ($end, @ends) = (0);
    push @ends, $end += bytes::length($_) for @$list;
    print $doc;
    print "#[rustfmt::skip]\nstatic $blob: &str = sized($end, concat!(\n";
    printf "    %s, // %d\n", rust_str($$list[$_]), $_ for 0 .. $#$list;
    print "));\n\n";
    print "#[rustfmt::skip]\nstatic $ends: [u32; ", scalar @ends, "] = [\n";
    print "    $_,\n" for fit($ends, 32, @ends);
    print "];\n\n";
}
sub emit_ints {
    my ($name, $bits, @v) = @_;
    fit($name, $bits, @v);
    print "#[rustfmt::skip]\nstatic $name: [u$bits; ", scalar @v, "] = [\n";
    for (my $i = 0; $i < @v; $i += 16) {
        my $j = $i + 15 > $#v ? $#v : $i + 15;
        print "    ", join(', ', @v[$i .. $j]), ",\n";
    }
    print "];\n\n";
}

emit_blob("/// Every converted string, concatenated: string `i` ends at `STR_END[i]`\n"
        . "/// and starts where string `i - 1` ends.\n", 'STRINGS', 'STR_END', \@strings);

my $n_rows = 0; $n_rows += @$_ for @tables;
print "/// The distinct tables, concatenated, each sorted by value: table `t` ends\n";
print "/// at `TAB_END[t]` and starts where table `t - 1` ends.\n";
print "#[rustfmt::skip]\nstatic KEYS: [i64; $n_rows] = [\n";
print "    ", join(', ', map { $$_[0] } @{$tables[$_]}), ", // $_\n" for 0 .. $#tables;
print "];\n\n";
print "/// What each entry of `KEYS` converts to: an index into `STR_END`.\n";
print "#[rustfmt::skip]\nstatic VALS: [u16; $n_rows] = [\n";
print "    ", join(', ', fit('VALS', 16, map { $$_[1] } @{$tables[$_]})), ", // $_\n"
    for 0 .. $#tables;
print "];\n\n";
my $end = 0;
print "#[rustfmt::skip]\nstatic TAB_END: [u32; ", scalar @tables, "] = [\n";
print "    $_,\n" for fit('TAB_END', 32, map { $end += @$_ } @tables);
print "];\n\n";

emit_blob("/// The modules, sorted. Module `m` owns the tags that end at `MOD_END[m]`\n"
        . "/// in `TAG_IDS` and start where module `m - 1`'s end.\n",
          'MODULES', 'MODULE_END', \@modules);
emit_ints('MODULE_SLOTS', 16, hash_slots('MODULE_SLOTS', @modules));
emit_ints('MOD_END', 32, @mod_end);
fit('TAG_IDS', 16, @tag_ids);
fit('TAG_TAB', 16, @tag_tabs);
print "/// Tag ids, sorted within each module, and the table each one converts with.\n";
print "#[rustfmt::skip]\nstatic TAG_IDS: [u16; ", scalar @tag_ids, "] = [\n";
printf "    0x%04X, // %s\n", $tag_ids[$_], $tag_names[$_] for 0 .. $#tag_ids;
print "];\n\n";
print "#[rustfmt::skip]\nstatic TAG_TAB: [u16; ", scalar @tag_tabs, "] = [\n";
printf "    %d, // %s\n", $tag_tabs[$_], $tag_names[$_] for 0 .. $#tag_tabs;
print "];\n\n";

emit_blob("/// Tag names with a conversion. Name `n` converts with table `NAME_TAB[n]`:\n"
        . "/// that of the first module to define it, in `Module::decimal id` order.\n",
          'NAMES', 'NAME_END', \@names);
emit_ints('NAME_SLOTS', 16, hash_slots('NAME_SLOTS', @names));
emit_ints('NAME_TAB', 16, @name_tabs);

print "/// Multi-value tags, sorted by (module, tag): (module, tag, first entry of\n";
print "/// `LIST_TABS`, number of elements). Element `i` converts with\n";
print "/// `LIST_TABS[first + i]`.\n";
print "#[rustfmt::skip]\nstatic LISTS: [(u16, u16, u16, u16); ", scalar @lists, "] = [\n";
printf "    (%d, 0x%04X, %d, %d), // %s\n", fit('LISTS', 16, @$_[0 .. 3]), $$_[4] for @lists;
print "];\n\n";
emit_ints('LIST_TABS', 16, @list_tabs);

print <<'RUST';
/// `blob`, checked at build time to be `len` bytes long. The generator counts
/// the offsets into a blob; a string it measured differently from rustc fails
/// the build here rather than slicing in the wrong place.
const fn sized(len: usize, blob: &'static str) -> &'static str {
    assert!(blob.len() == len);
    blob
}

/// Where entry `i` lies, in a list of offsets at which each entry ends.
fn span(ends: &[u32], i: usize) -> core::ops::Range<usize> {
    let start = if i == 0 { 0 } else { ends[i - 1] as usize };
    start..ends[i] as usize
}

/// FNV-1a, 32 bits (the generator computes the same to fill the slots).
fn fnv1a(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The index of `key` in a blob of strings, through its hash slots.
///
/// The generator keeps every slot table at most half full, so a probe meets
/// an empty slot within a few steps. The bound on the steps only turns a
/// corrupt table into a miss instead of an endless loop.
fn find(slots: &[u16], blob: &'static str, ends: &[u32], key: &str) -> Option<usize> {
    let mask = slots.len() - 1;
    let mut s = fnv1a(key) as usize & mask;
    for _ in 0..slots.len() {
        let i = usize::from(slots[s]).checked_sub(1)?;
        if &blob[span(ends, i)] == key {
            return Some(i);
        }
        s = (s + 1) & mask;
    }
    None
}

/// Convert `value` with table `t`.
fn convert(t: u16, value: i64) -> Option<&'static str> {
    let rows = span(&TAB_END, usize::from(t));
    let found = KEYS[rows.clone()].binary_search(&value).ok()?;
    Some(&STRINGS[span(&STR_END, usize::from(VALS[rows.start + found]))])
}

/// Look up print conversion for a tag value.
/// Returns the human-readable string for the given module, tag ID, and numeric value.
pub fn print_conv(module: &str, tag_id: u16, value: i64) -> Option<&'static str> {
    let tags = span(&MOD_END, find(&MODULE_SLOTS, MODULES, &MODULE_END, module)?);
    let i = TAG_IDS[tags.clone()].binary_search(&tag_id).ok()?;
    convert(TAG_TAB[tags.start + i], value)
}

/// Look up print conversion by tag name and numeric value.
pub fn print_conv_by_name(tag_name: &str, value: i64) -> Option<&'static str> {
    let n = find(&NAME_SLOTS, NAMES, &NAME_END, tag_name)?;
    convert(NAME_TAB[n], value)
}

/// The tables of a multi-value tag, one per element.
fn list(module: &str, tag_id: u16) -> Option<&'static [u16]> {
    let m = u16::try_from(find(&MODULE_SLOTS, MODULES, &MODULE_END, module)?).ok()?;
    let i = LISTS
        .binary_search_by(|&(lm, lt, ..)| (lm, lt).cmp(&(m, tag_id)))
        .ok()?;
    let (_, _, first, len) = LISTS[i];
    let first = usize::from(first);
    Some(&LIST_TABS[first..first + usize::from(len)])
}
RUST

# print_conv_str stays a match: 260 arms of string tuples, 9 KB of code.
print "\n/// Look up a print conversion whose key is the whole value, as text.\n";
print "///\n/// Sony's VariableLowPassFilter is `{ '0 0' => 'n/a', '1 0' => 'Off' }` over a\n";
print "/// two-element tag: no number keys it.\n";
print "#[must_use]\n";
print "pub fn print_conv_str(module: &str, tag_id: u16, value: &str) -> Option<&'static str> {\n";
print "    Some(match (module, tag_id, value) {\n";
for my $key (sort keys %all_str_convs) {
    my $info = $all_str_convs{$key};
    my %conv = %{$info->{conv}};
    for my $k (sort keys %conv) {
        my ($kk, $vv) = ($k, $conv{$k});
        for ($kk, $vv) { s/\\/\\\\/g; s/"/\\"/g }
        printf "        (\"%s\", %#06x, \"%s\") => \"%s\", // %s\n",
            $info->{module}, $info->{tag_id}, $kk, $vv, $info->{tag_name};
    }
}
print "        _ => return None,\n    })\n}\n";

print <<'RUST';

/// Look up the print conversion for one element of a multi-value tag.
///
/// ExifTool splits the value on spaces, converts each element with its own
/// hash, and joins the results with `; ` (ExifTool.pm:3550, 3696).
#[must_use]
pub fn print_conv_list(
    module: &str,
    tag_id: u16,
    index: usize,
    value: i64,
) -> Option<&'static str> {
    convert(*list(module, tag_id)?.get(index)?, value)
}

/// How many elements a multi-value tag's print conversion covers.
#[must_use]
pub fn print_conv_list_len(module: &str, tag_id: u16) -> Option<usize> {
    list(module, tag_id).map(<[u16]>::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every module and name must be found through its slots, and everything a
    /// lookup binary-searches must be sorted: a broken table would otherwise
    /// only show as a conversion missing on some file.
    #[test]
    fn every_key_is_reachable() {
        for m in 0..MOD_END.len() {
            let module = &MODULES[span(&MODULE_END, m)];
            assert_eq!(find(&MODULE_SLOTS, MODULES, &MODULE_END, module), Some(m));
            let tags = &TAG_IDS[span(&MOD_END, m)];
            assert!(tags.windows(2).all(|w| w[0] < w[1]), "{module}");
        }
        for n in 0..NAME_TAB.len() {
            let name = &NAMES[span(&NAME_END, n)];
            assert_eq!(find(&NAME_SLOTS, NAMES, &NAME_END, name), Some(n));
        }
        for t in 0..TAB_END.len() {
            let keys = &KEYS[span(&TAB_END, t)];
            assert!(keys.windows(2).all(|w| w[0] < w[1]), "table {t}");
        }
        let lists: Vec<_> = LISTS.iter().map(|&(m, tag, ..)| (m, tag)).collect();
        assert!(lists.windows(2).all(|w| w[0] < w[1]));
    }

    /// A probe through slots that are all taken ends as a miss.
    #[test]
    fn find_gives_up_on_a_full_table() {
        assert_eq!(find(&[1, 1, 1, 1], "a", &[1], "a"), Some(0));
        assert_eq!(find(&[1, 1, 1, 1], "a", &[1], "b"), None);
    }
}
RUST

if (@dropped) {
    warn sprintf("%d conversion(s) not emitted:\n", scalar @dropped);
    warn "  $_\n" for @dropped;
}
warn "Extracted $total_tags tags with $total_entries entries from " . scalar(@pm_files) . " files\n";
warn sprintf("plus %d string-keyed entries over %d tags\n",
             $total_str_entries, scalar keys %all_str_convs);
warn sprintf("pool: %d strings (%d bytes), %d tables (%d rows), %d modules, %d tags, %d names, %d lists\n",
             scalar @strings, bytes::length(join '', @strings), scalar @tables, $n_rows,
             scalar @modules, scalar @tag_ids, scalar @names, scalar @lists);
