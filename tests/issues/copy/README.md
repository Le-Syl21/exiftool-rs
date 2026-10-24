Fixtures of issue #36, synthetic: two FFmpeg test patterns, the source
tagged by ExifTool 13.59.

```sh
ffmpeg -y -f lavfi -i testsrc=size=48x32:rate=1 -frames:v 1 source.jpg
ffmpeg -y -f lavfi -i testsrc=size=32x24:rate=1 -frames:v 1 target.jpg
exiftool -overwrite_original -EXIF:Make="Fixture Camera" -EXIF:Model="Fixture Model" \
  -EXIF:LensModel="Fixture 35mm" -EXIF:DateTimeOriginal="2026:01:02 03:04:05" \
  -EXIF:ExposureTime=1/125 -EXIF:FNumber=2.8 -EXIF:ISO=400 -EXIF:FocalLength=35 \
  -EXIF:GPSLatitude=12.5 -EXIF:GPSLatitudeRef=N -EXIF:GPSLongitude=45.25 \
  -EXIF:GPSLongitudeRef=W -EXIF:GPSAltitude=10 -EXIF:GPSAltitudeRef#=0 source.jpg
```
