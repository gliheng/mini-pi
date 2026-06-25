#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-0.1.0}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE="$ROOT/target/package/mini-pi"
TOOLS="$ROOT/tools"
BUNDLE_DIR="$ROOT/target/release/bundle/osx"
APP_NAME="Mini Pi"
APP_BUNDLE="$BUNDLE_DIR/$APP_NAME.app"
DMG_OUT="$ROOT/target/mini-pi-$VERSION-x64.dmg"

mkdir -p "$TOOLS"

if ! command -v cargo-bundle &>/dev/null; then
  echo "Installing cargo-bundle..."
  cargo install cargo-bundle
fi

echo "Building release app bundle..."
cargo bundle --release

echo "Preparing production pi-bridge dependencies..."
rm -rf "$PACKAGE"
mkdir -p "$PACKAGE/pi-bridge"
cp "$ROOT/pi-bridge/package.json" "$PACKAGE/pi-bridge/"
cp "$ROOT/pi-bridge/package-lock.json" "$PACKAGE/pi-bridge/"
cp -R "$ROOT/pi-bridge/dist" "$PACKAGE/pi-bridge/"
(cd "$PACKAGE/pi-bridge" && npm ci --omit=dev)

echo "Injecting production pi-bridge into app bundle..."
cp -R "$PACKAGE/pi-bridge/node_modules" "$APP_BUNDLE/Contents/Resources/pi-bridge/"

ARCH="$(uname -m)"
NODE_VERSION="20.15.1"
if [ "$ARCH" = "arm64" ]; then
  NODE_TARBALL="node-v${NODE_VERSION}-darwin-arm64.tar.gz"
else
  NODE_TARBALL="node-v${NODE_VERSION}-darwin-x64.tar.gz"
fi
NODE_URL="https://nodejs.org/dist/v${NODE_VERSION}/${NODE_TARBALL}"
NODE_DIR="$TOOLS/${NODE_TARBALL%.tar.gz}"
DMG_OUT="$ROOT/target/mini-pi-$VERSION-$ARCH.dmg"

if [ ! -f "$TOOLS/$NODE_TARBALL" ]; then
  echo "Downloading Node.js v${NODE_VERSION} for ${ARCH}..."
  curl -L -o "$TOOLS/$NODE_TARBALL" "$NODE_URL"
fi
if [ ! -d "$NODE_DIR" ]; then
  tar -xzf "$TOOLS/$NODE_TARBALL" -C "$TOOLS"
fi

echo "Bundling Node runtime..."
cp "$NODE_DIR/bin/node" "$APP_BUNDLE/Contents/Resources/pi-bridge/node"

DMG_TMP="$ROOT/target/mini-pi-dmg"
rm -rf "$DMG_TMP"
mkdir -p "$DMG_TMP"
cp -R "$APP_BUNDLE" "$DMG_TMP/"

if command -v create-dmg &>/dev/null; then
  echo "Creating DMG with create-dmg..."
  create-dmg \
    --volname "$APP_NAME" \
    --window-size 800 400 \
    --icon-size 100 \
    --app-drop-link 600 185 \
    "$DMG_OUT" \
    "$DMG_TMP"
else
  echo "create-dmg not found; falling back to hdiutil..."
  hdiutil create -srcfolder "$DMG_TMP" -volname "$APP_NAME" -fs HFS+ -format UDZO "$DMG_OUT"
fi

echo "Installer created: $DMG_OUT"
