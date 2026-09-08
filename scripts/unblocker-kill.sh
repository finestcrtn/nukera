#!/usr/bin/env bash
# unblocker-kill — instant nuclear restore.
# Stops the systemd service — polkit rules make it passwordless.
set -euo pipefail

echo "=== Unblocker kill ==="

# Stop the systemd service — polkit auto-approves for local users.
systemctl stop unblocker.service 2>/dev/null || true

# Also kill any orphaned nfqws processes (safety net)
pkill -9 -x nfqws 2>/dev/null || true

# Non-root cleanup
gsettings set org.gnome.system.proxy mode 'none' 2>/dev/null || true
kwriteconfig5 --file kioslaverc --group "Proxy Settings" --key ProxyType 0 2>/dev/null || true
resolvectl flush-caches 2>/dev/null || true

echo "=== KILL DONE ==="
echo "nfqws: $(pgrep -x nfqws 2>/dev/null && echo alive || echo dead)"
