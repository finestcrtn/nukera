#!/usr/bin/env bash
# build-arch-pkg.sh — build the one-file Arch package (Manjaro-ready).
# Output: unblocker-<ver>-<rel>-x86_64.pkg.tar.zst in the repo root.
# Anyone can double-click that file (pamac/GNOME Software) to install.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Flutter lives in non-PATH locations on many setups (e.g. ~/opt/flutter).
if command -v flutter >/dev/null 2>&1; then
    :
elif [ -x "$HOME/opt/flutter/bin/flutter" ]; then
    export PATH="$HOME/opt/flutter/bin:$PATH"
elif [ -x "$HOME/flutter/bin/flutter" ]; then
    export PATH="$HOME/flutter/bin:$PATH"
elif [ -x /opt/flutter/bin/flutter ]; then
    export PATH="/opt/flutter/bin:$PATH"
fi

if [ "$(id -u)" -eq 0 ]; then
    echo "Do not run makepkg as root." >&2
    exit 1
fi

# 0. Sanity: prerequisites must exist before makepkg fails midway.
for c in cargo flutter curl git; do
    command -v "$c" >/dev/null 2>&1 || { echo "Missing build dep: $c" >&2; exit 1; }
done

# 1. Fetch vendored zapret engines if absent (nfqws is required).
if [ ! -f vendor/zapret/nfqws ]; then
    scripts/fetch-zapret.sh
fi

# 2. Build the package. makepkg compiles Rust + Flutter + stages payload.
# makepkg wants PKGBUILD in the working dir; symlink it in (and the
# .install script it references) for the duration of the build.
if [ ! -e PKGBUILD ] && [ ! -L PKGBUILD ]; then
    ln -s packaging/arch/PKGBUILD PKGBUILD
    ln -s packaging/arch/unblocker.install unblocker.install
    _linked=1
fi
makepkg -Ccf "$@"
if [ "${_linked:-0}" = "1" ]; then
    rm -f PKGBUILD unblocker.install
fi

# 3. Report + checksum.
PKG="$(ls -t unblocker-*.pkg.tar.zst | head -n1)"
echo
echo "✓ Package built: $PKG"
echo "  SHA256: $(sha256sum "$PKG" | cut -d' ' -f1)"
echo
echo "Install on Arch/Manjaro (plain CLI):   sudo pacman -U ./$PKG"
echo "Install on Manjaro (double click):     open-in-file-manager → pamac → Install"