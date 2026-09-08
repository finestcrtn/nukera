#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$ROOT/vendor"
if [ ! -d "$ROOT/vendor/byedpi" ]; then
  echo "Cloning hufrea/byedpi…"
  git clone https://github.com/hufrea/byedpi.git "$ROOT/vendor/byedpi"
else
  echo "vendor/byedpi already exists"
fi
if [ ! -d "$ROOT/vendor/tg-ws-proxy-rs" ]; then
  echo "Cloning valnesfjord/tg-ws-proxy-rs (fallback to Flowseal/tg-ws-proxy)…"
  git clone https://github.com/valnesfjord/tg-ws-proxy-rs.git "$ROOT/vendor/tg-ws-proxy-rs" || \
  git clone https://github.com/Flowseal/tg-ws-proxy.git "$ROOT/vendor/tg-ws-proxy"
else
  echo "vendor/tg-ws-proxy-rs already exists"
fi
echo "Building byedpi…"
make -C "$ROOT/vendor/byedpi" -j"$(nproc)" || echo "byedpi build failed — see vendor/byedpi/README"
echo "Done. Bin: $ROOT/vendor/byedpi/ciadpi"
