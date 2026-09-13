#!/bin/bash
#
# Nukera — one-click installer.
#
# This build is unsigned, so macOS would otherwise show
# "Nukera.app is damaged / cannot be opened". This script copies the app
# to Applications, clears the quarantine flag, and launches it.
#
# The DMG shows ONLY this script; the app itself lives in the hidden
# `.nukera-global` folder so there is exactly one way to install.
#
set -e

HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/.nukera-global/Nukera.app"
DEST="/Applications/Nukera.app"

echo "==============================================="
echo "  Nukera — installing to /Applications"
echo "==============================================="
echo

if [ ! -d "$APP" ]; then
  echo "error: Nukera.app not found next to this script."
  echo "Run this script from inside the mounted Nukera disk image."
  read -n 1 -s -r -p "Press any key to close..."
  exit 1
fi

echo "Copying Nukera.app ..."
rm -rf "$DEST"
ditto "$APP" "$DEST"

echo "Clearing quarantine ..."
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true

echo "Done. Launching Nukera ..."
open "$DEST"

echo
echo "You can now eject the disk image. Nukera is in your Applications folder."
sleep 2