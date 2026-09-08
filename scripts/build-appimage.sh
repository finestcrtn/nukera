#!/usr/bin/env bash
# build-appimage.sh — build the portable Nukera AppImage.
#
# Produces ./Nukera-<gitrev>-x86_64.AppImage containing the full payload
# (GUI bundle + CLI + nfqws + strategies + unit + polkit rule). The
# AppImage self-installs that payload into the exact /usr/lib/nukera
# layout the Arch package ships, then launches the installed GUI.
#
# We assemble the AppImage MANUALLY: static type2-runtime + squashfs built
# by the system mksquashfs. The static type2-runtime is musl/static (no
# libfuse2 dependency → works on Debian/Ubuntu 24.04/Fedora) and reads
# zstd squashfs; the filesystem starts right after the runtime's own ELF
# (same layout as appimagetool produces — verified: runtime || squashfs).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

[ "$(uname -m)" = x86_64 ] || { echo "AppImage build requires x86_64" >&2; exit 1; }
command -v mksquashfs >/dev/null 2>&1 || {
    echo "mksquashfs (squashfs-tools) required — e.g. sudo pacman -S squashfs-tools" >&2; exit 1; }

# Flutter lives outside PATH on many setups — same lookup as build-arch-pkg.sh.
if ! command -v flutter >/dev/null 2>&1; then
    for p in "$HOME/opt/flutter/bin" "$HOME/flutter/bin" /opt/flutter/bin; do
        if [ -x "$p/flutter" ]; then export PATH="$p:$PATH"; break; fi
    done
fi
command -v flutter >/dev/null 2>&1 || { echo "flutter not found — set PATH or install under ~/opt/flutter" >&2; exit 1; }

# Static type2-runtime (no libfuse2 needed on the target). Cached to ~/.cache/nukera.
CACHE="$HOME/.cache/nukera"
mkdir -p "$CACHE"
RUNTIME="${APPIMAGE_RUNTIME:-$CACHE/runtime-type2-x86_64}"
if [ ! -f "$RUNTIME" ]; then
    echo "Downloading static type2-runtime…"
    curl -fL --max-time 180 -o "$RUNTIME.part" \
        "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64"
    mv "$RUNTIME.part" "$RUNTIME"
fi

echo "=== Rust core + CLI ==="
cargo build --release
echo "=== Flutter GUI ==="
(cd unblocker_gui && flutter pub get >/dev/null && flutter build linux --release)

VER="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
OUT="Nukera-${VER}-x86_64.AppImage"

BUILD="$(mktemp -d /tmp/nukera-appimage.XXXXXX)"
trap 'rm -rf "$BUILD"' EXIT
ADIR="$BUILD/Nukera.AppDir"
RFS="$ADIR/payload/rootfs"
BUNDLE="unblocker_gui/build/linux/x64/release/bundle"

echo "=== Staging payload (mirrors PKGBUILD layout) ==="
install -d "$RFS/usr/lib/nukera" "$RFS/usr/lib/unblocker" "$RFS/usr/bin" \
    "$RFS/usr/lib/systemd/system" "$RFS/etc/polkit-1/rules.d" \
    "$RFS/usr/share/applications" "$RFS/usr/share/pixmaps"

# GUI bundle + CLI + emergency scripts
cp -r "$BUNDLE/." "$RFS/usr/lib/nukera/"
install -m755 target/release/unblocker "$RFS/usr/lib/nukera/unblocker"
install -m755 scripts/unblocker-kill.sh "$RFS/usr/lib/nukera/unblocker-kill.sh"
install -m755 scripts/unblocker-root-cleanup.sh "$RFS/usr/lib/nukera/unblocker-root-cleanup.sh"
install -m755 scripts/unblocker-kill.sh "$RFS/usr/bin/unblocker-kill"

# DPI engine
install -m755 vendor/zapret/nfqws "$RFS/usr/lib/unblocker/nfqws"

# Config: strategies (top-level + pool + byebyedpi), hostlists, sites
install -d "$RFS/etc/unblocker/strategies/pool/byebyedpi" \
    "$RFS/etc/unblocker/hostlists" "$RFS/etc/unblocker/sites"
install -m644 config/zapret/strategies/*.strat "$RFS/etc/unblocker/strategies/"
install -m644 config/zapret/strategies/pool/*.strat "$RFS/etc/unblocker/strategies/pool/"
install -m644 config/zapret/strategies/pool/byebyedpi/*.strat "$RFS/etc/unblocker/strategies/pool/byebyedpi/"
ln -s strategies/general-hostfakesplit.strat "$RFS/etc/unblocker/strategy.conf"
install -m644 config/zapret/hostlists/*.sites "$RFS/etc/unblocker/hostlists/"
install -m644 config/zapret/hostlists/ig-fb-known-good.txt "$RFS/etc/unblocker/hostlists/"
install -m644 config/hosts/telegram-web.txt "$RFS/etc/unblocker/hostlists/telegram-web.txt"
install -m644 config/hosts/magila-cdn.txt "$RFS/etc/unblocker/hostlists/magila-cdn.txt"
touch "$RFS/etc/unblocker/hostlists/auto.txt"

# System integration
install -m644 packaging/systemd/unblocker.service "$RFS/usr/lib/systemd/system/unblocker.service"
install -m644 packaging/polkit/50-unblocker.rules "$RFS/etc/polkit-1/rules.d/50-unblocker.rules"
install -m644 unblocker.desktop "$RFS/usr/share/applications/unblocker.desktop"
install -m644 assets/icons/nukera_icon.png "$RFS/usr/share/pixmaps/nukera.png"

# Version marker (self-update check)
PAYLOAD="$ADIR/payload"
printf '%s\n' "$VER" > "$PAYLOAD/version"
printf '%s\n' "$VER" > "$RFS/usr/lib/nukera/version"

# AppRun + desktop integration (icon must be relative to satisfy appimagetool
# conventions; we keep the packaged absolute-Icon desktop for usr/share).
install -m755 packaging/appimage/AppRun "$ADIR/AppRun"
install -m644 packaging/appimage/nukera.desktop "$ADIR/nukera.desktop"
install -m644 assets/icons/nukera_icon.png "$ADIR/nukera_icon.png"
ln -sf nukera_icon.png "$ADIR/.DirIcon"

echo "=== Building AppImage ($OUT) ==="
# Layout: the runtime parses its own ELF to find the end of its code and
# expects the squashfs to start exactly there — so runtime || squashfs.
SQFS="$BUILD/nukera.sqfs"
mksquashfs "$ADIR" "$SQFS" -comp zstd -noappend -no-progress >/dev/null
cat "$RUNTIME" "$SQFS" > "$OUT"
chmod +x "$OUT"

echo
echo "✓ Built: $OUT"
echo "  size:   $(du -h "$OUT" | cut -f1)"
echo "  sha256: $(sha256sum "$OUT" | cut -d' ' -f1)"
echo "  Run:    ./$OUT            (first run installs + launches)"
echo "  Remove: ./$OUT --uninstall"