#!/bin/sh
# Self test for check-flatpak-permissions.sh.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
check="$here/check-flatpak-permissions.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# manifest NAME ARG...: a manifest with these finish-args.
manifest() {
    name=$1
    shift
    args=
    for a in "$@"; do args="$args${args:+, }\"$a\""; done
    printf '{\n  "id": "org.example.Test",\n  "finish-args": [%s],\n  "modules": []\n}\n' \
        "$args" >"$tmp/$name.json"
}

# metadata NAME LINE...: a metadata keyfile with these [Context] lines.
metadata() {
    name=$1
    shift
    {
        printf '[Application]\nname=org.example.Test\nruntime=org.gnome.Platform/x86_64/51\n\n[Context]\n'
        printf '%s\n' "$@"
    } >"$tmp/$name"
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

ok_args='--socket=wayland --socket=fallback-x11 --share=ipc --device=dri'
# shellcheck disable=SC2086
manifest clean $ok_args
expect 0 "$tmp/clean.json"
manifest none
expect 0 "$tmp/none.json"

# shellcheck disable=SC2086
manifest network $ok_args --share=network
expect 1 "$tmp/network.json"
manifest home --socket=wayland --filesystem=home
expect 1 "$tmp/home.json"
manifest host --filesystem=host:ro
expect 1 "$tmp/host.json"
manifest talk --talk-name=org.freedesktop.Flatpak
expect 1 "$tmp/talk.json"
manifest devall --device=all
expect 1 "$tmp/devall.json"
manifest pulse --socket=pulseaudio
expect 1 "$tmp/pulse.json"

# Network in the build sandbox, in a module's build-options.
cat >"$tmp/build-net.json" <<'EOF'
{
  "id": "org.example.Test",
  "finish-args": ["--socket=wayland"],
  "modules": [{"name": "m", "build-options": {"build-args": ["--share=network"]}}]
}
EOF
expect 1 "$tmp/build-net.json"

# The shape flatpak-github-actions writes when run-tests is on.
cat >"$tmp/test-net.json" <<'EOF'
{
  "id": "org.example.Test",
  "finish-args": ["--socket=wayland"],
  "build-options": {"test-args": ["--socket=x11", "--share=network"]},
  "modules": []
}
EOF
expect 1 "$tmp/test-net.json"

# Network as two arguments.
cat >"$tmp/build-net2.json" <<'EOF'
{
  "id": "org.example.Test",
  "finish-args": ["--socket=wayland"],
  "modules": [{"name": "m", "build-options": {"build-args": ["--share", "network"]}}]
}
EOF
expect 1 "$tmp/build-net2.json"
manifest finish-net2 --socket=wayland --share network
expect 1 "$tmp/finish-net2.json"
# "--share" "ipc" in build-args is not network.
cat >"$tmp/build-ipc2.json" <<'EOF'
{
  "id": "org.example.Test",
  "finish-args": ["--socket=wayland"],
  "modules": [{"name": "m", "build-options": {"build-args": ["--share", "ipc"]}}]
}
EOF
expect 0 "$tmp/build-ipc2.json"

printf '{ not json'>"$tmp/broken.json"
expect 2 "$tmp/broken.json"
expect 2 "$tmp/missing.json"
expect 2

metadata meta-clean 'shared=ipc;' 'sockets=wayland;fallback-x11;' 'devices=dri;'
expect 0 "$tmp/clean.json" "$tmp/meta-clean"
mkdir "$tmp/build"
cp "$tmp/meta-clean" "$tmp/build/metadata"
expect 0 "$tmp/clean.json" "$tmp/build"

metadata meta-net 'shared=network;ipc;' 'sockets=wayland;'
expect 1 "$tmp/clean.json" "$tmp/meta-net"
metadata meta-fs 'sockets=wayland;' 'filesystems=home;'
expect 1 "$tmp/clean.json" "$tmp/meta-fs"
metadata meta-bus 'sockets=wayland;'
printf '\n[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n' >>"$tmp/meta-bus"
expect 1 "$tmp/clean.json" "$tmp/meta-bus"
printf 'shared=ipc;\n' >"$tmp/meta-nogroup"
expect 2 "$tmp/clean.json" "$tmp/meta-nogroup"
expect 2 "$tmp/clean.json" "$tmp/no-such-dir"

for m in "$here"/../flatpak/*.json; do
    [ "$(basename "$m")" = cargo-sources.json ] || expect 0 "$m"
done

[ "$fail" = 0 ] && echo "flatpak-permissions self test: ok"
exit "$fail"
