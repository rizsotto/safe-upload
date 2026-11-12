#!/bin/sh
# Checks the JPEGs that build-aux/flatpak/test-heic.sh made in the Flatpak
# from the HEIC fixtures: JPEG, under 5 MB, only allowed exiftool keys (the
# list in crates/cli/tests/leak.rs), and rot90.jpg upright: 480x640 with the
# red quadrant top right (glycin applies the irot box, not EXIF Orientation).
# Usage: check-heic-outputs.sh DIR   (with gps.jpg and rot90.jpg)
set -eu

[ "$#" -eq 1 ] || { echo "usage: $0 DIR" >&2; exit 2; }
dir=$1

# Same keys as ALLOWED in crates/cli/tests/leak.rs (JPEG groups).
allowed='
ExifTool:ExifToolVersion
System:FileName
System:Directory
System:FileSize
System:FileModifyDate
System:FileAccessDate
System:FileInodeChangeDate
System:FilePermissions
File:FileType
File:FileTypeExtension
File:MIMEType
File:ImageWidth
File:ImageHeight
File:EncodingProcess
File:BitsPerSample
File:ColorComponents
File:YCbCrSubSampling
JFIF:JFIFVersion
JFIF:ResolutionUnit
JFIF:XResolution
JFIF:YResolution
Composite:ImageSize
Composite:Megapixels
'

fail=0
bad() { echo "heic-outputs: FAIL: $*" >&2; fail=1; }

# color FILE X Y: red, blue or other at that pixel.
color() {
    magick "$1" -format "%[fx:p{$2,$3}.r > 0.6 && p{$2,$3}.b < 0.4 ? 1 : (p{$2,$3}.b > 0.6 && p{$2,$3}.r < 0.4 ? 2 : 0)]" info: |
        sed 's/^1$/red/; s/^2$/blue/; s/^0$/other/'
}

for name in gps rot90; do
    f="$dir/$name.jpg"
    [ -r "$f" ] || { bad "missing $f"; continue; }
    [ "$(head -c 3 "$f" | od -An -tx1 | tr -d ' \n')" = ffd8ff ] || bad "$f: not a JPEG"
    [ "$(wc -c <"$f")" -lt 5000000 ] || bad "$f: not under 5 MB"
    keys=$(exiftool -a -u -G1 -s "$f" | sed -n 's/^\[\([^]]*\)\] *\([^ ]*\).*/\1:\2/p')
    [ -n "$keys" ] || bad "$f: exiftool printed nothing"
    for k in $keys; do
        printf '%s' "$allowed" | grep -qxF "$k" || bad "$f: exiftool shows $k"
    done
done

if [ -r "$dir/gps.jpg" ]; then
    [ "$(magick identify -format %wx%h "$dir/gps.jpg")" = 640x480 ] || bad "gps.jpg: not 640x480"
fi
if [ -r "$dir/rot90.jpg" ]; then
    f="$dir/rot90.jpg"
    [ "$(magick identify -format %wx%h "$f")" = 480x640 ] || bad "rot90.jpg: not 480x640"
    [ "$(color "$f" 360 160)" = red ] || bad "rot90.jpg: top right is not red"
    [ "$(color "$f" 120 160)" = blue ] || bad "rot90.jpg: top left is not blue"
    [ "$(color "$f" 360 480)" = blue ] || bad "rot90.jpg: bottom right is not blue"
fi

[ "$fail" = 0 ] || exit 1
echo "heic-outputs: gps.jpg and rot90.jpg are clean, upright JPEGs under 5 MB"
