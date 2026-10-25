#!/bin/sh
set -eu
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
PLATFORM="${1:-linux}"
case "$PLATFORM" in
  linux)
    ls "$SCRIPT_DIR/src-tauri/target/release/bundle/deb"/*.deb \
      >/dev/null 2>&1 \
      || ls "$SCRIPT_DIR/target/release/bundle/deb"/*.deb \
        >/dev/null 2>&1 \
      || ls target/release/bundle/deb/*.deb >/dev/null 2>&1 \
      || ls src-tauri/target/release/bundle/deb/*.deb \
        >/dev/null 2>&1 \
      || exit 1
    ls "$SCRIPT_DIR/src-tauri/target/release/bundle/appimage"/*.AppImage \
      >/dev/null 2>&1 \
      || ls "$SCRIPT_DIR/target/release/bundle/appimage"/*.AppImage \
        >/dev/null 2>&1 \
      || ls target/release/bundle/appimage/*.AppImage \
        >/dev/null 2>&1 \
      || ls src-tauri/target/release/bundle/appimage/*.AppImage \
        >/dev/null 2>&1 \
      || exit 1
    ;;
  linux-deb)
    ls "$SCRIPT_DIR/src-tauri/target/release/bundle/deb"/*.deb \
      >/dev/null 2>&1 \
      || ls "$SCRIPT_DIR/target/release/bundle/deb"/*.deb \
        >/dev/null 2>&1 \
      || ls target/release/bundle/deb/*.deb >/dev/null 2>&1 \
      || ls src-tauri/target/release/bundle/deb/*.deb \
        >/dev/null 2>&1 \
      || exit 1
    ;;
  macos)
    ls "$SCRIPT_DIR/src-tauri/target/release/bundle/dmg"/*.dmg \
      >/dev/null 2>&1 \
      || ls "$SCRIPT_DIR/target/release/bundle/dmg"/*.dmg \
        >/dev/null 2>&1 \
      || ls target/release/bundle/dmg/*.dmg >/dev/null 2>&1 \
      || ls src-tauri/target/release/bundle/dmg/*.dmg \
        >/dev/null 2>&1 \
      || exit 1
    ;;
  windows)
    ls "$SCRIPT_DIR/src-tauri/target/release/bundle/msi"/*.msi \
      >/dev/null 2>&1 \
      || ls "$SCRIPT_DIR/target/release/bundle/msi"/*.msi \
        >/dev/null 2>&1 \
      || ls target/release/bundle/msi/*.msi >/dev/null 2>&1 \
      || ls src-tauri/target/release/bundle/msi/*.msi \
        >/dev/null 2>&1 \
      || exit 1
    ;;
  *)
    echo "unknown platform $PLATFORM (expected: linux linux-deb macos windows)" >&2
    exit 1
    ;;
esac
echo "artifacts-ok $PLATFORM"
