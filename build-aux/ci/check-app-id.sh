#!/bin/sh
# Fails if the app ID in crates/app/src/config.rs and the IDs in data/ and
# the Flatpak manifest disagree, so renaming the placeholder (SPEC 16.1) is
# a search and replace plus file moves.
# Usage: check-app-id.sh [REPO_ROOT]
set -eu

root=${1:-.}
config="$root/crates/app/src/config.rs"
[ -r "$config" ] || { echo "app-id: cannot read $config" >&2; exit 2; }
id=$(sed -n 's/^pub const APP_ID: &str = "\([^"]*\)";$/\1/p' "$config")
[ -n "$id" ] || { echo "app-id: no APP_ID in $config" >&2; exit 2; }

fail=0
bad() {
    echo "app-id: $*" >&2
    fail=1
}
has() {
    file=$1
    text=$2
    if [ ! -r "$file" ]; then
        bad "missing $file"
    elif ! grep -qF -- "$text" "$file"; then
        bad "$file does not contain: $text"
    fi
}

data="$root/data"
has "$data/$id.desktop" "Icon=$id"
has "$data/$id.metainfo.xml" "<id>$id</id>"
has "$data/$id.metainfo.xml" "<launchable type=\"desktop-id\">$id.desktop</launchable>"
path=/$(printf '%s' "$id" | tr . /)/
has "$data/$id.gschema.xml" "<schema id=\"$id\" path=\"$path\">"
[ -r "$data/icons/hicolor/scalable/apps/$id.svg" ] ||
    bad "missing $data/icons/hicolor/scalable/apps/$id.svg"

for f in $(find "$data" -type f ! -name .gitkeep); do
    case $(basename "$f") in
    "$id".*) ;;
    *) bad "$f is not named after $id" ;;
    esac
done

for f in $(find "$root/build-aux/flatpak" -maxdepth 1 -name '*.json' ! -name cargo-sources.json 2>/dev/null); do
    [ "$(basename "$f")" = "$id.json" ] || bad "$f is not named $id.json"
    grep -qE "\"id\": *\"$id\"" "$f" || bad "$f does not have \"id\": \"$id\""
done

[ "$fail" = 0 ] || exit 1
echo "app-id: $id is used consistently"
