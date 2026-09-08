#!/usr/bin/env bash
# fetch-zapret.sh — download bol-van/zapret nfqws + tpws into vendor/
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/vendor/zapret"
TAG="${ZAPRET_TAG:-v72.13}"
mkdir -p "$VENDOR"
if [ ! -f "$VENDOR/nfqws" ] || [ ! -f "$VENDOR/tpws" ]; then
    echo "Downloading bol-van/zapret $TAG..."
    URL="https://github.com/bol-van/zapret/releases/download/$TAG/zapret-$TAG.tar.gz"
    tmp=$(mktemp -d)
    curl -fL "$URL" -o "$tmp/zapret.tar.gz"
    tar -xzf "$tmp/zapret.tar.gz" -C "$tmp"
    src="$tmp/zapret-$TAG/binaries/linux-x86_64"
    if [ ! -d "$src" ]; then
        echo "ERROR: $src not found in release tarball"
        exit 1
    fi
    install -m 0755 "$src/nfqws" "$VENDOR/nfqws"
    install -m 0755 "$src/tpws"  "$VENDOR/tpws"
    rm -rf "$tmp"
fi
ls -lh "$VENDOR/nfqws" "$VENDOR/tpws"
echo "Done."
