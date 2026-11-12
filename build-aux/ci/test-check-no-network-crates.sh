#!/bin/sh
# Self test for check-no-network-crates.sh.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
check="$here/check-no-network-crates.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

lock() {
    printf 'version = 4\n'
    for n in "$@"; do
        printf '\n[[package]]\nname = "%s"\nversion = "1.0.0"\n' "$n"
    done
}

fail=0
expect() {
    want=$1
    shift
    file=$*
    if "$check" "$@" >/dev/null 2>&1; then got=0; else got=$?; fi
    if [ "$got" != "$want" ]; then
        echo "FAIL: $file exited $got, want $want" >&2
        fail=1
    fi
}

lock safe-upload-core thiserror >"$tmp/clean.lock"
expect 0 "$tmp/clean.lock"

lock safe-upload-core reqwest thiserror >"$tmp/reqwest.lock"
expect 1 "$tmp/reqwest.lock"

lock rustls >"$tmp/rustls.lock"
expect 1 "$tmp/rustls.lock"

# Names that only contain a denied name must not match.
lock hyperlocal-not-really rustls-pemfile-x my-curl-wrapper >"$tmp/similar.lock"
expect 0 "$tmp/similar.lock"

expect 2 "$tmp/missing.lock"

lock socket2 >"$tmp/socket2.lock"
expect 1 "$tmp/socket2.lock"

printf 'version = 4\n' >"$tmp/empty.lock"
expect 2 "$tmp/empty.lock"

mkdir -p "$tmp/src-clean" "$tmp/src-net/sub"
printf 'fn main() {}\n' >"$tmp/src-clean/main.rs"
printf 'use std::net::TcpStream;\n' >"$tmp/src-net/sub/lib.rs"
expect 0 "$tmp/clean.lock" "$tmp/src-clean"
expect 1 "$tmp/clean.lock" "$tmp/src-clean" "$tmp/src-net"
expect 2 "$tmp/clean.lock" "$tmp/no-such-dir"

expect 0 "$here/../../Cargo.lock" "$here/../../crates"

[ "$fail" = 0 ] && echo "no-network self test: ok"
exit "$fail"
