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
# has FILE LINE...: FILE has a line equal to one of the LINEs, apart from
# leading and trailing blanks. Exact, so a longer ID like "$id.Devel" fails.
has() {
    file=$1
    shift
    if [ ! -r "$file" ]; then
        bad "missing $file"
    elif ! sed 's/^[[:space:]]*//; s/[[:space:]]*$//' "$file" |
        grep -qxF "$(printf '%s\n' "$@")"; then
        bad "$file has no line: $1"
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

files=$(mktemp)
trap 'rm -f "$files"' EXIT

find "$data" -type f ! -name .gitkeep >"$files"
while IFS= read -r f; do
    case $(basename "$f") in
    "$id.desktop" | "$id.metainfo.xml" | "$id.gschema.xml" | "$id.svg") ;;
    *) bad "$f is not a known data file named after $id" ;;
    esac
done <"$files"

if [ -d "$root/build-aux/flatpak" ]; then
    find "$root/build-aux/flatpak" -maxdepth 1 -name '*.json' \
        ! -name cargo-sources.json >"$files"
    while IFS= read -r f; do
        [ "$(basename "$f")" = "$id.json" ] || bad "$f is not named $id.json"
        has "$f" "\"id\": \"$id\"," "\"id\": \"$id\""
    done <"$files"
fi

[ "$fail" = 0 ] || exit 1
echo "app-id: $id is used consistently"
