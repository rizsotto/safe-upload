#!/bin/sh
# Fails if a network crate appears in a Cargo.lock, or if Rust sources
# use the standard network API (SPEC S-3, NFR-3).
# Usage: check-no-network-crates.sh [path/to/Cargo.lock [SOURCE_DIR...]]
set -eu

lock=${1:-Cargo.lock}
[ "$#" -gt 0 ] && shift
[ -r "$lock" ] || { echo "no-network: cannot read $lock" >&2; exit 2; }

denied='
actix-web
async-net
async-tungstenite
attohttpc
awc
axum
curl
curl-sys
git2
h2
h3
hickory-resolver
hyper
hyper-rustls
hyper-tls
hyper-util
isahc
minreq
native-tls
quinn
quinn-proto
reqwest
reqwless
rustls
socket2
surf
tokio-native-tls
tokio-rustls
tokio-tungstenite
tonic
trust-dns-resolver
tungstenite
ureq
warp
'

names=$(sed -n 's/^name = "\([^"]*\)"$/\1/p' "$lock" | sort -u)
if [ -z "$names" ]; then
    echo "no-network: no packages found in $lock" >&2
    exit 2
fi

found=$(printf '%s\n' "$names" |
    grep -Fx -e "$(printf '%s' "$denied" | sed '/^$/d')" || true)

if [ -n "$found" ]; then
    echo "no-network: network crates found in $lock (SPEC S-3):" >&2
    printf '  %s\n' $found >&2
    exit 1
fi

for dir in "$@"; do
    [ -d "$dir" ] || { echo "no-network: no folder $dir" >&2; exit 2; }
    if grep -rnE --include='*.rs' 'std::net\b|\b(TcpStream|TcpListener|UdpSocket)\b' "$dir" >&2; then
        echo "no-network: network API used in $dir (SPEC S-3)" >&2
        exit 1
    fi
done
echo "no-network: $lock is clean${*:+, sources in $* are clean}"
