#!/usr/bin/env bash
# Package binary deltas only; neither this image nor its installer contains the complete app.
set -euo pipefail
MYAI_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MYAI_OLD="${1:?Usage: myaipdf-delta.sh old.app new.app /absolute/output.delta.dmg}"
MYAI_NEW="${2:?Missing new.app}"
MYAI_XTASK="${MYAIPDF_XTASK:-$MYAI_ROOT/target/release/xtask}"
MYAI_OUTPUT="${3:?Missing output.delta.dmg}"
case "$MYAI_OUTPUT" in /*.delta.dmg) ;; *) echo 'Expected an absolute .delta.dmg path' >&2; exit 2 ;; esac
if [ -e "$MYAI_OUTPUT" ] || [ -L "$MYAI_OUTPUT" ]; then echo 'Refusing to overwrite an existing delta package' >&2; exit 2; fi
MYAI_BASE=$(plutil -extract CFBundleShortVersionString raw "$MYAI_OLD/Contents/Info.plist")
MYAI_VERSION=$(plutil -extract CFBundleShortVersionString raw "$MYAI_NEW/Contents/Info.plist")
test "$(basename "$MYAI_OUTPUT")" = "MyAIPDF-$MYAI_VERSION-from-$MYAI_BASE.delta.dmg"
codesign --verify --strict --deep "$MYAI_OLD"
codesign --verify --strict --deep "$MYAI_NEW"
MYAI_WORK=$(mktemp -d "$MYAI_ROOT/target/myaipdf-delta.XXXXXX")
MYAI_STAGE="$MYAI_WORK/image"
MYAI_INSTALLER="$MYAI_STAGE/MyAIPDF增量安装.app"
mkdir -p "$MYAI_INSTALLER/Contents/MacOS" "$MYAI_INSTALLER/Contents/Resources/Licenses"
cp "$MYAI_NEW/Contents/MacOS/myaipdf-updater" "$MYAI_INSTALLER/Contents/MacOS/"
cp "$MYAI_ROOT/packaging/macos/MyAIPDF-Delta-Info.plist" "$MYAI_INSTALLER/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$MYAI_VERSION" "$MYAI_INSTALLER/Contents/Info.plist"
cp "$MYAI_ROOT/assets/myaipdf/MyAIPDF.icns" "$MYAI_INSTALLER/Contents/Resources/"
cp "$MYAI_ROOT/LICENSE-MIT" "$MYAI_ROOT/LICENSE-APACHE" "$MYAI_ROOT/NOTICE" "$MYAI_INSTALLER/Contents/Resources/Licenses/"
cp "$MYAI_ROOT/packaging/macos/MyAIPDF-使用说明.txt" "$MYAI_STAGE/使用说明.txt"
RAYON_NUM_THREADS=4 "$MYAI_XTASK" myaipdf-delta "$MYAI_OLD" "$MYAI_NEW" "$MYAI_INSTALLER/Contents/Resources/delta"
codesign --force --sign - --timestamp=none "$MYAI_INSTALLER/Contents/MacOS/myaipdf-updater"
codesign --force --sign - --timestamp=none "$MYAI_INSTALLER"
codesign --verify --deep --strict "$MYAI_INSTALLER"
hdiutil create -srcfolder "$MYAI_STAGE" -fs APFS -volname "MyAIPDF $MYAI_VERSION 增量更新" -format UDZO -imagekey zlib-level=9 "$MYAI_WORK/update.dmg"
codesign --force --sign - --timestamp=none "$MYAI_WORK/update.dmg"
hdiutil verify "$MYAI_WORK/update.dmg"
codesign --verify --strict "$MYAI_WORK/update.dmg"
mv -n "$MYAI_WORK/update.dmg" "$MYAI_OUTPUT"
shasum -a 256 "$MYAI_OUTPUT"
echo "Delta installer staging: $MYAI_INSTALLER"
echo "Delta package: $MYAI_OUTPUT"
