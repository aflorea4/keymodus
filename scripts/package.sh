#!/bin/sh
# Build the macOS installers into dist/:
#   Keymodus-<version>.dmg   drag-to-Applications disk image
#   Keymodus-<version>.pkg   guided Installer package (installs to /Applications, then opens the app)
# Both contain a universal (Apple Silicon + Intel) app.
#
# Optional, for distributing to other Macs without Gatekeeper warnings:
#   APPLE_SIGNING_IDENTITY="Developer ID Application: …" sign the app (also used for the dmg)
#   INSTALLER_IDENTITY="Developer ID Installer: …" sign the .pkg
#   NOTARY_PROFILE=<keychain profile>             notarize and staple both
#     (create one with: xcrun notarytool store-credentials <profile>)
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)
export COPYFILE_DISABLE=1  # no ._ AppleDouble files in archives
INSTALLER="$ROOT/scripts/installer"

scripts/bundle.sh --universal

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP="$ROOT/target/release/bundle/Keymodus.app"
WORK="$ROOT/target/installer"
DIST="$ROOT/dist"
rm -rf "$WORK"
mkdir -p "$WORK/art" "$WORK/resources" "$DIST"

echo "Rendering installer artwork"
swift "$INSTALLER/art.swift" "$ROOT/app/icons/icon.png" "$WORK/art" >/dev/null

# ---- .pkg -------------------------------------------------------------------
echo "Building Keymodus-$VERSION.pkg"
mkdir -p "$WORK/payload"
# No extended attributes: pkgbuild would store them as ._ files inside the bundle.
ditto --noextattr --norsrc "$APP" "$WORK/payload/Keymodus.app"
pkgbuild --analyze --root "$WORK/payload" "$WORK/component.plist" >/dev/null
# Always install to /Applications (not wherever an older copy happens to be),
# and allow downgrades so a reinstall always takes.
for key in BundleIsRelocatable BundleIsVersionChecked; do
  plutil -replace "0.$key" -bool NO "$WORK/component.plist"
done
pkgbuild --root "$WORK/payload" \
  --component-plist "$WORK/component.plist" \
  --identifier dev.keymodus.app \
  --version "$VERSION" \
  --install-location /Applications \
  --scripts "$INSTALLER/scripts" \
  "$WORK/Keymodus-component.pkg" >/dev/null

# Installer only ships files the distribution names, so inline the stylesheet
# and pack 1x + 2x artwork into one HiDPI TIFF.
for page in welcome conclusion; do
  awk -v css="$INSTALLER/resources/style.css" '
    /<link rel="stylesheet"/ { print "<style>"; while ((getline line < css) > 0) print line; print "</style>"; next }
    { print }' "$INSTALLER/resources/$page.html" > "$WORK/resources/$page.html"
done
for bg in pkg-background pkg-background-dark; do
  tiffutil -cathidpicheck "$WORK/art/$bg.png" "$WORK/art/$bg@2x.png" -out "$WORK/resources/$bg.tiff" 2>/dev/null
done
sed "s/@VERSION@/$VERSION/g" "$INSTALLER/distribution.xml" > "$WORK/distribution.xml"
PKG="$DIST/Keymodus-$VERSION.pkg"
rm -f "$PKG"
if [ -n "${INSTALLER_IDENTITY:-}" ]; then
  productbuild --distribution "$WORK/distribution.xml" --resources "$WORK/resources" \
    --package-path "$WORK" --sign "$INSTALLER_IDENTITY" --timestamp "$PKG" >/dev/null
else
  productbuild --distribution "$WORK/distribution.xml" --resources "$WORK/resources" \
    --package-path "$WORK" "$PKG" >/dev/null
fi

# ---- .dmg -------------------------------------------------------------------
echo "Building Keymodus-$VERSION.dmg"
TOOLS="$ROOT/target/.pkgtools"
if [ ! -x "$TOOLS/bin/dmgbuild" ]; then
  python3 -m venv "$TOOLS"
  "$TOOLS/bin/pip" install --quiet dmgbuild
fi
DMG="$DIST/Keymodus-$VERSION.dmg"
rm -f "$DMG"
"$TOOLS/bin/dmgbuild" -s "$INSTALLER/dmg-settings.py" -D app="$APP" -D art="$WORK/art" \
  "Keymodus $VERSION" "$DMG" >/dev/null
SIGN_IDENTITY="${APPLE_SIGNING_IDENTITY:--}"
if [ "$SIGN_IDENTITY" != "-" ]; then
  codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG"
fi

# ---- notarization (optional) ------------------------------------------------
if [ -n "${NOTARY_PROFILE:-}" ]; then
  for f in "$PKG" "$DMG"; do
    xcrun notarytool submit "$f" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$f"
  done
fi

echo
echo "Installers:"
ls -lh "$PKG" "$DMG" | awk '{print "  " $5 "  " $NF}'
