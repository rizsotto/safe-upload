#!/bin/sh
# Checks that make.sh built every fixture with the data the tests rely on.
# Usage: check.sh [OUTDIR]   (default: target/fixtures in the repo)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
dir=${1:-"$here/../../target/fixtures"}
cd "$dir"

fail=0
bad() { echo "FAIL: $*" >&2; fail=1; }

# has FILE TAG...: exiftool finds each tag (group:name, -a so nothing hides).
has() {
    f=$1; shift
    for t in "$@"; do
        [ -n "$(exiftool -a -s3 -"$t" "$f")" ] || bad "$f: no $t"
    done
}

chunk() { LC_ALL=C grep -q -a "$2" "$1" || bad "$1: no $2 chunk"; }

has exif-gps-thumb.jpg GPS:GPSLatitude GPS:GPSLongitude IFD0:Make \
    ExifIFD:MakerNoteUnknownText ExifIFD:DateTimeOriginal \
    XMP-dc:Creator IPTC:By-line IFD1:ThumbnailImage

# The thumbnail must show a different image: green, while the main image
# is a red to blue gradient with no green.
thumb=$(mktemp)
exiftool -b -ThumbnailImage exif-gps-thumb.jpg >"$thumb"
thumb_green=$(magick "$thumb" -format '%[fx:mean.g > 0.8 && mean.r < 0.2]' info: || true)
rm -f "$thumb"
main_green=$(magick exif-gps-thumb.jpg -format '%[fx:mean.g < 0.1]' info: || true)
[ "$thumb_green" = 1 ] || bad "exif-gps-thumb.jpg: thumbnail is not green"
[ "$main_green" = 1 ] || bad "exif-gps-thumb.jpg: main image has green"

for n in 1 2 3 4 5 6 7 8; do
    f=orient-$n.jpg
    [ "$(exiftool -n -s3 -Orientation "$f")" = "$n" ] || bad "$f: orientation is not $n"
    d=$(magick "$f" -auto-orient orient-reference.png -metric RMSE \
        -format '%[distortion]' -compare info: || echo 1)
    awk -v d="$d" 'BEGIN { exit !(d < 0.02) }' || bad "$f: does not show orient-reference.png when oriented"
done

for c in tEXt zTXt iTXt eXIf; do chunk png-text.png "$c"; done
has png-text.png PNG:Comment PNG:Description XMP-dc:Creator GPS:GPSLatitude

[ "$(magick alpha.png -format '%[opaque]' info:)" = False ] || bad "alpha.png: no transparency"

[ "$(wc -c <heif-header.heic)" -eq 24 ] || bad "heif-header.heic: not 24 bytes"
[ "$(head -c 12 heif-header.heic | tail -c 8)" = ftypheic ] || bad "heif-header.heic: no ftypheic at byte 4"

[ "$(exiftool -s3 -ICC_Profile:ProfileDescription p3.jpg)" = "Display P3" ] || bad "p3.jpg: no Display P3 profile"

[ "$(exiftool -s3 -ImageWidth -ImageHeight huge-dims.jpg | tr '\n' x)" = 20000x20000x ] || bad "huge-dims.jpg: not 20000x20000"

for f in truncated.jpg truncated.png; do
    [ -s "$f" ] || bad "$f: empty"
done
[ "$(head -c 2 truncated.jpg | od -An -tx1 | tr -d ' ')" = ffd8 ] || bad "truncated.jpg: no SOI"
[ "$(tail -c 2 truncated.jpg | od -An -tx1 | tr -d ' ')" != ffd9 ] || bad "truncated.jpg: ends with EOI"
if magick truncated.png info: >/dev/null 2>&1; then bad "truncated.png: still decodes"; fi
# The cut must be inside IDAT, after all chunks before it are whole.
perl -0777 -ne '
    my $i = 8;
    while ($i + 8 <= length) {
        my ($len, $type) = unpack "Na4", substr($_, $i, 8);
        if ($type eq "IDAT") { exit($i + 12 + $len > length ? 0 : 1) }
        $i += 12 + $len;
    }
    exit 1;
' truncated.png || bad "truncated.png: not cut inside IDAT"
for f in rgb.webp alpha.webp; do
    [ "$(head -c 4 "$f")$(head -c 12 "$f" | tail -c 4)" = RIFFWEBP ] || bad "$f: not WebP"
done
for f in rgb.avif alpha.avif p3-nclx.avif; do
    [ "$(head -c 12 "$f" | tail -c 8)" = ftypavif ] || bad "$f: no ftypavif at byte 4"
done
[ "$(magick alpha.webp -format '%[opaque]' info:)" = False ] || bad "alpha.webp: no transparency"
avif_info() { avifdec --info "$1" 2>/dev/null | sed -n "s/^ \* $2 *: //p"; }
[ "$(avif_info alpha.avif Alpha)" != Absent ] || bad "alpha.avif: no alpha"
[ "$(avif_info rgb.avif Alpha)" = Absent ] || bad "rgb.avif: has alpha"
p3=$(avif_info p3-nclx.avif 'Color Primaries')/$(avif_info p3-nclx.avif 'Transfer Char.')/$(avif_info p3-nclx.avif 'Matrix Coeffs.')
[ "$p3" = 12/13/0 ] || bad "p3-nclx.avif: CICP is $p3, not 12/13/0"
[ "$(avif_info p3-nclx.avif 'ICC Profile')" = Absent ] || bad "p3-nclx.avif: has an ICC profile"
[ -f .safe-upload-fixtures ] || bad "no .safe-upload-fixtures marker"

[ "$fail" = 0 ] && echo "fixtures: all checks passed"
exit "$fail"
