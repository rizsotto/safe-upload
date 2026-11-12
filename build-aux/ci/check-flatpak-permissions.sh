#!/bin/sh
# Fails if the Flatpak manifest or the built app's metadata grants more than
# the app needs: no network (SPEC S-3, M2 "no network permission"), no
# filesystem, no bus names. Only the finish-args below are allowed.
# Usage: check-flatpak-permissions.sh MANIFEST [METADATA_OR_BUILD_DIR...]
#   A build dir (flatpak-builder output) is checked through its metadata file.
set -eu

[ "$#" -ge 1 ] || { echo "usage: $0 MANIFEST [METADATA_OR_BUILD_DIR...]" >&2; exit 2; }

exec python3 -I - "$@" <<'EOF'
import configparser
import json
import os
import sys

# Manifest finish-args, and the [Context] keys they become in metadata.
ALLOWED_ARGS = {"--socket=wayland", "--socket=fallback-x11", "--share=ipc", "--device=dri"}
ALLOWED_CONTEXT = {
    "shared": {"ipc"},
    "sockets": {"wayland", "fallback-x11"},
    "devices": {"dri"},
}

errors = []


def bad(msg):
    errors.append(msg)


def strings(node, path):
    if isinstance(node, str):
        yield path, node
    elif isinstance(node, list):
        for i, v in enumerate(node):
            yield from strings(v, f"{path}[{i}]")
    elif isinstance(node, dict):
        for k, v in node.items():
            yield from strings(v, f"{path}.{k}")


def lists(node, path):
    if isinstance(node, list):
        yield path, node
        for i, v in enumerate(node):
            yield from lists(v, f"{path}[{i}]")
    elif isinstance(node, dict):
        for k, v in node.items():
            yield from lists(v, f"{path}.{k}")


def check_manifest(path):
    with open(path, encoding="utf-8") as f:
        manifest = json.load(f)
    if not isinstance(manifest, dict):
        raise ValueError("not a JSON object")
    args = manifest.get("finish-args", [])
    if not isinstance(args, list):
        bad(f"{path}: finish-args is not a list")
        args = []
    for a in args:
        if a not in ALLOWED_ARGS:
            bad(f"{path}: finish-arg {a!r} is not allowed")
    # Network in the build or test sandbox (build-args, test-args) counts
    # too, anywhere, also as the two arguments "--share" "network".
    for where, s in strings(manifest, "$"):
        if "--share=network" in s:
            bad(f"{path}: {where} has --share=network")
    for where, items in lists(manifest, "$"):
        for a, b in zip(items, items[1:]):
            if a == "--share" and b == "network":
                bad(f"{path}: {where} has --share network")


def check_metadata(path):
    if os.path.isdir(path):
        path = os.path.join(path, "metadata")
    meta = configparser.ConfigParser(interpolation=None, strict=False)
    meta.optionxform = str
    with open(path, encoding="utf-8") as f:
        meta.read_file(f)
    if not meta.has_section("Application"):
        raise ValueError("no [Application] group")
    for section in meta.sections():
        if section.endswith("Bus Policy"):
            bad(f"{path}: [{section}] is not allowed")
    if meta.has_section("Context"):
        for key, value in meta.items("Context"):
            allowed = ALLOWED_CONTEXT.get(key)
            if allowed is None:
                bad(f"{path}: [Context] {key}={value} is not allowed")
                continue
            for item in filter(None, value.split(";")):
                if item not in allowed:
                    bad(f"{path}: [Context] {key} has {item!r}")


try:
    check_manifest(sys.argv[1])
    for p in sys.argv[2:]:
        check_metadata(p)
except (OSError, ValueError, configparser.Error) as e:
    print(f"flatpak-permissions: cannot check: {e}", file=sys.stderr)
    sys.exit(2)

if errors:
    for e in errors:
        print(f"flatpak-permissions: {e}", file=sys.stderr)
    sys.exit(1)
print(f"flatpak-permissions: {' '.join(sys.argv[1:])} allow only {' '.join(sorted(ALLOWED_ARGS))}")
EOF
