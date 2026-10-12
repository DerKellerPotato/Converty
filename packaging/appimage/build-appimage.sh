#!/usr/bin/env bash
# Builds dist/Converty-<version>-x86_64.AppImage.
# Needs appimagetool on PATH or in $APPIMAGETOOL
# (https://github.com/AppImage/appimagetool/releases).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APPID=io.github.converty.Converty
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="${ARCH:-$(uname -m)}"
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
[ -x "$TOOL" ] || { echo "appimagetool not found; set APPIMAGETOOL=/path/to/appimagetool" >&2; exit 1; }

[ -n "${SKIP_BUILD:-}" ] || cargo build --release -p converty --manifest-path "$ROOT/Cargo.toml"

APPDIR="$ROOT/target/appimage/Converty.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/metainfo" "$APPDIR/usr/share/icons/hicolor/scalable/apps" \
         "$APPDIR/usr/share/icons/hicolor/symbolic/apps"

install -m755 "$ROOT/target/release/converty" "$APPDIR/usr/bin/converty"
strip "$APPDIR/usr/bin/converty" || true
install -m755 "$ROOT/packaging/appimage/AppRun" "$APPDIR/AppRun"

# libxkbcommon is the only non-glibc library the binary links against; bundle it.
# Wayland, Vulkan/GL and fonts are loaded from the host at runtime, as they must be.
lib="$(ldd "$APPDIR/usr/bin/converty" | awk '/libxkbcommon\.so/ {print $3}')"
[ -n "$lib" ] && cp -L "$lib" "$APPDIR/usr/lib/"

install -m644 "$ROOT/res/$APPID.desktop" "$APPDIR/usr/share/applications/"
install -m644 "$ROOT/res/$APPID.metainfo.xml" "$APPDIR/usr/share/metainfo/"
install -m644 "$ROOT/res/icons/hicolor/scalable/apps/$APPID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/"
install -m644 "$ROOT/res/icons/hicolor/symbolic/apps/$APPID-symbolic.svg" "$APPDIR/usr/share/icons/hicolor/symbolic/apps/"
cp "$ROOT/res/$APPID.desktop" "$APPDIR/"
cp "$ROOT/res/icons/hicolor/scalable/apps/$APPID.svg" "$APPDIR/"
ln -sf "$APPID.svg" "$APPDIR/.DirIcon"

mkdir -p "$ROOT/dist"
OUT="$ROOT/dist/Converty-$VERSION-$ARCH.AppImage"
ARCH=$ARCH "$TOOL" --no-appstream "$APPDIR" "$OUT"
echo "Built $OUT"
