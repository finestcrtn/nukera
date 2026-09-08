#!/usr/bin/env bash
# Quick run script for the Flutter GUI (dev or release).
set -euo pipefail
cd "$(dirname "$0")"

if [ "$1" = "--release" ]; then
    flutter run -d linux --release
else
    flutter run -d linux
fi
