#!/bin/sh
# Builds the M1 test fixtures (SPEC section 13). Nothing here is committed;
# tests and CI run this script. Needs ImageMagick 7, exiftool and perl.
# Usage: make.sh [OUTDIR]   (default: target/fixtures in the repo)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/../../target/fixtures"}
mkdir -p "$(dirname "$out")"
out=$(cd "$(dirname "$out")" && pwd)/$(basename "$out")
# The old output is deleted below. Only ever delete a folder this script
# made, never one that happens to be passed by mistake.
marker=.safe-upload-fixtures
if [ -e "$out" ] && [ ! -f "$out/$marker" ]; then
    echo "make.sh: $out exists and is not a fixture folder (no $marker). Refusing to replace it." >&2
    exit 1
fi
tmp=$(mktemp -d "$out.tmp.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"

et() { exiftool -q -q -overwrite_original "$@"; }
mk() { magick "$@"; }

# ICC v2 Display P3 profile: D50 colorants, sRGB tone curve. Written by hand
# because Fedora ships no P3 profile.
perl -e '
    sub s15 { pack "N", int($_[0] * 65536 + ($_[0] < 0 ? -0.5 : 0.5)) & 0xffffffff }
    sub xyz { "XYZ \0\0\0\0" . join "", map { s15($_) } @_ }
    sub pad { my $d = shift; $d . "\0" x ((4 - length($d) % 4) % 4) }
    my $name = "Display P3";
    my $desc = "desc\0\0\0\0" . pack("N", length($name) + 1) . "$name\0"
        . pack("NN", 0, 0) . pack("nC", 0, 0) . "\0" x 67;
    my $cprt = "text\0\0\0\0safe-upload test fixture\0";
    my @curve = map {
        my $v = $_ / 1023;
        my $l = $v <= 0.04045 ? $v / 12.92 : (($v + 0.055) / 1.055) ** 2.4;
        int($l * 65535 + 0.5)
    } 0 .. 1023;
    my $trc = "curv\0\0\0\0" . pack("N", 1024) . pack("n*", @curve);
    my @tags = (
        [desc => $desc],
        [cprt => $cprt],
        [wtpt => xyz(0.9642, 1.0, 0.8249)],
        [rXYZ => xyz(0.515102, 0.241196, -0.001053)],
        [gXYZ => xyz(0.291965, 0.692245, 0.041885)],
        [bXYZ => xyz(0.157103, 0.066560, 0.784088)],
        [rTRC => $trc],
    );
    my $off = 128 + 4 + 12 * (@tags + 2);
    my ($table, $data) = ("", "");
    for my $t (@tags) {
        my ($sig, $d) = @$t;
        $table .= pack("a4NN", $sig, $off + length($data), length($d));
        $trc_at = $off + length($data) if $sig eq "rTRC";
        $data .= pad($d);
    }
    $table .= pack("a4NN", $_, $trc_at, length($trc)) for qw(gTRC bTRC);
    my $body = pack("N", @tags + 2) . $table . $data;
    my $size = 128 + length($body);
    my $hdr = pack("N", $size) . "\0" x 4 . pack("N", 0x02100000)
        . "mntrRGB XYZ " . pack("n6", 2026, 1, 1, 0, 0, 0) . "acsp"
        . "\0" x 24 . pack("N", 0) . s15(0.9642) . s15(1.0) . s15(0.8249)
        . "\0" x 48;
    print $hdr . $body;
' >display-p3.icc

# 1. JPEG with GPS, MakerNote, XMP, IPTC and a thumbnail that shows a
#    different image (green) than the main image (red to blue).
mk -size 640x480 gradient:red-blue -strip -quality 90 exif-gps-thumb.jpg
mk -size 160x120 xc:lime -strip -quality 90 thumb-src.jpg
printf 'SECRET-MAKERNOTE-0123456789' >makernote.bin
et "-ThumbnailImage<=thumb-src.jpg" "-MakerNoteUnknownText<=makernote.bin" \
    -Make=FixtureCam -Model=FixtureModel -SerialNumber=SN12345 \
    -DateTimeOriginal="2026:04:12 18:33:01" \
    -GPSLatitude=47.5 -GPSLatitudeRef=N -GPSLongitude=19.04 -GPSLongitudeRef=E \
    -XMP-dc:Creator="Fixture Person" -XMP-photoshop:City=Budapest \
    -IPTC:By-line="Fixture Person" -IPTC:City=Budapest \
    exif-gps-thumb.jpg

# 2. Orientation 1 to 8. The stored pixels are the inverse transform of the
#    upright reference, so a correct reader shows orient-reference.png.
mk -size 48x32 xc:red -size 48x32 xc:lime +append \
    \( -size 48x32 xc:blue -size 48x32 xc:white +append \) -append \
    -strip PNG24:orient-reference.png
for n in 1 2 3 4 5 6 7 8; do
    case $n in
        1) op= ;;
        2) op=-flop ;;
        3) op="-rotate 180" ;;
        4) op=-flip ;;
        5) op=-transpose ;;
        6) op="-rotate 270" ;;
        7) op=-transverse ;;
        8) op="-rotate 90" ;;
    esac
    # shellcheck disable=SC2086
    mk orient-reference.png $op -strip -quality 95 "orient-$n.jpg"
    et -n -Orientation="$n" "orient-$n.jpg"
done

# 3. PNG with tEXt, zTXt, iTXt and eXIf chunks.
mk -size 64x48 gradient:yellow-purple -strip PNG24:png-text.png
et -PNG:Comment="Fixture comment" -PNG:Author="Fixture Person" png-text.png
# exiftool only compresses text that gets smaller, so repeat it.
et -z -PNG:Description="$(printf 'Fixture compressed description. %.0s' 1 2 3 4 5 6 7 8)" png-text.png
et -XMP-dc:Creator="Fixture Person" png-text.png
et -EXIF:Make=FixtureCam -EXIF:GPSLatitude=47.5 -EXIF:GPSLatitudeRef=N png-text.png

# 4. PNG with transparency.
mk -size 64x64 xc:none -fill red -draw "circle 32,32 32,8" -strip PNG32:alpha.png

# 5. HEIF header only: one 24 byte ftyp box, major brand heic, compatible
#    brands mif1 and heic (decision 0001). No HEVC encoder needed.
printf '\000\000\000\030ftypheic\000\000\000\000mif1heic' >heif-header.heic

# 6. Display P3 JPEG, profile attached, pixels not converted. Left half
#    P3 red, right half the mid-tone rgb(200,120,80), which is sRGB
#    rgb(213,115,70): pure red clips to sRGB red, the mid-tone does not.
mk -size 32x64 xc:"rgb(255,0,0)" -size 32x64 xc:"rgb(200,120,80)" +append \
    -strip -quality 95 p3.jpg
et "-ICC_Profile<=display-p3.icc" p3.jpg

# 7. Huge image: a real JPEG whose SOF header claims 20000x20000 (400 MP).
mk -size 64x64 xc:gray -strip -quality 90 huge-dims.jpg
perl -0777 -pi -e '
    my $i = index($_, "\xff\xc0");
    die "no SOF0" if $i < 0;
    substr($_, $i + 5, 4) = pack("nn", 20000, 20000);
' huge-dims.jpg

# 8. Truncated files. JPEG: the first half of a valid file. PNG: cut in
#    the middle of the IDAT data, so every chunk before it is whole.
size=$(wc -c <exif-gps-thumb.jpg)
head -c $((size / 2)) exif-gps-thumb.jpg >truncated.jpg
mk -size 256x256 gradient:red-blue -strip PNG24:png-src.png
perl -0777 -ne '
    my $i = index($_, "IDAT") - 4;
    die "no IDAT" if $i < 0;
    my $len = unpack "N", substr($_, $i, 4);
    print substr($_, 0, $i + 8 + int($len / 2));
' png-src.png >truncated.png

# 9. Real WebP and AVIF, with and without alpha. Check the magic bytes:
#    ImageMagick on stock Fedora silently writes PNG bytes to x.avif, so
#    AVIF comes from avifenc (libavif-tools).
mk -size 64x48 gradient:yellow-purple -strip -quality 90 rgb.webp
mk alpha.png -strip -quality 90 alpha.webp
mk -size 64x48 gradient:yellow-purple -strip PNG24:rgb-src.png
avifenc -s 10 rgb-src.png rgb.avif >/dev/null
avifenc -s 10 alpha.png alpha.avif >/dev/null
# P3 described only by CICP (nclx: primaries 12 Display P3, transfer 13
# sRGB, matrix 0 identity for lossless), no ICC profile. Left half
# rgb(255,0,0), right half rgb(200,120,80), as P3 values.
mk -size 32x32 xc:"rgb(255,0,0)" -size 32x32 xc:"rgb(200,120,80)" +append \
    -strip PNG24:p3-src.png
avifenc -s 10 --lossless --cicp 12/13/0 --ignore-icc p3-src.png p3-nclx.avif >/dev/null
for f in rgb.webp alpha.webp; do
    [ "$(head -c 4 "$f")$(head -c 12 "$f" | tail -c 4)" = RIFFWEBP ] || { echo "make.sh: $f is not WebP" >&2; exit 1; }
done
for f in rgb.avif alpha.avif p3-nclx.avif; do
    [ "$(head -c 12 "$f" | tail -c 8)" = ftypavif ] || { echo "make.sh: $f is not AVIF" >&2; exit 1; }
done

rm -f thumb-src.jpg makernote.bin png-src.png rgb-src.png p3-src.png
touch "$marker"
cd /
rm -rf "$out"
mv "$tmp" "$out"
trap - EXIT
echo "fixtures: built in $out"
