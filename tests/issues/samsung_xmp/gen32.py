"""Write the issue #32 XMP fixtures: a MicrosoftPhoto Rating declared under
`http://ns.microsoft.com/photo/1.0` (ExifTool's own spelling of the URI) and
under the same URI with a trailing slash, which ExifTool fixes."""
from pathlib import Path

TEMPLATE = (
    '<x:xmpmeta xmlns:x="adobe:ns:meta/">'
    '<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
    '<rdf:Description xmlns:MicrosoftPhoto="{uri}">'
    "<MicrosoftPhoto:Rating>{rating}</MicrosoftPhoto:Rating>"
    "</rdf:Description></rdf:RDF></x:xmpmeta>\n"
)
URI = "http://ns.microsoft.com/photo/1.0"

root = Path(__file__).parent
for name, uri, rating in [
    ("percent75.xmp", URI, 75),
    ("percent3.xmp", URI, 3),
    ("trailing-slash.xmp", URI + "/", 75),
]:
    (root / name).write_text(TEMPLATE.format(uri=uri, rating=rating), newline="\n")
