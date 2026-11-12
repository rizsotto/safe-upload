#!/bin/sh
# Runs the installed Flatpak's CLI on the HEIC fixtures (SPEC 12 M2, HEIC in
# the shipped Flatpak). build-aux/ci/check-heic-outputs.sh then checks the
# outputs.
#
# glycin's FlatpakSpawn sandbox asks org.freedesktop.portal.Flatpak to start
# each loader. The script runs a private D-Bus session with its own
# flatpak-portal (verbose, logged to OUT_DIR/portal.log) and fails unless
# that log shows the app's sandboxed spawn of the HEIF loader, so a pass
# proves the decode went through FlatpakSpawn.
# Usage: test-heic.sh FIXTURE_DIR OUT_DIR
set -eu

app=org.example.SafeUpload
portal_name=org.freedesktop.portal.Flatpak

if [ "${1:-}" != --in-session ]; then
    [ "$#" -eq 2 ] || { echo "usage: $0 FIXTURE_DIR OUT_DIR" >&2; exit 2; }
    fixtures=$(cd "$1" && pwd)
    mkdir -p "$2"
    out=$(cd "$2" && pwd)
    flatpak info "$app" >/dev/null || { echo "test-heic: $app is not installed" >&2; exit 2; }
    exec dbus-run-session -- "$0" --in-session "$fixtures" "$out"
fi
fixtures=$2
out=$3

portal=
for p in /usr/libexec/flatpak-portal /usr/lib/flatpak/flatpak-portal /app/libexec/flatpak-portal; do
    [ -x "$p" ] && { portal=$p; break; }
done
[ -n "$portal" ] || { echo "test-heic: flatpak-portal not found" >&2; exit 2; }
"$portal" --no-idle-exit --verbose >"$out/portal.log" 2>&1 &
portal_pid=$!
trap 'kill "$portal_pid" 2>/dev/null || true' EXIT
gdbus wait --session --timeout 20 "$portal_name" ||
    { echo "test-heic: $portal_name did not start" >&2; cat "$out/portal.log" >&2; exit 2; }

fail=0
for name in gps rot90; do
    in="$fixtures/heic-$name.heic"
    [ -r "$in" ] || { echo "test-heic: missing $in" >&2; exit 2; }
    rm -f "$out/$name.jpg"
    # --filesystem grants only these two folders, for this run only.
    if flatpak run --filesystem="$fixtures:ro" --filesystem="$out" \
        --command=safe-upload "$app" "$in" --max-size 5MB -o "$out/$name.jpg" \
        >"$out/$name.log" 2>&1; then
        echo "test-heic: heic-$name.heic -> $name.jpg"
    else
        echo "test-heic: FAIL: heic-$name.heic exited $?" >&2
        cat "$out/$name.log" >&2
        fail=1
    fi
done

grep -qF "spawn() called from app: '$app'" "$out/portal.log" &&
    grep -F "Starting:" "$out/portal.log" | grep -F -- "--sandbox" | grep -qF glycin-heif || {
    echo "test-heic: FAIL: portal.log shows no sandboxed glycin-heif spawn for $app" >&2
    fail=1
}
exit "$fail"
