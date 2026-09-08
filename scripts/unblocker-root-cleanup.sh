#!/usr/bin/env bash
# unblocker-root-cleanup — runs as root via pkexec.
# ALL privileged cleanup in one shot: kill processes, flush nft, clean hosts,
# restore nsswitch. Called by unblocker-kill.sh via single pkexec call.
set -euo pipefail

# 1. Kill nfqws
pkill -9 -x nfqws 2>/dev/null || true

# 2. Kill legacy byedpi/ciadpi
pkill -9 -x ciadpi 2>/dev/null || true

# 3. Flush ALL unblocker nft tables
nft delete table inet unblocker 2>/dev/null || true
nft delete table inet unblocker_nat 2>/dev/null || true

# 4. Flush iptables REDIRECT (legacy)
iptables -t nat -D OUTPUT -p tcp -m multiport --dports 80,443 -m owner ! --uid-owner 0 -j REDIRECT --to-ports 1080 2>/dev/null || true

# 5. Clean /etc/hosts
if grep -q "# BEGIN unblocker" /etc/hosts 2>/dev/null; then
    sed -i '/# BEGIN unblocker/,/# END unblocker/d' /etc/hosts
    echo "hosts: cleaned"
fi

# 6. Restore nsswitch.conf
if [ -f /etc/nsswitch.conf.unblocker-backup ]; then
    cp /etc/nsswitch.conf.unblocker-backup /etc/nsswitch.conf
    rm -f /etc/nsswitch.conf.unblocker-backup
    echo "nsswitch: restored"
fi

# 7. Remove state file
rm -f /tmp/unblocker.state

echo "ROOT CLEANUP DONE"
