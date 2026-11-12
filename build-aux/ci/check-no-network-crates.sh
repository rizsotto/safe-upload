#!/bin/sh
# Fails if a network crate appears in a Cargo.lock (SPEC S-3, NFR-3).
# Usage: check-no-network-crates.sh [path/to/Cargo.lock]
set -eu

lock=${1:-Cargo.lock}
[ -r "$lock" ] || { echo "no-network: cannot read $lock" >&2; exit 2; }

denied='
actix-web
async-tungstenite
attohttpc
awc
axum
curl
curl-sys
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
surf
tokio-native-tls
tokio-rustls
tokio-tungstenite
trust-dns-resolver
tungstenite
ureq
warp
'

found=$(sed -n 's/^name = "\([^"]*\)"$/\1/p' "$lock" | sort -u |
    grep -Fx -e "$(printf '%s' "$denied" | sed '/^$/d')" || true)

if [ -n "$found" ]; then
    echo "no-network: network crates found in $lock (SPEC S-3):" >&2
    printf '  %s\n' $found >&2
    exit 1
fi
echo "no-network: $lock is clean"
