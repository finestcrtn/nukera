#!/usr/bin/env bash
# unblocker-install.sh — dev fallback installer. Builds from source and
# installs to the same layout the distro packages use (/usr/lib/nukera).
# Do NOT run on a machine where the distro package is already installed.
#
# After this, `unblocker-gui` works from the app launcher and
# `unblocker enable/disable` works from a terminal. No password prompts.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/usr/local}"
GUI_DIR="/usr/lib/nukera"
ENGINE_DIR="/usr/lib/unblocker"
ETC_DIR="/etc/unblocker"
HOSTLISTDIR="$ETC_DIR/hostlists"
STRATEGYDIR="$ETC_DIR"

# We need root
if [ "$(id -u)" -ne 0 ]; then
    echo "Run with sudo: sudo $0"
    exit 1
fi

echo "→ Installing unblocker (dev fallback install)..."
echo "  Project root: $ROOT"

# ─── 1. DPI engine (nfqws) ─────────────────────────────────────────
install -d "$ENGINE_DIR"
if [ -f "$ROOT/vendor/zapret/nfqws" ]; then
    install -m 0755 "$ROOT/vendor/zapret/nfqws" "$ENGINE_DIR/nfqws"
    setcap cap_net_admin,cap_net_raw,cap_dac_override,cap_setuid,cap_setgid,cap_setpcap,cap_sys_admin=+ep "$ENGINE_DIR/nfqws" 2>/dev/null || true
    echo "  nfqws installed to $ENGINE_DIR/nfqws (with capabilities)"
else
    echo "⚠ vendor/zapret/nfqws not found — run scripts/fetch-zapret.sh first"
fi

# ─── 2. Rust CLI ────────────────────────────────────────────────────
if [ -f "$ROOT/target/release/unblocker" ]; then
    install -m 0755 "$ROOT/target/release/unblocker" "$PREFIX/bin/unblocker"
    echo "  CLI installed to $PREFIX/bin/unblocker"
else
    echo "⚠ target/release/unblocker not found — run 'cargo build --release' first"
fi

# ─── 3. Flutter GUI bundle ──────────────────────────────────────────
BUNDLE="$ROOT/unblocker_gui/build/linux/x64/release/bundle"
if [ -f "$BUNDLE/unblocker-gui" ]; then
    rm -rf "$GUI_DIR"
    install -d "$GUI_DIR/lib"
    cp -r "$BUNDLE/data" "$GUI_DIR/data"
    cp -r "$BUNDLE/lib/." "$GUI_DIR/lib/"
    install -m 0755 "$BUNDLE/unblocker-gui" "$GUI_DIR/unblocker-gui"
    if [ -f "$ROOT/assets/icons/nukera_icon.png" ]; then
        cp -f "$ROOT/assets/icons/nukera_icon.png" "$GUI_DIR/data/nukera_icon.png"
    fi
    # Bundle CLI so the directory is self-contained (systemd unit uses it).
    if [ -f "$ROOT/target/release/unblocker" ]; then
        cp -f "$ROOT/target/release/unblocker" "$GUI_DIR/unblocker"
        chmod 755 "$GUI_DIR/unblocker"
    fi
    # Emergency scripts (referred to by the polkit rule).
    install -m 0755 "$ROOT/scripts/unblocker-kill.sh" "$GUI_DIR/unblocker-kill.sh"
    install -m 0755 "$ROOT/scripts/unblocker-root-cleanup.sh" "$GUI_DIR/unblocker-root-cleanup.sh"
    setcap cap_net_admin,cap_net_raw,cap_dac_override=+ep "$GUI_DIR/unblocker" 2>/dev/null || true
    echo "  GUI bundle installed to $GUI_DIR/"
else
    echo "⚠ Flutter build output not found — run 'flutter build linux --release' first"
fi

# ─── 4. Wrapper script (so 'unblocker-gui' works from PATH) ─────────
cat > "$PREFIX/bin/unblocker-gui" << 'WEOF'
#!/bin/sh
export LD_LIBRARY_PATH="/usr/lib/nukera/lib:${LD_LIBRARY_PATH}"
exec /usr/lib/nukera/unblocker-gui "$@"
WEOF
chmod 755 "$PREFIX/bin/unblocker-gui"
echo "  Wrapper installed to $PREFIX/bin/unblocker-gui"

# ─── 5. Kill script to PATH ─────────────────────────────────────────
install -m 0755 "$ROOT/scripts/unblocker-kill.sh" "$PREFIX/bin/unblocker-kill"

# ─── 6. Polkit rules (passwordless systemctl, any active local user) ─
install -d /etc/polkit-1/rules.d
install -m 0644 "$ROOT/packaging/polkit/50-unblocker.rules" /etc/polkit-1/rules.d/50-unblocker.rules
echo "  Polkit rules installed (no username pin — any active local user)"

# ─── 7. Systemd service ─────────────────────────────────────────────
install -m 0644 "$ROOT/packaging/systemd/unblocker.service" /etc/systemd/system/unblocker.service
systemctl daemon-reload
systemctl enable unblocker.service
echo "  Systemd service installed and enabled"

# ─── 8. Config directories + strategy files ─────────────────────────
install -d "$STRATEGYDIR/strategies/pool/byebyedpi" "$HOSTLISTDIR" "$STRATEGYDIR/sites"
cp "$ROOT"/config/zapret/strategies/*.strat "$STRATEGYDIR/strategies/"
cp "$ROOT"/config/zapret/strategies/pool/*.strat "$STRATEGYDIR/strategies/pool/"
cp "$ROOT"/config/zapret/strategies/pool/byebyedpi/*.strat "$STRATEGYDIR/strategies/pool/byebyedpi/"
ln -sf "$STRATEGYDIR/strategies/general-hostfakesplit.strat" "$STRATEGYDIR/strategy.conf"
echo "  Strategy files installed (top-level + pool + byebyedpi)"

# ─── 9. Hostlists + static hosts ────────────────────────────────────
if [ -f "$ROOT/config/zapret/hostlists/reestr.txt" ]; then
    install -m 0644 "$ROOT/config/zapret/hostlists/reestr.txt" "$HOSTLISTDIR/reestr.txt"
elif [ ! -f "$HOSTLISTDIR/reestr.txt" ] || [ "$(stat -c %s "$HOSTLISTDIR/reestr.txt" 2>/dev/null || echo 0)" -lt 102400 ]; then
    echo "  Downloading reestr blocklist..."
    curl -fL --max-time 120 "https://raw.githubusercontent.com/bol-van/rulist/main/reestr_hostname_resolvable.txt" -o "$HOSTLISTDIR/reestr.txt" 2>/dev/null || echo "⚠ reestr download failed"
fi
touch "$HOSTLISTDIR/auto.txt"
if [ -f "$ROOT/config/hosts/telegram-web.txt" ]; then
    install -m 0644 "$ROOT/config/hosts/telegram-web.txt" "$HOSTLISTDIR/telegram-web.txt"
fi
if [ -f "$ROOT/config/hosts/magila-cdn.txt" ]; then
    install -m 0644 "$ROOT/config/hosts/magila-cdn.txt" "$HOSTLISTDIR/magila-cdn.txt"
fi
if [ -d "$ROOT/config/zapret/hostlists" ]; then
    for f in "$ROOT"/config/zapret/hostlists/*.sites; do
        [ -f "$f" ] || continue
        install -m 0644 "$f" "$HOSTLISTDIR/"
    done
fi
echo "  Hostlists + static hosts installed"

# ─── 10. .desktop file ──────────────────────────────────────────────
install -m 0644 "$ROOT/packaging/desktop/unblocker.desktop" /usr/share/applications/unblocker.desktop
echo "  Desktop file installed"

# ─── 11. Restart polkitd to pick up rules ───────────────────────────
systemctl restart polkit 2>/dev/null || true

echo
echo "✓ Installed. Run 'unblocker-gui' or click from app launcher."
echo "  Enable: click ▶ or 'unblocker enable'"
echo "  Disable: click ■ or 'unblocker disable'"
echo "  Emergency: 'unblocker-kill'"
echo "  No password prompts after install."
echo
echo "  Tip: run 'unblocker install-discover' once to pin working IPs for"
echo "  your ISP (the distro package does this automatically at install)."