# GUI Icons — How to fix Material Icons in Flutter Linux builds

## The problem

Flutter Linux builds bundle a **stub** `MaterialIcons-Regular.otf` (~1KB) instead of
the real font (~1.6MB). Icons render as empty squares or don't appear at all.

## The fix

After `flutter build linux --release`, copy the real font from the Flutter SDK cache:

```bash
cp ~/opt/flutter/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf \
   unblocker_gui/build/linux/x64/release/bundle/data/flutter_assets/fonts/MaterialIcons-Regular.otf
```

The real font is at: `~/opt/flutter/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf`
Size: 1,645,184 bytes (vs stub: ~1KB).

## In the install script

The `scripts/unblocker-install.sh` does this automatically after building:

```bash
REAL_FONT="$HOME/opt/flutter/bin/cache/artifacts/material_fonts/MaterialIcons-Regular.otf"
if [ -f "$REAL_FONT" ]; then
    cp -f "$REAL_FONT" "$BUNDLE/data/flutter_assets/fonts/MaterialIcons-Regular.otf"
fi
```

## If you copy the dart file to another project

1. Copy `home_page.dart` and `app.dart` to your project
2. Ensure `pubspec.yaml` has `uses-material-design: true`
3. After `flutter build linux`, run the font copy command above
4. Or add the font copy to your build script

## Why this happens

Flutter's Linux build system doesn't properly include the full Material Icons
font. It bundles a stub. The `uses-material-design: true` in pubspec.yaml
should include it, but the build output only gets the stub. This is a known
issue with Flutter Linux builds.
