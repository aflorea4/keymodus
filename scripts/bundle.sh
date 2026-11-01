#!/bin/sh
# Build Keymodus.app on macOS with tauri-cli (cargo install tauri-cli --version '^2' --locked).
#   scripts/bundle.sh                -> target/release/bundle/macos/Keymodus.app (this Mac's architecture)
#   scripts/bundle.sh --universal    -> Apple Silicon + Intel in one app
#   scripts/bundle.sh --install      -> also copies it to ~/Applications
# Signing: ad-hoc by default. Set APPLE_SIGNING_IDENTITY to use a certificate
# from your keychain. Updater artifacts are only produced when
# TAURI_SIGNING_PRIVATE_KEY is set (release builds in CI).
set -eu
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
UNIVERSAL=0
INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --universal) UNIVERSAL=1 ;;
    --install) INSTALL=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

ARGS="--bundles app"
OUT=target/release/bundle/macos
if [ "$UNIVERSAL" = 1 ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  ARGS="$ARGS --target universal-apple-darwin"
  OUT=target/universal-apple-darwin/release/bundle/macos
fi
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  # No updater key: build without updater artifacts.
  ARGS="$ARGS --config {\"bundle\":{\"createUpdaterArtifacts\":false}}"
fi
(cd app && cargo tauri build $ARGS)

mkdir -p target/release/bundle
rm -rf target/release/bundle/Keymodus.app
ditto "$OUT/Keymodus.app" target/release/bundle/Keymodus.app
echo "Built target/release/bundle/Keymodus.app"

if [ "$INSTALL" = 1 ]; then
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/Keymodus.app"
  ditto target/release/bundle/Keymodus.app "$HOME/Applications/Keymodus.app"
  echo "Installed to ~/Applications/Keymodus.app"
fi
