#!/bin/sh
# Regenerates cargo-sources.json from Cargo.lock with flathub's
# flatpak-cargo-generator, so the Flatpak build needs no network.
# With --check, fails if cargo-sources.json does not match Cargo.lock.
# Needs python3 with aiohttp and tomlkit, and curl.
# Usage: update-cargo-sources.sh [--check]
set -eu

# flatpak-builder-tools, cargo/flatpak-cargo-generator.py. Bump both together.
rev=41c20aa10819cdb2a4f3ca171758a96d1955c018
sha256=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out="$here/cargo-sources.json"
check=
case ${1:-} in
--check) check=1 ;;
"") ;;
*) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/gen"
curl -fsSL -o "$tmp/gen/flatpak-cargo-generator.py" \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$rev/cargo/flatpak-cargo-generator.py"
echo "$sha256  $tmp/gen/flatpak-cargo-generator.py" | sha256sum -c --quiet -

# -P, not -I: the flathub image finds aiohttp and tomlkit via PYTHONPATH.
python3 -P "$tmp/gen/flatpak-cargo-generator.py" "$root/Cargo.lock" -o "$tmp/cargo-sources.json"

if [ -n "$check" ]; then
    if ! cmp -s "$tmp/cargo-sources.json" "$out"; then
        echo "cargo-sources: $out does not match Cargo.lock; run $0" >&2
        diff -u "$out" "$tmp/cargo-sources.json" | head -40 >&2 || true
        exit 1
    fi
    echo "cargo-sources: matches Cargo.lock"
else
    mv "$tmp/cargo-sources.json" "$out"
    echo "cargo-sources: wrote $out"
fi
