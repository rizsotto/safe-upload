#!/bin/sh
# Self test for make.sh: without the x265 encoder it fails with a clear
# message, before it writes anything.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir "$tmp/bin"
printf '#!/bin/sh\necho "HEIC encoders:"\n' >"$tmp/bin/heif-enc"
chmod +x "$tmp/bin/heif-enc"

fail=0
if PATH="$tmp/bin:$PATH" "$here/make.sh" "$tmp/out" >"$tmp/log" 2>&1; then
    echo "FAIL: make.sh succeeded without the x265 encoder" >&2
    fail=1
fi
grep -q 'no x265 encoder' "$tmp/log" || { echo "FAIL: make.sh message does not name x265" >&2; fail=1; }
[ ! -e "$tmp/out" ] || { echo "FAIL: make.sh wrote output without the x265 encoder" >&2; fail=1; }

[ "$fail" = 0 ] && echo "make.sh self test: ok"
exit "$fail"
