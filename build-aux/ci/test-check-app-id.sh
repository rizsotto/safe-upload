#!/bin/sh
# Self test for check-app-id.sh.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
check="$here/check-app-id.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

id=$(sed -n 's/^pub const APP_ID: &str = "\([^"]*\)";$/\1/p' "$repo/crates/app/src/config.rs")

# A fresh copy of the files the check reads.
copy() {
    rm -rf "$tmp/r"
    mkdir -p "$tmp/r/crates/app/src" "$tmp/r/build-aux/flatpak"
    cp "$repo/crates/app/src/config.rs" "$tmp/r/crates/app/src/"
    cp -R "$repo/data" "$tmp/r/data"
}

fail=0
expect() {
    want=$1
    what=$2
    if "$check" "$tmp/r" >/dev/null 2>&1; then got=0; else got=$?; fi
    if [ "$got" != "$want" ]; then
        echo "FAIL: $what exited $got, want $want" >&2
        fail=1
    fi
}

copy
expect 0 "the repo files"

copy
sed -i 's/APP_ID: &str = ".*"/APP_ID: \&str = "org.example.Other"/' "$tmp/r/crates/app/src/config.rs"
expect 1 "another APP_ID in config.rs"

copy
sed -i "s/^Icon=.*/Icon=org.example.Other/" "$tmp/r/data/$id.desktop"
expect 1 "another Icon in the desktop file"

copy
sed -i "s|<id>.*</id>|<id>org.example.Other</id>|" "$tmp/r/data/$id.metainfo.xml"
expect 1 "another id in the metainfo"

copy
sed -i "s|<schema id=\"[^\"]*\"|<schema id=\"org.example.Other\"|" "$tmp/r/data/$id.gschema.xml"
expect 1 "another schema id"

copy
rm "$tmp/r/data/icons/hicolor/scalable/apps/$id.svg"
expect 1 "a missing icon"

copy
cp "$tmp/r/data/$id.desktop" "$tmp/r/data/org.example.Other.desktop"
expect 1 "a data file named after another ID"

copy
printf '{\n  "id": "%s"\n}\n' "$id" >"$tmp/r/build-aux/flatpak/$id.json"
echo '[]' >"$tmp/r/build-aux/flatpak/cargo-sources.json"
expect 0 "a matching Flatpak manifest"

copy
printf '{\n  "id": "org.example.Other"\n}\n' >"$tmp/r/build-aux/flatpak/$id.json"
expect 1 "a Flatpak manifest with another id"

copy
printf '{\n  "id": "%s"\n}\n' "$id" >"$tmp/r/build-aux/flatpak/org.example.Other.json"
expect 1 "a Flatpak manifest with another file name"

copy
rm "$tmp/r/crates/app/src/config.rs"
expect 2 "a missing config.rs"

[ "$fail" = 0 ] || exit 1
echo "check-app-id self test passed"
