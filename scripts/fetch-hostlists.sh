#!/usr/bin/env bash
# fetch-hostlists.sh — download the reestr blocklist (the canonical Russian-DPI
# domain list from bol-van). 700K+ domains. This is what makes hostfakesplit
# actually work — the strategy only fires on connections to listed domains.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-/etc/unblocker/hostlists}"
mkdir -p "$DEST"
REESTR="$DEST/reestr.txt"
URL="https://raw.githubusercontent.com/bol-van/rulist/main/reestr_hostname_resolvable.txt"
if [ -f "$REESTR" ] && [ "$(stat -c %s "$REESTR")" -gt 102400 ]; then
    echo "hostlist already present at $REESTR ($(stat -c %s "$REESTR") bytes)"
    exit 0
fi
echo "Downloading reestr blocklist (700K+ domains)..."
curl -fL --max-time 120 "$URL" -o "$REESTR"
ls -lh "$REESTR"
echo "Done. nfqws will use $REESTR for hostlist matching."
