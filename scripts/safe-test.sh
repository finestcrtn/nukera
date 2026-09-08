#!/usr/bin/env bash
# safe-test.sh — never leave the machine firewalled/broken, even on crash/kill
# Usage: ./scripts/safe-test.sh [unblocker args...]
# Example: ./scripts/safe-test.sh enable
#          ./scripts/safe-test.sh probe https://instagram.com
set -euo pipefail

cleanup() {
  echo ""
  echo "[safe-test] trap: restoring..."
  gsettings set org.gnome.system.proxy mode 'none' 2>/dev/null || true
  # Remove our /etc/hosts block (no sudo if not needed)
  if grep -q "# BEGIN unblocker" /etc/hosts 2>/dev/null; then
    if sudo -n true 2>/dev/null; then
      sudo sed -i '/# BEGIN unblocker/,/# END unblocker/d' /etc/hosts 2>/dev/null || true
    elif command -v pkexec >/dev/null 2>&1; then
      pkexec sed -i '/# BEGIN unblocker/,/# END unblocker/d' /etc/hosts 2>/dev/null || true
    fi
  fi
  # Remove any stray REDIRECT we may have left from old version
  if sudo -n true 2>/dev/null; then
    sudo iptables -t nat -D OUTPUT -p tcp -m multiport --dports 80,443 -m owner ! --uid-owner 0 -j REDIRECT --to-ports 1080 2>/dev/null || true
  elif command -v pkexec >/dev/null 2>&1; then
    pkexec iptables -t nat -D OUTPUT -p tcp -m multiport --dports 80,443 -m owner ! --uid-owner 0 -j REDIRECT --to-ports 1080 2>/dev/null || true
  fi
  rm -f /tmp/unblocker.state
  rm -f /etc/systemd/resolved.conf.d/unblocker.conf 2>/dev/null || sudo rm -f /etc/systemd/resolved.conf.d/unblocker.conf 2>/dev/null || true
  echo "[safe-test] restored"
}
trap cleanup EXIT INT TERM

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/unblocker"
[ -x "$BIN" ] || BIN="$ROOT/target/release/unblocker"

if [ $# -eq 0 ]; then
  echo "Usage: $0 <unblocker args>"
  echo "Examples:"
  echo "  $0 status"
  echo "  $0 probe https://instagram.com"
  echo "  $0 enable   # leaves proxy on until you Ctrl-C or run: $0 disable"
  echo "  $0 disable"
  exit 0
fi

# For long-running enable, user will Ctrl-C; trap above guarantees restore
exec "$BIN" "$@"
