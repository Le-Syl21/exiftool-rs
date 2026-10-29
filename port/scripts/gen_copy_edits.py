"""Writer.pl's SetNewValuesFromFile as CopyNewValues, whose SetNewValue calls
record their arguments: the Perl the facade (src/api/engine.rs) uses to turn
a copy into edits for a format extension.

usage: gen_copy_edits.py EXIFTOOL_LIB/Image/ExifTool/Writer.pl OUT
Rerun on each ExifTool upgrade and paste OUT over CopyNewValues in engine.rs.
"""
import re, sys
src = open(sys.argv[1]).read()
# the sub, from its line to the first closing brace at column 0
m = re.search(r'^sub SetNewValuesFromFile\(\$\$;@\)\n.*?^\}\n', src, re.M | re.S)
assert m, 'SetNewValuesFromFile not found'
text = m.group(0).rstrip('\n')
text = text.replace('sub SetNewValuesFromFile($$;@)', 'sub CopyNewValues')
# the three SetNewValue calls, recorded
text = text.replace("my ($n, $e) = $self->SetNewValue($tag, $$info{$tag}, %opts);",
                    "my ($n, $e) = Record($srcExifTool, $tag, $tag, $$info{$tag}, %opts);")
text = text.replace("my @rtnVals = $self->SetNewValue($dstTag, $val, %$opts);",
                    "my @rtnVals = Record($srcExifTool, undef, $dstTag, $val, %$opts);")
text = text.replace("my ($rtn, $wrn) = $self->SetNewValue($dstTag, $val, %$opts);",
                    "my ($rtn, $wrn) = Record($srcExifTool, $tag, $dstTag, $val, %$opts);")
assert text.count('Record(') == 3, text.count('Record(')
assert 'SetNewValue(' not in text.replace('SetNewValuesFromFile', '')
for f in ['ExpandShortcuts', 'ValidTagName', 'IsSameID', 'GetTagName', 'NextFreeTagKey']:
    text = re.sub(r'(?<![\w:>])' + f + r'\(', 'Image::ExifTool::' + f + '(', text)
text = text.replace('Image::ExifTool->new', 'Image::ExifTool->new')
# the code is in a Rust raw string r#"..."#
assert '"#' not in text
open(sys.argv[2], 'w').write(text + '\n')
