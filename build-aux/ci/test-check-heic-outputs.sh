#!/bin/sh
# Self test for check-heic-outputs.sh, on JPEGs made with ImageMagick.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
check="$here/check-heic-outputs.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# good DIR: a clean gps.jpg and an upright rot90.jpg (red top right).
good() {
    mkdir -p "$1"
    magick -size 640x480 gradient:red-blue -strip "$1/gps.jpg"
    magick -size 480x640 xc:blue -fill red -draw "rectangle 240,0 479,319" \
        -strip "$1/rot90.jpg"
}

fail=0
expect() {
    want=$1
    shift
    if "$check" "$@" >/dev/null 2>&1; then got=0; else got=$?; fi
    if [ "$got" != "$want" ]; then
        echo "FAIL: $* exited $got, want $want" >&2
        fail=1
    fi
}

good "$tmp/ok"
expect 0 "$tmp/ok"

good "$tmp/gps"
exiftool -q -overwrite_original -GPSLatitude=47.5 -GPSLatitudeRef=N "$tmp/gps/gps.jpg"
expect 1 "$tmp/gps"

good "$tmp/make"
exiftool -q -overwrite_original -Make=FixtureCam "$tmp/make/rot90.jpg"
expect 1 "$tmp/make"

# Not rotated: 640x480.
good "$tmp/sideways"
magick -size 640x480 xc:blue -fill red -draw "rectangle 0,0 319,239" -strip "$tmp/sideways/rot90.jpg"
expect 1 "$tmp/sideways"

# Rotated twice (irot and EXIF Orientation): red bottom right.
good "$tmp/twice"
magick -size 480x640 xc:blue -fill red -draw "rectangle 240,320 479,639" -strip "$tmp/twice/rot90.jpg"
expect 1 "$tmp/twice"

good "$tmp/png"
magick "$tmp/png/gps.jpg" "PNG:$tmp/png/gps.jpg"
expect 1 "$tmp/png"

good "$tmp/missing"
rm "$tmp/missing/gps.jpg"
expect 1 "$tmp/missing"

expect 2

[ "$fail" = 0 ] && echo "heic-outputs self test: ok"
exit "$fail"
