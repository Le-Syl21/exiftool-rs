#!/usr/bin/perl
# Extract ALL PrintConv hash tables from ExifTool Perl source.
# Generates Rust code with lookup functions.
#
# Usage: perl scripts/gen_print_conv.pl /path/to/exiftool/lib > src/tags/print_conv_generated.rs

use strict;
use warnings;
use File::Find;
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

    # Find all tag definitions with PrintConv hashes
    while ($content =~ /
        (0x[0-9a-fA-F]+|[\d]+)\s*=>\s*\{   # tag ID
        ((?:[^{}]|\{(?:[^{}]|\{[^{}]*\})*\})*)  # tag body (nested braces)
        \}/gx) {

        my $tag_hex = $1;
        my $body = $2;

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

# Generate a single lookup function
print "/// Look up print conversion for a tag value.\n";
print "/// Returns the human-readable string for the given module, tag ID, and numeric value.\n";
print "pub fn print_conv(module: &str, tag_id: u16, value: i64) -> Option<&'static str> {\n";
print "    match (module, tag_id) {\n";

for my $module (sort keys %by_module) {
    my @tags = @{$by_module{$module}};
    for my $tag (sort { $a->{tag_id} <=> $b->{tag_id} } @tags) {
        my %conv = %{$tag->{conv}};
        my @keys = sort { $a <=> $b } keys %conv;

        # A table of one entry is not a lookup to choose from. Everything
        # else is emitted however long it is: the lens lists run to several
        # hundred entries, and capping them at a hundred is why LensType
        # printed its raw id.
        if (scalar @keys < 2) {
            push @dropped, sprintf("%s 0x%04X %s: only %d entry",
                                   $module, $tag->{tag_id}, $tag->{tag_name}, scalar @keys);
            next;
        }

        printf "        (\"%s\", 0x%04X) => match value { // %s\n",
            $module, $tag->{tag_id}, $tag->{tag_name};

        for my $k (@keys) {
            my $v = $conv{$k};
            $v =~ s/\\/\\\\/g;
            $v =~ s/"/\\"/g;
            printf "            %d => Some(\"%s\"),\n", $k, $v;
        }
        print "            _ => None,\n";
        print "        },\n";
    }
}

print "        _ => None,\n";
print "    }\n";
print "}\n\n";

# Also generate a module-agnostic lookup by tag name
print "/// Look up print conversion by tag name and numeric value.\n";
print "pub fn print_conv_by_name(tag_name: &str, value: i64) -> Option<&'static str> {\n";
print "    match tag_name {\n";

my %by_name;
for my $key (sort keys %all_convs) {
    my $info = $all_convs{$key};
    my %conv = %{$info->{conv}};
    next if scalar(keys %conv) < 2;
    $by_name{$info->{tag_name}} = $info unless exists $by_name{$info->{tag_name}};
}

for my $name (sort keys %by_name) {
    my $tag = $by_name{$name};
    my %conv = %{$tag->{conv}};
    my @keys = sort { $a <=> $b } keys %conv;

    printf "        \"%s\" => match value {\n", $name;
    for my $k (@keys) {
        my $v = $conv{$k};
        $v =~ s/\\/\\\\/g;
        $v =~ s/"/\\"/g;
        printf "            %d => Some(\"%s\"),\n", $k, $v;
    }
    print "            _ => None,\n";
    print "        },\n";
}

print "        _ => None,\n";
print "    }\n";
print "}\n";

# ── The conversions keyed by the value as a string ──────────────────────────
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

if (@dropped) {
    warn sprintf("%d conversion(s) not emitted:\n", scalar @dropped);
    warn "  $_\n" for @dropped;
}
warn "Extracted $total_tags tags with $total_entries entries from " . scalar(@pm_files) . " files\n";
warn sprintf("plus %d string-keyed entries over %d tags\n",
             $total_str_entries, scalar keys %all_str_convs);
