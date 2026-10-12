#!/usr/bin/env bash
# Builds dist/Converty-<version>-macos-universal.{dmg,zip} (Apple Silicon + Intel).
# Run on macOS with the aarch64-apple-darwin and x86_64-apple-darwin targets installed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
cd "$ROOT"

for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p converty --target "$t"
done

APP="$ROOT/target/macos/Converty.app"
rm -rf "$ROOT/target/macos"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/converty" \
    target/aarch64-apple-darwin/release/converty target/x86_64-apple-darwin/release/converty
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp packaging/macos/Converty.icns "$APP/Contents/Resources/"
# Ad-hoc signature: required to run on Apple Silicon. Not notarized.
codesign --force --deep --sign - "$APP"

mkdir -p dist
ZIP="dist/Converty-$VERSION-macos-universal.zip"
DMG="dist/Converty-$VERSION-macos-universal.dmg"
rm -f "$ZIP" "$DMG"
ditto -c -k --keepParent "$APP" "$ZIP"

STAGE="$ROOT/target/macos/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Converty $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "Built $ZIP and $DMG"
